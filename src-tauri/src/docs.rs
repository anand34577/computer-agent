//! Documents attached to chats. Each is either sent to the model in full ("direct") or split
//! into chunks, embedded with the chosen embedding model and searched per question ("embed").

use crate::db::{new_id, now, Db};
use crate::models::{self, ModelRow};
use crate::settings;
use crate::state::Shared;
use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::io::Read;

pub const MAX_DOC_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_DOC_CHARS: usize = 2_000_000;
const CHUNK: usize = 1200;
const OVERLAP: usize = 200;

/// Extensions read as plain text.
const TEXT_EXT: &[&str] = &[
    "txt", "md", "markdown", "rst", "csv", "tsv", "json", "jsonl", "yaml", "yml", "toml", "ini", "cfg", "conf", "log", "xml", "html", "htm",
    "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "java", "kt", "c", "h", "cpp", "hpp", "cc", "cs", "go", "rb", "php", "swift", "m", "scala",
    "sh", "bash", "ps1", "bat", "sql", "r", "lua", "dart", "vue", "svelte", "css", "scss", "less", "tex", "srt", "vtt", "env", "gradle", "dockerfile",
];

pub fn supported(name: &str) -> bool {
    let ext = ext_of(name);
    ext == "pdf" || ext == "docx" || ext == "pptx" || ext == "xlsx" || TEXT_EXT.contains(&ext.as_str()) || !name.contains('.')
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default()
}

/// Pulls readable text out of a document.
pub fn extract_text(name: &str, bytes: &[u8]) -> Result<String> {
    if bytes.len() > MAX_DOC_BYTES {
        bail!("{name} is larger than 25 MB");
    }
    let ext = ext_of(name);
    let text = match ext.as_str() {
        "pdf" => {
            let b = bytes.to_vec();
            // The PDF parser can panic on damaged files; keep that from taking the app down.
            let r = std::panic::catch_unwind(move || pdf_extract::extract_text_from_mem(&b)).map_err(|_| anyhow!("{name} could not be read as a PDF"))?;
            let t = r.map_err(|e| anyhow!("{name} could not be read as a PDF: {e}"))?;
            if t.trim().is_empty() {
                bail!("{name} has no text layer (it may be a scanned image). Attach it as an image instead.");
            }
            t
        }
        "docx" => office_text(bytes, |n| n == "word/document.xml")?,
        "pptx" => office_text(bytes, |n| n.starts_with("ppt/slides/slide") && n.ends_with(".xml"))?,
        "xlsx" => office_text(bytes, |n| n == "xl/sharedStrings.xml" || (n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")))?,
        "html" | "htm" => strip_tags(&String::from_utf8_lossy(bytes)),
        _ => {
            if bytes.iter().take(8192).any(|b| *b == 0) {
                bail!("{name} looks like a binary file, not text");
            }
            String::from_utf8_lossy(bytes).replace("\r\n", "\n")
        }
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        bail!("{name} contains no readable text");
    }
    Ok(text.chars().take(MAX_DOC_CHARS).collect())
}

fn office_text(bytes: &[u8], want: impl Fn(&str) -> bool) -> Result<String> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| anyhow!("The file is not a valid Office document"))?;
    let mut names: Vec<String> = z.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).filter(|n| want(n)).collect();
    names.sort_by_key(|n| (n.len(), n.clone()));
    let mut out = String::new();
    for n in names {
        let mut xml = String::new();
        z.by_name(&n)?.take(50 * 1024 * 1024).read_to_string(&mut xml)?;
        let xml = xml.replace("</w:p>", "\n").replace("</a:p>", "\n").replace("<w:tab/>", "\t").replace("</si>", "\n").replace("</row>", "\n").replace("</c>", "\t");
        out.push_str(&strip_tags(&xml));
        out.push('\n');
    }
    Ok(out)
}

fn strip_tags(s: &str) -> String {
    let no_script = regex::Regex::new(r"(?is)<(script|style)[^>]*>.*?</(script|style)>").map(|r| r.replace_all(s, " ").to_string()).unwrap_or_else(|_| s.to_string());
    let mut out = String::with_capacity(no_script.len());
    let mut in_tag = false;
    for c in no_script.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ");
    let blank = regex::Regex::new(r"\n[ \t]*\n[ \t\n]*").unwrap();
    blank.replace_all(&out, "\n\n").to_string()
}

/// Splits text into overlapping chunks that break at paragraph or sentence ends where possible.
pub fn chunk(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = vec![];
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + CHUNK).min(chars.len());
        if end < chars.len() {
            let window: String = chars[start..end].iter().collect();
            let cut = window.rfind("\n\n").or_else(|| window.rfind(". ")).or_else(|| window.rfind('\n')).filter(|i| *i > CHUNK / 2);
            if let Some(i) = cut {
                end = start + window[..i].chars().count() + 1;
            }
        }
        let piece: String = chars[start..end].iter().collect::<String>().trim().to_string();
        if !piece.is_empty() {
            out.push(piece);
        }
        if end >= chars.len() {
            break;
        }
        start = end.saturating_sub(OVERLAP).max(start + 1);
    }
    out
}

#[derive(Clone, Copy, PartialEq)]
pub enum Purpose {
    Document,
    Query,
}

/// Task prefixes some embedding models are trained with.
fn prefixed(model: &str, p: Purpose, t: &str) -> String {
    let m = model.to_lowercase();
    if m.contains("nomic-embed") {
        return format!("{}{t}", if p == Purpose::Query { "search_query: " } else { "search_document: " });
    }
    if m.contains("embeddinggemma") {
        return if p == Purpose::Query { format!("task: search result | query: {t}") } else { format!("title: none | text: {t}") };
    }
    if m.contains("qwen3-embedding") && p == Purpose::Query {
        return format!("Instruct: Given a question, retrieve passages that answer it\nQuery: {t}");
    }
    t.to_string()
}

/// Embeds texts with any OpenAI-compatible embedding model, or the built-in engine.
pub async fn embed_with(db: &Db, http: &reqwest::Client, m: &ModelRow, texts: &[String], local_only: bool) -> Result<Vec<Vec<f32>>> {
    embed_for(db, http, m, texts, local_only, Purpose::Document).await
}

pub async fn embed_for(db: &Db, http: &reqwest::Client, m: &ModelRow, texts: &[String], local_only: bool, purpose: Purpose) -> Result<Vec<Vec<f32>>> {
    let p = models::get_provider(db, &m.provider_id)?;
    crate::llm::check_local_only(&p, local_only)?;
    if p.kind == "anthropic" {
        bail!("Anthropic has no embedding models; choose a local embedding model");
    }
    let base = if p.kind == "llamacpp" {
        let l = crate::llamacpp::global().ok_or_else(|| anyhow!("The built-in engine is not ready"))?;
        format!("http://127.0.0.1:{}/v1", l.ensure_embed(&m.name).await?)
    } else {
        p.openai_base()
    };
    let key = crate::secrets::get(&p.id);
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
    for batch in texts.chunks(16) {
        let input: Vec<String> = batch.iter().map(|t| prefixed(&m.name, purpose, t)).collect();
        let mut rb = http.post(format!("{base}/embeddings")).json(&json!({"model": m.name, "input": input})).timeout(std::time::Duration::from_secs(300));
        if let Some(k) = key.clone().filter(|k| !k.is_empty()) {
            rb = rb.bearer_auth(k);
        }
        let resp = rb.send().await.map_err(|e| anyhow!("Could not reach the embedding model: {e}"))?;
        if !resp.status().is_success() {
            let st = resp.status();
            let body: String = resp.text().await.unwrap_or_default().chars().take(400).collect();
            bail!("The embedding model returned {st}: {body}");
        }
        let v: Value = resp.json().await?;
        let mut rows: Vec<(u64, Vec<f32>)> = v["data"]
            .as_array()
            .ok_or_else(|| anyhow!("The server did not return embeddings"))?
            .iter()
            .map(|d| (d["index"].as_u64().unwrap_or(0), d["embedding"].as_array().map(|a| a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect()).unwrap_or_default()))
            .collect();
        rows.sort_by_key(|r| r.0);
        if rows.len() != batch.len() {
            bail!("The embedding model returned {} vectors for {} texts", rows.len(), batch.len());
        }
        out.extend(rows.into_iter().map(|(_, v)| normalise(v)));
    }
    Ok(out)
}

fn normalise(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn embed_model(sh: &Shared) -> Result<ModelRow> {
    let s = settings::load(&sh.db);
    let id = s.embed_model.ok_or_else(|| anyhow!("No embedding model is set. Choose one in Models → Built-in engine (download one) or mark a model as Embedding."))?;
    models::get_model(&sh.db, &id)
}

/// Stores a document for a chat. In "embed" mode its chunks are embedded now.
pub async fn add(sh: &Shared, conv_id: &str, name: &str, bytes: &[u8], mode: &str) -> Result<Value> {
    if !["embed", "direct"].contains(&mode) {
        bail!("Choose embed or direct");
    }
    let text = extract_text(name, bytes)?;
    let id = new_id();
    let mut embed_label = String::new();
    let mut n_chunks = 0usize;
    if mode == "embed" {
        let m = embed_model(sh)?;
        let s = settings::load(&sh.db);
        let pieces = chunk(&text);
        let vecs = embed_for(&sh.db, &sh.http, &m, &pieces, s.local_only, Purpose::Document).await?;
        let conn = sh.db.conn.lock();
        for (i, (t, v)) in pieces.iter().zip(vecs.iter()).enumerate() {
            conn.execute("INSERT INTO chunks(doc_id,idx,text,vec) VALUES(?1,?2,?3,?4)", params![id, i as i64, t, to_blob(v)])?;
        }
        n_chunks = pieces.len();
        embed_label = m.label;
    }
    let chars = text.chars().count();
    sh.db.conn.lock().execute(
        "INSERT INTO documents(id,conversation_id,name,mode,text,chars,embed_model,created) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, conv_id, name, mode, text, chars as i64, embed_label, now()],
    )?;
    crate::audit::log(&sh.db, "doc.add", name, json!({"conversation": conv_id, "mode": mode, "chars": chars, "chunks": n_chunks, "embed_model": embed_label}));
    Ok(json!({"id": id, "name": name, "mode": mode, "chars": chars, "chunks": n_chunks, "embed_model": embed_label}))
}

pub fn list(db: &Db, conv_id: &str) -> Result<Vec<Value>> {
    let conn = db.conn.lock();
    let mut st = conn.prepare("SELECT d.id,d.name,d.mode,d.chars,d.embed_model,d.created,(SELECT COUNT(*) FROM chunks c WHERE c.doc_id=d.id) FROM documents d WHERE d.conversation_id=?1 ORDER BY d.created")?;
    let rows = st
        .query_map([conv_id], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?, "mode": r.get::<_, String>(2)?, "chars": r.get::<_, i64>(3)?, "embed_model": r.get::<_, String>(4)?, "created": r.get::<_, i64>(5)?, "chunks": r.get::<_, i64>(6)?}))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn remove(db: &Db, id: &str) -> Result<()> {
    let conn = db.conn.lock();
    conn.execute("DELETE FROM chunks WHERE doc_id=?1", [id])?;
    conn.execute("DELETE FROM documents WHERE id=?1", [id])?;
    Ok(())
}

pub fn remove_for_conversation(db: &Db, conv_id: &str) {
    let conn = db.conn.lock();
    let _ = conn.execute("DELETE FROM chunks WHERE doc_id IN (SELECT id FROM documents WHERE conversation_id=?1)", [conv_id]);
    let _ = conn.execute("DELETE FROM documents WHERE conversation_id=?1", [conv_id]);
}

/// Full text of direct documents by id: (name, text).
pub fn texts(db: &Db, ids: &[String]) -> Vec<(String, String)> {
    let conn = db.conn.lock();
    ids.iter().filter_map(|id| conn.query_row("SELECT name,text FROM documents WHERE id=?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).ok()).collect()
}

pub fn has_embedded(db: &Db, conv_id: &str) -> bool {
    db.conn.lock().query_row("SELECT COUNT(*) FROM documents WHERE conversation_id=?1 AND mode='embed'", [conv_id], |r| r.get::<_, i64>(0)).unwrap_or(0) > 0
}

/// Copies a chat's documents (and their chunks) to another chat, for forks and branches.
pub fn copy_to(db: &Db, from: &str, to: &str) {
    let conn = db.conn.lock();
    let docs: Vec<String> = conn
        .prepare("SELECT id FROM documents WHERE conversation_id=?1")
        .and_then(|mut st| st.query_map([from], |r| r.get(0)).map(|r| r.filter_map(|x| x.ok()).collect()))
        .unwrap_or_default();
    for old in docs {
        let new = new_id();
        let _ = conn.execute("INSERT INTO documents(id,conversation_id,name,mode,text,chars,embed_model,created) SELECT ?1,?2,name,mode,text,chars,embed_model,created FROM documents WHERE id=?3", params![new, to, old]);
        let _ = conn.execute("INSERT INTO chunks(doc_id,idx,text,vec) SELECT ?1,idx,text,vec FROM chunks WHERE doc_id=?2", params![new, old]);
    }
}

/// The chunks of a chat's embedded documents that best match `query`: (document, text, score).
pub async fn search(sh: &Shared, conv_id: &str, query: &str, k: usize) -> Result<Vec<(String, String, f32)>> {
    let m = embed_model(sh)?;
    let s = settings::load(&sh.db);
    let q = embed_for(&sh.db, &sh.http, &m, &[query.to_string()], s.local_only, Purpose::Query).await?.pop().unwrap_or_default();
    let rows: Vec<(String, String, Vec<u8>)> = {
        let conn = sh.db.conn.lock();
        let mut st = conn.prepare("SELECT d.name,c.text,c.vec FROM chunks c JOIN documents d ON d.id=c.doc_id WHERE d.conversation_id=?1 AND d.mode='embed'")?;
        let v = st.query_map([conv_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.filter_map(|r| r.ok()).collect();
        v
    };
    let mut scored: Vec<(String, String, f32)> = rows
        .into_iter()
        .filter_map(|(n, t, b)| {
            let v = from_blob(&b);
            (v.len() == q.len()).then(|| (n, t, dot(&q, &v)))
        })
        .collect();
    if scored.is_empty() && !q.is_empty() && has_embedded(&sh.db, conv_id) {
        bail!("These documents were embedded with a different model. Remove and attach them again.");
    }
    scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(k);
    Ok(scored)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_overlap_and_cover_text() {
        let para = "This is a sentence about something. ".repeat(20);
        let text = format!("{para}\n\n{para}\n\n{para}");
        let c = chunk(&text);
        assert!(c.len() >= 2);
        assert!(c.iter().all(|x| x.chars().count() <= CHUNK));
        assert!(chunk("short").len() == 1);
        assert!(chunk("").is_empty());
    }

    #[test]
    fn extracts_text_and_office() {
        assert_eq!(extract_text("a.md", b"# Hi\r\nthere").unwrap(), "# Hi\nthere");
        assert!(extract_text("a.bin", &[0, 1, 2, 3]).is_err());
        assert_eq!(extract_text("p.html", b"<html><style>x{}</style><p>Hello &amp; bye</p></html>").unwrap(), "Hello & bye");
        // A minimal DOCX
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            z.start_file::<_, ()>("word/document.xml", zip::write::FileOptions::default()).unwrap();
            std::io::Write::write_all(&mut z, b"<w:document><w:body><w:p><w:r><w:t>First</w:t></w:r></w:p><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:body></w:document>").unwrap();
            z.finish().unwrap();
        }
        assert_eq!(extract_text("d.docx", buf.get_ref()).unwrap(), "First\nSecond");
        assert!(supported("x.pdf") && supported("y.rs") && !supported("z.exe"));
    }

    #[test]
    fn vectors_round_trip_and_rank() {
        let v = normalise(vec![3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6);
        assert_eq!(from_blob(&to_blob(&v)), v);
        assert!(dot(&v, &v) > 0.999);
        assert_eq!(prefixed("nomic-embed-text-v1.5", Purpose::Query, "x"), "search_query: x");
    }
}

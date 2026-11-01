//! Model hub: providers, models, honest capability profiles, the self-test,
//! local runtime detection, hardware check and the Ollama download manager.

use crate::db::{new_id, now, Db};
use crate::llm::{self, Msg, Part, ProviderCfg, Request, ToolDef};
use anyhow::{anyhow, Result};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Profile {
    pub vision: bool,
    pub tools: bool,
    pub reasoning: bool,
    pub audio: bool,
    /// Embedding model: turns text into vectors for document search; not a chat model.
    pub embedding: bool,
    pub context: u32,
    pub tokens_per_sec: f32,
    pub latency_ms: u32,
    pub local: bool,
    pub tested: bool,
    pub tested_at: i64,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Overrides {
    pub vision: Option<bool>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
    pub audio: Option<bool>,
    pub embedding: Option<bool>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelRow {
    pub id: String,
    pub provider_id: String,
    pub provider_name: String,
    pub provider_kind: String,
    pub name: String,
    pub label: String,
    pub profile: Profile,
    pub overrides: Overrides,
    pub effective: Profile,
    pub price_in: f64,
    pub price_out: f64,
    pub local: bool,
}

impl ModelRow {
    pub fn cost(&self, input: u64, output: u64) -> f64 {
        (input as f64 * self.price_in + output as f64 * self.price_out) / 1_000_000.0
    }
}

fn effective(p: &Profile, o: &Overrides) -> Profile {
    let mut e = p.clone();
    if let Some(v) = o.vision {
        e.vision = v;
    }
    if let Some(v) = o.tools {
        e.tools = v;
    }
    if let Some(v) = o.reasoning {
        e.reasoning = v;
    }
    if let Some(v) = o.audio {
        e.audio = v;
    }
    if let Some(v) = o.embedding {
        e.embedding = v;
    }
    if e.embedding {
        e.tools = false;
        e.vision = false;
        e.reasoning = false;
    }
    e
}

/// Names that mark an embedding model.
pub fn looks_like_embedding(name: &str) -> bool {
    let n = name.to_lowercase();
    ["embed", "bge-", "bge_", "e5-", "gte-", "minilm", "mxbai", "snowflake-arctic", "jina-emb", "-retriev", "colbert", "rerank"].iter().any(|k| n.contains(k))
}

/// A first guess from the model name, replaced by the self-test.
pub fn guess_profile(name: &str, local: bool) -> Profile {
    let n = name.to_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| n.contains(k));
    Profile {
        vision: has(&[
            "vision", "-vl", "vl-", "vl:", "llava", "gpt-4o", "gpt-4.1", "gpt-5", "claude", "gemini", "gemma3", "gemma-3",
            "pixtral", "moondream", "minicpm-v", "omni", "qwen2.5vl", "qwen3-vl", "llama4", "mistral-small3", "lfm2-vl",
        ]),
        tools: !has(&["embed", "moondream", "llava", "whisper", "tts"]),
        reasoning: has(&["o1", "o3", "o4", "r1", "thinking", "qwq", "reason", "gpt-5", "opus", "sonnet-4", "magistral", "qwen3", "gpt-oss", "glm-4.5", "glm-4.6", "deepseek-v3.1", "deepseek-v3.2", "hermes-4", "phi-4-reasoning", "exaone-deep", "seed-oss", "kimi-k2-thinking", "gemini-2.5", "gemini-3"]),
        audio: has(&["omni", "audio", "lfm2-audio", "realtime", "voxtral"]),
        embedding: looks_like_embedding(&n),
        local,
        ..Default::default()
    }
}

// ---------------- Providers ----------------

pub fn list_providers(db: &Db) -> Result<Vec<Value>> {
    let conn = db.conn.lock();
    let mut st = conn.prepare("SELECT id,kind,name,base_url FROM providers ORDER BY created")?;
    let rows = st
        .query_map([], |r| {
            let p = ProviderCfg { id: r.get(0)?, kind: r.get(1)?, name: r.get(2)?, base_url: r.get(3)? };
            Ok(p)
        })?
        .filter_map(|r| r.ok())
        .map(|p| {
            let has_key = crate::secrets::get(&p.id).map(|k| !k.is_empty()).unwrap_or(false);
            json!({"id": p.id, "kind": p.kind, "name": p.name, "base_url": p.base_url, "local": p.is_local(), "has_key": has_key})
        })
        .collect();
    Ok(rows)
}

pub fn get_provider(db: &Db, id: &str) -> Result<ProviderCfg> {
    let conn = db.conn.lock();
    conn.query_row("SELECT id,kind,name,base_url FROM providers WHERE id=?1", [id], |r| {
        Ok(ProviderCfg { id: r.get(0)?, kind: r.get(1)?, name: r.get(2)?, base_url: r.get(3)? })
    })
    .map_err(|_| anyhow!("Provider not found"))
}

pub fn add_provider(db: &Db, kind: &str, name: &str, base_url: &str, key: Option<String>) -> Result<String> {
    let id = new_id();
    {
        let conn = db.conn.lock();
        conn.execute(
            "INSERT INTO providers(id,kind,name,base_url,created) VALUES(?1,?2,?3,?4,?5)",
            params![id, kind, name, base_url.trim().trim_end_matches('/'), now()],
        )?;
    }
    if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        crate::secrets::set(&id, k.trim())?;
    }
    Ok(id)
}

pub fn update_provider(db: &Db, id: &str, name: &str, base_url: &str, key: Option<String>) -> Result<()> {
    db.conn.lock().execute(
        "UPDATE providers SET name=?2, base_url=?3 WHERE id=?1",
        params![id, name, base_url.trim().trim_end_matches('/')],
    )?;
    if let Some(k) = key.filter(|k| !k.trim().is_empty()) {
        crate::secrets::set(id, k.trim())?;
    }
    Ok(())
}

pub fn delete_provider(db: &Db, id: &str) -> Result<()> {
    let conn = db.conn.lock();
    conn.execute("DELETE FROM models WHERE provider_id=?1", [id])?;
    conn.execute("DELETE FROM providers WHERE id=?1", [id])?;
    crate::secrets::delete(id);
    Ok(())
}

/// Points settings away from models that no longer exist: the default moves to the first
/// remaining model, the other roles fall back to "not set". Returns true if anything changed.
pub fn heal_model_refs(db: &Db, s: &mut crate::settings::AppSettings) -> bool {
    let Ok(all) = list_models(db) else { return false };
    let exists = |id: &Option<String>| id.as_ref().map(|d| all.iter().any(|m| &m.id == d)).unwrap_or(true);
    let mut changed = false;
    let chat_ok = |id: &Option<String>| id.as_ref().map(|d| all.iter().any(|m| &m.id == d && !m.effective.embedding)).unwrap_or(true);
    if !chat_ok(&s.default_model) || (s.default_model.is_none() && all.iter().any(|m| !m.effective.embedding)) {
        s.default_model = all.iter().find(|m| !m.effective.embedding).map(|m| m.id.clone());
        changed = true;
    }
    let before = s.subagents.models.len();
    s.subagents.models.retain(|id| all.iter().any(|m| &m.id == id));
    changed |= before != s.subagents.models.len();
    for slot in [&mut s.fallback_model, &mut s.roles.planner, &mut s.roles.doer, &mut s.roles.eyes, &mut s.embed_model] {
        if !exists(slot) {
            *slot = None;
            changed = true;
        }
    }
    changed
}

// ---------------- Models ----------------

pub fn list_models(db: &Db) -> Result<Vec<ModelRow>> {
    let conn = db.conn.lock();
    let mut st = conn.prepare(
        "SELECT m.id,m.provider_id,p.name,p.kind,m.name,m.label,m.profile,m.overrides,m.price_in,m.price_out,p.base_url
         FROM models m JOIN providers p ON p.id=m.provider_id ORDER BY m.created",
    )?;
    let rows = st
        .query_map([], |r| {
            let profile: Profile = serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default();
            let overrides: Overrides = serde_json::from_str(&r.get::<_, String>(7)?).unwrap_or_default();
            let p = ProviderCfg { id: r.get(1)?, kind: r.get(3)?, name: r.get(2)?, base_url: r.get(10)? };
            let local = p.is_local();
            let mut eff = effective(&profile, &overrides);
            eff.local = local;
            Ok(ModelRow {
                id: r.get(0)?,
                provider_id: p.id,
                provider_name: p.name,
                provider_kind: p.kind,
                name: r.get(4)?,
                label: r.get(5)?,
                profile,
                overrides,
                effective: eff,
                price_in: r.get(8)?,
                price_out: r.get(9)?,
                local,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

pub fn get_model(db: &Db, id: &str) -> Result<ModelRow> {
    list_models(db)?.into_iter().find(|m| m.id == id).ok_or_else(|| anyhow!("Model not found. Pick a model in Models."))
}

pub fn add_model(db: &Db, provider_id: &str, name: &str) -> Result<String> {
    let p = get_provider(db, provider_id)?;
    let conn = db.conn.lock();
    let existing: Option<String> = conn
        .query_row("SELECT id FROM models WHERE provider_id=?1 AND name=?2", params![provider_id, name], |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    let id = new_id();
    let profile = guess_profile(name, p.is_local());
    let (pin, pout) = known_price(name);
    conn.execute(
        "INSERT INTO models(id,provider_id,name,label,profile,price_in,price_out,created) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![id, provider_id, name, name, serde_json::to_string(&profile)?, pin, pout, now()],
    )?;
    Ok(id)
}

/// Rough list prices (USD per million tokens) so cost tracking starts sensible; user-editable.
fn known_price(name: &str) -> (f64, f64) {
    let n = name.to_lowercase();
    let table: &[(&str, f64, f64)] = &[
        ("opus", 15.0, 75.0),
        ("sonnet", 3.0, 15.0),
        ("haiku", 1.0, 5.0),
        ("gpt-4o-mini", 0.15, 0.6),
        ("gpt-4o", 2.5, 10.0),
        ("gpt-4.1-mini", 0.4, 1.6),
        ("gpt-4.1", 2.0, 8.0),
        ("gpt-5-mini", 0.25, 2.0),
        ("gpt-5", 1.25, 10.0),
        ("o3", 2.0, 8.0),
        ("o4-mini", 1.1, 4.4),
    ];
    for (k, i, o) in table {
        if n.contains(k) {
            return (*i, *o);
        }
    }
    (0.0, 0.0)
}

pub fn update_model(db: &Db, id: &str, label: &str, overrides: &Overrides, price_in: f64, price_out: f64) -> Result<()> {
    db.conn.lock().execute(
        "UPDATE models SET label=?2, overrides=?3, price_in=?4, price_out=?5 WHERE id=?1",
        params![id, label, serde_json::to_string(overrides)?, price_in, price_out],
    )?;
    Ok(())
}

pub fn delete_model(db: &Db, id: &str) -> Result<()> {
    db.conn.lock().execute("DELETE FROM models WHERE id=?1", [id])?;
    Ok(())
}

fn save_profile(db: &Db, id: &str, p: &Profile) -> Result<()> {
    db.conn.lock().execute("UPDATE models SET profile=?2 WHERE id=?1", params![id, serde_json::to_string(p)?])?;
    Ok(())
}

pub fn record_usage(db: &Db, model: &ModelRow, input: u64, output: u64) -> f64 {
    let cost = model.cost(input, output);
    let _ = db.conn.lock().execute(
        "INSERT INTO usage(day,model_id,input,output,cost) VALUES(?1,?2,?3,?4,?5)",
        params![crate::db::today(), model.id, input as i64, output as i64, cost],
    );
    cost
}

/// One row per reply for the Stats screen: speed, latency and tokens.
#[allow(clippy::too_many_arguments)]
pub fn record_reply(db: &Db, model: &ModelRow, provider: &str, kind: &str, input: u64, output: u64, ttft_ms: u64, gen_ms: u64, cost: f64, outcome: &str) -> f64 {
    let tps = if gen_ms > 0 && output > 0 { output as f64 * 1000.0 / gen_ms as f64 } else { 0.0 };
    let _ = db.conn.lock().execute(
        "INSERT INTO replies(ts,model_id,model,provider,kind,input,output,ttft_ms,gen_ms,tps,cost,outcome) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
        params![now(), model.id, model.label, provider, kind, input as i64, output as i64, ttft_ms as i64, gen_ms as i64, tps, cost, outcome],
    );
    tps
}

pub fn spent_today(db: &Db) -> f64 {
    db.conn
        .lock()
        .query_row("SELECT COALESCE(SUM(cost),0) FROM usage WHERE day=?1", [crate::db::today()], |r| r.get(0))
        .unwrap_or(0.0)
}

// ---------------- Self-test ----------------

fn solid_png(rgb: [u8; 3]) -> String {
    let img = image::RgbImage::from_pixel(96, 96, image::Rgb(rgb));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img).write_to(&mut bytes, image::ImageFormat::Png).ok();
    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
}

/// Tests structured tool output, image reading and speed. Takes seconds to a minute,
/// since local models may need to load first.
pub async fn self_test(db: &Db, http: &reqwest::Client, model_id: &str, local_only: bool) -> Result<Profile> {
    let m = get_model(db, model_id)?;
    let p = get_provider(db, &m.provider_id)?;
    let key = crate::secrets::get(&p.id);
    let mut prof = guess_profile(&m.name, p.is_local());
    prof.notes.clear();
    let timeout = Duration::from_secs(120);
    if m.effective.embedding || (prof.embedding && m.overrides.embedding != Some(false)) {
        // Embedding models cannot chat; check that they return vectors instead.
        let t0 = Instant::now();
        let v = tokio::time::timeout(timeout, crate::docs::embed_with(db, http, &m, &["The sea is calm today.".to_string()], local_only))
            .await
            .map_err(|_| anyhow!("The model did not answer within 2 minutes"))??;
        let dims = v.first().map(|x| x.len()).unwrap_or(0);
        if dims == 0 {
            anyhow::bail!("The model returned no embedding");
        }
        prof = Profile { embedding: true, local: p.is_local(), tested: true, tested_at: now(), latency_ms: t0.elapsed().as_millis() as u32, ..Default::default() };
        prof.notes.push(format!("Embedding model: returns {dims}-dimension vectors."));
        save_profile(db, model_id, &prof)?;
        return Ok(prof);
    }
    if p.kind == "llamacpp" {
        if let Some(l) = crate::llamacpp::global() {
            l.ensure(&m.name).await?;
        }
    }

    // 1. Basic reply and speed
    let req = Request {
        reasoning: None,
        model: m.name.clone(),
        messages: vec![Msg::text("user", "Write one short sentence about the sea. Then stop.")],
        tools: vec![],
        max_tokens: 400,
        temperature: Some(0.0),
    };
    let t0 = Instant::now();
    let r = tokio::time::timeout(timeout, llm::complete(http, &p, key.clone(), &req, local_only))
        .await
        .map_err(|_| anyhow!("The model did not answer within 2 minutes"))??;
    let secs = t0.elapsed().as_secs_f32().max(0.01);
    prof.latency_ms = (secs * 1000.0) as u32;
    let out_tokens = if r.output_tokens > 0 { r.output_tokens as f32 } else { (r.text.len() as f32 / 4.0).max(1.0) };
    prof.tokens_per_sec = (out_tokens / secs * 10.0).round() / 10.0;
    prof.reasoning = prof.reasoning || !r.reasoning.is_empty();
    prof.notes.push(format!("Replied in {:.1}s", secs));

    // 2. Structured tool output
    let req = Request {
        reasoning: None,
        model: m.name.clone(),
        messages: vec![Msg::text("user", "Call the record_number tool with the number 42. Do not reply with text.")],
        tools: vec![ToolDef {
            name: "record_number".into(),
            description: "Records a number".into(),
            parameters: json!({"type":"object","properties":{"n":{"type":"integer"}},"required":["n"]}),
        }],
        max_tokens: 400,
        temperature: Some(0.0),
    };
    match tokio::time::timeout(timeout, llm::complete(http, &p, key.clone(), &req, local_only)).await {
        Ok(Ok(r)) => {
            let ok = r.tool_calls.iter().any(|t| t.name == "record_number" && t.arguments["n"].as_i64() == Some(42));
            prof.tools = ok;
            prof.notes.push(if ok { "Tool calling works".into() } else { "No valid tool call; will use JSON action mode".into() });
        }
        Ok(Err(e)) => {
            prof.tools = false;
            prof.notes.push(format!("Tool test failed: {}", short(&e.to_string())));
        }
        Err(_) => prof.notes.push("Tool test timed out".into()),
    }

    // 3. Image reading
    let colors = [("red", [220u8, 30, 30]), ("green", [30, 180, 60]), ("blue", [30, 70, 220])];
    let pick = colors[(now() as usize / 7) % 3];
    let req = Request {
        reasoning: None,
        model: m.name.clone(),
        messages: vec![Msg {
            role: "user".into(),
            parts: vec![
                Part::Image { media_type: "image/png".into(), data: solid_png(pick.1) },
                Part::Text { text: "What single colour fills this image? Answer with one word: red, green or blue.".into() },
            ],
            ..Default::default()
        }],
        tools: vec![],
        max_tokens: 300,
        temperature: Some(0.0),
    };
    match tokio::time::timeout(timeout, llm::complete(http, &p, key.clone(), &req, local_only)).await {
        Ok(Ok(r)) => {
            let ok = r.text.to_lowercase().contains(pick.0);
            prof.vision = ok;
            prof.notes.push(if ok { "Reads images correctly".into() } else { "Could not read the test image".into() });
        }
        Ok(Err(_)) => {
            prof.vision = false;
            prof.notes.push("Does not accept images".into());
        }
        Err(_) => prof.notes.push("Image test timed out".into()),
    }

    if p.kind == "ollama" {
        if let Ok(ctx) = ollama_context(http, &p, &m.name).await {
            prof.context = ctx;
        }
    }
    prof.tested = true;
    prof.tested_at = now();
    save_profile(db, model_id, &prof)?;
    Ok(prof)
}

fn short(s: &str) -> String {
    s.chars().take(160).collect()
}

async fn ollama_context(http: &reqwest::Client, p: &ProviderCfg, name: &str) -> Result<u32> {
    let v: Value = http
        .post(format!("{}/api/show", p.base_url))
        .json(&json!({"model": name}))
        .send()
        .await?
        .json()
        .await?;
    if let Some(info) = v["model_info"].as_object() {
        for (k, val) in info {
            if k.ends_with(".context_length") {
                return Ok(val.as_u64().unwrap_or(0) as u32);
            }
        }
    }
    Ok(0)
}

// ---------------- Local runtime detection ----------------

pub async fn detect_local(http: &reqwest::Client) -> Vec<Value> {
    // OLLAMA_HOST may point somewhere other than the default port.
    let mut ollama = vec!["http://127.0.0.1:11434".to_string(), "http://localhost:11434".to_string()];
    if let Ok(h) = std::env::var("OLLAMA_HOST") {
        let h = h.trim().trim_end_matches('/');
        if !h.is_empty() {
            let h = if h.contains("://") { h.to_string() } else { format!("http://{h}") };
            let h = h.replace("://0.0.0.0", "://127.0.0.1");
            ollama.insert(0, if h.rsplit(':').next().map(|p| p.chars().all(|c| c.is_ascii_digit())).unwrap_or(false) { h } else { format!("{h}:11434") });
        }
    }
    let candidates: Vec<(&str, &str, Vec<String>, &str)> = vec![
        ("Ollama", "ollama", ollama, "/api/tags"),
        ("LM Studio", "openai", vec!["http://127.0.0.1:1234/v1".into(), "http://localhost:1234/v1".into()], "/models"),
        ("llama.cpp", "openai", vec!["http://127.0.0.1:8080/v1".into()], "/models"),
        ("Jan", "openai", vec!["http://127.0.0.1:1337/v1".into()], "/models"),
        ("vLLM", "openai", vec!["http://127.0.0.1:8000/v1".into()], "/models"),
        ("LocalAI", "openai", vec!["http://127.0.0.1:8081/v1".into()], "/models"),
        ("KoboldCpp", "openai", vec!["http://127.0.0.1:5001/v1".into()], "/models"),
        ("text-generation-webui", "openai", vec!["http://127.0.0.1:5000/v1".into()], "/models"),
        ("GPT4All", "openai", vec!["http://127.0.0.1:4891/v1".into()], "/models"),
    ];
    let probes = candidates.into_iter().map(|(name, kind, bases, probe)| async move {
        for base in &bases {
            let ok = http
                .get(format!("{base}{probe}"))
                .timeout(Duration::from_millis(1200))
                .send()
                .await
                .map(|r| r.status().is_success())
                .unwrap_or(false);
            if ok {
                return json!({"name": name, "kind": kind, "base_url": base, "running": true});
            }
        }
        json!({"name": name, "kind": kind, "base_url": bases[0], "running": false})
    });
    futures_util::future::join_all(probes).await
}

/// Finds the address that actually answers, so "http://host:1234" works as well as
/// "http://host:1234/v1". Returns the working base address and the models it lists.
pub async fn resolve_base(http: &reqwest::Client, kind: &str, base: &str, key: Option<String>) -> Result<(String, Vec<crate::llm::RemoteModel>)> {
    let base = base.trim().trim_end_matches('/').to_string();
    let mut tries = vec![base.clone()];
    match kind {
        "ollama" => tries = vec![crate::llm::ollama_root(&base)],
        "openai" => {
            if let Some(stripped) = base.strip_suffix("/chat/completions") {
                tries = vec![stripped.to_string()];
            }
            if !tries[0].ends_with("/v1") {
                // A bare host:port almost always means the /v1 API; a longer path was typed on purpose.
                let bare = reqwest::Url::parse(&tries[0]).map(|u| u.path().trim_matches('/').is_empty()).unwrap_or(false);
                let with_v1 = format!("{}/v1", tries[0]);
                if bare {
                    tries.insert(0, with_v1);
                } else {
                    tries.push(with_v1);
                }
            }
        }
        _ => {}
    }
    let mut first_err = None;
    for b in tries {
        let p = crate::llm::ProviderCfg { id: String::new(), kind: kind.into(), name: "probe".into(), base_url: b.clone() };
        match crate::llm::list_models(http, &p, key.clone()).await {
            Ok(list) => return Ok((b, list)),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    Err(first_err.unwrap_or_else(|| anyhow!("No answer")))
}

// ---------------- Hardware check ----------------

pub fn hardware() -> Value {
    let Ok(mut sys) = sysinfo::System::new() else { return json!({"ram_gb": 0, "gpus": gpu_names()}) };
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let total = sys.total_memory();
    let avail = sys.available_memory();
    let cpu = sys.cpus().first().map(|c| c.brand().trim().to_string()).unwrap_or_default();
    let cores = sys.cpus().len();
    let gpus = gpu_names();
    let disk_free = data_disk_free();
    json!({
        "ram_gb": (total as f64 / 1e9 * 10.0).round() / 10.0,
        "ram_free_gb": (avail as f64 / 1e9 * 10.0).round() / 10.0,
        "cpu": cpu,
        "cores": cores,
        "gpus": gpus,
        "disk_free_gb": disk_free,
    })
}

fn data_disk_free() -> f64 {
    let Ok(disks) = sysinfo::Disks::new_with_refreshed_list() else { return 0.0 };
    let home = dirs::home_dir().unwrap_or_default();
    let mut best: Option<(usize, u64)> = None;
    for d in disks.iter() {
        let mp = d.mount_point();
        if home.starts_with(mp) {
            let len = mp.as_os_str().len();
            if best.map(|b| len > b.0).unwrap_or(true) {
                best = Some((len, d.available_space()));
            }
        }
    }
    best.map(|b| (b.1 as f64 / 1e9 * 10.0).round() / 10.0).unwrap_or(0.0)
}

pub fn gpu_names() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", "(Get-CimInstance Win32_VideoController).Name"])
            .creation_flags(0x0800_0000)
            .output();
        if let Ok(o) = out {
            return String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect();
        }
    }
    vec![]
}

/// Curated open-weight models. Sizes are approximate download sizes in GB.
pub fn catalog() -> Value {
    json!([
        {"name":"qwen2.5vl:3b","size_gb":3.2,"vision":true,"tools":true,"license":"Qwen Research (non-commercial)","role":"Eyes / Doer (small)","about":"Compact vision model, good at reading screens."},
        {"name":"qwen2.5vl:7b","size_gb":6.0,"vision":true,"tools":true,"license":"Apache 2.0","role":"Doer with vision","about":"Strong local vision model for operating the screen."},
        {"name":"gemma3:4b","size_gb":3.3,"vision":true,"tools":false,"license":"Gemma Terms of Use","role":"Eyes","about":"Small multimodal model, describes screens well."},
        {"name":"gemma3:12b","size_gb":8.1,"vision":true,"tools":false,"license":"Gemma Terms of Use","role":"Eyes / Chat","about":"Larger multimodal model."},
        {"name":"llama3.2-vision:11b","size_gb":7.8,"vision":true,"tools":false,"license":"Llama 3.2 Community","role":"Eyes","about":"Meta vision model."},
        {"name":"qwen3:4b","size_gb":2.6,"vision":false,"tools":true,"license":"Apache 2.0","role":"Fast Doer (text)","about":"Fast text model with tool calling and thinking."},
        {"name":"qwen3:8b","size_gb":5.2,"vision":false,"tools":true,"license":"Apache 2.0","role":"Planner (local)","about":"Good local planner with tool calling."},
        {"name":"qwen3:14b","size_gb":9.3,"vision":false,"tools":true,"license":"Apache 2.0","role":"Planner (local)","about":"Stronger planner, needs 16 GB+."},
        {"name":"llama3.2:3b","size_gb":2.0,"vision":false,"tools":true,"license":"Llama 3.2 Community","role":"Fast Doer (text)","about":"Small, quick text model."},
        {"name":"granite3.2-vision:2b","size_gb":2.4,"vision":true,"tools":false,"license":"Apache 2.0","role":"Eyes (tiny)","about":"Tiny document and screen reader."},
        {"name":"moondream:1.8b","size_gb":1.7,"vision":true,"tools":false,"license":"Apache 2.0","role":"Eyes (tiny)","about":"Very small vision model for weak machines."},
        {"name":"deepseek-r1:8b","size_gb":5.2,"vision":false,"tools":false,"license":"MIT","role":"Reasoning chat","about":"Local reasoning model."}
    ])
}

pub async fn ollama_delete(http: &reqwest::Client, base: &str, name: &str) -> Result<()> {
    let base = &crate::llm::ollama_root(base);
    let r = http.delete(format!("{base}/api/delete")).json(&json!({"model": name})).send().await?;
    if !r.status().is_success() {
        return Err(anyhow!("Delete failed: {}", r.status()));
    }
    Ok(())
}

/// Streams pull progress through `on` as (status, completed, total).
pub async fn ollama_pull(http: &reqwest::Client, base: &str, name: &str, mut on: impl FnMut(String, u64, u64)) -> Result<()> {
    let base = &crate::llm::ollama_root(base);
    use futures_util::StreamExt;
    let r = http.post(format!("{base}/api/pull")).json(&json!({"model": name, "stream": true})).send().await?;
    if !r.status().is_success() {
        return Err(anyhow!("Download failed: {}", r.status()));
    }
    let mut s = r.bytes_stream();
    let mut buf = String::new();
    while let Some(c) = s.next().await {
        buf.push_str(&String::from_utf8_lossy(&c?));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].to_string();
            buf.drain(..=pos);
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                if let Some(e) = v["error"].as_str() {
                    return Err(anyhow!("{e}"));
                }
                on(
                    v["status"].as_str().unwrap_or("").to_string(),
                    v["completed"].as_u64().unwrap_or(0),
                    v["total"].as_u64().unwrap_or(0),
                );
            }
        }
    }
    Ok(())
}

pub async fn ollama_unload(http: &reqwest::Client, base: &str, name: &str) {
    let base = &crate::llm::ollama_root(base);
    let _ = http
        .post(format!("{base}/api/generate"))
        .json(&json!({"model": name, "keep_alive": 0}))
        .timeout(Duration::from_secs(5))
        .send()
        .await;
}

#[cfg(test)]
mod tests {
    /// Needs scripts/mock_llm.py running: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn resolves_missing_v1_against_mock() {
        let http = reqwest::Client::new();
        let (base, list) = super::resolve_base(&http, "openai", "http://localhost:8765", None).await.unwrap();
        assert_eq!(base, "http://localhost:8765/v1");
        assert!(list.iter().any(|m| m.name == "mock-agent-1"));
        let (base, _) = super::resolve_base(&http, "openai", "http://127.0.0.1:8765/v1/chat/completions", None).await.unwrap();
        assert_eq!(base, "http://127.0.0.1:8765/v1");
    }
}

#[cfg(test)]
mod heal_tests {
    use super::*;

    #[test]
    fn stale_model_references_are_repaired() {
        let dir = std::env::temp_dir().join(format!("heal-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("t.db")).unwrap();
        let pid = add_provider(&db, "openai", "Local", "http://127.0.0.1:1/v1", None).unwrap();
        let mid = add_model(&db, &pid, "m1").unwrap();
        let mut s = crate::settings::AppSettings::default();
        s.default_model = Some("deleted-model".into());
        s.roles.doer = Some("deleted-model".into());
        s.roles.planner = Some(mid.clone());
        assert!(heal_model_refs(&db, &mut s));
        assert_eq!(s.default_model.as_deref(), Some(mid.as_str()));
        assert_eq!(s.roles.doer, None);
        assert_eq!(s.roles.planner.as_deref(), Some(mid.as_str()));
        assert!(crate::validate::settings(&s, &[mid.clone()]).is_empty());
        assert!(!heal_model_refs(&db, &mut s), "nothing left to repair");
    }
}

//! Append-only audit trail of everything the user and the AI do. Each row carries the hash
//! of the row before it, so a deleted or edited row breaks the chain and `verify` finds it.

use crate::db::{now, Db};
use crate::state::Shared;
use rusqlite::params;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tauri::Manager;

pub struct Entry<'a> {
    pub actor: &'a str,
    pub action: &'a str,
    pub subject: &'a str,
    pub session: &'a str,
    pub outcome: &'a str,
    pub details: Value,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn row_hash(prev: &str, ts: i64, e: &Entry, details: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("{prev}|{ts}|{}|{}|{}|{}|{}|{details}", e.actor, e.action, e.subject, e.session, e.outcome));
    hex(&h.finalize())
}

pub fn record(db: &Db, e: Entry) {
    let mut details = redact(e.details.clone()).to_string();
    if details.len() > 20_000 {
        details = json!({"truncated": details.chars().take(20_000).collect::<String>()}).to_string();
    }
    let category = e.action.split('.').next().unwrap_or(e.action).to_string();
    let subject: String = e.subject.chars().take(300).collect();
    let e = Entry { subject: &subject, ..e };
    let conn = db.conn.lock();
    let prev: String = conn.query_row("SELECT hash FROM audit ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap_or_default();
    let ts = now();
    let hash = row_hash(&prev, ts, &e, &details);
    let _ = conn.execute(
        "INSERT INTO audit(ts,category,action,actor,subject,session,outcome,details,prev_hash,hash) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![ts, category, e.action, e.actor, e.subject, e.session, e.outcome, details, prev, hash],
    );
}

/// Something the user did.
pub fn log(db: &Db, action: &str, subject: &str, details: Value) {
    record(db, Entry { actor: "user", action, subject, session: "", outcome: "ok", details });
}

/// Something the AI (chat or agent) did, tied to its chat or task.
pub fn ai(db: &Db, session: &str, action: &str, subject: &str, outcome: &str, details: Value) {
    record(db, Entry { actor: "ai", action, subject, session, outcome, details });
}

/// Something the app did on its own (startup, recovery, pruning).
pub fn system(db: &Db, action: &str, subject: &str, details: Value) {
    record(db, Entry { actor: "system", action, subject, session: "", outcome: "ok", details });
}

const SECRET_KEYS: &[&str] = &["key", "apikey", "api_key", "secret", "password", "token", "authorization", "headers", "env", "cookie"];

fn is_secret_key(k: &str) -> bool {
    let k = k.to_lowercase().replace(['-', '_'], "");
    SECRET_KEYS.iter().any(|s| {
        let s = s.replace('_', "");
        k == s || k.ends_with(&s) && k.len() <= s.len() + 6
    })
}

/// Removes secrets and shrinks large blobs (images, file bodies) before anything is stored.
pub fn redact(v: Value) -> Value {
    match v {
        Value::Object(m) => Value::Object(
            m.into_iter()
                .map(|(k, v)| {
                    if is_secret_key(&k) && !v.is_null() && v != json!("") {
                        (k, json!("[redacted]"))
                    } else {
                        (k, redact(v))
                    }
                })
                .collect(),
        ),
        Value::Array(a) => Value::Array(a.into_iter().map(redact).collect()),
        Value::String(s) if s.len() > 4000 => json!(format!("{}… [{} characters]", s.chars().take(1500).collect::<String>(), s.chars().count())),
        other => other,
    }
}

/// Commands that only read are not worth a row each.
fn is_read_only(cmd: &str) -> bool {
    [
        "get_", "list_", "read_", "detect_", "validate_", "has_", "audit_", "vm_status", "sandbox_detect", "hardware_info", "model_catalog", "platform_info",
        "agent_status", "usage_summary", "mcp_status", "plugins_dir", "data_dir", "provider_remote_models", "set_bar_size", "diagnostics_report", "plugin:",
    ]
    .iter()
    .any(|p| cmd.starts_with(p))
}

/// Records every state-changing command the interface sends, with its (redacted) arguments.
pub fn on_invoke<R: tauri::Runtime>(invoke: &tauri::ipc::Invoke<R>) {
    let cmd = invoke.message.command();
    if is_read_only(cmd) {
        return;
    }
    let args = match invoke.message.payload() {
        tauri::ipc::InvokeBody::Json(v) => v.clone(),
        tauri::ipc::InvokeBody::Raw(b) => json!({"bytes": b.len()}),
    };
    let wv = invoke.message.webview();
    if let Some(sh) = wv.try_state::<Shared>() {
        let session = ["conversationId", "taskId", "id"].iter().find_map(|k| args.get(*k).and_then(|v| v.as_str())).unwrap_or("").to_string();
        record(&sh.db, Entry { actor: "user", action: &format!("command.{cmd}"), subject: wv.label(), session: &session, outcome: "requested", details: args });
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Query {
    pub search: String,
    pub category: String,
    pub actor: String,
    pub outcome: String,
    pub session: String,
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub page: u32,
    pub page_size: u32,
}

fn where_clause(q: &Query) -> (String, Vec<rusqlite::types::Value>) {
    use rusqlite::types::Value as V;
    let mut w = vec!["1=1".to_string()];
    let mut p: Vec<V> = vec![];
    if !q.search.trim().is_empty() {
        let s = format!("%{}%", q.search.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_"));
        w.push("(action LIKE ?1 ESCAPE '\\' OR subject LIKE ?1 ESCAPE '\\' OR details LIKE ?1 ESCAPE '\\' OR session LIKE ?1 ESCAPE '\\')".into());
        p.push(V::Text(s));
    }
    for (col, val) in [("category", &q.category), ("actor", &q.actor), ("outcome", &q.outcome), ("session", &q.session)] {
        if !val.is_empty() {
            p.push(V::Text(val.clone()));
            w.push(format!("{col} = ?{}", p.len()));
        }
    }
    if let Some(f) = q.from {
        p.push(V::Integer(f));
        w.push(format!("ts >= ?{}", p.len()));
    }
    if let Some(t) = q.to {
        p.push(V::Integer(t));
        w.push(format!("ts <= ?{}", p.len()));
    }
    (w.join(" AND "), p)
}

fn row_json(r: &rusqlite::Row) -> rusqlite::Result<Value> {
    Ok(json!({
        "id": r.get::<_, i64>(0)?, "ts": r.get::<_, i64>(1)?, "category": r.get::<_, String>(2)?,
        "action": r.get::<_, String>(3)?, "actor": r.get::<_, String>(4)?, "subject": r.get::<_, String>(5)?,
        "session": r.get::<_, String>(6)?, "outcome": r.get::<_, String>(7)?,
        "details": serde_json::from_str::<Value>(&r.get::<_, String>(8)?).unwrap_or(json!({})),
    }))
}

pub fn query(db: &Db, q: &Query) -> anyhow::Result<Value> {
    let (w, p) = where_clause(q);
    let size = q.page_size.clamp(10, 500);
    let conn = db.conn.lock();
    let total: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM audit WHERE {w}"), rusqlite::params_from_iter(p.iter()), |r| r.get(0))?;
    let sql = format!("SELECT id,ts,category,action,actor,subject,session,outcome,details FROM audit WHERE {w} ORDER BY id DESC LIMIT {size} OFFSET {}", q.page as u64 * size as u64);
    let mut st = conn.prepare(&sql)?;
    let items: Vec<Value> = st.query_map(rusqlite::params_from_iter(p.iter()), row_json)?.filter_map(|r| r.ok()).collect();
    let cats: Vec<String> = conn.prepare("SELECT DISTINCT category FROM audit ORDER BY category")?.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
    Ok(json!({"items": items, "total": total, "page": q.page, "page_size": size, "categories": cats}))
}

pub fn export(db: &Db, q: &Query, format: &str) -> anyhow::Result<String> {
    let (w, p) = where_clause(q);
    let conn = db.conn.lock();
    let mut st = conn.prepare(&format!("SELECT id,ts,category,action,actor,subject,session,outcome,details FROM audit WHERE {w} ORDER BY id"))?;
    let rows: Vec<Value> = st.query_map(rusqlite::params_from_iter(p.iter()), row_json)?.filter_map(|r| r.ok()).collect();
    if format == "csv" {
        let esc = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        let mut out = String::from("id,time,category,action,actor,subject,session,outcome,details\n");
        for r in rows {
            let t = chrono::DateTime::from_timestamp_millis(r["ts"].as_i64().unwrap_or(0)).map(|d| d.with_timezone(&chrono::Local).to_rfc3339()).unwrap_or_default();
            out.push_str(&format!(
                "{},{},{},{},{},{},{},{},{}\n",
                r["id"],
                esc(&t),
                esc(r["category"].as_str().unwrap_or("")),
                esc(r["action"].as_str().unwrap_or("")),
                esc(r["actor"].as_str().unwrap_or("")),
                esc(r["subject"].as_str().unwrap_or("")),
                esc(r["session"].as_str().unwrap_or("")),
                esc(r["outcome"].as_str().unwrap_or("")),
                esc(&r["details"].to_string())
            ));
        }
        Ok(out)
    } else {
        Ok(serde_json::to_string_pretty(&rows)?)
    }
}

/// Walks the hash chain. Retention pruning removes the oldest rows, so the first remaining
/// row may point at a hash that is gone; that is expected and not reported.
pub fn verify(db: &Db) -> anyhow::Result<Value> {
    let conn = db.conn.lock();
    let mut st = conn.prepare("SELECT id,ts,actor,action,subject,session,outcome,details,prev_hash,hash FROM audit ORDER BY id")?;
    let mut rows = st.query([])?;
    let mut prev: Option<String> = None;
    let mut n = 0u64;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        let (actor, action, subject, session, outcome): (String, String, String, String, String) = (r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?);
        let details: String = r.get(7)?;
        let prev_hash: String = r.get(8)?;
        let hash: String = r.get(9)?;
        if let Some(p) = &prev {
            if &prev_hash != p {
                return Ok(json!({"ok": false, "checked": n, "broken_at": id, "reason": "A record before this one was removed or changed."}));
            }
        }
        let e = Entry { actor: &actor, action: &action, subject: &subject, session: &session, outcome: &outcome, details: Value::Null };
        if row_hash(&prev_hash, r.get(1)?, &e, &details) != hash {
            return Ok(json!({"ok": false, "checked": n, "broken_at": id, "reason": "This record was edited after it was written."}));
        }
        prev = Some(hash);
        n += 1;
    }
    Ok(json!({"ok": true, "checked": n}))
}

pub fn prune(db: &Db, keep_days: u32) {
    if keep_days == 0 {
        return;
    }
    let cutoff = now() - keep_days as i64 * 86_400_000;
    let removed = db.conn.lock().execute("DELETE FROM audit WHERE ts < ?1", params![cutoff]).unwrap_or(0);
    if removed > 0 {
        system(db, "audit.prune", "retention", json!({"removed": removed, "keep_days": keep_days}));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_secrets_and_blobs() {
        let v = redact(json!({"key": "sk-123", "apiKey": "x", "name": "OpenAI", "headers": {"A": "b"}, "nested": {"password": "p"}, "big": "a".repeat(5000), "monkey_count": 3}));
        assert_eq!(v["key"], "[redacted]");
        assert_eq!(v["apiKey"], "[redacted]");
        assert_eq!(v["headers"], "[redacted]");
        assert_eq!(v["nested"]["password"], "[redacted]");
        assert_eq!(v["name"], "OpenAI");
        assert_eq!(v["monkey_count"], 3);
        assert!(v["big"].as_str().unwrap().len() < 2000);
    }

    #[test]
    fn chain_detects_tampering() {
        let dir = std::env::temp_dir().join(format!("audit-test-{}", crate::db::new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("t.db")).unwrap();
        log(&db, "chat.create", "a", json!({}));
        ai(&db, "s1", "tool.run", "b", "ok", json!({"x": 1}));
        log(&db, "settings.save", "c", json!({}));
        assert_eq!(verify(&db).unwrap()["ok"], true);
        let q = query(&db, &Query { search: "tool".into(), page_size: 50, ..Default::default() }).unwrap();
        assert_eq!(q["total"], 1);
        db.conn.lock().execute("UPDATE audit SET subject='z' WHERE id=2", []).unwrap();
        let v = verify(&db).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["broken_at"], 2);
    }
}

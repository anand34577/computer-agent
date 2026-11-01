//! Local storage: one embedded SQLite database with versioned migrations.
//! A copy of the database is taken before every migration so upgrades can be rolled back.

use anyhow::Result;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::path::Path;

pub struct Db {
    pub conn: Mutex<Connection>,
    /// A second, read-only connection for long reports (stats, audit search and export), so they
    /// never hold up chat and agent writes. WAL mode lets it read while the other one writes.
    pub read: Mutex<Connection>,
}

const MIGRATIONS: &[&str] = &[
    // v1
    r#"
    CREATE TABLE providers (
        id TEXT PRIMARY KEY, kind TEXT NOT NULL, name TEXT NOT NULL,
        base_url TEXT NOT NULL, created INTEGER NOT NULL
    );
    CREATE TABLE models (
        id TEXT PRIMARY KEY, provider_id TEXT NOT NULL, name TEXT NOT NULL, label TEXT NOT NULL,
        profile TEXT NOT NULL DEFAULT '{}', overrides TEXT NOT NULL DEFAULT '{}',
        price_in REAL NOT NULL DEFAULT 0, price_out REAL NOT NULL DEFAULT 0,
        created INTEGER NOT NULL
    );
    CREATE TABLE conversations (
        id TEXT PRIMARY KEY, title TEXT NOT NULL, folder TEXT NOT NULL DEFAULT '',
        pinned INTEGER NOT NULL DEFAULT 0, model_id TEXT, system_prompt TEXT NOT NULL DEFAULT '',
        created INTEGER NOT NULL, updated INTEGER NOT NULL
    );
    CREATE TABLE messages (
        id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, role TEXT NOT NULL,
        content TEXT NOT NULL DEFAULT '', images TEXT NOT NULL DEFAULT '[]',
        reasoning TEXT NOT NULL DEFAULT '', meta TEXT NOT NULL DEFAULT '{}',
        model_id TEXT, created INTEGER NOT NULL
    );
    CREATE INDEX idx_messages_conv ON messages(conversation_id, created);
    CREATE TABLE settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE tasks (
        id TEXT PRIMARY KEY, goal TEXT NOT NULL, status TEXT NOT NULL,
        conversation_id TEXT, cost REAL NOT NULL DEFAULT 0, steps INTEGER NOT NULL DEFAULT 0,
        started INTEGER NOT NULL, ended INTEGER, summary TEXT NOT NULL DEFAULT '',
        plan TEXT NOT NULL DEFAULT '[]', models TEXT NOT NULL DEFAULT '{}'
    );
    CREATE TABLE steps (
        id TEXT PRIMARY KEY, task_id TEXT NOT NULL, idx INTEGER NOT NULL, kind TEXT NOT NULL,
        title TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '', action TEXT NOT NULL DEFAULT '{}',
        result TEXT NOT NULL DEFAULT '', screenshot TEXT NOT NULL DEFAULT '',
        risk TEXT NOT NULL DEFAULT 'routine', model TEXT NOT NULL DEFAULT '',
        cost REAL NOT NULL DEFAULT 0, created INTEGER NOT NULL
    );
    CREATE INDEX idx_steps_task ON steps(task_id, idx);
    CREATE TABLE undo_ops (
        id TEXT PRIMARY KEY, task_id TEXT NOT NULL, op TEXT NOT NULL,
        original TEXT NOT NULL, current TEXT NOT NULL, backup TEXT NOT NULL DEFAULT '',
        created INTEGER NOT NULL, undone INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE usage (
        id INTEGER PRIMARY KEY AUTOINCREMENT, day TEXT NOT NULL, model_id TEXT NOT NULL,
        input INTEGER NOT NULL, output INTEGER NOT NULL, cost REAL NOT NULL
    );
    CREATE TABLE consents (subject TEXT PRIMARY KEY, accepted INTEGER NOT NULL);
    "#,
    // v2: audit trail and indexes for lists that grow
    r#"
    CREATE TABLE audit (
        id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, category TEXT NOT NULL,
        action TEXT NOT NULL, actor TEXT NOT NULL, subject TEXT NOT NULL DEFAULT '',
        session TEXT NOT NULL DEFAULT '', outcome TEXT NOT NULL DEFAULT 'ok',
        details TEXT NOT NULL DEFAULT '{}', prev_hash TEXT NOT NULL DEFAULT '', hash TEXT NOT NULL DEFAULT ''
    );
    CREATE INDEX idx_audit_ts ON audit(ts);
    CREATE INDEX idx_audit_cat ON audit(category, ts);
    CREATE INDEX idx_audit_session ON audit(session, ts);
    CREATE INDEX idx_conv_updated ON conversations(updated);
    CREATE INDEX idx_tasks_started ON tasks(started);
    "#,
    // v3: chat documents (direct or embedded), per-reply speed log, per-chat parameters
    r#"
    CREATE TABLE documents (
        id TEXT PRIMARY KEY, conversation_id TEXT NOT NULL, name TEXT NOT NULL,
        mode TEXT NOT NULL, text TEXT NOT NULL DEFAULT '', chars INTEGER NOT NULL DEFAULT 0,
        embed_model TEXT NOT NULL DEFAULT '', created INTEGER NOT NULL
    );
    CREATE INDEX idx_documents_conv ON documents(conversation_id);
    CREATE TABLE chunks (
        id INTEGER PRIMARY KEY AUTOINCREMENT, doc_id TEXT NOT NULL, idx INTEGER NOT NULL,
        text TEXT NOT NULL, vec BLOB NOT NULL
    );
    CREATE INDEX idx_chunks_doc ON chunks(doc_id);
    CREATE TABLE replies (
        id INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, model_id TEXT NOT NULL,
        model TEXT NOT NULL, provider TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'chat',
        input INTEGER NOT NULL DEFAULT 0, output INTEGER NOT NULL DEFAULT 0,
        ttft_ms INTEGER NOT NULL DEFAULT 0, gen_ms INTEGER NOT NULL DEFAULT 0,
        tps REAL NOT NULL DEFAULT 0, cost REAL NOT NULL DEFAULT 0, outcome TEXT NOT NULL DEFAULT 'ok'
    );
    CREATE INDEX idx_replies_ts ON replies(ts);
    ALTER TABLE conversations ADD COLUMN params TEXT NOT NULL DEFAULT '{}';
    "#,
];

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let target = MIGRATIONS.len() as i64;
        if version < target {
            if version > 0 {
                // VACUUM INTO writes a consistent copy that includes what is still in the WAL file.
                let backup = path.with_extension(format!("v{version}.bak"));
                let _ = std::fs::remove_file(&backup);
                if conn.execute("VACUUM INTO ?1", [backup.to_string_lossy()]).is_err() {
                    let _ = std::fs::copy(path, &backup);
                }
            }
            for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
                let tx = conn.unchecked_transaction()?;
                tx.execute_batch(sql)?;
                tx.pragma_update(None, "user_version", (i + 1) as i64)?;
                tx.commit()?;
            }
        }
        let read = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        Ok(Self { conn: Mutex::new(conn), read: Mutex::new(read) })
    }
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

pub fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

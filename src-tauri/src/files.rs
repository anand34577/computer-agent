//! Direct file tools. Every change goes through a holding area so a whole task
//! can be rolled back within the undo window.

use crate::db::{new_id, now, Db};
use crate::settings::AppSettings;
use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct FileTools<'a> {
    pub db: &'a Db,
    pub holding: PathBuf,
    pub task_id: String,
    pub settings: &'a AppSettings,
}

pub fn expand(p: &str) -> PathBuf {
    let p = p.trim().trim_matches('"');
    if let Some(rest) = p.strip_prefix("~") {
        if let Some(h) = dirs::home_dir() {
            return h.join(rest.trim_start_matches(['/', '\\']));
        }
    }
    let lower = p.to_lowercase();
    for (name, dir) in [("downloads", dirs::download_dir()), ("documents", dirs::document_dir()), ("desktop", dirs::desktop_dir()), ("pictures", dirs::picture_dir())] {
        if lower == name {
            if let Some(d) = dir {
                return d;
            }
        }
    }
    PathBuf::from(p)
}

fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\").to_lowercase();
    if cfg!(windows) { s } else { p.to_string_lossy().to_string() }
}

pub fn cloud_synced(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    ["onedrive", "dropbox", "google drive", "icloud", "box sync"].iter().any(|k| s.contains(k))
}

impl<'a> FileTools<'a> {
    pub fn check(&self, p: &Path) -> Result<()> {
        if !p.is_absolute() {
            bail!("Use a full path (for example C:\\Users\\you\\Downloads\\file.txt)");
        }
        let n = norm(p);
        for prot in &self.settings.protected_folders {
            let pn = norm(Path::new(prot));
            if !pn.is_empty() && n.starts_with(&pn) {
                bail!("'{}' is inside a protected folder ({prot}). The agent may not change it.", p.display());
            }
        }
        if !self.settings.allowed_folders.is_empty()
            && !self.settings.allowed_folders.iter().any(|a| !a.is_empty() && n.starts_with(&norm(Path::new(a))))
        {
            bail!("'{}' is outside the allowed folders. Add it in Settings → Permissions.", p.display());
        }
        if n.contains(&norm(&self.holding)) {
            bail!("The undo holding area is off-limits");
        }
        Ok(())
    }

    fn record(&self, op: &str, original: &Path, current: &Path, backup: &str) -> Result<()> {
        self.db.conn.lock().execute(
            "INSERT INTO undo_ops(id,task_id,op,original,current,backup,created) VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![new_id(), self.task_id, op, original.to_string_lossy(), current.to_string_lossy(), backup, now()],
        )?;
        Ok(())
    }

    /// Logs a newly created file or folder so undo can remove it.
    pub fn record_create(&self, p: &Path) -> Result<()> {
        self.record(if p.is_dir() { "create_dir" } else { "create" }, p, p, "")
    }

    fn hold_path(&self, name: &str) -> Result<PathBuf> {
        let dir = self.holding.join(&self.task_id);
        fs::create_dir_all(&dir)?;
        Ok(dir.join(format!("{}-{}", &new_id()[..8], name)))
    }

    fn note(&self, p: &Path) -> &'static str {
        if cloud_synced(p) {
            " Note: this folder is cloud-synced, so undo may not fully reverse what the sync service already uploaded."
        } else {
            ""
        }
    }

    pub fn run(&self, tool: &str, a: &Value) -> Result<String> {
        match tool {
            "list_files" => {
                let p = expand(a["path"].as_str().unwrap_or(""));
                let mut out = vec![];
                for e in fs::read_dir(&p).map_err(|e| anyhow!("Cannot open {}: {e}", p.display()))?.flatten().take(400) {
                    let md = e.metadata().ok();
                    let is_dir = md.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                    let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
                    out.push(format!("{}{} {}", if is_dir { "[dir] " } else { "" }, e.file_name().to_string_lossy(), if is_dir { String::new() } else { human(size) }));
                }
                out.sort();
                Ok(format!("{} ({} items)\n{}", p.display(), out.len(), out.join("\n")))
            }
            "read_file" => {
                let p = expand(a["path"].as_str().unwrap_or(""));
                let bytes = fs::read(&p).map_err(|e| anyhow!("Cannot read {}: {e}", p.display()))?;
                let text = String::from_utf8_lossy(&bytes);
                let t: String = text.chars().take(20000).collect();
                Ok(if text.chars().count() > 20000 { format!("{t}\n…(truncated)") } else { t })
            }
            "create_folder" => {
                let p = expand(a["path"].as_str().unwrap_or(""));
                self.check(&p)?;
                if p.exists() {
                    return Ok(format!("{} already exists", p.display()));
                }
                fs::create_dir_all(&p)?;
                self.record("create_dir", &p, &p, "")?;
                Ok(format!("Created folder {}", p.display()))
            }
            "move_file" | "rename_file" => {
                let from = expand(a["from"].as_str().unwrap_or(""));
                let mut to = expand(a["to"].as_str().unwrap_or(""));
                if tool == "rename_file" && !to.is_absolute() {
                    to = from.with_file_name(a["to"].as_str().unwrap_or(""));
                }
                self.check(&from)?;
                if to.is_dir() {
                    to = to.join(from.file_name().ok_or_else(|| anyhow!("bad source"))?);
                }
                self.check(&to)?;
                if !from.exists() {
                    bail!("{} does not exist", from.display());
                }
                if to.exists() {
                    bail!("{} already exists; choose another name", to.display());
                }
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                move_path(&from, &to)?;
                self.record("move", &from, &to, "")?;
                Ok(format!("Moved {} → {}.{}", from.display(), to.display(), self.note(&to)))
            }
            "delete_file" => {
                let p = expand(a["path"].as_str().unwrap_or(""));
                self.check(&p)?;
                if !p.exists() {
                    bail!("{} does not exist", p.display());
                }
                let hold = self.hold_path(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "item".into()))?;
                move_path(&p, &hold)?;
                self.record("delete", &p, &hold, "")?;
                Ok(format!(
                    "Moved {} to the undo holding area (restorable for {} hours).{}",
                    p.display(),
                    self.settings.undo_hours,
                    self.note(&p)
                ))
            }
            "write_file" => {
                let p = expand(a["path"].as_str().unwrap_or(""));
                self.check(&p)?;
                let content = a["content"].as_str().unwrap_or("");
                if p.exists() {
                    let hold = self.hold_path(&p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())?;
                    fs::copy(&p, &hold)?;
                    fs::write(&p, content)?;
                    self.record("overwrite", &p, &p, &hold.to_string_lossy())?;
                    Ok(format!("Overwrote {} (previous version kept for undo)", p.display()))
                } else {
                    if let Some(parent) = p.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(&p, content)?;
                    self.record("create", &p, &p, "")?;
                    Ok(format!("Created {}", p.display()))
                }
            }
            _ => bail!("unknown file tool"),
        }
    }
}

fn move_path(from: &Path, to: &Path) -> Result<()> {
    if fs::rename(from, to).is_ok() {
        return Ok(());
    }
    // Cross-drive: copy then remove.
    if from.is_dir() {
        copy_dir(from, to)?;
        fs::remove_dir_all(from)?;
    } else {
        fs::copy(from, to)?;
        fs::remove_file(from)?;
    }
    Ok(())
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    fs::create_dir_all(to)?;
    for e in fs::read_dir(from)?.flatten() {
        let t = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &t)?;
        } else {
            fs::copy(e.path(), t)?;
        }
    }
    Ok(())
}

fn human(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

/// Reverses every file change of a task, newest first.
pub fn undo_task(db: &Db, holding: &Path, task_id: &str) -> Result<Value> {
    let ops: Vec<(String, String, String, String, String)> = {
        let conn = db.conn.lock();
        let mut st = conn.prepare("SELECT id,op,original,current,backup FROM undo_ops WHERE task_id=?1 AND undone=0 ORDER BY created DESC")?;
        let rows = st.query_map([task_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?.filter_map(|r| r.ok()).collect();
        rows
    };
    let mut restored = 0;
    let mut problems = vec![];
    for (id, op, original, current, backup) in ops {
        let (o, c) = (PathBuf::from(&original), PathBuf::from(&current));
        let r: Result<()> = (|| {
            match op.as_str() {
                "move" | "delete" => {
                    if o.exists() {
                        bail!("{} exists again; left in place", o.display());
                    }
                    if let Some(p) = o.parent() {
                        fs::create_dir_all(p)?;
                    }
                    move_path(&c, &o)
                }
                "overwrite" => {
                    fs::copy(&backup, &o)?;
                    Ok(())
                }
                "create" => {
                    let hold = holding.join(task_id).join(format!("undone-{}", o.file_name().unwrap_or_default().to_string_lossy()));
                    fs::create_dir_all(hold.parent().unwrap())?;
                    move_path(&o, &hold)
                }
                "create_dir" => {
                    fs::remove_dir(&o).map_err(|_| anyhow!("{} is not empty; left in place", o.display()))
                }
                _ => Ok(()),
            }
        })();
        match r {
            Ok(_) => {
                restored += 1;
                db.conn.lock().execute("UPDATE undo_ops SET undone=1 WHERE id=?1", [id])?;
            }
            Err(e) => problems.push(e.to_string()),
        }
    }
    Ok(json!({"restored": restored, "problems": problems}))
}

/// Deletes holding-area items older than the undo window.
pub fn purge_expired(db: &Db, holding: &Path, hours: u32) {
    let cutoff = now() - hours as i64 * 3600 * 1000;
    let tasks: Vec<String> = {
        let conn = db.conn.lock();
        let Ok(mut st) = conn.prepare("SELECT DISTINCT task_id FROM undo_ops WHERE created < ?1 AND undone=0") else { return };
        st.query_map([cutoff], |r| r.get(0)).map(|rows| rows.filter_map(|r| r.ok()).collect()).unwrap_or_default()
    };
    for t in tasks {
        let newest: i64 = db.conn.lock().query_row("SELECT MAX(created) FROM undo_ops WHERE task_id=?1", [&t], |r| r.get(0)).unwrap_or(0);
        if newest < cutoff {
            let _ = fs::remove_dir_all(holding.join(&t));
            let _ = db.conn.lock().execute("UPDATE undo_ops SET undone=2 WHERE task_id=?1", [&t]);
        }
    }
}

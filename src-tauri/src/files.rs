//! Direct file tools. Every change goes through a holding area so a whole task
//! can be rolled back within the undo window.

use crate::db::{new_id, now, Db};
use crate::settings::AppSettings;
use anyhow::{anyhow, bail, Result};
use rusqlite::params;
use serde_json::{json, Value};
use std::fs;
use std::path::{Component, Path, PathBuf};

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

/// The real location of a path: no `.`/`..` tricks, and links followed on the part that exists,
/// so a folder rule cannot be stepped around by how a path is spelled.
pub fn resolve(p: &Path) -> Result<PathBuf> {
    if p.components().any(|c| c == Component::ParentDir) {
        bail!("Use a full path without '..' ({})", p.display());
    }
    let clean: PathBuf = p.components().filter(|c| *c != Component::CurDir).collect();
    let mut base = clean.clone();
    let mut rest = vec![];
    while !base.exists() {
        match base.file_name() {
            Some(n) => {
                rest.push(n.to_os_string());
                base.pop();
            }
            None => break,
        }
    }
    let mut out = fs::canonicalize(&base).map(strip_verbatim).unwrap_or(base);
    for n in rest.into_iter().rev() {
        out.push(n);
    }
    Ok(out)
}

/// `\\?\C:\x` → `C:\x` and `\\?\UNC\server\share` → `\\server\share` (what canonicalize returns on Windows).
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{r}"));
    }
    if let Some(r) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(r);
    }
    p
}

/// Comparable form of a path: one separator, no trailing one, case-folded where the OS ignores case.
fn key(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.trim_end_matches('/').to_string();
    if cfg!(any(windows, target_os = "macos")) { s.to_lowercase() } else { s }
}

/// Whether `p` is `folder` or inside it, compared by whole path segments.
pub fn within(p: &Path, folder: &Path) -> bool {
    let (p, f) = (key(&resolve(p).unwrap_or_else(|_| p.to_path_buf())), key(&resolve(folder).unwrap_or_else(|_| folder.to_path_buf())));
    !f.is_empty() && (p == f || p.starts_with(&format!("{f}/")))
}

const SECRET_DIRS: &[&str] = &[
    "/.ssh/", "/.gnupg/", "/.aws/", "/.azure/", "/.kube/", "/.docker/", "/.password-store/", "/.local/share/keyrings/",
    "/library/keychains/", "/library/cookies/", "/library/application support/google/chrome/", "/library/application support/firefox/",
    "/appdata/local/google/chrome/user data/", "/appdata/local/microsoft/edge/user data/", "/appdata/local/bravesoftware/",
    "/appdata/roaming/mozilla/firefox/profiles/", "/appdata/roaming/microsoft/credentials/", "/appdata/local/microsoft/credentials/",
    "/appdata/roaming/microsoft/protect/", "/appdata/roaming/microsoft/crypto/", "/windows/system32/config/",
    "/.config/google-chrome/", "/.config/chromium/", "/.config/bravesoftware/", "/.mozilla/firefox/",
];
const SECRET_NAMES: &[&str] = &[".netrc", "_netrc", ".git-credentials", ".npmrc", ".pypirc", ".pgpass", "credentials.json", "secrets.json"];
const SECRET_PREFIXES: &[&str] = &["id_rsa", "id_dsa", "id_ecdsa", "id_ed25519", ".env"];
const SECRET_EXTS: &[&str] = &["kdbx", "kdb", "pem", "key", "pfx", "p12", "ppk", "keychain", "keychain-db", "jks", "keystore"];

/// Why a path holds secrets the agent and chat tools may never read (keys, passwords, browser
/// profiles, this app's own data). A fixed engine rule, not a setting a model can talk around.
pub fn secret_reason(p: &Path, app_data: &Path) -> Option<String> {
    if within(p, app_data) {
        return Some("this app's own data folder".into());
    }
    let k = format!("{}/", key(p).to_lowercase());
    if let Some(d) = SECRET_DIRS.iter().find(|d| k.contains(*d)) {
        return Some(format!("a folder that holds keys or passwords ({})", d.trim_matches('/')));
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if SECRET_NAMES.contains(&name.as_str()) || SECRET_PREFIXES.iter().any(|x| name.starts_with(x)) || SECRET_EXTS.contains(&ext.as_str()) {
        return Some(format!("a file that usually holds keys or passwords ({name})"));
    }
    None
}

pub fn cloud_synced(p: &Path) -> bool {
    let s = p.to_string_lossy().to_lowercase();
    ["onedrive", "dropbox", "google drive", "icloud", "box sync"].iter().any(|k| s.contains(k))
}

impl<'a> FileTools<'a> {
    fn app_data(&self) -> PathBuf {
        self.holding.parent().map(Path::to_path_buf).unwrap_or_else(|| self.holding.clone())
    }

    /// Rules for reading: never secrets, and only inside the allowed folders.
    pub fn check_read(&self, p: &Path) -> Result<PathBuf> {
        check_read(p, self.settings, &self.app_data())
    }

    /// Rules for changing: the read rules, plus protected folders and the undo area.
    pub fn check(&self, p: &Path) -> Result<()> {
        let r = self.check_read(p)?;
        for prot in self.settings.protected_folders.iter().filter(|x| !x.trim().is_empty()) {
            if within(&r, Path::new(prot)) {
                bail!("'{}' is inside a protected folder ({prot}). The agent may not change it.", p.display());
            }
        }
        Ok(())
    }
}

/// Read rules without a task (shared by the search tools, which walk folders).
pub fn check_read(p: &Path, s: &AppSettings, app_data: &Path) -> Result<PathBuf> {
    if !p.is_absolute() {
        bail!("Use a full path (for example C:\\Users\\you\\Downloads\\file.txt)");
    }
    let r = resolve(p)?;
    if let Some(why) = secret_reason(&r, app_data) {
        bail!("'{}' is off-limits: {why}. Open it yourself if you need it.", p.display());
    }
    let allowed: Vec<&String> = s.allowed_folders.iter().filter(|a| !a.trim().is_empty()).collect();
    if !allowed.is_empty() && !allowed.iter().any(|a| within(&r, Path::new(a.as_str()))) {
        bail!("'{}' is outside the allowed folders. Add it in Settings → Permissions.", p.display());
    }
    Ok(r)
}

impl<'a> FileTools<'a> {

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
                self.check_read(&p)?;
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
                self.check_read(&p)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ca-files-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn settings(allowed: &Path, protected: &Path) -> AppSettings {
        AppSettings { allowed_folders: vec![allowed.to_string_lossy().into()], protected_folders: vec![protected.to_string_lossy().into()], ..AppSettings::default() }
    }

    #[test]
    fn dot_dot_paths_are_refused() {
        let d = temp("dots");
        assert!(resolve(&d.join("a").join("..").join("b")).is_err());
        assert_eq!(key(&resolve(&d.join(".").join("new.txt")).unwrap()), key(&resolve(&d).unwrap().join("new.txt")));
    }

    #[test]
    fn within_compares_whole_segments() {
        let d = temp("within");
        fs::create_dir_all(d.join("Work")).unwrap();
        assert!(within(&d.join("Work").join("x.txt"), &d.join("Work")));
        assert!(!within(&d.join("Workshop").join("x.txt"), &d.join("Work")), "a shared prefix is not the same folder");
    }

    #[test]
    fn secrets_are_never_readable() {
        let app = Path::new("/nonexistent-app-data");
        for p in ["/home/u/.ssh/id_ed25519", "/home/u/project/.env", "/home/u/vault.kdbx", "C:/Users/u/AppData/Local/Google/Chrome/User Data/Default/Login Data"] {
            assert!(secret_reason(Path::new(p), app).is_some(), "{p}");
        }
        assert!(secret_reason(Path::new("/home/u/notes/environment.md"), app).is_none());
    }

    #[test]
    fn read_and_write_rules() {
        let root = temp("rules");
        let (allowed, protected, app) = (root.join("allowed"), root.join("allowed").join("locked"), root.join("appdata"));
        fs::create_dir_all(&protected).unwrap();
        let s = settings(&allowed, &protected);
        assert!(check_read(&allowed.join("a.txt"), &s, &app).is_ok());
        assert!(check_read(&root.join("outside.txt"), &s, &app).is_err(), "outside the allowed folders");
        assert!(check_read(&allowed.join("..").join("outside.txt"), &s, &app).is_err(), "'..' cannot step out");
        assert!(check_read(&app.join("agent.db"), &s, &app).is_err(), "the app's own data");
        let db = crate::db::Db::open(&root.join("t.db")).unwrap();
        let ft = FileTools { db: &db, holding: app.join("holding"), task_id: "t".into(), settings: &s };
        assert!(ft.check(&protected.join("x.txt")).is_err(), "protected folders cannot change");
        assert!(ft.check(&allowed.join("x.txt")).is_ok());
    }

    #[test]
    fn write_then_undo_restores() {
        let root = temp("undo");
        let (allowed, app) = (root.join("work"), root.join("appdata"));
        fs::create_dir_all(&allowed).unwrap();
        let s = settings(&allowed, &root.join("none"));
        let db = crate::db::Db::open(&root.join("t.db")).unwrap();
        let ft = FileTools { db: &db, holding: app.join("holding"), task_id: "t1".into(), settings: &s };
        let f = allowed.join("note.txt");
        fs::write(&f, "before").unwrap();
        ft.run("write_file", &json!({"path": f.to_string_lossy(), "content": "after"})).unwrap();
        ft.run("write_file", &json!({"path": allowed.join("new.txt").to_string_lossy(), "content": "x"})).unwrap();
        assert_eq!(fs::read_to_string(&f).unwrap(), "after");
        let r = undo_task(&db, &app.join("holding"), "t1").unwrap();
        assert_eq!(r["restored"], 2);
        assert_eq!(fs::read_to_string(&f).unwrap(), "before");
        assert!(!allowed.join("new.txt").exists());
    }
}

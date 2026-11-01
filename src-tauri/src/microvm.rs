//! Built-in micro VM: a small Linux system (Buildroot + BusyBox) running in the v86 x86
//! emulator inside a hidden app window. It needs no WSL, Docker, Hyper-V or admin rights,
//! works the same on every platform, and has no network, so commands cannot reach out.
//!
//! The engine owns the lifecycle (download, verify, start, stop) and talks to the window
//! through events: `vm-exec` goes in, the window answers with the `vm_result` command.
//! Files in the shared folder are synced in before each command and changed files come back.

use crate::sandbox::CmdOutput;
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

/// Pinned images: a mismatch means a corrupted or tampered download, which is refused.
pub const IMAGES: &[(&str, &str, u64, &str)] = &[
    ("seabios.bin", "https://raw.githubusercontent.com/copy/v86/315b2b0b758703ef0e6cc7b4088d6068f83f3c20/bios/seabios.bin", 131_072, "73e3f359102e3a9982c35fce98eb7cd08f18303ac7f1ba6ebfbe6cdc1c244d98"),
    ("vgabios.bin", "https://raw.githubusercontent.com/copy/v86/315b2b0b758703ef0e6cc7b4088d6068f83f3c20/bios/vgabios.bin", 36_352, "a4bc0d80cc3ca028c73dafa8fee396b8d054ce87ebd8abfbd31b06b437607880"),
    ("linux.bin", "https://i.copy.sh/buildroot-bzimage68.bin", 10_068_480, "507a759c70ab7a490a233be454d0b5b88bc667956a410b531cb4edc091e2eb1c"),
];

const SYNC_LIMIT: u64 = 32 * 1024 * 1024;

#[derive(Default)]
pub struct Vm {
    app: OnceLock<AppHandle>,
    dir: OnceLock<PathBuf>,
    /// "off" | "starting" | "ready" | "error"
    state: Mutex<String>,
    error: Mutex<String>,
    ready: tokio::sync::Notify,
    pending: Mutex<HashMap<u64, oneshot::Sender<Value>>>,
    next: AtomicU64,
    lock: tokio::sync::Mutex<()>,
    installing: AtomicBool,
    pub boot_ms: AtomicU64,
    last_used: AtomicU64,
    memory_mb: AtomicU64,
}

#[derive(Serialize, Clone)]
pub struct Status {
    pub installed: bool,
    pub installing: bool,
    pub state: String,
    pub error: String,
    pub size_mb: f64,
    pub boot_ms: u64,
    pub memory_mb: u64,
}

#[derive(Deserialize)]
pub struct ExecResult {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    #[serde(default)]
    pub timed_out: bool,
    /// Changed files in the shared folder: (relative path, base64 content).
    #[serde(default)]
    pub files: Vec<(String, String)>,
    /// Files the command deleted from the shared folder.
    #[serde(default)]
    pub deleted: Vec<String>,
}

fn sha256_file(p: &Path) -> Result<String> {
    let mut h = Sha256::new();
    let mut f = std::fs::File::open(p)?;
    std::io::copy(&mut f, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

impl Vm {
    pub fn init(&self, app: AppHandle, data_dir: &Path) {
        let _ = self.app.set(app);
        let _ = self.dir.set(data_dir.join("vm"));
        *self.state.lock() = "off".into();
        self.memory_mb.store(128, Ordering::Relaxed);
    }

    pub fn dir(&self) -> PathBuf {
        self.dir.get().cloned().unwrap_or_default()
    }

    pub fn installed(&self) -> bool {
        IMAGES.iter().all(|(n, _, size, _)| std::fs::metadata(self.dir().join(n)).map(|m| m.len() == *size).unwrap_or(false))
    }

    pub fn status(&self) -> Status {
        let size: u64 = IMAGES.iter().filter_map(|(n, ..)| std::fs::metadata(self.dir().join(n)).ok().map(|m| m.len())).sum();
        Status {
            installed: self.installed(),
            installing: self.installing.load(Ordering::SeqCst),
            state: self.state.lock().clone(),
            error: self.error.lock().clone(),
            size_mb: (size as f64 / 1e5).round() / 10.0,
            boot_ms: self.boot_ms.load(Ordering::Relaxed),
            memory_mb: self.memory_mb.load(Ordering::Relaxed),
        }
    }

    fn emit_status(&self) {
        if let Some(app) = self.app.get() {
            let _ = app.emit("vm-status", self.status());
        }
    }

    /// Downloads and verifies the images, reporting progress as `vm-progress`.
    pub async fn install(&self, http: &reqwest::Client) -> Result<()> {
        use futures_util::StreamExt;
        if self.installing.swap(true, Ordering::SeqCst) {
            bail!("A download is already running");
        }
        let res = async {
            let dir = self.dir();
            std::fs::create_dir_all(&dir)?;
            let total: u64 = IMAGES.iter().map(|i| i.2).sum();
            let mut done_before = 0u64;
            for (name, url, size, hash) in IMAGES {
                let dest = dir.join(name);
                if std::fs::metadata(&dest).map(|m| m.len() == *size).unwrap_or(false) && sha256_file(&dest)? == *hash {
                    done_before += size;
                    continue;
                }
                let part = dir.join(format!("{name}.part"));
                let resp = http.get(*url).timeout(Duration::from_secs(600)).send().await?;
                if !resp.status().is_success() {
                    bail!("Download of {name} failed: {}", resp.status());
                }
                let mut f = tokio::fs::File::create(&part).await?;
                let mut got = 0u64;
                let mut s = resp.bytes_stream();
                let mut last = std::time::Instant::now();
                while let Some(c) = s.next().await {
                    let c = c?;
                    got += c.len() as u64;
                    if got > size * 2 {
                        bail!("{name} is larger than expected; download refused");
                    }
                    tokio::io::AsyncWriteExt::write_all(&mut f, &c).await?;
                    if last.elapsed() > Duration::from_millis(120) {
                        last = std::time::Instant::now();
                        if let Some(app) = self.app.get() {
                            let _ = app.emit("vm-progress", json!({"file": name, "done": done_before + got, "total": total}));
                        }
                    }
                }
                tokio::io::AsyncWriteExt::flush(&mut f).await?;
                drop(f);
                let actual = sha256_file(&part)?;
                if actual != *hash {
                    let _ = std::fs::remove_file(&part);
                    bail!("{name} failed its integrity check (expected {hash}, got {actual}). Nothing was installed.");
                }
                std::fs::rename(&part, &dest)?;
                done_before += size;
            }
            if let Some(app) = self.app.get() {
                let _ = app.emit("vm-progress", json!({"file": "", "done": total, "total": total}));
            }
            Ok(())
        }
        .await;
        self.installing.store(false, Ordering::SeqCst);
        self.emit_status();
        res
    }

    pub async fn remove(&self) -> Result<()> {
        self.stop();
        let dir = self.dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        self.emit_status();
        Ok(())
    }

    pub fn image(&self, name: &str) -> Result<Vec<u8>> {
        if !IMAGES.iter().any(|(n, ..)| *n == name) {
            bail!("Unknown image");
        }
        Ok(std::fs::read(self.dir().join(name))?)
    }

    /// Opens the hidden VM window if needed and waits until Linux has booted.
    pub async fn ensure_started(&self, memory_mb: u64) -> Result<()> {
        if !self.installed() {
            bail!("The built-in Linux VM is not downloaded yet. Open Extensions → Sandbox and choose Download.");
        }
        let app = self.app.get().ok_or_else(|| anyhow!("VM not initialised"))?.clone();
        loop {
            let st = self.state.lock().clone();
            match st.as_str() {
                "ready" if app.get_webview_window("vm").is_some() => return Ok(()),
                "starting" => {}
                _ => {
                    *self.state.lock() = "starting".into();
                    self.error.lock().clear();
                    self.memory_mb.store(memory_mb.clamp(64, 1024), Ordering::Relaxed);
                    self.emit_status();
                    if let Some(w) = app.get_webview_window("vm") {
                        let _ = w.destroy();
                    }
                    let url = tauri::WebviewUrl::App(format!("index.html#vm={}", memory_mb.clamp(64, 1024)).into());
                    let app2 = app.clone();
                    // Window creation must happen on the main thread.
                    let (tx, rx) = oneshot::channel();
                    app.run_on_main_thread(move || {
                        let r = tauri::WebviewWindowBuilder::new(&app2, "vm", url).title("Linux VM").visible(false).skip_taskbar(true).inner_size(400.0, 300.0).build();
                        let _ = tx.send(r.map(|_| ()).map_err(|e| e.to_string()));
                    })?;
                    rx.await.map_err(|_| anyhow!("VM window did not open"))?.map_err(|e| anyhow!("Could not open the VM window: {e}"))?;
                }
            }
            let notified = self.ready.notified();
            if *self.state.lock() == "ready" {
                return Ok(());
            }
            if tokio::time::timeout(Duration::from_secs(120), notified).await.is_err() {
                self.fail("Linux did not finish booting within 2 minutes");
            }
            match self.state.lock().as_str() {
                "ready" => return Ok(()),
                "error" => bail!("The VM failed to start: {}", self.error.lock()),
                _ => {}
            }
        }
    }

    /// Called by the VM window once the shell prompt is up.
    pub fn on_ready(&self, boot_ms: u64) {
        self.boot_ms.store(boot_ms, Ordering::Relaxed);
        *self.state.lock() = "ready".into();
        self.ready.notify_waiters();
        self.emit_status();
    }

    pub fn fail(&self, msg: &str) {
        *self.state.lock() = "error".into();
        *self.error.lock() = msg.to_string();
        self.ready.notify_waiters();
        for (_, tx) in self.pending.lock().drain() {
            let _ = tx.send(json!({"error": msg}));
        }
        self.emit_status();
    }

    pub fn stop(&self) {
        if let Some(app) = self.app.get() {
            if let Some(w) = app.get_webview_window("vm") {
                let _ = w.destroy();
            }
        }
        for (_, tx) in self.pending.lock().drain() {
            let _ = tx.send(json!({"error": "The VM was stopped"}));
        }
        *self.state.lock() = "off".into();
        self.emit_status();
    }

    pub fn on_result(&self, v: Value) {
        let id = v["id"].as_u64().unwrap_or(0);
        if let Some(tx) = self.pending.lock().remove(&id) {
            let _ = tx.send(v);
        }
    }

    /// Runs a shell command in the VM. The shared folder appears as /mnt/work (the working directory).
    pub async fn exec(&self, cmdline: &str, shared: &Path, timeout_secs: u64, memory_mb: u64) -> Result<CmdOutput> {
        let _one_at_a_time = self.lock.lock().await;
        self.ensure_started(memory_mb).await?;
        let app = self.app.get().ok_or_else(|| anyhow!("VM not initialised"))?.clone();
        std::fs::create_dir_all(shared)?;
        let files = collect_files(shared)?;
        let id = self.next.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        app.emit_to("vm", "vm-exec", json!({"id": id, "cmd": cmdline, "timeout_ms": timeout_secs * 1000, "files": files}))?;
        let v = match tokio::time::timeout(Duration::from_secs(timeout_secs + 30), rx).await {
            Ok(Ok(v)) => v,
            Ok(Err(_)) => bail!("The VM closed while the command ran"),
            Err(_) => {
                self.pending.lock().remove(&id);
                // The guest is unresponsive; restart it next time.
                self.stop();
                bail!("The VM stopped responding and was restarted");
            }
        };
        self.last_used.store(crate::db::now() as u64, Ordering::Relaxed);
        if let Some(err) = v["error"].as_str() {
            bail!("{err}");
        }
        let r: ExecResult = serde_json::from_value(v)?;
        let mut synced = 0;
        for (rel, b64) in &r.files {
            let rel = rel.trim_start_matches('/');
            if rel.is_empty() || rel.split('/').any(|c| c == ".." || c.is_empty()) {
                continue;
            }
            if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                let dest = shared.join(rel);
                if let Some(p) = dest.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                if std::fs::write(&dest, bytes).is_ok() {
                    synced += 1;
                }
            }
        }
        for rel in &r.deleted {
            let rel = rel.trim_start_matches('/');
            if !rel.is_empty() && !rel.split('/').any(|c| c == ".." || c.is_empty()) {
                let _ = std::fs::remove_file(shared.join(rel));
            }
        }
        let mut stderr = r.stderr;
        if synced > 0 {
            stderr.push_str(&format!("\n[{synced} changed file(s) copied to the shared folder {}]", shared.display()));
        }
        Ok(CmdOutput { code: r.code, stdout: r.stdout, stderr, target: "built-in Linux VM".into(), timed_out: r.timed_out })
    }

    /// Stops a VM that has sat idle, to give its memory back.
    pub fn idle_check(&self, idle_minutes: u64) {
        if idle_minutes == 0 || *self.state.lock() != "ready" {
            return;
        }
        let last = self.last_used.load(Ordering::Relaxed);
        if last > 0 && (crate::db::now() as u64).saturating_sub(last) > idle_minutes * 60_000 && self.lock.try_lock().is_ok() {
            self.stop();
        }
    }
}

/// Files of the shared folder to copy into the VM (skipped above the size limit).
fn collect_files(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut out = vec![];
    let mut total = 0u64;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            let Ok(m) = e.metadata() else { continue };
            if m.is_dir() {
                stack.push(p);
            } else if m.is_file() {
                total += m.len();
                if total > SYNC_LIMIT {
                    return Ok(out);
                }
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/");
                out.push((rel, base64::engine::general_purpose::STANDARD.encode(std::fs::read(&p)?)));
            }
        }
    }
    Ok(out)
}

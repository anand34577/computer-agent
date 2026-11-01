//! Command execution: inside the built-in Linux VM, a WSL distribution or a Docker container
//! when configured, otherwise on the host behind a safety gate.

use crate::settings::SandboxCfg;
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

#[derive(Serialize, Clone, Debug, Default)]
pub struct Availability {
    pub wsl: bool,
    pub wsl_distros: Vec<String>,
    pub docker: bool,
    pub docker_version: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct CmdOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
    pub target: String,
    pub timed_out: bool,
}

impl CmdOutput {
    pub fn for_model(&self) -> String {
        let mut s = format!("Ran in {} · exit code {}{}\n", self.target, self.code, if self.timed_out { " (timed out)" } else { "" });
        if !self.stdout.trim().is_empty() {
            s.push_str("stdout:\n");
            s.push_str(&clip(&self.stdout, 12000));
            s.push('\n');
        }
        if !self.stderr.trim().is_empty() {
            s.push_str("stderr:\n");
            s.push_str(&clip(&self.stderr, 4000));
        }
        s
    }
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}\n…(truncated)", s.chars().take(n).collect::<String>())
    } else {
        s.to_string()
    }
}

/// Output kept per stream; the rest is read and dropped so the program never blocks on a full pipe.
const MAX_OUTPUT: usize = 2_000_000;

pub struct Finished {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

async fn drain(mut r: impl tokio::io::AsyncRead + Unpin) -> Vec<u8> {
    let (mut keep, mut chunk) = (Vec::new(), vec![0u8; 16384]);
    while let Ok(n) = r.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        let room = MAX_OUTPUT.saturating_sub(keep.len());
        keep.extend_from_slice(&chunk[..n.min(room)]);
    }
    keep
}

/// Stops a process and everything it started (a shell's children would otherwise keep running
/// and hold the output pipes open).
async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)]
        let _ = command("taskkill").args(["/T", "/F", "/PID", &pid.to_string()]).output().await;
        #[cfg(unix)]
        let _ = Command::new("kill").args(["-KILL", &format!("-{pid}")]).output().await;
    }
    let _ = child.kill().await;
}

/// Runs a prepared command with optional stdin, a time limit that ends the whole process tree,
/// and bounded output.
pub async fn run_bounded(mut c: Command, input: Option<String>, limit: Duration) -> Result<Finished> {
    use tokio::io::AsyncWriteExt;
    c.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    c.process_group(0);
    let mut child = c.spawn()?;
    if let (Some(text), Some(mut si)) = (input, child.stdin.take()) {
        // Write in the background so a program that prints before reading cannot deadlock us.
        tokio::spawn(async move {
            let _ = si.write_all(text.as_bytes()).await;
        });
    }
    let out = tokio::spawn(drain(child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?));
    let err = tokio::spawn(drain(child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?));
    let (code, timed_out) = match tokio::time::timeout(limit, child.wait()).await {
        Ok(Ok(st)) => (st.code(), false),
        Ok(Err(e)) => bail!("Command failed: {e}"),
        Err(_) => {
            kill_tree(&mut child).await;
            (None, true)
        }
    };
    // Grandchildren may still hold the pipes; never wait on them for long.
    let grace = Duration::from_secs(if timed_out { 2 } else { 10 });
    let collect = |h: tokio::task::JoinHandle<Vec<u8>>| async move {
        let abort = h.abort_handle();
        match tokio::time::timeout(grace, h).await {
            Ok(Ok(b)) => b,
            _ => {
                abort.abort();
                Vec::new()
            }
        }
    };
    let (o, e) = tokio::join!(collect(out), collect(err));
    Ok(Finished { code, stdout: String::from_utf8_lossy(&o).to_string(), stderr: String::from_utf8_lossy(&e).to_string(), timed_out })
}

/// Builds a command that hides the console window on Windows.
pub fn command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        c.creation_flags(0x0800_0000);
    }
    c
}

/// On Windows, scripts such as `npx` are `.cmd` shims that must run through cmd.exe.
pub fn program_command(program: &str, args: &[String]) -> Command {
    #[cfg(windows)]
    {
        let has_ext = Path::new(program).extension().is_some();
        if !has_ext {
            let mut c = command("cmd");
            c.arg("/C").arg(program).args(args);
            return c;
        }
    }
    let mut c = command(program);
    c.args(args);
    c
}

/// WSL prints its list in UTF-16.
fn decode_wsl(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).take(8).all(|b| *b == 0) {
        let u16s: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

pub async fn detect() -> Availability {
    let mut a = Availability::default();
    if cfg!(windows) {
        if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(8), command("wsl.exe").args(["-l", "-q"]).output()).await {
            if out.status.success() {
                a.wsl_distros = decode_wsl(&out.stdout)
                    .lines()
                    .map(|l| l.trim().trim_matches('\u{0}').to_string())
                    .filter(|l| !l.is_empty() && !l.starts_with("docker-desktop"))
                    .collect();
                a.wsl = !a.wsl_distros.is_empty();
            }
        }
    }
    if let Ok(Ok(out)) = tokio::time::timeout(Duration::from_secs(8), command("docker").args(["version", "--format", "{{.Server.Version}}"]).output()).await {
        if out.status.success() {
            a.docker = true;
            a.docker_version = String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    a
}

/// Where a command would run with this configuration.
pub fn target(cfg: &SandboxCfg) -> &'static str {
    match cfg.kind.as_str() {
        "microvm" => "sandbox",
        "wsl" if !cfg.distro.is_empty() => "sandbox",
        "docker" if !cfg.image.is_empty() => "sandbox",
        _ => "host",
    }
}

pub async fn run(vm: &crate::microvm::Vm, cfg: &SandboxCfg, drop_dir: &Path, cmdline: &str, force_host: bool) -> Result<CmdOutput> {
    if cmdline.trim().is_empty() {
        bail!("No command given");
    }
    if cmdline.len() > 8000 {
        bail!("Command is too long");
    }
    std::fs::create_dir_all(drop_dir)?;
    if !force_host && cfg.kind == "microvm" {
        return vm.exec(cmdline, drop_dir, cfg.timeout_secs.clamp(5, 3600) as u64, cfg.vm_memory_mb as u64).await;
    }
    let (mut c, label) = if !force_host && target(cfg) == "sandbox" {
        match cfg.kind.as_str() {
            "wsl" => {
                let mut c = command("wsl.exe");
                c.args(["-d", &cfg.distro, "--cd", "~", "--", "sh", "-lc", cmdline]);
                (c, format!("WSL sandbox ({})", cfg.distro))
            }
            _ => {
                let mut c = command("docker");
                let mount = format!("{}:/work", drop_dir.display());
                c.args(["run", "--rm", "-i", "--memory", "1g", "--cpus", "2", "-v", &mount, "-w", "/work"]);
                if !cfg.network {
                    c.args(["--network", "none"]);
                }
                c.args([cfg.image.as_str(), "sh", "-lc", cmdline]);
                (c, format!("Docker sandbox ({})", cfg.image))
            }
        }
    } else {
        if !cfg.allow_host {
            bail!("Host commands are turned off and no sandbox is configured. Set one up in Extensions → Sandbox.");
        }
        #[cfg(windows)]
        let c = {
            let mut c = command("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", cmdline]);
            c
        };
        #[cfg(not(windows))]
        let c = {
            let mut c = command("sh");
            c.args(["-lc", cmdline]);
            c
        };
        (c, "host shell".to_string())
    };
    if label == "host shell" {
        c.current_dir(dirs::home_dir().unwrap_or_else(std::env::temp_dir));
    }
    let timeout = Duration::from_secs(cfg.timeout_secs.clamp(5, 3600) as u64);
    let f = run_bounded(c, None, timeout).await.map_err(|e| anyhow!("Could not start the command ({label}): {e}"))?;
    Ok(CmdOutput { code: f.code.unwrap_or(-1), stdout: f.stdout, stderr: f.stderr, target: label, timed_out: f.timed_out })
}

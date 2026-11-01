//! Fast decision model ("System 1"): a small model that answers multiple-choice questions about
//! the screen (Intern-Decision). The agent asks it the cheap, frequent questions (still loading?
//! did that work? done? which plan step?) and only acts on answers above the confidence
//! threshold; anything else falls back to the main model.
//!
//! Two ways to run it, both as a local server the app installs, starts and stops:
//! - "gguf" (default): the Q8_0 GGUF on the built-in llama.cpp engine (about 1 GB, GPU via Vulkan).
//!   The scoring protocol is reproduced here: one prompt per question, reading the next-token
//!   probabilities of the answer symbols just before each `<decision>` marker.
//! - "python": the model's official engine in its own Python environment (PyTorch, about 4 GB).

use crate::settings::{AppSettings, DecisionCfg};
use anyhow::{anyhow, bail, Result};
use parking_lot::Mutex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

pub const DEFAULT_REPO: &str = "internlm/Intern-Decision-0.8B";
/// The official model is pinned to a reviewed commit: its repository ships the inference code we run.
const DEFAULT_REVISION: &str = "42457fafd13d10d0b0d9be46cb30a23caa409994";
const SERVER_PY: &str = include_str!("decision_server.py");
/// Versions from the model's requirements.txt.
const TORCH: [&str; 2] = ["torch==2.9.1", "torchvision==0.24.1"];
const LIBS: [&str; 3] = ["transformers==5.14.1", "Pillow>=10.0.0", "huggingface_hub>=0.34"];

/// Community GGUF conversion, pinned and checked by size and SHA-256 (weights only, no code).
const GGUF_REPO: &str = "bombdefuser-124/Intern-Decision-0.8B-GGUF";
const GGUF_REVISION: &str = "8b5d502f407b59e258018aee9fca5bf1cac08ac9";
const GGUF_MODEL: (&str, u64, &str) = ("intern-decision-0.8b-q8_0.gguf", 811_843_552, "3b3df57b50bfe695f31b0d8fb9ad9dbb7a4b1e08134d1140051a32a585e54f9e");
const GGUF_MMPROJ: (&str, u64, &str) = ("mmproj-f16.gguf", 204_987_072, "abcd48e3cda47dfa20497adf7f382f4ca8416b389c425ca05595c165bbd1dc6b");

/// From the model's inference code: the prompt it was trained with and its calibration.
const SYSTEM_PROMPT: &str = "You are a careful decision assistant. Use the state and decision schema in the user message to make the requested decisions. For every field, choose exactly one answer symbol (e.g. A, B, C, ...) from its listed options and return one valid JSON object mapping each field name to its chosen symbol. Use the field names and symbols exactly as given. Do not include explanations, Markdown, or extra text.";
const DECISION_TOKEN: &str = "<decision>";
const ANSWER_SYMBOLS: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const TEMPERATURE: f64 = 2.747760550703;
/// Screenshots for decisions are this wide: answers match full size, at under half the cost.
pub const IMAGE_WIDTH: u32 = 768;

/// One question: yes/no when `options` is None, otherwise a choice among (value, description).
#[derive(Clone, Debug)]
pub struct Q {
    pub name: String,
    pub instructions: String,
    pub options: Option<Vec<(String, String)>>,
}

impl Q {
    pub fn yes_no(name: &str, instructions: &str) -> Self {
        Q { name: name.into(), instructions: instructions.into(), options: None }
    }
    pub fn choice(name: &str, instructions: &str, options: &[(&str, &str)]) -> Self {
        Q { name: name.into(), instructions: instructions.into(), options: Some(options.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()) }
    }
    fn all_options(&self) -> Vec<(String, String)> {
        self.options.clone().unwrap_or_else(|| {
            vec![
                ("no".into(), "The answer is no (negative, or disagree with the claim).".into()),
                ("yes".into(), "The answer is yes (affirmative, or align with the claim).".into()),
            ]
        })
    }
    fn to_json(&self) -> Value {
        match &self.options {
            None => json!({"type": "noul", "instructions": self.instructions}),
            Some(o) => json!({"type": "choice", "instructions": self.instructions, "criteria": o.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>()}),
        }
    }
}

/// The user message and, per question, the assistant text up to its `<decision>` marker.
pub fn compile(state: &Value, qs: &[Q]) -> Result<(String, Vec<String>)> {
    if qs.is_empty() || qs.len() > 16 {
        bail!("Ask 1 to 16 questions");
    }
    let mut lines = vec![];
    for q in qs {
        let opts = q.all_options();
        if opts.is_empty() || opts.len() > ANSWER_SYMBOLS.len() {
            bail!("Question {} needs 1 to {} options", q.name, ANSWER_SYMBOLS.len());
        }
        lines.push(format!("{}: {}", q.name, q.instructions));
        for (sym, (value, desc)) in ANSWER_SYMBOLS.chars().zip(opts) {
            lines.push(format!("    {sym} = {value}: {desc}"));
        }
    }
    // The marker must not appear in the evidence (screen text could contain it).
    let user = format!(
        "Return one answer for every field using the supplied answer symbols.\n\n## State\n{}\n## Decision schema\n{}",
        serde_json::to_string_pretty(state)?,
        lines.join("\n")
    )
    .replace(DECISION_TOKEN, "<decision\u{200b}>");
    let fields: Vec<String> = qs.iter().map(|q| format!("    {}: \"{DECISION_TOKEN}\"", serde_json::to_string(&q.name).unwrap_or_default())).collect();
    let skeleton = format!("{{\n{}\n}}", fields.join(",\n"));
    let mut prefixes = vec![];
    let mut from = 0;
    for _ in qs {
        let at = from + skeleton[from..].find(DECISION_TOKEN).ok_or_else(|| anyhow!("marker missing"))?;
        prefixes.push(skeleton[..at].to_string());
        from = at + DECISION_TOKEN.len();
    }
    Ok((user, prefixes))
}

/// Probabilities over the answer symbols from the reported log-probabilities, calibrated.
fn calibrated(symbols: &[String], logprobs: &HashMap<String, f64>) -> Vec<f64> {
    let raw: Vec<f64> = symbols.iter().map(|s| logprobs.get(s).map(|l| l.exp()).unwrap_or(0.0)).collect();
    let z: f64 = raw.iter().sum();
    if z <= 0.0 {
        return vec![1.0 / symbols.len() as f64; symbols.len()];
    }
    // Temperature scaling on the renormalised distribution, as the reference engine does.
    let w: Vec<f64> = raw.iter().map(|p| if *p > 0.0 { ((p / z).ln() / TEMPERATURE).exp() } else { 0.0 }).collect();
    let t: f64 = w.iter().sum();
    w.iter().map(|x| x / t).collect()
}

fn answer_json(q: &Q, probs: &[f64]) -> Value {
    let opts = q.all_options();
    let (best, conf) = probs.iter().enumerate().fold((0, -1.0), |acc, (i, p)| if *p > acc.1 { (i, *p) } else { acc });
    let map: serde_json::Map<String, Value> = opts.iter().zip(probs).map(|((v, _), p)| (v.clone(), json!(p))).collect();
    let mut a = json!({"type": if q.options.is_some() { "choice" } else { "noul" }, "probabilities": map, "confidence": conf, "decision": opts[best].0});
    if q.options.is_none() {
        a["noul"] = json!(probs.get(1).copied().unwrap_or(0.0));
    }
    a
}

async fn predict_gguf(http: &reqwest::Client, base: &str, key: &str, state: &Value, qs: &[Q], image: Option<String>) -> Result<Value> {
    let (user_text, prefixes) = compile(state, qs)?;
    let mut content = vec![];
    if let Some(b) = image {
        content.push(json!({"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{b}")}}));
    }
    content.push(json!({"type": "text", "text": user_text}));
    let mut answers = serde_json::Map::new();
    let t0 = std::time::Instant::now();
    // One request per question. They share everything up to the question's marker, so the
    // server's prompt cache makes every request after the first cost a few tokens.
    for (q, prefix) in qs.iter().zip(prefixes) {
        let n = q.all_options().len();
        let body = json!({
            "messages": [{"role": "system", "content": SYSTEM_PROMPT}, {"role": "user", "content": content}, {"role": "assistant", "content": prefix}],
            "max_tokens": 1, "temperature": 0, "logprobs": true, "top_logprobs": 20, "n_probs": n.max(20), "cache_prompt": true,
            "chat_template_kwargs": {"enable_thinking": false, "add_vision_id": true},
        });
        let r = http.post(format!("{base}/v1/chat/completions")).bearer_auth(key).json(&body).send().await.map_err(|e| anyhow!("Decision server not reachable: {e}"))?;
        let code = r.status();
        let v: Value = r.json().await.map_err(|e| anyhow!("Bad reply from the decision server: {e}"))?;
        if !code.is_success() {
            bail!("Decision server: {}", v["error"]["message"].as_str().or(v["error"].as_str()).unwrap_or("error"));
        }
        let lp: HashMap<String, f64> = v["choices"][0]["logprobs"]["content"][0]["top_logprobs"]
            .as_array()
            .map(|a| a.iter().filter_map(|t| Some((t["token"].as_str()?.to_string(), t["logprob"].as_f64()?))).collect())
            .unwrap_or_default();
        let symbols: Vec<String> = ANSWER_SYMBOLS.chars().take(n).map(String::from).collect();
        answers.insert(q.name.clone(), answer_json(q, &calibrated(&symbols, &lp)));
    }
    Ok(json!({"answers": answers, "model": "Intern-Decision-0.8B Q8_0", "timing": {"inference_ms": t0.elapsed().as_secs_f64() * 1000.0}}))
}

pub struct Decider {
    dir: PathBuf,
    app: AppHandle,
    child: Mutex<Option<tokio::process::Child>>,
    log: Mutex<VecDeque<String>>,
    setup_running: AtomicBool,
    ready: AtomicBool,
    info: Mutex<Value>,
    /// Latency of the last decision call, for the settings page.
    last_ms: Mutex<Option<f64>>,
    /// Random key for the server the app started, so other programs and web pages can't use it.
    key: Mutex<String>,
}

static GLOBAL: OnceLock<Arc<Decider>> = OnceLock::new();

pub fn global() -> Option<Arc<Decider>> {
    GLOBAL.get().cloned()
}

pub fn init(app: AppHandle, data_dir: &Path) -> Arc<Decider> {
    let d = Arc::new(Decider {
        dir: data_dir.join("decision"),
        app,
        child: Mutex::new(None),
        log: Mutex::new(VecDeque::new()),
        setup_running: AtomicBool::new(false),
        ready: AtomicBool::new(false),
        info: Mutex::new(json!({})),
        last_ms: Mutex::new(None),
        key: Mutex::new(String::new()),
    });
    let _ = GLOBAL.set(d.clone());
    d
}

fn venv_python(venv: &Path) -> PathBuf {
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// Port from a local URL like http://127.0.0.1:47615.
fn local_port(url: &str) -> Option<u16> {
    let u = reqwest::Url::parse(url).ok()?;
    matches!(u.host_str()?, "127.0.0.1" | "localhost" | "[::1]").then(|| u.port_or_known_default())?
}

fn boxed<R: AsyncRead + Unpin + Send + 'static>(r: R) -> Box<dyn AsyncRead + Unpin + Send> {
    Box::new(r)
}

fn sha256_file(p: &Path) -> Result<String> {
    let mut h = Sha256::new();
    std::io::copy(&mut std::fs::File::open(p)?, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

impl Decider {
    fn venv(&self) -> PathBuf {
        self.dir.join("venv")
    }
    fn model_dir(&self, cfg: &DecisionCfg) -> PathBuf {
        let name = cfg.model_repo.rsplit('/').next().unwrap_or("model");
        self.dir.join("models").join(name)
    }
    fn gguf_dir(&self) -> PathBuf {
        self.dir.join("gguf")
    }
    fn gguf_ok(&self) -> bool {
        [GGUF_MODEL, GGUF_MMPROJ].iter().all(|(f, size, _)| std::fs::metadata(self.gguf_dir().join(f)).map(|m| m.len() == *size).unwrap_or(false))
    }
    pub fn installed(&self, cfg: &DecisionCfg) -> bool {
        match cfg.backend.as_str() {
            "python" => venv_python(&self.venv()).exists() && self.model_dir(cfg).join("inference.py").exists(),
            _ => self.gguf_ok() && crate::llamacpp::global().and_then(|l| l.server_exe()).is_some(),
        }
    }
    fn running(&self) -> bool {
        let mut c = self.child.lock();
        match c.as_mut() {
            Some(ch) => match ch.try_wait() {
                Ok(None) => true,
                _ => {
                    *c = None;
                    self.ready.store(false, Ordering::SeqCst);
                    false
                }
            },
            None => false,
        }
    }

    fn push_log(&self, line: &str) {
        let line = line.trim_end().to_string();
        if line.is_empty() {
            return;
        }
        {
            let mut l = self.log.lock();
            l.push_back(line.clone());
            while l.len() > 200 {
                l.pop_front();
            }
        }
        let _ = self.app.emit("decision-log", line);
    }

    pub fn status(&self, s: &AppSettings) -> Value {
        let cfg = &s.decision;
        json!({
            "backend": cfg.backend,
            "installed": self.installed(cfg),
            "engine_installed": crate::llamacpp::global().and_then(|l| l.server_exe()).is_some(),
            "setup_running": self.setup_running.load(Ordering::SeqCst),
            "running": self.running(),
            "ready": self.ready.load(Ordering::SeqCst),
            "info": self.info.lock().clone(),
            "last_ms": *self.last_ms.lock(),
            "log": self.log.lock().iter().rev().take(40).rev().cloned().collect::<Vec<_>>(),
            "dir": self.dir,
            "local": local_port(&cfg.url).is_some(),
            "files": self.files(cfg),
            "model_dir": if cfg.backend == "python" { self.model_dir(cfg) } else { self.gguf_dir() },
        })
    }

    fn emit_status(&self, s: &AppSettings) {
        let _ = self.app.emit("decision-status", self.status(s));
    }

    /// Runs one setup command, streaming its output into the log.
    async fn run_logged(&self, program: &Path, args: &[&str]) -> Result<()> {
        self.push_log(&format!("> {} {}", program.file_name().and_then(|f| f.to_str()).unwrap_or("python"), args.join(" ")));
        let mut c = crate::sandbox::command(&program.to_string_lossy());
        c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        c.env("PYTHONUNBUFFERED", "1").env("PIP_DISABLE_PIP_VERSION_CHECK", "1");
        let mut ch = c.spawn().map_err(|e| anyhow!("Could not run {}: {e}", program.display()))?;
        let pump = |r: Option<Box<dyn AsyncRead + Unpin + Send>>| async move {
            if let Some(r) = r {
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    self.push_log(&l);
                }
            }
        };
        let (out, err) = (ch.stdout.take().map(boxed), ch.stderr.take().map(boxed));
        let (_, _, st) = tokio::join!(pump(out), pump(err), ch.wait());
        if !st?.success() {
            bail!("Step failed; see the log above");
        }
        Ok(())
    }

    /// A Python 3.12+ on this computer (the model's code needs it).
    async fn find_python(&self) -> Result<PathBuf> {
        let candidates: &[(&str, &[&str])] = if cfg!(windows) {
            &[("py", &["-3.13"]), ("py", &["-3.12"]), ("python", &[]), ("python3", &[])]
        } else {
            &[("python3.13", &[]), ("python3.12", &[]), ("python3", &[]), ("python", &[])]
        };
        for (prog, pre) in candidates {
            let mut c = crate::sandbox::command(prog);
            c.args(*pre).args(["-c", "import sys;print(sys.executable);print(sys.version_info>=(3,12))"]).stdin(Stdio::null());
            if let Ok(Ok(o)) = tokio::time::timeout(Duration::from_secs(15), c.output()).await {
                let s = String::from_utf8_lossy(&o.stdout);
                let mut lines = s.lines();
                if let (Some(exe), Some("True")) = (lines.next(), lines.next()) {
                    return Ok(PathBuf::from(exe.trim()));
                }
            }
        }
        bail!("Python 3.12 or newer is needed. Install it from python.org (tick \"Add to PATH\"), then try again.")
    }

    /// Installs what the chosen backend needs. `gpu` picks the CUDA build of PyTorch (Python only).
    pub async fn setup(self: Arc<Self>, s: AppSettings, gpu: bool) -> Result<()> {
        if self.setup_running.swap(true, Ordering::SeqCst) {
            bail!("Setup is already running");
        }
        self.emit_status(&s);
        let r = match s.decision.backend.as_str() {
            "python" => self.clone().setup_python(&s.decision, gpu).await,
            _ => self.clone().setup_gguf().await,
        };
        self.setup_running.store(false, Ordering::SeqCst);
        match &r {
            Ok(()) => self.push_log("Setup finished. Press Start."),
            Err(e) => self.push_log(&format!("Setup stopped: {e}")),
        }
        self.emit_status(&s);
        r
    }

    async fn setup_gguf(self: Arc<Self>) -> Result<()> {
        use futures_util::StreamExt;
        if crate::llamacpp::global().and_then(|l| l.server_exe()).is_none() {
            bail!("The built-in engine is needed. Open Models → Built-in engine and download it first.");
        }
        let dir = self.gguf_dir();
        std::fs::create_dir_all(&dir)?;
        let http = reqwest::Client::new();
        for (file, size, sha) in [GGUF_MODEL, GGUF_MMPROJ] {
            let dest = dir.join(file);
            if std::fs::metadata(&dest).map(|m| m.len() == size).unwrap_or(false) && sha256_file(&dest)? == sha {
                self.push_log(&format!("{file} is already here and verified."));
                continue;
            }
            self.push_log(&format!("Downloading {file} ({} MB)…", size / 1_000_000));
            let url = format!("https://huggingface.co/{GGUF_REPO}/resolve/{GGUF_REVISION}/{file}");
            let resp = http.get(url).timeout(Duration::from_secs(3600)).send().await?;
            if !resp.status().is_success() {
                bail!("Download failed: {}", resp.status());
            }
            let part = dir.join(format!("{file}.part"));
            let mut f = tokio::fs::File::create(&part).await?;
            let (mut got, mut next) = (0u64, 10u64);
            let mut shown = std::time::Instant::now();
            let mut stream = resp.bytes_stream();
            while let Some(c) = stream.next().await {
                let c = c?;
                got += c.len() as u64;
                if got > size {
                    bail!("{file} is larger than expected; download refused");
                }
                tokio::io::AsyncWriteExt::write_all(&mut f, &c).await?;
                if got * 100 / size >= next {
                    self.push_log(&format!("  {file}: {next}%"));
                    next += 10;
                }
                if shown.elapsed() > Duration::from_millis(200) || got == size {
                    shown = std::time::Instant::now();
                    let _ = self.app.emit("decision-progress", json!({"file": file, "done": got, "total": size}));
                }
            }
            tokio::io::AsyncWriteExt::flush(&mut f).await?;
            drop(f);
            if sha256_file(&part)? != sha {
                let _ = std::fs::remove_file(&part);
                bail!("{file} failed its integrity check and was deleted");
            }
            std::fs::rename(&part, &dest)?;
            self.push_log(&format!("{file} verified."));
        }
        Ok(())
    }

    async fn setup_python(self: Arc<Self>, cfg: &DecisionCfg, gpu: bool) -> Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let py = venv_python(&self.venv());
        if !py.exists() {
            let base = self.find_python().await?;
            self.push_log(&format!("Using {}", base.display()));
            self.run_logged(&base, &["-m", "venv", &self.venv().to_string_lossy()]).await?;
        }
        let index = if gpu { "https://download.pytorch.org/whl/cu126" } else { "https://download.pytorch.org/whl/cpu" };
        self.push_log(if gpu { "Installing PyTorch with CUDA (about 2.5 GB)…" } else { "Installing PyTorch for CPU (about 250 MB)…" });
        let mut a = vec!["-m", "pip", "install", "--progress-bar", "off"];
        a.extend(TORCH);
        a.extend(["--index-url", index]);
        self.run_logged(&py, &a).await?;
        self.push_log("Installing Transformers…");
        let mut a = vec!["-m", "pip", "install", "--progress-bar", "off"];
        a.extend(LIBS);
        self.run_logged(&py, &a).await?;
        let dest = self.model_dir(cfg);
        let rev = if cfg.model_repo == DEFAULT_REPO { DEFAULT_REVISION } else { "main" };
        self.push_log(&format!("Downloading {} ({})…", cfg.model_repo, &rev[..rev.len().min(7)]));
        let code = format!(
            "from huggingface_hub import snapshot_download as s; print(s({}, revision={}, local_dir={}))",
            serde_json::to_string(&cfg.model_repo)?,
            serde_json::to_string(rev)?,
            serde_json::to_string(&dest.to_string_lossy())?
        );
        self.run_logged(&py, &["-c", &code]).await?;
        Ok(())
    }

    /// Starts the local server and returns once it answers (the model is loaded).
    pub async fn start(self: Arc<Self>, s: &AppSettings) -> Result<()> {
        let cfg = &s.decision;
        if self.running() {
            return Ok(());
        }
        if !self.installed(cfg) {
            bail!("Set up the decision model first");
        }
        let port = local_port(&cfg.url).ok_or_else(|| anyhow!("The app only starts a server for a local address such as http://127.0.0.1:47615"))?;
        let gguf = cfg.backend != "python";
        let key = crate::db::new_id().replace('-', "");
        *self.key.lock() = key.clone();
        let mut c = if gguf {
            let (exe, gpu_build) = crate::llamacpp::global().and_then(|l| l.server_exe()).ok_or_else(|| anyhow!("The built-in engine is not installed"))?;
            let d = self.gguf_dir();
            let mut c = crate::sandbox::command(&exe.to_string_lossy());
            c.args(["-m", &d.join(GGUF_MODEL.0).to_string_lossy(), "--mmproj", &d.join(GGUF_MMPROJ.0).to_string_lossy()])
                .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-c", "8192", "--parallel", "1", "--no-webui", "-a", "intern-decision", "--api-key", &key]);
            if cfg.device == "cpu" || !gpu_build {
                c.args(["-ngl", "0", "--no-mmproj-offload"]);
            } else {
                c.args(["-ngl", "999"]);
                // Same GPU choice as the built-in engine, when one is set.
                if s.llama.devices != "auto" && s.llama.devices != "none" {
                    c.args(["-dev", &s.llama.devices]);
                }
            }
            if let Some(p) = exe.parent() {
                c.current_dir(p);
            }
            c
        } else {
            let script = self.dir.join("decision_server.py");
            std::fs::write(&script, SERVER_PY)?;
            let mut c = crate::sandbox::command(&venv_python(&self.venv()).to_string_lossy());
            c.arg(&script)
                .args(["--model", &self.model_dir(cfg).to_string_lossy(), "--port", &port.to_string(), "--device", &cfg.device])
                .env("PYTHONUNBUFFERED", "1")
                .env("HF_HUB_OFFLINE", "1")
                .env("DECISION_TOKEN", &key);
            c
        };
        c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut ch = c.spawn().map_err(|e| anyhow!("Could not start the decision server: {e}"))?;
        self.ready.store(false, Ordering::SeqCst);
        *self.info.lock() = json!({});
        self.push_log(if gguf { "Starting the decision model on the built-in engine…" } else { "Starting the Python decision server…" });
        for r in [ch.stdout.take().map(boxed), ch.stderr.take().map(boxed)].into_iter().flatten() {
            let me = self.clone();
            tauri::async_runtime::spawn(async move {
                let mut lines = BufReader::new(r).lines();
                while let Ok(Some(l)) = lines.next_line().await {
                    // llama-server is chatty; keep warnings, errors and milestones.
                    let keep = !gguf || [" E ", " W ", "error", "listening", "model loaded", "Vulkan", "CUDA", "offloaded"].iter().any(|k| l.contains(k));
                    // Noise for this use: grounding hints and the API-key notice (a key is set).
                    if keep && !["non-consecutive token position", "load_hparams", "no API key"].iter().any(|k| l.contains(k)) {
                        me.push_log(&l);
                    }
                }
            });
        }
        *self.child.lock() = Some(ch);
        // Ready when /health answers. Loading takes a while on first start, longer on CPU.
        let http = reqwest::Client::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
        let health = format!("http://127.0.0.1:{port}/health");
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            if !self.running() {
                self.emit_status(s);
                bail!("The decision server stopped while loading; see its log");
            }
            if let Ok(r) = http.get(&health).bearer_auth(&key).timeout(Duration::from_secs(2)).send().await {
                if r.status().is_success() {
                    let v: Value = r.json().await.unwrap_or_default();
                    // The Python server reports ok=false until its model is loaded.
                    if v.get("ok").map(|x| x.as_bool() == Some(true)).unwrap_or(true) {
                        *self.info.lock() = if gguf {
                            json!({"model": "Intern-Decision-0.8B Q8_0", "device": if cfg.device == "cpu" { "cpu" } else { "gpu" }, "dtype": "q8_0"})
                        } else {
                            json!({"model": v["model"], "device": v["device"], "dtype": v["dtype"]})
                        };
                        break;
                    }
                }
            }
            if std::time::Instant::now() > deadline {
                self.stop();
                bail!("The decision model did not load within 5 minutes");
            }
        }
        if gguf {
            // Warm up so the first real decision is fast.
            let _ = predict_gguf(&http, &format!("http://127.0.0.1:{port}"), &key, &json!("warm-up"), &[Q::yes_no("ok", "Is this a warm-up?")], None).await;
        }
        self.ready.store(true, Ordering::SeqCst);
        self.push_log("Ready.");
        self.emit_status(s);
        Ok(())
    }

    /// The key for requests: only for the server this app started at a local address.
    fn key_for(&self, cfg: &DecisionCfg) -> String {
        if local_port(&cfg.url).is_some() && self.running() {
            self.key.lock().clone()
        } else {
            String::new()
        }
    }

    /// The model files of the chosen backend: name, expected size and whether they are here.
    fn files(&self, cfg: &DecisionCfg) -> Vec<Value> {
        if cfg.backend == "python" {
            let ok = self.model_dir(cfg).join("inference.py").exists();
            return vec![json!({"name": cfg.model_repo, "size": 1_734_731_371u64, "present": ok, "source": format!("https://huggingface.co/{}", cfg.model_repo)})];
        }
        [GGUF_MODEL, GGUF_MMPROJ]
            .iter()
            .map(|(f, size, _)| {
                let have = std::fs::metadata(self.gguf_dir().join(f)).map(|m| m.len()).unwrap_or(0);
                json!({"name": f, "size": size, "present": have == *size, "source": format!("https://huggingface.co/{GGUF_REPO}/blob/{GGUF_REVISION}/{f}")})
            })
            .collect()
    }

    /// Removes the downloaded model of the chosen backend (and the Python environment for it).
    pub fn delete_model(&self, s: &AppSettings) -> Result<()> {
        if self.setup_running.load(Ordering::SeqCst) {
            bail!("Wait for the download to finish first");
        }
        self.stop();
        if s.decision.backend == "python" {
            for d in [self.model_dir(&s.decision), self.venv()] {
                if d.exists() {
                    std::fs::remove_dir_all(&d)?;
                }
            }
        } else if self.gguf_dir().exists() {
            std::fs::remove_dir_all(self.gguf_dir())?;
        }
        self.push_log("Model removed from this computer.");
        self.emit_status(s);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut ch) = self.child.lock().take() {
            let _ = ch.start_kill();
        }
        self.ready.store(false, Ordering::SeqCst);
    }

    /// False while the app's own local server is not up yet; a remote server is always tried.
    pub fn usable(&self, cfg: &DecisionCfg) -> bool {
        local_port(&cfg.url).is_none() || !self.installed(cfg) || self.ready.load(Ordering::SeqCst)
    }

    pub fn note_latency(&self, ms: f64) {
        *self.last_ms.lock() = Some(ms);
    }
}

/// Starts the local server in the background when the agent will use it and it is not up yet.
pub fn ensure_started(s: &AppSettings) {
    let cfg = &s.decision;
    let Some(d) = global() else { return };
    if !cfg.enabled || !cfg.autostart || local_port(&cfg.url).is_none() || !d.installed(cfg) || d.running() {
        return;
    }
    let s = s.clone();
    tauri::async_runtime::spawn(async move {
        let _ = d.start(&s).await;
    });
}

/// Asks the decision model; `image` is a JPEG in base64. The reply has the reference engine's
/// shape: {"answers": {name: {decision, confidence, probabilities, noul?}}, ...}.
pub async fn predict(http: &reqwest::Client, cfg: &DecisionCfg, state: Value, qs: &[Q], image: Option<String>, timeout: Duration) -> Result<Value> {
    let base = cfg.url.trim_end_matches('/').to_string();
    let key = global().map(|d| d.key_for(cfg)).unwrap_or_default();
    let t0 = std::time::Instant::now();
    let fut = async {
        if cfg.backend == "python" {
            let questions: serde_json::Map<String, Value> = qs.iter().map(|q| (q.name.clone(), q.to_json())).collect();
            let body = json!({"state": state, "questions": questions, "images_b64": image.into_iter().collect::<Vec<_>>()});
            let r = http.post(format!("{base}/predict")).bearer_auth(&key).json(&body).send().await.map_err(|e| anyhow!("Decision server not reachable: {e}"))?;
            let code = r.status();
            let v: Value = r.json().await.map_err(|e| anyhow!("Bad reply from the decision server: {e}"))?;
            if !code.is_success() {
                bail!("Decision server: {}", v["error"].as_str().unwrap_or("error"));
            }
            Ok(v)
        } else {
            predict_gguf(http, &base, &key, &state, qs, image).await
        }
    };
    let v = tokio::time::timeout(timeout, fut).await.map_err(|_| anyhow!("The fast check took longer than {} ms", timeout.as_millis()))??;
    if let Some(d) = global() {
        d.note_latency(t0.elapsed().as_secs_f64() * 1000.0);
    }
    Ok(v)
}

/// One answer: its decision and calibrated confidence; for yes/no, the probability of yes.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub decision: String,
    pub confidence: f64,
    pub yes: Option<f64>,
}

pub fn answers(v: &Value) -> Vec<(String, Answer)> {
    v["answers"]
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(k, a)| {
                    (
                        k.clone(),
                        Answer { decision: a["decision"].as_str().unwrap_or("").to_string(), confidence: a["confidence"].as_f64().unwrap_or(0.0), yes: a["noul"].as_f64() },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The per-step questions about the screen, limited to the checks that are switched on.
pub fn step_questions(cfg: &DecisionCfg, last_action: Option<&str>, plan: &[String]) -> Vec<Q> {
    let mut q = vec![];
    if cfg.check_loading {
        q.push(Q::yes_no("loading", "Is the screen still loading or changing: a spinner, a progress bar, or a blank or half-drawn page?"));
    }
    if cfg.check_result {
        if let Some(a) = last_action {
            q.push(Q::choice(
                "last_action",
                &format!("What happened after the last action ({a})?"),
                &[
                    ("worked", "It had its intended effect."),
                    ("no_effect", "Nothing visible changed."),
                    ("error", "An error message or warning is shown."),
                    ("popup", "A dialog, pop-up or menu appeared that must be handled first."),
                ],
            ));
        }
    }
    if cfg.check_done {
        q.push(Q::yes_no("goal_done", "Is the goal fully achieved and visible on this screen?"));
    }
    if cfg.check_plan && plan.len() >= 2 {
        let opts: Vec<(String, String)> = plan.iter().enumerate().take(60).map(|(i, s)| ((i + 1).to_string(), s.clone())).collect();
        q.push(Q { name: "plan_step".into(), instructions: "Which plan step should be worked on now?".into(), options: Some(opts) });
    }
    if cfg.check_signin {
        q.push(Q::yes_no("sign_in", "Does the screen ask for a password, a sign-in, a CAPTCHA, a verification code or payment card details?"));
    }
    q
}

/// Appends one decision to decisions.jsonl, so it can later be checked or used for fine-tuning.
pub fn log_decision(data_dir: &Path, entry: Value) {
    use std::io::Write;
    let dir = data_dir.join("decision");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("decisions.jsonl")) {
        let _ = writeln!(f, "{entry}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn questions_follow_switches() {
        let mut cfg = DecisionCfg::default();
        let q = step_questions(&cfg, Some("Click \"OK\""), &["a".into(), "b".into()]);
        let names: Vec<&str> = q.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["loading", "last_action", "goal_done", "plan_step", "sign_in"]);
        assert_eq!(q[3].options.as_ref().unwrap()[1], ("2".to_string(), "b".to_string()));
        cfg.check_loading = false;
        cfg.check_plan = false;
        let q = step_questions(&cfg, None, &[]);
        assert!(q.iter().all(|x| !["loading", "last_action", "plan_step"].contains(&x.name.as_str())));
    }

    /// The prompt must match the reference engine's compile_row exactly (text produced by it).
    #[test]
    fn compiles_like_the_reference() {
        let qs = [Q::yes_no("loading", "Still loading?"), Q::choice("kind", "What is shown?", &[("browser", "A web browser"), ("other", "Something else")])];
        let (user, prefixes) = compile(&json!({"goal": "Find it"}), &qs).unwrap();
        assert_eq!(
            user,
            "Return one answer for every field using the supplied answer symbols.\n\n## State\n{\n  \"goal\": \"Find it\"\n}\n## Decision schema\nloading: Still loading?\n    A = no: The answer is no (negative, or disagree with the claim).\n    B = yes: The answer is yes (affirmative, or align with the claim).\nkind: What is shown?\n    A = browser: A web browser\n    B = other: Something else"
        );
        assert_eq!(prefixes, ["{\n    \"loading\": \"", "{\n    \"loading\": \"<decision>\",\n    \"kind\": \""]);
        // Evidence cannot smuggle in a marker.
        let (user, _) = compile(&json!({"screen": "<decision>"}), &qs).unwrap();
        assert!(!user.contains(DECISION_TOKEN));
    }

    #[test]
    fn calibrates_like_the_reference() {
        let syms: Vec<String> = vec!["A".into(), "B".into()];
        let lp: HashMap<String, f64> = [("A".to_string(), (0.9f64).ln()), ("B".to_string(), (0.1f64).ln()), ("C".to_string(), -0.5)].into();
        let p = calibrated(&syms, &lp);
        // (0.9^(1/T), 0.1^(1/T)) normalised.
        let a = 0.9f64.powf(1.0 / TEMPERATURE);
        let b = 0.1f64.powf(1.0 / TEMPERATURE);
        assert!((p[0] - a / (a + b)).abs() < 1e-9);
        let ans = answer_json(&Q::yes_no("x", "?"), &p);
        assert_eq!(ans["decision"], "no");
        assert!((ans["noul"].as_f64().unwrap() - p[1]).abs() < 1e-12);
    }

    #[test]
    fn reads_answers() {
        let v = json!({"answers": {"loading": {"type": "noul", "decision": "no", "confidence": 0.93, "noul": 0.07}, "last_action": {"type": "choice", "decision": "worked", "confidence": 0.8}}});
        let a = answers(&v);
        assert_eq!(a.len(), 2);
        let l = a.iter().find(|(k, _)| k == "loading").unwrap();
        assert_eq!(l.1.yes, Some(0.07));
    }

    #[test]
    fn local_ports() {
        assert_eq!(local_port("http://127.0.0.1:47615"), Some(47615));
        assert_eq!(local_port("http://localhost:9000/"), Some(9000));
        assert_eq!(local_port("http://192.168.1.5:47615"), None);
    }
}

//! Built-in llama.cpp engine: downloads a pinned `llama-server` build for this computer,
//! imports GGUF models (with optional vision projectors) where they are, and runs one model
//! at a time on a local port. Models appear as an ordinary local OpenAI-compatible provider;
//! the server is started or switched on demand and unloaded when idle.

use crate::db::{now, Db};
use crate::settings::{self, LlamaModel};
use anyhow::{anyhow, bail, Context, Result};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// Pinned release; every file is checked against the SHA-256 GitHub publishes for it.
pub const TAG: &str = "b11685";

struct Asset {
    name: &'static str,
    size: u64,
    sha256: &'static str,
}

pub struct Variant {
    pub id: &'static str,
    pub label: &'static str,
    pub about: &'static str,
    os: &'static str,
    arch: &'static str,
    gpu: bool,
    assets: &'static [Asset],
}

const fn a(name: &'static str, size: u64, sha256: &'static str) -> Asset {
    Asset { name, size, sha256 }
}

pub const VARIANTS: &[Variant] = &[
    Variant { id: "win-vulkan-x64", label: "GPU (Vulkan)", about: "Works with NVIDIA, AMD and Intel graphics. Recommended.", os: "windows", arch: "x86_64", gpu: true,
        assets: &[a("llama-b11685-bin-win-vulkan-x64.zip", 32_209_499, "f08862ea8be9e3487f2012ba9c2ce9c2ad99f358332a546e156a0076d4f09dda")] },
    Variant { id: "win-cpu-x64", label: "CPU only", about: "No graphics card needed. Slower for large models.", os: "windows", arch: "x86_64", gpu: false,
        assets: &[a("llama-b11685-bin-win-cpu-x64.zip", 19_565_071, "3ac857118b17b5403feca3aaa9e02d3048b51e9d00630dccd965b224a474b4ec")] },
    Variant { id: "win-cuda-12.4-x64", label: "NVIDIA CUDA 12", about: "Fastest on NVIDIA cards. About 660 MB including the CUDA runtime.", os: "windows", arch: "x86_64", gpu: true,
        assets: &[
            a("llama-b11685-bin-win-cuda-12.4-x64.zip", 266_609_273, "f2f502e5486ae9924a400603efdd1be9fb69f7ea29b697012386e6e5ac9b8f71"),
            a("cudart-llama-bin-win-cuda-12.4-x64.zip", 391_443_627, "8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6"),
        ] },
    Variant { id: "win-vulkan-arm64", label: "GPU (Vulkan)", about: "For Windows on ARM.", os: "windows", arch: "aarch64", gpu: true,
        assets: &[a("llama-b11685-bin-win-vulkan-arm64.zip", 24_844_724, "3008f83e4b23d559978b00c549971dd7bbc0b467c1b042bd49baa0ae5b17e760")] },
    Variant { id: "win-cpu-arm64", label: "CPU only", about: "For Windows on ARM.", os: "windows", arch: "aarch64", gpu: false,
        assets: &[a("llama-b11685-bin-win-cpu-arm64.zip", 12_465_743, "f754135e62f8be0ed8a04a702fe6aef9a1b74528743f922c136e1e18ca0b67e6")] },
    Variant { id: "macos-arm64", label: "Apple Silicon (Metal)", about: "Uses the Mac's GPU.", os: "macos", arch: "aarch64", gpu: true,
        assets: &[a("llama-b11685-bin-macos-arm64.tar.gz", 12_220_320, "3b664d387679c57eb91ffcf00daea95bc2f7b45bde4bba36019ebccff12c15bd")] },
    Variant { id: "macos-x64", label: "Intel Mac", about: "CPU build for Intel Macs.", os: "macos", arch: "x86_64", gpu: false,
        assets: &[a("llama-b11685-bin-macos-x64.tar.gz", 11_703_159, "b90c51686ad01c8b92b5fa0bf5e3e514e751e03c14d5b3b8fa7aa690228f6fbc")] },
    Variant { id: "linux-vulkan-x64", label: "GPU (Vulkan)", about: "NVIDIA, AMD and Intel graphics with Vulkan drivers.", os: "linux", arch: "x86_64", gpu: true,
        assets: &[a("llama-b11685-bin-ubuntu-vulkan-x64.tar.gz", 30_522_716, "7332c4f3dab034f271898509415d0b72418a5c2f1c7e0ffa33077da69ebdb20f")] },
    Variant { id: "linux-x64", label: "CPU only", about: "No graphics card needed.", os: "linux", arch: "x86_64", gpu: false,
        assets: &[a("llama-b11685-bin-ubuntu-x64.tar.gz", 17_880_818, "d385a4277c9353cdf540361318a74cb3cbc7eec7e2d4b417e5da83f20730edeb")] },
    Variant { id: "linux-vulkan-arm64", label: "GPU (Vulkan)", about: "ARM64 with Vulkan drivers.", os: "linux", arch: "aarch64", gpu: true,
        assets: &[a("llama-b11685-bin-ubuntu-vulkan-arm64.tar.gz", 24_223_811, "a4a01c69d3acf1827ec25de8262d305d0071c3e4be6f3e18908611a533cf6d43")] },
    Variant { id: "linux-arm64", label: "CPU only", about: "ARM64.", os: "linux", arch: "aarch64", gpu: false,
        assets: &[a("llama-b11685-bin-ubuntu-arm64.tar.gz", 13_935_068, "0d0f77de9c902b1382aeb133835feddfc9680334df28dfcf8424f694e1d1bec2")] },
];

fn release_url(asset: &str) -> String {
    format!("https://github.com/ggml-org/llama.cpp/releases/download/{TAG}/{asset}")
}

impl Variant {
    pub fn assets_size(&self) -> u64 {
        self.assets.iter().map(|a| a.size).sum()
    }
}

pub fn variants_here() -> Vec<&'static Variant> {
    VARIANTS.iter().filter(|v| v.os == std::env::consts::OS && v.arch == std::env::consts::ARCH).collect()
}

pub fn variant(id: &str) -> Option<&'static Variant> {
    VARIANTS.iter().find(|v| v.id == id)
}

// ---------------- GGUF header ----------------

#[derive(Serialize, Clone, Debug, Default)]
pub struct GgufInfo {
    pub arch: String,
    pub name: String,
    pub context_length: u64,
    pub is_projector: bool,
    /// Sentence-embedding model (has a pooling type or an encoder architecture).
    pub is_embedding: bool,
    pub size: u64,
}

const EMBED_ARCHS: &[&str] = &["bert", "nomic-bert", "nomic-bert-moe", "jina-bert-v2", "jina-bert-v3", "modern-bert", "neo-bert", "xlm-roberta", "t5encoder", "gemma-embedding"];

// ---------------- Embedding model catalogue ----------------

pub struct EmbedPick {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub repo: &'static str,
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// Small embedding models for document search, pinned by SHA-256.
pub const EMBED_MODELS: &[EmbedPick] = &[
    EmbedPick { id: "nomic-embed-text-v1.5", label: "Nomic Embed Text v1.5", note: "Recommended. English, fast, 8K context", repo: "nomic-ai/nomic-embed-text-v1.5-GGUF", file: "nomic-embed-text-v1.5.Q8_0.gguf", size: 146146432, sha256: "3e24342164b3d94991ba9692fdc0dd08e3fd7362e0aacc396a9a5c54a544c3b7" },
    EmbedPick { id: "bge-small-en-v1.5", label: "BGE Small EN v1.5", note: "Smallest. English, 512-token chunks", repo: "CompendiumLabs/bge-small-en-v1.5-gguf", file: "bge-small-en-v1.5-q8_0.gguf", size: 36806944, sha256: "ec38e8da142596baa913124ae50550de284b6916bf59577ef2f0cb9660c2f514" },
    EmbedPick { id: "embeddinggemma-300m", label: "EmbeddingGemma 300M", note: "Multilingual, 100+ languages", repo: "ggml-org/embeddinggemma-300M-GGUF", file: "embeddinggemma-300M-Q8_0.gguf", size: 333590944, sha256: "b5ce9d77a3fc4b3b39ccb5643c36777911cc4eb46a66962eadfa3f5f60490d63" },
    EmbedPick { id: "qwen3-embedding-0.6b", label: "Qwen3 Embedding 0.6B", note: "Best quality, multilingual, larger", repo: "Qwen/Qwen3-Embedding-0.6B-GGUF", file: "Qwen3-Embedding-0.6B-Q8_0.gguf", size: 639150592, sha256: "06507c7b42688469c4e7298b0a1e16deff06caf291cf0a5b278c308249c3e439" },
];

/// A GPU or other device llama.cpp can offload to, as reported by `--list-devices`.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub total_mb: u64,
    pub free_mb: u64,
    /// Integrated GPUs share system memory; dedicated ones have their own.
    pub integrated: bool,
}

/// Parses `llama-server --list-devices` lines such as
/// `  Vulkan1: NVIDIA GeForce GTX 1650 (4149 MiB, 3558 MiB free)`.
pub fn parse_devices(out: &str) -> Vec<Device> {
    out.lines()
        .filter_map(|l| {
            let l = l.trim();
            let (id, rest) = l.split_once(": ")?;
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || !id.chars().any(|c| c.is_ascii_digit()) {
                return None;
            }
            let (name, mem) = match rest.rfind(" (") {
                Some(i) => (rest[..i].trim(), &rest[i + 2..]),
                None => (rest.trim(), ""),
            };
            let nums: Vec<u64> = mem.split(|c: char| !c.is_ascii_digit()).filter(|x| !x.is_empty()).filter_map(|x| x.parse().ok()).collect();
            let lower = name.to_lowercase();
            let integrated = (lower.contains("radeon(tm) graphics") || lower.contains("uhd graphics") || lower.contains("iris") || lower.contains("intel(r) graphics") || lower.contains("vega") && lower.contains("graphics"))
                && !lower.contains(" rx ");
            Some(Device { id: id.to_string(), name: name.to_string(), total_mb: nums.first().copied().unwrap_or(0), free_mb: nums.get(1).copied().unwrap_or(0), integrated })
        })
        .collect()
}

/// Performance and placement flags from the engine settings.
pub fn tuning_args(cfg: &settings::LlamaCfg, gpu_build: bool) -> Vec<String> {
    let mut a: Vec<String> = vec![];
    let mut push = |k: &str, v: String| {
        a.push(k.to_string());
        a.push(v);
    };
    let cpu_only = cfg.devices == "none" || !gpu_build;
    if cfg.devices != "auto" && gpu_build {
        push("-dev", cfg.devices.clone());
    }
    let ngl = if cpu_only { 0 } else if cfg.gpu_layers >= 0 { cfg.gpu_layers } else { 999 };
    push("-ngl", ngl.to_string());
    if !cpu_only {
        if cfg.split_mode != "layer" {
            push("-sm", cfg.split_mode.clone());
        }
        if !cfg.tensor_split.trim().is_empty() {
            push("-ts", cfg.tensor_split.replace(' ', ""));
        }
        if cfg.main_gpu > 0 {
            push("-mg", cfg.main_gpu.to_string());
        }
    }
    if cfg.flash_attn != "auto" {
        push("-fa", cfg.flash_attn.clone());
    }
    if cfg.threads > 0 {
        push("-t", cfg.threads.to_string());
    }
    if cfg.batch > 0 {
        push("-b", cfg.batch.to_string());
    }
    if cfg.ubatch > 0 {
        push("-ub", cfg.ubatch.to_string());
    }
    if cfg.cache_k != "f16" {
        push("-ctk", cfg.cache_k.clone());
    }
    if cfg.cache_v != "f16" {
        push("-ctv", cfg.cache_v.clone());
    }
    if cfg.parallel > 0 {
        push("-np", cfg.parallel.to_string());
    }
    if cfg.cpu_moe > 0 {
        push("--n-cpu-moe", cfg.cpu_moe.to_string());
    }
    if let Some(t) = cfg.temperature {
        push("--temp", t.to_string());
    }
    if let Some(t) = cfg.top_k {
        push("--top-k", t.to_string());
    }
    if let Some(t) = cfg.top_p {
        push("--top-p", t.to_string());
    }
    if let Some(t) = cfg.min_p {
        push("--min-p", t.to_string());
    }
    if let Some(t) = cfg.repeat_penalty {
        push("--repeat-penalty", t.to_string());
    }
    if cfg.mlock {
        a.push("--mlock".into());
    }
    if cfg.no_mmap {
        a.push("--no-mmap".into());
    }
    a
}

/// Reads the GGUF header (key/value metadata only). Rejects files that are not GGUF.
pub fn gguf_info(path: &Path) -> Result<GgufInfo> {
    let f = std::fs::File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
    let size = f.metadata()?.len();
    let mut r = std::io::BufReader::with_capacity(1 << 16, f);
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|_| anyhow!("The file is too small to be a GGUF model"))?;
    if &magic != b"GGUF" {
        bail!("{} is not a GGUF file", path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    }
    let u32_ = |r: &mut dyn Read| -> Result<u32> {
        let mut b = [0u8; 4];
        r.read_exact(&mut b)?;
        Ok(u32::from_le_bytes(b))
    };
    let u64_ = |r: &mut dyn Read| -> Result<u64> {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        Ok(u64::from_le_bytes(b))
    };
    let version = u32_(&mut r)?;
    if !(2..=3).contains(&version) {
        bail!("Unsupported GGUF version {version}");
    }
    let _tensors = u64_(&mut r)?;
    let kv = u64_(&mut r)?;
    if kv > 100_000 {
        bail!("The GGUF header looks damaged");
    }
    let mut info = GgufInfo { size, ..Default::default() };
    let read_str = |r: &mut std::io::BufReader<std::fs::File>| -> Result<String> {
        let mut b = [0u8; 8];
        r.read_exact(&mut b)?;
        let n = u64::from_le_bytes(b);
        if n > 1 << 20 {
            bail!("The GGUF header looks damaged");
        }
        let mut s = vec![0u8; n as usize];
        r.read_exact(&mut s)?;
        Ok(String::from_utf8_lossy(&s).to_string())
    };
    fn skip(r: &mut std::io::BufReader<std::fs::File>, ty: u32) -> Result<Option<u64>> {
        let fixed = match ty {
            0 | 1 | 7 => 1,
            2 | 3 => 2,
            4 | 5 | 6 => 4,
            10 | 11 | 12 => 8,
            _ => 0,
        };
        if fixed > 0 {
            let mut b = vec![0u8; fixed];
            r.read_exact(&mut b)?;
            let mut v = [0u8; 8];
            v[..fixed].copy_from_slice(&b);
            return Ok(Some(u64::from_le_bytes(v)));
        }
        match ty {
            8 => {
                let mut b = [0u8; 8];
                r.read_exact(&mut b)?;
                r.seek_relative(u64::from_le_bytes(b) as i64)?;
            }
            9 => {
                let mut t = [0u8; 4];
                r.read_exact(&mut t)?;
                let inner = u32::from_le_bytes(t);
                let mut n = [0u8; 8];
                r.read_exact(&mut n)?;
                let n = u64::from_le_bytes(n);
                let width = match inner {
                    0 | 1 | 7 => 1,
                    2 | 3 => 2,
                    4 | 5 | 6 => 4,
                    10 | 11 | 12 => 8,
                    _ => 0,
                };
                if width > 0 {
                    r.seek_relative((n * width) as i64)?;
                } else {
                    for _ in 0..n {
                        skip(r, inner)?;
                    }
                }
            }
            _ => bail!("The GGUF header looks damaged"),
        }
        Ok(None)
    }
    for _ in 0..kv {
        let key = read_str(&mut r)?;
        let ty = u32_(&mut r)?;
        if ty == 8 && (key == "general.architecture" || key == "general.name" || key == "general.type") {
            let v = read_str(&mut r)?;
            match key.as_str() {
                "general.architecture" => info.arch = v,
                "general.name" => info.name = v,
                _ => info.is_projector |= v == "mmproj",
            }
            continue;
        }
        let num = skip(&mut r, ty)?;
        if key.ends_with(".context_length") && !key.starts_with("clip.") {
            info.context_length = num.unwrap_or(0);
        }
        if key.starts_with("clip.has_vision_encoder") || key.starts_with("clip.vision.") || key.starts_with("clip.projector_type") {
            info.is_projector = true;
        }
        if key.ends_with(".pooling_type") {
            info.is_embedding = true;
        }
    }
    if info.arch == "clip" {
        info.is_projector = true;
    }
    if EMBED_ARCHS.contains(&info.arch.as_str()) || crate::models::looks_like_embedding(&info.name) || crate::models::looks_like_embedding(&path.to_string_lossy()) {
        info.is_embedding = true;
    }
    let _ = r.seek(SeekFrom::Start(0));
    Ok(info)
}

// ---------------- Engine ----------------

#[derive(Serialize, Clone)]
pub struct Status {
    pub installed: bool,
    pub variant: String,
    pub tag: String,
    pub installing: bool,
    /// "stopped" | "loading" | "ready" | "error"
    pub state: String,
    pub current: Option<String>,
    pub port: u16,
    pub error: String,
    /// Embedding server: the loaded embedding model, if any.
    pub embed_current: Option<String>,
    pub embed_state: String,
}

struct Running {
    child: tokio::process::Child,
    alias: String,
    port: u16,
}

#[derive(Default)]
pub struct Llama {
    app: OnceLock<AppHandle>,
    db: OnceLock<Arc<Db>>,
    http: OnceLock<reqwest::Client>,
    dir: OnceLock<PathBuf>,
    running: Mutex<Option<Running>>,
    switch: tokio::sync::Mutex<()>,
    state: Mutex<String>,
    error: Mutex<String>,
    installing: AtomicBool,
    last_used: AtomicU64,
    devices: Mutex<Option<Vec<Device>>>,
    embed: Mutex<Option<Running>>,
    embed_switch: tokio::sync::Mutex<()>,
    embed_state: Mutex<String>,
    embed_used: AtomicU64,
    fetching: AtomicBool,
}

static GLOBAL: OnceLock<Arc<Llama>> = OnceLock::new();

pub fn global() -> Option<Arc<Llama>> {
    GLOBAL.get().cloned()
}

pub fn init(app: AppHandle, db: Arc<Db>, http: reqwest::Client, data_dir: &Path) -> Arc<Llama> {
    let l = Arc::new(Llama::default());
    let _ = l.app.set(app);
    let _ = l.db.set(db);
    let _ = l.http.set(http);
    let _ = l.dir.set(data_dir.join("llamacpp"));
    *l.state.lock() = "stopped".into();
    *l.embed_state.lock() = "stopped".into();
    let _ = GLOBAL.set(l.clone());
    l
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "llama-server.exe" } else { "llama-server" }
}

fn find_exe(dir: &Path) -> Option<PathBuf> {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).ok()?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().map(|n| n == exe_name()).unwrap_or(false) {
                return Some(p);
            }
        }
    }
    None
}

fn sha256_file(p: &Path) -> Result<String> {
    let mut h = Sha256::new();
    std::io::copy(&mut std::fs::File::open(p)?, &mut h)?;
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn extract(archive: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    let name = archive.to_string_lossy().to_lowercase();
    if name.ends_with(".zip") {
        let mut z = zip::ZipArchive::new(std::fs::File::open(archive)?)?;
        for i in 0..z.len() {
            let mut f = z.by_index(i)?;
            let Some(rel) = f.enclosed_name() else { continue };
            let out = dest.join(rel);
            if f.is_dir() {
                std::fs::create_dir_all(&out)?;
                continue;
            }
            if let Some(p) = out.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::io::copy(&mut f, &mut std::fs::File::create(&out)?)?;
        }
    } else {
        let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive)?);
        let mut t = tar::Archive::new(gz);
        t.set_preserve_permissions(true);
        // unpack_in refuses entries that would land outside `dest`.
        for e in t.entries()? {
            e?.unpack_in(dest)?;
        }
    }
    Ok(())
}

impl Llama {
    fn dir(&self) -> PathBuf {
        self.dir.get().cloned().unwrap_or_default()
    }
    fn db(&self) -> &Db {
        self.db.get().expect("llama engine initialised")
    }
    fn emit(&self) {
        if let Some(app) = self.app.get() {
            let _ = app.emit("llama-status", self.status());
        }
    }

    fn installed_variant(&self) -> Option<String> {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(self.dir().join("installed.json")).ok()?).ok()?;
        find_exe(&self.dir().join("bin"))?;
        v["variant"].as_str().map(|s| s.to_string())
    }

    pub fn status(&self) -> Status {
        let r = self.running.lock();
        let s = settings::load(self.db());
        Status {
            installed: self.installed_variant().is_some(),
            variant: self.installed_variant().unwrap_or_default(),
            tag: TAG.into(),
            installing: self.installing.load(Ordering::SeqCst),
            state: self.state.lock().clone(),
            current: r.as_ref().map(|x| x.alias.clone()),
            port: r.as_ref().map(|x| x.port).unwrap_or(s.llama.port),
            error: self.error.lock().clone(),
            embed_current: self.embed.lock().as_ref().map(|x| x.alias.clone()),
            embed_state: self.embed_state.lock().clone(),
        }
    }

    /// Devices this build can offload to. Cached until the build changes.
    pub async fn devices(&self, refresh: bool) -> Result<Vec<Device>> {
        if !refresh {
            if let Some(d) = self.devices.lock().clone() {
                return Ok(d);
            }
        }
        let exe = find_exe(&self.dir().join("bin")).ok_or_else(|| anyhow!("The built-in engine is not installed"))?;
        let mut c = crate::sandbox::command(&exe.to_string_lossy());
        c.arg("--list-devices").current_dir(exe.parent().unwrap_or(&self.dir())).stdin(Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(Duration::from_secs(30), c.output()).await.map_err(|_| anyhow!("Listing devices timed out"))??;
        let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        let d = parse_devices(&text);
        *self.devices.lock() = Some(d.clone());
        Ok(d)
    }

    /// Downloads a catalogue embedding model into the engine folder and returns its path.
    pub async fn fetch_embed_model(&self, id: &str) -> Result<PathBuf> {
        use futures_util::StreamExt;
        let pick = EMBED_MODELS.iter().find(|m| m.id == id).ok_or_else(|| anyhow!("Unknown embedding model"))?;
        if self.fetching.swap(true, Ordering::SeqCst) {
            bail!("A model download is already running");
        }
        let http = self.http.get().cloned().unwrap_or_default();
        let res: Result<PathBuf> = async {
            let dir = self.dir().join("models");
            std::fs::create_dir_all(&dir)?;
            let dest = dir.join(pick.file);
            if std::fs::metadata(&dest).map(|m| m.len() == pick.size).unwrap_or(false) && sha256_file(&dest)? == pick.sha256 {
                return Ok(dest);
            }
            let url = format!("https://huggingface.co/{}/resolve/main/{}", pick.repo, pick.file);
            let resp = http.get(url).timeout(Duration::from_secs(3600)).send().await?;
            if !resp.status().is_success() {
                bail!("Download failed: {}", resp.status());
            }
            let part = dir.join(format!("{}.part", pick.file));
            let mut f = tokio::fs::File::create(&part).await?;
            let mut got = 0u64;
            let mut s = resp.bytes_stream();
            let mut last = std::time::Instant::now();
            while let Some(c) = s.next().await {
                let c = c?;
                got += c.len() as u64;
                if got > pick.size + 1024 {
                    bail!("The file is larger than expected; download refused");
                }
                tokio::io::AsyncWriteExt::write_all(&mut f, &c).await?;
                if last.elapsed() > Duration::from_millis(150) {
                    last = std::time::Instant::now();
                    if let Some(app) = self.app.get() {
                        let _ = app.emit("llama-progress", json!({"stage": "model", "id": pick.id, "done": got, "total": pick.size}));
                    }
                }
            }
            tokio::io::AsyncWriteExt::flush(&mut f).await?;
            drop(f);
            if sha256_file(&part)? != pick.sha256 {
                let _ = std::fs::remove_file(&part);
                bail!("The download failed its integrity check and was deleted.");
            }
            std::fs::rename(&part, &dest)?;
            Ok(dest)
        }
        .await;
        self.fetching.store(false, Ordering::SeqCst);
        if let Some(app) = self.app.get() {
            let _ = app.emit("llama-progress", json!({"stage": "model-done", "id": pick.id}));
        }
        res
    }

    /// Makes sure `alias` is loaded on the embedding server and returns its port.
    pub async fn ensure_embed(&self, alias: &str) -> Result<u16> {
        let _one = self.embed_switch.lock().await;
        self.embed_used.store(now() as u64, Ordering::Relaxed);
        {
            let mut r = self.embed.lock();
            if let Some(x) = r.as_mut() {
                if x.alias == alias && matches!(x.child.try_wait(), Ok(None)) {
                    return Ok(x.port);
                }
            }
        }
        self.stop_embed();
        let s = settings::load(self.db());
        let m = s.llama.models.iter().find(|m| m.id == alias).cloned().ok_or_else(|| anyhow!("'{alias}' is not imported in the built-in engine"))?;
        let exe = find_exe(&self.dir().join("bin")).ok_or_else(|| anyhow!("The built-in engine is not installed. Open Models → Built-in engine and download it."))?;
        if !Path::new(&m.path).is_file() {
            bail!("The model file is missing: {}", m.path);
        }
        let port = free_port(s.llama.embed_port);
        let gpu = self.installed_variant().and_then(|v| variant(&v).map(|x| x.gpu)).unwrap_or(false);
        let ctx = if m.train_ctx > 0 { (m.train_ctx as u32).min(8192) } else { 2048 };
        let mut args: Vec<String> = vec![
            "-m".into(), m.path.clone(), "--host".into(), "127.0.0.1".into(), "--port".into(), port.to_string(),
            "-a".into(), m.id.clone(), "--embedding".into(), "--no-webui".into(),
            "-c".into(), ctx.to_string(), "-b".into(), ctx.to_string(), "-ub".into(), ctx.to_string(),
        ];
        let mut cfg = s.llama.clone();
        // Sampling and batch settings are for chat models; placement still applies.
        (cfg.temperature, cfg.top_k, cfg.top_p, cfg.min_p, cfg.repeat_penalty, cfg.batch, cfg.ubatch, cfg.parallel, cfg.cpu_moe) = (None, None, None, None, None, 0, 0, 0, 0);
        args.extend(tuning_args(&cfg, gpu));
        *self.embed_state.lock() = "loading".into();
        self.emit();
        let log = std::fs::File::create(self.dir().join("embed.log"))?;
        let mut c = crate::sandbox::command(&exe.to_string_lossy());
        c.args(&args).current_dir(exe.parent().unwrap_or(&self.dir())).stdin(Stdio::null()).stdout(Stdio::from(log.try_clone()?)).stderr(Stdio::from(log)).kill_on_drop(true);
        let child = c.spawn().map_err(|e| anyhow!("Could not start the embedding server: {e}"))?;
        *self.embed.lock() = Some(Running { child, alias: m.id.clone(), port });
        crate::audit::system(self.db(), "llama.embed_start", &m.id, json!({"args": args, "port": port}));
        let http = self.http.get().cloned().unwrap_or_default();
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
        loop {
            tokio::time::sleep(Duration::from_millis(300)).await;
            let exited = match self.embed.lock().as_mut() {
                None => Some("stopped".to_string()),
                Some(x) => match x.child.try_wait() {
                    Ok(Some(st)) => Some(format!("exit code {}", st.code().unwrap_or(-1))),
                    _ => None,
                },
            };
            if let Some(why) = exited {
                self.embed.lock().take();
                *self.embed_state.lock() = "error".into();
                self.emit();
                let t = std::fs::read_to_string(self.dir().join("embed.log")).unwrap_or_default();
                let tail: Vec<&str> = t.lines().rev().take(10).collect();
                bail!("The embedding server stopped while loading ({why}).\n{}", tail.into_iter().rev().collect::<Vec<_>>().join("\n"));
            }
            if http.get(format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(2)).send().await.map(|r| r.status().is_success()).unwrap_or(false) {
                *self.embed_state.lock() = "ready".into();
                self.emit();
                return Ok(port);
            }
            if std::time::Instant::now() > deadline {
                self.stop_embed();
                bail!("The embedding model did not load within 5 minutes");
            }
        }
    }

    pub fn stop_embed(&self) {
        if let Some(mut r) = self.embed.lock().take() {
            let _ = r.child.start_kill();
        }
        *self.embed_state.lock() = "stopped".into();
        self.emit();
    }

    /// Stops both servers (on quit).
    pub fn stop_all(&self) {
        self.stop();
        self.stop_embed();
    }

    pub fn log_tail(&self, lines: usize) -> String {
        let t = std::fs::read_to_string(self.dir().join("server.log")).unwrap_or_default();
        let v: Vec<&str> = t.lines().collect();
        v[v.len().saturating_sub(lines)..].join("\n")
    }

    /// Downloads, verifies and unpacks the chosen build, replacing any earlier one.
    pub async fn install(&self, variant_id: &str) -> Result<()> {
        use futures_util::StreamExt;
        let v = variant(variant_id).ok_or_else(|| anyhow!("Unknown build"))?;
        if v.os != std::env::consts::OS || v.arch != std::env::consts::ARCH {
            bail!("That build is for another kind of computer");
        }
        if self.installing.swap(true, Ordering::SeqCst) {
            bail!("A download is already running");
        }
        let http = self.http.get().cloned().unwrap_or_default();
        let res: Result<()> = async {
            let dir = self.dir();
            let dl = dir.join("download");
            std::fs::create_dir_all(&dl)?;
            let total: u64 = v.assets.iter().map(|a| a.size).sum();
            let mut done = 0u64;
            for asset in v.assets {
                let dest = dl.join(asset.name);
                if !(std::fs::metadata(&dest).map(|m| m.len() == asset.size).unwrap_or(false) && sha256_file(&dest)? == asset.sha256) {
                    let resp = http.get(release_url(asset.name)).timeout(Duration::from_secs(3600)).send().await?;
                    if !resp.status().is_success() {
                        bail!("Download of {} failed: {}", asset.name, resp.status());
                    }
                    let part = dl.join(format!("{}.part", asset.name));
                    let mut f = tokio::fs::File::create(&part).await?;
                    let mut got = 0u64;
                    let mut s = resp.bytes_stream();
                    let mut last = std::time::Instant::now();
                    while let Some(c) = s.next().await {
                        let c = c?;
                        got += c.len() as u64;
                        if got > asset.size + 1024 {
                            bail!("{} is larger than expected; download refused", asset.name);
                        }
                        tokio::io::AsyncWriteExt::write_all(&mut f, &c).await?;
                        if last.elapsed() > Duration::from_millis(150) {
                            last = std::time::Instant::now();
                            if let Some(app) = self.app.get() {
                                let _ = app.emit("llama-progress", json!({"stage": "download", "done": done + got, "total": total}));
                            }
                        }
                    }
                    tokio::io::AsyncWriteExt::flush(&mut f).await?;
                    drop(f);
                    let actual = sha256_file(&part)?;
                    if actual != asset.sha256 {
                        let _ = std::fs::remove_file(&part);
                        bail!("{} failed its integrity check. Nothing was installed.", asset.name);
                    }
                    std::fs::rename(&part, &dest)?;
                }
                done += asset.size;
            }
            if let Some(app) = self.app.get() {
                let _ = app.emit("llama-progress", json!({"stage": "unpack", "done": total, "total": total}));
            }
            self.stop();
            let staging = dir.join("bin.new");
            let _ = std::fs::remove_dir_all(&staging);
            for asset in v.assets {
                let (a, s) = (dl.join(asset.name), staging.clone());
                tokio::task::spawn_blocking(move || extract(&a, &s)).await??;
            }
            let exe = find_exe(&staging).ok_or_else(|| anyhow!("The download did not contain {}", exe_name()))?;
            // CUDA runtime DLLs must sit next to the server.
            if let Some(exe_dir) = exe.parent() {
                if exe_dir != staging {
                    for e in std::fs::read_dir(&staging)?.flatten() {
                        let p = e.path();
                        if p.is_file() && p.extension().map(|x| x == "dll").unwrap_or(false) {
                            let _ = std::fs::rename(&p, exe_dir.join(e.file_name()));
                        }
                    }
                }
            }
            let bin = dir.join("bin");
            let _ = std::fs::remove_dir_all(&bin);
            std::fs::rename(&staging, &bin)?;
            std::fs::write(dir.join("installed.json"), json!({"tag": TAG, "variant": v.id, "installed": now()}).to_string())?;
            *self.devices.lock() = None;
            let _ = std::fs::remove_dir_all(&dl);
            Ok(())
        }
        .await;
        self.installing.store(false, Ordering::SeqCst);
        self.emit();
        res
    }

    pub fn uninstall(&self) -> Result<()> {
        self.stop_all();
        *self.devices.lock() = None;
        let d = self.dir();
        for sub in ["bin", "download", "bin.new"] {
            let _ = std::fs::remove_dir_all(d.join(sub));
        }
        let _ = std::fs::remove_file(d.join("installed.json"));
        self.emit();
        Ok(())
    }

    /// Stops the server (frees its memory). Safe to call from any thread.
    pub fn stop(&self) {
        if let Some(mut r) = self.running.lock().take() {
            let _ = r.child.start_kill();
        }
        *self.state.lock() = "stopped".into();
        self.emit();
    }

    pub fn touch(&self) {
        self.last_used.store(now() as u64, Ordering::Relaxed);
    }

    pub fn idle_check(&self, idle_minutes: u64) {
        if idle_minutes == 0 {
            return;
        }
        let used = self.embed_used.load(Ordering::Relaxed);
        if self.embed.lock().is_some() && used > 0 && (now() as u64).saturating_sub(used) > idle_minutes * 60_000 && self.embed_switch.try_lock().is_ok() {
            self.stop_embed();
        }
        if self.running.lock().is_none() {
            return;
        }
        let last = self.last_used.load(Ordering::Relaxed);
        if last > 0 && (now() as u64).saturating_sub(last) > idle_minutes * 60_000 && self.switch.try_lock().is_ok() {
            self.stop();
        }
    }

    pub fn model_names(&self) -> Vec<(String, u64)> {
        settings::load(self.db()).llama.models.iter().map(|m| (m.id.clone(), m.size)).collect()
    }

    /// Makes sure `alias` is the loaded model, starting or switching the server as needed.
    pub async fn ensure(&self, alias: &str) -> Result<u16> {
        let _one = self.switch.lock().await;
        self.touch();
        {
            let mut r = self.running.lock();
            if let Some(x) = r.as_mut() {
                if x.alias == alias && matches!(x.child.try_wait(), Ok(None)) {
                    return Ok(x.port);
                }
            }
        }
        let s = settings::load(self.db());
        let m = s.llama.models.iter().find(|m| m.id == alias).cloned().ok_or_else(|| anyhow!("'{alias}' is not imported in the built-in engine"))?;
        self.start(&s.llama, &m).await
    }

    async fn start(&self, cfg: &settings::LlamaCfg, m: &LlamaModel) -> Result<u16> {
        self.stop();
        let exe = find_exe(&self.dir().join("bin")).ok_or_else(|| anyhow!("The built-in engine is not installed. Open Models → Built-in engine and download it."))?;
        if !Path::new(&m.path).is_file() {
            bail!("The model file is missing: {}", m.path);
        }
        if !m.mmproj.is_empty() && !Path::new(&m.mmproj).is_file() {
            bail!("The vision projector file is missing: {}", m.mmproj);
        }
        *self.state.lock() = "loading".into();
        self.error.lock().clear();
        self.emit();
        let port = free_port(cfg.port);
        if port != cfg.port {
            // Something else took our port; move and keep the provider address in sync.
            let mut s = settings::load(self.db());
            s.llama.port = port;
            let _ = settings::save(self.db(), &s);
            let _ = ensure_provider(self.db(), port);
        }
        let gpu = self.installed_variant().and_then(|v| variant(&v).map(|x| x.gpu)).unwrap_or(false);
        let ctx = if m.ctx > 0 { m.ctx } else { cfg.ctx_size };
        let ctx = if m.train_ctx > 0 { ctx.min(m.train_ctx as u32) } else { ctx };
        let mut args: Vec<String> = vec![
            "-m".into(), m.path.clone(),
            "--host".into(), "127.0.0.1".into(),
            "--port".into(), port.to_string(),
            "-c".into(), ctx.to_string(),
            "-a".into(), m.id.clone(),
            "--jinja".into(),
            "--reasoning-format".into(), "deepseek".into(),
            "--no-webui".into(),
        ];
        if !m.mmproj.is_empty() {
            args.extend(["--mmproj".into(), m.mmproj.clone()]);
        }
        args.extend(tuning_args(cfg, gpu));
        args.extend(cfg.extra_args.split_whitespace().map(|s| s.to_string()));
        let log_path = self.dir().join("server.log");
        let log = std::fs::File::create(&log_path)?;
        let mut c = crate::sandbox::command(&exe.to_string_lossy());
        c.args(&args).current_dir(exe.parent().unwrap_or(&self.dir())).stdin(Stdio::null()).stdout(Stdio::from(log.try_clone()?)).stderr(Stdio::from(log)).kill_on_drop(true);
        let child = c.spawn().map_err(|e| anyhow!("Could not start llama-server: {e}"))?;
        *self.running.lock() = Some(Running { child, alias: m.id.clone(), port });
        crate::audit::system(self.db(), "llama.start", &m.id, json!({"args": args, "port": port}));

        // Large models can take minutes to load; poll health and watch for an early exit.
        let http = self.http.get().cloned().unwrap_or_default();
        let deadline = std::time::Instant::now() + Duration::from_secs(600);
        loop {
            tokio::time::sleep(Duration::from_millis(400)).await;
            let exited = {
                let mut r = self.running.lock();
                match r.as_mut() {
                    None => Some("stopped".to_string()),
                    Some(x) => match x.child.try_wait() {
                        Ok(Some(st)) => Some(format!("exit code {}", st.code().unwrap_or(-1))),
                        _ => None,
                    },
                }
            };
            if let Some(why) = exited {
                self.running.lock().take();
                return self.fail(format!("llama-server stopped while loading ({why}).\n{}", self.log_tail(12)));
            }
            let ok = http.get(format!("http://127.0.0.1:{port}/health")).timeout(Duration::from_secs(2)).send().await.map(|r| r.status().is_success()).unwrap_or(false);
            if ok {
                *self.state.lock() = "ready".into();
                self.touch();
                self.emit();
                return Ok(port);
            }
            if std::time::Instant::now() > deadline {
                self.stop();
                return self.fail(format!("The model did not finish loading within 10 minutes.\n{}", self.log_tail(12)));
            }
        }
    }

    fn fail<T>(&self, msg: String) -> Result<T> {
        *self.state.lock() = "error".into();
        *self.error.lock() = msg.clone();
        self.emit();
        Err(anyhow!(msg))
    }
}

fn free_port(wanted: u16) -> u16 {
    if std::net::TcpListener::bind(("127.0.0.1", wanted)).is_ok() {
        return wanted;
    }
    std::net::TcpListener::bind(("127.0.0.1", 0)).and_then(|l| l.local_addr()).map(|a| a.port()).unwrap_or(wanted)
}

pub fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/v1")
}

/// Creates (or re-points) the provider that lists the built-in engine's models.
pub fn ensure_provider(db: &Db, port: u16) -> Result<String> {
    let existing = crate::models::list_providers(db)?.into_iter().find(|p| p["kind"] == "llamacpp");
    match existing {
        Some(p) => {
            let id = p["id"].as_str().unwrap_or("").to_string();
            if p["base_url"].as_str() != Some(base_url(port).as_str()) {
                crate::models::update_provider(db, &id, p["name"].as_str().unwrap_or("Built-in llama.cpp"), &base_url(port), None)?;
            }
            Ok(id)
        }
        None => crate::models::add_provider(db, "llamacpp", "Built-in llama.cpp", &base_url(port), None),
    }
}

/// A short, unique, server-safe name for a model file.
pub fn alias_for(path: &Path, taken: &[String]) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_else(|| "model".into());
    let base: String = stem.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '-' }).collect();
    let base = base.trim_matches('-').chars().take(60).collect::<String>();
    let base = if base.is_empty() { "model".to_string() } else { base };
    let mut name = base.clone();
    let mut n = 2;
    while taken.contains(&name) {
        name = format!("{base}-{n}");
        n += 1;
    }
    name
}

/// Finds a vision projector next to a model: an mmproj GGUF whose name shares the model's prefix,
/// or the only projector in the folder.
pub fn find_projector(model: &Path) -> Option<PathBuf> {
    let dir = model.parent()?;
    let stem = model.file_stem()?.to_string_lossy().to_lowercase();
    let mut cands: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("gguf")).unwrap_or(false) && p != model)
        .filter(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().contains("mmproj")).unwrap_or(false) || gguf_info(p).map(|i| i.is_projector).unwrap_or(false))
        .collect();
    if cands.len() == 1 {
        return cands.pop();
    }
    let prefix: String = stem.split(['-', '.', '_']).take(2).collect::<Vec<_>>().join("-");
    cands.into_iter().find(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase().replace(['.', '_'], "-").contains(&prefix)).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_gguf(path: &Path, kvs: &[(&str, u32, Vec<u8>)]) {
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend((kvs.len() as u64).to_le_bytes());
        for (k, ty, v) in kvs {
            b.extend((k.len() as u64).to_le_bytes());
            b.extend(k.as_bytes());
            b.extend(ty.to_le_bytes());
            b.extend(v);
        }
        std::fs::write(path, b).unwrap();
    }
    fn s(v: &str) -> Vec<u8> {
        let mut b = (v.len() as u64).to_le_bytes().to_vec();
        b.extend(v.as_bytes());
        b
    }

    #[test]
    fn reads_gguf_headers() {
        let d = std::env::temp_dir().join(format!("gguf-{}", crate::db::new_id()));
        std::fs::create_dir_all(&d).unwrap();
        // A string array (like tokenizer tokens) must be skipped correctly.
        let mut arr = 8u32.to_le_bytes().to_vec();
        arr.extend(2u64.to_le_bytes());
        arr.extend(s("a"));
        arr.extend(s("bc"));
        write_gguf(&d.join("qwen3-4b-q4.gguf"), &[("general.architecture", 8, s("qwen3")), ("tokenizer.ggml.tokens", 9, arr), ("qwen3.context_length", 4, 40960u32.to_le_bytes().to_vec()), ("general.name", 8, s("Qwen3 4B"))]);
        let i = gguf_info(&d.join("qwen3-4b-q4.gguf")).unwrap();
        assert_eq!((i.arch.as_str(), i.name.as_str(), i.context_length, i.is_projector), ("qwen3", "Qwen3 4B", 40960, false));
        write_gguf(&d.join("mmproj-qwen3-f16.gguf"), &[("general.architecture", 8, s("clip")), ("clip.has_vision_encoder", 7, vec![1])]);
        assert!(gguf_info(&d.join("mmproj-qwen3-f16.gguf")).unwrap().is_projector);
        assert_eq!(find_projector(&d.join("qwen3-4b-q4.gguf")).unwrap(), d.join("mmproj-qwen3-f16.gguf"));
        std::fs::write(d.join("x.gguf"), b"nope").unwrap();
        assert!(gguf_info(&d.join("x.gguf")).is_err());
        assert_eq!(alias_for(Path::new("C:/m/Qwen3 4B Q4.gguf"), &["qwen3-4b-q4".into()]), "qwen3-4b-q4-2");
        write_gguf(&d.join("nomic.gguf"), &[("general.architecture", 8, s("nomic-bert")), ("nomic-bert.pooling_type", 4, 1u32.to_le_bytes().to_vec())]);
        assert!(gguf_info(&d.join("nomic.gguf")).unwrap().is_embedding);
        assert!(!gguf_info(&d.join("qwen3-4b-q4.gguf")).unwrap().is_embedding);
    }

    #[test]
    fn parses_devices_and_flags() {
        let out = "ggml_vulkan: Found 2 Vulkan devices:\nAvailable devices:\n  Vulkan0: AMD Radeon(TM) Graphics (12207 MiB, 11596 MiB free)\n  Vulkan1: NVIDIA GeForce GTX 1650 (4149 MiB, 3558 MiB free)\n";
        let d = parse_devices(out);
        assert_eq!(d.len(), 2);
        assert_eq!((d[1].id.as_str(), d[1].name.as_str(), d[1].total_mb, d[1].free_mb, d[1].integrated), ("Vulkan1", "NVIDIA GeForce GTX 1650", 4149, 3558, false));
        assert!(d[0].integrated);
        let mut c = settings::LlamaCfg::default();
        assert_eq!(tuning_args(&c, true), vec!["-ngl", "999"]);
        c.devices = "Vulkan1".into();
        c.flash_attn = "on".into();
        c.cache_k = "q8_0".into();
        c.temperature = Some(0.7);
        let a = tuning_args(&c, true).join(" ");
        assert!(a.contains("-dev Vulkan1") && a.contains("-fa on") && a.contains("-ctk q8_0") && a.contains("--temp 0.7"), "{a}");
        c.devices = "none".into();
        assert!(tuning_args(&c, true).join(" ").contains("-dev none -ngl 0"));
        assert!(tuning_args(&settings::LlamaCfg::default(), false).join(" ").contains("-ngl 0"));
    }
}

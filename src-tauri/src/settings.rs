//! Application settings, stored as one JSON document. Every field has a safe default.

use crate::db::Db;
use anyhow::Result;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Roles {
    pub planner: Option<String>,
    pub doer: Option<String>,
    pub eyes: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxCfg {
    /// "none" | "microvm" (built-in Linux VM) | "wsl" | "docker"
    pub kind: String,
    /// WSL distribution name, e.g. "Alpine"
    pub distro: String,
    /// Docker image, e.g. "alpine:3.20"
    pub image: String,
    /// Allow network access inside the sandbox.
    pub network: bool,
    /// Let the agent run host commands (always behind an approval gate) when no sandbox is set.
    pub allow_host: bool,
    pub timeout_secs: u32,
    /// Memory for the built-in VM.
    pub vm_memory_mb: u32,
    /// Stop the built-in VM after this many idle minutes (0 = keep it running).
    pub vm_idle_minutes: u32,
}

impl Default for SandboxCfg {
    fn default() -> Self {
        Self { kind: "none".into(), distro: String::new(), image: "alpine:3.20".into(), network: false, allow_host: true, timeout_secs: 60, vm_memory_mb: 128, vm_idle_minutes: 10 }
    }
}

/// A GGUF model imported into the built-in llama.cpp engine (the file stays where it is).
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LlamaModel {
    /// Name the model is served under; also the model name in the app.
    pub id: String,
    pub path: String,
    /// Vision projector (mmproj) GGUF; empty for text-only models.
    pub mmproj: String,
    /// Context size for this model; 0 uses the engine default.
    pub ctx: u32,
    pub size: u64,
    pub arch: String,
    pub train_ctx: u64,
    /// Embedding model (served with --embedding on its own port) rather than a chat model.
    pub embedding: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LlamaCfg {
    pub port: u16,
    pub ctx_size: u32,
    /// Layers on the GPU; -1 means all on GPU builds and none on CPU builds.
    pub gpu_layers: i32,
    /// CPU threads; 0 lets llama.cpp decide.
    pub threads: u32,
    /// Unload the model after this many idle minutes (0 = keep it loaded).
    pub idle_minutes: u32,
    /// Extra llama-server arguments for experts.
    pub extra_args: String,
    pub models: Vec<LlamaModel>,
    /// Where layers run: "auto", "none" (CPU only) or a comma list of devices such as "Vulkan1".
    pub devices: String,
    /// How a model is spread over several GPUs: "layer" | "row" | "none".
    pub split_mode: String,
    /// Share of the model per GPU, e.g. "3,1"; empty splits by free memory.
    pub tensor_split: String,
    /// Index (within the chosen devices) of the GPU that holds the model when split_mode is "none".
    pub main_gpu: u32,
    /// "auto" | "on" | "off"
    pub flash_attn: String,
    /// Prompt batch sizes; 0 uses llama.cpp's defaults.
    pub batch: u32,
    pub ubatch: u32,
    /// KV cache types: "f16", "q8_0", "q4_0"…; quantised caches save memory.
    pub cache_k: String,
    pub cache_v: String,
    pub mlock: bool,
    pub no_mmap: bool,
    /// Parallel request slots; 0 lets llama.cpp decide.
    pub parallel: u32,
    /// Keep the expert weights of the first N MoE layers on the CPU (0 = off).
    pub cpu_moe: u32,
    /// Sampling defaults; None keeps the model's own.
    pub temperature: Option<f32>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
    pub min_p: Option<f32>,
    pub repeat_penalty: Option<f32>,
    /// Port of the separate embedding server.
    pub embed_port: u16,
}

impl Default for LlamaCfg {
    fn default() -> Self {
        Self {
            port: 39281,
            ctx_size: 8192,
            gpu_layers: -1,
            threads: 0,
            idle_minutes: 10,
            extra_args: String::new(),
            models: vec![],
            devices: "auto".into(),
            split_mode: "layer".into(),
            tensor_split: String::new(),
            main_gpu: 0,
            flash_attn: "auto".into(),
            batch: 0,
            ubatch: 0,
            cache_k: "f16".into(),
            cache_v: "f16".into(),
            mlock: false,
            no_mmap: false,
            parallel: 0,
            cpu_moe: 0,
            temperature: None,
            top_k: None,
            top_p: None,
            min_p: None,
            repeat_penalty: None,
            embed_port: 39282,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SubagentCfg {
    /// The chat model may hand work to other models (off by default).
    pub enabled: bool,
    /// Models the main model may call; empty means every local chat model.
    pub models: Vec<String>,
    /// How many sub-agents may run at the same time.
    pub max_parallel: u32,
    /// Let the main model call local models only, never cloud ones.
    pub local_only: bool,
}

impl Default for SubagentCfg {
    fn default() -> Self {
        Self { enabled: false, models: vec![], max_parallel: 3, local_only: true }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct McpServer {
    pub id: String,
    pub name: String,
    /// "stdio" | "http"
    pub transport: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub url: String,
    pub enabled: bool,
    /// "allow" | "ask" | "off" for every tool of this server.
    pub permission: String,
}

impl Default for McpServer {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), transport: "stdio".into(), command: String::new(), args: vec![], env: HashMap::new(), url: String::new(), enabled: true, permission: "ask".into() }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PluginPref {
    pub enabled: bool,
    /// "allow" | "ask" | "off"; empty means use the plugin's own risk level.
    pub permission: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // Appearance
    pub theme: String,
    pub accent: String,
    pub app_name: String,
    pub assistant_name: String,
    pub radius: String,
    pub sidebar_collapsed: bool,
    pub show_suggestions: bool,
    pub send_on_enter: bool,
    pub text_size: u32,
    pub density: String,
    pub layer: String,
    // Models
    pub default_model: Option<String>,
    pub roles: Roles,
    pub fallback_model: Option<String>,
    pub local_only: bool,
    pub system_prompt: String,
    pub chat_tools: bool,
    /// "auto" (provider default) | "off" | "low" | "medium" | "high" for models that can reason.
    pub reasoning: String,
    /// Show the model's reasoning in chat while it thinks.
    pub show_reasoning: bool,
    pub tool_permissions: HashMap<String, String>,
    /// Model that turns documents into vectors for search (any model marked as embedding).
    pub embed_model: Option<String>,
    /// How attached documents reach the model: "ask" | "embed" | "direct".
    pub doc_mode: String,
    pub subagents: SubagentCfg,
    /// Show input and output prices on the Models screen.
    pub show_prices: bool,
    // Agent behaviour
    pub autonomy: String,
    pub plan_first: bool,
    pub draft_only: bool,
    pub pointer_delay_ms: u64,
    pub max_steps: u32,
    pub max_task_minutes: u32,
    pub max_retries: u32,
    pub screenshot_width: u32,
    pub settle_ms: u64,
    pub yield_to_user: bool,
    pub corner_pause: bool,
    pub minimize_on_run: bool,
    // Cost
    pub task_cost_cap: f64,
    pub daily_cost_cap: f64,
    // Permissions
    pub allowed_folders: Vec<String>,
    pub protected_folders: Vec<String>,
    pub blocked_apps: Vec<String>,
    pub blocked_sites: Vec<String>,
    pub undo_hours: u32,
    // Privacy
    pub mask_secrets: bool,
    pub keep_screenshots_days: u32,
    pub telemetry: bool,
    /// Days to keep audit records; 0 keeps them forever.
    pub audit_retention_days: u32,
    // Shortcuts
    pub stop_shortcut: String,
    pub summon_shortcut: String,
    // Extensions
    pub sandbox: SandboxCfg,
    pub llama: LlamaCfg,
    pub mcp_servers: Vec<McpServer>,
    pub plugins: HashMap<String, PluginPref>,
    pub web_search: bool,
    // Onboarding
    pub onboarded: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        let home = dirs::home_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
        let mut protected = vec![];
        if cfg!(windows) {
            protected.extend([
                "C:\\Windows".to_string(),
                "C:\\Program Files".to_string(),
                "C:\\Program Files (x86)".to_string(),
                "C:\\ProgramData".to_string(),
            ]);
            if !home.is_empty() {
                protected.push(format!("{home}\\AppData"));
            }
        } else {
            protected.extend(["/etc", "/usr", "/bin", "/System", "/Library"].map(String::from));
            if !home.is_empty() {
                protected.push(format!("{home}/.ssh"));
            }
        }
        let mut tool_permissions = HashMap::new();
        for (t, p) in [
            ("list_files", "allow"),
            ("read_file", "allow"),
            ("fetch_url", "ask"),
            ("clipboard_read", "ask"),
            ("clipboard_write", "allow"),
            ("current_time", "allow"),
            ("web_search", "allow"),
            ("search_files", "allow"),
            ("system_info", "allow"),
            ("run_command", "ask"),
        ] {
            tool_permissions.insert(t.to_string(), p.to_string());
        }
        Self {
            theme: "system".into(),
            accent: "#4f6bed".into(),
            app_name: "Computer Agent".into(),
            assistant_name: "Assistant".into(),
            radius: "rounded".into(),
            sidebar_collapsed: false,
            show_suggestions: true,
            send_on_enter: true,
            text_size: 14,
            density: "comfortable".into(),
            layer: "basic".into(),
            default_model: None,
            roles: Roles::default(),
            fallback_model: None,
            local_only: false,
            system_prompt: String::new(),
            chat_tools: true,
            reasoning: "auto".into(),
            show_reasoning: true,
            tool_permissions,
            embed_model: None,
            doc_mode: "ask".into(),
            subagents: SubagentCfg::default(),
            show_prices: true,
            autonomy: "balanced".into(),
            plan_first: true,
            draft_only: true,
            pointer_delay_ms: 400,
            max_steps: 40,
            max_task_minutes: 20,
            max_retries: 2,
            screenshot_width: 1366,
            settle_ms: 2500,
            yield_to_user: true,
            corner_pause: true,
            minimize_on_run: true,
            task_cost_cap: 1.0,
            daily_cost_cap: 5.0,
            allowed_folders: if home.is_empty() { vec![] } else { vec![home] },
            protected_folders: protected,
            blocked_apps: [
                "1password", "bitwarden", "keepass", "lastpass", "dashlane", "keeper",
                "credential manager", "windows security", "user account control",
            ]
            .map(String::from)
            .to_vec(),
            blocked_sites: ["bank", "paypal.com", "wise.com", "venmo.com", "coinbase.com", "binance.com"]
                .map(String::from)
                .to_vec(),
            undo_hours: 24,
            mask_secrets: true,
            keep_screenshots_days: 30,
            telemetry: false,
            audit_retention_days: 365,
            stop_shortcut: "Ctrl+Shift+F12".into(),
            summon_shortcut: "Ctrl+Shift+F11".into(),
            sandbox: SandboxCfg::default(),
            llama: LlamaCfg::default(),
            mcp_servers: vec![],
            plugins: HashMap::new(),
            web_search: true,
            onboarded: false,
        }
    }
}

pub fn load(db: &Db) -> AppSettings {
    let conn = db.conn.lock();
    let raw: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key='app'", [], |r| r.get(0))
        .optional()
        .ok()
        .flatten();
    raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn save(db: &Db, s: &AppSettings) -> Result<()> {
    let conn = db.conn.lock();
    conn.execute(
        "INSERT INTO settings(key,value) VALUES('app',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [serde_json::to_string(s)?],
    )?;
    Ok(())
}

/// One-click profiles from the spec.
pub fn apply_profile(s: &mut AppSettings, profile: &str) {
    match profile {
        "local" => {
            s.local_only = true;
            s.autonomy = "balanced".into();
            s.chat_tools = true;
        }
        "balanced" => {
            s.local_only = false;
            s.autonomy = "balanced".into();
            s.plan_first = true;
            s.draft_only = true;
            s.pointer_delay_ms = 400;
        }
        "power" => {
            s.local_only = false;
            s.autonomy = "autonomous".into();
            s.plan_first = false;
            s.pointer_delay_ms = 150;
            s.max_steps = 80;
        }
        _ => {}
    }
}

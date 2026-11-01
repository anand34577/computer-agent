//! Engine-side validation. The interface validates too, but these checks are the ones that count.

use crate::settings::AppSettings;
use std::str::FromStr;

/// Field-keyed problems, so the interface can show each next to its input.
pub type Problems = Vec<(String, String)>;

pub fn to_error(p: Problems) -> String {
    p.into_iter().map(|(f, m)| format!("{f}: {m}")).collect::<Vec<_>>().join("\n")
}

/// Where an address points. Loopback and private-network hosts count as "local":
/// the data stays on this computer or the user's own network.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Scope {
    Loopback,
    Private,
    Public,
}

pub fn host_scope(host: &str) -> Scope {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']').to_lowercase();
    if h == "localhost" || h.ends_with(".localhost") || h == "0.0.0.0" || h == "::1" || h == "::" {
        return Scope::Loopback;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v) => {
                let o = v.octets();
                if v.is_loopback() || v.is_unspecified() {
                    Scope::Loopback
                } else if v.is_private() || v.is_link_local() || (o[0] == 100 && (64..128).contains(&o[1])) {
                    Scope::Private
                } else {
                    Scope::Public
                }
            }
            std::net::IpAddr::V6(v) => {
                let seg = v.segments()[0];
                if v.is_loopback() || v.is_unspecified() {
                    Scope::Loopback
                } else if (seg & 0xfe00) == 0xfc00 || (seg & 0xffc0) == 0xfe80 {
                    Scope::Private
                } else {
                    Scope::Public
                }
            }
        };
    }
    // Single-label names (my-pc) and home-network suffixes resolve inside the user's network.
    if !h.contains('.') || [".local", ".lan", ".home", ".internal", ".home.arpa", ".intranet", ".corp", ".ts.net"].iter().any(|s| h.ends_with(s)) {
        return Scope::Private;
    }
    Scope::Public
}

pub fn url_scope(u: &str) -> Scope {
    reqwest::Url::parse(u.trim()).ok().and_then(|u| u.host_str().map(host_scope)).unwrap_or(Scope::Public)
}

/// Accepts http and https. Plain http is allowed everywhere (local runtimes rarely have
/// certificates); the interface warns when it points at a public host.
pub fn url(base: &str) -> Result<(), String> {
    let b = base.trim();
    if b.is_empty() {
        return Err("Enter a base address".into());
    }
    let u = reqwest::Url::parse(b).map_err(|_| "Not a valid address. Example: http://127.0.0.1:11434 or https://api.example.com/v1".to_string())?;
    if u.scheme() != "http" && u.scheme() != "https" {
        return Err("Address must start with http:// or https://".into());
    }
    if u.host_str().map(|h| h.is_empty()).unwrap_or(true) {
        return Err("Address has no host name".into());
    }
    Ok(())
}

pub fn is_local_url(u: &str) -> bool {
    url_scope(u) != Scope::Public
}

pub fn mcp_server(m: &crate::settings::McpServer) -> Result<(), String> {
    let mut p: Problems = vec![];
    if m.name.trim().is_empty() || m.name.chars().count() > 40 {
        p.push(("Name".into(), "Use 1 to 40 characters".into()));
    }
    match m.transport.as_str() {
        "stdio" => {
            if m.command.trim().is_empty() {
                p.push(("Command".into(), "Enter the program to start, e.g. npx or python".into()));
            }
            for k in m.env.keys() {
                if k.trim().is_empty() || k.contains(['=', ' ']) {
                    p.push(("Environment".into(), format!("'{k}' is not a valid variable name")));
                }
            }
        }
        "http" => {
            if let Err(e) = url(&m.url) {
                p.push(("Address".into(), e));
            }
        }
        _ => p.push(("Transport".into(), "Choose Local program or Web address".into())),
    }
    if !["allow", "ask", "off"].contains(&m.permission.as_str()) {
        p.push(("Permission".into(), "Must be allow, ask or off".into()));
    }
    if p.is_empty() { Ok(()) } else { Err(to_error(p)) }
}

pub fn provider(kind: &str, name: &str, base: &str, key: Option<&str>, is_new: bool) -> Result<(), String> {
    let mut p: Problems = vec![];
    if !["openai", "anthropic", "ollama", "llamacpp"].contains(&kind) {
        p.push(("Type".into(), "Unknown provider type".into()));
    }
    let n = name.trim();
    if n.is_empty() {
        p.push(("Name".into(), "Enter a name".into()));
    } else if n.chars().count() > 60 {
        p.push(("Name".into(), "Keep the name under 60 characters".into()));
    }
    if let Err(e) = url(base) {
        p.push(("Base address".into(), e));
    }
    if let Some(k) = key {
        if k.chars().any(char::is_whitespace) {
            p.push(("API key".into(), "The key contains spaces; paste it again".into()));
        }
    }
    if is_new && kind == "anthropic" && key.map(|k| k.trim().is_empty()).unwrap_or(true) {
        p.push(("API key".into(), "Anthropic requires an API key".into()));
    }
    if p.is_empty() { Ok(()) } else { Err(to_error(p)) }
}

pub fn model(label: &str, price_in: f64, price_out: f64) -> Result<(), String> {
    let mut p: Problems = vec![];
    if label.trim().is_empty() {
        p.push(("Display name".into(), "Enter a name".into()));
    } else if label.chars().count() > 80 {
        p.push(("Display name".into(), "Keep it under 80 characters".into()));
    }
    for (f, v) in [("Input price", price_in), ("Output price", price_out)] {
        if !v.is_finite() || !(0.0..=1000.0).contains(&v) {
            p.push((f.into(), "Must be between 0 and 1000 USD per million tokens".into()));
        }
    }
    if p.is_empty() { Ok(()) } else { Err(to_error(p)) }
}

fn range<T: PartialOrd + std::fmt::Display + Copy>(p: &mut Problems, field: &str, v: T, lo: T, hi: T) {
    if v < lo || v > hi {
        p.push((field.into(), format!("Must be between {lo} and {hi}")));
    }
}

pub fn shortcut(s: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(());
    }
    let lower = t.to_lowercase();
    if !["ctrl", "control", "alt", "shift", "super", "cmd", "command", "commandorcontrol", "cmdorctrl"].iter().any(|m| lower.split('+').any(|p| p.trim() == *m)) {
        return Err("Include at least one modifier (Ctrl, Alt or Shift)".into());
    }
    if lower.replace(' ', "") == "ctrl+shift+esc" || lower.replace(' ', "") == "ctrl+shift+escape" {
        return Err("Ctrl+Shift+Esc opens Task Manager on Windows; choose another".into());
    }
    if lower.replace(' ', "") == "ctrl+alt+delete" || lower.replace(' ', "") == "ctrl+alt+del" {
        return Err("Ctrl+Alt+Delete is reserved by the system".into());
    }
    tauri_plugin_global_shortcut::Shortcut::from_str(t).map(|_| ()).map_err(|_| "Not a recognised key combination. Example: Ctrl+Shift+F12".into())
}

pub fn settings(s: &AppSettings, model_ids: &[String]) -> Problems {
    let mut p: Problems = vec![];
    let one_of = |p: &mut Problems, f: &str, v: &str, opts: &[&str]| {
        if !opts.contains(&v) {
            p.push((f.into(), format!("Must be one of: {}", opts.join(", "))));
        }
    };
    one_of(&mut p, "theme", &s.theme, &["system", "light", "dark", "contrast"]);
    one_of(&mut p, "density", &s.density, &["comfortable", "compact"]);
    one_of(&mut p, "layer", &s.layer, &["basic", "advanced", "expert"]);
    one_of(&mut p, "autonomy", &s.autonomy, &["supervised", "balanced", "autonomous", "full"]);
    let hex = s.accent.len() == 7 && s.accent.starts_with('#') && s.accent[1..].chars().all(|c| c.is_ascii_hexdigit());
    if !hex {
        p.push(("accent".into(), "Use a colour like #4f6bed".into()));
    }
    range(&mut p, "text_size", s.text_size, 11, 22);
    range(&mut p, "pointer_delay_ms", s.pointer_delay_ms, 0, 3000);
    range(&mut p, "max_steps", s.max_steps, 1, 500);
    range(&mut p, "max_task_minutes", s.max_task_minutes, 1, 240);
    range(&mut p, "max_retries", s.max_retries, 0, 10);
    range(&mut p, "screenshot_width", s.screenshot_width, 640, 3840);
    range(&mut p, "settle_ms", s.settle_ms, 0, 10000);
    range(&mut p, "undo_hours", s.undo_hours, 1, 720);
    range(&mut p, "keep_screenshots_days", s.keep_screenshots_days, 1, 365);
    for (f, v) in [("task_cost_cap", s.task_cost_cap), ("daily_cost_cap", s.daily_cost_cap)] {
        if !v.is_finite() || !(0.0..=10000.0).contains(&v) {
            p.push((f.into(), "Must be between 0 and 10000 (0 means no cap)".into()));
        }
    }
    if s.daily_cost_cap > 0.0 && s.task_cost_cap > s.daily_cost_cap {
        p.push(("task_cost_cap".into(), "The per-task cap cannot exceed the daily cap".into()));
    }
    for (f, v) in [("stop_shortcut", &s.stop_shortcut), ("summon_shortcut", &s.summon_shortcut)] {
        if let Err(e) = shortcut(v) {
            p.push((f.into(), e));
        }
    }
    if s.stop_shortcut.trim().is_empty() {
        p.push(("stop_shortcut".into(), "An emergency stop shortcut is required".into()));
    }
    if !s.summon_shortcut.trim().is_empty() && s.stop_shortcut.trim().eq_ignore_ascii_case(s.summon_shortcut.trim()) {
        p.push(("summon_shortcut".into(), "Must differ from the stop shortcut".into()));
    }
    for (f, list) in [("allowed_folders", &s.allowed_folders), ("protected_folders", &s.protected_folders)] {
        for d in list.iter().filter(|d| !d.trim().is_empty()) {
            if !std::path::Path::new(d.trim()).is_absolute() {
                p.push((f.into(), format!("'{d}' is not a full path")));
            }
        }
    }
    for (tool, perm) in &s.tool_permissions {
        if !["allow", "ask", "off"].contains(&perm.as_str()) {
            p.push(("tool_permissions".into(), format!("{tool}: must be allow, ask or off")));
        }
    }
    one_of(&mut p, "radius", &s.radius, &["sharp", "rounded", "round"]);
    if s.app_name.trim().is_empty() || s.app_name.chars().count() > 40 {
        p.push(("app_name".into(), "Use 1 to 40 characters".into()));
    }
    if s.assistant_name.trim().is_empty() || s.assistant_name.chars().count() > 30 {
        p.push(("assistant_name".into(), "Use 1 to 30 characters".into()));
    }
    one_of(&mut p, "sandbox.kind", &s.sandbox.kind, &["none", "microvm", "wsl", "docker"]);
    one_of(&mut p, "reasoning", &s.reasoning, &["auto", "off", "low", "medium", "high"]);
    if ![64, 128, 256, 512].contains(&s.sandbox.vm_memory_mb) {
        p.push(("sandbox.vm_memory_mb".into(), "Choose 64, 128, 256 or 512 MB".into()));
    }
    range(&mut p, "sandbox.vm_idle_minutes", s.sandbox.vm_idle_minutes, 0, 1440);
    if s.sandbox.kind == "wsl" && s.sandbox.distro.trim().is_empty() {
        p.push(("sandbox.distro".into(), "Choose a WSL distribution".into()));
    }
    if s.sandbox.kind == "docker" && s.sandbox.image.trim().is_empty() {
        p.push(("sandbox.image".into(), "Enter a Docker image, e.g. alpine:3.20".into()));
    }
    range(&mut p, "sandbox.timeout_secs", s.sandbox.timeout_secs, 5, 3600);
    range(&mut p, "audit_retention_days", s.audit_retention_days, 0, 3650);
    range(&mut p, "llama.port", s.llama.port, 1024, 65535);
    range(&mut p, "llama.ctx_size", s.llama.ctx_size, 512, 1_048_576);
    range(&mut p, "llama.gpu_layers", s.llama.gpu_layers, -1, 999);
    range(&mut p, "llama.threads", s.llama.threads, 0, 256);
    range(&mut p, "llama.idle_minutes", s.llama.idle_minutes, 0, 1440);
    for bad in ["-m", "--model", "--host", "--port", "-a", "--alias", "--mmproj", "--api-key"] {
        if s.llama.extra_args.split_whitespace().any(|a| a == bad) {
            p.push(("llama.extra_args".into(), format!("{bad} is set by the app; remove it")));
        }
    }
    if s.llama.extra_args.contains(['&', '|', ';', '`', '$']) {
        p.push(("llama.extra_args".into(), "Use plain arguments only".into()));
    }
    let dev_ok = s.llama.devices == "auto"
        || s.llama.devices == "none"
        || (!s.llama.devices.is_empty() && s.llama.devices.split(',').all(|d| !d.is_empty() && d.len() <= 32 && d.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')));
    if !dev_ok {
        p.push(("llama.devices".into(), "Choose Auto, CPU only or detected devices".into()));
    }
    one_of(&mut p, "llama.split_mode", &s.llama.split_mode, &["layer", "row", "none"]);
    one_of(&mut p, "llama.flash_attn", &s.llama.flash_attn, &["auto", "on", "off"]);
    let caches = ["f16", "bf16", "f32", "q8_0", "q5_1", "q5_0", "q4_1", "q4_0", "iq4_nl"];
    one_of(&mut p, "llama.cache_k", &s.llama.cache_k, &caches);
    one_of(&mut p, "llama.cache_v", &s.llama.cache_v, &caches);
    if !s.llama.tensor_split.is_empty() && !s.llama.tensor_split.split(',').all(|x| x.trim().parse::<f32>().map(|f| (0.0..=100.0).contains(&f)).unwrap_or(false)) {
        p.push(("llama.tensor_split".into(), "Use numbers separated by commas, e.g. 3,1".into()));
    }
    range(&mut p, "llama.main_gpu", s.llama.main_gpu, 0, 15);
    if s.llama.batch != 0 {
        range(&mut p, "llama.batch", s.llama.batch, 32, 16384);
    }
    if s.llama.ubatch != 0 {
        range(&mut p, "llama.ubatch", s.llama.ubatch, 16, 16384);
    }
    range(&mut p, "llama.parallel", s.llama.parallel, 0, 64);
    range(&mut p, "llama.cpu_moe", s.llama.cpu_moe, 0, 999);
    range(&mut p, "llama.embed_port", s.llama.embed_port, 1024, 65535);
    if s.llama.embed_port == s.llama.port {
        p.push(("llama.embed_port".into(), "Must differ from the chat port".into()));
    }
    if let Some(t) = s.llama.temperature {
        range(&mut p, "llama.temperature", t, 0.0, 2.0);
    }
    if let Some(t) = s.llama.top_p {
        range(&mut p, "llama.top_p", t, 0.0, 1.0);
    }
    if let Some(t) = s.llama.min_p {
        range(&mut p, "llama.min_p", t, 0.0, 1.0);
    }
    if let Some(t) = s.llama.top_k {
        range(&mut p, "llama.top_k", t, 0, 1000);
    }
    if let Some(t) = s.llama.repeat_penalty {
        range(&mut p, "llama.repeat_penalty", t, 0.5, 2.0);
    }
    one_of(&mut p, "doc_mode", &s.doc_mode, &["ask", "embed", "direct"]);
    one_of(&mut p, "decision.device", &s.decision.device, &["auto", "cuda", "cpu"]);
    one_of(&mut p, "decision.backend", &s.decision.backend, &["gguf", "python"]);
    range(&mut p, "decision.threshold", s.decision.threshold, 0.5, 0.99);
    range(&mut p, "decision.budget_ms", s.decision.budget_ms, 100, 10_000);
    if !(s.decision.url.starts_with("http://") || s.decision.url.starts_with("https://")) || reqwest::Url::parse(&s.decision.url).is_err() {
        p.push(("decision.url".into(), "Enter an address such as http://127.0.0.1:47615".into()));
    }
    let repo = &s.decision.model_repo;
    if repo.split('/').count() != 2 || !repo.chars().all(|c| c.is_ascii_alphanumeric() || "-_./".contains(c)) {
        p.push(("decision.model_repo".into(), "Enter a Hugging Face repository such as internlm/Intern-Decision-0.8B".into()));
    }
    one_of(&mut p, "roles.locator_coords", &s.roles.locator_coords, &["auto", "pixels", "norm1000", "norm1"]);
    if let Some(e) = &s.embed_model {
        if !model_ids.contains(e) {
            p.push(("embed_model".into(), "That model no longer exists".into()));
        }
    }
    range(&mut p, "subagents.max_parallel", s.subagents.max_parallel, 1, 8);
    for m in &s.mcp_servers {
        if let Err(e) = mcp_server(m) {
            p.push((format!("mcp:{}", m.name), e));
        }
    }
    if s.system_prompt.chars().count() > 8000 {
        p.push(("system_prompt".into(), "Keep instructions under 8000 characters".into()));
    }
    for (f, id) in [("default_model", &s.default_model), ("fallback_model", &s.fallback_model), ("roles.planner", &s.roles.planner), ("roles.doer", &s.roles.doer), ("roles.eyes", &s.roles.eyes), ("roles.locator", &s.roles.locator)] {
        if let Some(id) = id {
            if !model_ids.contains(id) {
                p.push((f.into(), "That model no longer exists".into()));
            }
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert!(url("https://api.openai.com/v1").is_ok());
        assert!(url("http://127.0.0.1:11434").is_ok());
        assert!(url("http://192.168.1.20:1234/v1").is_ok());
        assert!(url("http://my-pc:11434").is_ok());
        assert!(url("ftp://x").is_err());
        assert!(url("not a url").is_err());
        assert!(url("").is_err());
    }

    #[test]
    fn scopes() {
        assert_eq!(url_scope("http://localhost:11434"), Scope::Loopback);
        assert_eq!(url_scope("http://[::1]:8080"), Scope::Loopback);
        assert_eq!(url_scope("http://172.20.0.5:11434"), Scope::Private);
        assert_eq!(url_scope("http://100.101.1.2:11434"), Scope::Private);
        assert_eq!(url_scope("http://gpu-box:11434"), Scope::Private);
        assert_eq!(url_scope("http://nas.local:1234/v1"), Scope::Private);
        assert_eq!(url_scope("https://api.openai.com/v1"), Scope::Public);
        assert_eq!(url_scope("http://8.8.8.8"), Scope::Public);
    }

    #[test]
    fn shortcuts() {
        assert!(shortcut("Ctrl+Shift+F12").is_ok());
        assert!(shortcut("F12").is_err());
        assert!(shortcut("Ctrl+Shift+Esc").is_err());
        assert!(shortcut("Ctrl+Shift+Nope").is_err());
    }

    #[test]
    fn default_settings_are_valid() {
        let p = settings(&AppSettings::default(), &[]);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn bad_settings_are_reported_per_field() {
        let s = AppSettings { max_steps: 0, accent: "blue".into(), task_cost_cap: 50.0, ..AppSettings::default() };
        let fields: Vec<String> = settings(&s, &[]).into_iter().map(|x| x.0).collect();
        assert!(fields.contains(&"max_steps".to_string()));
        assert!(fields.contains(&"accent".to_string()));
        assert!(fields.contains(&"task_cost_cap".to_string()));
    }
}

//! Folder-based plugins. Each folder in <app data>/plugins holds a plugin.json manifest that
//! declares tools; a tool runs a local program, receives its arguments as JSON on stdin
//! (and in the PLUGIN_ARGS variable) and returns text on stdout.

use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginTool {
    pub name: String,
    pub description: String,
    #[serde(default = "empty_schema")]
    pub parameters: Value,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// "routine" | "risky" | "gate"
    #[serde(default = "risky")]
    pub risk: String,
    #[serde(default = "thirty")]
    pub timeout_secs: u64,
}

fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}})
}
fn risky() -> String {
    "risky".into()
}
fn thirty() -> u64 {
    30
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    pub tools: Vec<PluginTool>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plugin {
    pub id: String,
    pub dir: String,
    pub manifest: Option<Manifest>,
    pub error: Option<String>,
}

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("plugins")
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 48 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn check(m: &Manifest) -> Result<()> {
    if !valid_name(&m.name) {
        bail!("'name' must be 1–48 letters, digits, '-' or '_'");
    }
    if m.tools.is_empty() {
        bail!("Declare at least one tool");
    }
    for t in &m.tools {
        if !valid_name(&t.name) {
            bail!("Tool '{}': name must be letters, digits, '-' or '_'", t.name);
        }
        if t.description.trim().is_empty() {
            bail!("Tool '{}': add a description so models know when to use it", t.name);
        }
        if t.command.trim().is_empty() {
            bail!("Tool '{}': 'command' is empty", t.name);
        }
        if !["routine", "risky", "gate"].contains(&t.risk.as_str()) {
            bail!("Tool '{}': risk must be routine, risky or gate", t.name);
        }
        if t.parameters["type"] != "object" {
            bail!("Tool '{}': parameters must be a JSON schema of type object", t.name);
        }
    }
    Ok(())
}

pub fn scan(data_dir: &Path) -> Vec<Plugin> {
    let root = dir(data_dir);
    let _ = std::fs::create_dir_all(&root);
    let mut out = vec![];
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten().filter(|e| e.path().is_dir()) {
            let id = e.file_name().to_string_lossy().to_string();
            let mf = e.path().join("plugin.json");
            let (manifest, error) = match std::fs::read_to_string(&mf) {
                Err(_) => (None, Some("plugin.json is missing".to_string())),
                Ok(txt) => match serde_json::from_str::<Manifest>(&txt) {
                    Err(err) => (None, Some(format!("plugin.json is invalid: {err}"))),
                    Ok(m) => match check(&m) {
                        Ok(_) => (Some(m), None),
                        Err(err) => (None, Some(err.to_string())),
                    },
                },
            };
            out.push(Plugin { id, dir: e.path().to_string_lossy().to_string(), manifest, error });
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Tool names exposed to models: plugin__<plugin>__<tool>.
pub fn exposed_name(plugin: &str, tool: &str) -> String {
    format!("plugin__{plugin}__{tool}").chars().take(64).collect()
}

pub async fn run(plugin_dir: &Path, tool: &PluginTool, args: &Value) -> Result<String> {
    let input = args.to_string();
    let mut c = crate::sandbox::program_command(&tool.command, &tool.args);
    c.current_dir(plugin_dir)
        .env("PLUGIN_ARGS", &input)
        .env("PLUGIN_DIR", plugin_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = c.spawn().map_err(|e| anyhow!("Could not start '{}': {e}", tool.command))?;
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(input.as_bytes()).await;
    }
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let ro = tokio::spawn(async move {
        let mut b = vec![];
        let _ = so.read_to_end(&mut b).await;
        b
    });
    let re = tokio::spawn(async move {
        let mut b = vec![];
        let _ = se.read_to_end(&mut b).await;
        b
    });
    let status = match tokio::time::timeout(Duration::from_secs(tool.timeout_secs.clamp(1, 600)), child.wait()).await {
        Ok(s) => s?,
        Err(_) => {
            let _ = child.kill().await;
            bail!("The plugin took longer than {}s and was stopped", tool.timeout_secs);
        }
    };
    let out = String::from_utf8_lossy(&ro.await.unwrap_or_default()).to_string();
    let err = String::from_utf8_lossy(&re.await.unwrap_or_default()).to_string();
    if !status.success() {
        bail!("Plugin exited with code {}: {}", status.code().unwrap_or(-1), err.trim().chars().take(1500).collect::<String>());
    }
    Ok(out.chars().take(20000).collect())
}

/// Creates a working example plugin the user can copy and edit.
pub fn create_template(data_dir: &Path, name: &str) -> Result<PathBuf> {
    if !valid_name(name) {
        bail!("Use 1–48 letters, digits, '-' or '_' for the plugin name");
    }
    let d = dir(data_dir).join(name);
    if d.exists() {
        bail!("A plugin named '{name}' already exists");
    }
    std::fs::create_dir_all(&d)?;
    let (command, args, script_name, script) = if cfg!(windows) {
        (
            "powershell",
            vec!["-NoProfile".to_string(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into(), "tool.ps1".into()],
            "tool.ps1",
            "# Receives the tool arguments as JSON on stdin. Whatever you print is returned to the model.\n$in = [Console]::In.ReadToEnd() | ConvertFrom-Json\n$text = [string]$in.text\n\"Words: $(($text -split '\\s+' | Where-Object { $_ }).Count)  Characters: $($text.Length)\"\n",
        )
    } else {
        (
            "sh",
            vec!["tool.sh".to_string()],
            "tool.sh",
            "#!/bin/sh\n# Receives the tool arguments as JSON on stdin. Whatever you print is returned to the model.\ninput=$(cat)\necho \"Received: $input\"\n",
        )
    };
    let manifest = json!({
        "name": name,
        "version": "1.0.0",
        "description": "Example plugin: counts words in a text. Edit tool script and this file to build your own.",
        "author": "",
        "tools": [{
            "name": "count_words",
            "description": "Counts the words and characters in a piece of text.",
            "parameters": {"type": "object", "properties": {"text": {"type": "string", "description": "The text to count"}}, "required": ["text"]},
            "command": command,
            "args": args,
            "risk": "routine",
            "timeout_secs": 20
        }]
    });
    std::fs::write(d.join("plugin.json"), serde_json::to_string_pretty(&manifest)?)?;
    std::fs::write(d.join(script_name), script)?;
    std::fs::write(
        d.join("README.md"),
        "# Plugin\n\n`plugin.json` declares tools. Each tool runs `command` with `args` inside this folder.\n\n- Arguments arrive as JSON on stdin and in the `PLUGIN_ARGS` environment variable.\n- Print the result to stdout. A non-zero exit code is reported as an error.\n- `risk`: `routine` (runs freely), `risky` (asks in Balanced mode) or `gate` (always asks).\n- `parameters` is a JSON Schema object describing the arguments.\n",
    )?;
    Ok(d)
}

/// Copies a plugin folder chosen by the user into the plugins directory.
pub fn import(data_dir: &Path, src: &Path) -> Result<String> {
    let mf = src.join("plugin.json");
    let m: Manifest = serde_json::from_str(&std::fs::read_to_string(&mf).map_err(|_| anyhow!("That folder has no plugin.json"))?)?;
    check(&m)?;
    let dest = dir(data_dir).join(&m.name);
    if dest.exists() {
        bail!("A plugin named '{}' is already installed", m.name);
    }
    copy_dir(src, &dest)?;
    Ok(m.name)
}

fn copy_dir(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let t = to.join(e.file_name());
        if e.path().is_dir() {
            copy_dir(&e.path(), &t)?;
        } else {
            std::fs::copy(e.path(), t)?;
        }
    }
    Ok(())
}

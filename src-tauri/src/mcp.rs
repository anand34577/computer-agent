//! Model Context Protocol client: user-added servers over stdio (a local program) or
//! streamable HTTP. Server descriptions and results are treated as untrusted data.

use crate::settings::McpServer;
use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::oneshot;

const PROTOCOL: &str = "2025-06-18";

#[derive(Clone, Debug, Serialize)]
pub struct McpTool {
    pub server_id: String,
    pub server_name: String,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

// One connection per server, so the size difference between the two kinds does not matter.
#[allow(clippy::large_enum_variant)]
enum Transport {
    Stdio {
        stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
        pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>>,
        _child: tokio::sync::Mutex<tokio::process::Child>,
        stderr_tail: Arc<Mutex<String>>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        session: Mutex<Option<String>>,
        http: reqwest::Client,
    },
}

pub struct Conn {
    transport: Transport,
    next_id: AtomicU64,
    pub tools: Mutex<Vec<McpTool>>,
}

#[derive(Default)]
pub struct Manager {
    conns: Mutex<HashMap<String, Arc<Conn>>>,
    errors: Mutex<HashMap<String, String>>,
}

impl Conn {
    async fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let resp = match &self.transport {
            Transport::Stdio { stdin, pending, stderr_tail, .. } => {
                let (tx, rx) = oneshot::channel();
                pending.lock().insert(id, tx);
                {
                    let mut w = stdin.lock().await;
                    w.write_all(format!("{msg}\n").as_bytes()).await.map_err(|e| anyhow!("Server stopped: {e}"))?;
                    w.flush().await?;
                }
                match tokio::time::timeout(timeout, rx).await {
                    Ok(Ok(v)) => v,
                    Ok(Err(_)) => {
                        let tail = stderr_tail.lock().clone();
                        bail!("The server closed the connection.{}", if tail.is_empty() { String::new() } else { format!(" Its last output: {}", tail.chars().rev().take(400).collect::<String>().chars().rev().collect::<String>()) })
                    }
                    Err(_) => {
                        pending.lock().remove(&id);
                        bail!("The server did not answer '{method}' within {}s", timeout.as_secs())
                    }
                }
            }
            Transport::Http { url, headers, session, http } => {
                let mut rb = http.post(url).header("Content-Type", "application/json").header("Accept", "application/json, text/event-stream").header("MCP-Protocol-Version", PROTOCOL).timeout(timeout).json(&msg);
                for (k, v) in headers {
                    rb = rb.header(k, v);
                }
                if let Some(s) = session.lock().clone() {
                    rb = rb.header("Mcp-Session-Id", s);
                }
                let r = rb.send().await.map_err(|e| anyhow!("Could not reach the server: {e}"))?;
                if let Some(s) = r.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
                    *session.lock() = Some(s.to_string());
                }
                if !r.status().is_success() {
                    let st = r.status();
                    let body: String = r.text().await.unwrap_or_default().chars().take(300).collect();
                    bail!("Server returned {st}{}: {body}", if st.as_u16() == 401 { " (check the authorization header)" } else { "" });
                }
                let is_sse = r.headers().get("content-type").and_then(|v| v.to_str().ok()).map(|v| v.contains("event-stream")).unwrap_or(false);
                if is_sse {
                    let mut stream = r.bytes_stream();
                    let mut buf = String::new();
                    let mut found = None;
                    'outer: while let Some(chunk) = stream.next().await {
                        buf.push_str(&String::from_utf8_lossy(&chunk?));
                        while let Some(pos) = buf.find('\n') {
                            let line = buf[..pos].trim().to_string();
                            buf.drain(..=pos);
                            if let Some(d) = line.strip_prefix("data:") {
                                if let Ok(v) = serde_json::from_str::<Value>(d.trim()) {
                                    if v["id"].as_u64() == Some(id) {
                                        found = Some(v);
                                        break 'outer;
                                    }
                                }
                            }
                        }
                    }
                    found.ok_or_else(|| anyhow!("No response in the event stream"))?
                } else {
                    r.json().await?
                }
            }
        };
        if let Some(err) = resp.get("error").filter(|e| !e.is_null()) {
            bail!("{}", err["message"].as_str().unwrap_or("MCP error"));
        }
        Ok(resp["result"].clone())
    }

    async fn notify(&self, method: &str) {
        let msg = json!({"jsonrpc": "2.0", "method": method});
        match &self.transport {
            Transport::Stdio { stdin, .. } => {
                let mut w = stdin.lock().await;
                let _ = w.write_all(format!("{msg}\n").as_bytes()).await;
                let _ = w.flush().await;
            }
            Transport::Http { url, headers, session, http } => {
                let mut rb = http.post(url).header("Content-Type", "application/json").header("Accept", "application/json, text/event-stream").json(&msg);
                for (k, v) in headers {
                    rb = rb.header(k, v);
                }
                if let Some(s) = session.lock().clone() {
                    rb = rb.header("Mcp-Session-Id", s);
                }
                let _ = rb.send().await;
            }
        }
    }
}

fn secret_headers(server_id: &str) -> Vec<(String, String)> {
    // Stored as "Header-Name: value" lines in the OS credential store.
    crate::secrets::get(&format!("mcp:{server_id}"))
        .map(|s| {
            s.lines()
                .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                .filter(|(k, _)| !k.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub async fn connect(cfg: &McpServer, local_only: bool) -> Result<Arc<Conn>> {
    let transport = match cfg.transport.as_str() {
        "http" => {
            crate::validate::url(&cfg.url).map_err(|e| anyhow!("Address: {e}"))?;
            if local_only && !crate::validate::is_local_url(&cfg.url) {
                bail!("Local-only mode is on, so remote MCP servers are blocked");
            }
            Transport::Http {
                url: cfg.url.trim().to_string(),
                headers: secret_headers(&cfg.id),
                session: Mutex::new(None),
                http: reqwest::Client::builder().connect_timeout(Duration::from_secs(10)).build()?,
            }
        }
        _ => {
            if cfg.command.trim().is_empty() {
                bail!("No command set");
            }
            let mut c = crate::sandbox::program_command(cfg.command.trim(), &cfg.args);
            for (k, v) in &cfg.env {
                c.env(k, v);
            }
            // Secret environment values (e.g. API tokens) live in the credential store as KEY=value lines.
            if let Some(sec) = crate::secrets::get(&format!("mcp-env:{}", cfg.id)) {
                for l in sec.lines() {
                    if let Some((k, v)) = l.split_once('=') {
                        c.env(k.trim(), v.trim());
                    }
                }
            }
            c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
            let mut child = c.spawn().map_err(|e| anyhow!("Could not start '{}': {e}. Is it installed and on PATH?", cfg.command))?;
            let stdin = child.stdin.take().ok_or_else(|| anyhow!("no stdin"))?;
            let stdout = child.stdout.take().ok_or_else(|| anyhow!("no stdout"))?;
            let stderr = child.stderr.take().ok_or_else(|| anyhow!("no stderr"))?;
            let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Value>>>> = Arc::new(Mutex::new(HashMap::new()));
            let p2 = pending.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stdout).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if let Ok(v) = serde_json::from_str::<Value>(&line) {
                        if let Some(id) = v["id"].as_u64() {
                            if let Some(tx) = p2.lock().remove(&id) {
                                let _ = tx.send(v);
                            }
                        }
                    }
                }
                p2.lock().clear();
            });
            let tail = Arc::new(Mutex::new(String::new()));
            let t2 = tail.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let mut t = t2.lock();
                    t.push_str(&line);
                    t.push('\n');
                    if t.len() > 4000 {
                        let cut = t.len() - 2000;
                        let mut c = cut;
                        while !t.is_char_boundary(c) {
                            c += 1;
                        }
                        t.drain(..c);
                    }
                }
            });
            Transport::Stdio { stdin: tokio::sync::Mutex::new(stdin), pending, _child: tokio::sync::Mutex::new(child), stderr_tail: tail }
        }
    };
    let conn = Arc::new(Conn { transport, next_id: AtomicU64::new(1), tools: Mutex::new(vec![]) });
    conn.request(
        "initialize",
        json!({"protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": {"name": "computer-agent", "version": env!("CARGO_PKG_VERSION")}}),
        Duration::from_secs(45),
    )
    .await?;
    conn.notify("notifications/initialized").await;
    let mut tools = vec![];
    let mut cursor: Option<String> = None;
    for _ in 0..20 {
        let params = match &cursor {
            Some(c) => json!({"cursor": c}),
            None => json!({}),
        };
        let r = conn.request("tools/list", params, Duration::from_secs(30)).await?;
        for t in r["tools"].as_array().cloned().unwrap_or_default() {
            tools.push(McpTool {
                server_id: cfg.id.clone(),
                server_name: cfg.name.clone(),
                name: t["name"].as_str().unwrap_or("").to_string(),
                description: t["description"].as_str().unwrap_or("").chars().take(1000).collect(),
                input_schema: if t["inputSchema"].is_object() { t["inputSchema"].clone() } else { json!({"type":"object","properties":{}}) },
            });
        }
        cursor = r["nextCursor"].as_str().map(String::from);
        if cursor.is_none() {
            break;
        }
    }
    *conn.tools.lock() = tools;
    Ok(conn)
}

/// Tool names exposed to models: mcp__<server>__<tool>, limited to 64 safe characters.
pub fn exposed_name(server: &str, tool: &str) -> String {
    let clean = |s: &str| s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect::<String>();
    let n = format!("mcp__{}__{}", clean(server), clean(tool));
    n.chars().take(64).collect()
}

impl Manager {
    pub async fn ensure(&self, cfg: &McpServer, local_only: bool) -> Result<Arc<Conn>> {
        if let Some(c) = self.conns.lock().get(&cfg.id).cloned() {
            return Ok(c);
        }
        match connect(cfg, local_only).await {
            Ok(c) => {
                self.conns.lock().insert(cfg.id.clone(), c.clone());
                self.errors.lock().remove(&cfg.id);
                Ok(c)
            }
            Err(e) => {
                self.errors.lock().insert(cfg.id.clone(), e.to_string());
                Err(e)
            }
        }
    }

    pub fn drop_conn(&self, id: &str) {
        self.conns.lock().remove(id);
        self.errors.lock().remove(id);
    }

    pub fn status(&self, id: &str) -> (bool, Option<String>, usize) {
        let c = self.conns.lock().get(id).cloned();
        (c.is_some(), self.errors.lock().get(id).cloned(), c.map(|c| c.tools.lock().len()).unwrap_or(0))
    }

    /// Connects to every enabled server (in parallel) and returns their tools.
    pub async fn all_tools(&self, servers: &[McpServer], local_only: bool) -> Vec<McpTool> {
        let futs = servers.iter().filter(|s| s.enabled && s.permission != "off").map(|s| async move {
            match tokio::time::timeout(Duration::from_secs(50), self.ensure(s, local_only)).await {
                Ok(Ok(c)) => c.tools.lock().clone(),
                _ => vec![],
            }
        });
        futures_util::future::join_all(futs).await.into_iter().flatten().collect()
    }

    pub async fn call(&self, cfg: &McpServer, tool: &str, args: &Value, local_only: bool) -> Result<String> {
        let conn = self.ensure(cfg, local_only).await?;
        let r = match conn.request("tools/call", json!({"name": tool, "arguments": args}), Duration::from_secs(120)).await {
            Ok(r) => r,
            Err(e) => {
                // A dead stdio server is restarted once on the next call.
                self.drop_conn(&cfg.id);
                return Err(e);
            }
        };
        let mut out = String::new();
        for c in r["content"].as_array().cloned().unwrap_or_default() {
            match c["type"].as_str() {
                Some("text") => out.push_str(c["text"].as_str().unwrap_or("")),
                Some("image") => out.push_str("[image returned]"),
                Some("resource") => out.push_str(c["resource"]["text"].as_str().unwrap_or("[resource]")),
                _ => {}
            }
            out.push('\n');
        }
        if out.trim().is_empty() && !r["structuredContent"].is_null() {
            out = r["structuredContent"].to_string();
        }
        if r["isError"].as_bool().unwrap_or(false) {
            bail!("{}", out.trim());
        }
        Ok(out.chars().take(20000).collect())
    }
}

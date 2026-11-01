//! Provider connections. Kept separate from the rest of the engine so an API change
//! only touches this file. Two wire formats cover every source: OpenAI-compatible
//! (OpenAI, routers, Ollama, LM Studio, llama.cpp, Jan, vLLM) and Anthropic.

use anyhow::{anyhow, bail, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProviderCfg {
    pub id: String,
    pub kind: String, // "openai" | "anthropic" | "ollama"
    pub name: String,
    pub base_url: String,
}

impl ProviderCfg {
    pub fn is_local(&self) -> bool {
        crate::validate::is_local_url(&self.base_url)
    }

    pub fn openai_base(&self) -> String {
        let b = self.base_url.trim_end_matches('/');
        if self.kind == "ollama" {
            format!("{}/v1", ollama_root(b))
        } else {
            b.to_string()
        }
    }
}

/// Ollama's native root, whether the user typed it with /v1 or /api or not.
pub fn ollama_root(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    b.strip_suffix("/v1").or_else(|| b.strip_suffix("/api")).unwrap_or(b).to_string()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Part {
    Text { text: String },
    Image { media_type: String, data: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Msg {
    pub role: String, // system | user | assistant | tool
    pub parts: Vec<Part>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}

impl Msg {
    pub fn text(role: &str, s: impl Into<String>) -> Self {
        Msg { role: role.into(), parts: vec![Part::Text { text: s.into() }], ..Default::default() }
    }
    pub fn text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| if let Part::Text { text } = p { Some(text.as_str()) } else { None })
            .collect::<Vec<_>>()
            .join("\n")
    }
    pub fn has_images(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, Part::Image { .. }))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Clone, Debug)]
pub struct Request {
    pub model: String,
    pub messages: Vec<Msg>,
    pub tools: Vec<ToolDef>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    /// "low" | "medium" | "high" asks reasoning models to think; None leaves the provider default.
    pub reasoning: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Response {
    pub text: String,
    pub reasoning: String,
    pub tool_calls: Vec<ToolCall>,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

pub enum Delta {
    Text(String),
    Reasoning(String),
}

pub fn check_local_only(p: &ProviderCfg, local_only: bool) -> Result<()> {
    if local_only && !p.is_local() {
        bail!("Local-only mode is on, so '{}' (a cloud provider) is blocked. Turn off local-only mode or pick a local model.", p.name);
    }
    Ok(())
}

/// The built-in engine serves one model at a time: load the requested one first.
async fn prepare(p: &ProviderCfg, req: &Request) -> Result<()> {
    if p.kind == "llamacpp" {
        let l = crate::llamacpp::global().ok_or_else(|| anyhow!("The built-in engine is not ready"))?;
        l.ensure(&req.model).await?;
    }
    Ok(())
}

pub async fn complete(http: &reqwest::Client, p: &ProviderCfg, key: Option<String>, req: &Request, local_only: bool) -> Result<Response> {
    check_local_only(p, local_only)?;
    prepare(p, req).await?;
    let mut sink = |_d: Delta| {};
    match p.kind.as_str() {
        "anthropic" => anthropic(http, p, key, req, false, &mut sink).await,
        _ => openai(http, p, key, req, false, &mut sink).await,
    }
}

pub async fn stream(
    http: &reqwest::Client,
    p: &ProviderCfg,
    key: Option<String>,
    req: &Request,
    local_only: bool,
    on: &mut (dyn FnMut(Delta) + Send),
) -> Result<Response> {
    check_local_only(p, local_only)?;
    prepare(p, req).await?;
    match p.kind.as_str() {
        "anthropic" => anthropic(http, p, key, req, true, on).await,
        _ => openai(http, p, key, req, true, on).await,
    }
}

/// Retries rate limits, timeouts and server errors a limited number of times.
async fn send_with_retry(rb: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    let mut delay = 1500u64;
    for attempt in 0..3 {
        let r = rb.try_clone().ok_or_else(|| anyhow!("request not cloneable"))?.send().await;
        match r {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let status = resp.status();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt < 2 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                    delay *= 2;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                let body: String = body.chars().take(600).collect();
                let hint = match status.as_u16() {
                    401 | 403 => " (check the API key)",
                    404 => " (check the base address and model name)",
                    429 => " (rate limited; try again shortly)",
                    _ => "",
                };
                bail!("Provider returned {status}{hint}: {body}");
            }
            Err(e) if (e.is_timeout() || e.is_connect()) && attempt < 2 => {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                delay *= 2;
            }
            Err(e) => {
                if e.is_connect() {
                    bail!("Could not connect to the provider. Is it running and reachable? ({e})");
                }
                bail!("Request failed: {e}");
            }
        }
    }
    bail!("Provider did not respond after retries")
}

// ---------------- OpenAI-compatible ----------------

fn openai_messages(msgs: &[Msg]) -> Vec<Value> {
    msgs.iter()
        .map(|m| match m.role.as_str() {
            "tool" => json!({"role":"tool","tool_call_id": m.tool_call_id.clone().unwrap_or_default(),"content": m.text_content()}),
            "assistant" => {
                let mut v = json!({"role":"assistant","content": m.text_content()});
                if !m.tool_calls.is_empty() {
                    v["tool_calls"] = Value::Array(
                        m.tool_calls
                            .iter()
                            .map(|t| json!({"id": t.id, "type":"function","function":{"name": t.name, "arguments": t.arguments.to_string()}}))
                            .collect(),
                    );
                }
                v
            }
            role => {
                if m.has_images() {
                    let parts: Vec<Value> = m
                        .parts
                        .iter()
                        .map(|p| match p {
                            Part::Text { text } => json!({"type":"text","text": text}),
                            Part::Image { media_type, data } => {
                                json!({"type":"image_url","image_url":{"url": format!("data:{media_type};base64,{data}")}})
                            }
                        })
                        .collect();
                    json!({"role": role, "content": parts})
                } else {
                    json!({"role": role, "content": m.text_content()})
                }
            }
        })
        .collect()
}

fn parse_args(s: &str) -> Value {
    if s.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str(s).unwrap_or_else(|_| json!({"_raw": s}))
}

async fn openai(
    http: &reqwest::Client,
    p: &ProviderCfg,
    key: Option<String>,
    req: &Request,
    streaming: bool,
    on: &mut (dyn FnMut(Delta) + Send),
) -> Result<Response> {
    let url = format!("{}/chat/completions", p.openai_base());
    let mut body = json!({
        "model": req.model,
        "messages": openai_messages(&req.messages),
        "stream": streaming,
    });
    if p.base_url.contains("api.openai.com") {
        body["max_completion_tokens"] = json!(req.max_tokens);
    } else {
        body["max_tokens"] = json!(req.max_tokens);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    let extra = reasoning_fields(p, &req.model, req.reasoning.as_deref());
    for (k, v) in &extra {
        body[*k] = v.clone();
    }
    if streaming {
        body["stream_options"] = json!({"include_usage": true});
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools
                .iter()
                .map(|t| json!({"type":"function","function":{"name": t.name, "description": t.description, "parameters": t.parameters}}))
                .collect(),
        );
    }
    let build = |body: &Value| {
        let mut rb = http.post(&url).json(body);
        if let Some(k) = key.clone().filter(|k| !k.is_empty()) {
            rb = rb.bearer_auth(k);
        }
        if p.base_url.contains("openrouter.ai") {
            rb = rb.header("X-Title", "Computer Agent");
        }
        rb
    };
    let resp = match send_with_retry(build(&body)).await {
        // Servers that do not know a reasoning field may reject the request; ask again without it.
        Err(e) if !extra.is_empty() && is_bad_request(&e) => {
            if let Some(o) = body.as_object_mut() {
                for (k, _) in &extra {
                    o.remove(*k);
                }
            }
            send_with_retry(build(&body)).await?
        }
        r => r?,
    };
    let mut out = Response::default();

    if !streaming {
        let v: Value = resp.json().await?;
        let msg = &v["choices"][0]["message"];
        out.text = msg["content"].as_str().unwrap_or("").to_string();
        out.reasoning = msg["reasoning_content"].as_str().or(msg["reasoning"].as_str()).unwrap_or("").to_string();
        if let Some(tcs) = msg["tool_calls"].as_array() {
            for t in tcs {
                out.tool_calls.push(ToolCall {
                    id: t["id"].as_str().unwrap_or("call_0").to_string(),
                    name: t["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments: match &t["function"]["arguments"] {
                        Value::String(s) => parse_args(s),
                        Value::Object(_) => t["function"]["arguments"].clone(),
                        _ => json!({}),
                    },
                });
            }
        }
        out.input_tokens = v["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
        out.output_tokens = v["usage"]["completion_tokens"].as_u64().unwrap_or(0);
        split_think(&mut out);
        return Ok(out);
    }

    let mut partial: Vec<(String, String, String)> = vec![]; // id, name, args
    let mut think = ThinkSplit::default();
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let data = data.trim();
            if data == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(data) else { continue };
            if let Some(u) = v.get("usage").filter(|u| !u.is_null()) {
                out.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(out.input_tokens);
                out.output_tokens = u["completion_tokens"].as_u64().unwrap_or(out.output_tokens);
            }
            let d = &v["choices"][0]["delta"];
            if let Some(t) = d["content"].as_str().filter(|t| !t.is_empty()) {
                out.text.push_str(t);
                think.push(t, on);
            }
            if let Some(t) = d["reasoning_content"].as_str().or(d["reasoning"].as_str()).filter(|t| !t.is_empty()) {
                out.reasoning.push_str(t);
                on(Delta::Reasoning(t.to_string()));
            }
            if let Some(tcs) = d["tool_calls"].as_array() {
                for t in tcs {
                    let idx = t["index"].as_u64().unwrap_or(0) as usize;
                    while partial.len() <= idx {
                        partial.push((String::new(), String::new(), String::new()));
                    }
                    if let Some(id) = t["id"].as_str() {
                        partial[idx].0 = id.to_string();
                    }
                    if let Some(n) = t["function"]["name"].as_str() {
                        partial[idx].1.push_str(n);
                    }
                    if let Some(a) = t["function"]["arguments"].as_str() {
                        partial[idx].2.push_str(a);
                    }
                }
            }
        }
    }
    think.finish(on);
    for (i, (id, name, args)) in partial.into_iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        out.tool_calls.push(ToolCall { id: if id.is_empty() { format!("call_{i}") } else { id }, name, arguments: parse_args(&args) });
    }
    split_think(&mut out);
    Ok(out)
}

fn is_bad_request(e: &anyhow::Error) -> bool {
    let m = e.to_string();
    m.contains("returned 400") || m.contains("returned 422")
}

/// Request fields that turn a model's thinking on, off or to a level, in the dialect each
/// kind of server understands. `level`: None (provider default), "on", "off", "low", "medium", "high".
pub fn reasoning_fields(p: &ProviderCfg, model: &str, level: Option<&str>) -> Vec<(&'static str, Value)> {
    let Some(level) = level else { return vec![] };
    let graded = ["low", "medium", "high"].contains(&level);
    let base = p.base_url.to_lowercase();
    let model = model.to_lowercase();
    if base.contains("api.openai.com") {
        // OpenAI: only reasoning_effort. GPT-5.1+ accept "none", GPT-5 "minimal"; o-series cannot switch off.
        return match level {
            "off" if model.starts_with("gpt-5.") => vec![("reasoning_effort", json!("none"))],
            "off" if model.starts_with("gpt-5") => vec![("reasoning_effort", json!("minimal"))],
            "off" if model.starts_with('o') => vec![("reasoning_effort", json!("low"))],
            l if graded => vec![("reasoning_effort", json!(l))],
            _ => vec![],
        };
    }
    if base.contains("openrouter.ai") {
        return vec![("reasoning", match level {
            "off" => json!({"enabled": false}),
            "on" => json!({"enabled": true}),
            l => json!({"effort": l}),
        })];
    }
    let local = p.kind == "ollama" || p.kind == "llamacpp" || p.is_local();
    if !local {
        // Other hosted OpenAI-compatible APIs (Gemini, Groq, Mistral, Together…).
        return match level {
            "off" => vec![("reasoning_effort", json!("none"))],
            l if graded => vec![("reasoning_effort", json!(l))],
            _ => vec![],
        };
    }
    // Local servers. llama.cpp, vLLM, SGLang and LM Studio pass chat_template_kwargs to the
    // model's template (Qwen3 reads enable_thinking, DeepSeek reads thinking); Ollama reads think.
    let on = level != "off";
    let mut v = vec![("chat_template_kwargs", json!({"enable_thinking": on, "thinking": on}))];
    if p.kind == "ollama" {
        v.push(("think", json!(if graded { json!(level) } else { json!(on) })));
    } else if graded {
        v.push(("reasoning_effort", json!(level)));
    } else if !on {
        v.push(("reasoning_effort", json!("none")));
    }
    v
}

/// Routes inline <think>…</think> reasoning (DeepSeek-R1, QwQ and similar local models) to the
/// reasoning stream as it arrives. Only a tag at the start of the reply counts, so a reply that
/// merely talks about "<think>" is left alone. Tags split across chunks are held back until whole.
#[derive(Default)]
struct ThinkSplit {
    inside: bool,
    seen_text: bool,
    pending: String,
}

impl ThinkSplit {
    fn push(&mut self, t: &str, on: &mut (dyn FnMut(Delta) + Send)) {
        self.pending.push_str(t);
        loop {
            if !self.inside && self.seen_text {
                let all = std::mem::take(&mut self.pending);
                self.emit(all, on);
                return;
            }
            let tag = if self.inside { "</think>" } else { "<think>" };
            if let Some(i) = self.pending.find(tag) {
                let before = self.pending[..i].to_string();
                if !self.inside && !before.trim().is_empty() {
                    // Text came first: this is not a reasoning block.
                    self.seen_text = true;
                    continue;
                }
                self.emit(before, on);
                self.pending.drain(..i + tag.len());
                self.inside = !self.inside;
                continue;
            }
            let keep = (1..tag.len()).rev().find(|&k| self.pending.ends_with(&tag[..k])).unwrap_or(0);
            let cut = self.pending.len() - keep;
            let out: String = self.pending.drain(..cut).collect();
            if !self.inside && out.trim().is_empty() && keep > 0 {
                // Only whitespace before a possible opening tag: wait for more.
                self.pending.insert_str(0, &out);
                return;
            }
            self.emit(out, on);
            return;
        }
    }

    fn emit(&mut self, s: String, on: &mut (dyn FnMut(Delta) + Send)) {
        if s.is_empty() {
            return;
        }
        if self.inside {
            on(Delta::Reasoning(s));
        } else {
            if !s.trim().is_empty() {
                self.seen_text = true;
            }
            on(Delta::Text(s));
        }
    }

    fn finish(&mut self, on: &mut (dyn FnMut(Delta) + Send)) {
        let rest = std::mem::take(&mut self.pending);
        self.emit(rest, on);
    }
}

/// Some local models put their reasoning inline as <think>...</think>.
fn split_think(out: &mut Response) {
    if let Some(start) = out.text.find("<think>").filter(|s| out.text[..*s].trim().is_empty()) {
        if let Some(end) = out.text.find("</think>") {
            if end > start {
                let thought = out.text[start + 7..end].trim().to_string();
                let rest = format!("{}{}", &out.text[..start], &out.text[end + 8..]);
                if out.reasoning.is_empty() {
                    out.reasoning = thought;
                }
                out.text = rest.trim().to_string();
            }
        }
    }
}

// ---------------- Anthropic ----------------

fn anthropic_messages(msgs: &[Msg]) -> (String, Vec<Value>) {
    let mut system = String::new();
    let mut out: Vec<Value> = vec![];
    for m in msgs {
        let (role, blocks): (&str, Vec<Value>) = match m.role.as_str() {
            "system" => {
                if !system.is_empty() {
                    system.push_str("\n\n");
                }
                system.push_str(&m.text_content());
                continue;
            }
            "tool" => (
                "user",
                vec![json!({"type":"tool_result","tool_use_id": m.tool_call_id.clone().unwrap_or_default(),"content": m.text_content()})],
            ),
            "assistant" => {
                let mut b = vec![];
                let t = m.text_content();
                if !t.trim().is_empty() {
                    b.push(json!({"type":"text","text": t}));
                }
                for tc in &m.tool_calls {
                    let input = if tc.arguments.is_object() { tc.arguments.clone() } else { json!({}) };
                    b.push(json!({"type":"tool_use","id": tc.id,"name": tc.name,"input": input}));
                }
                if b.is_empty() {
                    b.push(json!({"type":"text","text":"(no reply)"}));
                }
                ("assistant", b)
            }
            _ => (
                "user",
                m.parts
                    .iter()
                    .map(|p| match p {
                        Part::Text { text } => json!({"type":"text","text": text}),
                        Part::Image { media_type, data } => {
                            json!({"type":"image","source":{"type":"base64","media_type": media_type,"data": data}})
                        }
                    })
                    .collect(),
            ),
        };
        if let Some(last) = out.last_mut() {
            if last["role"] == role {
                if let Some(arr) = last["content"].as_array_mut() {
                    arr.extend(blocks);
                    continue;
                }
            }
        }
        out.push(json!({"role": role, "content": blocks}));
    }
    (system, out)
}

async fn anthropic(
    http: &reqwest::Client,
    p: &ProviderCfg,
    key: Option<String>,
    req: &Request,
    streaming: bool,
    on: &mut (dyn FnMut(Delta) + Send),
) -> Result<Response> {
    let base = p.base_url.trim_end_matches('/');
    let url = if base.ends_with("/v1") { format!("{base}/messages") } else { format!("{base}/v1/messages") };
    let (system, messages) = anthropic_messages(&req.messages);
    let mut body = json!({"model": req.model, "max_tokens": req.max_tokens, "messages": messages, "stream": streaming});
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if let Some(t) = req.temperature {
        body["temperature"] = json!(t);
    }
    if let Some(budget) = match req.reasoning.as_deref() {
        Some("low") => Some(2048u32),
        Some("medium") => Some(8192),
        Some("high") => Some(16000),
        _ => None,
    } {
        // Extended thinking needs room above the budget and no custom temperature.
        body["thinking"] = json!({"type": "enabled", "budget_tokens": budget});
        body["max_tokens"] = json!(req.max_tokens.max(budget + 4096));
        if let Some(o) = body.as_object_mut() {
            o.remove("temperature");
        }
    }
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(
            req.tools.iter().map(|t| json!({"name": t.name,"description": t.description,"input_schema": t.parameters})).collect(),
        );
    }
    let rb = http
        .post(&url)
        .header("x-api-key", key.unwrap_or_default())
        .header("anthropic-version", "2023-06-01")
        .json(&body);
    let resp = send_with_retry(rb).await?;
    let mut out = Response::default();
    if !streaming {
        let v: Value = resp.json().await?;
        for b in v["content"].as_array().cloned().unwrap_or_default() {
            match b["type"].as_str() {
                Some("text") => out.text.push_str(b["text"].as_str().unwrap_or("")),
                Some("thinking") => out.reasoning.push_str(b["thinking"].as_str().unwrap_or("")),
                Some("tool_use") => out.tool_calls.push(ToolCall {
                    id: b["id"].as_str().unwrap_or("").to_string(),
                    name: b["name"].as_str().unwrap_or("").to_string(),
                    arguments: b["input"].clone(),
                }),
                _ => {}
            }
        }
        out.input_tokens = v["usage"]["input_tokens"].as_u64().unwrap_or(0);
        out.output_tokens = v["usage"]["output_tokens"].as_u64().unwrap_or(0);
        return Ok(out);
    }
    let mut blocks: Vec<(String, String, String, String)> = vec![]; // type, id, name, json
    let mut buf = String::new();
    let mut s = resp.bytes_stream();
    while let Some(chunk) = s.next().await {
        buf.push_str(&String::from_utf8_lossy(&chunk?));
        while let Some(pos) = buf.find('\n') {
            let line = buf[..pos].trim().to_string();
            buf.drain(..=pos);
            let Some(data) = line.strip_prefix("data:") else { continue };
            let Ok(v) = serde_json::from_str::<Value>(data.trim()) else { continue };
            match v["type"].as_str() {
                Some("message_start") => out.input_tokens = v["message"]["usage"]["input_tokens"].as_u64().unwrap_or(0),
                Some("content_block_start") => {
                    let cb = &v["content_block"];
                    blocks.push((
                        cb["type"].as_str().unwrap_or("").into(),
                        cb["id"].as_str().unwrap_or("").into(),
                        cb["name"].as_str().unwrap_or("").into(),
                        String::new(),
                    ));
                }
                Some("content_block_delta") => {
                    let d = &v["delta"];
                    match d["type"].as_str() {
                        Some("text_delta") => {
                            let t = d["text"].as_str().unwrap_or("");
                            out.text.push_str(t);
                            on(Delta::Text(t.into()));
                        }
                        Some("thinking_delta") => {
                            let t = d["thinking"].as_str().unwrap_or("");
                            out.reasoning.push_str(t);
                            on(Delta::Reasoning(t.into()));
                        }
                        Some("input_json_delta") => {
                            if let Some(b) = blocks.last_mut() {
                                b.3.push_str(d["partial_json"].as_str().unwrap_or(""));
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => out.output_tokens = v["usage"]["output_tokens"].as_u64().unwrap_or(out.output_tokens),
                Some("error") => bail!("Provider error: {}", v["error"]["message"].as_str().unwrap_or("unknown")),
                _ => {}
            }
        }
    }
    for (t, id, name, js) in blocks {
        if t == "tool_use" {
            out.tool_calls.push(ToolCall { id, name, arguments: parse_args(&js) });
        }
    }
    Ok(out)
}

// ---------------- Listing models ----------------

#[derive(Serialize, Clone, Debug)]
pub struct RemoteModel {
    pub name: String,
    pub size: u64,
}

pub async fn list_models(http: &reqwest::Client, p: &ProviderCfg, key: Option<String>) -> Result<Vec<RemoteModel>> {
    let base = p.base_url.trim_end_matches('/');
    match p.kind.as_str() {
        "llamacpp" => Ok(crate::llamacpp::global().map(|l| l.model_names()).unwrap_or_default().into_iter().map(|(name, size)| RemoteModel { name, size }).collect()),
        "ollama" => {
            let base = ollama_root(base);
            let v: Value = send_with_retry(http.get(format!("{base}/api/tags"))).await?.json().await?;
            Ok(v["models"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|m| RemoteModel { name: m["name"].as_str().unwrap_or("").into(), size: m["size"].as_u64().unwrap_or(0) })
                .collect())
        }
        "anthropic" => {
            let url = if base.ends_with("/v1") { format!("{base}/models?limit=100") } else { format!("{base}/v1/models?limit=100") };
            let rb = http.get(url).header("x-api-key", key.unwrap_or_default()).header("anthropic-version", "2023-06-01");
            let v: Value = send_with_retry(rb).await?.json().await?;
            Ok(v["data"].as_array().cloned().unwrap_or_default().iter().map(|m| RemoteModel { name: m["id"].as_str().unwrap_or("").into(), size: 0 }).collect())
        }
        _ => {
            let mut rb = http.get(format!("{base}/models"));
            if let Some(k) = key.filter(|k| !k.is_empty()) {
                rb = rb.bearer_auth(k);
            }
            let v: Value = send_with_retry(rb).await?.json().await?;
            let mut names: Vec<RemoteModel> = v["data"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|m| RemoteModel { name: m["id"].as_str().unwrap_or("").into(), size: 0 })
                .collect();
            names.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(names)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&str]) -> (String, String) {
        let (mut text, mut reason) = (String::new(), String::new());
        {
            let mut on = |d: Delta| match d {
                Delta::Text(t) => text.push_str(&t),
                Delta::Reasoning(t) => reason.push_str(&t),
            };
            let mut t = ThinkSplit::default();
            for c in chunks {
                t.push(c, &mut on);
            }
            t.finish(&mut on);
        }
        (text, reason)
    }

    #[test]
    fn think_tags_split_across_chunks() {
        let (t, r) = run(&["<thi", "nk>let me ", "see</th", "ink>The answer", " is 4."]);
        assert_eq!(r, "let me see");
        assert_eq!(t, "The answer is 4.");
    }

    #[test]
    fn think_tag_mid_text_is_kept() {
        let (t, r) = run(&["Use the ", "<think> tag like this"]);
        assert_eq!(r, "");
        assert_eq!(t, "Use the <think> tag like this");
    }

    #[test]
    fn reasoning_dialects() {
        let p = |kind: &str, base: &str| ProviderCfg { id: String::new(), kind: kind.into(), name: String::new(), base_url: base.into() };
        let oa = p("openai", "https://api.openai.com/v1");
        assert_eq!(reasoning_fields(&oa, "gpt-5-mini", Some("off")), vec![("reasoning_effort", json!("minimal"))]);
        assert_eq!(reasoning_fields(&oa, "gpt-4.1", Some("on")), vec![]);
        assert_eq!(reasoning_fields(&oa, "o3", Some("high")), vec![("reasoning_effort", json!("high"))]);
        let or = p("openai", "https://openrouter.ai/api/v1");
        assert_eq!(reasoning_fields(&or, "x", Some("off")), vec![("reasoning", json!({"enabled": false}))]);
        let ol = p("ollama", "http://127.0.0.1:11434");
        let f = reasoning_fields(&ol, "qwen3:8b", Some("off"));
        assert!(f.contains(&("think", json!(false))));
        let lc = p("llamacpp", "http://127.0.0.1:39281/v1");
        assert_eq!(reasoning_fields(&lc, "m", Some("on"))[0], ("chat_template_kwargs", json!({"enable_thinking": true, "thinking": true})));
        assert!(reasoning_fields(&lc, "m", None).is_empty());
    }

    #[test]
    fn ollama_roots() {
        assert_eq!(ollama_root("http://h:11434/v1/"), "http://h:11434");
        assert_eq!(ollama_root("http://h:11434/api"), "http://h:11434");
        assert_eq!(ollama_root("http://h:11434"), "http://h:11434");
    }
}

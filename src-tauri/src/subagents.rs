//! Sub-agents: when enabled, the chat model can hand self-contained work to other models
//! (usually small local ones), run several at once, and search the chat's documents.

use crate::llm::{self, Msg, Request, ToolDef};
use crate::models::{self, ModelRow};
use crate::settings::AppSettings;
use crate::state::Shared;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter};

pub const TOOL_NAMES: &[&str] = &["list_models", "run_subagents", "search_documents"];

/// Chat models the main model may call.
pub fn allowed(sh: &Shared, s: &AppSettings) -> Vec<ModelRow> {
    models::list_models(&sh.db)
        .unwrap_or_default()
        .into_iter()
        .filter(|m| !m.effective.embedding)
        .filter(|m| if s.subagents.models.is_empty() { m.local } else { s.subagents.models.contains(&m.id) })
        .filter(|m| !s.subagents.local_only || m.local)
        .collect()
}

pub fn defs(sh: &Shared, s: &AppSettings, conv_id: &str) -> Vec<ToolDef> {
    let mut d = vec![];
    if s.subagents.enabled && !allowed(sh, s).is_empty() {
        d.push(ToolDef {
            name: "list_models".into(),
            description: "List the models you can hand work to with run_subagents, with what each can do.".into(),
            parameters: json!({"type":"object","properties":{}}),
        });
        d.push(ToolDef {
            name: "run_subagents".into(),
            description: format!(
                "Hand self-contained tasks to other models and get their answers back. Tasks run in parallel (up to {}). Each sub-agent sees only the task text you write, so include everything it needs. Good for drafts, summaries of text you paste in, translations, second opinions, or splitting a big job.",
                s.subagents.max_parallel
            ),
            parameters: json!({"type":"object","properties":{"tasks":{"type":"array","minItems":1,"maxItems":8,"items":{"type":"object","properties":{
                "task":{"type":"string","description":"Complete instructions and any text the sub-agent needs"},
                "model":{"type":"string","description":"Model name from list_models; omit to use the first one"}
            },"required":["task"]}}},"required":["tasks"]}),
        });
    }
    if s.embed_model.is_some() && crate::docs::has_embedded(&sh.db, conv_id) {
        d.push(ToolDef {
            name: "search_documents".into(),
            description: "Search the documents attached to this chat for passages about a topic. Use when the excerpts you were given are not enough.".into(),
            parameters: json!({"type":"object","properties":{"query":{"type":"string"},"top_k":{"type":"integer","minimum":1,"maximum":12}},"required":["query"]}),
        });
    }
    d
}

pub async fn run(app: &AppHandle, sh: &Shared, s: &AppSettings, conv_id: &str, msg_id: &str, name: &str, a: &Value) -> Result<String> {
    match name {
        "list_models" => {
            let list: Vec<Value> = allowed(sh, s)
                .iter()
                .map(|m| json!({"name": m.label, "provider": m.provider_name, "local": m.local, "vision": m.effective.vision, "tools": m.effective.tools, "reasoning": m.effective.reasoning, "context": m.effective.context, "speed_tokens_per_sec": m.effective.tokens_per_sec}))
                .collect();
            Ok(serde_json::to_string_pretty(&list)?)
        }
        "search_documents" => {
            let q = a["query"].as_str().unwrap_or("").trim();
            if q.is_empty() {
                bail!("query is empty");
            }
            let k = a["top_k"].as_u64().unwrap_or(6).clamp(1, 12) as usize;
            let hits = crate::docs::search(sh, conv_id, q, k).await?;
            if hits.is_empty() {
                return Ok("No matching passages.".into());
            }
            Ok(hits.iter().enumerate().map(|(i, (d, t, sc))| format!("[{}] {d} (match {:.2})\n{t}", i + 1, sc)).collect::<Vec<_>>().join("\n\n"))
        }
        "run_subagents" => run_subagents(app, sh, s, conv_id, msg_id, a).await,
        _ => bail!("Unknown tool"),
    }
}

async fn run_subagents(app: &AppHandle, sh: &Shared, s: &AppSettings, conv_id: &str, msg_id: &str, a: &Value) -> Result<String> {
    if !s.subagents.enabled {
        bail!("Sub-agents are turned off in Settings");
    }
    let pool = allowed(sh, s);
    let first = pool.first().cloned().ok_or_else(|| anyhow!("No models are available for sub-agents"))?;
    let tasks = a["tasks"].as_array().cloned().unwrap_or_default();
    if tasks.is_empty() {
        bail!("Give at least one task");
    }
    let sem = Arc::new(tokio::sync::Semaphore::new(s.subagents.max_parallel.max(1) as usize));
    let mut futs = vec![];
    for (i, t) in tasks.into_iter().take(8).enumerate() {
        let task = t["task"].as_str().unwrap_or("").trim().to_string();
        let wanted = t["model"].as_str().unwrap_or("").to_lowercase();
        let m = pool.iter().find(|m| !wanted.is_empty() && (m.label.to_lowercase() == wanted || m.name.to_lowercase() == wanted)).cloned().unwrap_or_else(|| first.clone());
        let (sem, sh, app, s, conv, mid) = (sem.clone(), sh.clone(), app.clone(), s.clone(), conv_id.to_string(), msg_id.to_string());
        futs.push(async move {
            if task.is_empty() {
                return format!("### Sub-agent {} \n(empty task skipped)", i + 1);
            }
            let _permit = sem.acquire().await;
            let label = format!("sub-agent {} · {}", i + 1, m.label);
            let _ = app.emit("chat-tool", json!({"conversation_id": conv, "message_id": mid, "name": label, "args": {"task": task}, "status": "running"}));
            let r = one(&sh, &s, &m, &task).await;
            let text = match &r {
                Ok(t) => t.clone(),
                Err(e) => format!("Error: {e}"),
            };
            let shown: String = text.chars().take(4000).collect();
            let _ = app.emit("chat-tool", json!({"conversation_id": conv, "message_id": mid, "name": label, "args": {"task": task}, "status": "done", "result": shown}));
            crate::audit::ai(&sh.db, &conv, "chat.subagent", &m.label, if r.is_ok() { "ok" } else { "error" }, json!({"task": task, "result": shown}));
            format!("### Sub-agent {} ({})\n{}", i + 1, m.label, text)
        });
    }
    Ok(futures_util::future::join_all(futs).await.join("\n\n"))
}

async fn one(sh: &Shared, s: &AppSettings, m: &ModelRow, task: &str) -> Result<String> {
    let p = models::get_provider(&sh.db, &m.provider_id)?;
    let key = crate::secrets::get(&p.id);
    let req = Request {
        model: m.name.clone(),
        messages: vec![
            Msg::text("system", "You are a sub-agent helping another assistant. Do exactly the task you are given and reply with the result only, in Markdown, without preamble."),
            Msg::text("user", task),
        ],
        tools: vec![],
        max_tokens: 4096,
        temperature: None,
        reasoning: if m.effective.reasoning { Some("off".into()) } else { None },
    };
    let t0 = Instant::now();
    let r = llm::complete(&sh.http, &p, key, &req, s.local_only || s.subagents.local_only).await?;
    let ms = t0.elapsed().as_millis() as u64;
    let cost = models::record_usage(&sh.db, m, r.input_tokens, r.output_tokens);
    models::record_reply(&sh.db, m, &p.name, "subagent", r.input_tokens, r.output_tokens, 0, ms, cost, "ok");
    let t = r.text.trim().to_string();
    Ok(if t.is_empty() { "(no answer)".into() } else { t })
}

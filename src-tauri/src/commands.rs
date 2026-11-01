//! Commands the interface can call. The interface never makes safety decisions.

use crate::chat::{self, Attachment};
use crate::db::{new_id, now};
use crate::models::{self, Overrides};
use crate::settings::{self, AppSettings};
use crate::state::Shared;
use crate::{agent, files};
use base64::Engine;
use rusqlite::{params, OptionalExtension};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------- Settings ----------

#[tauri::command]
pub fn get_settings(sh: State<'_, Shared>) -> AppSettings {
    settings::load(&sh.db)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, sh: State<'_, Shared>, mut settings: AppSettings) -> R<AppSettings> {
    // A window may still hold a model that was removed meanwhile; repair rather than refuse.
    models::heal_model_refs(&sh.db, &mut settings);
    check_settings(&sh, &settings)?;
    settings::save(&sh.db, &settings).map_err(e)?;
    crate::register_shortcuts(&app, &settings);
    let _ = app.emit("settings-changed", &settings);
    Ok(settings)
}

/// Rejects invalid settings with a JSON list of {field, message} the interface shows inline.
fn check_settings(sh: &Shared, s: &AppSettings) -> R<()> {
    let ids: Vec<String> = models::list_models(&sh.db).map_err(e)?.into_iter().map(|m| m.id).collect();
    let problems = crate::validate::settings(s, &ids);
    if problems.is_empty() {
        return Ok(());
    }
    Err(json!(problems.iter().map(|(f, m)| json!({"field": f, "message": m})).collect::<Vec<_>>()).to_string())
}

#[tauri::command]
pub fn export_settings(sh: State<'_, Shared>) -> R<String> {
    let s = settings::load(&sh.db);
    let path = dirs::download_dir().unwrap_or_else(std::env::temp_dir).join("computer-agent-settings.json");
    std::fs::write(&path, serde_json::to_string_pretty(&s).map_err(e)?).map_err(e)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn import_settings(app: AppHandle, sh: State<'_, Shared>, json_text: String) -> R<AppSettings> {
    let mut s: AppSettings = serde_json::from_str(&json_text).map_err(|err| format!("This file is not a valid settings export: {err}"))?;
    // Model ids are machine-specific; keep the current choices.
    let cur = settings::load(&sh.db);
    s.default_model = cur.default_model;
    s.fallback_model = cur.fallback_model;
    s.roles = cur.roles;
    s.onboarded = true;
    check_settings(&sh, &s)?;
    settings::save(&sh.db, &s).map_err(e)?;
    crate::register_shortcuts(&app, &s);
    let _ = app.emit("settings-changed", &s);
    Ok(s)
}

#[tauri::command]
pub fn reset_settings(app: AppHandle, sh: State<'_, Shared>) -> R<AppSettings> {
    let cur = settings::load(&sh.db);
    let s = AppSettings { default_model: cur.default_model, fallback_model: cur.fallback_model, roles: cur.roles, onboarded: true, ..AppSettings::default() };
    settings::save(&sh.db, &s).map_err(e)?;
    crate::register_shortcuts(&app, &s);
    let _ = app.emit("settings-changed", &s);
    Ok(s)
}

#[tauri::command]
pub fn validate_shortcut(shortcut: String) -> R<()> {
    crate::validate::shortcut(&shortcut)
}

#[tauri::command]
pub fn apply_profile(app: AppHandle, sh: State<'_, Shared>, profile: String) -> R<AppSettings> {
    let mut s = settings::load(&sh.db);
    settings::apply_profile(&mut s, &profile);
    settings::save(&sh.db, &s).map_err(e)?;
    let _ = app.emit("settings-changed", &s);
    Ok(s)
}

// ---------- Providers & models ----------

#[tauri::command]
pub fn list_providers(sh: State<'_, Shared>) -> R<Vec<Value>> {
    models::list_providers(&sh.db).map_err(e)
}

#[tauri::command]
pub async fn add_provider(sh: State<'_, Shared>, kind: String, name: String, base_url: String, key: Option<String>) -> R<String> {
    crate::validate::provider(&kind, &name, &base_url, key.as_deref(), true)?;
    // Save the address that really answers (adds a missing /v1, drops a pasted /chat/completions).
    let mut base_url = base_url.trim().trim_end_matches('/').to_string();
    if let Ok(Ok((b, _))) = tokio::time::timeout(std::time::Duration::from_secs(8), models::resolve_base(&sh.http, &kind, &base_url, key.clone())).await {
        base_url = b;
    } else if kind == "ollama" {
        base_url = crate::llm::ollama_root(&base_url);
    }
    let wanted = base_url.to_lowercase();
    let dup = models::list_providers(&sh.db).map_err(e)?.into_iter().any(|p| p["base_url"].as_str().map(|b| b.to_lowercase() == wanted).unwrap_or(false));
    if dup {
        return Err("Base address: a provider with this address already exists".into());
    }
    let id = models::add_provider(&sh.db, &kind, &name, &base_url, key).map_err(e)?;
    crate::audit::log(&sh.db, "provider.add", &name, json!({"kind": kind, "base_url": base_url}));
    Ok(id)
}

#[tauri::command]
pub fn update_provider(sh: State<'_, Shared>, id: String, name: String, base_url: String, key: Option<String>) -> R<()> {
    let p = models::get_provider(&sh.db, &id).map_err(e)?;
    crate::validate::provider(&p.kind, &name, &base_url, key.as_deref(), false)?;
    models::update_provider(&sh.db, &id, &name, &base_url, key).map_err(e)
}

#[tauri::command]
pub fn delete_provider(app: AppHandle, sh: State<'_, Shared>, id: String) -> R<()> {
    models::delete_provider(&sh.db, &id).map_err(e)?;
    heal_and_emit(&app, &sh)
}

fn heal_and_emit(app: &AppHandle, sh: &Shared) -> R<()> {
    let mut s = settings::load(&sh.db);
    if models::heal_model_refs(&sh.db, &mut s) {
        settings::save(&sh.db, &s).map_err(e)?;
        let _ = app.emit("settings-changed", &s);
    }
    Ok(())
}

#[tauri::command]
pub async fn provider_remote_models(sh: State<'_, Shared>, id: String) -> R<Vec<crate::llm::RemoteModel>> {
    let p = models::get_provider(&sh.db, &id).map_err(e)?;
    let s = settings::load(&sh.db);
    if s.local_only && !p.is_local() {
        return Err("Local-only mode is on; cloud providers are blocked.".into());
    }
    crate::llm::list_models(&sh.http, &p, crate::secrets::get(&id)).await.map_err(e)
}

/// Verifies an address and key before saving by listing the provider's models.
/// Returns the address that answered, which may differ from what was typed (e.g. /v1 added).
#[tauri::command]
pub async fn test_provider(sh: State<'_, Shared>, kind: String, base_url: String, key: Option<String>, provider_id: Option<String>) -> R<Value> {
    crate::validate::url(&base_url).map_err(|m| format!("Base address: {m}"))?;
    if settings::load(&sh.db).local_only && !crate::validate::is_local_url(&base_url) {
        return Err("Local-only mode is on, so cloud addresses cannot be contacted.".into());
    }
    let key = key.filter(|k| !k.trim().is_empty()).or_else(|| provider_id.as_deref().and_then(crate::secrets::get));
    let fut = models::resolve_base(&sh.http, &kind, &base_url, key);
    match tokio::time::timeout(std::time::Duration::from_secs(25), fut).await {
        Ok(Ok((base, models))) => Ok(json!({"base_url": base, "models": models})),
        Ok(Err(x)) => Err(e(x)),
        Err(_) => Err("No answer within 25 seconds. Check the address and that the service is running.".into()),
    }
}

#[tauri::command]
pub async fn detect_local(sh: State<'_, Shared>) -> R<Vec<Value>> {
    Ok(models::detect_local(&sh.http).await)
}

#[tauri::command]
pub fn list_models(sh: State<'_, Shared>) -> R<Vec<models::ModelRow>> {
    models::list_models(&sh.db).map_err(e)
}

#[tauri::command]
pub fn add_model(app: AppHandle, sh: State<'_, Shared>, provider_id: String, name: String) -> R<String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.len() > 200 {
        return Err("Model name: enter the exact model name".into());
    }
    let id = models::add_model(&sh.db, &provider_id, &name).map_err(e)?;
    let mut s = settings::load(&sh.db);
    if s.default_model.is_none() {
        s.default_model = Some(id.clone());
        let _ = settings::save(&sh.db, &s);
        let _ = app.emit("settings-changed", &s);
    }
    Ok(id)
}

#[tauri::command]
pub fn update_model(sh: State<'_, Shared>, id: String, label: String, overrides: Overrides, price_in: f64, price_out: f64) -> R<()> {
    crate::validate::model(&label, price_in, price_out)?;
    models::update_model(&sh.db, &id, &label, &overrides, price_in, price_out).map_err(e)
}

#[tauri::command]
pub fn delete_model(app: AppHandle, sh: State<'_, Shared>, id: String) -> R<()> {
    models::delete_model(&sh.db, &id).map_err(e)?;
    heal_and_emit(&app, &sh)
}

#[tauri::command]
pub async fn test_model(sh: State<'_, Shared>, id: String) -> R<models::Profile> {
    let s = settings::load(&sh.db);
    models::self_test(&sh.db, &sh.http, &id, s.local_only).await.map_err(e)
}

#[tauri::command]
pub fn hardware_info() -> Value {
    models::hardware()
}

#[tauri::command]
pub fn model_catalog() -> Value {
    models::catalog()
}

#[tauri::command]
pub async fn ollama_pull(app: AppHandle, sh: State<'_, Shared>, base_url: String, name: String) -> R<()> {
    let n2 = name.clone();
    let app2 = app.clone();
    models::ollama_pull(&sh.http, &base_url, &name, move |status, done, total| {
        let _ = app2.emit("ollama-pull", json!({"name": n2, "status": status, "completed": done, "total": total}));
    })
    .await
    .map_err(e)?;
    let _ = app.emit("ollama-pull", json!({"name": name, "status": "success", "completed": 1, "total": 1}));
    Ok(())
}

#[tauri::command]
pub async fn ollama_delete(sh: State<'_, Shared>, base_url: String, name: String) -> R<()> {
    models::ollama_delete(&sh.http, &base_url, &name).await.map_err(e)
}

#[tauri::command]
pub async fn ollama_unload(sh: State<'_, Shared>, base_url: String, name: String) -> R<()> {
    models::ollama_unload(&sh.http, &base_url, &name).await;
    Ok(())
}

// ---------- Chat ----------

#[tauri::command]
pub fn list_conversations(sh: State<'_, Shared>, query: String, limit: Option<u32>) -> R<Value> {
    chat::list_conversations(&sh, &query, limit.unwrap_or(40)).map_err(e)
}

#[tauri::command]
pub fn create_conversation(sh: State<'_, Shared>, model_id: Option<String>, folder: Option<String>) -> R<String> {
    chat::create_conversation(&sh, model_id, folder.as_deref().unwrap_or("")).map_err(e)
}

#[tauri::command]
pub fn get_conversation(sh: State<'_, Shared>, id: String) -> R<Value> {
    chat::get_conversation(&sh, &id).map_err(e)
}

#[tauri::command]
pub fn update_conversation(sh: State<'_, Shared>, id: String, patch: Value) -> R<()> {
    if let Some(t) = patch["title"].as_str() {
        if t.trim().is_empty() || t.chars().count() > 120 {
            return Err("Title: use 1 to 120 characters".into());
        }
    }
    if let Some(f) = patch["folder"].as_str() {
        if f.chars().count() > 40 || f.contains(['/', '\\']) {
            return Err("Folder: up to 40 characters, no slashes".into());
        }
    }
    chat::update_conversation(&sh, &id, &patch).map_err(e)
}

#[tauri::command]
pub fn delete_conversation(sh: State<'_, Shared>, id: String) -> R<()> {
    chat::delete_conversation(&sh, &id).map_err(e)
}

/// Thumbs up / down on a reply; kept with the message and in the audit trail.
#[tauri::command]
pub fn rate_message(sh: State<'_, Shared>, conversation_id: String, message_id: String, rating: i32) -> R<()> {
    let conn = sh.db.conn.lock();
    let meta: String = conn.query_row("SELECT meta FROM messages WHERE id=?1 AND conversation_id=?2", params![message_id, conversation_id], |r| r.get(0)).map_err(|_| "Message not found".to_string())?;
    let mut v: Value = serde_json::from_str(&meta).unwrap_or(json!({}));
    v["rating"] = json!(rating.clamp(-1, 1));
    conn.execute("UPDATE messages SET meta=?2 WHERE id=?1", params![message_id, v.to_string()]).map_err(e)?;
    Ok(())
}

/// Copies the whole conversation so it can go a different way.
#[tauri::command]
pub fn fork_conversation(sh: State<'_, Shared>, id: String) -> R<String> {
    let new = chat::branch(&sh, &id, "").map_err(e)?;
    let title = chat::get_conversation(&sh, &id).map_err(e)?["title"].as_str().unwrap_or("Chat").to_string();
    chat::update_conversation(&sh, &new, &json!({"title": crate::a11y::trunc(&format!("{title} (fork)"), 120)})).map_err(e)?;
    Ok(new)
}

/// Native file dialogs. A path chosen here is the only kind the save and import commands accept,
/// so a page that misbehaves cannot read or write files the user never picked.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn pick_path(app: AppHandle, sh: State<'_, Shared>, mode: String, title: Option<String>, filter_name: Option<String>, extensions: Option<Vec<String>>, default_name: Option<String>) -> R<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let mut d = app.dialog().file();
    if let Some(t) = title {
        d = d.set_title(t);
    }
    if let Some(exts) = extensions.filter(|x| !x.is_empty()) {
        let refs: Vec<&str> = exts.iter().map(String::as_str).collect();
        d = d.add_filter(filter_name.unwrap_or_else(|| "Files".into()), &refs);
    }
    if let Some(n) = default_name {
        d = d.set_file_name(n);
    }
    let picked = tokio::task::spawn_blocking(move || match mode.as_str() {
        "save" => d.blocking_save_file(),
        "folder" => d.blocking_pick_folder(),
        _ => d.blocking_pick_file(),
    })
    .await
    .map_err(e)?;
    let Some(fp) = picked else { return Ok(None) };
    let p = fp.into_path().map_err(e)?;
    sh.granted.lock().insert(p.clone());
    Ok(Some(p.to_string_lossy().to_string()))
}

/// The path, if the user picked it in a dialog this session.
pub fn granted(sh: &Shared, path: &str) -> R<std::path::PathBuf> {
    let p = std::path::PathBuf::from(path.trim().trim_matches('"'));
    if sh.granted.lock().contains(&p) {
        Ok(p)
    } else {
        Err("Choose the file in the file dialog first".into())
    }
}

/// Writes an export the user chose a location for in the save dialog.
#[tauri::command]
pub fn save_text_file(sh: State<'_, Shared>, path: String, content: String) -> R<()> {
    let p = granted(&sh, &path)?;
    let p = p.as_path();
    let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
    if !["md", "json", "txt", "csv", "html"].contains(&ext.as_str()) {
        return Err("Exports can be saved as .md, .json, .txt, .csv or .html".into());
    }
    if content.len() > 200_000_000 {
        return Err("Export is too large".into());
    }
    std::fs::write(p, content).map_err(e)
}

// ---------- Audit ----------

#[tauri::command]
pub fn audit_query(sh: State<'_, Shared>, query: crate::audit::Query) -> R<Value> {
    crate::audit::query(&sh.db, &query).map_err(e)
}

#[tauri::command]
pub fn audit_export(sh: State<'_, Shared>, query: crate::audit::Query, format: String) -> R<String> {
    crate::audit::log(&sh.db, "audit.export", &format, json!({}));
    crate::audit::export(&sh.db, &query, &format).map_err(e)
}

#[tauri::command]
pub fn audit_verify(sh: State<'_, Shared>) -> R<Value> {
    crate::audit::verify(&sh.db).map_err(e)
}

#[tauri::command]
pub fn branch_conversation(sh: State<'_, Shared>, id: String, message_id: String) -> R<String> {
    chat::branch(&sh, &id, &message_id).map_err(e)
}

/// Claims the conversation for one reply at a time; Stop finds the reply through this handle.
fn reserve(sh: &Shared, conv: &str) -> R<std::sync::Arc<tokio::sync::Notify>> {
    let mut m = sh.chat_cancels.lock();
    if m.contains_key(conv) {
        return Err("A reply is still being written in this chat. Stop it or wait for it to finish.".into());
    }
    let n = std::sync::Arc::new(tokio::sync::Notify::new());
    m.insert(conv.to_string(), n.clone());
    Ok(n)
}

/// Runs the reply in the background. Any failure the reply did not report itself (for example a
/// model that was removed) still reaches the interface, so it never waits forever.
fn spawn_reply(app: AppHandle, sh: Shared, conv: String, model_id: String, think: Option<bool>, cancel: std::sync::Arc<tokio::sync::Notify>) {
    tauri::async_runtime::spawn(async move {
        let r = chat::respond(app.clone(), sh.clone(), conv.clone(), model_id, think, cancel).await;
        sh.chat_cancels.lock().remove(&conv);
        if let Err(err) = r {
            let _ = app.emit("chat-error", json!({"conversation_id": conv, "error": chat::friendly_error(&err.to_string()), "fallback": null}));
        }
    });
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn send_message(app: AppHandle, sh: State<'_, Shared>, conversation_id: String, content: String, images: Vec<Attachment>, model_id: String, think: Option<bool>, docs: Option<Vec<Value>>) -> R<String> {
    let docs = docs.unwrap_or_default();
    if docs.len() > 20 {
        return Err("Attach up to 20 documents per message".into());
    }
    let cancel = reserve(&sh, &conversation_id)?;
    let added = chat::add_user_message(&sh, &conversation_id, &content, &images, &docs).and_then(|id| chat::update_conversation(&sh, &conversation_id, &json!({"model_id": model_id})).map(|_| id));
    let id = match added {
        Ok(id) => id,
        Err(x) => {
            sh.chat_cancels.lock().remove(&conversation_id);
            return Err(e(x));
        }
    };
    spawn_reply(app, sh.inner().clone(), conversation_id, model_id, think, cancel);
    Ok(id)
}

#[tauri::command]
pub fn regenerate(app: AppHandle, sh: State<'_, Shared>, conversation_id: String, message_id: String, model_id: String, think: Option<bool>) -> R<()> {
    let cancel = reserve(&sh, &conversation_id)?;
    if let Err(x) = chat::truncate_from(&sh, &conversation_id, &message_id) {
        sh.chat_cancels.lock().remove(&conversation_id);
        return Err(e(x));
    }
    spawn_reply(app, sh.inner().clone(), conversation_id, model_id, think, cancel);
    Ok(())
}

#[tauri::command]
pub fn edit_and_resend(app: AppHandle, sh: State<'_, Shared>, conversation_id: String, message_id: String, content: String, model_id: String, think: Option<bool>) -> R<()> {
    let conv = chat::get_conversation(&sh, &conversation_id).map_err(e)?;
    let old = conv["messages"].as_array().and_then(|a| a.iter().find(|m| m["id"] == message_id.as_str())).cloned().unwrap_or(json!({}));
    let images: Vec<Attachment> = serde_json::from_value(old["images"].clone()).unwrap_or_default();
    let docs: Vec<Value> = old["meta"]["docs"].as_array().cloned().unwrap_or_default();
    let cancel = reserve(&sh, &conversation_id)?;
    if let Err(x) = chat::truncate_from(&sh, &conversation_id, &message_id).and_then(|_| chat::add_user_message(&sh, &conversation_id, &content, &images, &docs)) {
        sh.chat_cancels.lock().remove(&conversation_id);
        return Err(e(x));
    }
    spawn_reply(app, sh.inner().clone(), conversation_id, model_id, think, cancel);
    Ok(())
}

#[tauri::command]
pub fn stop_chat(sh: State<'_, Shared>, conversation_id: String) {
    if let Some(n) = sh.chat_cancels.lock().get(&conversation_id) {
        n.notify_waiters();
    }
}

#[tauri::command]
pub fn export_conversation(sh: State<'_, Shared>, id: String, format: String) -> R<String> {
    let text = chat::export(&sh, &id, &format).map_err(e)?;
    let conv = chat::get_conversation(&sh, &id).map_err(e)?;
    let title: String = conv["title"].as_str().unwrap_or("chat").chars().filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-').collect();
    let dir = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
    let path = dir.join(format!("{}.{}", title.trim(), if format == "json" { "json" } else { "md" }));
    std::fs::write(&path, text).map_err(e)?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn capture_screenshot(app: AppHandle) -> R<String> {
    let main = app.get_webview_window("main");
    if let Some(w) = &main {
        let _ = w.minimize();
    }
    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
    let r = tokio::task::spawn_blocking(|| crate::screen::capture_for_chat(1600)).await.map_err(e)?;
    if let Some(w) = &main {
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
    r.map_err(e)
}

// ---------- Agent ----------

#[tauri::command]
pub fn start_task(app: AppHandle, sh: State<'_, Shared>, goal: String, conversation_id: Option<String>, model_id: Option<String>) -> R<String> {
    if goal.trim().len() < 3 {
        return Err("Describe what the agent should do".into());
    }
    if goal.chars().count() > 4000 {
        return Err("Keep the task description under 4000 characters".into());
    }
    if models::list_models(&sh.db).map_err(e)?.is_empty() {
        return Err("No model is available for the agent. Add one in Models first.".into());
    }
    if let Some(cid) = &conversation_id {
        chat::add_user_message(&sh, cid, &goal, &[], &[]).map_err(e)?;
    }
    Ok(agent::enqueue(app, sh.inner().clone(), goal, conversation_id, None, model_id))
}

#[tauri::command]
pub fn resume_task(app: AppHandle, sh: State<'_, Shared>, task_id: String) -> R<String> {
    let (goal, conv): (String, Option<String>) = sh
        .db
        .conn
        .lock()
        .query_row("SELECT goal, conversation_id FROM tasks WHERE id=?1", [&task_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(e)?;
    let _ = sh.db.conn.lock().execute("UPDATE tasks SET status='resumed' WHERE id=?1", [&task_id]);
    Ok(agent::enqueue(app, sh.inner().clone(), goal, conv, Some(task_id), None))
}

#[tauri::command]
pub fn agent_stop(app: AppHandle) {
    crate::state::emergency_stop(&app);
}

/// Cancels one running tool call (chat or agent); the reply or task carries on without it.
#[tauri::command]
pub fn cancel_tool(sh: State<'_, Shared>, call_id: String) -> R<bool> {
    let ok = sh.tool_cancels.cancel(&call_id);
    if ok {
        crate::audit::log(&sh.db, "tool.cancel", &call_id, json!({}));
    }
    Ok(ok)
}

#[tauri::command]
pub fn agent_pause(app: AppHandle, sh: State<'_, Shared>) {
    sh.agent.paused.store(true, Ordering::SeqCst);
    let _ = app.emit("agent-paused", "Paused by you.");
}

#[tauri::command]
pub fn agent_resume(sh: State<'_, Shared>) {
    *sh.agent.last_agent_pos.lock() = crate::screen::cursor();
    sh.agent.paused.store(false, Ordering::SeqCst);
}

#[tauri::command]
pub fn agent_respond(sh: State<'_, Shared>, id: String, value: Value) -> R<()> {
    agent::respond(&sh, &id, value).map_err(e)
}

#[tauri::command]
pub fn agent_status(sh: State<'_, Shared>) -> Value {
    json!({
        "running": sh.agent.running.load(Ordering::SeqCst),
        "paused": sh.agent.paused.load(Ordering::SeqCst),
        "status": sh.agent.status.lock().clone(),
        "prompts": agent::pending_prompts(&sh),
        "queue": sh.agent.queue.lock().iter().cloned().collect::<Vec<_>>(),
    })
}

#[tauri::command]
pub fn cancel_queued(app: AppHandle, sh: State<'_, Shared>, id: String) {
    sh.agent.queue.lock().retain(|t| t.id != id);
    let q: Vec<_> = sh.agent.queue.lock().iter().cloned().collect();
    let _ = app.emit("agent-queue", q);
}

// ---------- Activity ----------

#[tauri::command]
pub fn list_tasks(sh: State<'_, Shared>) -> R<Vec<Value>> {
    let conn = sh.db.conn.lock();
    let mut st = conn
        .prepare(
            "SELECT t.id,t.goal,t.status,t.cost,t.steps,t.started,t.ended,t.summary,t.models,
             (SELECT COUNT(*) FROM undo_ops u WHERE u.task_id=t.id AND u.undone=0)
             FROM tasks t ORDER BY t.started DESC LIMIT 300",
        )
        .map_err(e)?;
    let rows = st
        .query_map([], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "goal": r.get::<_, String>(1)?, "status": r.get::<_, String>(2)?, "cost": r.get::<_, f64>(3)?,
                "steps": r.get::<_, i64>(4)?, "started": r.get::<_, i64>(5)?, "ended": r.get::<_, Option<i64>>(6)?, "summary": r.get::<_, String>(7)?,
                "models": serde_json::from_str::<Value>(&r.get::<_, String>(8)?).unwrap_or(json!({})), "undoable": r.get::<_, i64>(9)?}))
        })
        .map_err(e)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[tauri::command]
pub fn get_task_steps(sh: State<'_, Shared>, id: String, query: Option<String>) -> R<Vec<Value>> {
    let conn = sh.db.conn.lock();
    let q = format!("%{}%", query.unwrap_or_default());
    let mut st = conn
        .prepare("SELECT id,idx,kind,title,detail,action,result,screenshot,risk,model,cost,created FROM steps WHERE task_id=?1 AND (title LIKE ?2 OR detail LIKE ?2 OR result LIKE ?2) ORDER BY created")
        .map_err(e)?;
    let rows = st
        .query_map(params![id, q], |r| {
            Ok(json!({"id": r.get::<_, String>(0)?, "idx": r.get::<_, i64>(1)?, "kind": r.get::<_, String>(2)?, "title": r.get::<_, String>(3)?,
                "detail": r.get::<_, String>(4)?, "action": serde_json::from_str::<Value>(&r.get::<_, String>(5)?).unwrap_or(json!({})),
                "result": r.get::<_, String>(6)?, "screenshot": r.get::<_, String>(7)?, "risk": r.get::<_, String>(8)?, "model": r.get::<_, String>(9)?,
                "cost": r.get::<_, f64>(10)?, "created": r.get::<_, i64>(11)?}))
        })
        .map_err(e)?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

#[tauri::command]
pub fn read_screenshot(sh: State<'_, Shared>, path: String) -> R<String> {
    let p = crate::files::resolve(std::path::Path::new(&path)).map_err(|_| "Not an agent screenshot".to_string())?;
    if !crate::files::within(&p, &sh.shots_dir()) || p.extension().map(|x| x != "jpg").unwrap_or(true) {
        return Err("Not an agent screenshot".into());
    }
    let bytes = std::fs::read(p).map_err(|_| "Screenshot no longer available".to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[tauri::command]
pub fn undo_task(sh: State<'_, Shared>, id: String) -> R<Value> {
    files::undo_task(&sh.db, &sh.holding_dir(), &id).map_err(e)
}

#[tauri::command]
pub fn delete_task(sh: State<'_, Shared>, id: String) -> R<()> {
    if !is_id(&id) {
        return Err("Task not found".into());
    }
    let conn = sh.db.conn.lock();
    conn.execute("DELETE FROM steps WHERE task_id=?1", [&id]).map_err(e)?;
    conn.execute("DELETE FROM tasks WHERE id=?1", [&id]).map_err(e)?;
    let _ = std::fs::remove_dir_all(sh.shots_dir().join(&id));
    Ok(())
}

/// Exports a task log as JSON; screenshots are referenced, secrets were masked at capture.
#[tauri::command]
pub fn export_task(sh: State<'_, Shared>, id: String) -> R<String> {
    let steps = get_task_steps(sh.clone(), id.clone(), None)?;
    let task: Value = list_tasks(sh.clone())?.into_iter().find(|t| t["id"] == id.as_str()).unwrap_or(json!({}));
    let out = json!({"task": task, "steps": steps});
    let path = dirs::download_dir().unwrap_or_else(std::env::temp_dir).join(format!("agent-task-{}.json", id.chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect::<String>()));
    std::fs::write(&path, serde_json::to_string_pretty(&out).map_err(e)?).map_err(e)?;
    Ok(path.to_string_lossy().to_string())
}

/// Ids are generated UUIDs; anything else (such as "..") never reaches a file path.
fn is_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

// ---------- Playbooks (saved tasks) ----------

fn load_playbooks(sh: &Shared) -> Vec<Value> {
    sh.db
        .conn
        .lock()
        .query_row("SELECT value FROM settings WHERE key='playbooks'", [], |r| r.get::<_, String>(0))
        .optional()
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_playbooks(sh: &Shared, v: &[Value]) -> R<()> {
    sh.db
        .conn
        .lock()
        .execute(
            "INSERT INTO settings(key,value) VALUES('playbooks',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            [serde_json::to_string(v).map_err(e)?],
        )
        .map_err(e)?;
    Ok(())
}

#[tauri::command]
pub fn list_playbooks(sh: State<'_, Shared>) -> Vec<Value> {
    load_playbooks(&sh)
}

#[tauri::command]
pub fn save_playbook(sh: State<'_, Shared>, name: String, goal: String) -> R<()> {
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err("Name: use 1 to 80 characters".into());
    }
    if goal.trim().is_empty() {
        return Err("A playbook needs a task description".into());
    }
    let mut v = load_playbooks(&sh);
    if v.iter().any(|p| p["name"].as_str().map(|n| n.eq_ignore_ascii_case(name.trim())).unwrap_or(false)) {
        return Err("Name: a playbook with this name already exists".into());
    }
    v.push(json!({"id": new_id(), "name": name, "goal": goal, "created": now()}));
    save_playbooks(&sh, &v)
}

#[tauri::command]
pub fn delete_playbook(sh: State<'_, Shared>, id: String) -> R<()> {
    let mut v = load_playbooks(&sh);
    v.retain(|p| p["id"] != id.as_str());
    save_playbooks(&sh, &v)
}

// ---------- Privacy, platform, diagnostics ----------

#[tauri::command]
/// Deletes chats, documents, task logs, statistics, playbooks, screenshots, the undo area and the
/// database backups. With `include_audit`, the audit trail goes too (it restarts with one record of
/// the wipe). Models, providers, keys and settings are kept.
pub fn wipe_all_data(sh: State<'_, Shared>, include_audit: Option<bool>) -> R<()> {
    let include_audit = include_audit.unwrap_or(false);
    {
        let conn = sh.db.conn.lock();
        let mut tables = vec!["chunks", "documents", "messages", "conversations", "steps", "tasks", "undo_ops", "usage", "replies"];
        if include_audit {
            tables.push("audit");
        }
        for t in tables {
            conn.execute(&format!("DELETE FROM {t}"), []).map_err(e)?;
        }
        conn.execute("DELETE FROM settings WHERE key='playbooks'", []).map_err(e)?;
    }
    crate::audit::log(&sh.db, "data.wipe", if include_audit { "everything, including the audit trail" } else { "chats, documents and task data" }, json!({}));
    {
        let conn = sh.db.conn.lock();
        // Rewrite the file so deleted text does not linger in free pages or the write-ahead log.
        conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);").map_err(e)?;
    }
    let _ = std::fs::remove_dir_all(sh.shots_dir());
    let _ = std::fs::remove_dir_all(sh.holding_dir());
    if let Ok(rd) = std::fs::read_dir(&sh.data_dir) {
        for f in rd.flatten() {
            let n = f.file_name().to_string_lossy().to_string();
            if n.starts_with("agent.") && n.ends_with(".bak") {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn platform_info(sh: State<'_, Shared>) -> R<Value> {
    let capture = tokio::task::spawn_blocking(|| crate::screen::capture().map(|s| (s.img.width(), s.img.height()))).await.map_err(e)?;
    let worker = sh.a11y.clone();
    let snap = tokio::task::spawn_blocking(move || worker.snapshot([-100000, -100000, 100000, 100000])).await.map_err(e)?;
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
    let monitors: Vec<Value> = xcap::Monitor::all()
        .map(|ms| {
            ms.iter()
                .map(|m| json!({"name": m.friendly_name().unwrap_or_default(), "width": m.width().unwrap_or(0), "height": m.height().unwrap_or(0), "scale": m.scale_factor().unwrap_or(1.0), "primary": m.is_primary().unwrap_or(false)}))
                .collect()
        })
        .unwrap_or_default();
    Ok(json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "version": env!("CARGO_PKG_VERSION"),
        "wayland": wayland,
        "capture": capture.as_ref().map(|_| true).unwrap_or(false),
        "capture_error": capture.as_ref().err().map(|e| e.to_string()),
        "accessibility": snap.coverage != "none" || cfg!(windows),
        "accessibility_backend": if cfg!(windows) { "UI Automation" } else { "not yet available" },
        "input": enigo::Enigo::new(&enigo::Settings::default()).is_ok(),
        "monitors": monitors,
    }))
}

#[tauri::command]
pub fn diagnostics_report(sh: State<'_, Shared>) -> R<String> {
    let models: Vec<Value> = models::list_models(&sh.db)
        .map_err(e)?
        .into_iter()
        .map(|m| json!({"model": m.name, "provider": m.provider_kind, "local": m.local, "profile": m.effective}))
        .collect();
    let failures: Vec<Value> = {
        let conn = sh.db.conn.lock();
        let mut st = conn.prepare("SELECT status, summary, steps FROM tasks WHERE status IN ('failed','interrupted') ORDER BY started DESC LIMIT 10").map_err(e)?;
        let rows = st
            .query_map([], |r| Ok(json!({"status": r.get::<_, String>(0)?, "summary": r.get::<_, String>(1)?, "steps": r.get::<_, i64>(2)?})))
            .map_err(e)?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };
    let report = json!({
        "app_version": env!("CARGO_PKG_VERSION"),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "models": models,
        "recent_failures": failures,
        "note": "No screenshots, keys, chats or file contents are included."
    });
    serde_json::to_string_pretty(&report).map_err(e)
}

#[tauri::command]
pub fn usage_summary(sh: State<'_, Shared>) -> R<Value> {
    let conn = sh.db.conn.lock();
    let today: f64 = conn.query_row("SELECT COALESCE(SUM(cost),0) FROM usage WHERE day=?1", [crate::db::today()], |r| r.get(0)).map_err(e)?;
    let total: f64 = conn.query_row("SELECT COALESCE(SUM(cost),0) FROM usage", [], |r| r.get(0)).map_err(e)?;
    Ok(json!({"today": today, "total": total}))
}

#[tauri::command]
pub fn record_consent(sh: State<'_, Shared>, subject: String) -> R<()> {
    sh.db
        .conn
        .lock()
        .execute("INSERT INTO consents(subject,accepted) VALUES(?1,?2) ON CONFLICT(subject) DO UPDATE SET accepted=excluded.accepted", params![subject, now()])
        .map_err(e)?;
    Ok(())
}

#[tauri::command]
pub fn has_consent(sh: State<'_, Shared>, subject: String) -> bool {
    sh.db.conn.lock().query_row("SELECT 1 FROM consents WHERE subject=?1", [subject], |_| Ok(())).is_ok()
}

#[tauri::command]
pub fn reveal_path(path: String) -> R<()> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(e)
}

/// Opens a web link in the default browser. Only http(s) links are allowed.
#[tauri::command]
pub fn open_link(url: String) -> R<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Only web links can be opened".into());
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(e)
}

#[tauri::command]
pub fn set_bar_size(app: AppHandle, height: f64) {
    if let Some(bar) = app.get_webview_window("bar") {
        let _ = bar.set_size(tauri::LogicalSize::new(560.0, height));
    }
}

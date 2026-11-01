//! Commands for extensions: plugins, MCP servers, the sandbox and the tool catalog.

use crate::db::new_id;
use crate::settings::{self, McpServer, PluginPref, SandboxCfg};
use crate::state::Shared;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

type R<T> = Result<T, String>;
fn e<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ---------- Plugins ----------

#[tauri::command]
pub fn list_plugins(sh: State<'_, Shared>) -> Vec<Value> {
    let s = settings::load(&sh.db);
    crate::plugins::scan(&sh.data_dir)
        .into_iter()
        .map(|p| {
            let pref = s.plugins.get(&p.id).cloned().unwrap_or_default();
            json!({"id": p.id, "dir": p.dir, "manifest": p.manifest, "error": p.error, "enabled": pref.enabled,
                   "permission": if pref.permission.is_empty() { "auto".to_string() } else { pref.permission }})
        })
        .collect()
}

#[tauri::command]
pub fn create_plugin(sh: State<'_, Shared>, name: String) -> R<String> {
    crate::plugins::create_template(&sh.data_dir, name.trim()).map(|p| p.to_string_lossy().to_string()).map_err(e)
}

#[tauri::command]
pub fn import_plugin(sh: State<'_, Shared>, path: String) -> R<String> {
    crate::plugins::import(&sh.data_dir, std::path::Path::new(path.trim().trim_matches('"'))).map_err(e)
}

#[tauri::command]
pub fn delete_plugin(app: AppHandle, sh: State<'_, Shared>, id: String) -> R<()> {
    if id.is_empty() || id.contains(['/', '\\', '.']) {
        return Err("Invalid plugin".into());
    }
    std::fs::remove_dir_all(crate::plugins::dir(&sh.data_dir).join(&id)).map_err(e)?;
    let mut s = settings::load(&sh.db);
    s.plugins.remove(&id);
    settings::save(&sh.db, &s).map_err(e)?;
    let _ = app.emit("settings-changed", &s);
    Ok(())
}

#[tauri::command]
pub fn set_plugin_pref(app: AppHandle, sh: State<'_, Shared>, id: String, enabled: bool, permission: String) -> R<()> {
    if !["auto", "allow", "ask", "off"].contains(&permission.as_str()) {
        return Err("Permission must be auto, allow, ask or off".into());
    }
    let mut s = settings::load(&sh.db);
    s.plugins.insert(id, PluginPref { enabled, permission: if permission == "auto" { String::new() } else { permission } });
    settings::save(&sh.db, &s).map_err(e)?;
    let _ = app.emit("settings-changed", &s);
    Ok(())
}

#[tauri::command]
pub fn plugins_dir(sh: State<'_, Shared>) -> String {
    let d = crate::plugins::dir(&sh.data_dir);
    let _ = std::fs::create_dir_all(&d);
    d.to_string_lossy().to_string()
}

#[tauri::command]
pub async fn test_plugin_tool(sh: State<'_, Shared>, id: String, tool: String, args: Value) -> R<String> {
    let p = crate::plugins::scan(&sh.data_dir).into_iter().find(|p| p.id == id).ok_or("Plugin not found")?;
    let dir = p.dir.clone();
    let m = p.manifest.ok_or_else(|| p.error.unwrap_or_default())?;
    let t = m.tools.into_iter().find(|t| t.name == tool).ok_or("Tool not found")?;
    crate::plugins::run(std::path::Path::new(&dir), &t, &args).await.map_err(e)
}

// ---------- MCP servers ----------

/// Saves a server (new or edited), keeps its secrets in the credential store, then tests the connection.
#[tauri::command]
pub async fn save_mcp_server(app: AppHandle, sh: State<'_, Shared>, server: McpServer, secret: Option<String>) -> R<Value> {
    let mut server = server;
    server.name = server.name.trim().to_string();
    server.command = server.command.trim().to_string();
    server.args.retain(|a| !a.trim().is_empty());
    crate::validate::mcp_server(&server)?;
    let mut s = settings::load(&sh.db);
    if s.mcp_servers.iter().any(|m| m.id != server.id && m.name.eq_ignore_ascii_case(&server.name)) {
        return Err("Name: a server with this name already exists".into());
    }
    if server.id.is_empty() {
        server.id = new_id();
    }
    if let Some(sec) = secret.filter(|x| !x.trim().is_empty()) {
        let key = if server.transport == "http" { format!("mcp:{}", server.id) } else { format!("mcp-env:{}", server.id) };
        crate::secrets::set(&key, sec.trim()).map_err(e)?;
    }
    match s.mcp_servers.iter_mut().find(|m| m.id == server.id) {
        Some(m) => *m = server.clone(),
        None => s.mcp_servers.push(server.clone()),
    }
    settings::save(&sh.db, &s).map_err(e)?;
    let _ = app.emit("settings-changed", &s);
    sh.mcp.drop_conn(&server.id);
    Ok(mcp_test(&sh, &server, s.local_only).await)
}

async fn mcp_test(sh: &Shared, server: &McpServer, local_only: bool) -> Value {
    if !server.enabled {
        return json!({"id": server.id, "ok": false, "error": "Disabled", "tools": []});
    }
    match tokio::time::timeout(std::time::Duration::from_secs(60), sh.mcp.ensure(server, local_only)).await {
        Ok(Ok(c)) => {
            let tools: Vec<Value> = c.tools.lock().iter().map(|t| json!({"name": t.name, "description": t.description})).collect();
            json!({"id": server.id, "ok": true, "tools": tools})
        }
        Ok(Err(err)) => json!({"id": server.id, "ok": false, "error": err.to_string(), "tools": []}),
        Err(_) => json!({"id": server.id, "ok": false, "error": "Timed out after 60 seconds while connecting", "tools": []}),
    }
}

#[tauri::command]
pub async fn test_mcp_server(sh: State<'_, Shared>, id: String) -> R<Value> {
    let s = settings::load(&sh.db);
    let server = s.mcp_servers.iter().find(|m| m.id == id).cloned().ok_or("Server not found")?;
    sh.mcp.drop_conn(&id);
    Ok(mcp_test(&sh, &server, s.local_only).await)
}

#[tauri::command]
pub fn delete_mcp_server(app: AppHandle, sh: State<'_, Shared>, id: String) -> R<()> {
    let mut s = settings::load(&sh.db);
    s.mcp_servers.retain(|m| m.id != id);
    settings::save(&sh.db, &s).map_err(e)?;
    sh.mcp.drop_conn(&id);
    crate::secrets::delete(&format!("mcp:{id}"));
    crate::secrets::delete(&format!("mcp-env:{id}"));
    let _ = app.emit("settings-changed", &s);
    Ok(())
}

#[tauri::command]
pub fn mcp_status(sh: State<'_, Shared>) -> Vec<Value> {
    let s = settings::load(&sh.db);
    s.mcp_servers
        .iter()
        .map(|m| {
            let (connected, error, tools) = sh.mcp.status(&m.id);
            let has_secret = crate::secrets::get(&format!("mcp:{}", m.id)).is_some() || crate::secrets::get(&format!("mcp-env:{}", m.id)).is_some();
            json!({"id": m.id, "connected": connected, "error": error, "tools": tools, "has_secret": has_secret})
        })
        .collect()
}

// ---------- Sandbox & tool catalog ----------

#[tauri::command]
pub async fn sandbox_detect() -> crate::sandbox::Availability {
    crate::sandbox::detect().await
}

#[tauri::command]
pub async fn sandbox_test(sh: State<'_, Shared>, cfg: SandboxCfg) -> R<crate::sandbox::CmdOutput> {
    if crate::sandbox::target(&cfg) != "sandbox" {
        return Err("Choose a sandbox and fill in its details first".into());
    }
    let r = crate::sandbox::run(&sh.vm, &cfg, &sh.data_dir.join("sandbox-drop"), "uname -a; cat /etc/os-release 2>/dev/null | head -2; echo sandbox-ok", false).await;
    crate::audit::ai(&sh.db, "", "sandbox.test", &cfg.kind, if r.is_ok() { "ok" } else { "error" }, json!({"error": r.as_ref().err().map(|x| x.to_string())}));
    r.map_err(e)
}

/// Runs a command typed by the user in the sandbox terminal (never on the host).
#[tauri::command]
pub async fn sandbox_exec(sh: State<'_, Shared>, command: String) -> R<crate::sandbox::CmdOutput> {
    let s = settings::load(&sh.db);
    if crate::sandbox::target(&s.sandbox) != "sandbox" {
        return Err("No sandbox is set up yet".into());
    }
    let r = crate::sandbox::run(&sh.vm, &s.sandbox, &sh.data_dir.join("sandbox-drop"), &command, false).await;
    crate::audit::log(&sh.db, "sandbox.exec", &command, json!({"code": r.as_ref().ok().map(|o| o.code), "error": r.as_ref().err().map(|x| x.to_string())}));
    r.map_err(e)
}

/// Opens the shared folder that is mirrored into the sandbox as /work.
#[tauri::command]
pub fn sandbox_folder(sh: State<'_, Shared>) -> R<String> {
    let d = sh.data_dir.join("sandbox-drop");
    std::fs::create_dir_all(&d).map_err(e)?;
    Ok(d.to_string_lossy().to_string())
}

// ---------- Built-in Linux VM ----------

#[tauri::command]
pub fn vm_status(sh: State<'_, Shared>) -> crate::microvm::Status {
    sh.vm.status()
}

#[tauri::command]
pub async fn vm_install(app: AppHandle, sh: State<'_, Shared>) -> R<crate::microvm::Status> {
    let r = sh.vm.install(&sh.http).await;
    crate::audit::log(&sh.db, "sandbox.vm_install", "built-in Linux VM", json!({"ok": r.is_ok(), "error": r.as_ref().err().map(|x| x.to_string())}));
    r.map_err(e)?;
    // Downloading it is the clearest sign the user wants it: make it the sandbox.
    let mut s = settings::load(&sh.db);
    if s.sandbox.kind == "none" {
        s.sandbox.kind = "microvm".into();
        settings::save(&sh.db, &s).map_err(e)?;
        let _ = app.emit("settings-changed", &s);
    }
    Ok(sh.vm.status())
}

#[tauri::command]
pub async fn vm_remove(app: AppHandle, sh: State<'_, Shared>) -> R<()> {
    sh.vm.remove().await.map_err(e)?;
    let mut s = settings::load(&sh.db);
    if s.sandbox.kind == "microvm" {
        s.sandbox.kind = "none".into();
        settings::save(&sh.db, &s).map_err(e)?;
        let _ = app.emit("settings-changed", &s);
    }
    Ok(())
}

#[tauri::command]
pub async fn vm_start(sh: State<'_, Shared>) -> R<crate::microvm::Status> {
    let mem = settings::load(&sh.db).sandbox.vm_memory_mb as u64;
    sh.vm.ensure_started(mem).await.map_err(e)?;
    Ok(sh.vm.status())
}

#[tauri::command]
pub fn vm_stop(sh: State<'_, Shared>) {
    sh.vm.stop();
}

#[tauri::command]
pub fn vm_image(sh: State<'_, Shared>, name: String) -> R<tauri::ipc::Response> {
    sh.vm.image(&name).map(tauri::ipc::Response::new).map_err(e)
}

#[tauri::command]
pub fn vm_ready(sh: State<'_, Shared>, boot_ms: u64) {
    sh.vm.on_ready(boot_ms);
    crate::audit::system(&sh.db, "sandbox.vm_boot", "built-in Linux VM", json!({"boot_ms": boot_ms}));
}

#[tauri::command]
pub fn vm_failed(sh: State<'_, Shared>, error: String) {
    crate::audit::system(&sh.db, "sandbox.vm_error", "built-in Linux VM", json!({"error": error}));
    sh.vm.fail(&error);
}

#[tauri::command]
pub fn vm_result(sh: State<'_, Shared>, result: Value) {
    sh.vm.on_result(result);
}

#[tauri::command]
pub async fn list_all_tools(sh: State<'_, Shared>) -> R<Value> {
    let s = settings::load(&sh.db);
    let mut builtin: Vec<Value> = crate::tools::agent_tools().into_iter().map(|t| json!({"name": t.name, "description": t.description, "scope": "Agent"})).collect();
    let all_chat = crate::tools::chat_tools(&std::collections::HashMap::new());
    for t in crate::toolbox::builtin_defs(&s).into_iter().chain(all_chat) {
        let perm = s.tool_permissions.get(&t.name).cloned().unwrap_or_else(|| "ask".into());
        match builtin.iter_mut().find(|b| b["name"] == t.name.as_str()) {
            Some(b) => {
                b["scope"] = json!("Chat + Agent");
                b["permission"] = json!(perm);
            }
            None => builtin.push(json!({"name": t.name, "description": t.description, "scope": "Chat + Agent", "permission": perm})),
        }
    }
    let ext: Vec<Value> = crate::toolbox::extension_tools(&sh, &s)
        .await
        .into_iter()
        .map(|t| json!({"name": t.def.name, "description": t.def.description, "origin": t.origin, "permission": t.permission}))
        .collect();
    Ok(json!({"builtin": builtin, "extensions": ext}))
}

#[tauri::command]
pub fn data_dir(sh: State<'_, Shared>) -> String {
    sh.data_dir.to_string_lossy().to_string()
}

/// Reads a small JSON file the user picked (settings import).
#[tauri::command]
pub fn read_text_file(path: String) -> R<String> {
    let p = std::path::Path::new(&path);
    if p.extension().and_then(|x| x.to_str()).map(|x| x.eq_ignore_ascii_case("json")) != Some(true) {
        return Err("Choose a .json file".into());
    }
    let meta = std::fs::metadata(p).map_err(e)?;
    if meta.len() > 2_000_000 {
        return Err("That file is too large to be a settings export".into());
    }
    std::fs::read_to_string(p).map_err(e)
}

// ---------- Built-in llama.cpp engine ----------

fn llama() -> R<std::sync::Arc<crate::llamacpp::Llama>> {
    crate::llamacpp::global().ok_or_else(|| "The built-in engine is not ready".to_string())
}

#[tauri::command]
pub fn llama_status() -> R<crate::llamacpp::Status> {
    Ok(llama()?.status())
}

/// Builds for this computer, with the recommended one first.
#[tauri::command]
pub fn llama_variants() -> Vec<Value> {
    let gpus = crate::models::gpu_names().join(" ").to_lowercase();
    let mut v: Vec<Value> = crate::llamacpp::variants_here().into_iter().map(|v| json!({"id": v.id, "label": v.label, "about": v.about, "size": v.assets_size()})).collect();
    // Vulkan is the safe GPU default; with no GPU at all, recommend the CPU build.
    let has_gpu = ["nvidia", "radeon", "amd", "intel(r) arc", "geforce", "apple"].iter().any(|g| gpus.contains(g)) || cfg!(target_os = "macos");
    let rec = v.iter().position(|x| if has_gpu { x["label"].as_str().unwrap_or("").contains("Vulkan") || x["label"].as_str().unwrap_or("").contains("Metal") } else { x["label"] == "CPU only" }).unwrap_or(0);
    if let Some(x) = v.get_mut(rec) {
        x["recommended"] = json!(true);
    }
    v
}

#[tauri::command]
pub async fn llama_install(app: AppHandle, sh: State<'_, Shared>, variant: String) -> R<crate::llamacpp::Status> {
    let l = llama()?;
    let r = l.install(&variant).await;
    crate::audit::log(&sh.db, "llama.install", &variant, json!({"tag": crate::llamacpp::TAG, "ok": r.is_ok(), "error": r.as_ref().err().map(|x| x.to_string())}));
    r.map_err(e)?;
    let s = settings::load(&sh.db);
    crate::llamacpp::ensure_provider(&sh.db, s.llama.port).map_err(e)?;
    let _ = app.emit("settings-changed", &s);
    Ok(l.status())
}

#[tauri::command]
pub fn llama_uninstall(sh: State<'_, Shared>) -> R<()> {
    crate::audit::log(&sh.db, "llama.uninstall", "", json!({}));
    llama()?.uninstall().map_err(e)
}

/// Reads a GGUF file's header so the import dialog can show what it is.
#[tauri::command]
pub async fn llama_inspect(path: String) -> R<Value> {
    tokio::task::spawn_blocking(move || {
        let p = std::path::PathBuf::from(&path);
        let info = crate::llamacpp::gguf_info(&p).map_err(e)?;
        let projector = if info.is_projector { None } else { crate::llamacpp::find_projector(&p).map(|x| x.to_string_lossy().to_string()) };
        Ok(json!({"info": info, "projector": projector}))
    })
    .await
    .map_err(e)?
}

fn import_one(app: &AppHandle, sh: &Shared, path: &str, mmproj: &str, ctx: u32) -> R<String> {
    let p = std::path::PathBuf::from(path);
    if p.extension().map(|x| !x.eq_ignore_ascii_case("gguf")).unwrap_or(true) {
        return Err("Model: choose a .gguf file".into());
    }
    let info = crate::llamacpp::gguf_info(&p).map_err(|x| format!("Model: {x}"))?;
    if info.is_projector {
        return Err("Model: this file is a vision projector (mmproj). Choose the model file, then add this as its projector.".into());
    }
    if !mmproj.is_empty() {
        let pi = crate::llamacpp::gguf_info(std::path::Path::new(mmproj)).map_err(|x| format!("Vision projector: {x}"))?;
        if !pi.is_projector {
            return Err("Vision projector: this file is a model, not an mmproj projector".into());
        }
    }
    let mut s = settings::load(&sh.db);
    if let Some(existing) = s.llama.models.iter().find(|m| std::path::Path::new(&m.path) == p.as_path()) {
        return Err(format!("Model: already imported as {}", existing.id));
    }
    let taken: Vec<String> = s.llama.models.iter().map(|m| m.id.clone()).collect();
    let alias = crate::llamacpp::alias_for(&p, &taken);
    s.llama.models.push(crate::settings::LlamaModel { id: alias.clone(), path: path.into(), mmproj: mmproj.into(), ctx, size: info.size, arch: info.arch.clone(), train_ctx: info.context_length, embedding: info.is_embedding });
    settings::save(&sh.db, &s).map_err(e)?;
    let pid = crate::llamacpp::ensure_provider(&sh.db, s.llama.port).map_err(e)?;
    let mid = crate::models::add_model(&sh.db, &pid, &alias).map_err(e)?;
    // The file's own facts beat name guesses: a projector means it can see.
    if !mmproj.is_empty() {
        let _ = crate::models::update_model(&sh.db, &mid, &alias, &crate::models::Overrides { vision: Some(true), ..Default::default() }, 0.0, 0.0);
    }
    if info.is_embedding {
        let _ = crate::models::update_model(&sh.db, &mid, &alias, &crate::models::Overrides { embedding: Some(true), ..Default::default() }, 0.0, 0.0);
    }
    let mut s = settings::load(&sh.db);
    if crate::models::heal_model_refs(&sh.db, &mut s) {
        let _ = settings::save(&sh.db, &s);
    }
    let _ = app.emit("settings-changed", &s);
    crate::audit::log(&sh.db, "llama.import", &alias, json!({"path": path, "mmproj": mmproj, "arch": info.arch, "size": info.size}));
    Ok(mid)
}

#[tauri::command]
pub fn llama_import(app: AppHandle, sh: State<'_, Shared>, path: String, mmproj: String, ctx: u32) -> R<String> {
    if ctx != 0 && !(512..=1_048_576).contains(&ctx) {
        return Err("Context size: use 0 (default) or 512 to 1048576".into());
    }
    import_one(&app, &sh, path.trim(), mmproj.trim(), ctx)
}

/// Imports every model GGUF in a folder, pairing each with its projector when there is one.
#[tauri::command]
pub async fn llama_import_folder(app: AppHandle, sh: State<'_, Shared>, dir: String) -> R<Value> {
    let d = std::path::PathBuf::from(&dir);
    let files: Vec<std::path::PathBuf> = std::fs::read_dir(&d).map_err(e)?.flatten().map(|x| x.path()).filter(|p| p.extension().map(|x| x.eq_ignore_ascii_case("gguf")).unwrap_or(false)).collect();
    let (mut added, mut skipped) = (vec![], vec![]);
    for f in files {
        let info = match crate::llamacpp::gguf_info(&f) {
            Ok(i) => i,
            Err(x) => {
                skipped.push(format!("{}: {x}", f.display()));
                continue;
            }
        };
        if info.is_projector {
            continue;
        }
        // Split models (-00002-of-00003) load from their first part only.
        let name = f.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if name.contains("-of-0") && !name.contains("-00001-of-") {
            continue;
        }
        let mm = crate::llamacpp::find_projector(&f).map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
        match import_one(&app, &sh, &f.to_string_lossy(), &mm, 0) {
            Ok(_) => added.push(f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
            Err(x) => skipped.push(x),
        }
    }
    Ok(json!({"added": added, "skipped": skipped}))
}

#[tauri::command]
pub fn llama_update_model(app: AppHandle, sh: State<'_, Shared>, id: String, mmproj: String, ctx: u32) -> R<()> {
    if ctx != 0 && !(512..=1_048_576).contains(&ctx) {
        return Err("Context size: use 0 (default) or 512 to 1048576".into());
    }
    if !mmproj.trim().is_empty() {
        let pi = crate::llamacpp::gguf_info(std::path::Path::new(mmproj.trim())).map_err(|x| format!("Vision projector: {x}"))?;
        if !pi.is_projector {
            return Err("Vision projector: this file is a model, not an mmproj projector".into());
        }
    }
    let mut s = settings::load(&sh.db);
    let m = s.llama.models.iter_mut().find(|m| m.id == id).ok_or("Model not found")?;
    m.mmproj = mmproj.trim().into();
    m.ctx = ctx;
    settings::save(&sh.db, &s).map_err(e)?;
    // Reload with the new options next time it is used.
    if llama()?.status().current.as_deref() == Some(id.as_str()) {
        llama()?.stop();
    }
    let _ = app.emit("settings-changed", &s);
    Ok(())
}

/// Removes a model from the app. The GGUF file on disk is left alone.
#[tauri::command]
pub fn llama_remove_model(app: AppHandle, sh: State<'_, Shared>, id: String) -> R<()> {
    let l = llama()?;
    if l.status().current.as_deref() == Some(id.as_str()) {
        l.stop();
    }
    let mut s = settings::load(&sh.db);
    s.llama.models.retain(|m| m.id != id);
    if let Some(p) = crate::models::list_providers(&sh.db).map_err(e)?.into_iter().find(|p| p["kind"] == "llamacpp") {
        let pid = p["id"].as_str().unwrap_or("");
        if let Some(m) = crate::models::list_models(&sh.db).map_err(e)?.into_iter().find(|m| m.provider_id == pid && m.name == id) {
            crate::models::delete_model(&sh.db, &m.id).map_err(e)?;
        }
    }
    crate::models::heal_model_refs(&sh.db, &mut s);
    settings::save(&sh.db, &s).map_err(e)?;
    let _ = app.emit("settings-changed", &s);
    Ok(())
}

#[tauri::command]
pub async fn llama_load(id: String) -> R<crate::llamacpp::Status> {
    let l = llama()?;
    l.ensure(&id).await.map_err(e)?;
    Ok(l.status())
}

#[tauri::command]
pub fn llama_unload() -> R<()> {
    llama()?.stop();
    Ok(())
}

#[tauri::command]
pub fn llama_log() -> R<String> {
    Ok(llama()?.log_tail(200))
}


// ---------------- Documents, embeddings, devices, stats ----------------

/// Attaches a document to a chat, from a path or from base64 data.
#[tauri::command]
pub async fn docs_add(sh: State<'_, Shared>, conversation_id: String, name: String, mode: String, path: Option<String>, data: Option<String>) -> R<Value> {
    use base64::Engine;
    let (name, bytes) = match (path, data) {
        (Some(p), _) if !p.trim().is_empty() => {
            let pb = std::path::PathBuf::from(p.trim());
            let meta = std::fs::metadata(&pb).map_err(|_| "File: not found".to_string())?;
            if meta.len() as usize > crate::docs::MAX_DOC_BYTES {
                return Err("File: larger than 25 MB".into());
            }
            let n = pb.file_name().map(|x| x.to_string_lossy().to_string()).unwrap_or(name);
            (n, std::fs::read(&pb).map_err(e)?)
        }
        (_, Some(d)) => (name, base64::engine::general_purpose::STANDARD.decode(d.as_bytes()).map_err(|_| "File: could not be read".to_string())?),
        _ => return Err("File: choose a document".into()),
    };
    let name: String = name.chars().filter(|c| !c.is_control()).take(200).collect();
    if !crate::docs::supported(&name) {
        return Err(format!("{name}: this file type is not supported. Use PDF, Word, PowerPoint, Excel, text, Markdown, CSV, JSON or code files."));
    }
    crate::docs::add(&sh, &conversation_id, &name, &bytes, &mode).await.map_err(e)
}

#[tauri::command]
pub fn docs_list(sh: State<'_, Shared>, conversation_id: String) -> R<Vec<Value>> {
    crate::docs::list(&sh.db, &conversation_id).map_err(e)
}

#[tauri::command]
pub fn docs_remove(sh: State<'_, Shared>, id: String) -> R<()> {
    crate::docs::remove(&sh.db, &id).map_err(e)
}

#[tauri::command]
pub async fn llama_devices(refresh: bool) -> R<Vec<crate::llamacpp::Device>> {
    let l = crate::llamacpp::global().ok_or("The engine is not ready")?;
    l.devices(refresh).await.map_err(e)
}

#[tauri::command]
pub fn llama_embed_catalog(sh: State<'_, Shared>) -> Vec<Value> {
    let s = settings::load(&sh.db);
    crate::llamacpp::EMBED_MODELS
        .iter()
        .map(|m| {
            let installed = s.llama.models.iter().any(|x| x.path.replace('\\', "/").ends_with(m.file));
            json!({"id": m.id, "label": m.label, "note": m.note, "size": m.size, "installed": installed})
        })
        .collect()
}

/// Downloads a catalogue embedding model, imports it and makes it the embedding model if none is set.
#[tauri::command]
pub async fn llama_fetch_embed(app: AppHandle, sh: State<'_, Shared>, id: String) -> R<String> {
    let l = crate::llamacpp::global().ok_or("The engine is not ready")?;
    let path = l.fetch_embed_model(&id).await.map_err(e)?;
    let ps = path.to_string_lossy().to_string();
    let s = settings::load(&sh.db);
    let mid = match s.llama.models.iter().find(|m| m.path == ps) {
        Some(m) => crate::models::list_models(&sh.db).map_err(e)?.into_iter().find(|x| x.provider_kind == "llamacpp" && x.name == m.id).map(|x| x.id).ok_or("Imported model is missing")?,
        None => import_one(&app, &sh, &ps, "", 0)?,
    };
    let mut s = settings::load(&sh.db);
    if s.embed_model.is_none() {
        s.embed_model = Some(mid.clone());
        settings::save(&sh.db, &s).map_err(e)?;
        let _ = app.emit("settings-changed", &s);
    }
    Ok(mid)
}

/// Usage and speed figures for the Stats screen over the last `days` days (0 = all time).
#[tauri::command]
pub fn stats(sh: State<'_, Shared>, days: u32) -> R<Value> {
    let since = if days == 0 { 0 } else { crate::db::now() - days as i64 * 86_400_000 };
    let conn = sh.db.conn.lock();
    let one = |sql: &str| -> i64 { conn.query_row(sql, [since], |r| r.get(0)).unwrap_or(0) };
    let totals = conn
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(input),0),COALESCE(SUM(output),0),COALESCE(SUM(cost),0),COALESCE(AVG(NULLIF(tps,0)),0),COALESCE(AVG(NULLIF(ttft_ms,0)),0),
                    COALESCE(SUM(outcome='error'),0),COALESCE(SUM(outcome='stopped'),0) FROM replies WHERE ts>=?1",
            [since],
            |r| Ok(json!({"replies": r.get::<_, i64>(0)?, "input": r.get::<_, i64>(1)?, "output": r.get::<_, i64>(2)?, "cost": r.get::<_, f64>(3)?, "avg_tps": r.get::<_, f64>(4)?, "avg_ttft_ms": r.get::<_, f64>(5)?, "errors": r.get::<_, i64>(6)?, "stopped": r.get::<_, i64>(7)?})),
        )
        .map_err(e)?;
    let mut by_model = vec![];
    {
        let mut st = conn
            .prepare(
                "SELECT model_id,model,provider,COUNT(*),SUM(input),SUM(output),SUM(cost),COALESCE(AVG(NULLIF(tps,0)),0),COALESCE(MAX(tps),0),COALESCE(AVG(NULLIF(ttft_ms,0)),0),MAX(ts),SUM(outcome='error')
                 FROM replies WHERE ts>=?1 GROUP BY model_id ORDER BY COUNT(*) DESC",
            )
            .map_err(e)?;
        let rows = st
            .query_map([since], |r| {
                Ok(json!({"model_id": r.get::<_, String>(0)?, "model": r.get::<_, String>(1)?, "provider": r.get::<_, String>(2)?, "replies": r.get::<_, i64>(3)?, "input": r.get::<_, i64>(4)?, "output": r.get::<_, i64>(5)?,
                    "cost": r.get::<_, f64>(6)?, "avg_tps": r.get::<_, f64>(7)?, "max_tps": r.get::<_, f64>(8)?, "avg_ttft_ms": r.get::<_, f64>(9)?, "last": r.get::<_, i64>(10)?, "errors": r.get::<_, i64>(11)?}))
            })
            .map_err(e)?;
        by_model.extend(rows.filter_map(|x| x.ok()));
    }
    let mut daily = vec![];
    {
        let mut st = conn
            .prepare("SELECT strftime('%Y-%m-%d', ts/1000, 'unixepoch', 'localtime') d, COUNT(*), SUM(input), SUM(output), SUM(cost) FROM replies WHERE ts>=?1 GROUP BY d ORDER BY d")
            .map_err(e)?;
        let rows = st
            .query_map([since], |r| Ok(json!({"day": r.get::<_, String>(0)?, "replies": r.get::<_, i64>(1)?, "input": r.get::<_, i64>(2)?, "output": r.get::<_, i64>(3)?, "cost": r.get::<_, f64>(4)?})))
            .map_err(e)?;
        daily.extend(rows.filter_map(|x| x.ok()));
    }
    let mut hours = vec![0i64; 24];
    {
        let mut st = conn.prepare("SELECT CAST(strftime('%H', ts/1000, 'unixepoch', 'localtime') AS INTEGER), COUNT(*) FROM replies WHERE ts>=?1 GROUP BY 1").map_err(e)?;
        for (h, n) in st.query_map([since], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))).map_err(e)?.flatten() {
            if (0..24).contains(&h) {
                hours[h as usize] = n;
            }
        }
    }
    let mut tools = vec![];
    {
        let mut st = conn
            .prepare("SELECT subject, COUNT(*), SUM(outcome<>'ok') FROM audit WHERE action IN ('chat.tool','chat.subagent') AND ts>=?1 GROUP BY subject ORDER BY COUNT(*) DESC LIMIT 12")
            .map_err(e)?;
        let rows = st.query_map([since], |r| Ok(json!({"name": r.get::<_, String>(0)?, "calls": r.get::<_, i64>(1)?, "failed": r.get::<_, i64>(2)?}))).map_err(e)?;
        tools.extend(rows.filter_map(|x| x.ok()));
    }
    let mut tasks = json!({});
    {
        let mut st = conn.prepare("SELECT status, COUNT(*) FROM tasks WHERE started>=?1 GROUP BY status").map_err(e)?;
        for (k, n) in st.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))).map_err(e)?.flatten() {
            tasks[k] = json!(n);
        }
    }
    Ok(json!({
        "totals": totals, "by_model": by_model, "daily": daily, "hours": hours, "tools": tools, "tasks": tasks,
        "chats": one("SELECT COUNT(*) FROM conversations WHERE created>=?1"),
        "messages": one("SELECT COUNT(*) FROM messages WHERE created>=?1 AND role='user'"),
        "documents": one("SELECT COUNT(*) FROM documents WHERE created>=?1"),
        "agent_steps": one("SELECT COUNT(*) FROM steps WHERE created>=?1"),
    }))
}

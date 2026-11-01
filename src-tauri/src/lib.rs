mod a11y;
mod agent;
mod audit;
mod chat;
mod commands;
mod db;
mod docs;
mod subagents;
mod ext_commands;
mod files;
mod input;
mod llamacpp;
mod llm;
mod mcp;
mod microvm;
mod plugins;
mod sandbox;
mod toolbox;
mod models;
mod safety;
mod screen;
mod secrets;
mod settings;
mod state;
mod tools;
mod validate;
mod xtools;

use parking_lot::Mutex;
use state::{AgentControl, Shared};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn register_shortcuts(app: &AppHandle, s: &settings::AppSettings) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    for (name, combo) in [("stop", &s.stop_shortcut), ("summon", &s.summon_shortcut)] {
        if combo.trim().is_empty() {
            continue;
        }
        if let Err(e) = gs.register(combo.as_str()) {
            eprintln!("could not register {name} shortcut '{combo}': {e}");
        }
    }
}

fn on_shortcut(app: &AppHandle, shortcut: &Shortcut) {
    let Some(sh) = app.try_state::<Shared>() else { return };
    let s = settings::load(&sh.db);
    if Shortcut::from_str(&s.stop_shortcut).map(|x| &x == shortcut).unwrap_or(false) {
        state::emergency_stop(app);
    } else if Shortcut::from_str(&s.summon_shortcut).map(|x| &x == shortcut).unwrap_or(false) {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

fn prune_screenshots(dir: &std::path::Path, days: u32) {
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(days as u64 * 86400);
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.metadata().and_then(|m| m.modified()).map(|t| t < cutoff).unwrap_or(false) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// Writes panics to a local crash log (no data leaves the machine) and keeps the default output.
/// Wraps the command handler so every state-changing command lands in the audit trail.
fn audited<R: tauri::Runtime>(h: impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke<R>) -> bool + Send + Sync + 'static {
    move |invoke| {
        audit::on_invoke(&invoke);
        h(invoke)
    }
}

fn install_crash_log(path: std::path::PathBuf) {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "[{}] v{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"), env!("CARGO_PKG_VERSION"), info);
        }
        state::release_inputs();
        default(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // A second launch focuses the running app instead of starting another agent.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        on_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(data_dir.join("logs"))?;
            install_crash_log(data_dir.join("logs").join("crash.log"));
            let db = Arc::new(db::Db::open(&data_dir.join("agent.db"))?);
            // Anything still "running" was cut off by a crash or restart: offer resume, never re-run silently.
            let _ = db.conn.lock().execute("UPDATE tasks SET status='interrupted', summary='Interrupted (app closed or crashed). You can resume it.' WHERE status='running'", []);
            let http = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .timeout(std::time::Duration::from_secs(600))
                .build()?;
            let shared = Shared {
                db: db.clone(),
                data_dir: data_dir.clone(),
                http,
                agent: Arc::new(AgentControl::default()),
                a11y: Arc::new(a11y::Worker::start()),
                chat_cancels: Arc::new(Mutex::new(HashMap::new())),
                mcp: Arc::new(mcp::Manager::default()),
                vm: Arc::new(microvm::Vm::default()),
            };
            shared.vm.init(app.handle().clone(), &data_dir);
            let llama = llamacpp::init(app.handle().clone(), db.clone(), shared.http.clone(), &data_dir);
            let mut s = settings::load(&db);
            if models::heal_model_refs(&db, &mut s) {
                let _ = settings::save(&db, &s);
            }
            files::purge_expired(&db, &shared.holding_dir(), s.undo_hours);
            audit::prune(&db, s.audit_retention_days);
            audit::system(&db, "app.start", env!("CARGO_PKG_VERSION"), serde_json::json!({"os": std::env::consts::OS}));
            {
                // Give an idle built-in VM's memory back.
                let vm = shared.vm.clone();
                let db = db.clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        let s = settings::load(&db);
                        vm.idle_check(s.sandbox.vm_idle_minutes as u64);
                        llama.idle_check(s.llama.idle_minutes as u64);
                    }
                });
            }
            prune_screenshots(&shared.shots_dir(), s.keep_screenshots_days.max(1));
            app.manage(shared);
            // Windows are created only now, so no page can call a command before the state exists.
            for cfg in app.config().app.windows.clone() {
                tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?.build()?;
            }
            register_shortcuts(app.handle(), &s);

            let show = MenuItem::with_id(app, "show", "Show Computer Agent", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "Emergency stop", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &stop, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main").menu(&menu).tooltip("Computer Agent").on_menu_event(|app, ev| match ev.id().as_ref() {
                "show" => {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.unminimize();
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
                "stop" => state::emergency_stop(app),
                "quit" => {
                    state::emergency_stop(app);
                    if let Some(l) = llamacpp::global() {
                        l.stop_all();
                    }
                    app.exit(0);
                }
                _ => {}
            });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    state::emergency_stop(window.app_handle());
                    if let Some(l) = llamacpp::global() {
                        l.stop_all();
                    }
                    window.app_handle().exit(0);
                }
            }
        })
        .invoke_handler(audited(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::apply_profile,
            commands::export_settings,
            commands::import_settings,
            commands::reset_settings,
            commands::validate_shortcut,
            commands::test_provider,
            commands::list_providers,
            commands::add_provider,
            commands::update_provider,
            commands::delete_provider,
            commands::provider_remote_models,
            commands::detect_local,
            commands::list_models,
            commands::add_model,
            commands::update_model,
            commands::delete_model,
            commands::test_model,
            commands::hardware_info,
            commands::model_catalog,
            commands::ollama_pull,
            commands::ollama_delete,
            commands::ollama_unload,
            commands::list_conversations,
            commands::create_conversation,
            commands::get_conversation,
            commands::update_conversation,
            commands::delete_conversation,
            commands::branch_conversation,
            commands::send_message,
            commands::regenerate,
            commands::edit_and_resend,
            commands::stop_chat,
            commands::export_conversation,
            commands::capture_screenshot,
            commands::start_task,
            commands::resume_task,
            commands::agent_stop,
            commands::agent_pause,
            commands::agent_resume,
            commands::agent_respond,
            commands::agent_status,
            commands::cancel_queued,
            commands::list_tasks,
            commands::get_task_steps,
            commands::read_screenshot,
            commands::undo_task,
            commands::delete_task,
            commands::export_task,
            commands::list_playbooks,
            commands::save_playbook,
            commands::delete_playbook,
            commands::wipe_all_data,
            commands::platform_info,
            commands::diagnostics_report,
            commands::usage_summary,
            commands::record_consent,
            commands::has_consent,
            commands::reveal_path,
            commands::set_bar_size,
            ext_commands::list_plugins,
            ext_commands::create_plugin,
            ext_commands::import_plugin,
            ext_commands::delete_plugin,
            ext_commands::set_plugin_pref,
            ext_commands::plugins_dir,
            ext_commands::test_plugin_tool,
            ext_commands::save_mcp_server,
            ext_commands::test_mcp_server,
            ext_commands::delete_mcp_server,
            ext_commands::mcp_status,
            ext_commands::sandbox_detect,
            ext_commands::sandbox_test,
            ext_commands::list_all_tools,
            ext_commands::data_dir,
            ext_commands::read_text_file,
            ext_commands::sandbox_folder,
            ext_commands::sandbox_exec,
            ext_commands::vm_status,
            ext_commands::vm_install,
            ext_commands::vm_remove,
            ext_commands::vm_start,
            ext_commands::vm_stop,
            ext_commands::vm_image,
            ext_commands::vm_ready,
            ext_commands::vm_failed,
            ext_commands::vm_result,
            ext_commands::llama_status,
            ext_commands::llama_variants,
            ext_commands::llama_install,
            ext_commands::llama_uninstall,
            ext_commands::llama_inspect,
            ext_commands::llama_import,
            ext_commands::llama_import_folder,
            ext_commands::llama_update_model,
            ext_commands::llama_remove_model,
            ext_commands::llama_load,
            ext_commands::llama_unload,
            ext_commands::llama_log,
            ext_commands::docs_add,
            ext_commands::docs_list,
            ext_commands::docs_remove,
            ext_commands::llama_devices,
            ext_commands::llama_embed_catalog,
            ext_commands::llama_fetch_embed,
            ext_commands::stats,
            commands::audit_query,
            commands::audit_export,
            commands::audit_verify,
            commands::fork_conversation,
            commands::save_text_file,
            commands::open_link,
            commands::rate_message,
        ]))
        .run(tauri::generate_context!())
        .expect("error while running Computer Agent");
}

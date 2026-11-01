//! Shared engine state and the agent control block (stop, pause, prompts, queue).

use crate::a11y::Worker;
use crate::db::Db;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{oneshot, Notify};

#[derive(Default)]
pub struct AgentControl {
    pub running: AtomicBool,
    pub stop: AtomicBool,
    pub paused: AtomicBool,
    pub waiting_user: AtomicBool,
    pub agent_moving: AtomicBool,
    pub last_agent_pos: Mutex<(i32, i32)>,
    pub stop_notify: Notify,
    pub pending: Mutex<HashMap<String, (Value, oneshot::Sender<Value>)>>,
    pub status: Mutex<Value>,
    pub queue: Mutex<VecDeque<QueuedTask>>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct QueuedTask {
    pub id: String,
    pub goal: String,
    pub conversation_id: Option<String>,
    pub resume_of: Option<String>,
    /// The model picked in the chat, used when no Doer role is set.
    pub model_id: Option<String>,
}

impl AgentControl {
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.paused.store(false, Ordering::SeqCst);
        self.stop_notify.notify_waiters();
        // Resolve any open prompt so nothing waits forever.
        let pending: Vec<_> = self.pending.lock().drain().collect();
        for (_, (_, tx)) in pending {
            let _ = tx.send(json!({"decision": "stop"}));
        }
    }
}

#[derive(Clone)]
pub struct Shared {
    pub db: Arc<Db>,
    pub data_dir: PathBuf,
    pub http: reqwest::Client,
    pub agent: Arc<AgentControl>,
    pub a11y: Arc<Worker>,
    pub chat_cancels: Arc<Mutex<HashMap<String, Arc<Notify>>>>,
    pub mcp: Arc<crate::mcp::Manager>,
    pub vm: Arc<crate::microvm::Vm>,
}

impl Shared {
    pub fn holding_dir(&self) -> PathBuf {
        self.data_dir.join("holding")
    }
    pub fn shots_dir(&self) -> PathBuf {
        self.data_dir.join("shots")
    }
}

/// Emergency stop: halts new actions, releases held keys and buttons.
pub fn emergency_stop(app: &AppHandle) {
    if let Some(s) = app.try_state::<Shared>() {
        crate::audit::log(&s.db, "agent.emergency_stop", "stop", json!({}));
        s.agent.request_stop();
        for c in s.chat_cancels.lock().values() {
            c.notify_waiters();
        }
    }
    release_inputs();
    let _ = app.emit("agent-stop-requested", ());
}

pub fn release_inputs() {
    use enigo::{Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};
    if let Ok(mut e) = Enigo::new(&Settings::default()) {
        for k in [Key::Control, Key::Shift, Key::Alt, Key::Meta] {
            let _ = e.key(k, Direction::Release);
        }
        let _ = e.button(Button::Left, Direction::Release);
    }
}

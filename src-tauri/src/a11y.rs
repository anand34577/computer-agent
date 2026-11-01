//! Accessibility view. A dedicated worker thread owns the OS accessibility objects
//! (COM on Windows) so the agent can list, tag, press and fill controls by number.

use serde::Serialize;
use std::sync::mpsc;

#[derive(Clone, Debug, Serialize)]
pub struct Elem {
    pub id: usize,
    pub role: String,
    pub name: String,
    pub value: String,
    pub rect: [i32; 4],
    pub enabled: bool,
    pub password: bool,
    pub can_invoke: bool,
}

impl Elem {
    pub fn center(&self) -> (i32, i32) {
        ((self.rect[0] + self.rect[2]) / 2, (self.rect[1] + self.rect[3]) / 2)
    }
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct WinInfo {
    pub id: u32,
    pub title: String,
    pub app: String,
    pub minimized: bool,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Snapshot {
    /// Native handle of the window that was in front (0 if unknown).
    pub hwnd: isize,
    pub window_title: String,
    pub app: String,
    pub elements: Vec<Elem>,
    pub texts: Vec<String>,
    pub windows: Vec<WinInfo>,
    /// "good" | "thin" | "none"
    pub coverage: String,
}

impl Snapshot {
    /// Compact text map of the screen for text-only models.
    pub fn text_map(&self, include_texts: bool) -> String {
        let mut s = format!("Active window: \"{}\" ({})\n", self.window_title, self.app);
        if !self.windows.is_empty() {
            s.push_str("Open windows: ");
            let names: Vec<String> = self.windows.iter().take(15).map(|w| format!("\"{}\"", trunc(&w.title, 50))).collect();
            s.push_str(&names.join(", "));
            s.push('\n');
        }
        s.push_str(&format!("Interactive elements ({}):\n", self.elements.len()));
        for e in &self.elements {
            let mut line = format!("[{}] {} \"{}\"", e.id, e.role, trunc(&e.name, 80));
            if !e.value.is_empty() && !e.password {
                line.push_str(&format!(" value=\"{}\"", trunc(&e.value, 60)));
            }
            if e.password {
                line.push_str(" (password field)");
            }
            if !e.enabled {
                line.push_str(" (disabled)");
            }
            s.push_str(&line);
            s.push('\n');
        }
        if include_texts && !self.texts.is_empty() {
            s.push_str("Visible text:\n");
            for t in self.texts.iter().take(80) {
                s.push_str("- ");
                s.push_str(&trunc(t, 160));
                s.push('\n');
            }
        }
        s
    }

    pub fn find(&self, id: usize) -> Option<&Elem> {
        self.elements.iter().find(|e| e.id == id)
    }
}

pub fn trunc(s: &str, n: usize) -> String {
    let s = s.replace(['\n', '\r'], " ");
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n).collect::<String>())
    } else {
        s
    }
}

enum Cmd {
    Snapshot([i32; 4], mpsc::Sender<Snapshot>),
    Invoke(usize, mpsc::Sender<Result<String, String>>),
    Focus(usize, mpsc::Sender<Result<(), String>>),
}

pub struct Worker {
    tx: parking_lot::Mutex<mpsc::Sender<Cmd>>,
}

impl Worker {
    pub fn start() -> Self {
        let (tx, rx) = mpsc::channel::<Cmd>();
        std::thread::Builder::new().name("a11y".into()).spawn(move || imp::run(rx)).ok();
        Self { tx: parking_lot::Mutex::new(tx) }
    }

    /// `bounds` limits elements to the captured monitor.
    pub fn snapshot(&self, bounds: [i32; 4]) -> Snapshot {
        let (rtx, rrx) = mpsc::channel();
        if self.tx.lock().send(Cmd::Snapshot(bounds, rtx)).is_err() {
            return Snapshot { coverage: "none".into(), ..Default::default() };
        }
        rrx.recv_timeout(std::time::Duration::from_secs(8)).unwrap_or_else(|_| Snapshot { coverage: "none".into(), ..Default::default() })
    }

    /// Presses a control through the accessibility API (no mouse). Returns which method worked.
    pub fn invoke(&self, id: usize) -> Result<String, String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.lock().send(Cmd::Invoke(id, rtx)).map_err(|e| e.to_string())?;
        rrx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| "accessibility timeout".to_string())?
    }

    pub fn focus(&self, id: usize) -> Result<(), String> {
        let (rtx, rrx) = mpsc::channel();
        self.tx.lock().send(Cmd::Focus(id, rtx)).map_err(|e| e.to_string())?;
        rrx.recv_timeout(std::time::Duration::from_secs(5)).map_err(|_| "accessibility timeout".to_string())?
    }
}

pub fn list_windows() -> Vec<WinInfo> {
    xcap::Window::all()
        .map(|ws| {
            ws.into_iter()
                .filter_map(|w| {
                    let title = w.title().ok()?;
                    if title.trim().is_empty() {
                        return None;
                    }
                    let app = w.app_name().unwrap_or_default();
                    if ["Agent Bar", "Agent Pointer"].contains(&title.as_str()) {
                        return None;
                    }
                    Some(WinInfo { id: w.id().unwrap_or(0), title, app, minimized: w.is_minimized().unwrap_or(false) })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(windows)]
pub mod win {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE};

    pub fn foreground() -> isize {
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn activate(hwnd: isize) -> bool {
        if hwnd == 0 {
            return false;
        }
        let h = HWND(hwnd as *mut core::ffi::c_void);
        unsafe {
            if IsIconic(h).as_bool() {
                let _ = ShowWindow(h, SW_RESTORE);
            }
            // Windows only lets the foreground process change focus; a synthetic Alt tap lifts that lock.
            use enigo::{Direction, Enigo, Key, Keyboard, Settings};
            if let Ok(mut e) = Enigo::new(&Settings::default()) {
                let _ = e.key(Key::Alt, Direction::Click);
            }
            SetForegroundWindow(h).as_bool()
        }
    }
}

#[cfg(not(windows))]
pub mod win {
    pub fn foreground() -> isize {
        0
    }
    pub fn activate(_hwnd: isize) -> bool {
        false
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use uiautomation::patterns::{UIExpandCollapsePattern, UIInvokePattern, UISelectionItemPattern, UITogglePattern, UIValuePattern};
    use uiautomation::types::ControlType;
    use uiautomation::{UIAutomation, UIElement, UITreeWalker};

    fn role_name(ct: ControlType) -> Option<&'static str> {
        Some(match ct {
            ControlType::Button => "button",
            ControlType::SplitButton => "button",
            ControlType::CheckBox => "checkbox",
            ControlType::ComboBox => "combobox",
            ControlType::Edit => "textbox",
            ControlType::Hyperlink => "link",
            ControlType::ListItem => "listitem",
            ControlType::MenuItem => "menuitem",
            ControlType::RadioButton => "radio",
            ControlType::TabItem => "tab",
            ControlType::TreeItem => "treeitem",
            ControlType::Slider => "slider",
            ControlType::Spinner => "spinner",
            ControlType::DataItem => "dataitem",
            ControlType::Document => "document",
            _ => return None,
        })
    }

    struct Ctx<'a> {
        walker: &'a UITreeWalker,
        bounds: [i32; 4],
        elems: Vec<(Elem, UIElement)>,
        texts: Vec<String>,
        visited: usize,
    }

    fn walk(ctx: &mut Ctx, el: &UIElement, depth: usize) {
        if depth > 22 || ctx.visited > 1500 || ctx.elems.len() >= 250 {
            return;
        }
        ctx.visited += 1;
        let Ok(ct) = el.get_control_type() else { return };
        let offscreen = el.is_offscreen().unwrap_or(false);
        let rect = el.get_bounding_rectangle().ok().map(|r| [r.get_left(), r.get_top(), r.get_right(), r.get_bottom()]);
        let visible = rect
            .map(|r| r[2] > r[0] + 2 && r[3] > r[1] + 2 && r[2] > ctx.bounds[0] && r[0] < ctx.bounds[2] && r[3] > ctx.bounds[1] && r[1] < ctx.bounds[3])
            .unwrap_or(false);
        if !offscreen && visible {
            let name = el.get_name().unwrap_or_default();
            if let Some(role) = role_name(ct) {
                let is_doc = role == "document";
                if !is_doc || !name.is_empty() {
                    let password = el.is_password().unwrap_or(false);
                    let value = if password {
                        String::new()
                    } else {
                        el.get_pattern::<UIValuePattern>().and_then(|p| p.get_value()).unwrap_or_default()
                    };
                    let can_invoke = el.get_pattern::<UIInvokePattern>().is_ok();
                    let r = rect.unwrap();
                    let elem = Elem {
                        id: ctx.elems.len() + 1,
                        role: role.into(),
                        name: name.clone(),
                        value,
                        rect: [r[0].max(ctx.bounds[0]), r[1].max(ctx.bounds[1]), r[2].min(ctx.bounds[2]), r[3].min(ctx.bounds[3])],
                        enabled: el.is_enabled().unwrap_or(true),
                        password,
                        can_invoke,
                    };
                    if !is_doc {
                        ctx.elems.push((elem, el.clone()));
                    }
                }
            } else if matches!(ct, ControlType::Text) && !name.trim().is_empty() && ctx.texts.len() < 120 {
                ctx.texts.push(name);
            }
        }
        if offscreen && !matches!(ct, ControlType::Window | ControlType::Pane | ControlType::Document | ControlType::Group | ControlType::Custom) {
            return;
        }
        if let Ok(child) = ctx.walker.get_first_child(el) {
            let mut cur = child;
            loop {
                walk(ctx, &cur, depth + 1);
                match ctx.walker.get_next_sibling(&cur) {
                    Ok(n) => cur = n,
                    Err(_) => break,
                }
            }
        }
    }

    pub fn run(rx: mpsc::Receiver<Cmd>) {
        let Ok(auto) = UIAutomation::new() else {
            for cmd in rx {
                match cmd {
                    Cmd::Snapshot(_, r) => {
                        let _ = r.send(Snapshot { coverage: "none".into(), windows: list_windows(), ..Default::default() });
                    }
                    Cmd::Invoke(_, r) => {
                        let _ = r.send(Err("accessibility unavailable".into()));
                    }
                    Cmd::Focus(_, r) => {
                        let _ = r.send(Err("accessibility unavailable".into()));
                    }
                }
            }
            return;
        };
        let mut last: Vec<(Elem, UIElement)> = vec![];
        for cmd in rx {
            match cmd {
                Cmd::Snapshot(bounds, reply) => {
                    let mut snap = Snapshot { windows: list_windows(), ..Default::default() };
                    let fg = super::win::foreground();
                    snap.hwnd = fg;
                    if fg != 0 {
                        if let (Ok(root), Ok(walker)) = (auto.element_from_handle((fg).into()), auto.get_control_view_walker()) {
                            snap.window_title = root.get_name().unwrap_or_default();
                            snap.app = snap
                                .windows
                                .iter()
                                .find(|w| w.id as isize == fg)
                                .map(|w| w.app.clone())
                                .unwrap_or_default();
                            let mut ctx = Ctx { walker: &walker, bounds, elems: vec![], texts: vec![], visited: 0 };
                            walk(&mut ctx, &root, 0);
                            snap.texts = ctx.texts;
                            last = ctx.elems;
                        }
                    }
                    snap.elements = last.iter().map(|(e, _)| e.clone()).collect();
                    snap.coverage = match snap.elements.len() {
                        0 => "none",
                        1..=3 => "thin",
                        _ => "good",
                    }
                    .into();
                    let _ = reply.send(snap);
                }
                Cmd::Invoke(id, reply) => {
                    let res = match last.iter().find(|(e, _)| e.id == id) {
                        None => Err(format!("No element {id} on the current screen")),
                        Some((_, el)) => {
                            if let Ok(p) = el.get_pattern::<UIInvokePattern>() {
                                p.invoke().map(|_| "invoked".to_string()).map_err(|e| e.to_string())
                            } else if let Ok(p) = el.get_pattern::<UITogglePattern>() {
                                p.toggle().map(|_| "toggled".to_string()).map_err(|e| e.to_string())
                            } else if let Ok(p) = el.get_pattern::<UISelectionItemPattern>() {
                                p.select().map(|_| "selected".to_string()).map_err(|e| e.to_string())
                            } else if let Ok(p) = el.get_pattern::<UIExpandCollapsePattern>() {
                                p.expand().map(|_| "expanded".to_string()).map_err(|e| e.to_string())
                            } else {
                                Err("no accessibility action".into())
                            }
                        }
                    };
                    let _ = reply.send(res);
                }
                Cmd::Focus(id, reply) => {
                    let res = match last.iter().find(|(e, _)| e.id == id) {
                        None => Err(format!("No element {id}")),
                        Some((_, el)) => el.set_focus().map_err(|e| e.to_string()),
                    };
                    let _ = reply.send(res);
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;
    // macOS (AX API) and Linux (AT-SPI) back-ends are planned; until then the agent
    // uses vision with the honest "no accessibility view" banner.
    pub fn run(rx: mpsc::Receiver<Cmd>) {
        for cmd in rx {
            match cmd {
                Cmd::Snapshot(_, r) => {
                    let _ = r.send(Snapshot { coverage: "none".into(), windows: list_windows(), ..Default::default() });
                }
                Cmd::Invoke(_, r) => {
                    let _ = r.send(Err("accessibility view not available on this OS yet".into()));
                }
                Cmd::Focus(_, r) => {
                    let _ = r.send(Err("accessibility view not available on this OS yet".into()));
                }
            }
        }
    }
}

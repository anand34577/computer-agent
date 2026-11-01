//! Mouse and keyboard. All coordinates are physical screen pixels.

use anyhow::{anyhow, bail, Result};
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings};
use std::{thread::sleep, time::Duration};

fn enigo() -> Result<Enigo> {
    Enigo::new(&Settings::default()).map_err(|e| anyhow!("Input control unavailable: {e}"))
}

pub fn move_to(x: i32, y: i32) -> Result<()> {
    let mut e = enigo()?;
    // Short glide so the movement is visible and apps register hover.
    let (sx, sy) = e.location().unwrap_or((x, y));
    let steps = 8;
    for i in 1..=steps {
        let nx = sx + (x - sx) * i / steps;
        let ny = sy + (y - sy) * i / steps;
        e.move_mouse(nx, ny, Coordinate::Abs).map_err(|e| anyhow!("{e}"))?;
        sleep(Duration::from_millis(8));
    }
    Ok(())
}

pub fn click(x: i32, y: i32, button: &str, count: u32) -> Result<()> {
    move_to(x, y)?;
    let mut e = enigo()?;
    let b = match button {
        "right" => Button::Right,
        "middle" => Button::Middle,
        _ => Button::Left,
    };
    for i in 0..count.max(1) {
        e.button(b, Direction::Click).map_err(|e| anyhow!("{e}"))?;
        if i + 1 < count {
            sleep(Duration::from_millis(60));
        }
    }
    Ok(())
}

pub fn drag(x0: i32, y0: i32, x1: i32, y1: i32) -> Result<()> {
    move_to(x0, y0)?;
    let mut e = enigo()?;
    e.button(Button::Left, Direction::Press).map_err(|e| anyhow!("{e}"))?;
    sleep(Duration::from_millis(120));
    let steps = 20;
    for i in 1..=steps {
        e.move_mouse(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps, Coordinate::Abs).map_err(|e| anyhow!("{e}"))?;
        sleep(Duration::from_millis(15));
    }
    sleep(Duration::from_millis(120));
    e.button(Button::Left, Direction::Release).map_err(|e| anyhow!("{e}"))?;
    Ok(())
}

pub fn scroll(dir: &str, amount: i32) -> Result<()> {
    let mut e = enigo()?;
    let (len, axis) = match dir {
        "up" => (-amount, Axis::Vertical),
        "left" => (-amount, Axis::Horizontal),
        "right" => (amount, Axis::Horizontal),
        _ => (amount, Axis::Vertical),
    };
    e.scroll(len, axis).map_err(|e| anyhow!("{e}"))
}

pub fn type_text(text: &str) -> Result<()> {
    let mut e = enigo()?;
    // Chunked so a stop request can land between chunks of long text.
    for chunk in text.chars().collect::<Vec<_>>().chunks(40) {
        let s: String = chunk.iter().collect();
        e.text(&s).map_err(|e| anyhow!("{e}"))?;
        sleep(Duration::from_millis(15));
    }
    Ok(())
}

fn key_of(name: &str) -> Result<Key> {
    let n = name.trim().to_lowercase();
    Ok(match n.as_str() {
        "ctrl" | "control" => Key::Control,
        "shift" => Key::Shift,
        "alt" | "option" => Key::Alt,
        "win" | "windows" | "super" | "meta" | "cmd" | "command" => Key::Meta,
        "enter" | "return" => Key::Return,
        "tab" => Key::Tab,
        "esc" | "escape" => Key::Escape,
        "space" => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "up" | "arrowup" => Key::UpArrow,
        "down" | "arrowdown" => Key::DownArrow,
        "left" | "arrowleft" => Key::LeftArrow,
        "right" | "arrowright" => Key::RightArrow,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" | "pgup" => Key::PageUp,
        "pagedown" | "pgdn" => Key::PageDown,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "f3" => Key::F3,
        "f4" => Key::F4,
        "f5" => Key::F5,
        "f6" => Key::F6,
        "f7" => Key::F7,
        "f8" => Key::F8,
        "f9" => Key::F9,
        "f10" => Key::F10,
        "f11" => Key::F11,
        "f12" => Key::F12,
        "plus" => Key::Unicode('+'),
        s if s.chars().count() == 1 => Key::Unicode(s.chars().next().unwrap()),
        other => bail!("Unknown key '{other}'"),
    })
}

/// Presses a combination such as "ctrl+shift+s" or a single key such as "enter".
pub fn press_keys(combo: &str) -> Result<()> {
    let parts: Vec<&str> = combo.split('+').filter(|s| !s.trim().is_empty()).collect();
    if parts.is_empty() {
        bail!("No keys given");
    }
    let keys: Vec<Key> = parts.iter().map(|p| key_of(p)).collect::<Result<_>>()?;
    let mut e = enigo()?;
    let (mods, last) = keys.split_at(keys.len() - 1);
    for m in mods {
        e.key(*m, Direction::Press).map_err(|e| anyhow!("{e}"))?;
    }
    let r = e.key(last[0], Direction::Click).map_err(|e| anyhow!("{e}"));
    for m in mods.iter().rev() {
        let _ = e.key(*m, Direction::Release);
    }
    r
}

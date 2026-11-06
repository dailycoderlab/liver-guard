//! Input injection through `SendInput`.
//!
//! Coordinates are physical pixels in virtual-screen space. Windows cannot inject
//! into a window that runs at a higher integrity level (UIPI): run Sky elevated
//! to drive an elevated game.

use std::thread::sleep;
use std::time::Duration;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK,
    MOUSEINPUT, VIRTUAL_KEY,
};

use crate::window::virtual_screen;

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("输入被系统拒绝：目标可能以更高权限（管理员）运行，请以管理员身份运行 Sky")]
    Rejected,
    #[error("未知按键：{0}")]
    UnknownKey(String),
}

pub type Result<T> = std::result::Result<T, InputError>;

/// Virtual-key codes for the keys a script realistically needs.
pub fn virtual_key(name: &str) -> Option<u16> {
    Some(match name {
        "return" | "enter" => 0x0D,
        "tab" => 0x09,
        "space" => 0x20,
        "delete" | "backspace" => 0x08,
        "escape" | "esc" => 0x1B,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "f1" => 0x70,
        "f2" => 0x71,
        "f3" => 0x72,
        "f4" => 0x73,
        "f5" => 0x74,
        "f6" => 0x75,
        "f7" => 0x76,
        "f8" => 0x77,
        "f9" => 0x78,
        "f10" => 0x79,
        "f11" => 0x7A,
        "f12" => 0x7B,
        _ => return None,
    })
}

fn send(inputs: &[INPUT]) -> Result<()> {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(InputError::Rejected)
    }
}

/// Normalised 0..=65535 position across the whole virtual desktop.
fn absolute(x: f64, y: f64) -> (i32, i32) {
    let screen = virtual_screen();
    let nx = (x - screen.x) * 65535.0 / (screen.w - 1.0).max(1.0);
    let ny = (y - screen.y) * 65535.0 / (screen.h - 1.0).max(1.0);
    (nx.round().clamp(0.0, 65535.0) as i32, ny.round().clamp(0.0, 65535.0) as i32)
}

fn mouse(x: f64, y: f64, flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    let (dx, dy) = absolute(x, y);
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

pub fn move_to(x: f64, y: f64) -> Result<()> {
    send(&[mouse(x, y, MOUSEEVENTF_MOVE)])
}

pub fn tap(x: f64, y: f64, duration_ms: u32) -> Result<()> {
    send(&[mouse(x, y, MOUSEEVENTF_MOVE)])?;
    send(&[mouse(x, y, MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTDOWN)])?;
    sleep(Duration::from_millis(duration_ms.max(1) as u64));
    send(&[mouse(x, y, MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTUP)])
}

pub fn swipe(x1: f64, y1: f64, x2: f64, y2: f64, duration_ms: u32) -> Result<()> {
    const STEPS: u32 = 12;
    send(&[mouse(x1, y1, MOUSEEVENTF_MOVE)])?;
    send(&[mouse(x1, y1, MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTDOWN)])?;
    let step_delay = ((duration_ms.max(50) as f64 / 1000.0) / STEPS as f64).max(0.004);
    for index in 1..=STEPS {
        let ratio = index as f64 / STEPS as f64;
        send(&[mouse(x1 + (x2 - x1) * ratio, y1 + (y2 - y1) * ratio, MOUSEEVENTF_MOVE)])?;
        sleep(Duration::from_secs_f64(step_delay));
    }
    send(&[mouse(x2, y2, MOUSEEVENTF_MOVE | MOUSEEVENTF_LEFTUP)])
}

fn key_event(vk: u16, scan: u16, flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

pub fn key(name: &str) -> Result<()> {
    let lowered = name.trim().to_lowercase();
    let Some(code) = virtual_key(&lowered) else {
        if lowered.chars().count() == 1 {
            return text(name.trim());
        }
        return Err(InputError::UnknownKey(name.to_string()));
    };
    send(&[key_event(code, 0, Default::default())])?;
    sleep(Duration::from_millis(20));
    send(&[key_event(code, 0, KEYEVENTF_KEYUP)])
}

/// Type one UTF-16 unit at a time: games commonly drop characters sent in bulk.
pub fn text(value: &str) -> Result<()> {
    for unit in value.encode_utf16() {
        send(&[key_event(0, unit, KEYEVENTF_UNICODE)])?;
        send(&[key_event(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)])?;
        sleep(Duration::from_millis(12));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_agree() {
        assert_eq!(virtual_key("enter"), virtual_key("return"));
        assert_eq!(virtual_key("esc"), virtual_key("escape"));
        assert_eq!(virtual_key("f12"), Some(0x7B));
        assert_eq!(virtual_key("nope"), None);
    }
}

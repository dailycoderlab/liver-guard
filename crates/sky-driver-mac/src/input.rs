//! Input injection through Quartz CGEvents.
//!
//! Coordinates are global logical points with the origin at the top-left of the
//! main display, the same space the window list and Accessibility report, so no
//! conversion is needed between them.

use std::thread::sleep;
use std::time::Duration;

use core_graphics::event::{CGEvent, CGEventTapLocation, CGEventType, CGMouseButton};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::geometry::CGPoint;

#[derive(Debug, thiserror::Error)]
pub enum InputError {
    #[error("无法创建输入事件：请检查辅助功能权限")]
    Event,
    #[error("未知按键：{0}")]
    UnknownKey(String),
}

pub type Result<T> = std::result::Result<T, InputError>;

/// macOS virtual key codes for the keys a script realistically needs.
pub fn keycode(name: &str) -> Option<u16> {
    Some(match name {
        "return" | "enter" => 36,
        "tab" => 48,
        "space" => 49,
        "delete" | "backspace" => 51,
        "escape" | "esc" => 53,
        "left" => 123,
        "right" => 124,
        "down" => 125,
        "up" => 126,
        "home" => 115,
        "end" => 119,
        "pageup" => 116,
        "pagedown" => 121,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        _ => return None,
    })
}

fn source() -> Result<CGEventSource> {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| InputError::Event)
}

fn mouse(kind: CGEventType, x: f64, y: f64) -> Result<CGEvent> {
    CGEvent::new_mouse_event(source()?, kind, CGPoint::new(x, y), CGMouseButton::Left).map_err(|_| InputError::Event)
}

fn post_mouse(kind: CGEventType, x: f64, y: f64) -> Result<()> {
    mouse(kind, x, y)?.post(CGEventTapLocation::HID);
    Ok(())
}

pub fn move_to(x: f64, y: f64) -> Result<()> {
    post_mouse(CGEventType::MouseMoved, x, y)
}

pub fn tap(x: f64, y: f64, duration_ms: u32) -> Result<()> {
    move_to(x, y)?;
    post_mouse(CGEventType::LeftMouseDown, x, y)?;
    sleep(Duration::from_millis(duration_ms.max(1) as u64));
    post_mouse(CGEventType::LeftMouseUp, x, y)
}

pub fn swipe(x1: f64, y1: f64, x2: f64, y2: f64, duration_ms: u32) -> Result<()> {
    const STEPS: u32 = 12;
    move_to(x1, y1)?;
    post_mouse(CGEventType::LeftMouseDown, x1, y1)?;

    let step_delay = ((duration_ms.max(50) as f64 / 1000.0) / STEPS as f64).max(0.004);
    for index in 1..=STEPS {
        let ratio = index as f64 / STEPS as f64;
        post_mouse(CGEventType::LeftMouseDragged, x1 + (x2 - x1) * ratio, y1 + (y2 - y1) * ratio)?;
        sleep(Duration::from_secs_f64(step_delay));
    }
    post_mouse(CGEventType::LeftMouseUp, x2, y2)
}

pub fn key(name: &str) -> Result<()> {
    let lowered = name.trim().to_lowercase();
    let Some(code) = keycode(&lowered) else {
        // A single character is typed rather than looked up.
        if lowered.chars().count() == 1 {
            return text(name.trim());
        }
        return Err(InputError::UnknownKey(name.to_string()));
    };
    let down = CGEvent::new_keyboard_event(source()?, code, true).map_err(|_| InputError::Event)?;
    down.post(CGEventTapLocation::HID);
    sleep(Duration::from_millis(20));
    let up = CGEvent::new_keyboard_event(source()?, code, false).map_err(|_| InputError::Event)?;
    up.post(CGEventTapLocation::HID);
    Ok(())
}

/// Type one character at a time: sending a whole string in a single event is
/// faster, but games commonly drop characters without a delay between them.
pub fn text(value: &str) -> Result<()> {
    let mut buffer = [0u8; 4];
    for ch in value.chars() {
        let s: &str = ch.encode_utf8(&mut buffer);
        for pressed in [true, false] {
            let event = CGEvent::new_keyboard_event(source()?, 0, pressed).map_err(|_| InputError::Event)?;
            event.set_string(s);
            event.post(CGEventTapLocation::HID);
        }
        sleep(Duration::from_millis(12));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_keys_have_codes_and_aliases_agree() {
        assert_eq!(keycode("enter"), keycode("return"));
        assert_eq!(keycode("esc"), keycode("escape"));
        assert_eq!(keycode("f12"), Some(111));
        assert_eq!(keycode("nope"), None);
    }

    #[test]
    fn unknown_multi_character_keys_are_rejected_before_any_event() {
        assert!(matches!(key("ctrl+alt+del"), Err(InputError::UnknownKey(_))));
    }
}

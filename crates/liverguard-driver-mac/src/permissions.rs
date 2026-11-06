//! Permission checks.
//!
//! macOS does not let an app grant itself either permission, and it never
//! attributes it to a bare executable: TCC stores the *responsible* process,
//! which for `cargo run` or a binary started from a terminal is that terminal or
//! IDE. Only a packaged `Sky.app` gets its own entry in System Settings.
//! [`responsible_application`] reports who will actually show up there.

use std::process::Command;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use objc2_app_kit::NSRunningApplication;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionState {
    pub accessibility: bool,
    pub screen_recording: bool,
}

impl PermissionState {
    pub fn ok(&self) -> bool {
        self.accessibility && self.screen_recording
    }

    /// Human-readable fixes, naming the app macOS will list.
    pub fn problems(&self, owner: &str) -> Vec<String> {
        let mut issues = Vec::new();
        if !self.accessibility {
            issues.push(format!("辅助功能：系统设置 → 隐私与安全性 → 辅助功能，勾选 {owner}"));
        }
        if !self.screen_recording {
            issues.push(format!("屏幕录制：系统设置 → 隐私与安全性 → 屏幕录制，勾选 {owner}"));
            issues.push("勾选后要完全退出该程序再重新启动，权限才会生效。".to_string());
        }
        issues
    }
}

pub fn accessibility_trusted(prompt: bool) -> bool {
    if !prompt {
        return unsafe { AXIsProcessTrusted() };
    }
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) }
}

pub fn screen_recording_allowed(prompt: bool) -> bool {
    unsafe {
        if prompt {
            CGRequestScreenCaptureAccess()
        } else {
            CGPreflightScreenCaptureAccess()
        }
    }
}

pub fn current_state() -> PermissionState {
    PermissionState { accessibility: accessibility_trusted(false), screen_recording: screen_recording_allowed(false) }
}

/// Trigger the system prompts once; the user still has to flip the switches.
pub fn request_missing() -> PermissionState {
    if !accessibility_trusted(false) {
        accessibility_trusted(true);
    }
    if !screen_recording_allowed(false) {
        screen_recording_allowed(true);
    }
    current_state()
}

fn parent_pid(pid: i32) -> Option<i32> {
    let output = Command::new("ps").args(["-o", "ppid=", "-p", &pid.to_string()]).output().ok()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// `(name, bundle path)` of the outermost app bundle among this process's
/// ancestors. Nested helpers such as `Cursor Helper (Plugin).app` live inside
/// another app and are not what System Settings lists, so the outermost wins.
pub fn responsible_application() -> Option<(String, String)> {
    let mut found = None;
    let mut pid = std::process::id() as i32;
    for _ in 0..64 {
        if pid <= 1 {
            break;
        }
        if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
            if let Some(url) = app.bundleURL() {
                let name = app.localizedName().map(|n| n.to_string()).unwrap_or_default();
                let path = url.path().map(|p| p.to_string()).unwrap_or_default();
                found = Some((name, path));
            }
        }
        match parent_pid(pid) {
            Some(parent) => pid = parent,
            None => break,
        }
    }
    found
}

/// Who to tick in System Settings, as a single label.
pub fn owner_label() -> String {
    match responsible_application() {
        Some((name, path)) => format!("{name}（{path}）"),
        None => "运行本程序的终端或应用".to_string(),
    }
}

pub fn settings_url(kind: &str) -> Option<&'static str> {
    match kind {
        "accessibility" => Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"),
        "screen_recording" => Some("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"),
        _ => None,
    }
}

/// Open the System Settings pane for one permission.
pub fn open_settings(kind: &str) -> bool {
    let Some(url) = settings_url(kind) else { return false };
    Command::new("open").arg(url).status().map(|s| s.success()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_name_the_owner_and_only_missing_permissions() {
        let none = PermissionState { accessibility: false, screen_recording: false };
        assert_eq!(none.problems("Terminal").len(), 3);
        let only_screen = PermissionState { accessibility: true, screen_recording: false };
        assert!(only_screen.problems("Terminal")[0].contains("Terminal"));
        assert!(PermissionState { accessibility: true, screen_recording: true }.problems("x").is_empty());
    }

    #[test]
    fn settings_urls_exist_for_known_kinds_only() {
        assert!(settings_url("accessibility").is_some());
        assert!(settings_url("screen_recording").is_some());
        assert!(settings_url("camera").is_none());
    }
}

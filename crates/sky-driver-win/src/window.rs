//! Window discovery and activation.

use std::ffi::c_void;

use sky_ir::Rect;
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, HMONITOR, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetAncestor, GetWindowLongW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, GA_ROOT, GWL_EXSTYLE,
    SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_RESTORE, WS_EX_TOOLWINDOW,
    GetSystemMetrics, GetClassNameW,
};

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    /// The HWND value; stable for the lifetime of the window.
    pub number: isize,
    pub pid: u32,
    pub owner: String,
    pub title: String,
    pub bounds: Rect,
}

impl std::fmt::Display for WindowInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let title = if self.title.is_empty() { "(untitled)" } else { &self.title };
        write!(f, "#{} {} — {} [{:.0}x{:.0}]", self.number, self.owner, title, self.bounds.w, self.bounds.h)
    }
}

pub fn hwnd(number: isize) -> HWND {
    HWND(number as *mut c_void)
}

fn text(window: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(window);
        if len <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len as usize + 1];
        let copied = GetWindowTextW(window, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

fn class_name(window: HWND) -> String {
    unsafe {
        let mut buffer = [0u16; 256];
        let copied = GetClassNameW(window, &mut buffer);
        String::from_utf16_lossy(&buffer[..copied.max(0) as usize])
    }
}

fn process_name(pid: u32) -> String {
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buffer = [0u16; 512];
        let mut len = buffer.len() as u32;
        let result = QueryFullProcessImageNameW(process, PROCESS_NAME_FORMAT(0), windows::core::PWSTR(buffer.as_mut_ptr()), &mut len);
        let _ = windows::Win32::Foundation::CloseHandle(process);
        if result.is_err() {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..len as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or("").trim_end_matches(".exe").to_string()
    }
}

/// Visible frame of a window: excludes the invisible resize borders Windows 10+
/// adds, which is also what Graphics Capture returns.
pub fn frame_bounds(window: HWND) -> Option<Rect> {
    unsafe {
        let mut rect = RECT::default();
        let ok = DwmGetWindowAttribute(
            window,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut RECT as *mut c_void,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok();
        if !ok {
            GetWindowRect(window, &mut rect).ok()?;
        }
        Some(Rect::new(rect.left as f64, rect.top as f64, (rect.right - rect.left) as f64, (rect.bottom - rect.top) as f64))
    }
}

fn is_cloaked(window: HWND) -> bool {
    unsafe {
        let mut cloaked = 0u32;
        DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut c_void, 4).is_ok() && cloaked != 0
    }
}

fn describe(window: HWND) -> Option<WindowInfo> {
    unsafe {
        let bounds = frame_bounds(window)?;
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, Some(&mut pid));
        Some(WindowInfo { number: window.0 as isize, pid, owner: process_name(pid), title: text(window), bounds })
    }
}

/// Top-level windows in z-order (front to back). Skips invisible, cloaked
/// (other virtual desktops, suspended UWP), tool windows and anything under 40x40.
pub fn list_windows() -> Vec<WindowInfo> {
    unsafe extern "system" fn collect(window: HWND, lparam: LPARAM) -> BOOL {
        let out = &mut *(lparam.0 as *mut Vec<WindowInfo>);
        if !IsWindowVisible(window).as_bool() || is_cloaked(window) || GetAncestor(window, GA_ROOT) != window {
            return true.into();
        }
        if GetWindowLongW(window, GWL_EXSTYLE) as u32 & WS_EX_TOOLWINDOW.0 != 0 {
            return true.into();
        }
        // The shell's own windows are never a script target.
        if matches!(class_name(window).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd") {
            return true.into();
        }
        if let Some(info) = describe(window) {
            if info.bounds.w >= 40.0 && info.bounds.h >= 40.0 && !(info.title.is_empty() && info.owner.is_empty()) {
                out.push(info);
            }
        }
        true.into()
    }

    let mut windows: Vec<WindowInfo> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut windows as *mut Vec<WindowInfo> as isize));
    }
    windows
}

/// Case-insensitive substring of the process name first, then the title.
pub fn find_window(query: &str) -> Option<WindowInfo> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let windows = list_windows();
    windows
        .iter()
        .find(|w| w.owner.to_lowercase().contains(&needle))
        .or_else(|| windows.iter().find(|w| w.title.to_lowercase().contains(&needle)))
        .cloned()
}

pub fn window_by_number(number: isize) -> Option<WindowInfo> {
    list_windows().into_iter().find(|w| w.number == number)
}

/// Windows stacked above `target` that cover part of it. Window-only capture is
/// unaffected, but a click lands on whatever is on top, so input needs this.
pub fn occluding_windows(target: &WindowInfo, min_overlap: f64) -> Vec<WindowInfo> {
    let target_area = (target.bounds.w * target.bounds.h).max(1.0);
    let mut above = Vec::new();
    for window in list_windows() {
        if window.number == target.number {
            break;
        }
        let overlap = window.bounds.intersection(&target.bounds);
        if overlap.w * overlap.h / target_area >= min_overlap {
            above.push(window);
        }
    }
    above
}

/// Bring the window forward so injected input reaches it.
pub fn activate(window: &WindowInfo) -> bool {
    unsafe {
        let handle = hwnd(window.number);
        if IsIconic(handle).as_bool() {
            let _ = ShowWindow(handle, SW_RESTORE);
        }
        SetForegroundWindow(handle).as_bool()
    }
}

/// The primary monitor; its top-left corner is always the virtual-screen origin.
pub fn primary_monitor() -> HMONITOR {
    unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) }
}

/// The whole virtual desktop (all monitors) in physical pixels.
pub fn virtual_screen() -> Rect {
    unsafe {
        Rect::new(
            GetSystemMetrics(SM_XVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_YVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_CXVIRTUALSCREEN) as f64,
            GetSystemMetrics(SM_CYVIRTUALSCREEN) as f64,
        )
    }
}

/// The primary monitor's rectangle in physical pixels.
pub fn primary_display_bounds() -> Rect {
    use windows::Win32::UI::WindowsAndMessaging::{SM_CXSCREEN, SM_CYSCREEN};
    unsafe { Rect::new(0.0, 0.0, GetSystemMetrics(SM_CXSCREEN) as f64, GetSystemMetrics(SM_CYSCREEN) as f64) }
}

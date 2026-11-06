//! Window discovery and activation.

use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::display::CGDisplay;
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
};
use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
use sky_ir::Rect;

#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub number: u32,
    pub pid: i32,
    pub owner: String,
    pub title: String,
    pub bounds: Rect,
    pub layer: i32,
}

impl std::fmt::Display for WindowInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let title = if self.title.is_empty() { "(untitled)" } else { &self.title };
        write!(f, "#{} {} — {} [{:.0}x{:.0}]", self.number, self.owner, title, self.bounds.w, self.bounds.h)
    }
}

type Info = CFDictionary<CFString, CFType>;

fn get(info: &Info, key: &'static str) -> Option<CFType> {
    info.find(CFString::from_static_string(key)).map(|item| item.clone())
}

fn number(info: &Info, key: &'static str) -> Option<f64> {
    get(info, key)?.downcast::<CFNumber>()?.to_f64()
}

fn text(info: &Info, key: &'static str) -> String {
    get(info, key).and_then(|v| v.downcast::<CFString>()).map(|s| s.to_string()).unwrap_or_default()
}

fn parse(info: &Info) -> Option<WindowInfo> {
    let bounds = get(info, "kCGWindowBounds")?.downcast::<CFDictionary>()?;
    let bounds: CFDictionary<CFString, CFType> = unsafe { CFDictionary::wrap_under_get_rule(bounds.as_concrete_TypeRef()) };
    let rect = Rect::new(number(&bounds, "X")?, number(&bounds, "Y")?, number(&bounds, "Width")?, number(&bounds, "Height")?);
    Some(WindowInfo {
        number: number(info, "kCGWindowNumber")? as u32,
        pid: number(info, "kCGWindowOwnerPID")? as i32,
        owner: text(info, "kCGWindowOwnerName"),
        title: text(info, "kCGWindowName"),
        bounds: rect,
        layer: number(info, "kCGWindowLayer").unwrap_or(0.0) as i32,
    })
}

/// Windows front to back. By default only ordinary windows (layer 0) that are at
/// least 40x40 points, which skips menu bar items, overlays and tiny helpers.
pub fn list_windows(on_screen_only: bool, include_all_layers: bool) -> Vec<WindowInfo> {
    let mut options = kCGWindowListExcludeDesktopElements;
    if on_screen_only {
        options |= kCGWindowListOptionOnScreenOnly;
    }
    let Some(array) = copy_window_info(options, kCGNullWindowID) else { return vec![] };

    let mut windows = Vec::new();
    for item in array.iter() {
        let info: Info = unsafe { CFDictionary::wrap_under_get_rule(*item as CFDictionaryRef) };
        let Some(window) = parse(&info) else { continue };
        if !include_all_layers && window.layer != 0 {
            continue;
        }
        if window.bounds.w < 40.0 || window.bounds.h < 40.0 {
            continue;
        }
        windows.push(window);
    }
    windows
}

/// Case-insensitive substring of the owner name first, then the window title.
pub fn find_window(query: &str) -> Option<WindowInfo> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return None;
    }
    let candidates = list_windows(true, false);
    candidates
        .iter()
        .find(|w| w.owner.to_lowercase().contains(&needle))
        .or_else(|| candidates.iter().find(|w| w.title.to_lowercase().contains(&needle)))
        .cloned()
}

pub fn window_by_number(number: u32) -> Option<WindowInfo> {
    list_windows(false, true).into_iter().find(|w| w.number == number)
}

/// Windows stacked above `target` that cover part of it. Window-only capture is
/// not affected, but a click lands on whatever is on top, so input needs this.
pub fn occluding_windows(target: &WindowInfo, min_overlap: f64) -> Vec<WindowInfo> {
    let target_area = (target.bounds.w * target.bounds.h).max(1.0);
    let mut above = Vec::new();
    for window in list_windows(true, false) {
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

/// Bring the window's app forward so injected events reach it.
pub fn activate(window: &WindowInfo) -> bool {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(window.pid) else {
        return false;
    };
    #[allow(deprecated)]
    app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps)
}

/// Main display in global logical points.
pub fn main_display_bounds() -> Rect {
    let bounds = CGDisplay::main().bounds();
    Rect::new(bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height)
}

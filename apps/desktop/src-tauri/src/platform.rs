//! The only place the desktop app knows which OS it runs on.

use serde::{Deserialize, Serialize};
use liverguard_engine::Driver;
use liverguard_ir::{Bounds, Rect};

/// What a script drives: the whole display or a single window.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Surface {
    FullScreen,
    Window { number: i64 },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRow {
    pub number: i64,
    pub owner: String,
    pub title: String,
    pub bounds: Rect,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionInfo {
    pub platform: &'static str,
    pub accessibility: bool,
    pub screen_recording: bool,
    pub owner: String,
    pub problems: Vec<String>,
    pub ok: bool,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use liverguard_driver_mac::{permissions, window, MacDriver, SurfaceSelector};

    pub fn make_driver(surface: Surface) -> Box<dyn Driver> {
        let selector = match surface {
            Surface::FullScreen => SurfaceSelector::FullScreen,
            Surface::Window { number } => SurfaceSelector::Window { number: u32::try_from(number).ok(), query: None },
        };
        Box::new(MacDriver::new(selector))
    }

    pub fn list_windows() -> Vec<WindowRow> {
        window::list_windows(true, false)
            .into_iter()
            .map(|w| WindowRow {
                number: w.number as i64,
                label: w.to_string(),
                owner: w.owner,
                title: w.title,
                bounds: Rect::new(w.bounds.x, w.bounds.y, w.bounds.w, w.bounds.h),
            })
            .collect()
    }

    fn info(state: permissions::PermissionState) -> PermissionInfo {
        let owner = permissions::owner_label();
        PermissionInfo {
            platform: "macos",
            accessibility: state.accessibility,
            screen_recording: state.screen_recording,
            problems: state.problems(&owner),
            ok: state.ok(),
            owner,
        }
    }

    pub fn permissions() -> PermissionInfo {
        info(permissions::current_state())
    }

    pub fn request_permissions() -> PermissionInfo {
        info(permissions::request_missing())
    }

    pub fn open_permission_settings(kind: &str) {
        permissions::open_settings(kind);
    }

    pub fn prepare_process() {}
}

#[cfg(windows)]
mod imp {
    use super::*;
    use liverguard_driver_win::{capture, window, SurfaceSelector, WinDriver};

    pub fn make_driver(surface: Surface) -> Box<dyn Driver> {
        let selector = match surface {
            Surface::FullScreen => SurfaceSelector::FullScreen,
            Surface::Window { number } => SurfaceSelector::Window { number: Some(number as isize), query: None },
        };
        Box::new(WinDriver::new(selector))
    }

    pub fn list_windows() -> Vec<WindowRow> {
        window::list_windows()
            .into_iter()
            .map(|w| WindowRow {
                number: w.number as i64,
                label: w.to_string(),
                owner: w.owner,
                title: w.title,
                bounds: Rect::new(w.bounds.x, w.bounds.y, w.bounds.w, w.bounds.h),
            })
            .collect()
    }

    pub fn permissions() -> PermissionInfo {
        let supported = capture::is_supported();
        PermissionInfo {
            platform: "windows",
            accessibility: true,
            screen_recording: supported,
            owner: "LiverGuard".to_string(),
            problems: if supported {
                vec![]
            } else {
                vec!["需要 Windows 10 1803 或更高版本才能使用 Windows Graphics Capture".to_string()]
            },
            ok: supported,
        }
    }

    pub fn request_permissions() -> PermissionInfo {
        permissions()
    }

    pub fn open_permission_settings(_kind: &str) {}

    /// Coordinates must be physical pixels, so opt out of DPI virtualization.
    pub fn prepare_process() {
        liverguard_driver_win::init_dpi_awareness();
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::*;

    pub fn make_driver(_surface: Surface) -> Box<dyn Driver> {
        unimplemented!("this platform is not supported yet (macOS and Windows only)")
    }

    pub fn list_windows() -> Vec<WindowRow> {
        vec![]
    }

    pub fn permissions() -> PermissionInfo {
        PermissionInfo {
            platform: "other",
            accessibility: false,
            screen_recording: false,
            owner: String::new(),
            problems: vec!["this platform is not supported yet (macOS and Windows only)".to_string()],
            ok: false,
        }
    }

    pub fn request_permissions() -> PermissionInfo {
        permissions()
    }

    pub fn open_permission_settings(_kind: &str) {}

    pub fn prepare_process() {}
}

pub use imp::{list_windows, make_driver, open_permission_settings, permissions, prepare_process, request_permissions};

/// Logical point of a pixel in a frame.
pub fn pixel_to_global(bounds: &Bounds, px: f64, py: f64) -> (f64, f64) {
    (bounds.x + px / bounds.scale, bounds.y + py / bounds.scale)
}

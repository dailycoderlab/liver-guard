//! The only place the CLI knows which OS it runs on.

use anyhow::Result;
use sky_engine::Driver;

pub struct WindowRow {
    pub label: String,
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use sky_driver_mac::{permissions, window, MacDriver, SurfaceSelector};

    fn selector(window: Option<&str>) -> SurfaceSelector {
        match window {
            None => SurfaceSelector::FullScreen,
            Some(query) => match query.trim_start_matches('#').parse::<u32>() {
                Ok(number) if query.starts_with('#') => SurfaceSelector::Window { number: Some(number), query: None },
                _ => SurfaceSelector::Window { number: None, query: Some(query.to_string()) },
            },
        }
    }

    pub fn make_driver(window: Option<&str>) -> Result<Box<dyn Driver>> {
        Ok(Box::new(MacDriver::new(selector(window))))
    }

    pub fn list_windows(all: bool) -> Vec<WindowRow> {
        window::list_windows(!all, all).into_iter().map(|w| WindowRow { label: w.to_string() }).collect()
    }

    /// Returns `false` when something blocks the run.
    pub fn doctor_report() -> bool {
        let state = permissions::current_state();
        println!("辅助功能 (Accessibility): {}", if state.accessibility { "已授权" } else { "未授权" });
        println!("屏幕录制 (Screen Recording): {}", if state.screen_recording { "已授权" } else { "未授权" });
        println!("权限归属：{}", permissions::owner_label());
        for problem in state.problems(&permissions::owner_label()) {
            println!("  - {problem}");
        }
        state.ok()
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use sky_driver_win::{capture, window, SurfaceSelector, WinDriver};

    fn selector(window: Option<&str>) -> SurfaceSelector {
        match window {
            None => SurfaceSelector::FullScreen,
            Some(query) => match query.strip_prefix('#').and_then(|n| n.parse::<isize>().ok()) {
                Some(number) => SurfaceSelector::Window { number: Some(number), query: None },
                None => SurfaceSelector::Window { number: None, query: Some(query.to_string()) },
            },
        }
    }

    pub fn make_driver(window: Option<&str>) -> Result<Box<dyn Driver>> {
        sky_driver_win::init_dpi_awareness();
        Ok(Box::new(WinDriver::new(selector(window))))
    }

    pub fn list_windows(_all: bool) -> Vec<WindowRow> {
        sky_driver_win::init_dpi_awareness();
        window::list_windows().into_iter().map(|w| WindowRow { label: w.to_string() }).collect()
    }

    pub fn doctor_report() -> bool {
        let supported = capture::is_supported();
        println!("Windows Graphics Capture: {}", if supported { "可用" } else { "不可用（需要 Windows 10 1803 或更高版本）" });
        println!("提示：若目标程序以管理员身份运行，需要同样以管理员身份运行 Sky，否则输入会被系统拒绝。");
        supported
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::*;
    use anyhow::anyhow;

    pub fn make_driver(_window: Option<&str>) -> Result<Box<dyn Driver>> {
        Err(anyhow!("this platform is not supported yet (macOS and Windows only)"))
    }

    pub fn list_windows(_all: bool) -> Vec<WindowRow> {
        vec![]
    }

    pub fn doctor_report() -> bool {
        println!("this platform is not supported yet (macOS and Windows only)");
        false
    }
}

pub use imp::{doctor_report, list_windows, make_driver};

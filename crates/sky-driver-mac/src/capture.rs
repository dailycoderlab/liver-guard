//! ScreenCaptureKit screenshots.
//!
//! A window is captured through a desktop-independent filter: only that window's
//! own pixels, whatever is stacked above it. The shareable-content lookup is the
//! slow part (tens of milliseconds), so display / window handles are cached and
//! only refreshed when a capture fails or the window is not in the cache.

use std::collections::HashMap;

use core_graphics::display::CGDisplay;
use screencapturekit::prelude::*;
use screencapturekit::screenshot_manager::{CGImageExt, SCScreenshotManager};
use screencapturekit::shareable_content::SCShareableContentInfo;

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("无法截图：{0}。请检查屏幕录制权限；授权后需要完全退出程序再重新启动")]
    Failed(String),
    #[error("找不到可截取的目标（窗口可能已关闭或在其他桌面空间）")]
    Missing,
}

pub type Result<T> = std::result::Result<T, CaptureError>;

fn failed(error: impl std::fmt::Display) -> CaptureError {
    CaptureError::Failed(error.to_string())
}

/// Tightly packed BGRA, device pixels.
#[derive(Debug, Clone)]
pub struct RawImage {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    MainDisplay,
    Window(u32),
}

/// ScreenCaptureKit objects are reference-counted Objective-C handles that
/// Apple documents as safe to use from any thread; the crate just does not mark
/// them. The capturer is only ever used by one thread at a time (`&mut self`).
struct Handles {
    display: Option<SCDisplay>,
    windows: HashMap<u32, SCWindow>,
}

unsafe impl Send for Handles {}

pub struct Capturer {
    handles: Handles,
}

impl Default for Capturer {
    fn default() -> Self {
        Self::new()
    }
}

impl Capturer {
    pub fn new() -> Self {
        Self { handles: Handles { display: None, windows: HashMap::new() } }
    }

    /// Drop cached handles, e.g. after a display configuration change.
    pub fn invalidate(&mut self) {
        self.handles.display = None;
        self.handles.windows.clear();
    }

    pub fn grab(&mut self, target: Target) -> Result<RawImage> {
        match self.grab_cached(target) {
            Ok(image) => Ok(image),
            Err(_) => {
                // Stale handle (display reconfigured, window recreated): look up again once.
                self.invalidate();
                self.grab_cached(target)
            }
        }
    }

    fn grab_cached(&mut self, target: Target) -> Result<RawImage> {
        let filter = match target {
            Target::MainDisplay => {
                let display = self.display()?;
                SCContentFilter::create().with_display(&display).with_excluding_windows(&[]).build().map_err(failed)?
            }
            Target::Window(id) => {
                let window = self.window(id)?;
                SCContentFilter::create().with_window(&window).build().map_err(failed)?
            }
        };

        let info = SCShareableContentInfo::for_filter(&filter).ok_or(CaptureError::Missing)?;
        let (width, height) = info.pixel_size();
        if width == 0 || height == 0 {
            return Err(CaptureError::Missing);
        }

        let mut config = SCStreamConfiguration::new().with_width(width).with_height(height).with_shows_cursor(false);
        if matches!(target, Target::Window(_)) {
            // Without this the image grows by the window's drop shadow and no
            // longer matches the window frame the coordinates are based on.
            config = config.with_ignores_shadows_single_window(true).map_err(failed)?;
        }

        let image = SCScreenshotManager::capture_image(&filter, &config).map_err(failed)?;
        let bgra = image.bgra_data().map_err(failed)?;
        let (w, h) = (image.width() as u32, image.height() as u32);
        if w == 0 || h == 0 || bgra.len() < (w as usize) * (h as usize) * 4 {
            return Err(CaptureError::Failed("截图数据不完整".into()));
        }
        Ok(RawImage { width: w, height: h, bgra })
    }

    fn display(&mut self) -> Result<SCDisplay> {
        if let Some(display) = &self.handles.display {
            return Ok(display.clone());
        }
        let content = SCShareableContent::get().map_err(failed)?;
        let main = CGDisplay::main().id;
        let displays = content.displays();
        let display = displays
            .iter()
            .find(|d| d.display_id() == main)
            .or_else(|| displays.first())
            .cloned()
            .ok_or(CaptureError::Missing)?;
        self.handles.display = Some(display.clone());
        Ok(display)
    }

    fn window(&mut self, id: u32) -> Result<SCWindow> {
        if let Some(window) = self.handles.windows.get(&id) {
            return Ok(window.clone());
        }
        // Off-screen windows (other Spaces, minimized) cannot be captured.
        let content = SCShareableContent::create().with_on_screen_windows_only(true).get().map_err(failed)?;
        let window = content.windows().into_iter().find(|w| w.window_id() == id).ok_or(CaptureError::Missing)?;
        self.handles.windows.insert(id, window.clone());
        Ok(window)
    }
}

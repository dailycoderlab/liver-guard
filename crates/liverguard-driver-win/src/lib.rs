//! Windows platform layer.
//!
//! * [`window`]: top-level window discovery, activation, monitor lookup
//! * [`capture`]: Windows.Graphics.Capture (a window is captured on its own, so
//!   windows stacked on top of it never end up in the frame)
//! * [`input`]: SendInput mouse and keyboard injection
//! * [`uia`]: UI Automation element lookup
//!
//! Coordinates are physical pixels in virtual-screen space and `scale` is the
//! measured pixels-per-point of the captured image (1.0 unless the window's
//! frame and capture size disagree). The process must be Per-Monitor-V2 DPI
//! aware, otherwise Windows hands out virtualised coordinates; call
//! [`init_dpi_awareness`] first in a bare executable (Tauri's manifest already
//! does this for the desktop app).
//!
//! With the `driver` feature (default) [`WinDriver`] adapts all of it to the
//! engine's `Driver` trait.

#![cfg(windows)]

pub mod capture;
pub mod input;
pub mod uia;
pub mod window;

#[cfg(feature = "driver")]
mod driver;

#[cfg(feature = "driver")]
pub use driver::{SurfaceSelector, WinDriver};

pub use capture::{CaptureError, RawImage};
pub use window::WindowInfo;

/// Opt this process into Per-Monitor-V2 DPI awareness. Idempotent; failures
/// (already set by the manifest) are fine.
pub fn init_dpi_awareness() {
    use windows::Win32::UI::HiDpi::{
        SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    };
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

//! macOS platform layer.
//!
//! * [`window`]: window discovery through CoreGraphics' window list
//! * [`capture`]: ScreenCaptureKit screenshots (a single window is captured on
//!   its own, so windows stacked on top of it never end up in the frame)
//! * [`input`]: CGEvent mouse and keyboard injection
//! * [`ax`]: Accessibility element lookup
//! * [`permissions`]: Accessibility / Screen Recording state and guidance
//!
//! With the `driver` feature (default) [`MacDriver`] adapts all of it to the
//! engine's `Driver` trait.

#![cfg(target_os = "macos")]

pub mod ax;
pub mod capture;
pub mod input;
pub mod permissions;
pub mod window;

#[cfg(feature = "driver")]
mod driver;

#[cfg(feature = "driver")]
pub use driver::{MacDriver, Surface, SurfaceSelector};

pub use capture::{CaptureError, RawImage};
pub use window::WindowInfo;

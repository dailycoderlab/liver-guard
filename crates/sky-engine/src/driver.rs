//! The only surface a platform driver has to implement.
//!
//! Six primitives plus an optional native UI lookup. Matching, waiting,
//! branching, retry and reporting live in the engine and are shared by every
//! platform.

use sky_ir::{Bounds, Rect};
use sky_vision::Frame;

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// The surface changed while a frame was being captured; the engine retries.
    #[error("capture surface changed")]
    SurfaceChanged,
    #[error("{0}")]
    Failed(String),
}

impl DriverError {
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed(message.into())
    }
}

pub type DriverResult<T> = Result<T, DriverError>;

pub trait Driver: Send {
    /// Current capture surface, logical points in global coordinates.
    fn bounds(&mut self) -> DriverResult<Bounds>;

    /// Screenshot at device pixel resolution.
    fn capture(&mut self) -> DriverResult<Frame>;

    /// Click at a global logical point.
    fn tap(&mut self, x: f64, y: f64, duration_ms: u32) -> DriverResult<()>;

    /// Drag between two global logical points.
    fn swipe(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, duration_ms: u32) -> DriverResult<()>;

    /// Press a named key (`return`, `escape`, `space`, `tab`, ...).
    fn key(&mut self, name: &str) -> DriverResult<()>;

    /// Type a string into the focused control.
    fn text(&mut self, value: &str) -> DriverResult<()>;

    /// Native UI element under a point, when the platform offers one.
    fn ax_at(&mut self, _x: f64, _y: f64) -> DriverResult<Option<Rect>> {
        Ok(None)
    }
}

impl<D: Driver + ?Sized> Driver for Box<D> {
    fn bounds(&mut self) -> DriverResult<Bounds> {
        (**self).bounds()
    }
    fn capture(&mut self) -> DriverResult<Frame> {
        (**self).capture()
    }
    fn tap(&mut self, x: f64, y: f64, duration_ms: u32) -> DriverResult<()> {
        (**self).tap(x, y, duration_ms)
    }
    fn swipe(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, duration_ms: u32) -> DriverResult<()> {
        (**self).swipe(x1, y1, x2, y2, duration_ms)
    }
    fn key(&mut self, name: &str) -> DriverResult<()> {
        (**self).key(name)
    }
    fn text(&mut self, value: &str) -> DriverResult<()> {
        (**self).text(value)
    }
    fn ax_at(&mut self, x: f64, y: f64) -> DriverResult<Option<Rect>> {
        (**self).ax_at(x, y)
    }
}

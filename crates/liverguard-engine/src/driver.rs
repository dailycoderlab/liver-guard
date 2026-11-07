//! The only surface a platform driver has to implement.
//!
//! A handful of primitives (`tap` / `swipe` for exact input, `pointer` for paced
//! human-like input) plus optional lookups. Matching, waiting,
//! branching, retry and reporting live in the engine and are shared by every
//! platform.

use liverguard_ir::{Bounds, Rect};
use liverguard_vision::Frame;

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

/// One raw pointer event for human-like input, which the engine paces itself.
/// `Move` is a hover, `Drag` is a move while the button is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerAction {
    Move,
    Down,
    Drag,
    Up,
}

pub trait Driver: Send {
    /// Current capture surface, logical points in global coordinates.
    fn bounds(&mut self) -> DriverResult<Bounds>;

    /// Screenshot at device pixel resolution.
    fn capture(&mut self) -> DriverResult<Frame>;

    /// Click at a global logical point.
    fn tap(&mut self, x: f64, y: f64, duration_ms: u32) -> DriverResult<()>;

    /// Drag between two global logical points.
    fn swipe(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, duration_ms: u32) -> DriverResult<()>;

    /// Inject one raw pointer event at a global logical point. The engine plans the
    /// path and timing (see `human`); the driver only sends the event, with no
    /// target checks of its own (`check_input` runs once before a whole plan).
    fn pointer(&mut self, action: PointerAction, x: f64, y: f64) -> DriverResult<()>;

    /// Where the pointer is now, so a human-like move can start from there.
    /// `None` when the platform cannot tell.
    fn pointer_position(&mut self) -> DriverResult<Option<(f64, f64)>> {
        Ok(None)
    }

    /// Verify, before a sequence of raw pointer events, that clicking and dragging
    /// at these points is allowed (right window, in front, not covered). Hover moves
    /// are not checked.
    fn check_input(&mut self, _points: &[(f64, f64)]) -> DriverResult<()> {
        Ok(())
    }

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
    fn pointer(&mut self, action: PointerAction, x: f64, y: f64) -> DriverResult<()> {
        (**self).pointer(action, x, y)
    }
    fn pointer_position(&mut self) -> DriverResult<Option<(f64, f64)>> {
        (**self).pointer_position()
    }
    fn check_input(&mut self, points: &[(f64, f64)]) -> DriverResult<()> {
        (**self).check_input(points)
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

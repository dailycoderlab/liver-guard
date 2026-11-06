//! Why a run (or a node) failed.

use std::fmt;

/// Stable reason codes, used for run aggregation and for choosing which
/// failure handle a node may route to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Stopped,
    Timeout,
    NotFound,
    BadStep,
    UnknownTarget,
    UnknownFlow,
    CallDepth,
    StepLimit,
    VisitLimit,
    Stagnation,
    CaptureStalled,
    Driver,
    Engine,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::Stopped => "stopped",
            Reason::Timeout => "timeout",
            Reason::NotFound => "not_found",
            Reason::BadStep => "bad_step",
            Reason::UnknownTarget => "unknown_target",
            Reason::UnknownFlow => "unknown_flow",
            Reason::CallDepth => "call_depth",
            Reason::StepLimit => "step_limit",
            Reason::VisitLimit => "visit_limit",
            Reason::Stagnation => "stagnation",
            Reason::CaptureStalled => "capture_stalled",
            Reason::Driver => "driver_error",
            Reason::Engine => "engine_error",
        }
    }

    /// Only ordinary "did not find it in time" failures can be routed to a
    /// `fail` / `timeout` handle. Stops and infrastructure faults always abort.
    pub fn is_routable(self) -> bool {
        matches!(self, Reason::NotFound | Reason::Timeout)
    }
}

#[derive(Debug, Clone)]
pub struct RunError {
    pub reason: Reason,
    pub message: String,
    pub node_id: Option<String>,
}

impl RunError {
    pub fn new(reason: Reason, message: impl Into<String>) -> Self {
        Self { reason, message: message.into(), node_id: None }
    }

    pub fn at(reason: Reason, message: impl Into<String>, node_id: &str) -> Self {
        Self { reason, message: message.into(), node_id: Some(node_id.to_string()) }
    }

    pub fn stopped() -> Self {
        Self::new(Reason::Stopped, "stop requested")
    }

    pub fn with_node(mut self, node_id: &str) -> Self {
        if self.node_id.is_none() {
            self.node_id = Some(node_id.to_string());
        }
        self
    }
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for RunError {}

impl From<liverguard_vision::VisionError> for RunError {
    fn from(value: liverguard_vision::VisionError) -> Self {
        RunError::new(Reason::Engine, value.to_string())
    }
}

impl From<crate::driver::DriverError> for RunError {
    fn from(value: crate::driver::DriverError) -> Self {
        RunError::new(Reason::Driver, value.to_string())
    }
}

pub type RunResultOf<T> = Result<T, RunError>;

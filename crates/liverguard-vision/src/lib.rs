//! Image matching for LiverGuard.
//!
//! Everything here works on OpenCV `Mat`s (BGR, 8 bit). The matcher is a direct
//! port of the reference implementation the golden fixtures were produced from,
//! including its rounding rules, so scores stay comparable.

pub mod assets;
pub mod author;
pub mod detect;
pub mod engine;
pub mod frame;
pub mod keypoint;
pub mod normalize;
pub mod tpl;
pub mod track;
pub mod util;
pub mod verify;

pub use assets::AssetStore;
pub use engine::{Hit, MatcherEngine, Resolver};
pub use frame::Frame;
pub use util::{Result, VisionError};

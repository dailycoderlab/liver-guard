//! LiverGuard IR v2: the script graph, match targets and coordinate maths.
//!
//! No OpenCV and no platform code here, so the editor, the engine and every
//! driver can depend on it.

pub mod doc;
pub mod geometry;
pub mod node;
pub mod target;

pub use doc::{Flow, IrDoc, Meta, Problem, Severity, IR_VERSION};
pub use geometry::{Bounds, Rect};
pub use node::{
    handle, CompareOp, Condition, Edge, EmptyData, IfData, KeyData, LogData, LongPressData, LoopData,
    LoopMode, Node, NodeKind, Pos, RetryData, SetVarData, SleepData, SnapshotData, SwipeData, SwitchData, TapData,
    TextData, VarValue, WaitForData, WaitGoneData, WaitStableData, DEFAULT_SPREAD, MAX_SPREAD, MIN_SPREAD,
};
pub use target::{AssetBox, Mask, Multi, PointTarget, ScaleMode, ScaleSpec, Strategy, Target, TemplateTarget, Verify};

/// JSON Schema of [`IrDoc`], used to generate the editor's TypeScript types.
pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(IrDoc)
}

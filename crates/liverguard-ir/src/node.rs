//! Graph nodes. The wire shape is what React Flow stores:
//! `{ id, type, pos, data }` plus optional editor metadata.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct Pos {
    pub x: f64,
    pub y: f64,
}

/// A script variable value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum VarValue {
    Bool(bool),
    Num(f64),
    Str(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum CompareOp {
    #[default]
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Contains,
    Match,
}

/// Branch condition. `visible` / `missing` look at the screen; the rest combine
/// them or read variables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Condition {
    /// True when any listed target is on screen.
    Visible { targets: Vec<String> },
    /// True when the target is not on screen.
    Missing { target: String },
    Var {
        name: String,
        #[serde(default)]
        op: CompareOp,
        value: VarValue,
    },
    Not { cond: Box<Condition> },
    All { conds: Vec<Condition> },
    Any { conds: Vec<Condition> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TapData {
    pub target: String,
    #[serde(default = "tap_duration")]
    pub duration_ms: u32,
    /// Act like a human hand instead of an exact injection: a point scattered around
    /// the anchor inside the matched box, a curved mouse trip to it, a hesitation and
    /// a jittered press length. Scripts saved with the old `randomPoint` flag read it
    /// as this one.
    #[serde(default, alias = "randomPoint")]
    pub human: bool,
    /// How widely a human landing point scatters, as a multiple of the standard
    /// spread (1.0, `DEFAULT_SPREAD`): below 1 clusters tighter around the anchor,
    /// above 1 wanders further across the box. Only used when `human` is on, and
    /// kept within `MIN_SPREAD..=MAX_SPREAD`.
    #[serde(default = "default_spread")]
    pub spread: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LongPressData {
    pub target: String,
    #[serde(default = "long_press_duration")]
    pub duration_ms: u32,
    /// Act like a human hand instead of an exact injection: a point scattered around
    /// the anchor inside the matched box, a curved mouse trip to it, a hesitation and
    /// a jittered press length. Scripts saved with the old `randomPoint` flag read it
    /// as this one.
    #[serde(default, alias = "randomPoint")]
    pub human: bool,
    /// How widely a human landing point scatters, as a multiple of the standard
    /// spread (1.0, `DEFAULT_SPREAD`): below 1 clusters tighter around the anchor,
    /// above 1 wanders further across the box. Only used when `human` is on, and
    /// kept within `MIN_SPREAD..=MAX_SPREAD`.
    #[serde(default = "default_spread")]
    pub spread: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SwipeData {
    pub from: String,
    #[serde(default)]
    pub to: Option<String>,
    /// Relative drag `[dx, dy]` in fractions of the surface when `to` is absent.
    #[serde(default)]
    pub vector: Option<[f64; 2]>,
    #[serde(default = "swipe_duration")]
    pub duration_ms: u32,
    /// Act like a human hand: start and end scattered inside their boxes (or the
    /// vector's end point jittered), a curved mouse trip to the start, and a curved
    /// stroke over a jittered duration.
    #[serde(default)]
    pub human: bool,
    /// How widely a human landing points (and a vector end's jitter) scatter, as a multiple of the standard
    /// spread (1.0, `DEFAULT_SPREAD`): below 1 clusters tighter around the anchor,
    /// above 1 wanders further across the box. Only used when `human` is on, and
    /// kept within `MIN_SPREAD..=MAX_SPREAD`.
    #[serde(default = "default_spread")]
    pub spread: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KeyData {
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TextData {
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogData {
    #[serde(default = "info")]
    pub level: String,
    #[serde(default)]
    pub msg: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotData {
    #[serde(default = "snap")]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SleepData {
    /// Shortest wait, milliseconds. The run waits a random whole number of
    /// milliseconds in `min_ms..=max_ms`; equal bounds make a fixed wait.
    #[serde(default = "one_second")]
    pub min_ms: u64,
    /// Longest wait, milliseconds. Must not be below `min_ms`.
    #[serde(default = "one_second")]
    pub max_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitForData {
    /// Proceeds as soon as any of these shows up.
    pub any: Vec<String>,
    #[serde(default = "ten_seconds")]
    pub timeout_ms: u64,
    #[serde(default = "poll_interval")]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitGoneData {
    pub target: String,
    #[serde(default = "ten_seconds")]
    pub timeout_ms: u64,
    #[serde(default = "poll_interval")]
    pub interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WaitStableData {
    /// How long the screen must stay unchanged.
    #[serde(default = "stable_hold")]
    pub ms: u64,
    #[serde(default = "five_seconds")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IfData {
    pub when: Condition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SwitchData {
    /// Targets checked in order; the first visible one takes its `case:<target>` handle.
    pub cases: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum LoopMode {
    #[default]
    Count,
    While,
    Forever,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LoopData {
    #[serde(default)]
    pub mode: LoopMode,
    #[serde(default = "one_u32")]
    pub count: u32,
    /// Required for `while`.
    #[serde(default)]
    pub when: Option<Condition>,
    /// Safety cap for `while` / `forever`.
    #[serde(default)]
    pub max_iterations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetryData {
    #[serde(default = "three")]
    pub times: u32,
    #[serde(default = "default_backoff")]
    pub backoff_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SetVarData {
    pub name: String,
    pub value: VarValue,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
pub struct EmptyData {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", content = "data", rename_all = "camelCase")]
pub enum NodeKind {
    Start(EmptyData),
    End(EmptyData),
    Tap(TapData),
    LongPress(LongPressData),
    Swipe(SwipeData),
    Key(KeyData),
    Text(TextData),
    Log(LogData),
    Snapshot(SnapshotData),
    Sleep(SleepData),
    WaitFor(WaitForData),
    WaitGone(WaitGoneData),
    WaitStable(WaitStableData),
    If(IfData),
    Switch(SwitchData),
    Loop(LoopData),
    Retry(RetryData),
    SetVar(SetVarData),
}

/// Handle names. Kept as constants so the engine, validator and editor agree.
pub mod handle {
    pub const NEXT: &str = "next";
    pub const FAIL: &str = "fail";
    pub const FOUND: &str = "found";
    pub const TIMEOUT: &str = "timeout";
    pub const TRUE: &str = "true";
    pub const FALSE: &str = "false";
    pub const DEFAULT: &str = "default";
    pub const BODY: &str = "body";
    pub const DONE: &str = "done";
    pub const FAILED: &str = "failed";
    pub const CASE_PREFIX: &str = "case:";
}

impl NodeKind {
    /// Stable wire name, equal to the serialized `type`.
    pub fn type_name(&self) -> &'static str {
        match self {
            NodeKind::Start(_) => "start",
            NodeKind::End(_) => "end",
            NodeKind::Tap(_) => "tap",
            NodeKind::LongPress(_) => "longPress",
            NodeKind::Swipe(_) => "swipe",
            NodeKind::Key(_) => "key",
            NodeKind::Text(_) => "text",
            NodeKind::Log(_) => "log",
            NodeKind::Snapshot(_) => "snapshot",
            NodeKind::Sleep(_) => "sleep",
            NodeKind::WaitFor(_) => "waitFor",
            NodeKind::WaitGone(_) => "waitGone",
            NodeKind::WaitStable(_) => "waitStable",
            NodeKind::If(_) => "if",
            NodeKind::Switch(_) => "switch",
            NodeKind::Loop(_) => "loop",
            NodeKind::Retry(_) => "retry",
            NodeKind::SetVar(_) => "setVar",
        }
    }

    /// Output handles this node exposes, in display order.
    pub fn output_handles(&self) -> Vec<String> {
        use handle::*;
        let list: Vec<&str> = match self {
            NodeKind::End(_) => vec![],
            NodeKind::Tap(_) | NodeKind::LongPress(_) | NodeKind::Swipe(_) => vec![NEXT, FAIL],
            NodeKind::WaitFor(_) => vec![FOUND, TIMEOUT],
            NodeKind::WaitGone(_) | NodeKind::WaitStable(_) => vec![NEXT, TIMEOUT],
            NodeKind::If(_) => vec![TRUE, FALSE],
            NodeKind::Loop(_) => vec![BODY, DONE],
            NodeKind::Retry(_) => vec![BODY, DONE, FAILED],
            NodeKind::Switch(data) => {
                let mut handles: Vec<String> = data.cases.iter().map(|c| format!("{CASE_PREFIX}{c}")).collect();
                handles.push(DEFAULT.to_string());
                return handles;
            }
            _ => vec![NEXT],
        };
        list.into_iter().map(str::to_string).collect()
    }

    /// Targets this node reads.
    pub fn target_refs(&self) -> Vec<&str> {
        match self {
            NodeKind::Tap(d) => vec![d.target.as_str()],
            NodeKind::LongPress(d) => vec![d.target.as_str()],
            NodeKind::Swipe(d) => {
                let mut refs = vec![d.from.as_str()];
                if let Some(to) = &d.to {
                    refs.push(to.as_str());
                }
                refs
            }
            NodeKind::WaitFor(d) => d.any.iter().map(String::as_str).collect(),
            NodeKind::WaitGone(d) => vec![d.target.as_str()],
            NodeKind::Switch(d) => d.cases.iter().map(String::as_str).collect(),
            NodeKind::If(d) => {
                let mut refs = Vec::new();
                d.when.collect_targets(&mut refs);
                refs
            }
            NodeKind::Loop(d) => {
                let mut refs = Vec::new();
                if let Some(when) = &d.when {
                    when.collect_targets(&mut refs);
                }
                refs
            }
            _ => vec![],
        }
    }
}

impl Condition {
    pub fn collect_targets<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Condition::Visible { targets } => out.extend(targets.iter().map(String::as_str)),
            Condition::Missing { target } => out.push(target.as_str()),
            Condition::Var { .. } => {}
            Condition::Not { cond } => cond.collect_targets(out),
            Condition::All { conds } | Condition::Any { conds } => {
                for cond in conds {
                    cond.collect_targets(out);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: String,
    /// Editor position; the engine ignores it.
    #[serde(default)]
    pub pos: Pos,
    /// Display name; only affects the editor and run logs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Optional cap on how often the node may run (guards cycles).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_visits: Option<u32>,
    #[serde(flatten)]
    pub kind: NodeKind,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub id: String,
    pub source: String,
    pub source_handle: String,
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_handle: Option<String>,
}

/// Standard spread of a human landing point: `sigma = box size / 4.133` (Fitts).
pub const DEFAULT_SPREAD: f64 = 1.0;
pub const MIN_SPREAD: f64 = 0.3;
pub const MAX_SPREAD: f64 = 2.0;

fn default_spread() -> f64 {
    DEFAULT_SPREAD
}
fn tap_duration() -> u32 {
    60
}
fn long_press_duration() -> u32 {
    600
}
fn swipe_duration() -> u32 {
    300
}
fn info() -> String {
    "info".to_string()
}
fn snap() -> String {
    "snap".to_string()
}
fn one_second() -> u64 {
    1_000
}
fn ten_seconds() -> u64 {
    10_000
}
fn five_seconds() -> u64 {
    5_000
}
fn poll_interval() -> u64 {
    500
}
fn stable_hold() -> u64 {
    400
}
fn one_u32() -> u32 {
    1
}
fn three() -> u32 {
    3
}
fn default_backoff() -> u64 {
    500
}

//! Match targets: what a node points at on screen.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AssetBox {
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum ScaleMode {
    #[default]
    Auto,
    Fixed,
}

/// Scale search ladder. The defaults cover "fixed size UI on a bigger screen"
/// (0.7x) as well as proportional layouts that come out slightly off (1.5x).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ScaleSpec {
    pub mode: ScaleMode,
    pub min: f64,
    pub max: f64,
    pub steps: u32,
}

impl Default for ScaleSpec {
    fn default() -> Self {
        Self { mode: ScaleMode::Auto, min: 0.7, max: 1.5, steps: 9 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Tpl,
    Mstpl,
    Sift,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Verify {
    None,
    #[default]
    Rgb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mask {
    #[default]
    None,
    Edges,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Multi {
    #[default]
    NearestLast,
    Highest,
    Center,
    Nth,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TemplateTarget {
    /// SHA-256 of the PNG asset in the local asset directory.
    pub asset: String,
    pub asset_box: AssetBox,
    /// Pixels per logical point of the recording surface.
    #[serde(default = "one")]
    pub record_scale: f64,
    /// Logical width of the surface the crop was taken from. This makes one crop
    /// work on a differently sized screen; 0 falls back to the runtime width.
    #[serde(default)]
    pub record_width: f64,
    /// Normalized search region `[x, y, w, h]`.
    pub roi: [f64; 4],
    #[serde(default = "center")]
    pub anchor: [f64; 2],
    #[serde(default = "default_strategy")]
    pub strategy: Vec<Strategy>,
    #[serde(default)]
    pub scale: ScaleSpec,
    #[serde(default = "default_threshold")]
    pub threshold: f64,
    #[serde(default = "default_warn_threshold")]
    pub warn_threshold: f64,
    #[serde(default)]
    pub verify: Verify,
    #[serde(default)]
    pub mask: Mask,
    #[serde(default)]
    pub multi: Multi,
    #[serde(default)]
    pub nth: Option<u32>,
    #[serde(default = "yes")]
    pub auto: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PointTarget {
    pub norm: [f64; 2],
    pub zone: [f64; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Target {
    Template(TemplateTarget),
    Point(PointTarget),
}

fn one() -> f64 {
    1.0
}

fn center() -> [f64; 2] {
    [0.5, 0.5]
}

fn default_strategy() -> Vec<Strategy> {
    vec![Strategy::Tpl, Strategy::Mstpl]
}

fn default_threshold() -> f64 {
    0.85
}

fn default_warn_threshold() -> f64 {
    0.7
}

fn yes() -> bool {
    true
}

impl TemplateTarget {
    /// A template target with the standard defaults, as produced by the recorder.
    pub fn new(asset: impl Into<String>, asset_box: AssetBox, roi: [f64; 4]) -> Self {
        Self {
            asset: asset.into(),
            asset_box,
            record_scale: 1.0,
            record_width: 0.0,
            roi,
            anchor: center(),
            strategy: default_strategy(),
            scale: ScaleSpec::default(),
            threshold: default_threshold(),
            warn_threshold: default_warn_threshold(),
            verify: Verify::Rgb,
            mask: Mask::None,
            multi: Multi::NearestLast,
            nth: None,
            auto: true,
        }
    }
}

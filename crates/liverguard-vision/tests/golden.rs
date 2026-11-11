//! Parity with the reference matcher.
//!
//! `tests/golden/cases.json` was produced by the original Python implementation
//! (OpenCV 5.0.0). The Rust matcher must reproduce its scores within 0.01 and
//! its click points within 1 logical point.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use liverguard_ir::geometry::{self, Bounds, Rect};
use liverguard_ir::{ScaleSpec, Target};
use liverguard_vision::normalize::template_factor;
use liverguard_vision::tpl::scale_ladder;
use liverguard_vision::verify::color_similarity;
use liverguard_vision::{AssetStore, Frame, MatcherEngine};
use opencv::core::{Mat, MatTraitConst, Vector};
use opencv::imgcodecs;
use serde::Deserialize;

const SCORE_TOLERANCE: f64 = 0.01;
const POINT_TOLERANCE: f64 = 1.0;

#[derive(Deserialize)]
struct Manifest {
    frames: HashMap<String, FrameSpec>,
    assets: HashMap<String, String>,
    cases: Vec<Case>,
    point_cases: Vec<PointCase>,
    units: Units,
}

#[derive(Deserialize)]
struct FrameSpec {
    file: String,
    bounds: Bounds,
    hash: String,
}

#[derive(Deserialize)]
struct Expected {
    #[serde(rename = "box")]
    rect: [f64; 4],
    point: [f64; 2],
    score: f64,
    strategy: String,
    warnings: Vec<String>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    frame: String,
    target: Target,
    offset: [f64; 2],
    expected: Option<Expected>,
}

#[derive(Deserialize)]
struct PointCase {
    name: String,
    frame: String,
    target: Target,
    expected: Option<Expected>,
}

#[derive(Deserialize)]
struct Units {
    similarity: Vec<Similarity>,
    scale_ladder: Vec<Ladder>,
    template_factor: Vec<Factor>,
    roi_to_pixels: Vec<RoiPixels>,
    infer_roi: Vec<InferRoi>,
    anchor_in_box: Vec<AnchorInBox>,
}

#[derive(Deserialize)]
struct Similarity {
    name: String,
    value: f64,
}

#[derive(Deserialize)]
struct Ladder {
    spec: ScaleSpec,
    values: Vec<f64>,
}

#[derive(Deserialize)]
struct Factor {
    args: [f64; 3],
    value: f64,
}

#[derive(Deserialize)]
struct RoiPixels {
    roi: [f64; 4],
    size: [i32; 2],
    value: [i32; 4],
}

#[derive(Deserialize)]
struct InferRoi {
    #[serde(rename = "box")]
    rect: [f64; 4],
    bounds: [f64; 4],
    value: [f64; 4],
}

#[derive(Deserialize)]
struct AnchorInBox {
    click: [f64; 2],
    #[serde(rename = "box")]
    rect: [f64; 4],
    value: [f64; 2],
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn manifest() -> Manifest {
    let text = fs::read_to_string(golden_dir().join("cases.json")).expect("cases.json");
    serde_json::from_str(&text).expect("manifest parses")
}

fn read_image(path: &Path) -> Mat {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let image =
        imgcodecs::imdecode(&Vector::<u8>::from_slice(&bytes), imgcodecs::IMREAD_COLOR).unwrap();
    assert!(!image.empty(), "{} did not decode", path.display());
    image
}

fn load_frame(manifest: &Manifest, name: &str) -> Frame {
    let spec = &manifest.frames[name];
    Frame::new(read_image(&golden_dir().join(&spec.file)), spec.bounds)
}

fn engine(manifest: &Manifest) -> MatcherEngine {
    let mut store = AssetStore::in_memory();
    for (sha, file) in &manifest.assets {
        let bytes = fs::read(golden_dir().join(file)).unwrap();
        store.put_bytes(sha, &bytes).unwrap();
    }
    MatcherEngine::without_reuse(store)
}

fn assert_close(label: &str, actual: f64, expected: f64, tolerance: f64) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{label}: got {actual}, expected {expected} (tolerance {tolerance})"
    );
}

#[test]
fn frame_hashes_match_the_reference() {
    let manifest = manifest();
    for (name, spec) in &manifest.frames {
        let frame = load_frame(&manifest, name);
        assert_eq!(frame.hash().unwrap(), spec.hash, "hash of {name}");
    }
}

#[test]
fn template_cases_match_the_reference() {
    let manifest = manifest();
    for case in &manifest.cases {
        let frame = load_frame(&manifest, &case.frame);
        let mut engine = engine(&manifest);
        let hit = engine
            .resolve(
                &case.target,
                "btn",
                &frame,
                (case.offset[0], case.offset[1]),
            )
            .unwrap();

        match (&case.expected, hit) {
            (None, None) => {}
            (Some(expected), Some(hit)) => {
                let name = &case.name;
                assert_close(
                    &format!("{name} score"),
                    hit.score,
                    expected.score,
                    SCORE_TOLERANCE,
                );
                assert_close(
                    &format!("{name} x"),
                    hit.point.0,
                    expected.point[0],
                    POINT_TOLERANCE,
                );
                assert_close(
                    &format!("{name} y"),
                    hit.point.1,
                    expected.point[1],
                    POINT_TOLERANCE,
                );
                assert_close(
                    &format!("{name} box.w"),
                    hit.rect.w,
                    expected.rect[2],
                    POINT_TOLERANCE,
                );
                assert_close(
                    &format!("{name} box.h"),
                    hit.rect.h,
                    expected.rect[3],
                    POINT_TOLERANCE,
                );
                assert_eq!(hit.strategy, expected.strategy, "{name} strategy");
                assert_eq!(hit.warnings, expected.warnings, "{name} warnings");
            }
            (expected, hit) => panic!(
                "{}: expected {:?}, got {:?}",
                case.name,
                expected.is_some(),
                hit.map(|h| h.score)
            ),
        }
    }
}

#[test]
fn point_targets_match_the_reference() {
    let manifest = manifest();
    for case in &manifest.point_cases {
        let frame = load_frame(&manifest, &case.frame);
        let mut engine = engine(&manifest);
        let hit = engine
            .resolve(&case.target, "p", &frame, (0.0, 0.0))
            .unwrap();
        match (&case.expected, hit) {
            (None, None) => {}
            (Some(expected), Some(hit)) => {
                assert_close(
                    &format!("{} x", case.name),
                    hit.point.0,
                    expected.point[0],
                    1e-6,
                );
                assert_close(
                    &format!("{} y", case.name),
                    hit.point.1,
                    expected.point[1],
                    1e-6,
                );
                assert_eq!(hit.strategy, expected.strategy);
                assert_eq!(hit.warnings, expected.warnings);
            }
            _ => panic!("{}: presence differs", case.name),
        }
    }
}

#[test]
fn color_similarity_matches_the_reference() {
    let manifest = manifest();
    for item in &manifest.units.similarity {
        let patch = read_image(&golden_dir().join(format!("patches/{}_patch.png", item.name)));
        let template =
            read_image(&golden_dir().join(format!("patches/{}_template.png", item.name)));
        assert_close(
            &format!("similarity {}", item.name),
            color_similarity(&patch, &template),
            item.value,
            1e-3,
        );
    }
}

#[test]
fn geometry_helpers_match_the_reference() {
    let manifest = manifest();

    for item in &manifest.units.scale_ladder {
        let values = scale_ladder(&item.spec);
        assert_eq!(
            values.len(),
            item.values.len(),
            "ladder length for {:?}",
            item.spec
        );
        for (a, b) in values.iter().zip(&item.values) {
            assert_close("ladder", *a, *b, 1e-9);
        }
    }

    for item in &manifest.units.template_factor {
        let value = template_factor(item.args[0], item.args[1], item.args[2], 480.0);
        assert_close("template_factor", value, item.value, 1e-9);
    }

    for item in &manifest.units.roi_to_pixels {
        let value = geometry::roi_to_pixels(&item.roi, item.size[0], item.size[1]);
        assert_eq!(
            [value.0, value.1, value.2, value.3],
            item.value,
            "roi {:?}",
            item.roi
        );
    }

    for item in &manifest.units.infer_roi {
        let rect = Rect::new(item.rect[0], item.rect[1], item.rect[2], item.rect[3]);
        let bounds = Bounds::new(
            item.bounds[0],
            item.bounds[1],
            item.bounds[2],
            item.bounds[3],
            2.0,
        );
        let value = geometry::infer_roi(&rect, &bounds, 3, 5);
        for (a, b) in value.iter().zip(&item.value) {
            assert_close("infer_roi", *a, *b, 1e-9);
        }
    }

    for item in &manifest.units.anchor_in_box {
        let rect = Rect::new(item.rect[0], item.rect[1], item.rect[2], item.rect[3]);
        let value = geometry::anchor_in_box((item.click[0], item.click[1]), &rect);
        assert_close("anchor x", value.0, item.value[0], 1e-9);
        assert_close("anchor y", value.1, item.value[1], 1e-9);
    }
}

//! Test support: synthetic game-like scenes, a recording driver and canned
//! resolvers. Public so the CLI and driver crates can reuse it in their tests.

use opencv::prelude::MatTrait;
use opencv::core::{Mat, MatTraitConst, Point, Rect as CvRect, Scalar, Vec3b, CV_8UC3};
use opencv::imgproc::{self, FONT_HERSHEY_SIMPLEX, LINE_8, LINE_AA};
use sky_ir::geometry::py_round;
use sky_ir::{AssetBox, Bounds, Rect, Target, TemplateTarget};
use sky_vision::{AssetStore, Frame, Hit, Resolver};

use crate::driver::{Driver, DriverResult};

pub const BUTTON_CENTER: (f64, f64) = (300.0, 220.0);
pub const BUTTON_SIZE: (f64, f64) = (120.0, 56.0);

fn draw_button(image: &mut Mat, x: i32, y: i32, w: i32, h: i32, label: &str) {
    let rect = CvRect::new(x, y, w, h);
    imgproc::rectangle(image, rect, Scalar::new(196.0, 120.0, 60.0, 0.0), -1, LINE_8, 0).unwrap();
    imgproc::rectangle(image, rect, Scalar::new(250.0, 240.0, 220.0, 0.0), 2.max(w / 40), LINE_8, 0).unwrap();
    let font_scale = 0.4f64.max(h as f64 / 90.0);
    let thickness = 1.max(py_round(h as f64 / 40.0) as i32);
    imgproc::put_text(
        image,
        label,
        Point::new(x + (w as f64 * 0.12) as i32, y + (h as f64 * 0.66) as i32),
        FONT_HERSHEY_SIMPLEX,
        font_scale,
        Scalar::new(255.0, 252.0, 245.0, 0.0),
        thickness,
        LINE_AA,
        false,
    )
    .unwrap();
}

/// A dark noisy background with one labelled button (and optionally an identical decoy).
pub fn make_scene(
    scale: f64,
    width_pt: f64,
    height_pt: f64,
    button_center_pt: (f64, f64),
    button_size_pt: (f64, f64),
    decoy_at_pt: Option<(f64, f64)>,
) -> Frame {
    let width = py_round(width_pt * scale) as i32;
    let height = py_round(height_pt * scale) as i32;
    let mut image = Mat::new_rows_cols_with_default(height, width, CV_8UC3, Scalar::all(0.0)).unwrap();

    // Deterministic noise (xorshift) so tests never flake.
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state % 11) as i32 - 5;
            let px = image.at_2d_mut::<Vec3b>(y, x).unwrap();
            *px = Vec3b::from([(48 + noise) as u8, (44 + noise) as u8, (40 + noise) as u8]);
        }
    }

    let bw = py_round(button_size_pt.0 * scale) as i32;
    let bh = py_round(button_size_pt.1 * scale) as i32;
    let place = |c: (f64, f64)| -> (i32, i32) {
        (py_round(c.0 * scale - bw as f64 / 2.0) as i32, py_round(c.1 * scale - bh as f64 / 2.0) as i32)
    };
    let (bx, by) = place(button_center_pt);
    draw_button(&mut image, bx, by, bw, bh, "START");
    if let Some(decoy) = decoy_at_pt {
        let (dx, dy) = place(decoy);
        draw_button(&mut image, dx, dy, bw, bh, "START");
    }

    Frame::new(image, Bounds::new(0.0, 0.0, width_pt, height_pt, scale))
}

/// The default 600x400pt scene with the button at `BUTTON_CENTER`.
pub fn default_scene(scale: f64) -> Frame {
    make_scene(scale, 600.0, 400.0, BUTTON_CENTER, BUTTON_SIZE, None)
}

/// PNG-encode the region around `center_pt`, as the recorder would store it.
pub fn crop_asset(image: &Mat, center_pt: (f64, f64), size_pt: (f64, f64), scale: f64) -> Vec<u8> {
    let x = py_round(center_pt.0 * scale - size_pt.0 * scale / 2.0) as i32;
    let y = py_round(center_pt.1 * scale - size_pt.1 * scale / 2.0) as i32;
    let w = py_round(size_pt.0 * scale) as i32;
    let h = py_round(size_pt.1 * scale) as i32;
    let patch = sky_vision::util::crop(image, x, y, w, h).unwrap();
    sky_vision::frame::encode(&patch, ".png", &opencv::core::Vector::new()).unwrap()
}

pub fn button_target(asset_bytes: &[u8], size_pt: (f64, f64), record_scale: f64, record_width: f64) -> TemplateTarget {
    let mut target = TemplateTarget::new(
        AssetStore::hash_bytes(asset_bytes),
        AssetBox { w: size_pt.0 * record_scale, h: size_pt.1 * record_scale },
        [0.0, 0.0, 1.0, 1.0],
    );
    target.record_scale = record_scale;
    target.record_width = record_width;
    target.threshold = 0.8;
    target.warn_threshold = 0.6;
    target.multi = sky_ir::Multi::Highest;
    target
}

/// A real matcher over the synthetic scene, with the button registered as target "btn".
pub fn scene_engine(record_scale: f64) -> (sky_vision::MatcherEngine, TemplateTarget, Frame) {
    let scene = make_scene(record_scale, 600.0, 400.0, BUTTON_CENTER, BUTTON_SIZE, None);
    let asset = crop_asset(&scene.image, BUTTON_CENTER, BUTTON_SIZE, record_scale);
    let target = button_target(&asset, BUTTON_SIZE, record_scale, 600.0);
    let mut store = AssetStore::in_memory();
    store.put_bytes(&target.asset, &asset).unwrap();
    (sky_vision::MatcherEngine::without_reuse(store), target, scene)
}

/// Records every injected input against a fixed screen.
pub struct FakeDriver {
    image: Mat,
    bounds: Bounds,
    pub taps: Vec<(f64, f64, u32)>,
    pub swipes: Vec<(f64, f64, f64, f64)>,
    pub keys: Vec<String>,
    pub texts: Vec<String>,
    pub captures: u32,
    /// Reported as the frame's capture time, to simulate a stalled grab.
    pub capture_ms: f64,
}

impl FakeDriver {
    pub fn new(frame: &Frame) -> Self {
        Self {
            image: frame.image.try_clone().unwrap(),
            bounds: frame.bounds,
            taps: vec![],
            swipes: vec![],
            keys: vec![],
            texts: vec![],
            captures: 0,
            capture_ms: 0.0,
        }
    }

    /// A black 800x600 screen: enough for tests that never match real pixels.
    pub fn blank() -> Self {
        let bounds = Bounds::new(0.0, 0.0, 800.0, 600.0, 1.0);
        let image = Mat::new_rows_cols_with_default(600, 800, CV_8UC3, Scalar::all(0.0)).unwrap();
        Self::new(&Frame::new(image, bounds))
    }
}

impl Driver for FakeDriver {
    fn bounds(&mut self) -> DriverResult<Bounds> {
        Ok(self.bounds)
    }

    fn capture(&mut self) -> DriverResult<Frame> {
        self.captures += 1;
        let mut frame = Frame::new(self.image.try_clone().unwrap(), self.bounds);
        frame.seq = self.captures as u64;
        frame.capture_ms = self.capture_ms;
        Ok(frame)
    }

    fn tap(&mut self, x: f64, y: f64, duration_ms: u32) -> DriverResult<()> {
        self.taps.push((x, y, duration_ms));
        Ok(())
    }

    fn swipe(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, _duration_ms: u32) -> DriverResult<()> {
        self.swipes.push((x1, y1, x2, y2));
        Ok(())
    }

    fn key(&mut self, name: &str) -> DriverResult<()> {
        self.keys.push(name.to_string());
        Ok(())
    }

    fn text(&mut self, value: &str) -> DriverResult<()> {
        self.texts.push(value.to_string());
        Ok(())
    }
}

/// Always returns the same hit (or none), regardless of the screen.
pub struct FixedResolver {
    pub hit: Option<Hit>,
    pub reused: bool,
}

impl FixedResolver {
    pub fn hit(rect: Rect, point: (f64, f64), score: f64) -> Self {
        Self {
            hit: Some(Hit {
                rect,
                point,
                score,
                strategy: "tpl".to_string(),
                elapsed_ms: 1.0,
                reused: false,
                warnings: vec![],
            }),
            reused: false,
        }
    }

    pub fn none() -> Self {
        Self { hit: None, reused: false }
    }
}

impl Resolver for FixedResolver {
    fn resolve(&mut self, _target: &Target, _id: &str, _frame: &Frame, offset: (f64, f64)) -> sky_vision::Result<Option<Hit>> {
        let Some(mut hit) = self.hit.clone() else { return Ok(None) };
        if self.reused {
            hit.reused = true;
            hit.strategy = "reuse".to_string();
            if offset != (0.0, 0.0) {
                hit.point = (hit.point.0 + offset.0 * hit.rect.w, hit.point.1 + offset.1 * hit.rect.h);
            }
        }
        Ok(Some(hit))
    }
}

// ---------------------------------------------------------------- graph tests

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use sky_ir::{Edge, Flow, IrDoc, Node, NodeKind};

/// Per-target visibility, shared with the test so it can flip things mid-run.
#[derive(Clone, Default)]
pub struct Screen {
    inner: Arc<Mutex<ScreenState>>,
}

#[derive(Default)]
struct ScreenState {
    /// name -> (rect, score)
    visible: HashMap<String, (Rect, f64)>,
    /// name -> number of resolves before it shows up
    appear_after: HashMap<String, u32>,
    calls: HashMap<String, u32>,
}

impl Screen {
    pub fn show(self, name: &str, score: f64) -> Self {
        self.inner.lock().unwrap().visible.insert(name.into(), (Rect { x: 100.0, y: 100.0, w: 40.0, h: 20.0 }, score));
        self
    }

    pub fn appear_after(self, name: &str, resolves: u32, score: f64) -> Self {
        let screen = self.show(name, score);
        screen.inner.lock().unwrap().appear_after.insert(name.into(), resolves);
        screen
    }

    pub fn hide(&self, name: &str) {
        self.inner.lock().unwrap().visible.remove(name);
    }

    pub fn calls(&self, name: &str) -> u32 {
        self.inner.lock().unwrap().calls.get(name).copied().unwrap_or(0)
    }

    pub fn resolver(&self) -> Box<dyn Resolver> {
        Box::new(ScreenResolver { screen: self.clone() })
    }
}

struct ScreenResolver {
    screen: Screen,
}

impl Resolver for ScreenResolver {
    fn resolve(&mut self, _target: &Target, id: &str, _frame: &Frame, _offset: (f64, f64)) -> sky_vision::Result<Option<Hit>> {
        let mut state = self.screen.inner.lock().unwrap();
        let calls = {
            let entry = state.calls.entry(id.to_string()).or_insert(0);
            *entry += 1;
            *entry
        };
        if state.appear_after.get(id).is_some_and(|after| calls <= *after) {
            return Ok(None);
        }
        Ok(state.visible.get(id).map(|(rect, score)| Hit {
            rect: *rect,
            point: (rect.x + rect.w / 2.0, rect.y + rect.h / 2.0),
            score: *score,
            strategy: "tpl".into(),
            elapsed_ms: 0.5,
            reused: false,
            warnings: vec![],
        }))
    }
}

/// A template with default thresholds (0.82 / 0.7). The `Screen` resolver never
/// reads its pixels; it only needs to exist so the graph validates.
pub fn stub_target() -> Target {
    Target::Template(TemplateTarget::new("0".repeat(64), AssetBox { w: 40.0, h: 20.0 }, [0.0, 0.0, 1.0, 1.0]))
}

/// Fluent construction of one-flow documents.
pub struct GraphBuilder {
    doc: IrDoc,
    flow: String,
}

impl GraphBuilder {
    /// A document whose `main` flow already holds `start`, with every name in
    /// `targets` registered as a stub target.
    pub fn new(targets: &[&str]) -> Self {
        let mut doc = IrDoc::empty("test");
        for name in targets {
            doc.targets.insert((*name).into(), stub_target());
        }
        Self { doc, flow: "main".into() }
    }

    /// Switch to (and create) another flow, with its own `start`.
    pub fn flow(mut self, name: &str) -> Self {
        let mut fresh = IrDoc::empty("x");
        let start = fresh.flows.remove("main").unwrap();
        self.doc.flows.entry(name.into()).or_insert(start);
        self.flow = name.into();
        self
    }

    pub fn var(mut self, name: &str, value: sky_ir::VarValue) -> Self {
        self.doc.vars.insert(name.into(), value);
        self
    }

    pub fn max_duration(mut self, ms: u64) -> Self {
        self.doc.meta.max_duration_ms = ms;
        self
    }

    fn current(&mut self) -> &mut Flow {
        self.doc.flows.get_mut(&self.flow).unwrap()
    }

    pub fn node(mut self, id: &str, kind: NodeKind) -> Self {
        self.current().nodes.push(Node { id: id.into(), pos: Default::default(), label: None, max_visits: None, kind });
        self
    }

    pub fn node_with_visits(mut self, id: &str, kind: NodeKind, max_visits: u32) -> Self {
        self.current().nodes.push(Node {
            id: id.into(),
            pos: Default::default(),
            label: None,
            max_visits: Some(max_visits),
            kind,
        });
        self
    }

    pub fn wire(mut self, source: &str, handle: &str, target: &str) -> Self {
        let flow = self.current();
        let id = format!("e{}", flow.edges.len());
        flow.edges.push(Edge {
            id,
            source: source.into(),
            source_handle: handle.into(),
            target: target.into(),
            target_handle: None,
        });
        self
    }

    pub fn build(self) -> IrDoc {
        let problems = self.doc.validate();
        let errors = IrDoc::errors(&problems);
        assert!(errors.is_empty(), "test graph is invalid: {errors:?}");
        self.doc
    }

    /// Skip validation, for tests that exercise the engine's own guards.
    pub fn build_unchecked(self) -> IrDoc {
        self.doc
    }
}
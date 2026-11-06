//! LiverGuard engine: the `Driver` abstraction and the graph interpreter.
//!
//! Everything platform independent lives here; a platform only supplies a
//! [`Driver`].

pub mod driver;
pub mod error;
pub mod interp;
pub mod report;
pub mod testing;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub use driver::{Driver, DriverError, DriverResult};
pub use error::{Reason, RunError};
pub use interp::{Interpreter, RunResult, RunState};
pub use report::{EventSink, LogLine, NodeState, Reporter, RunEvent};

use serde::Serialize;
use liverguard_ir::{IrDoc, Rect, Target};
use liverguard_vision::{AssetStore, MatcherEngine};

/// Check a document and make sure every asset it needs is on disk.
pub fn prepare(ir: &IrDoc, assets: &mut AssetStore) -> Result<(), String> {
    let problems = ir.validate();
    let errors = IrDoc::errors(&problems);
    if !errors.is_empty() {
        return Err(errors.iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; "));
    }
    for target in ir.targets.values() {
        if let Target::Template(template) = target {
            let sha = &template.asset;
            if sha.len() != 64 || !sha.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
                return Err("Invalid asset hash".to_string());
            }
            assets.get(sha).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Run a script to completion on the calling thread.
pub fn run_local(
    ir: &IrDoc,
    driver: &mut dyn Driver,
    mut assets: AssetStore,
    run_id: &str,
    stop: Arc<AtomicBool>,
    sink: Option<EventSink>,
) -> Result<(RunResult, Reporter), String> {
    prepare(ir, &mut assets)?;
    let mut reporter = Reporter::new(run_id);
    if let Some(sink) = sink {
        reporter.subscribe(sink);
    }
    let mut interpreter = Interpreter::new(ir, driver, Box::new(MatcherEngine::new(assets)), reporter, stop);
    let result = interpreter.run();
    Ok((result, interpreter.into_reporter()))
}

/// One target's result in a dry run.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRow {
    pub target: String,
    /// The matcher produced a candidate at all.
    pub found: bool,
    /// The candidate clears the target's threshold, i.e. a `tap` would fire.
    pub ok: bool,
    pub score: Option<f64>,
    pub threshold: Option<f64>,
    pub rect: Option<Rect>,
    pub point: Option<(f64, f64)>,
    pub strategy: Option<String>,
    pub elapsed_ms: f64,
}

/// Dry run: grab one frame and locate every target in it. Nothing is clicked.
pub fn probe(ir: &IrDoc, driver: &mut dyn Driver, mut assets: AssetStore) -> Result<Vec<ProbeRow>, String> {
    prepare(ir, &mut assets)?;
    let frame = driver.capture().map_err(|e| e.to_string())?;
    let mut engine = MatcherEngine::without_reuse(assets);
    let mut rows = Vec::new();
    for (name, target) in &ir.targets {
        let threshold = match target {
            Target::Template(t) => Some(t.threshold),
            Target::Point(_) => None,
        };
        let hit = engine.resolve(target, name, &frame, (0.0, 0.0)).map_err(|e| e.to_string())?;
        rows.push(match hit {
            Some(hit) => ProbeRow {
                target: name.clone(),
                found: true,
                ok: threshold.map_or(true, |t| hit.score >= t),
                score: Some(hit.score),
                threshold,
                rect: Some(hit.rect),
                point: Some(hit.point),
                strategy: Some(hit.strategy),
                elapsed_ms: hit.elapsed_ms,
            },
            None => ProbeRow {
                target: name.clone(),
                found: false,
                ok: false,
                score: None,
                threshold,
                rect: None,
                point: None,
                strategy: None,
                elapsed_ms: 0.0,
            },
        });
    }
    Ok(rows)
}

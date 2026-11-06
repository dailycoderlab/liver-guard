//! Graph interpreter.
//!
//! Synchronous on purpose: it runs on a worker thread, so capture, matching and
//! input injection never interleave with the UI thread.
//!
//! A flow is walked from its `start` node by following the wired output handle
//! each node picks. Containers (`loop`, `retry`) run the chain hanging off their
//! `body` handle to its end, then continue from `done`. Reaching an `end` node
//! ends the current flow; inside a called flow it only returns to the caller.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::Rng;
use serde_json::json;
use liverguard_ir::node::handle;
use liverguard_ir::{
    CompareOp, Condition, Flow, IrDoc, Jitter, LoopMode, Node, NodeKind, Rect, SwipeData, Target, VarValue,
};
use liverguard_vision::{Frame, Hit, Resolver};

use crate::driver::Driver;
use crate::error::{Reason, RunError, RunResultOf};
use crate::report::{now_ms, NodeState, Reporter};

pub const MAX_CALL_DEPTH: usize = 8;
pub const MAX_STEPS: u64 = 200_000;
pub const STAGNATION_LIMIT: u32 = 10;
/// macOS blocks a grab for ~30s when the process has lost screen recording
/// permission, then returns a blank frame. A stalled capture aborts the run
/// with a reason the operator can act on instead of polling through it.
pub const CAPTURE_STALL_MS: f64 = 3000.0;
const DEFAULT_LOOP_CAP: u32 = 100_000;
const POLL_SLICE_MS: f64 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Done,
    Failed,
    Stopped,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            RunState::Done => "done",
            RunState::Failed => "failed",
            RunState::Stopped => "stopped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RunResult {
    pub state: RunState,
    pub step_count: u64,
    pub error_code: Option<String>,
    pub error_msg: Option<String>,
    pub error_node: Option<String>,
    pub started_at: u64,
    pub ended_at: u64,
}

impl RunResult {
    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "state": self.state.as_str(),
            "stepCount": self.step_count,
            "errorCode": self.error_code,
            "errorMsg": self.error_msg,
            "errorNode": self.error_node,
            "startedAt": self.started_at,
            "endedAt": self.ended_at,
        })
    }
}

/// Where a chain stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChainEnd {
    /// Ran off the end of the wiring.
    Done,
    /// Hit an `end` node.
    Ended,
}

/// What a node asks the walker to do next.
enum Outcome {
    Follow(&'static str),
    FollowOwned(String),
    End,
}

pub struct Interpreter<'a> {
    ir: &'a IrDoc,
    driver: &'a mut dyn Driver,
    engine: Box<dyn Resolver>,
    pub reporter: Reporter,
    stop: Arc<AtomicBool>,
    vars: std::collections::BTreeMap<String, VarValue>,
    frame: Option<Rc<Frame>>,
    step_count: u64,
    stagnation: u32,
    last_action_hash: Option<String>,
    reject: Option<String>,
    deadline: Instant,
    visits: HashMap<(String, String), u32>,
    current_flow: String,
}

impl<'a> Interpreter<'a> {
    pub fn new(
        ir: &'a IrDoc,
        driver: &'a mut dyn Driver,
        engine: Box<dyn Resolver>,
        reporter: Reporter,
        stop: Arc<AtomicBool>,
    ) -> Self {
        let budget = Duration::from_millis(ir.meta.max_duration_ms.max(1000));
        Self {
            ir,
            driver,
            engine,
            reporter,
            stop,
            vars: ir.vars.clone(),
            frame: None,
            step_count: 0,
            stagnation: 0,
            last_action_hash: None,
            reject: None,
            deadline: Instant::now() + budget,
            visits: HashMap::new(),
            current_flow: ir.entry.clone(),
        }
    }

    pub fn vars(&self) -> &std::collections::BTreeMap<String, VarValue> {
        &self.vars
    }

    pub fn into_reporter(self) -> Reporter {
        self.reporter
    }

    // ------------------------------------------------------------------ run

    pub fn run(&mut self) -> RunResult {
        let started_at = now_ms();
        self.reporter.log("info", "run_start", json!({ "script": self.ir.meta.name, "entry": self.ir.entry }));

        let entry = self.ir.entry.clone();
        let outcome = self.run_flow(&entry, 0);
        let mut result = RunResult {
            state: RunState::Done,
            step_count: self.step_count,
            error_code: None,
            error_msg: None,
            error_node: None,
            started_at,
            ended_at: 0,
        };

        match outcome {
            Ok(_) => self.reporter.log("info", "run_done", json!({ "steps": self.step_count })),
            Err(error) if error.reason == Reason::Stopped => {
                self.reporter.log("info", "run_stopped", json!({ "node": error.node_id }));
                result.state = RunState::Stopped;
                result.error_code = Some(Reason::Stopped.as_str().into());
                result.error_msg = Some(error.message);
            }
            Err(error) => {
                self.reporter.log(
                    "error",
                    "run_failed",
                    json!({ "reason": error.reason.as_str(), "message": error.message, "nodeId": error.node_id }),
                );
                result.state = RunState::Failed;
                result.error_code = Some(error.reason.as_str().into());
                result.error_msg = Some(error.message);
                result.error_node = error.node_id;
            }
        }
        result.step_count = self.step_count;
        result.ended_at = now_ms();
        result
    }

    fn run_flow(&mut self, name: &str, depth: usize) -> RunResultOf<ChainEnd> {
        if depth > MAX_CALL_DEPTH {
            return Err(RunError::new(Reason::CallDepth, format!("call depth over {MAX_CALL_DEPTH}")));
        }
        let ir = self.ir;
        let flow = ir
            .flow(name)
            .ok_or_else(|| RunError::new(Reason::UnknownFlow, format!("flow \"{name}\" is not defined")))?;
        let start = flow
            .start()
            .ok_or_else(|| RunError::new(Reason::BadStep, format!("flow \"{name}\" has no start node")))?;

        let previous = std::mem::replace(&mut self.current_flow, name.to_string());
        let result = self.run_chain(flow, Some(start.id.as_str()), depth);
        self.current_flow = previous;
        result
    }

    /// Walk from `first`, following wired handles until the chain runs out.
    fn run_chain(&mut self, flow: &'a Flow, first: Option<&'a str>, depth: usize) -> RunResultOf<ChainEnd> {
        let mut current = first;
        while let Some(id) = current {
            let node = flow
                .node(id)
                .ok_or_else(|| RunError::new(Reason::BadStep, format!("edge points at unknown node \"{id}\"")))?;
            self.tick(node)?;

            let flow_name = self.current_flow.clone();
            let is_start = matches!(node.kind, NodeKind::Start(_));
            if !is_start {
                self.reporter.node(&flow_name, &node.id, NodeState::Running, None, None, None);
            }

            let outcome = match self.exec(flow, node, depth) {
                Ok(outcome) => outcome,
                Err(error) => {
                    let error = error.with_node(&node.id);
                    if let Some(route) = routed_handle(&node.kind, error.reason) {
                        if flow.next(&node.id, route).is_some() {
                            self.reporter.log(
                                "warn",
                                "routed_failure",
                                json!({ "nodeId": node.id, "reason": error.reason.as_str(), "handle": route, "message": error.message }),
                            );
                            self.reporter.node(&flow_name, &node.id, NodeState::Failed, None, None, Some(error.message.clone()));
                            self.count_step()?;
                            current = flow.next(&node.id, route);
                            continue;
                        }
                    }
                    self.reporter.node(&flow_name, &node.id, NodeState::Failed, None, None, Some(error.message.clone()));
                    self.count_step().ok();
                    return Err(error);
                }
            };

            if !is_start {
                self.reporter.node(&flow_name, &node.id, NodeState::Ok, None, None, None);
                self.count_step()?;
            }

            match outcome {
                Outcome::End => return Ok(ChainEnd::Ended),
                Outcome::Follow(handle_name) => current = flow.next(&node.id, handle_name),
                Outcome::FollowOwned(handle_name) => current = flow.next(&node.id, &handle_name),
            }
        }
        Ok(ChainEnd::Done)
    }

    fn count_step(&mut self) -> RunResultOf<()> {
        self.step_count += 1;
        if self.step_count > MAX_STEPS {
            return Err(RunError::new(Reason::StepLimit, format!("exceeded {MAX_STEPS} steps")));
        }
        Ok(())
    }

    fn tick(&mut self, node: &Node) -> RunResultOf<()> {
        self.check_stop()?;
        self.check_deadline(&node.id)?;
        let visits = self.visits.entry((self.current_flow.clone(), node.id.clone())).or_insert(0);
        *visits += 1;
        if let Some(limit) = node.max_visits {
            if *visits > limit {
                return Err(RunError::at(Reason::VisitLimit, format!("node ran more than {limit} times"), &node.id));
            }
        }
        Ok(())
    }

    fn check_stop(&self) -> RunResultOf<()> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(RunError::stopped());
        }
        Ok(())
    }

    fn check_deadline(&self, node_id: &str) -> RunResultOf<()> {
        if Instant::now() > self.deadline {
            return Err(RunError::at(Reason::Timeout, "run exceeded maxDurationMs", node_id));
        }
        Ok(())
    }

    fn sleep(&self, ms: f64) -> RunResultOf<()> {
        if ms <= 0.0 {
            return Ok(());
        }
        // Long sleeps are sliced so a stop request is honoured within ~100ms.
        let mut remaining = ms;
        while remaining > 0.0 {
            self.check_stop()?;
            let chunk = remaining.min(POLL_SLICE_MS);
            std::thread::sleep(Duration::from_secs_f64(chunk / 1000.0));
            remaining -= chunk;
        }
        Ok(())
    }

    // ------------------------------------------------------------- capture

    fn frame(&mut self, fresh: bool) -> RunResultOf<Rc<Frame>> {
        if fresh || self.frame.is_none() {
            let frame = match self.driver.capture() {
                Ok(frame) => frame,
                // The surface moved mid-grab; one more try gets a consistent frame.
                Err(crate::driver::DriverError::SurfaceChanged) => self.driver.capture()?,
                Err(error) => return Err(error.into()),
            };
            if frame.capture_ms > CAPTURE_STALL_MS {
                self.reporter.log(
                    "error",
                    "capture_stalled",
                    json!({ "ms": frame.capture_ms as u64, "hint": "屏幕录制权限可能失效，或目标窗口被全屏应用遮住" }),
                );
                return Err(RunError::new(
                    Reason::CaptureStalled,
                    format!("a single screenshot took {:.0}ms", frame.capture_ms),
                ));
            }
            self.frame = Some(Rc::new(frame));
        }
        Ok(self.frame.clone().expect("frame was just captured"))
    }

    fn invalidate(&mut self) {
        self.frame = None;
    }

    // ------------------------------------------------------------ matching

    /// Locate a target. A match below the target's `threshold` counts as not
    /// found when `enforce_threshold` is set, because an injected click cannot
    /// be undone. Waiting-for-disappearance and conditions pass `false`: for
    /// them a low score means the screen is changing, not that the control left.
    fn find(&mut self, name: &str, node_id: &str, warn: bool, enforce_threshold: bool) -> RunResultOf<Option<Hit>> {
        let ir = self.ir;
        let target = ir
            .target(name)
            .ok_or_else(|| RunError::at(Reason::UnknownTarget, format!("target \"{name}\" is not defined"), node_id))?;

        let frame = self.frame(false)?;
        let hit = self.engine.resolve(target, name, &frame, (0.0, 0.0))?;
        let Some(hit) = hit else {
            self.reject = Some(format!("target \"{name}\" not matched"));
            return Ok(None);
        };

        self.reporter.score(node_id, name, hit.score, hit.elapsed_ms);

        let (threshold, warn_threshold) = match target {
            Target::Template(t) => (t.threshold, t.warn_threshold),
            // A point target is pure geometry: there is no score to compare.
            Target::Point(_) => (0.0, 0.0),
        };

        if enforce_threshold && hit.score < threshold {
            self.reject = Some(format!("target \"{name}\" matched at {:.3}, below threshold {:.2}", hit.score, threshold));
            // A rejected match is worth reporting: it usually means the crop is
            // slightly off or the UI changed, not that nothing is there.
            self.reporter.log(
                "warn",
                "match_rejected",
                json!({
                    "stepId": node_id, "target": name, "score": round3(hit.score), "threshold": round3(threshold),
                    "x": round1(hit.point.0), "y": round1(hit.point.1),
                }),
            );
            return Ok(None);
        }

        self.reject = None;

        if warn && hit.score < warn_threshold {
            self.reporter.log(
                "warn",
                "low_confidence",
                json!({ "stepId": node_id, "target": name, "score": round3(hit.score), "strategy": hit.strategy }),
            );
        }
        if !hit.warnings.is_empty() {
            self.reporter.log("debug", "match_warnings", json!({ "stepId": node_id, "target": name, "warnings": hit.warnings }));
        }
        Ok(Some(hit))
    }

    fn require(&mut self, name: &str, node_id: &str) -> RunResultOf<Hit> {
        match self.find(name, node_id, true, true)? {
            Some(hit) => Ok(hit),
            None => Err(RunError::at(
                Reason::NotFound,
                self.reject.clone().unwrap_or_else(|| format!("target \"{name}\" not found")),
                node_id,
            )),
        }
    }

    /// Detect a genuine stall: the same screen after a completed input action.
    /// An idle screen alone is normal (games sit still), so only consecutive
    /// *actions* that changed nothing count. The frame is dropped first so the
    /// hash describes the screen *after* the action.
    fn record_action(&mut self, node_id: &str, name: Option<&str>) -> RunResultOf<()> {
        self.invalidate();
        let current = self.frame(false)?.hash().map_err(RunError::from)?.to_string();
        if self.last_action_hash.as_deref() == Some(current.as_str()) {
            self.stagnation += 1;
            if self.stagnation >= STAGNATION_LIMIT {
                return Err(RunError::at(
                    Reason::Stagnation,
                    format!("screen unchanged after {STAGNATION_LIMIT} actions"),
                    node_id,
                ));
            }
            self.reporter.log("debug", "no_screen_change", json!({ "stepId": node_id, "target": name, "count": self.stagnation }));
        } else {
            self.stagnation = 0;
        }
        self.last_action_hash = Some(current);
        self.invalidate();
        Ok(())
    }

    // ------------------------------------------------------------ dispatch

    fn exec(&mut self, flow: &'a Flow, node: &'a Node, depth: usize) -> RunResultOf<Outcome> {
        let id = node.id.as_str();
        match &node.kind {
            NodeKind::Start(_) => Ok(Outcome::Follow(handle::NEXT)),
            NodeKind::End(_) => Ok(Outcome::End),
            NodeKind::Tap(data) => {
                let hit = self.require(&data.target, id)?;
                self.jittered_click(id, &data.target, &hit, data.duration_ms, &data.jitter)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::LongPress(data) => {
                let hit = self.require(&data.target, id)?;
                self.jittered_click(id, &data.target, &hit, data.duration_ms, &data.jitter)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Swipe(data) => {
                self.swipe(id, data)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Key(data) => {
                self.driver.key(&data.key)?;
                self.record_action(id, None)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Text(data) => {
                self.driver.text(&data.text)?;
                self.record_action(id, None)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Log(data) => {
                self.reporter.log(&data.level, "script_log", json!({ "stepId": id, "msg": data.msg }));
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Snapshot(data) => {
                let frame = self.frame(true)?;
                let hash = frame.hash().map_err(RunError::from)?;
                self.reporter.log("info", "snapshot", json!({ "stepId": id, "label": data.label, "hash": &hash[..16.min(hash.len())] }));
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Sleep(data) => {
                // A long sleep must not sail past the run deadline: cap it, then check.
                let remaining = self.deadline.saturating_duration_since(Instant::now()).as_secs_f64() * 1000.0;
                self.sleep((data.ms as f64).min(remaining))?;
                self.check_deadline(id)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::WaitFor(data) => {
                self.wait_for(id, &data.any, data.timeout_ms, data.interval_ms)?;
                Ok(Outcome::Follow(handle::FOUND))
            }
            NodeKind::WaitGone(data) => {
                self.wait_gone(id, &data.target, data.timeout_ms, data.interval_ms)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::WaitStable(data) => {
                self.wait_stable(id, data.ms, data.timeout_ms)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::If(data) => {
                let value = self.eval(&data.when, id)?;
                Ok(Outcome::Follow(if value { handle::TRUE } else { handle::FALSE }))
            }
            NodeKind::Switch(data) => {
                self.invalidate();
                for case in &data.cases {
                    if self.find(case, id, false, false)?.is_some() {
                        return Ok(Outcome::FollowOwned(format!("{}{case}", handle::CASE_PREFIX)));
                    }
                }
                Ok(Outcome::Follow(handle::DEFAULT))
            }
            NodeKind::Loop(data) => {
                if let Some(Ended) = self.run_loop(flow, node, data, depth)? {
                    return Ok(Outcome::End);
                }
                Ok(Outcome::Follow(handle::DONE))
            }
            NodeKind::Retry(data) => self.run_retry(flow, node, data.times, data.backoff_ms, depth),
            NodeKind::SetVar(data) => {
                self.vars.insert(data.name.clone(), data.value.clone());
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Call(data) => {
                // `end` inside the callee only returns to this node.
                self.run_flow(&data.flow, depth + 1)?;
                Ok(Outcome::Follow(handle::NEXT))
            }
        }
    }

    // ----------------------------------------------------------- primitives

    fn jittered_click(&mut self, id: &str, name: &str, hit: &Hit, duration_ms: u32, jitter: &Jitter) -> RunResultOf<()> {
        let delay_ms = random_delay_ms(jitter);
        if delay_ms > 0.0 {
            self.sleep(delay_ms)?;
            self.check_deadline(id)?;
        }

        let (dx, dy) = self.random_offset(jitter.offset_ratio, id);
        let mut point = hit.point;
        if (dx, dy) != (0.0, 0.0) {
            point = clamp_to_rect(point.0 + dx * hit.rect.w, point.1 + dy * hit.rect.h, &hit.rect);
        }
        self.driver.tap(point.0, point.1, duration_ms)?;
        self.reporter.log(
            "info",
            "tap",
            json!({
                "stepId": id,
                "target": name,
                "score": round3(hit.score),
                "x": round2(point.0),
                "y": round2(point.1),
                "anchorX": round2(hit.point.0),
                "anchorY": round2(hit.point.1),
                "dx": round2(point.0 - hit.point.0),
                "dy": round2(point.1 - hit.point.1),
                "delayMs": delay_ms as i64,
                "boxW": round1(hit.rect.w),
                "boxH": round1(hit.rect.h),
                "at": chrono::Local::now().format("%H:%M:%S").to_string(),
                "strategy": hit.strategy,
            }),
        );
        self.reporter.node(&self.current_flow.clone(), id, NodeState::Running, Some(hit.score), Some(point), None);
        self.record_action(id, Some(name))
    }

    /// Random offset as a fraction of the matched box. `0.2` picks a point
    /// within ±20% of the box around the anchor; capped at half a box so the
    /// click stays "inside the region".
    fn random_offset(&mut self, ratio: f64, id: &str) -> (f64, f64) {
        if ratio <= 0.0 || !ratio.is_finite() {
            return (0.0, 0.0);
        }
        let mut ratio = ratio;
        if ratio > 0.5 {
            self.reporter.log("warn", "offset_clamped", json!({ "stepId": id, "requested": ratio, "used": 0.5 }));
            ratio = 0.5;
        }
        let mut rng = rand::rng();
        (rng.random_range(-ratio..=ratio), rng.random_range(-ratio..=ratio))
    }

    fn swipe(&mut self, id: &str, data: &SwipeData) -> RunResultOf<()> {
        let start = self.require(&data.from, id)?;
        let end_point = match &data.to {
            Some(to) => self.require(to, id)?.point,
            None => {
                let vector = data.vector.unwrap_or([0.0, -0.3]);
                let bounds = self.driver.bounds()?;
                (start.point.0 + vector[0] * bounds.w, start.point.1 + vector[1] * bounds.h)
            }
        };
        self.driver.swipe(start.point.0, start.point.1, end_point.0, end_point.1, data.duration_ms)?;
        self.record_action(id, Some(&data.from))
    }

    fn wait_for(&mut self, id: &str, names: &[String], timeout_ms: u64, interval_ms: u64) -> RunResultOf<()> {
        if names.is_empty() {
            return Err(RunError::at(Reason::BadStep, "waitFor needs at least one target", id));
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let interval = interval_ms.max(50) as f64;
        loop {
            self.check_stop()?;
            self.check_deadline(id)?;
            self.invalidate();
            for name in names {
                if self.find(name, id, true, true)?.is_some() {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(RunError::at(Reason::Timeout, format!("none of {names:?} appeared"), id));
            }
            self.sleep(interval)?;
        }
    }

    fn wait_gone(&mut self, id: &str, name: &str, timeout_ms: u64, interval_ms: u64) -> RunResultOf<()> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let interval = interval_ms.max(50) as f64;
        loop {
            self.check_stop()?;
            self.check_deadline(id)?;
            self.invalidate();
            if self.find(name, id, false, false)?.is_none() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(RunError::at(Reason::Timeout, format!("target \"{name}\" never disappeared"), id));
            }
            self.sleep(interval)?;
        }
    }

    fn wait_stable(&mut self, id: &str, hold_ms: u64, timeout_ms: u64) -> RunResultOf<()> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let mut stable_since: Option<Instant> = None;
        let mut previous: Option<String> = None;
        loop {
            self.check_stop()?;
            self.check_deadline(id)?;
            let frame = self.frame(true)?;
            let current = frame.hash().map_err(RunError::from)?.to_string();
            let now = Instant::now();
            if previous.as_deref() == Some(current.as_str()) {
                let since = *stable_since.get_or_insert(now);
                if now.duration_since(since).as_millis() as u64 >= hold_ms {
                    return Ok(());
                }
            } else {
                stable_since = None;
            }
            previous = Some(current);
            if now >= deadline {
                return Err(RunError::at(Reason::Timeout, "screen never settled", id));
            }
            self.sleep(120.0)?;
        }
    }

    // ------------------------------------------------------------ containers

    fn run_loop(
        &mut self,
        flow: &'a Flow,
        node: &'a Node,
        data: &'a liverguard_ir::LoopData,
        depth: usize,
    ) -> RunResultOf<Option<Ended>> {
        let body = flow.next(&node.id, handle::BODY);
        let cap = data.max_iterations.unwrap_or(match data.mode {
            LoopMode::Count => data.count,
            _ => DEFAULT_LOOP_CAP,
        });
        let mut iterations = 0u32;

        loop {
            self.check_stop()?;
            self.check_deadline(&node.id)?;
            let proceed = match data.mode {
                LoopMode::Count => iterations < data.count,
                LoopMode::Forever => true,
                LoopMode::While => match &data.when {
                    Some(condition) => self.eval(condition, &node.id)?,
                    None => return Err(RunError::at(Reason::BadStep, "a while loop needs a condition", &node.id)),
                },
            };
            if !proceed || iterations >= cap {
                if proceed && data.mode != LoopMode::Count {
                    self.reporter.log("warn", "loop_cap", json!({ "nodeId": node.id, "iterations": iterations }));
                }
                return Ok(None);
            }
            iterations += 1;
            if body.is_some() && self.run_chain(flow, body, depth)? == ChainEnd::Ended {
                return Ok(Some(Ended));
            }
        }
    }

    fn run_retry(&mut self, flow: &'a Flow, node: &'a Node, times: u32, backoff_ms: u64, depth: usize) -> RunResultOf<Outcome> {
        let body = flow.next(&node.id, handle::BODY);
        let times = times.max(1);
        let mut last: Option<RunError> = None;

        for attempt in 0..times {
            match self.run_chain(flow, body, depth) {
                Ok(ChainEnd::Ended) => return Ok(Outcome::End),
                Ok(ChainEnd::Done) => return Ok(Outcome::Follow(handle::DONE)),
                Err(error) => {
                    if matches!(error.reason, Reason::Stopped | Reason::Stagnation) {
                        return Err(error);
                    }
                    self.reporter.log(
                        "warn",
                        "retry",
                        json!({ "stepId": node.id, "attempt": attempt + 1, "reason": error.reason.as_str() }),
                    );
                    last = Some(error);
                    self.invalidate();
                    if attempt + 1 < times {
                        self.sleep(backoff_ms as f64 * 2f64.powi(attempt as i32))?;
                    }
                }
            }
        }

        let last = last.expect("at least one attempt ran");
        if flow.next(&node.id, handle::FAILED).is_some() {
            return Ok(Outcome::Follow(handle::FAILED));
        }
        Err(RunError::at(last.reason, format!("retry exhausted: {}", last.message), &node.id))
    }

    // ------------------------------------------------------------ conditions

    fn eval(&mut self, condition: &Condition, id: &str) -> RunResultOf<bool> {
        match condition {
            Condition::Visible { targets } => {
                self.invalidate();
                // Conditions ask "is it there"; a low score means the screen is in
                // motion, so it must not be treated as absent.
                for name in targets {
                    if self.find(name, id, false, false)?.is_some() {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Condition::Missing { target } => {
                self.invalidate();
                Ok(self.find(target, id, false, false)?.is_none())
            }
            Condition::Var { name, op, value } => Ok(compare(self.vars.get(name), *op, value)),
            Condition::Not { cond } => Ok(!self.eval(cond, id)?),
            Condition::All { conds } => {
                for cond in conds {
                    if !self.eval(cond, id)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Condition::Any { conds } => {
                for cond in conds {
                    if self.eval(cond, id)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
}

struct Ended;

/// Which failure handle a node routes `reason` to, if it has one.
fn routed_handle(kind: &NodeKind, reason: Reason) -> Option<&'static str> {
    if !reason.is_routable() {
        return None;
    }
    match kind {
        NodeKind::Tap(_) | NodeKind::LongPress(_) | NodeKind::Swipe(_) if reason == Reason::NotFound => Some(handle::FAIL),
        NodeKind::WaitFor(_) | NodeKind::WaitGone(_) | NodeKind::WaitStable(_) if reason == Reason::Timeout => {
            Some(handle::TIMEOUT)
        }
        _ => None,
    }
}

fn random_delay_ms(jitter: &Jitter) -> f64 {
    let fixed = jitter.delay_ms.max(0.0);
    match jitter.delay_range_ms {
        Some([low, high]) => {
            let low = low.max(0.0);
            let high = high.max(low);
            fixed + rand::rng().random_range(low..=high)
        }
        None => fixed,
    }
}

/// Keeps a jittered point on the matched control; `dx`/`dy` in the log still
/// report where it actually landed.
fn clamp_to_rect(x: f64, y: f64, rect: &Rect) -> (f64, f64) {
    (x.max(rect.x).min(rect.x + rect.w), y.max(rect.y).min(rect.y + rect.h))
}

fn as_number(value: &VarValue) -> Option<f64> {
    match value {
        VarValue::Num(n) => Some(*n),
        VarValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        VarValue::Str(s) => s.trim().parse().ok(),
    }
}

fn as_text(value: &VarValue) -> String {
    match value {
        VarValue::Str(s) => s.clone(),
        VarValue::Num(n) => n.to_string(),
        VarValue::Bool(b) => b.to_string(),
    }
}

/// Variable comparison. Anything that cannot be compared is `false`.
pub fn compare(actual: Option<&VarValue>, op: CompareOp, expected: &VarValue) -> bool {
    match op {
        CompareOp::Eq | CompareOp::Ne => {
            let equal = match (actual, expected) {
                (None, _) => false,
                (Some(a), b) => match (a, b) {
                    (VarValue::Str(x), VarValue::Str(y)) => x == y,
                    _ => match (as_number(a), as_number(b)) {
                        (Some(x), Some(y)) if !matches!((a, b), (VarValue::Str(_), _) | (_, VarValue::Str(_))) => x == y,
                        _ => as_text(a) == as_text(b),
                    },
                },
            };
            if op == CompareOp::Eq {
                equal
            } else {
                !equal
            }
        }
        CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge => {
            let (Some(a), Some(b)) = (actual.and_then(as_number), as_number(expected)) else { return false };
            match op {
                CompareOp::Lt => a < b,
                CompareOp::Le => a <= b,
                CompareOp::Gt => a > b,
                _ => a >= b,
            }
        }
        CompareOp::Contains => actual.is_some_and(|a| as_text(a).contains(&as_text(expected))),
        CompareOp::Match => match (actual, regex::Regex::new(&as_text(expected))) {
            (Some(a), Ok(re)) => re.is_match(&as_text(a)),
            _ => false,
        },
    }
}

fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}
fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}
fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
mod compare_tests {
    use super::*;

    #[test]
    fn numeric_and_text_comparisons() {
        let n = |v| VarValue::Num(v);
        let s = |v: &str| VarValue::Str(v.into());
        assert!(compare(Some(&n(3.0)), CompareOp::Gt, &n(2.0)));
        assert!(compare(Some(&s("12")), CompareOp::Ge, &n(12.0)));
        assert!(!compare(None, CompareOp::Eq, &n(1.0)));
        assert!(compare(None, CompareOp::Ne, &n(1.0)) == !compare(None, CompareOp::Eq, &n(1.0)));
        assert!(compare(Some(&s("hello world")), CompareOp::Contains, &s("lo wo")));
        assert!(compare(Some(&s("abc123")), CompareOp::Match, &s(r"\d+$")));
        assert!(!compare(Some(&s("abc")), CompareOp::Lt, &n(1.0)));
        assert!(compare(Some(&VarValue::Bool(true)), CompareOp::Eq, &VarValue::Bool(true)));
    }
}

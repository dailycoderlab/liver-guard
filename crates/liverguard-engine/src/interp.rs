//! Graph interpreter.
//!
//! Synchronous on purpose: it runs on a worker thread, so capture, matching and
//! input injection never interleave with the UI thread.
//!
//! The graph is walked from its `start` node by following the wired output handle
//! each node picks. Containers (`loop`, `retry`) run the chain hanging off their
//! `body` handle to its end, then continue from `done`. Reaching an `end` node
//! ends the run.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use liverguard_ir::node::handle;
use liverguard_ir::{
    CompareOp, Condition, Flow, IrDoc, LoopMode, Node, NodeKind, SwipeData, Target, VarValue,
};
use liverguard_vision::{Frame, Hit, Resolver};
use rand::Rng;
use serde_json::json;

use crate::driver::{Driver, PointerAction};
use crate::error::{Reason, RunError, RunResultOf};
use crate::human;
use crate::report::{now_ms, NodeState, Reporter};

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
    /// How many times `run` walks the graph from `start`; always at least 1.
    rounds: u32,
    /// The round being walked (1-based); stays on the failing round when a run fails.
    round: u32,
    alert: Option<(liverguard_notify::NotifyConfig, crate::alert::AlertSender)>,
    /// `step_count` when the current round began; `MAX_STEPS` is measured from here.
    round_first_step: u64,
    stagnation: u32,
    last_action_hash: Option<String>,
    reject: Option<String>,
    visits: HashMap<String, u32>,
}

impl<'a> Interpreter<'a> {
    pub fn new(
        ir: &'a IrDoc,
        driver: &'a mut dyn Driver,
        engine: Box<dyn Resolver>,
        reporter: Reporter,
        stop: Arc<AtomicBool>,
    ) -> Self {
        Self {
            ir,
            driver,
            engine,
            reporter,
            stop,
            vars: Default::default(),
            frame: None,
            step_count: 0,
            rounds: 1,
            round: 0,
            alert: None,
            round_first_step: 0,
            stagnation: 0,
            last_action_hash: None,
            reject: None,
            visits: HashMap::new(),
        }
    }

    /// Walk the graph this many times in one run (default 1, values below 1 count as 1).
    pub fn set_rounds(&mut self, rounds: u32) {
        self.rounds = rounds.max(1);
    }

    /// Send a webhook message when the run ends in an unhandled failure, and, for a looping run
    /// that completes every round, a done notice. Does nothing unless
    /// `config.is_enabled()` (a webhook URL is set). The outcome is logged as `alert_sent` / `alert_failed`; a failed
    /// delivery never changes the run's own result.
    pub fn set_alert(&mut self, config: liverguard_notify::NotifyConfig) {
        self.set_alert_with(config, crate::alert::webhook_sender());
    }

    /// Like `set_alert` with a custom delivery function (tests).
    pub fn set_alert_with(
        &mut self,
        config: liverguard_notify::NotifyConfig,
        sender: crate::alert::AlertSender,
    ) {
        self.alert = config.is_enabled().then_some((config, sender));
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
        self.reporter
            .log("info", "run_start", json!({ "script": self.ir.meta.name }));

        let outcome = self.run_rounds();
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
            Ok(_) => self.reporter.log(
                "info",
                "run_done",
                json!({ "steps": self.step_count, "rounds": self.rounds }),
            ),
            Err(error) if error.reason == Reason::Stopped => {
                self.reporter
                    .log("info", "run_stopped", json!({ "node": error.node_id }));
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
        match result.state {
            RunState::Failed => self.notify(&result),
            // Only a looping run that finished every round: a single run's success is not news.
            RunState::Done if self.rounds > 1 => self.notify(&result),
            _ => {}
        }
        result
    }

    /// Tell the configured webhook how the run ended: a failure alert for `Failed`, the
    /// loop-done notice for `Done`. Delivery trouble is only logged.
    fn notify(&mut self, result: &RunResult) {
        let Some((config, sender)) = self.alert.clone() else {
            return;
        };
        let message = match result.state {
            RunState::Failed => {
                crate::alert::failure_message(self.ir, result, self.round, self.rounds)
            }
            RunState::Done => crate::alert::loop_done_message(self.ir, result, self.rounds),
            _ => return,
        };
        match sender(&config, &message) {
            Ok(()) => self.reporter.log(
                "info",
                "alert_sent",
                json!({ "event": message.event, "round": self.round }),
            ),
            Err(error) => self.reporter.log(
                "warn",
                "alert_failed",
                json!({ "event": message.event, "message": error.to_string() }),
            ),
        }
    }

    /// Run the graph `rounds` times (set by the caller, not part of the script). Every round starts from `start` with no
    /// variables set; an `end` node finishes the current round only. A failure or a stop ends the
    /// whole run. There is no overall time limit: a run lasts until it ends or is stopped.
    fn run_rounds(&mut self) -> RunResultOf<ChainEnd> {
        let rounds = self.rounds;
        let mut last = ChainEnd::Done;
        for round in 1..=rounds {
            self.round = round;
            self.begin_round();
            if rounds > 1 {
                self.reporter.log(
                    "info",
                    "round_start",
                    json!({ "round": round, "total": rounds }),
                );
            }
            last = self.run_graph()?;
        }
        Ok(last)
    }

    /// Forget everything one round learned so the next behaves like a fresh run.
    fn begin_round(&mut self) {
        self.vars.clear();
        self.frame = None;
        self.stagnation = 0;
        self.last_action_hash = None;
        self.reject = None;
        self.visits.clear();
        self.round_first_step = self.step_count;
    }

    fn run_graph(&mut self) -> RunResultOf<ChainEnd> {
        let ir = self.ir;
        let flow = &ir.flow;
        let start = flow
            .start()
            .ok_or_else(|| RunError::new(Reason::BadStep, "script has no start node"))?;
        self.run_chain(flow, Some(start.id.as_str()))
    }

    /// Walk from `first`, following wired handles until the chain runs out.
    fn run_chain(&mut self, flow: &'a Flow, first: Option<&'a str>) -> RunResultOf<ChainEnd> {
        let mut current = first;
        while let Some(id) = current {
            let node = flow.node(id).ok_or_else(|| {
                RunError::new(
                    Reason::BadStep,
                    format!("edge points at unknown node \"{id}\""),
                )
            })?;
            self.tick(node)?;

            let is_start = matches!(node.kind, NodeKind::Start(_));
            if !is_start {
                self.reporter
                    .node(&node.id, NodeState::Running, None, None, None);
            }

            let outcome = match self.exec(flow, node) {
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
                            self.reporter.node(
                                &node.id,
                                NodeState::Failed,
                                None,
                                None,
                                Some(error.message.clone()),
                            );
                            self.count_step()?;
                            current = flow.next(&node.id, route);
                            continue;
                        }
                    }
                    self.reporter.node(
                        &node.id,
                        NodeState::Failed,
                        None,
                        None,
                        Some(error.message.clone()),
                    );
                    self.count_step().ok();
                    return Err(error);
                }
            };

            if !is_start {
                self.reporter
                    .node(&node.id, NodeState::Ok, None, None, None);
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
        // The cap guards one round against a runaway graph; many rounds may add up to more.
        if self.step_count - self.round_first_step > MAX_STEPS {
            return Err(RunError::new(
                Reason::StepLimit,
                format!("exceeded {MAX_STEPS} steps"),
            ));
        }
        Ok(())
    }

    fn tick(&mut self, node: &Node) -> RunResultOf<()> {
        self.check_stop()?;
        let visits = self.visits.entry(node.id.clone()).or_insert(0);
        *visits += 1;
        if let Some(limit) = node.max_visits {
            if *visits > limit {
                return Err(RunError::at(
                    Reason::VisitLimit,
                    format!("node ran more than {limit} times"),
                    &node.id,
                ));
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
    fn find(
        &mut self,
        name: &str,
        node_id: &str,
        warn: bool,
        enforce_threshold: bool,
    ) -> RunResultOf<Option<Hit>> {
        let ir = self.ir;
        let target = ir.target(name).ok_or_else(|| {
            RunError::at(
                Reason::UnknownTarget,
                format!("target \"{name}\" is not defined"),
                node_id,
            )
        })?;

        let frame = self.frame(false)?;
        let hit = self.engine.resolve(target, name, &frame, (0.0, 0.0))?;
        let Some(hit) = hit else {
            self.reject = Some(format!("target \"{name}\" not matched"));
            return Ok(None);
        };

        self.reporter
            .score(node_id, name, hit.score, hit.elapsed_ms);

        let (threshold, warn_threshold) = match target {
            Target::Template(t) => (t.threshold, t.warn_threshold),
            // A point target is pure geometry: there is no score to compare.
            Target::Point(_) => (0.0, 0.0),
        };

        if enforce_threshold && hit.score < threshold {
            self.reject = Some(format!(
                "target \"{name}\" matched at {:.3}, below threshold {:.2}",
                hit.score, threshold
            ));
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
            self.reporter.log(
                "debug",
                "match_warnings",
                json!({ "stepId": node_id, "target": name, "warnings": hit.warnings }),
            );
        }
        Ok(Some(hit))
    }

    fn require(&mut self, name: &str, node_id: &str) -> RunResultOf<Hit> {
        match self.find(name, node_id, true, true)? {
            Some(hit) => Ok(hit),
            None => Err(RunError::at(
                Reason::NotFound,
                self.reject
                    .clone()
                    .unwrap_or_else(|| format!("target \"{name}\" not found")),
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
        let current = self
            .frame(false)?
            .hash()
            .map_err(RunError::from)?
            .to_string();
        if self.last_action_hash.as_deref() == Some(current.as_str()) {
            self.stagnation += 1;
            if self.stagnation >= STAGNATION_LIMIT {
                return Err(RunError::at(
                    Reason::Stagnation,
                    format!("screen unchanged after {STAGNATION_LIMIT} actions"),
                    node_id,
                ));
            }
            self.reporter.log(
                "debug",
                "no_screen_change",
                json!({ "stepId": node_id, "target": name, "count": self.stagnation }),
            );
        } else {
            self.stagnation = 0;
        }
        self.last_action_hash = Some(current);
        self.invalidate();
        Ok(())
    }

    // ------------------------------------------------------------ dispatch

    fn exec(&mut self, flow: &'a Flow, node: &'a Node) -> RunResultOf<Outcome> {
        let id = node.id.as_str();
        match &node.kind {
            NodeKind::Start(_) => Ok(Outcome::Follow(handle::NEXT)),
            NodeKind::End(_) => Ok(Outcome::End),
            NodeKind::Tap(data) => {
                let hit = self.require(&data.target, id)?;
                self.click(
                    id,
                    &data.target,
                    &hit,
                    data.duration_ms,
                    data.human,
                    data.spread,
                )?;
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::LongPress(data) => {
                let hit = self.require(&data.target, id)?;
                self.click(
                    id,
                    &data.target,
                    &hit,
                    data.duration_ms,
                    data.human,
                    data.spread,
                )?;
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
                self.reporter.log(
                    &data.level,
                    "script_log",
                    json!({ "stepId": id, "msg": data.msg }),
                );
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Snapshot(data) => {
                let frame = self.frame(true)?;
                let hash = frame.hash().map_err(RunError::from)?;
                self.reporter.log("info", "snapshot", json!({ "stepId": id, "label": data.label, "hash": &hash[..16.min(hash.len())] }));
                Ok(Outcome::Follow(handle::NEXT))
            }
            NodeKind::Sleep(data) => {
                let wait_ms = pick_sleep_ms(data.min_ms, data.max_ms);
                self.reporter.log("info", "sleep", json!({ "stepId": id, "ms": wait_ms, "minMs": data.min_ms, "maxMs": data.max_ms }));
                self.sleep(wait_ms as f64)?;
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
                Ok(Outcome::Follow(if value {
                    handle::TRUE
                } else {
                    handle::FALSE
                }))
            }
            NodeKind::Switch(data) => {
                self.invalidate();
                for case in &data.cases {
                    if self.find(case, id, false, false)?.is_some() {
                        return Ok(Outcome::FollowOwned(format!(
                            "{}{case}",
                            handle::CASE_PREFIX
                        )));
                    }
                }
                Ok(Outcome::Follow(handle::DEFAULT))
            }
            NodeKind::Loop(data) => {
                if let Some(Ended) = self.run_loop(flow, node, data)? {
                    return Ok(Outcome::End);
                }
                Ok(Outcome::Follow(handle::DONE))
            }
            NodeKind::Retry(data) => self.run_retry(flow, node, data.times, data.backoff_ms),
            NodeKind::SetVar(data) => {
                self.vars.insert(data.name.clone(), data.value.clone());
                Ok(Outcome::Follow(handle::NEXT))
            }
        }
    }

    // ----------------------------------------------------------- primitives

    /// Click the matched box's anchor exactly, or, when `human` is set, the way a
    /// hand would: a landing point scattered around the anchor, a curved mouse trip
    /// from where the pointer is, a hesitation, and a jittered press (see `human`).
    /// `dx`/`dy` in the log report where it landed.
    fn click(
        &mut self,
        id: &str,
        name: &str,
        hit: &Hit,
        duration_ms: u32,
        human: bool,
        spread: f64,
    ) -> RunResultOf<()> {
        let mut human_fields = serde_json::Map::new();
        let point = if human {
            let (point, plan) = {
                let mut rng = rand::rng();
                let point = human::human_point(&hit.rect, hit.point, spread, &mut rng);
                let from = self.driver.pointer_position()?;
                (
                    point,
                    human::plan_click(
                        from,
                        point,
                        hit.rect.w.min(hit.rect.h),
                        duration_ms,
                        &mut rng,
                    ),
                )
            };
            self.play(&plan)?;
            human_fields.insert("travelMs".into(), json!(round1(plan.travel_ms)));
            human_fields.insert("holdMs".into(), json!(round1(plan.hold_ms)));
            human_fields.insert("overshoot".into(), json!(plan.overshoot));
            human_fields.insert("spread".into(), json!(human::clamp_spread(spread)));
            point
        } else {
            self.driver.tap(hit.point.0, hit.point.1, duration_ms)?;
            hit.point
        };
        let mut fields = json!({
            "stepId": id,
            "target": name,
            "score": round3(hit.score),
            "x": round2(point.0),
            "y": round2(point.1),
            "anchorX": round2(hit.point.0),
            "anchorY": round2(hit.point.1),
            "dx": round2(point.0 - hit.point.0),
            "dy": round2(point.1 - hit.point.1),
            "boxW": round1(hit.rect.w),
            "boxH": round1(hit.rect.h),
            "at": chrono::Local::now().format("%H:%M:%S").to_string(),
            "strategy": hit.strategy,
            "human": human,
        });
        if let Some(object) = fields.as_object_mut() {
            object.extend(human_fields);
        }
        self.reporter.log("info", "tap", fields);
        self.reporter
            .node(id, NodeState::Running, Some(hit.score), Some(point), None);
        self.record_action(id, Some(name))
    }

    fn swipe(&mut self, id: &str, data: &SwipeData) -> RunResultOf<()> {
        let start = self.require(&data.from, id)?;
        let end_hit = match &data.to {
            Some(to) => Some(self.require(to, id)?),
            None => None,
        };
        if data.human {
            return self.swipe_human(id, data, &start, end_hit.as_ref());
        }
        let end_point = match &end_hit {
            Some(hit) => hit.point,
            None => {
                let vector = data.vector.unwrap_or([0.0, -0.3]);
                let bounds = self.driver.bounds()?;
                (
                    start.point.0 + vector[0] * bounds.w,
                    start.point.1 + vector[1] * bounds.h,
                )
            }
        };
        self.driver.swipe(
            start.point.0,
            start.point.1,
            end_point.0,
            end_point.1,
            data.duration_ms,
        )?;
        self.record_action(id, Some(&data.from))
    }

    /// A drag as a hand draws it: start and end scattered inside their boxes (or the
    /// vector's end point jittered), approached with a curved mouse trip, stroked
    /// along an arc with a bell-shaped speed over a jittered duration.
    fn swipe_human(
        &mut self,
        id: &str,
        data: &SwipeData,
        start_hit: &Hit,
        end_hit: Option<&Hit>,
    ) -> RunResultOf<()> {
        let surface = self.driver.bounds()?;
        let (start, end, plan) = {
            let mut rng = rand::rng();
            let start = human::human_point(&start_hit.rect, start_hit.point, data.spread, &mut rng);
            let end = match end_hit {
                Some(hit) => human::human_point(&hit.rect, hit.point, data.spread, &mut rng),
                None => {
                    let vector = data.vector.unwrap_or([0.0, -0.3]);
                    let nominal = (
                        start.0 + vector[0] * surface.w,
                        start.1 + vector[1] * surface.h,
                    );
                    let jittered = human::jitter_drag_end(start, nominal, data.spread, &mut rng);
                    // A drag that runs off the captured surface would press into whatever is outside it.
                    (
                        jittered.0.clamp(surface.x, surface.x + surface.w),
                        jittered.1.clamp(surface.y, surface.y + surface.h),
                    )
                }
            };
            let from = self.driver.pointer_position()?;
            let plan = human::plan_drag(
                from,
                start,
                start_hit.rect.w.min(start_hit.rect.h),
                end,
                data.duration_ms,
                &mut rng,
            );
            (start, end, plan)
        };
        self.play(&plan)?;
        self.reporter.log(
            "info",
            "swipe",
            json!({
                "stepId": id,
                "target": data.from,
                "fromX": round2(start.0),
                "fromY": round2(start.1),
                "toX": round2(end.0),
                "toY": round2(end.1),
                "travelMs": round1(plan.travel_ms),
                "strokeMs": round1(plan.hold_ms),
                "human": true,
                "spread": human::clamp_spread(data.spread),
            }),
        );
        self.record_action(id, Some(&data.from))
    }

    /// Play a planned list of raw pointer events, honouring stop requests. If the
    /// run is cut short while the button is down, the button is released first so
    /// it is never left stuck.
    fn play(&mut self, plan: &human::Plan) -> RunResultOf<()> {
        self.driver.check_input(&plan.press_points())?;
        let mut held: Option<(f64, f64)> = None;
        for step in &plan.steps {
            let sent = self.driver.pointer(step.action, step.x, step.y);
            match step.action {
                PointerAction::Down | PointerAction::Drag => held = Some((step.x, step.y)),
                PointerAction::Up => held = None,
                PointerAction::Move => {}
            }
            let paused = match sent {
                Ok(()) => self.sleep(step.wait_ms),
                Err(error) => Err(error.into()),
            };
            if let Err(error) = paused {
                if let Some((x, y)) = held {
                    let _ = self.driver.pointer(PointerAction::Up, x, y);
                }
                return Err(error);
            }
        }
        Ok(())
    }

    fn wait_for(
        &mut self,
        id: &str,
        names: &[String],
        timeout_ms: u64,
        interval_ms: u64,
    ) -> RunResultOf<()> {
        if names.is_empty() {
            return Err(RunError::at(
                Reason::BadStep,
                "waitFor needs at least one target",
                id,
            ));
        }
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let interval = interval_ms.max(50) as f64;
        loop {
            self.check_stop()?;
            self.invalidate();
            for name in names {
                if self.find(name, id, true, true)?.is_some() {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                return Err(RunError::at(
                    Reason::Timeout,
                    format!("none of {names:?} appeared"),
                    id,
                ));
            }
            self.sleep(interval)?;
        }
    }

    fn wait_gone(
        &mut self,
        id: &str,
        name: &str,
        timeout_ms: u64,
        interval_ms: u64,
    ) -> RunResultOf<()> {
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        let interval = interval_ms.max(50) as f64;
        loop {
            self.check_stop()?;
            self.invalidate();
            if self.find(name, id, false, false)?.is_none() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(RunError::at(
                    Reason::Timeout,
                    format!("target \"{name}\" never disappeared"),
                    id,
                ));
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
    ) -> RunResultOf<Option<Ended>> {
        let body = flow.next(&node.id, handle::BODY);
        let cap = data.max_iterations.unwrap_or(match data.mode {
            LoopMode::Count => data.count,
            _ => DEFAULT_LOOP_CAP,
        });
        let mut iterations = 0u32;

        loop {
            self.check_stop()?;
            let proceed = match data.mode {
                LoopMode::Count => iterations < data.count,
                LoopMode::Forever => true,
                LoopMode::While => match &data.when {
                    Some(condition) => self.eval(condition, &node.id)?,
                    None => {
                        return Err(RunError::at(
                            Reason::BadStep,
                            "a while loop needs a condition",
                            &node.id,
                        ))
                    }
                },
            };
            if !proceed || iterations >= cap {
                if proceed && data.mode != LoopMode::Count {
                    self.reporter.log(
                        "warn",
                        "loop_cap",
                        json!({ "nodeId": node.id, "iterations": iterations }),
                    );
                }
                return Ok(None);
            }
            iterations += 1;
            if body.is_some() && self.run_chain(flow, body)? == ChainEnd::Ended {
                return Ok(Some(Ended));
            }
        }
    }

    fn run_retry(
        &mut self,
        flow: &'a Flow,
        node: &'a Node,
        times: u32,
        backoff_ms: u64,
    ) -> RunResultOf<Outcome> {
        let body = flow.next(&node.id, handle::BODY);
        let times = times.max(1);
        let mut last: Option<RunError> = None;

        for attempt in 0..times {
            match self.run_chain(flow, body) {
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
        Err(RunError::at(
            last.reason,
            format!("retry exhausted: {}", last.message),
            &node.id,
        ))
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
        NodeKind::Tap(_) | NodeKind::LongPress(_) | NodeKind::Swipe(_)
            if reason == Reason::NotFound =>
        {
            Some(handle::FAIL)
        }
        NodeKind::WaitFor(_) | NodeKind::WaitGone(_) | NodeKind::WaitStable(_)
            if reason == Reason::Timeout =>
        {
            Some(handle::TIMEOUT)
        }
        _ => None,
    }
}

/// A uniformly random whole number of milliseconds in `min..=max` (a `max`
/// below `min` is treated as `min`; validation reports it as an error).
fn pick_sleep_ms(min: u64, max: u64) -> u64 {
    rand::rng().random_range(min..=max.max(min))
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
                        (Some(x), Some(y))
                            if !matches!((a, b), (VarValue::Str(_), _) | (_, VarValue::Str(_))) =>
                        {
                            x == y
                        }
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
            let (Some(a), Some(b)) = (actual.and_then(as_number), as_number(expected)) else {
                return false;
            };
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
        assert!(compare(
            Some(&s("hello world")),
            CompareOp::Contains,
            &s("lo wo")
        ));
        assert!(compare(Some(&s("abc123")), CompareOp::Match, &s(r"\d+$")));
        assert!(!compare(Some(&s("abc")), CompareOp::Lt, &n(1.0)));
        assert!(compare(
            Some(&VarValue::Bool(true)),
            CompareOp::Eq,
            &VarValue::Bool(true)
        ));
    }
}

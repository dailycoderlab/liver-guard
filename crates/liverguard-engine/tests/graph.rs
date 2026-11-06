//! Graph interpreter behaviour against a scripted screen. Ports the old Python
//! runtime / threshold / jitter suites to the node-graph IR.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use liverguard_engine::testing::{FakeDriver, GraphBuilder, Screen};
use liverguard_engine::{Interpreter, NodeState, Reporter, RunEvent, RunResult, RunState};
use liverguard_ir::node::{handle, *};
use liverguard_ir::{CompareOp, Condition, NodeKind as K, VarValue};

fn run_with(ir: &liverguard_ir::IrDoc, screen: &Screen, driver: &mut FakeDriver) -> (RunResult, Reporter) {
    let stop = Arc::new(AtomicBool::new(false));
    run_stoppable(ir, screen, driver, stop)
}

fn run_stoppable(ir: &liverguard_ir::IrDoc, screen: &Screen, driver: &mut FakeDriver, stop: Arc<AtomicBool>) -> (RunResult, Reporter) {
    let mut interpreter = Interpreter::new(ir, driver, screen.resolver(), Reporter::new("t"), stop);
    let result = interpreter.run();
    (result, interpreter.into_reporter())
}

fn run(ir: &liverguard_ir::IrDoc, screen: &Screen) -> (RunResult, Reporter, FakeDriver) {
    let mut driver = FakeDriver::blank();
    let (result, reporter) = run_with(ir, screen, &mut driver);
    (result, reporter, driver)
}

fn tap(target: &str) -> K {
    K::Tap(TapData { target: target.into(), duration_ms: 60, jitter: Default::default() })
}

fn log(msg: &str) -> K {
    K::Log(LogData { level: "info".into(), msg: msg.into() })
}

fn end() -> K {
    K::End(Default::default())
}

fn messages(reporter: &Reporter) -> Vec<String> {
    reporter
        .lines()
        .iter()
        .filter(|line| line.event == "script_log")
        .filter_map(|line| line.payload.as_ref()?["msg"].as_str().map(str::to_string))
        .collect()
}

fn has_event(reporter: &Reporter, event: &str) -> bool {
    reporter.lines().iter().any(|line| line.event == event)
}

fn wait_for(any: &[&str], timeout_ms: u64) -> K {
    K::WaitFor(WaitForData { any: any.iter().map(|s| s.to_string()).collect(), timeout_ms, interval_ms: 50 })
}

// ------------------------------------------------------------------ basics

#[test]
fn tap_clicks_the_center_of_the_match_and_follows_next() {
    let screen = Screen::default().show("a", 0.95);
    let ir = GraphBuilder::new(&["a"])
        .node("t", tap("a"))
        .node("done", log("after"))
        .wire("start", handle::NEXT, "t")
        .wire("t", handle::NEXT, "done")
        .build();
    let (result, reporter, driver) = run(&ir, &screen);

    assert_eq!(result.state, RunState::Done);
    assert_eq!(driver.taps, vec![(120.0, 110.0, 60)]);
    assert_eq!(messages(&reporter), vec!["after"]);
    assert_eq!(result.step_count, 2);
}

#[test]
fn an_unwired_failure_aborts_with_the_node_id() {
    let screen = Screen::default();
    let ir = GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build();
    let (result, _, driver) = run(&ir, &screen);

    assert_eq!(result.state, RunState::Failed);
    assert_eq!(result.error_code.as_deref(), Some("not_found"));
    assert_eq!(result.error_node.as_deref(), Some("t"));
    assert!(driver.taps.is_empty());
}

#[test]
fn a_wired_fail_handle_recovers() {
    let screen = Screen::default();
    let ir = GraphBuilder::new(&["a"])
        .node("t", tap("a"))
        .node("ok", log("clicked"))
        .node("recover", log("recovered"))
        .wire("start", handle::NEXT, "t")
        .wire("t", handle::NEXT, "ok")
        .wire("t", handle::FAIL, "recover")
        .build();
    let (result, reporter, driver) = run(&ir, &screen);

    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["recovered"]);
    assert!(driver.taps.is_empty());
    assert!(has_event(&reporter, "routed_failure"));
}

#[test]
fn a_match_below_the_threshold_is_not_clicked() {
    let screen = Screen::default().show("a", 0.5);
    let ir = GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build();
    let (result, reporter, driver) = run(&ir, &screen);

    assert_eq!(result.state, RunState::Failed);
    assert!(result.error_msg.unwrap().contains("below threshold"));
    assert!(driver.taps.is_empty());
    assert!(has_event(&reporter, "match_rejected"));
}

#[test]
fn a_score_just_above_the_threshold_is_clicked_without_warning() {
    // The warn line (0.7) sits below the threshold (0.82), so an accepted match never warns.
    let screen = Screen::default().show("a", 0.83);
    let ir = GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build();
    let (result, reporter, _) = run(&ir, &screen);
    assert_eq!(result.state, RunState::Done);
    assert!(!has_event(&reporter, "low_confidence"));
}

#[test]
fn scores_are_aggregated_per_step_and_target() {
    let screen = Screen::default().show("a", 0.9);
    let ir = GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build();
    let (_, reporter, _) = run(&ir, &screen);
    let rows = reporter.summary_rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["stepId"], "t");
    assert_eq!(rows[0]["targetId"], "a");
}

// --------------------------------------------------------------- waiting

#[test]
fn wait_for_polls_until_the_target_appears() {
    let screen = Screen::default().appear_after("a", 2, 0.95);
    let ir = GraphBuilder::new(&["a"])
        .node("w", wait_for(&["a"], 5_000))
        .node("seen", log("seen"))
        .wire("start", handle::NEXT, "w")
        .wire("w", handle::FOUND, "seen")
        .build();
    let (result, reporter, _) = run(&ir, &screen);

    assert_eq!(result.state, RunState::Done);
    assert_eq!(screen.calls("a"), 3);
    assert_eq!(messages(&reporter), vec!["seen"]);
}

#[test]
fn wait_for_routes_a_timeout() {
    let screen = Screen::default();
    let ir = GraphBuilder::new(&["a"])
        .node("w", wait_for(&["a"], 120))
        .node("late", log("gave up"))
        .wire("start", handle::NEXT, "w")
        .wire("w", handle::TIMEOUT, "late")
        .build();
    let (result, reporter, _) = run(&ir, &screen);

    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["gave up"]);
}

#[test]
fn wait_for_without_a_timeout_wire_fails_the_run() {
    let screen = Screen::default();
    let ir = GraphBuilder::new(&["a"]).node("w", wait_for(&["a"], 80)).wire("start", handle::NEXT, "w").build();
    let (result, _, _) = run(&ir, &screen);
    assert_eq!(result.error_code.as_deref(), Some("timeout"));
}

#[test]
fn wait_gone_ignores_the_threshold() {
    // Score 0.3 is far below the threshold, but the control is still "there".
    let screen = Screen::default().show("a", 0.3);
    let ir = GraphBuilder::new(&["a"])
        .node("g", K::WaitGone(WaitGoneData { target: "a".into(), timeout_ms: 100, interval_ms: 50 }))
        .node("gone", log("gone"))
        .node("stuck", log("stuck"))
        .wire("start", handle::NEXT, "g")
        .wire("g", handle::NEXT, "gone")
        .wire("g", handle::TIMEOUT, "stuck")
        .build();
    let (_, reporter, _) = run(&ir, &screen);
    assert_eq!(messages(&reporter), vec!["stuck"]);
}

#[test]
fn wait_gone_succeeds_once_the_target_leaves() {
    let screen = Screen::default();
    let ir = GraphBuilder::new(&["a"])
        .node("g", K::WaitGone(WaitGoneData { target: "a".into(), timeout_ms: 500, interval_ms: 50 }))
        .node("gone", log("gone"))
        .wire("start", handle::NEXT, "g")
        .wire("g", handle::NEXT, "gone")
        .build();
    let (_, reporter, _) = run(&ir, &screen);
    assert_eq!(messages(&reporter), vec!["gone"]);
}

// -------------------------------------------------------------- branching

#[test]
fn if_follows_the_visible_condition() {
    let build = || {
        GraphBuilder::new(&["a"])
            .node("i", K::If(IfData { when: Condition::Visible { targets: vec!["a".into()] } }))
            .node("yes", log("yes"))
            .node("no", log("no"))
            .wire("start", handle::NEXT, "i")
            .wire("i", handle::TRUE, "yes")
            .wire("i", handle::FALSE, "no")
            .build()
    };
    let (_, shown, _) = run(&build(), &Screen::default().show("a", 0.9));
    assert_eq!(messages(&shown), vec!["yes"]);
    let (_, hidden, _) = run(&build(), &Screen::default());
    assert_eq!(messages(&hidden), vec!["no"]);
}

#[test]
fn if_conditions_do_not_enforce_the_threshold() {
    let screen = Screen::default().show("a", 0.2);
    let ir = GraphBuilder::new(&["a"])
        .node("i", K::If(IfData { when: Condition::Missing { target: "a".into() } }))
        .node("yes", log("missing"))
        .node("no", log("present"))
        .wire("start", handle::NEXT, "i")
        .wire("i", handle::TRUE, "yes")
        .wire("i", handle::FALSE, "no")
        .build();
    let (_, reporter, _) = run(&ir, &screen);
    assert_eq!(messages(&reporter), vec!["present"]);
}

#[test]
fn set_var_and_variable_conditions() {
    let ir = GraphBuilder::new(&[])
        .var("level", VarValue::Num(1.0))
        .node("set", K::SetVar(SetVarData { name: "level".into(), value: VarValue::Num(7.0) }))
        .node(
            "i",
            K::If(IfData {
                when: Condition::All {
                    conds: vec![
                        Condition::Var { name: "level".into(), op: CompareOp::Ge, value: VarValue::Num(5.0) },
                        Condition::Not {
                            cond: Box::new(Condition::Var { name: "level".into(), op: CompareOp::Eq, value: VarValue::Num(1.0) }),
                        },
                    ],
                },
            }),
        )
        .node("hi", log("high"))
        .node("lo", log("low"))
        .wire("start", handle::NEXT, "set")
        .wire("set", handle::NEXT, "i")
        .wire("i", handle::TRUE, "hi")
        .wire("i", handle::FALSE, "lo")
        .build();
    let (_, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(messages(&reporter), vec!["high"]);
}

#[test]
fn switch_takes_the_first_visible_case_then_default() {
    let build = || {
        GraphBuilder::new(&["a", "b"])
            .node("s", K::Switch(SwitchData { cases: vec!["a".into(), "b".into()] }))
            .node("ca", log("case a"))
            .node("cb", log("case b"))
            .node("cd", log("default"))
            .wire("start", handle::NEXT, "s")
            .wire("s", "case:a", "ca")
            .wire("s", "case:b", "cb")
            .wire("s", handle::DEFAULT, "cd")
            .build()
    };
    let (_, both, _) = run(&build(), &Screen::default().show("a", 0.9).show("b", 0.9));
    assert_eq!(messages(&both), vec!["case a"]);
    let (_, only_b, _) = run(&build(), &Screen::default().show("b", 0.9));
    assert_eq!(messages(&only_b), vec!["case b"]);
    let (_, none, _) = run(&build(), &Screen::default());
    assert_eq!(messages(&none), vec!["default"]);
}

// ------------------------------------------------------------- containers

#[test]
fn a_count_loop_runs_its_body_then_done() {
    let ir = GraphBuilder::new(&[])
        .node("l", K::Loop(LoopData { mode: Default::default(), count: 3, when: None, max_iterations: None }))
        .node("body", log("tick"))
        .node("after", log("after"))
        .wire("start", handle::NEXT, "l")
        .wire("l", handle::BODY, "body")
        .wire("l", handle::DONE, "after")
        .build();
    let (result, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["tick", "tick", "tick", "after"]);
}

#[test]
fn a_forever_loop_is_bounded_by_max_iterations() {
    let ir = GraphBuilder::new(&[])
        .node("l", K::Loop(LoopData { mode: LoopMode::Forever, count: 1, when: None, max_iterations: Some(4) }))
        .node("body", log("tick"))
        .node("after", log("after"))
        .wire("start", handle::NEXT, "l")
        .wire("l", handle::BODY, "body")
        .wire("l", handle::DONE, "after")
        .build();
    let (_, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(messages(&reporter).len(), 5);
    assert!(has_event(&reporter, "loop_cap"));
}

#[test]
fn a_while_loop_exits_when_the_condition_turns_false() {
    // The target is visible for the first two checks only.
    let screen = Screen::default().show("a", 0.9);
    let ir = GraphBuilder::new(&["a"])
        .node(
            "l",
            K::Loop(LoopData {
                mode: LoopMode::While,
                count: 1,
                when: Some(Condition::Visible { targets: vec!["a".into()] }),
                max_iterations: Some(10),
            }),
        )
        .node("body", log("tick"))
        .node("after", log("after"))
        .wire("start", handle::NEXT, "l")
        .wire("l", handle::BODY, "body")
        .wire("l", handle::DONE, "after")
        .build();
    let hider = screen.clone();
    let sink_screen = screen.clone();
    let seen = Arc::new(Mutex::new(0));
    let counter = seen.clone();
    let sink: liverguard_engine::EventSink = Arc::new(move |event| {
        if let RunEvent::Log { event, .. } = event {
            if event == "script_log" {
                let mut n = counter.lock().unwrap();
                *n += 1;
                if *n == 2 {
                    sink_screen.hide("a");
                }
            }
        }
    });
    let mut driver = FakeDriver::blank();
    let mut reporter = Reporter::new("t");
    reporter.subscribe(sink);
    let stop = Arc::new(AtomicBool::new(false));
    let mut interpreter = Interpreter::new(&ir, &mut driver, hider.resolver(), reporter, stop);
    let result = interpreter.run();
    let reporter = interpreter.into_reporter();

    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["tick", "tick", "after"]);
}

#[test]
fn end_inside_a_loop_body_ends_the_flow() {
    let ir = GraphBuilder::new(&[])
        .node("l", K::Loop(LoopData { mode: Default::default(), count: 5, when: None, max_iterations: None }))
        .node("body", log("tick"))
        .node("stop", end())
        .node("after", log("after"))
        .wire("start", handle::NEXT, "l")
        .wire("l", handle::BODY, "body")
        .wire("body", handle::NEXT, "stop")
        .wire("l", handle::DONE, "after")
        .build();
    let (result, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["tick"]);
}

#[test]
fn retry_succeeds_on_a_later_attempt() {
    // `a` shows up on the third resolve, i.e. the second attempt of a 1-shot wait.
    let screen = Screen::default().appear_after("a", 1, 0.95);
    let ir = GraphBuilder::new(&["a"])
        .node("r", K::Retry(RetryData { times: 3, backoff_ms: 10 }))
        .node("try", tap("a"))
        .node("ok", log("done"))
        .node("bad", log("failed"))
        .wire("start", handle::NEXT, "r")
        .wire("r", handle::BODY, "try")
        .wire("r", handle::DONE, "ok")
        .wire("r", handle::FAILED, "bad")
        .build();
    let (result, reporter, driver) = run(&ir, &screen);
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["done"]);
    assert_eq!(driver.taps.len(), 1);
    assert!(has_event(&reporter, "retry"));
}

#[test]
fn retry_exhaustion_takes_the_failed_handle() {
    let ir = GraphBuilder::new(&["a"])
        .node("r", K::Retry(RetryData { times: 2, backoff_ms: 10 }))
        .node("try", tap("a"))
        .node("bad", log("failed"))
        .wire("start", handle::NEXT, "r")
        .wire("r", handle::BODY, "try")
        .wire("r", handle::FAILED, "bad")
        .build();
    let (result, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["failed"]);
}

#[test]
fn retry_exhaustion_without_a_failed_wire_aborts() {
    let ir = GraphBuilder::new(&["a"])
        .node("r", K::Retry(RetryData { times: 2, backoff_ms: 10 }))
        .node("try", tap("a"))
        .wire("start", handle::NEXT, "r")
        .wire("r", handle::BODY, "try")
        .build();
    let (result, _, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Failed);
    assert!(result.error_msg.unwrap().starts_with("retry exhausted"));
}

// ------------------------------------------------------------------ calls

#[test]
fn end_inside_a_called_flow_only_returns_to_the_caller() {
    let ir = GraphBuilder::new(&[])
        .node("c", K::Call(CallData { flow: "sub".into() }))
        .node("after", log("after"))
        .wire("start", handle::NEXT, "c")
        .wire("c", handle::NEXT, "after")
        .flow("sub")
        .node("inner", log("inner"))
        .node("e", end())
        .node("never", log("never"))
        .wire("start", handle::NEXT, "inner")
        .wire("inner", handle::NEXT, "e")
        .build();
    let (result, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["inner", "after"]);
}

#[test]
fn runaway_recursion_is_cut_off() {
    let ir = GraphBuilder::new(&[])
        .node("c", K::Call(CallData { flow: "main".into() }))
        .wire("start", handle::NEXT, "c")
        .build_unchecked();
    let (result, _, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Failed);
    assert_eq!(result.error_code.as_deref(), Some("call_depth"));
}

// ----------------------------------------------------------------- guards

#[test]
fn visit_limits_stop_a_cycle() {
    let ir = GraphBuilder::new(&[])
        .node_with_visits("spin", log("spin"), 3)
        .wire("start", handle::NEXT, "spin")
        .wire("spin", handle::NEXT, "spin")
        .build();
    let (result, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(result.error_code.as_deref(), Some("visit_limit"));
    assert_eq!(messages(&reporter).len(), 3);
}

#[test]
fn a_stop_request_ends_the_run_as_stopped() {
    let ir = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let stop = Arc::new(AtomicBool::new(true));
    let mut driver = FakeDriver::blank();
    let (result, _) = run_stoppable(&ir, &Screen::default(), &mut driver, stop);
    assert_eq!(result.state, RunState::Stopped);
}

#[test]
fn a_stop_during_a_long_sleep_is_honoured_promptly() {
    let ir = GraphBuilder::new(&[])
        .node("nap", K::Sleep(SleepData { ms: 60_000 }))
        .wire("start", handle::NEXT, "nap")
        .build();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        flag.store(true, Ordering::Relaxed);
    });
    let mut driver = FakeDriver::blank();
    let started = std::time::Instant::now();
    let (result, _) = run_stoppable(&ir, &Screen::default(), &mut driver, stop);
    handle.join().unwrap();
    assert_eq!(result.state, RunState::Stopped);
    assert!(started.elapsed().as_secs() < 5);
}

#[test]
fn the_run_deadline_cuts_a_long_sleep() {
    let ir = GraphBuilder::new(&[])
        .max_duration(1_000)
        .node("nap", K::Sleep(SleepData { ms: 600_000 }))
        .wire("start", handle::NEXT, "nap")
        .build();
    let started = std::time::Instant::now();
    let (result, _, _) = run(&ir, &Screen::default());
    assert_eq!(result.error_code.as_deref(), Some("timeout"));
    assert!(started.elapsed().as_secs() < 10);
}

#[test]
fn repeated_actions_that_change_nothing_trip_the_stagnation_guard() {
    let screen = Screen::default().show("a", 0.95);
    let ir = GraphBuilder::new(&["a"])
        .node("l", K::Loop(LoopData { mode: LoopMode::Forever, count: 1, when: None, max_iterations: Some(50) }))
        .node("t", tap("a"))
        .wire("start", handle::NEXT, "l")
        .wire("l", handle::BODY, "t")
        .build();
    let (result, _, driver) = run(&ir, &screen);
    assert_eq!(result.error_code.as_deref(), Some("stagnation"));
    assert_eq!(driver.taps.len(), 11);
}

#[test]
fn a_stalled_capture_aborts_with_an_actionable_reason() {
    let screen = Screen::default().show("a", 0.95);
    let ir = GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build();
    let mut driver = FakeDriver::blank();
    driver.capture_ms = 4_000.0;
    let (result, reporter) = run_with(&ir, &screen, &mut driver);
    assert_eq!(result.error_code.as_deref(), Some("capture_stalled"));
    assert!(has_event(&reporter, "capture_stalled"));
    assert!(driver.taps.is_empty());
}

#[test]
fn an_unknown_target_is_reported_not_panicked() {
    let ir = GraphBuilder::new(&[]).node("t", tap("ghost")).wire("start", handle::NEXT, "t").build_unchecked();
    let (result, _, _) = run(&ir, &Screen::default());
    assert_eq!(result.error_code.as_deref(), Some("unknown_target"));
}

// ----------------------------------------------------------------- inputs

#[test]
fn key_text_and_swipe_reach_the_driver() {
    let screen = Screen::default().show("a", 0.95);
    let ir = GraphBuilder::new(&["a"])
        .node("k", K::Key(KeyData { key: "enter".into() }))
        .node("x", K::Text(TextData { text: "hello".into() }))
        .node("s", K::Swipe(SwipeData { from: "a".into(), to: None, vector: Some([0.0, -0.1]), duration_ms: 300 }))
        .wire("start", handle::NEXT, "k")
        .wire("k", handle::NEXT, "x")
        .wire("x", handle::NEXT, "s")
        .build();
    let (result, _, driver) = run(&ir, &screen);
    assert_eq!(result.state, RunState::Done);
    assert_eq!(driver.keys, vec!["enter"]);
    assert_eq!(driver.texts, vec!["hello"]);
    // Surface is 800x600, so -0.1 of the height is -60pt from the anchor (120, 110).
    assert_eq!(driver.swipes, vec![(120.0, 110.0, 120.0, 50.0)]);
}

fn jittered(offset_ratio: f64) -> liverguard_ir::IrDoc {
    GraphBuilder::new(&["a"])
        .node(
            "t",
            K::Tap(TapData {
                target: "a".into(),
                duration_ms: 60,
                jitter: Jitter { delay_ms: 0.0, delay_range_ms: None, offset_ratio },
            }),
        )
        .wire("start", handle::NEXT, "t")
        .build()
}

#[test]
fn jitter_stays_inside_the_matched_box() {
    // Match box is x 100..140, y 100..120, anchor (120, 110).
    for _ in 0..30 {
        let (result, _, driver) = run(&jittered(0.2), &Screen::default().show("a", 0.95));
        assert_eq!(result.state, RunState::Done);
        let (x, y, _) = driver.taps[0];
        assert!((120.0 - 0.2 * 40.0 - 1e-9..=120.0 + 0.2 * 40.0 + 1e-9).contains(&x), "x={x}");
        assert!((110.0 - 0.2 * 20.0 - 1e-9..=110.0 + 0.2 * 20.0 + 1e-9).contains(&y), "y={y}");
    }
}

#[test]
fn an_oversized_offset_ratio_is_capped_and_reported() {
    let (_, reporter, driver) = run(&jittered(5.0), &Screen::default().show("a", 0.95));
    let (x, y, _) = driver.taps[0];
    assert!((100.0..=140.0).contains(&x) && (100.0..=120.0).contains(&y));
    assert!(has_event(&reporter, "offset_clamped"));
}

#[test]
fn zero_jitter_taps_exactly_on_the_anchor() {
    for _ in 0..5 {
        let (_, _, driver) = run(&jittered(0.0), &Screen::default().show("a", 0.95));
        assert_eq!(driver.taps[0].0, 120.0);
        assert_eq!(driver.taps[0].1, 110.0);
    }
}

#[test]
fn a_fixed_delay_is_waited_before_the_tap() {
    let ir = GraphBuilder::new(&["a"])
        .node(
            "t",
            K::Tap(TapData {
                target: "a".into(),
                duration_ms: 60,
                jitter: Jitter { delay_ms: 150.0, delay_range_ms: Some([0.0, 0.0]), offset_ratio: 0.0 },
            }),
        )
        .wire("start", handle::NEXT, "t")
        .build();
    let started = std::time::Instant::now();
    let (_, reporter, _) = run(&ir, &Screen::default().show("a", 0.95));
    assert!(started.elapsed().as_millis() >= 150);
    let tap_line = reporter.lines().iter().find(|l| l.event == "tap").unwrap();
    assert_eq!(tap_line.payload.as_ref().unwrap()["delayMs"], 150);
}

// ------------------------------------------------------------------ events

#[test]
fn node_events_drive_the_canvas_highlight() {
    let screen = Screen::default().show("a", 0.95);
    let ir = GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build();

    let events: Arc<Mutex<Vec<(String, NodeState)>>> = Arc::default();
    let collected = events.clone();
    let mut reporter = Reporter::new("t");
    reporter.subscribe(Arc::new(move |event| {
        if let RunEvent::Node { node_id, state, .. } = event {
            collected.lock().unwrap().push((node_id.clone(), *state));
        }
    }));
    let mut driver = FakeDriver::blank();
    let stop = Arc::new(AtomicBool::new(false));
    let mut interpreter = Interpreter::new(&ir, &mut driver, screen.resolver(), reporter, stop);
    interpreter.run();

    let seen = events.lock().unwrap().clone();
    assert_eq!(seen.first(), Some(&("t".to_string(), NodeState::Running)));
    assert_eq!(seen.last(), Some(&("t".to_string(), NodeState::Ok)));
}

#[test]
fn a_panicking_subscriber_never_aborts_the_run() {
    let ir = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let mut reporter = Reporter::new("t");
    reporter.subscribe(Arc::new(|_| panic!("ui exploded")));
    let mut driver = FakeDriver::blank();
    let stop = Arc::new(AtomicBool::new(false));
    let screen = Screen::default();
    let mut interpreter = Interpreter::new(&ir, &mut driver, screen.resolver(), reporter, stop);
    let result = interpreter.run();
    let reporter = interpreter.into_reporter();
    assert_eq!(result.state, RunState::Done);
    assert!(has_event(&reporter, "log_subscriber_failed"));
}

#[test]
fn the_jsonl_log_is_one_json_object_per_line() {
    let ir = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let (_, reporter, _) = run(&ir, &Screen::default());
    for line in reporter.jsonl().lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        assert!(value["event"].is_string());
    }
}

#[test]
fn a_node_wired_to_nothing_just_ends_the_run() {
    let ir = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let (result, _, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Done);
}

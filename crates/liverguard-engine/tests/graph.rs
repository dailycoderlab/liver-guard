//! Graph interpreter behaviour against a scripted screen. Ports the old Python
//! runtime / threshold / random-point suites to the node-graph IR.

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
    K::Tap(TapData { target: target.into(), duration_ms: 60, human: false, spread: 1.0 })
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
    // The warn line (0.7) sits below the threshold (0.85), so an accepted match never warns.
    let screen = Screen::default().show("a", 0.86);
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
fn end_inside_a_loop_body_ends_the_run() {
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
        .node("nap", K::Sleep(SleepData { min_ms: 60_000, max_ms: 60_000 }))
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

fn slept_ms(min_ms: u64, max_ms: u64) -> u64 {
    let ir = GraphBuilder::new(&[])
        .node("nap", K::Sleep(SleepData { min_ms, max_ms }))
        .wire("start", handle::NEXT, "nap")
        .build();
    let (result, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(result.state, RunState::Done);
    let line = reporter.lines().iter().find(|l| l.event == "sleep").unwrap();
    line.payload.as_ref().unwrap()["ms"].as_u64().unwrap()
}

#[test]
fn a_sleep_range_picks_whole_milliseconds_inside_it() {
    let picks: Vec<u64> = (0..40).map(|_| slept_ms(3, 8)).collect();
    assert!(picks.iter().all(|ms| (3..=8).contains(ms)), "{picks:?}");
    // 40 draws from 6 values: all landing on one value is (1/6)^39, i.e. the draw is not random.
    assert!(picks.iter().any(|ms| *ms != picks[0]), "{picks:?}");
}

#[test]
fn equal_sleep_bounds_wait_exactly_that_long() {
    assert_eq!(slept_ms(7, 7), 7);
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
        .node("s", K::Swipe(SwipeData { from: "a".into(), to: None, vector: Some([0.0, -0.1]), duration_ms: 300, human: false, spread: 1.0 }))
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

fn click_at(human: bool) -> liverguard_ir::IrDoc {
    click_with_spread(human, 1.0)
}

fn click_with_spread(human: bool, spread: f64) -> liverguard_ir::IrDoc {
    GraphBuilder::new(&["a"])
        .node("t", K::Tap(TapData { target: "a".into(), duration_ms: 60, human, spread }))
        .wire("start", handle::NEXT, "t")
        .build_unchecked() // an out-of-range spread is invalid, and one test feeds the engine one anyway
}

fn swipe_doc(to: Option<&str>, duration_ms: u32, human: bool) -> liverguard_ir::IrDoc {
    GraphBuilder::new(&["a", "b"])
        .node(
            "s",
            K::Swipe(SwipeData {
                from: "a".into(),
                to: to.map(str::to_string),
                vector: Some([0.0, -0.1]),
                duration_ms,
                human,
                spread: 1.0,
            }),
        )
        .wire("start", handle::NEXT, "s")
        .build()
}

use liverguard_engine::PointerAction as P;

#[test]
fn without_human_the_click_lands_exactly_on_the_anchor_through_tap() {
    for _ in 0..5 {
        let (_, _, driver) = run(&click_at(false), &Screen::default().show("a", 0.95));
        assert_eq!(driver.taps[0].0, 120.0);
        assert_eq!(driver.taps[0].1, 110.0);
        assert!(driver.pointer_events.is_empty() && driver.input_checks.is_empty());
    }
}

#[test]
fn a_human_click_goes_through_raw_pointer_events_inside_the_matched_box() {
    // Match box is x 100..140, y 100..120, anchor (120, 110).
    let mut xs = Vec::new();
    for _ in 0..8 {
        let (result, reporter, driver) = run(&click_at(true), &Screen::default().show("a", 0.95));
        assert_eq!(result.state, RunState::Done);
        assert!(driver.taps.is_empty(), "human clicks must not use the exact tap");

        let kinds: Vec<P> = driver.pointer_events.iter().map(|e| e.0).collect();
        assert_eq!(kinds, vec![P::Move, P::Down, P::Up], "unknown pointer start: it just appears");
        let (_, x, y) = driver.pointer_events[1];
        assert!((100.0..=140.0).contains(&x) && (100.0..=120.0).contains(&y), "({x}, {y})");
        assert_eq!((driver.pointer_events[2].1, driver.pointer_events[2].2), (x, y));
        // The input check saw the pressed points once, before anything was sent.
        assert_eq!(driver.input_checks, vec![vec![(x, y), (x, y)]]);
        xs.push(x);

        let tap = reporter.lines().iter().find(|l| l.event == "tap").unwrap().payload.clone().unwrap();
        assert_eq!(tap["human"], true);
        let hold = tap["holdMs"].as_f64().unwrap();
        assert!((36.0..=108.0).contains(&hold), "{hold}");
        assert_eq!(tap["overshoot"], false);
    }
    assert!(xs.iter().any(|x| *x != xs[0]), "{xs:?}");
}

#[test]
fn the_spread_setting_widens_or_narrows_where_a_human_click_lands() {
    // Box is x 100..140 around anchor x = 120; compare the scatter at two settings.
    let spread_of = |spread: f64| {
        let xs: Vec<f64> = (0..40)
            .map(|_| {
                let (result, _, driver) = run(&click_with_spread(true, spread), &Screen::default().show("a", 0.95));
                assert_eq!(result.state, RunState::Done);
                driver.pointer_events[1].1
            })
            .collect();
        let mean = xs.iter().sum::<f64>() / xs.len() as f64;
        (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / xs.len() as f64).sqrt()
    };
    let (tight, wide) = (spread_of(0.4), spread_of(2.0));
    assert!(wide > tight * 1.8, "tight {tight}, wide {wide}");
}

#[test]
fn the_log_reports_the_spread_and_an_unusable_value_is_pulled_into_range() {
    let (_, reporter, _) = run(&click_with_spread(true, 1.4), &Screen::default().show("a", 0.95));
    let tap = reporter.lines().iter().find(|l| l.event == "tap").unwrap().payload.clone().unwrap();
    assert_eq!(tap["spread"], 1.4);

    // Not reachable through validation, but the engine must not misbehave on a bad value.
    let (result, reporter, driver) = run(&click_with_spread(true, f64::NAN), &Screen::default().show("a", 0.95));
    assert_eq!(result.state, RunState::Done);
    let (_, x, y) = driver.pointer_events[1];
    assert!((100.0..=140.0).contains(&x) && (100.0..=120.0).contains(&y));
    let tap = reporter.lines().iter().find(|l| l.event == "tap").unwrap().payload.clone().unwrap();
    assert_eq!(tap["spread"], 1.0);
}

#[test]
fn a_human_click_travels_along_a_curve_from_where_the_pointer_is() {
    let mut driver = FakeDriver::blank();
    driver.pointer_at = Some((700.0, 500.0));
    let (result, _) = run_with(&click_at(true), &Screen::default().show("a", 0.95), &mut driver);
    assert_eq!(result.state, RunState::Done);

    let events = &driver.pointer_events;
    assert!(events.len() > 20, "{}", events.len());
    assert_eq!(events.iter().filter(|e| e.0 == P::Move).count(), events.len() - 2);
    let (_, x, y) = events[events.len() - 2];
    let (_, last_move_x, last_move_y) = events[events.len() - 3];
    assert_eq!((last_move_x, last_move_y), (x, y), "the trip ends exactly on the press point");
    // It starts near where it was and never teleports: consecutive hops stay short.
    let (_, first_x, first_y) = events[0];
    assert!(((first_x - 700.0).powi(2) + (first_y - 500.0).powi(2)).sqrt() < 30.0);
    for pair in events[..events.len() - 2].windows(2) {
        let hop = ((pair[1].1 - pair[0].1).powi(2) + (pair[1].2 - pair[0].2).powi(2)).sqrt();
        assert!(hop < 120.0, "jumped {hop}px");
    }
}

#[test]
fn a_human_swipe_between_targets_drags_from_box_to_box() {
    let screen = Screen::default().show("a", 0.95).show("b", 0.95);
    let (result, reporter, driver) = run(&swipe_doc(Some("b"), 200, true), &screen);
    assert_eq!(result.state, RunState::Done);
    assert!(driver.swipes.is_empty(), "human swipes must not use the exact swipe");

    let kinds: Vec<P> = driver.pointer_events.iter().map(|e| e.0).collect();
    assert_eq!(kinds.first(), Some(&P::Move));
    assert_eq!(kinds.last(), Some(&P::Up));
    assert_eq!(kinds.iter().filter(|k| **k == P::Down).count(), 1);
    assert!(kinds.iter().filter(|k| **k == P::Drag).count() > 10);
    // Every pressed point is covered by the input check.
    assert_eq!(driver.input_checks.len(), 1);
    assert_eq!(driver.input_checks[0].len(), kinds.iter().filter(|k| **k != P::Move).count());

    let swipe = reporter.lines().iter().find(|l| l.event == "swipe").unwrap().payload.clone().unwrap();
    assert_eq!(swipe["human"], true);
}

#[test]
fn a_human_swipe_by_vector_jitters_its_end_and_stays_on_the_surface() {
    let mut ends = Vec::new();
    for _ in 0..6 {
        let (result, _, driver) = run(&swipe_doc(None, 120, true), &Screen::default().show("a", 0.95));
        assert_eq!(result.state, RunState::Done);
        let (action, x, y) = *driver.pointer_events.last().unwrap();
        assert_eq!(action, P::Up);
        assert!((0.0..=800.0).contains(&x) && (0.0..=600.0).contains(&y), "({x}, {y})");
        // Nominally 60 pt up from the start; 3% of 60 pt is small, so it stays close.
        assert!(y < 110.0 + 20.0 && y > -20.0, "{y}");
        ends.push((x, y));
    }
    assert!(ends.iter().any(|e| *e != ends[0]), "{ends:?}");
}

#[test]
fn a_failure_while_the_button_is_held_still_releases_it() {
    let mut driver = FakeDriver::blank();
    driver.fail_pointer_on = Some(P::Drag);
    let (result, _) = run_with(&swipe_doc(None, 120, true), &Screen::default().show("a", 0.95), &mut driver);
    assert_ne!(result.state, RunState::Done);
    let kinds: Vec<P> = driver.pointer_events.iter().map(|e| e.0).collect();
    assert!(kinds.contains(&P::Down));
    assert_eq!(kinds.last(), Some(&P::Up), "{kinds:?}");
}

#[test]
fn stopping_in_the_middle_of_a_human_drag_releases_the_button() {
    let stop = Arc::new(AtomicBool::new(false));
    let trigger = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(250));
        trigger.store(true, Ordering::Relaxed);
    });
    let mut driver = FakeDriver::blank();
    let started = std::time::Instant::now();
    let (result, _) = run_stoppable(&swipe_doc(None, 3000, true), &Screen::default().show("a", 0.95), &mut driver, stop);
    assert_eq!(result.state, RunState::Stopped);
    assert!(started.elapsed().as_millis() < 1500, "a stop should cut the stroke short");
    let kinds: Vec<P> = driver.pointer_events.iter().map(|e| e.0).collect();
    assert!(kinds.contains(&P::Down) && kinds.contains(&P::Drag));
    assert_eq!(kinds.last(), Some(&P::Up), "the button must not stay down");
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

// ------------------------------------------------------------------ rounds

fn rounds_started(reporter: &Reporter) -> usize {
    reporter.lines().iter().filter(|line| line.event == "round_start").count()
}

fn run_rounds(ir: &liverguard_ir::IrDoc, screen: &Screen, rounds: u32) -> (RunResult, Reporter, FakeDriver) {
    let mut driver = FakeDriver::blank();
    let mut interpreter = Interpreter::new(ir, &mut driver, screen.resolver(), Reporter::new("t"), Arc::new(AtomicBool::new(false)));
    interpreter.set_rounds(rounds);
    let result = interpreter.run();
    (result, interpreter.into_reporter(), driver)
}

#[test]
fn several_rounds_walk_the_graph_that_many_times() {
    let screen = Screen::default().show("a", 0.95);
    let ir = GraphBuilder::new(&["a"])
        .node("t", tap("a"))
        .node("done", log("tick"))
        .wire("start", handle::NEXT, "t")
        .wire("t", handle::NEXT, "done")
        .build();
    let (result, reporter, driver) = run_rounds(&ir, &screen, 3);

    assert_eq!(result.state, RunState::Done);
    assert_eq!(driver.taps.len(), 3);
    assert_eq!(messages(&reporter), vec!["tick", "tick", "tick"]);
    assert_eq!(rounds_started(&reporter), 3);
    assert_eq!(result.step_count, 6);
}

#[test]
fn one_round_logs_no_round_markers_and_zero_counts_as_one() {
    let ir = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let (_, reporter, _) = run(&ir, &Screen::default());
    assert_eq!(rounds_started(&reporter), 0);

    let (result, reporter, _) = run_rounds(&ir, &Screen::default(), 0);
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["a"]);
}

#[test]
fn end_finishes_only_the_current_round() {
    let ir = GraphBuilder::new(&[])
        .node("a", log("a"))
        .node("stop", end())
        .node("never", log("never"))
        .wire("start", handle::NEXT, "a")
        .wire("a", handle::NEXT, "stop")
        .build();
    let (result, reporter, _) = run_rounds(&ir, &Screen::default(), 2);
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["a", "a"]);
}

#[test]
fn every_round_starts_with_no_variables_and_fresh_visit_counts() {
    // Variables start unset. Round one sets `level` to 7; if that carried over into round two
    // the `If` would take the "stale" branch instead of setting it again.
    let ir = GraphBuilder::new(&[])
        .node("i", K::If(IfData { when: Condition::Var { name: "level".into(), op: CompareOp::Eq, value: VarValue::Num(7.0) } }))
        .node("stale", log("stale"))
        .node("set", K::SetVar(SetVarData { name: "level".into(), value: VarValue::Num(7.0) }))
        // Allowed once per round: a second round would trip it if visits carried over.
        .node_with_visits("first", log("first"), 1)
        .wire("start", handle::NEXT, "i")
        .wire("i", handle::TRUE, "stale")
        .wire("i", handle::FALSE, "set")
        .wire("set", handle::NEXT, "first")
        .build();
    let (result, reporter, _) = run_rounds(&ir, &Screen::default(), 2);
    assert_eq!(result.state, RunState::Done);
    assert_eq!(messages(&reporter), vec!["first", "first"]);
}

#[test]
fn an_unset_variable_is_never_equal_and_never_ordered() {
    let check = |op: CompareOp| {
        let ir = GraphBuilder::new(&[])
            .node("i", K::If(IfData { when: Condition::Var { name: "ghost".into(), op, value: VarValue::Num(0.0) } }))
            .node("yes", log("yes"))
            .node("no", log("no"))
            .wire("start", handle::NEXT, "i")
            .wire("i", handle::TRUE, "yes")
            .wire("i", handle::FALSE, "no")
            .build();
        let (_, reporter, _) = run(&ir, &Screen::default());
        messages(&reporter)
    };
    assert_eq!(check(CompareOp::Eq), vec!["no"]);
    assert_eq!(check(CompareOp::Ne), vec!["yes"]);
    assert_eq!(check(CompareOp::Ge), vec!["no"]);
    assert_eq!(check(CompareOp::Lt), vec!["no"]);
}

#[test]
fn a_failure_in_any_round_aborts_the_remaining_rounds() {
    // The target disappears after round one, so the second tap has nothing to hit.
    let screen = Screen::default().show("a", 0.95);
    let hider = screen.clone();
    let ir = GraphBuilder::new(&["a"])
        .node("t", tap("a"))
        .node("mark", log("after tap"))
        .wire("start", handle::NEXT, "t")
        .wire("t", handle::NEXT, "mark")
        .build();
    let sink: liverguard_engine::EventSink = Arc::new(move |event| {
        if let RunEvent::Log { event, .. } = event {
            if event == "script_log" {
                hider.hide("a");
            }
        }
    });
    let mut driver = FakeDriver::blank();
    let mut reporter = Reporter::new("t");
    reporter.subscribe(sink);
    let mut interpreter = Interpreter::new(&ir, &mut driver, screen.resolver(), reporter, Arc::new(AtomicBool::new(false)));
    interpreter.set_rounds(3);
    let result = interpreter.run();
    let reporter = interpreter.into_reporter();

    assert_eq!(result.state, RunState::Failed);
    assert_eq!(result.error_node.as_deref(), Some("t"));
    assert_eq!(rounds_started(&reporter), 2);
    assert_eq!(driver.taps.len(), 1);
}

#[test]
fn stopping_between_rounds_stops_the_run() {
    let ir = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let sink: liverguard_engine::EventSink = Arc::new(move |event| {
        if let RunEvent::Log { event, .. } = event {
            if event == "round_start" {
                flag.store(true, Ordering::Relaxed);
            }
        }
    });
    let mut driver = FakeDriver::blank();
    let mut reporter = Reporter::new("t");
    reporter.subscribe(sink);
    let mut interpreter = Interpreter::new(&ir, &mut driver, Screen::default().resolver(), reporter, stop);
    interpreter.set_rounds(5);
    let result = interpreter.run();
    assert_eq!(result.state, RunState::Stopped);
}

// ------------------------------------------------------------------ alerts

use liverguard_engine::alert::AlertSender;
use liverguard_notify::{Message, NotifyConfig, NotifyError};

/// Alerts are on exactly when a webhook URL is set, so "disabled" is an empty URL.
fn alert_config(enabled: bool) -> NotifyConfig {
    NotifyConfig { url: if enabled { "https://example.com/hook".into() } else { String::new() }, ..Default::default() }
}

/// A sender that records what it was asked to send (and optionally fails).
fn recording_sender(fail: bool) -> (AlertSender, Arc<Mutex<Vec<Message>>>) {
    let sent = Arc::new(Mutex::new(Vec::new()));
    let log = sent.clone();
    let sender: AlertSender = Arc::new(move |_config, message| {
        log.lock().unwrap().push(message.clone());
        if fail {
            Err(NotifyError::Network("连接失败".into()))
        } else {
            Ok(())
        }
    });
    (sender, sent)
}

fn run_alerting(ir: &liverguard_ir::IrDoc, screen: &Screen, rounds: u32, config: NotifyConfig, sender: AlertSender) -> (RunResult, Reporter) {
    let mut driver = FakeDriver::blank();
    let mut interpreter = Interpreter::new(ir, &mut driver, screen.resolver(), Reporter::new("t"), Arc::new(AtomicBool::new(false)));
    interpreter.set_rounds(rounds);
    interpreter.set_alert_with(config, sender);
    let result = interpreter.run();
    (result, interpreter.into_reporter())
}

fn failing_tap_graph() -> liverguard_ir::IrDoc {
    GraphBuilder::new(&["a"]).node("t", tap("a")).wire("start", handle::NEXT, "t").build()
}

#[test]
fn an_unhandled_failure_sends_one_alert_naming_the_node_and_reason() {
    let (sender, sent) = recording_sender(false);
    let (result, reporter) = run_alerting(&failing_tap_graph(), &Screen::default(), 1, alert_config(true), sender);

    assert_eq!(result.state, RunState::Failed);
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    let text = &sent[0].text;
    assert!(text.contains("脚本运行失败"), "{text}");
    assert!(text.contains("脚本：test"), "{text}");
    assert!(text.contains("节点：「t」（tap）"), "{text}");
    assert!(text.contains("not_found"), "{text}");
    assert!(!text.contains("第 1/1 轮"), "{text}");
    assert_eq!(sent[0].details["errorNode"], "t");
    assert_eq!(sent[0].details["errorCode"], "not_found");
    assert!(has_event(&reporter, "alert_sent"));
}

#[test]
fn a_failure_the_script_routes_stays_silent() {
    let screen = Screen::default();
    let ir = GraphBuilder::new(&["a"])
        .node("t", tap("a"))
        .node("recover", log("recovered"))
        .wire("start", handle::NEXT, "t")
        .wire("t", handle::FAIL, "recover")
        .build();
    let (sender, sent) = recording_sender(false);
    let (result, _) = run_alerting(&ir, &screen, 1, alert_config(true), sender);

    assert_eq!(result.state, RunState::Done);
    assert!(sent.lock().unwrap().is_empty());
}

#[test]
fn success_a_manual_stop_and_a_disabled_alert_send_nothing() {
    let ok = GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build();
    let (sender, sent) = recording_sender(false);
    run_alerting(&ok, &Screen::default(), 1, alert_config(true), sender);
    assert!(sent.lock().unwrap().is_empty());

    // Disabled: the failing run is reported as failed but nobody is told.
    let (sender, sent) = recording_sender(false);
    let (result, reporter) = run_alerting(&failing_tap_graph(), &Screen::default(), 1, alert_config(false), sender);
    assert_eq!(result.state, RunState::Failed);
    assert!(sent.lock().unwrap().is_empty());
    assert!(!has_event(&reporter, "alert_sent"));

    // Stopped by the user: not a failure.
    let stop = Arc::new(AtomicBool::new(true));
    let (sender, sent) = recording_sender(false);
    let mut driver = FakeDriver::blank();
    let mut interpreter = Interpreter::new(&ok, &mut driver, Screen::default().resolver(), Reporter::new("t"), stop);
    interpreter.set_alert_with(alert_config(true), sender);
    assert_eq!(interpreter.run().state, RunState::Stopped);
    assert!(sent.lock().unwrap().is_empty());
}

fn two_logs() -> liverguard_ir::IrDoc {
    GraphBuilder::new(&[]).node("a", log("a")).wire("start", handle::NEXT, "a").build()
}

#[test]
fn a_loop_that_finishes_every_round_sends_one_done_notice() {
    let (sender, sent) = recording_sender(false);
    let (result, reporter) = run_alerting(&two_logs(), &Screen::default(), 3, alert_config(true), sender);

    assert_eq!(result.state, RunState::Done);
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "one notice for the whole loop, not one per round");
    assert_eq!(sent[0].event, "loop_done");
    assert!(sent[0].text.contains("循环执行完成"), "{}", sent[0].text);
    assert!(sent[0].text.contains("脚本：test"), "{}", sent[0].text);
    assert!(sent[0].text.contains("共 3 轮全部成功"), "{}", sent[0].text);
    assert_eq!(sent[0].details["rounds"], 3);
    assert!(has_event(&reporter, "alert_sent"));
}

#[test]
fn the_done_notice_never_fires_for_a_single_run_or_without_a_webhook() {
    let (sender, sent) = recording_sender(false);
    run_alerting(&two_logs(), &Screen::default(), 1, alert_config(true), sender);
    assert!(sent.lock().unwrap().is_empty(), "one round is not a loop");

    let (sender, sent) = recording_sender(false);
    run_alerting(&two_logs(), &Screen::default(), 3, alert_config(false), sender);
    assert!(sent.lock().unwrap().is_empty(), "no webhook configured");
}

#[test]
fn a_stopped_loop_and_a_failed_loop_do_not_send_the_done_notice() {
    let stop = Arc::new(AtomicBool::new(true));
    let (sender, sent) = recording_sender(false);
    let mut driver = FakeDriver::blank();
    let ir = two_logs();
    let mut interpreter = Interpreter::new(&ir, &mut driver, Screen::default().resolver(), Reporter::new("t"), stop);
    interpreter.set_rounds(3);
    interpreter.set_alert_with(alert_config(true), sender);
    assert_eq!(interpreter.run().state, RunState::Stopped);
    assert!(sent.lock().unwrap().is_empty());

    let (sender, sent) = recording_sender(false);
    run_alerting(&failing_tap_graph(), &Screen::default(), 3, alert_config(true), sender);
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].event, "run_failed");
}

#[test]
fn a_failed_done_notice_is_logged_and_the_loop_still_counts_as_done() {
    let (sender, _) = recording_sender(true);
    let (result, reporter) = run_alerting(&two_logs(), &Screen::default(), 2, alert_config(true), sender);
    assert_eq!(result.state, RunState::Done);
    assert!(has_event(&reporter, "alert_failed"));
}

#[test]
fn a_failure_in_a_later_round_stops_the_loop_and_says_how_far_it_got() {
    // The target is on screen for round one only; round two cannot tap it.
    let screen = Screen::default().show("a", 0.95);
    let hider = screen.clone();
    let ir = GraphBuilder::new(&["a"])
        .node("t", tap("a"))
        .node("mark", log("after tap"))
        .wire("start", handle::NEXT, "t")
        .wire("t", handle::NEXT, "mark")
        .build();
    let (sender, sent) = recording_sender(false);
    let sink: liverguard_engine::EventSink = Arc::new(move |event| {
        if let RunEvent::Log { event, .. } = event {
            if event == "script_log" {
                hider.hide("a");
            }
        }
    });
    let mut driver = FakeDriver::blank();
    let mut reporter = Reporter::new("t");
    reporter.subscribe(sink);
    let mut interpreter = Interpreter::new(&ir, &mut driver, screen.resolver(), reporter, Arc::new(AtomicBool::new(false)));
    interpreter.set_rounds(3);
    interpreter.set_alert_with(alert_config(true), sender);
    let result = interpreter.run();
    let reporter = interpreter.into_reporter();

    assert_eq!(result.state, RunState::Failed);
    assert_eq!(rounds_started(&reporter), 2, "round three must not start");
    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert!(sent[0].text.contains("第 2/3 轮"), "{}", sent[0].text);
    assert!(sent[0].text.contains("已停止循环，剩余 1 轮不会再执行"), "{}", sent[0].text);
    assert_eq!(sent[0].details["round"], 2);
    assert_eq!(sent[0].details["rounds"], 3);
}

#[test]
fn a_delivery_failure_is_logged_and_never_changes_the_run_result() {
    let (sender, sent) = recording_sender(true);
    let (result, reporter) = run_alerting(&failing_tap_graph(), &Screen::default(), 1, alert_config(true), sender);

    assert_eq!(result.state, RunState::Failed);
    assert_eq!(result.error_code.as_deref(), Some("not_found"));
    assert_eq!(sent.lock().unwrap().len(), 1);
    let line = reporter.lines().iter().find(|l| l.event == "alert_failed").expect("alert_failed is logged");
    assert!(line.payload.as_ref().unwrap()["message"].as_str().unwrap().contains("连接失败"));
    assert!(!has_event(&reporter, "alert_sent"));
}

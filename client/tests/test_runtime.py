import threading

from tests.helpers import FakeDriver, button_target, crop_asset, make_scene

from skycore.matcher import AssetStore, MatcherEngine
from skycore.report import NullReporter
from skycore.runtime import RunContext, execute
from skycore.schema import IRDoc

BUTTON_CENTER = (300.0, 220.0)
BUTTON_SIZE = (120.0, 56.0)


def build(ir_payload: dict):
    """A run context over a synthetic screen with one locatable button."""
    scene = make_scene(scale=2.0)
    asset = crop_asset(scene.image, BUTTON_CENTER, BUTTON_SIZE, 2.0)
    target = button_target(asset, BUTTON_SIZE, record_scale=2.0, multi="highest")

    ir_payload.setdefault("targets", {})["btn"] = target.model_dump()
    ir = IRDoc.model_validate(ir_payload)

    driver = FakeDriver(scene)
    store = AssetStore()
    store.put_bytes(target.asset, asset)
    engine = MatcherEngine(store, allow_reuse=False)
    reporter = NullReporter()
    ctx = RunContext(ir=ir, driver=driver, engine=engine, reporter=reporter, run_id="test")
    return ir, ctx, driver, reporter


def ir_with(steps, **meta):
    return {
        "irVersion": 1,
        "meta": {"name": "t", "maxDurationMs": 20000, **meta},
        "entry": "main",
        "blocks": {"main": steps},
    }


def test_tap_locates_the_control_and_clicks_it():
    ir, ctx, driver, reporter = build(ir_with([{"op": "tap", "target": "btn", "id": "s1"}]))
    result = execute(ir, ctx)

    assert result.state == "done"
    assert len(driver.taps) == 1
    x, y = driver.taps[0]
    assert abs(x - BUTTON_CENTER[0]) <= 3
    assert abs(y - BUTTON_CENTER[1]) <= 3

    rows = reporter.summary_rows()
    assert rows and rows[0]["targetId"] == "btn"
    assert rows[0]["min"] > 0.8


def test_wait_for_returns_as_soon_as_the_target_is_visible():
    ir, ctx, driver, reporter = build(
        ir_with([{"op": "waitFor", "any": ["btn"], "timeoutMs": 2000, "intervalMs": 100, "id": "w1"}])
    )
    result = execute(ir, ctx)

    assert result.state == "done"
    assert result.step_count == 1
    assert driver.captures < 5


def test_wait_for_times_out_with_a_stable_reason():
    ir, ctx, _, _ = build(
        ir_with([{"op": "waitFor", "any": ["btn"], "timeoutMs": 300, "intervalMs": 100, "id": "w1"}])
    )
    # Point the search at an empty corner: the control is simply not there.
    ctx.ir.targets["btn"].roi = (0.58, 0.02, 0.40, 0.12)

    result = execute(ir, ctx)

    assert result.state == "failed"
    assert result.error_code == "timeout"
    assert result.error_msg and "none of" in result.error_msg


def test_missing_target_fails_the_step_with_not_found():
    ir, ctx, driver, _ = build(
        ir_with([{"op": "tap", "target": "nope", "id": "s1"}])
    )
    result = execute(ir, ctx)
    assert result.state == "failed"
    assert result.error_code == "unknown_target"
    assert driver.taps == []


def test_loop_runs_the_body_the_requested_number_of_times():
    ir, ctx, driver, _ = build(
        ir_with(
            [
                {
                    "op": "loop",
                    "count": 3,
                    "body": [{"op": "tap", "target": "btn", "id": "s1"}],
                }
            ]
        )
    )
    result = execute(ir, ctx)

    assert result.state == "done"
    assert len(driver.taps) == 3


def test_if_branches_on_visibility():
    ir, ctx, driver, _ = build(
        ir_with(
            [
                {
                    "op": "if",
                    "when": {"visible": "btn"},
                    "then": [{"op": "key", "key": "return"}],
                    "else": [{"op": "key", "key": "escape"}],
                }
            ]
        )
    )
    result = execute(ir, ctx)

    assert result.state == "done"
    assert driver.keys == ["return"]


def test_if_else_takes_the_else_branch_when_the_target_is_absent():
    ir, ctx, driver, _ = build(
        ir_with(
            [
                {
                    "op": "if",
                    "when": {"visible": "missing_target"},
                    "then": [{"op": "key", "key": "return"}],
                    "else": [{"op": "key", "key": "escape"}],
                }
            ]
        )
    )
    ir.blocks["main"][0]["when"] = {"missing": "btn"}
    result = execute(ir, ctx)

    assert result.state == "done"
    assert driver.keys == ["escape"]


def test_call_runs_another_block():
    ir, ctx, driver, _ = build(
        ir_with([{"op": "call", "block": "extra"}])
    )
    ir.blocks["extra"] = [{"op": "key", "key": "tab"}]
    result = execute(ir, ctx)

    assert result.state == "done"
    assert driver.keys == ["tab"]


def test_retry_reports_the_attempt_count_then_fails():
    ir, ctx, _, reporter = build(
        ir_with(
            [
                {
                    "op": "retry",
                    "times": 2,
                    "backoffMs": 1,
                    "body": [{"op": "waitFor", "any": ["btn"], "timeoutMs": 60, "intervalMs": 60}],
                }
            ]
        )
    )
    ctx.ir.targets["btn"].roi = (0.58, 0.02, 0.40, 0.12)

    result = execute(ir, ctx)

    assert result.state == "failed"
    retries = [event for event in reporter.log_lines if event["event"] == "retry"]
    assert len(retries) >= 1


def test_stop_request_ends_the_run_cleanly():
    ir, ctx, driver, _ = build(
        ir_with([{"op": "sleep", "ms": 2000}, {"op": "tap", "target": "btn"}])
    )
    stop = threading.Event()
    ctx.stop_event = stop
    threading.Timer(0.15, stop.set).start()

    result = execute(ir, ctx)

    assert result.state == "stopped"
    assert driver.taps == []


def test_stagnation_watchdog_trips_after_repeated_actions_with_no_change():
    ir, ctx, driver, _ = build(
        ir_with([{"op": "loop", "count": 20, "body": [{"op": "tap", "target": "btn"}]}])
    )
    # The fake frame never changes, so every tap is a no-op on screen.
    result = execute(ir, ctx)

    assert result.state == "failed"
    assert result.error_code == "stagnation"
    assert 10 <= len(driver.taps) <= 12


def test_run_deadline_is_enforced():
    ir, ctx, _, _ = build(ir_with([{"op": "sleep", "ms": 5000}], maxDurationMs=200))
    result = execute(ir, ctx)
    assert result.state == "failed"
    assert result.error_code == "timeout"


def test_swipe_uses_a_normalized_vector():
    ir, ctx, driver, _ = build(
        ir_with([{"op": "swipe", "from": "btn", "vector": [0.0, -0.25], "durationMs": 200}])
    )
    result = execute(ir, ctx)

    assert result.state == "done"
    assert len(driver.swipes) == 1
    x1, y1, x2, y2 = driver.swipes[0]
    assert abs(x2 - x1) <= 1
    assert y2 < y1 - 50


def test_validate_catches_dangling_targets():
    ir, _, _, _ = build(ir_with([{"op": "tap", "target": "ghost"}]))
    problems = ir.validate()
    assert any("ghost" in problem for problem in problems)


def test_a_stalled_capture_aborts_instead_of_polling_for_minutes():
    """macOS blocks a grab for ~30s when screen recording permission is gone."""
    ir, ctx, driver, reporter = build(
        ir_with([{"op": "waitFor", "any": ["btn"], "timeoutMs": 8000, "intervalMs": 100, "id": "w1"}])
    )
    original = driver.capture

    def stalled():
        frame = original()
        frame.capture_ms = 31_000.0
        return frame

    driver.capture = stalled
    result = execute(ir, ctx)

    assert result.state == "failed"
    assert result.error_code == "capture_stalled"
    assert any(event["event"] == "capture_stalled" for event in reporter.log_lines)
    # The point of the guard: fail immediately, not after the full timeout.
    assert driver.captures <= 2

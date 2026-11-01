"""IR interpreter.

Synchronous on purpose: it runs in a worker thread, so capture, matching and
input injection never interleave with the GUI thread.
"""

from __future__ import annotations

import re
import threading
import time
from dataclasses import dataclass, field
from typing import Any, Callable

from .driver import Driver
from .frame import Frame
from .geometry import Bounds
from .matcher.engine import Hit, MatcherEngine
from .report import Reporter
from .schema import IRDoc, PointTarget, TemplateTarget

MAX_CALL_DEPTH = 8
MAX_STEPS = 200_000
STAGNATION_LIMIT = 10
# macOS blocks a grab for ~30s when the process has lost screen recording
# permission, then returns a blank frame. Polling through that wastes the whole
# run, so a stalled capture aborts with a reason the operator can act on.
CAPTURE_STALL_MS = 3000.0


class RunError(Exception):
    """A step failed. ``reason`` is a stable enum used for run aggregation."""

    def __init__(self, reason: str, message: str = "", step_id: str | None = None, recoverable: bool = False) -> None:
        super().__init__(message or reason)
        self.reason = reason
        self.message = message or reason
        self.step_id = step_id
        self.recoverable = recoverable


@dataclass
class RunResult:
    state: str
    step_count: int
    error_code: str | None = None
    error_msg: str | None = None
    started_at: float = field(default_factory=time.time)
    ended_at: float = 0.0

    def to_json(self) -> dict[str, Any]:
        return {
            "state": self.state,
            "stepCount": self.step_count,
            "errorCode": self.error_code,
            "errorMsg": self.error_msg,
        }


@dataclass
class StepReport:
    step_id: str
    op: str
    ok: bool
    reason: str | None = None
    target: str | None = None
    score: float | None = None


class RunContext:
    """Everything one run needs, plus the rules for when to re-capture."""

    def __init__(
        self,
        ir: IRDoc,
        driver: Driver,
        engine: MatcherEngine,
        reporter: Reporter,
        run_id: str,
        stop_event: threading.Event | None = None,
        on_step: Callable[[StepReport], None] | None = None,
    ) -> None:
        self.ir = ir
        self.driver = driver
        self.engine = engine
        self.reporter = reporter
        self.run_id = run_id
        self.stop_event = stop_event or threading.Event()
        self.on_step = on_step
        self.vars: dict[str, Any] = dict(ir.vars)
        self._frame: Frame | None = None
        self.step_count = 0
        self._stagnation = 0
        self._last_action_hash: str | None = None

    # ------------------------------------------------------------- lifecycle

    def check_stop(self) -> None:
        if self.stop_event.is_set():
            raise RunError("stopped", "stop requested")

    def bounds(self) -> Bounds:
        return self.driver.bounds()

    def frame(self, fresh: bool = False) -> Frame:
        if fresh or self._frame is None:
            frame = self.driver.capture()
            capture_ms = getattr(frame, "capture_ms", 0.0)
            if capture_ms and capture_ms > CAPTURE_STALL_MS:
                self.reporter.log(
                    "error",
                    "capture_stalled",
                    {"ms": int(capture_ms), "hint": "屏幕录制权限可能失效，或目标窗口被全屏应用遮住"},
                )
                raise RunError("capture_stalled", f"a single screenshot took {capture_ms:.0f}ms")
            self._frame = frame
        return self._frame

    def invalidate(self) -> None:
        self._frame = None

    # -------------------------------------------------------------- matching

    def find(self, name: str, step_id: str, warn: bool = True) -> Hit | None:
        target = self.ir.target(name)
        if target is None:
            raise RunError("unknown_target", f'target "{name}" is not defined', step_id)

        hit = self.engine.resolve(target, name, self.frame())
        if hit is None:
            return None

        self.reporter.score(step_id, name, hit.score, hit.strategy, hit.elapsed_ms, hit.reused)

        if warn and self._is_low_confidence(target, hit):
            self.reporter.log(
                "warn",
                "low_confidence",
                {"stepId": step_id, "target": name, "score": round(hit.score, 3), "strategy": hit.strategy},
            )

        if hit.warnings:
            self.reporter.log("debug", "match_warnings", {"stepId": step_id, "target": name, "warnings": hit.warnings})

        return hit

    @staticmethod
    def _is_low_confidence(target: TemplateTarget | PointTarget, hit: Hit) -> bool:
        if isinstance(target, PointTarget):
            return False
        return hit.score < target.warnThreshold

    def record_action(self, step_id: str, name: str | None = None) -> None:
        """Detect a genuine stall: same screen after a completed input action.

        An idle screen on its own is normal (games sit still), so stagnation only
        counts consecutive *actions* that changed nothing. The frame is dropped
        first so the hash describes the screen *after* the action, not the frame
        that was used to locate the control.
        """
        self.invalidate()
        current = self.frame().hash()
        if self._last_action_hash == current:
            self._stagnation += 1
            if self._stagnation >= STAGNATION_LIMIT:
                raise RunError("stagnation", f"screen unchanged after {STAGNATION_LIMIT} actions", step_id)
            self.reporter.log("debug", "no_screen_change", {"stepId": step_id, "target": name, "count": self._stagnation})
        else:
            self._stagnation = 0
        self._last_action_hash = current
        self.invalidate()

    def note_step(self, step_id: str, op: str, ok: bool, reason: str | None = None, target: str | None = None, score: float | None = None) -> None:
        self.step_count += 1
        if self.step_count > MAX_STEPS:
            raise RunError("step_limit", f"exceeded {MAX_STEPS} steps")
        if self.on_step is not None:
            self.on_step(StepReport(step_id=step_id, op=op, ok=ok, reason=reason, target=target, score=score))


class Interpreter:
    def __init__(self, ctx: RunContext) -> None:
        self.ctx = ctx
        self.ir = ctx.ir
        self.deadline = time.monotonic() + max(1.0, self.ir.meta.maxDurationMs / 1000.0)
        self._call_depth = 0

    # --------------------------------------------------------------- driver

    def run(self) -> RunResult:
        self.ctx.reporter.log("info", "run_start", {"script": self.ir.meta.name, "entry": self.ir.entry})
        try:
            self.run_block(self.ir.entry)
        except RunError as error:
            if error.reason == "stopped":
                self.ctx.reporter.log("info", "run_stopped", {"step": error.step_id})
                return RunResult(state="stopped", step_count=self.ctx.step_count, error_code="stopped", error_msg=error.message)
            self.ctx.reporter.log("error", "run_failed", {"reason": error.reason, "message": error.message, "stepId": error.step_id})
            return RunResult(
                state="failed",
                step_count=self.ctx.step_count,
                error_code=error.reason,
                error_msg=error.message,
            )
        except Exception as error:  # pragma: no cover - defensive
            self.ctx.reporter.log("error", "run_crashed", {"message": str(error)})
            return RunResult(state="failed", step_count=self.ctx.step_count, error_code="engine_error", error_msg=str(error))

        self.ctx.reporter.log("info", "run_done", {"steps": self.ctx.step_count})
        return RunResult(state="done", step_count=self.ctx.step_count)

    def run_block(self, name: str, depth: int = 0) -> None:
        if depth > MAX_CALL_DEPTH:
            raise RunError("call_depth", f"call depth over {MAX_CALL_DEPTH}")
        steps = self.ir.steps(name)
        if name not in self.ir.blocks:
            raise RunError("unknown_block", f'block "{name}" is not defined')
        for index, step in enumerate(steps):
            self.tick(step, index)
            self.run_step(step, index)

    def tick(self, step: dict[str, Any], index: int) -> None:
        self.ctx.check_stop()
        self.check_deadline(self.step_id(step, index))

    def check_deadline(self, step_id: str) -> None:
        if time.monotonic() > self.deadline:
            raise RunError("timeout", "run exceeded maxDurationMs", step_id)

    @staticmethod
    def step_id(step: dict[str, Any], index: int) -> str:
        value = step.get("id")
        return str(value) if value else f"step{index}"

    # -------------------------------------------------------------- dispatch

    def run_step(self, step: dict[str, Any], index: int) -> None:
        op = str(step.get("op") or "")
        step_id = self.step_id(step, index)
        handler = getattr(self, f"op_{op}", None)
        if handler is None:
            raise RunError("unknown_op", f'unsupported op "{op}"', step_id)

        try:
            handler(step, step_id)
        except RunError as error:
            error.step_id = error.step_id or step_id
            self.ctx.note_step(step_id, op, ok=False, reason=error.reason, target=as_str(step.get("target")))
            raise
        self.ctx.note_step(step_id, op, ok=True, target=as_str(step.get("target")))

    # ------------------------------------------------------------ primitives

    def op_tap(self, step: dict[str, Any], step_id: str) -> None:
        name = require_target(step, step_id)
        hit = self.ctx.find(name, step_id)
        if hit is None:
            raise RunError("not_found", f'target "{name}" not found', step_id)
        self.ctx.reporter.log("debug", "tap", {"stepId": step_id, "target": name, "score": round(hit.score, 3)})
        self.ctx.driver.tap(hit.point[0], hit.point[1], int(step.get("durationMs", 60)))
        self.ctx.record_action(step_id, name)

    def op_longPress(self, step: dict[str, Any], step_id: str) -> None:
        name = require_target(step, step_id)
        hit = self.ctx.find(name, step_id)
        if hit is None:
            raise RunError("not_found", f'target "{name}" not found', step_id)
        self.ctx.driver.tap(hit.point[0], hit.point[1], int(step.get("durationMs", 600)))
        self.ctx.record_action(step_id, name)

    def op_swipe(self, step: dict[str, Any], step_id: str) -> None:
        start_name = step.get("from")
        if not isinstance(start_name, str):
            raise RunError("bad_step", "swipe needs `from`", step_id)
        start = self.ctx.find(start_name, step_id)
        if start is None:
            raise RunError("not_found", f'target "{start_name}" not found', step_id)

        end_name = step.get("to")
        if isinstance(end_name, str):
            end = self.ctx.find(end_name, step_id)
            if end is None:
                raise RunError("not_found", f'target "{end_name}" not found', step_id)
            target = end.point
        else:
            vector = step.get("vector") or [0.0, -0.3]
            bounds = self.ctx.bounds()
            target = (
                start.point[0] + float(vector[0]) * bounds.w,
                start.point[1] + float(vector[1]) * bounds.h,
            )

        self.ctx.driver.swipe(start.point[0], start.point[1], target[0], target[1], int(step.get("durationMs", 300)))
        self.ctx.record_action(step_id, start_name)

    def op_sleep(self, step: dict[str, Any], step_id: str) -> None:
        # A long sleep must not sail past the run deadline: cap it, then check.
        remaining = max(0.0, self.deadline - time.monotonic())
        requested = max(0.0, float(step.get("ms", 0)) / 1000.0)
        sleep_interruptible(min(requested, remaining) * 1000.0, self.ctx.stop_event)
        self.check_deadline(step_id)

    def op_waitFor(self, step: dict[str, Any], step_id: str) -> None:
        names = [name for name in step.get("any", []) if isinstance(name, str)]
        if not names:
            raise RunError("bad_step", "waitFor needs `any`", step_id)
        timeout = int(step.get("timeoutMs", 10_000)) / 1000.0
        interval = max(0.05, int(step.get("intervalMs", 500)) / 1000.0)
        deadline = time.monotonic() + timeout

        while True:
            self.ctx.check_stop()
            self.check_deadline(step_id)
            self.ctx.invalidate()
            for name in names:
                hit = self.ctx.find(name, step_id)
                if hit is not None:
                    return
            if time.monotonic() >= deadline:
                raise RunError("timeout", f"none of {names} appeared", step_id)
            sleep_interruptible(interval * 1000, self.ctx.stop_event)

    def op_waitGone(self, step: dict[str, Any], step_id: str) -> None:
        name = require_target(step, step_id)
        timeout = int(step.get("timeoutMs", 10_000)) / 1000.0
        interval = max(0.05, int(step.get("intervalMs", 500)) / 1000.0)
        deadline = time.monotonic() + timeout

        while True:
            self.ctx.check_stop()
            self.ctx.invalidate()
            if self.ctx.find(name, step_id, warn=False) is None:
                return
            if time.monotonic() >= deadline:
                raise RunError("timeout", f'target "{name}" never disappeared', step_id)
            sleep_interruptible(interval * 1000, self.ctx.stop_event)

    def op_waitStable(self, step: dict[str, Any], step_id: str) -> None:
        hold_ms = int(step.get("ms", 400))
        timeout = int(step.get("timeoutMs", 5000)) / 1000.0
        deadline = time.monotonic() + timeout
        stable_since: float | None = None
        previous: str | None = None

        while True:
            self.ctx.check_stop()
            self.check_deadline(step_id)
            frame = self.ctx.frame(fresh=True)
            current = frame.hash()
            now = time.monotonic()
            if previous is not None and current == previous:
                stable_since = stable_since if stable_since is not None else now
                if (now - stable_since) * 1000.0 >= hold_ms:
                    return
            else:
                stable_since = None
            previous = current
            if now >= deadline:
                raise RunError("timeout", "screen never settled", step_id)
            sleep_interruptible(120, self.ctx.stop_event)

    def op_if(self, step: dict[str, Any], step_id: str) -> None:
        when = step.get("when") or {}
        branch = step.get("then") if self.eval_condition(when, step_id) else step.get("else")
        for index, child in enumerate(branch or []):
            self.tick(child, index)
            self.run_step(child, index)

    def op_loop(self, step: dict[str, Any], step_id: str) -> None:
        count = int(step.get("count", 1))
        body = step.get("body") or []
        for _ in range(max(0, count)):
            self.ctx.check_stop()
            for index, child in enumerate(body):
                self.tick(child, index)
                self.run_step(child, index)

    def op_call(self, step: dict[str, Any], step_id: str) -> None:
        block = step.get("block")
        if not isinstance(block, str):
            raise RunError("bad_step", "call needs `block`", step_id)
        self.run_block(block, self._call_depth + 1)

    def op_retry(self, step: dict[str, Any], step_id: str) -> None:
        times = max(1, int(step.get("times", 3)))
        backoff = max(0, int(step.get("backoffMs", 500))) / 1000.0
        body = step.get("body") or []
        last: RunError | None = None

        for attempt in range(times):
            try:
                for index, child in enumerate(body):
                    self.tick(child, index)
                    self.run_step(child, index)
                return
            except RunError as error:
                if error.reason in ("stopped", "stagnation"):
                    raise
                last = error
                self.ctx.reporter.log(
                    "warn",
                    "retry",
                    {"stepId": step_id, "attempt": attempt + 1, "reason": error.reason},
                )
                self.ctx.invalidate()
                if attempt < times - 1:
                    sleep_interruptible(backoff * 1000 * (2**attempt), self.ctx.stop_event)

        if last is not None:
            raise RunError(last.reason, f"retry exhausted: {last.message}", step_id)

    def op_log(self, step: dict[str, Any], step_id: str) -> None:
        self.ctx.reporter.log(str(step.get("level", "info")), "script_log", {"stepId": step_id, "msg": str(step.get("msg", ""))})

    def op_key(self, step: dict[str, Any], step_id: str) -> None:
        self.ctx.driver.key(str(step.get("key", "")))
        self.ctx.record_action(step_id)

    def op_text(self, step: dict[str, Any], step_id: str) -> None:
        self.ctx.driver.text(str(step.get("text", "")))
        self.ctx.record_action(step_id)

    def op_snapshot(self, step: dict[str, Any], step_id: str) -> None:
        frame = self.ctx.frame(fresh=True)
        self.ctx.reporter.log("info", "snapshot", {"stepId": step_id, "label": str(step.get("label", "snap")), "hash": frame.hash()[:16]})

    # ------------------------------------------------------------ conditions

    def eval_condition(self, condition: dict[str, Any], step_id: str) -> bool:
        if "visible" in condition:
            names = condition["visible"]
            names = [names] if isinstance(names, str) else [n for n in names if isinstance(n, str)]
            self.ctx.invalidate()
            return any(self.ctx.find(name, step_id, warn=False) is not None for name in names)

        if "missing" in condition:
            self.ctx.invalidate()
            return self.ctx.find(str(condition["missing"]), step_id, warn=False) is None

        if "var" in condition:
            name = str(condition["var"])
            op = str(condition.get("op", "eq"))
            actual = self.ctx.vars.get(name)
            expected = condition.get("value")
            return compare(actual, op, expected)

        if "not" in condition:
            return not self.eval_condition(condition["not"], step_id)

        if "all" in condition:
            return all(self.eval_condition(item, step_id) for item in condition["all"])

        if "any" in condition:
            return any(self.eval_condition(item, step_id) for item in condition["any"])

        return False


# -------------------------------------------------------------------- helpers


def require_target(step: dict[str, Any], step_id: str) -> str:
    name = step.get("target")
    if not isinstance(name, str) or not name:
        raise RunError("bad_step", f"{step.get('op')} needs `target`", step_id)
    return name


def as_str(value: Any) -> str | None:
    return value if isinstance(value, str) else None


def sleep_interruptible(ms: float, stop_event: threading.Event) -> None:
    if ms <= 0:
        return
    # Long sleeps are sliced so a stop request is honoured within ~100ms.
    remaining = ms / 1000.0
    while remaining > 0:
        if stop_event.is_set():
            raise RunError("stopped", "stop requested")
        chunk = min(remaining, 0.1)
        time.sleep(chunk)
        remaining -= chunk


def compare(actual: Any, op: str, expected: Any) -> bool:
    try:
        if op == "eq":
            return actual == expected
        if op == "ne":
            return actual != expected
        if op == "lt":
            return float(actual) < float(expected)
        if op == "le":
            return float(actual) <= float(expected)
        if op == "gt":
            return float(actual) > float(expected)
        if op == "ge":
            return float(actual) >= float(expected)
        if op == "contains":
            return str(expected) in str(actual)
        if op == "match":
            return re.search(str(expected), str(actual)) is not None
    except (TypeError, ValueError):
        return False
    return False


def execute(ir: IRDoc, ctx: RunContext) -> RunResult:
    result = Interpreter(ctx).run()
    result.ended_at = time.time()
    return result

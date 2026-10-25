"""Run reporting: live over the websocket, durable over HTTP.

The split exists because the two have different costs. Live updates are tiny and
frequent (a browser is watching), durable logs are large and can wait. Scores are
aggregated per step so a 30 minute run writes tens of rows, not tens of
thousands - that is what keeps D1 inside its free daily write limit.
"""

from __future__ import annotations

import queue
import threading
import time
from dataclasses import dataclass, field
from typing import Any


@dataclass
class Aggregate:
    step_id: str
    target_id: str
    n: int = 0
    total: float = 0.0
    min_score: float = 1.0
    max_ms: int = 0
    last: float = 0.0
    last_strategy: str = ""

    def add(self, score: float, ms: float, strategy: str) -> None:
        self.n += 1
        self.total += score
        self.min_score = min(self.min_score, score)
        self.max_ms = max(self.max_ms, int(ms))
        self.last = score
        self.last_strategy = strategy

    @property
    def avg(self) -> float:
        return self.total / self.n if self.n else 0.0

    def to_row(self) -> dict[str, Any]:
        return {
            "stepId": self.step_id,
            "targetId": self.target_id,
            "n": self.n,
            "avg": round(self.avg, 4),
            "min": round(self.min_score, 4),
            "maxMs": self.max_ms,
            "last": round(self.last, 4),
        }


@dataclass
class Reporter:
    run_id: str
    queue: queue.Queue = field(default_factory=queue.Queue)
    aggregates: dict[tuple[str, str], Aggregate] = field(default_factory=dict)
    log_lines: list[dict[str, Any]] = field(default_factory=list)
    _lock: threading.Lock = field(default_factory=threading.Lock)
    _seq: int = 0

    # ------------------------------------------------------------- producers

    def log(self, level: str, event: str, payload: dict[str, Any] | None = None) -> None:
        item: dict[str, Any] = {
            "runId": self.run_id,
            "ts": int(time.time() * 1000),
            "level": level,
            "event": event,
        }
        if payload:
            item["payload"] = payload
        with self._lock:
            self.log_lines.append(item)
            self._seq += 1
        self.queue.put(("log", item))

    def score(self, step_id: str, target_id: str, score: float, strategy: str, ms: float, reused: bool) -> None:
        item = {
            "runId": self.run_id,
            "stepId": step_id,
            "targetId": target_id,
            "score": round(float(score), 4),
            "strategy": strategy,
            "ms": int(ms),
            "reused": bool(reused),
        }
        with self._lock:
            key = (step_id, target_id)
            aggregate = self.aggregates.get(key)
            if aggregate is None:
                aggregate = Aggregate(step_id=step_id, target_id=target_id)
                self.aggregates[key] = aggregate
            aggregate.add(float(score), ms, strategy)
        self.queue.put(("score", item))

    # ------------------------------------------------------------- consumers

    def take(self, limit: int = 200) -> tuple[list[dict], list[dict]]:
        """Drain queued items; called from the transport loop."""
        scores: list[dict] = []
        logs: list[dict] = []
        while len(scores) + len(logs) < limit:
            try:
                kind, item = self.queue.get_nowait()
            except queue.Empty:
                break
            if kind == "score":
                scores.append(item)
            else:
                logs.append(item)
        return scores, logs

    def pending(self) -> int:
        return self.queue.qsize()

    def summary_rows(self) -> list[dict]:
        with self._lock:
            rows = [aggregate.to_row() for aggregate in self.aggregates.values()]
        return sorted(rows, key=lambda row: row["min"])

    def summary(self) -> dict[str, Any]:
        rows = self.summary_rows()
        return {
            "steps": rows,
            "worst": rows[0] if rows else None,
            "low": [row for row in rows if row["min"] < 0.82],
        }

    def jsonl(self) -> str:
        with self._lock:
            lines = list(self.log_lines)
        return "\n".join(_encode(line) for line in lines)

    def clear_jsonl(self) -> None:
        with self._lock:
            self.log_lines.clear()


def _encode(item: dict[str, Any]) -> str:
    import json

    return json.dumps(item, ensure_ascii=False, separators=(",", ":"))


class NullReporter(Reporter):
    """Used by ``probe`` runs that must not talk to a platform."""

    def __init__(self) -> None:
        super().__init__(run_id="probe")

    def log(self, level: str, event: str, payload: dict[str, Any] | None = None) -> None:  # noqa: D401
        item = {"runId": self.run_id, "ts": int(time.time() * 1000), "level": level, "event": event}
        if payload:
            item["payload"] = payload
        with self._lock:
            self.log_lines.append(item)
        if level in ("warn", "error"):
            print(f"[{level}] {event} {payload or ''}")

    def score(self, step_id: str, target_id: str, score: float, strategy: str, ms: float, reused: bool) -> None:
        with self._lock:
            key = (step_id, target_id)
            aggregate = self.aggregates.get(key)
            if aggregate is None:
                aggregate = Aggregate(step_id=step_id, target_id=target_id)
                self.aggregates[key] = aggregate
            aggregate.add(float(score), ms, strategy)

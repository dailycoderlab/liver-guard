"""Per target hit memory.

Two jobs:

* skip matching entirely when the screen has not changed since the last hit -
  idle game screens are the common case and this makes the average step cost
  single digit milliseconds,
* give the disambiguation policy a reference point so "the second identical
  button" resolves to the one that was clicked last time.
"""

from __future__ import annotations

from dataclasses import dataclass, field

from ..geometry import Rect


@dataclass
class TrackEntry:
    frame_hash: str
    box: Rect
    score: float
    point: tuple[float, float]


@dataclass
class Tracker:
    entries: dict[str, TrackEntry] = field(default_factory=dict)
    hits: int = 0
    reuses: int = 0

    def remember(self, target_id: str, frame_hash: str, box: Rect, score: float, point: tuple[float, float]) -> None:
        self.entries[target_id] = TrackEntry(frame_hash=frame_hash, box=box, score=score, point=point)
        self.hits += 1

    def reusable(self, target_id: str, frame_hash: str) -> TrackEntry | None:
        entry = self.entries.get(target_id)
        if entry is None:
            return None
        if entry.frame_hash != frame_hash:
            return None
        self.reuses += 1
        return entry

    def last_box(self, target_id: str) -> Rect | None:
        entry = self.entries.get(target_id)
        return entry.box if entry else None

    def forget(self, target_id: str) -> None:
        self.entries.pop(target_id, None)

    def clear(self) -> None:
        self.entries.clear()

    def stats(self) -> dict[str, int]:
        return {"hits": self.hits, "reuses": self.reuses}

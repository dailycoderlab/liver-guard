"""The only surface a platform client has to implement.

Six primitives plus optional native UI lookup. Everything else - matching,
waiting, branching, retry, reporting - lives in ``skycore`` and is shared by
every platform.
"""

from __future__ import annotations

from typing import Protocol, runtime_checkable

from .frame import Frame
from .geometry import Bounds, Rect
from .protocol import Caps


@runtime_checkable
class Driver(Protocol):
    def bounds(self) -> Bounds:
        """Current capture surface, logical points in global coordinates."""

    def capture(self) -> Frame:
        """Screenshot at device pixel resolution."""

    def tap(self, x: float, y: float, duration_ms: int = 60) -> None:
        """Click at a global logical point."""

    def swipe(self, x1: float, y1: float, x2: float, y2: float, duration_ms: int = 300) -> None:
        """Drag between two global logical points."""

    def key(self, name: str) -> None:
        """Press a named key (return, escape, space, tab, ...)."""

    def text(self, value: str) -> None:
        """Type a string into the focused control."""

    def ax_at(self, x: float, y: float) -> Rect | None:
        """Native UI element under a point, when the platform offers one."""

    def caps(self) -> Caps:
        """Declared capabilities, used by the console to adapt its UI."""

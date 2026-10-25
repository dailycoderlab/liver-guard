"""Window discovery and activation via CoreGraphics."""

from __future__ import annotations

from dataclasses import dataclass

from skycore.geometry import Rect


@dataclass
class WindowInfo:
    number: int
    pid: int
    owner: str
    title: str
    bounds: Rect
    layer: int

    def __str__(self) -> str:
        return f"#{self.number} {self.owner} — {self.title or '(untitled)'} [{self.bounds.w:.0f}x{self.bounds.h:.0f}]"


def list_windows(on_screen_only: bool = True, include_all_layers: bool = False) -> list[WindowInfo]:
    try:
        from Quartz import (
            CGWindowListCopyWindowInfo,
            kCGNullWindowID,
            kCGWindowListExcludeDesktopElements,
            kCGWindowListOptionOnScreenOnly,
        )
    except Exception:
        return []

    options = kCGWindowListExcludeDesktopElements
    if on_screen_only:
        options |= kCGWindowListOptionOnScreenOnly

    try:
        raw = CGWindowListCopyWindowInfo(options, kCGNullWindowID) or []
    except Exception:
        return []

    windows: list[WindowInfo] = []
    for item in raw:
        try:
            layer = int(item.get("kCGWindowLayer", 0))
            if not include_all_layers and layer != 0:
                continue
            bounds = item.get("kCGWindowBounds") or {}
            width = float(bounds.get("Width", 0))
            height = float(bounds.get("Height", 0))
            if width < 40 or height < 40:
                continue
            windows.append(
                WindowInfo(
                    number=int(item.get("kCGWindowNumber", 0)),
                    pid=int(item.get("kCGWindowOwnerPID", 0)),
                    owner=str(item.get("kCGWindowOwnerName", "")),
                    title=str(item.get("kCGWindowName", "") or ""),
                    bounds=Rect(
                        float(bounds.get("X", 0)),
                        float(bounds.get("Y", 0)),
                        width,
                        height,
                    ),
                    layer=layer,
                )
            )
        except Exception:
            continue
    return windows


def find_window(query: str) -> WindowInfo | None:
    """Match by case-insensitive substring of the owner name or window title."""
    needle = query.strip().lower()
    if not needle:
        return None
    candidates = list_windows()
    for window in candidates:
        if needle in window.owner.lower():
            return window
    for window in candidates:
        if needle in window.title.lower():
            return window
    return None


def window_by_number(number: int) -> WindowInfo | None:
    for window in list_windows(on_screen_only=False, include_all_layers=True):
        if window.number == number:
            return window
    return None


def occluding_windows(target: WindowInfo, min_overlap: float = 0.15) -> list[WindowInfo]:
    """Windows stacked above ``target`` that cover a meaningful part of it.

    Capture grabs a screen *rectangle*, so anything drawn on top of the target
    ends up in the frame. Detecting this up front is far friendlier than letting
    the operator wonder why the templates never match.
    """
    above: list[WindowInfo] = []
    target_area = max(1.0, target.bounds.w * target.bounds.h)
    for window in list_windows():  # front-to-back order
        if window.number == target.number:
            break
        overlap = window.bounds.intersection(target.bounds)
        if overlap.w * overlap.h / target_area >= min_overlap:
            above.append(window)
    return above


def activate(window: WindowInfo) -> bool:
    """Bring a window's app forward so CGEvents reach it."""
    try:
        from AppKit import NSApplicationActivateIgnoringOtherApps, NSRunningApplication

        app = NSRunningApplication.runningApplicationWithProcessIdentifier_(window.pid)
        if app is None:
            return False
        try:
            return bool(app.activateWithOptions_(NSApplicationActivateIgnoringOtherApps))
        except Exception:
            return bool(app.activate())
    except Exception:
        return False


def main_display_bounds() -> Rect:
    try:
        from Quartz import CGDisplayBounds, CGMainDisplayID

        bounds = CGDisplayBounds(CGMainDisplayID())
        return Rect(
            float(bounds.origin.x),
            float(bounds.origin.y),
            float(bounds.size.width),
            float(bounds.size.height),
        )
    except Exception:
        return Rect(0.0, 0.0, 1440.0, 900.0)

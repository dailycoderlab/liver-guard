"""MacDriver: the macOS implementation of ``skycore.driver.Driver``."""

from __future__ import annotations

import time
from dataclasses import dataclass

from skycore.driver import CaptureSurfaceChanged
from skycore.frame import Frame
from skycore.geometry import Bounds, Rect

from . import ax as ax_lookup
from . import window as window_tools
from .capture import MssCapturer
from .input import QuartzInput


@dataclass
class Surface:
    """What we are looking at: the whole display or one window."""

    rect: Rect
    label: str
    window_number: int | None = None
    pid: int | None = None


class MacDriver:
    def __init__(
        self,
        window_query: str | None = None,
        window_number: int | None = None,
        full_screen: bool = True,
        activate_window: bool = True,
    ) -> None:
        self.capturer = MssCapturer()
        self.injector = QuartzInput()
        self.window_query = window_query
        self.window_number = window_number
        self.full_screen = full_screen and not window_query and window_number is None
        self.activate_window = activate_window
        self._scale_cache: dict[tuple[int, int, int, int], float] = {}
        self._last_surface: Surface | None = None
        self.last_capture_ms: float = 0.0

    # ----------------------------------------------------------------- surface

    def surface(self) -> Surface:
        if self.full_screen:
            return Surface(rect=window_tools.main_display_bounds(), label="display")

        found = None
        if self.window_number is not None:
            found = window_tools.window_by_number(self.window_number)
        if found is None and self.window_query:
            found = window_tools.find_window(self.window_query)
        if found is None:
            # Never turn a missing selected window into an entire-screen target:
            # doing so could inject input into an unrelated application.
            raise RuntimeError("Selected window is unavailable; select the target again")

        return Surface(
            rect=found.bounds,
            label=f"{found.owner} — {found.title or '(untitled)'}",
            window_number=found.number,
            pid=found.pid,
        )

    def activate(self) -> bool:
        if self.full_screen or not self.activate_window:
            return True
        surface = self.surface()
        if surface.window_number is None:
            return False
        found = window_tools.window_by_number(surface.window_number)
        if found is None:
            return False
        return window_tools.activate(found)

    # ------------------------------------------------------------------ scale

    def measure_scale(self, rect: Rect, force: bool = False, surface: Surface | None = None) -> float:
        """Pixels per logical point, measured from a real capture.

        Measured rather than assumed: a scaled display mode, an external monitor
        and a window straddling two displays all break the "2x means 2x" rule,
        and every downstream conversion depends on this number.
        """
        key = (int(rect.x), int(rect.y), int(rect.w), int(rect.h))
        if not force and key in self._scale_cache:
            return self._scale_cache[key]

        probe = Rect(rect.x, rect.y, min(64.0, rect.w), min(64.0, rect.h))
        if surface is None:
            surface = self.surface()
        if surface.window_number is not None:
            image = self.capturer.grab_window(probe, surface.window_number)
        else:
            image = self.capturer.grab(probe)
        scale = float(image.shape[1]) / float(probe.w)

        self._scale_cache[key] = scale
        return scale

    def refresh(self, force: bool = True) -> Surface:
        surface = self.surface()
        self.measure_scale(surface.rect, force=force, surface=surface)
        self._last_surface = surface
        return surface

    # ------------------------------------------------------------- Driver API

    def bounds(self) -> Bounds:
        surface = self.surface()
        scale = self.measure_scale(surface.rect, surface=surface)
        return Bounds(
            x=surface.rect.x,
            y=surface.rect.y,
            w=surface.rect.w,
            h=surface.rect.h,
            scale=scale,
            orientation="landscape" if surface.rect.w >= surface.rect.h else "portrait",
        )

    def capture(self) -> Frame:
        surface = self.surface()
        started = time.perf_counter()
        if surface.window_number is not None:
            image = self.capturer.grab_window(surface.rect, surface.window_number)
        else:
            image = self.capturer.grab(surface.rect)
        elapsed_ms = (time.perf_counter() - started) * 1000.0
        if surface.window_number is not None:
            current = self.surface()
            if (current.window_number, current.pid, current.rect) != (surface.window_number, surface.pid, surface.rect):
                raise CaptureSurfaceChanged('截图期间目标窗口位置或状态发生变化，请重试')
        scale = image.shape[1] / surface.rect.w
        bounds = Bounds(
            x=surface.rect.x, y=surface.rect.y, w=surface.rect.w, h=surface.rect.h,
            scale=scale,
            orientation="landscape" if surface.rect.w >= surface.rect.h else "portrait",
        )
        frame = Frame(image=image, bounds=bounds, seq=0, ts=time.time(), capture_ms=elapsed_ms)
        # Keep the measured scale honest even if the surface moved mid-flight.
        self._scale_cache[(int(surface.rect.x), int(surface.rect.y), int(surface.rect.w), int(surface.rect.h))] = frame.scale
        self.last_capture_ms = elapsed_ms
        return frame

    def _ensure_input_target(self, *points: tuple[float, float]) -> None:
        if self.full_screen:
            return
        surface = self.surface()
        if surface.window_number is None or not self.activate():
            raise RuntimeError("Selected window cannot be activated")
        target = window_tools.window_by_number(surface.window_number)
        if target is None:
            raise RuntimeError("Selected window is unavailable")
        if any(not target.bounds.contains(x, y) for x, y in points):
            raise ValueError("Input point is outside the selected window")
        for other in window_tools.occluding_windows(target, min_overlap=0.000001):
            if any(other.bounds.contains(x, y) for x, y in points):
                raise RuntimeError("Selected window is covered at the input point")

    def tap(self, x: float, y: float, duration_ms: int = 60) -> None:
        self._ensure_input_target((x, y))
        self.injector.tap(x, y, duration_ms)

    def swipe(self, x1: float, y1: float, x2: float, y2: float, duration_ms: int = 300) -> None:
        self._ensure_input_target((x1, y1), (x2, y2))
        self.injector.swipe(x1, y1, x2, y2, duration_ms)

    def key(self, name: str) -> None:
        self._ensure_input_target()
        self.injector.key(name)

    def text(self, value: str) -> None:
        self._ensure_input_target()
        self.injector.text(value)

    def ax_at(self, x: float, y: float) -> Rect | None:
        return ax_lookup.element_at(x, y)

"""MacDriver: the macOS implementation of ``skycore.driver.Driver``."""

from __future__ import annotations

import time
from dataclasses import dataclass

from skycore.frame import Frame
from skycore.geometry import Bounds, Rect
from skycore.protocol import Caps

from . import ax as ax_lookup
from . import window as window_tools
from .capture import MssCapturer
from .input import QuartzInput

DEFAULT_CAPS = Caps(
    max_fps=8,
    inject_kinds=["tap", "longPress", "swipe", "key", "text"],
    ui_query="ax",
    max_window=(600, 400),
    secure_screen=False,
    surface="window",
)


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
            # Falling back to the whole display beats failing a recording session.
            return Surface(rect=window_tools.main_display_bounds(), label="display (window not found)")

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

    def measure_scale(self, rect: Rect, force: bool = False) -> float:
        """Pixels per logical point, measured from a real capture.

        Measured rather than assumed: a scaled display mode, an external monitor
        and a window straddling two displays all break the "2x means 2x" rule,
        and every downstream conversion depends on this number.
        """
        key = (int(rect.x), int(rect.y), int(rect.w), int(rect.h))
        if not force and key in self._scale_cache:
            return self._scale_cache[key]

        probe = Rect(rect.x, rect.y, min(64.0, rect.w), min(64.0, rect.h))
        scale = 1.0
        try:
            image = self.capturer.grab(probe)
            if image.shape[1] > 0 and probe.w > 0:
                scale = float(image.shape[1]) / float(probe.w)
        except Exception:
            scale = 1.0

        self._scale_cache[key] = scale
        return scale

    def refresh(self, force: bool = True) -> Surface:
        surface = self.surface()
        self.measure_scale(surface.rect, force=force)
        self._last_surface = surface
        return surface

    # ------------------------------------------------------------- Driver API

    def bounds(self) -> Bounds:
        surface = self.surface()
        scale = self.measure_scale(surface.rect)
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
        bounds = self.bounds()
        started = time.perf_counter()
        image = self.capturer.grab(surface.rect)
        elapsed_ms = (time.perf_counter() - started) * 1000.0
        frame = Frame(image=image, bounds=bounds, seq=0, ts=time.time(), capture_ms=elapsed_ms)
        # Keep the measured scale honest even if the surface moved mid-flight.
        self._scale_cache[(int(surface.rect.x), int(surface.rect.y), int(surface.rect.w), int(surface.rect.h))] = frame.scale
        self.last_capture_ms = elapsed_ms
        return frame

    def tap(self, x: float, y: float, duration_ms: int = 60) -> None:
        self.injector.tap(x, y, duration_ms)

    def swipe(self, x1: float, y1: float, x2: float, y2: float, duration_ms: int = 300) -> None:
        self.injector.swipe(x1, y1, x2, y2, duration_ms)

    def key(self, name: str) -> None:
        self.injector.key(name)

    def text(self, value: str) -> None:
        self.injector.text(value)

    def ax_at(self, x: float, y: float) -> Rect | None:
        return ax_lookup.element_at(x, y)

    def caps(self) -> Caps:
        caps = Caps(**{**DEFAULT_CAPS.__dict__})
        caps.surface = "screen" if self.full_screen else "window"
        return caps

    # ----------------------------------------------------------- diagnostics

    def describe(self) -> dict:
        surface = self.surface()
        return {
            "surface": surface.label,
            "rect": surface.rect.as_tuple(),
            "scale": self.measure_scale(surface.rect, force=True),
        }

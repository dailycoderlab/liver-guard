"""Windows implementation of the skycore Driver protocol.

All public coordinates are physical virtual-desktop pixels (scale 1), including
on mixed-DPI displays. A window capture is a screen crop, not an isolated window.
"""
from __future__ import annotations

import math
import time
from dataclasses import dataclass

from skycore.frame import Frame
from skycore.geometry import Bounds, Rect
from skycore.protocol import Caps

from . import window as windows
from ._native import POINT, WinAPI
from .capture import MssCapturer
from .input import SendInputInjector


class SurfaceUnavailable(RuntimeError):
    """The chosen window is unavailable; no full-screen fallback is performed."""


@dataclass(frozen=True)
class Surface:
    rect: Rect
    label: str
    hwnd: int | None = None
    pid: int | None = None


class WindowsDriver:
    def __init__(self, window_query: str | None = None, window_number: int | None = None,
                 full_screen: bool = False, activate_window: bool = True):
        if window_query is not None and not window_query.strip():
            raise ValueError('window query must not be empty')
        if window_query is not None and window_number is not None:
            raise ValueError('choose a window query or a window handle, not both')
        if full_screen and (window_query is not None or window_number is not None):
            raise ValueError('choose either full screen or a window')
        if not full_screen and window_query is None and window_number is None:
            raise ValueError('specify a window or explicitly select full_screen=True')
        self.api = WinAPI()
        self.capturer = MssCapturer()
        self.injector = SendInputInjector(self.api)
        self.window_query = window_query
        self.window_number = window_number
        self.full_screen = full_screen
        self.activate_window = activate_window
        self._identity = None
        self.last_capture_ms = 0.0

    def _surface(self):
        desktop = windows.virtual_bounds(self.api)
        if self.full_screen:
            return Surface(desktop, 'virtual desktop')
        if self._identity is None:
            found = (windows.window_by_handle(self.api, self.window_number)
                     if self.window_number is not None else windows.find_window(self.api, self.window_query))
            if found is None:
                raise SurfaceUnavailable('Selected window was not found or is hidden or minimized')
            self._identity = (found.hwnd, found.pid, found.thread_id, found.class_name)
        else:
            found = windows.window_by_handle(self.api, self._identity[0])
            if found is None or (found.hwnd, found.pid, found.thread_id, found.class_name) != self._identity:
                raise SurfaceUnavailable('Selected window is invalid, hidden, minimized, or has been replaced')
        rect = found.bounds.intersection(desktop)
        if rect.w < 1 or rect.h < 1:
            raise SurfaceUnavailable('Selected window is outside the virtual desktop')
        return Surface(rect, found.title or '(untitled)', found.hwnd, found.pid)

    def surface(self):
        with self.api.physical_pixels():
            return self._surface()

    @staticmethod
    def _bounds(surface):
        r = surface.rect
        return Bounds(r.x, r.y, r.w, r.h, scale=1.0,
                      orientation='landscape' if r.w >= r.h else 'portrait')

    def bounds(self):
        with self.api.physical_pixels():
            return self._bounds(self._surface())

    def capture(self):
        with self.api.physical_pixels():
            surface = self._surface()
            started = time.perf_counter()
            image = self.capturer.grab(surface.rect)
            elapsed = (time.perf_counter() - started) * 1000
            if self._surface() != surface:
                raise SurfaceUnavailable('Capture surface changed during capture; retry')
            if image.shape[:2] != (int(surface.rect.h), int(surface.rect.w)):
                raise RuntimeError('Capture dimensions do not match the physical surface')
            self.last_capture_ms = elapsed
            return Frame(image=image, bounds=self._bounds(surface), capture_ms=elapsed)

    def _foreground(self, surface):
        if surface.hwnd is None:
            return
        u = self.api.user32
        if self.activate_window:
            u.SetForegroundWindow(surface.hwnd)
        foreground = u.GetForegroundWindow()
        if not foreground or u.GetAncestor(foreground, 2) != surface.hwnd:
            raise SurfaceUnavailable('Selected window is not foreground; Windows may have denied activation')
        self._surface()

    def activate(self):
        with self.api.physical_pixels():
            surface = self._surface()
            self._foreground(surface)
            return True

    def _point(self, x, y):
        surface = self._surface()
        rect = surface.rect
        if not math.isfinite(x) or not math.isfinite(y) or not (rect.x <= x < rect.right and rect.y <= y < rect.bottom):
            raise ValueError('point is outside the selected surface')
        if surface.hwnd is not None:
            u = self.api.user32
            foreground = u.GetForegroundWindow()
            if not foreground or u.GetAncestor(foreground, 2) != surface.hwnd:
                raise SurfaceUnavailable('Selected window lost foreground during input')
            hit = u.WindowFromPoint(POINT(math.floor(x), math.floor(y)))
            if not hit or u.GetAncestor(hit, 2) != surface.hwnd:
                raise SurfaceUnavailable('Selected window is occluded at the input point')

    def tap(self, x, y, duration_ms=60):
        with self.api.physical_pixels():
            self._foreground(self._surface())
            self._point(x, y)
            self.injector.tap(x, y, duration_ms)

    def swipe(self, x1, y1, x2, y2, duration_ms=300):
        with self.api.physical_pixels():
            self._foreground(self._surface())
            self._point(x1, y1)
            self._point(x2, y2)
            self.injector.swipe(x1, y1, x2, y2, duration_ms, check=self._point)

    def key(self, name):
        with self.api.physical_pixels():
            self._foreground(self._surface())
            self.injector.key(name, check=self._check_focus)

    def _check_focus(self):
        surface = self._surface()
        if surface.hwnd is not None:
            u = self.api.user32
            foreground = u.GetForegroundWindow()
            if not foreground or u.GetAncestor(foreground, 2) != surface.hwnd:
                raise SurfaceUnavailable('Selected window lost foreground during input')

    def text(self, value):
        with self.api.physical_pixels():
            self._foreground(self._surface())
            self.injector.text(value, check=self._check_focus)

    def ax_at(self, x, y):
        return None

    def caps(self):
        return Caps(ui_query='none', surface='screen' if self.full_screen else 'window')

    def describe(self):
        """Report observed capture status, without claiming a permission guarantee."""
        try:
            frame = self.capture()
            flat = frame.content_std() < 1.0
            return {'ok': True, 'surface': self.surface().label, 'rect': frame.bounds.rect.as_tuple(),
                    'scale': 1.0, 'capture_ms': frame.capture_ms, 'flat_image': flat,
                    'note': 'Screen-region capture may be occluded or protected; a flat image can be legitimate. SendInput can be blocked by integrity levels or the secure desktop.'}
        except Exception as exc:  # noqa: BLE001 - diagnostics report backend failures
            return {'ok': False, 'error': str(exc),
                    'note': 'Check window visibility, desktop session, capture availability and process integrity levels.'}

    def close(self):
        self.capturer.close()

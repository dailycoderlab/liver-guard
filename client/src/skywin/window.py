"""Visible top-level windows and physical virtual-desktop geometry."""
from __future__ import annotations

import ctypes as C
from dataclasses import dataclass

from skycore.geometry import Rect

from ._native import RECT, U32


@dataclass(frozen=True)
class Window:
    hwnd: int
    pid: int
    thread_id: int
    class_name: str
    title: str
    bounds: Rect


def virtual_bounds(api) -> Rect:
    u = api.user32
    rect = Rect(u.GetSystemMetrics(76), u.GetSystemMetrics(77), u.GetSystemMetrics(78), u.GetSystemMetrics(79))
    if rect.w <= 0 or rect.h <= 0:
        raise RuntimeError('Windows virtual desktop is unavailable')
    return rect


def window_by_handle(api, hwnd: int) -> Window | None:
    u = api.user32
    if not u.IsWindow(hwnd) or not u.IsWindowVisible(hwnd) or u.IsIconic(hwnd):
        return None
    rect = RECT()
    if not u.GetWindowRect(hwnd, C.byref(rect)):
        return None
    bounds = Rect(rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top)
    if bounds.w <= 0 or bounds.h <= 0:
        return None
    pid = U32()
    thread = u.GetWindowThreadProcessId(hwnd, C.byref(pid))
    if not thread or not pid.value:
        return None
    title = C.create_unicode_buffer(u.GetWindowTextLengthW(hwnd) + 1)
    u.GetWindowTextW(hwnd, title, len(title))
    cls = C.create_unicode_buffer(256)
    if not u.GetClassNameW(hwnd, cls, len(cls)):
        return None
    return Window(int(hwnd), pid.value, int(thread), cls.value, title.value, bounds)


def list_windows(api) -> list[Window]:
    result = []
    @api.WNDENUMPROC
    def callback(hwnd, _):
        found = window_by_handle(api, hwnd)
        if found and found.title:
            result.append(found)
        return True
    if not api.user32.EnumWindows(callback, 0):
        raise OSError('EnumWindows failed')
    return result


def find_window(api, query: str) -> Window | None:
    needle = query.strip().casefold()
    if not needle:
        raise ValueError('window query must not be empty')
    return next((w for w in list_windows(api) if needle in w.title.casefold()), None)

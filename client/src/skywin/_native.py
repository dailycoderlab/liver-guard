"""Small, explicitly typed Win32 bindings. Coordinates are physical pixels."""
from __future__ import annotations

import ctypes as C
import os
from contextlib import contextmanager

I32 = C.c_int32
U32 = C.c_uint32
U16 = C.c_uint16
HANDLE = C.c_void_p
ULONG_PTR = C.c_size_t


class RECT(C.Structure):
    _fields_ = [(name, I32) for name in ('left', 'top', 'right', 'bottom')]


class POINT(C.Structure):
    _fields_ = [('x', I32), ('y', I32)]


class MOUSEINPUT(C.Structure):
    _fields_ = [('dx', I32), ('dy', I32), ('mouseData', U32), ('dwFlags', U32),
                ('time', U32), ('dwExtraInfo', ULONG_PTR)]


class KEYBDINPUT(C.Structure):
    _fields_ = [('wVk', U16), ('wScan', U16), ('dwFlags', U32),
                ('time', U32), ('dwExtraInfo', ULONG_PTR)]


class HARDWAREINPUT(C.Structure):
    _fields_ = [('uMsg', U32), ('wParamL', U16), ('wParamH', U16)]


class _INPUTUNION(C.Union):
    _fields_ = [('mi', MOUSEINPUT), ('ki', KEYBDINPUT), ('hi', HARDWAREINPUT)]


class INPUT(C.Structure):
    _anonymous_ = ('data',)
    _fields_ = [('type', U32), ('data', _INPUTUNION)]


class WinAPI:
    def __init__(self):
        if os.name != 'nt':
            raise OSError('Windows desktop APIs are only available on Windows')
        self.user32 = C.WinDLL('user32', use_last_error=True)
        u = self.user32
        self.WNDENUMPROC = C.WINFUNCTYPE(I32, HANDLE, C.c_ssize_t)
        signatures = {
            'EnumWindows': ([self.WNDENUMPROC, C.c_ssize_t], I32),
            'IsWindow': ([HANDLE], I32), 'IsWindowVisible': ([HANDLE], I32),
            'IsIconic': ([HANDLE], I32), 'GetWindowRect': ([HANDLE, C.POINTER(RECT)], I32),
            'GetWindowTextLengthW': ([HANDLE], I32),
            'GetWindowTextW': ([HANDLE, C.c_wchar_p, I32], I32),
            'GetClassNameW': ([HANDLE, C.c_wchar_p, I32], I32),
            'GetWindowThreadProcessId': ([HANDLE, C.POINTER(U32)], U32),
            'GetSystemMetrics': ([I32], I32),
            'SetForegroundWindow': ([HANDLE], I32), 'GetForegroundWindow': ([], HANDLE),
            'GetAncestor': ([HANDLE, U32], HANDLE), 'WindowFromPoint': ([POINT], HANDLE),
            'SendInput': ([U32, C.POINTER(INPUT), I32], U32),
        }
        for name, (args, result) in signatures.items():
            fn = getattr(u, name)
            fn.argtypes, fn.restype = args, result
        try:
            u.SetThreadDpiAwarenessContext.argtypes = [HANDLE]
            u.SetThreadDpiAwarenessContext.restype = HANDLE
        except AttributeError as exc:
            raise OSError('Windows 10 per-monitor DPI awareness is required') from exc

    @contextmanager
    def physical_pixels(self):
        # Per-monitor-aware v2 makes window, screen, mss and mouse coordinates
        # share one physical-pixel space even across mixed-DPI monitors.
        previous = self.user32.SetThreadDpiAwarenessContext(HANDLE(-4))
        if not previous:
            raise OSError('Could not enable per-monitor DPI awareness for this thread')
        try:
            yield
        finally:
            self.user32.SetThreadDpiAwarenessContext(previous)

    def send(self, event: INPUT):
        if self.user32.SendInput(1, C.byref(event), C.sizeof(INPUT)) != 1:
            raise OSError(C.get_last_error(), 'SendInput failed; input may be blocked by Windows integrity levels, the secure desktop, or permissions')

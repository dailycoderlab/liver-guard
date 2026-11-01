"""SendInput mouse and keyboard events in virtual-desktop coordinates."""
from __future__ import annotations

import math
import time

from ._native import INPUT, KEYBDINPUT, MOUSEINPUT
from .window import virtual_bounds

MOVE, LEFTDOWN, LEFTUP = 0x0001, 0x0002, 0x0004
VIRTUALDESK, ABSOLUTE = 0x4000, 0x8000
KEYUP, UNICODE, EXTENDED = 0x0002, 0x0004, 0x0001
KEYS = {'backspace': 0x08, 'tab': 0x09, 'return': 0x0d, 'enter': 0x0d,
        'escape': 0x1b, 'esc': 0x1b, 'space': 0x20, 'pageup': 0x21,
        'pagedown': 0x22, 'end': 0x23, 'home': 0x24, 'left': 0x25,
        'up': 0x26, 'right': 0x27, 'down': 0x28, 'insert': 0x2d, 'delete': 0x2e}
KEYS.update({f'f{i}': 0x6f + i for i in range(1, 13)})
EXTENDED_KEYS = {0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x2d, 0x2e}


class SendInputInjector:
    def __init__(self, api):
        self.api = api

    def _mouse(self, flags, x=0, y=0):
        event = INPUT(type=0, mi=MOUSEINPUT(dx=x, dy=y, dwFlags=flags))
        self.api.send(event)

    def move(self, x, y):
        rect = virtual_bounds(self.api)
        if not math.isfinite(x) or not math.isfinite(y) or not (rect.x <= x < rect.x + rect.w and rect.y <= y < rect.y + rect.h):
            raise ValueError('point is outside the virtual desktop')
        # Aim at the middle of the requested pixel's 16-bit coordinate bin.
        dx = min(65535, int((math.floor(x) - rect.x + 0.5) * 65536 / rect.w))
        dy = min(65535, int((math.floor(y) - rect.y + 0.5) * 65536 / rect.h))
        self._mouse(MOVE | ABSOLUTE | VIRTUALDESK, dx, dy)

    def tap(self, x, y, duration_ms=60):
        self.move(x, y)
        self._mouse(LEFTDOWN)
        try:
            time.sleep(max(0, duration_ms) / 1000)
        finally:
            self._mouse(LEFTUP)

    def swipe(self, x1, y1, x2, y2, duration_ms=300, check=None):
        self.move(x1, y1)
        self._mouse(LEFTDOWN)
        try:
            steps = max(2, min(120, round(max(0, duration_ms) / 16)))
            for i in range(1, steps + 1):
                x, y = x1 + (x2 - x1) * i / steps, y1 + (y2 - y1) * i / steps
                if check is not None:
                    check(x, y)
                self.move(x, y)
                time.sleep(max(0, duration_ms) / 1000 / steps)
        finally:
            self._mouse(LEFTUP)

    def _key(self, vk, scan, flags):
        self.api.send(INPUT(type=1, ki=KEYBDINPUT(wVk=vk, wScan=scan, dwFlags=flags)))

    def key(self, name, check=None):
        key = name.strip().lower()
        code = KEYS.get(key)
        if code is None:
            if len(name) == 1:
                self.text(name, check=check)
                return
            raise ValueError(f'unknown key: {name}')
        flags = EXTENDED if code in EXTENDED_KEYS else 0
        self._key(code, 0, flags)
        self._key(code, 0, flags | KEYUP)

    def text(self, value, check=None):
        # KEYEVENTF_UNICODE uses UTF-16 code units, including surrogate pairs.
        encoded = value.encode('utf-16-le')
        for i in range(0, len(encoded), 2):
            if check is not None:
                check()
            unit = int.from_bytes(encoded[i:i + 2], 'little')
            self._key(0, unit, UNICODE)
            self._key(0, unit, UNICODE | KEYUP)

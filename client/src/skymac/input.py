"""Input injection through Quartz CGEvents.

Coordinates are global logical points with the origin at the top-left of the main
display - the same space ``CGWindowListCopyWindowInfo`` and ``AXUIElement*``
report, which is why no conversion is needed between them.
"""

from __future__ import annotations

import time

# macOS virtual key codes for the keys a script realistically needs.
KEYCODES = {
    "return": 36,
    "enter": 36,
    "tab": 48,
    "space": 49,
    "delete": 51,
    "backspace": 51,
    "escape": 53,
    "esc": 53,
    "left": 123,
    "right": 124,
    "down": 125,
    "up": 126,
    "home": 115,
    "end": 119,
    "pageup": 116,
    "pagedown": 121,
    "f1": 122,
    "f2": 120,
    "f3": 99,
    "f4": 118,
    "f5": 96,
    "f6": 97,
    "f7": 98,
    "f8": 100,
    "f9": 101,
    "f10": 109,
    "f11": 103,
    "f12": 111,
}


class QuartzInput:
    def __init__(self) -> None:
        import Quartz  # noqa: F401  (imported here so the module stays importable elsewhere)

        self._quartz = Quartz

    # ------------------------------------------------------------------ mouse

    def move(self, x: float, y: float) -> None:
        quartz = self._quartz
        event = quartz.CGEventCreateMouseEvent(None, quartz.kCGEventMouseMoved, (x, y), quartz.kCGMouseButtonLeft)
        quartz.CGEventPost(quartz.kCGHIDEventTap, event)

    def tap(self, x: float, y: float, duration_ms: int = 60) -> None:
        quartz = self._quartz
        self.move(x, y)
        down = quartz.CGEventCreateMouseEvent(None, quartz.kCGEventLeftMouseDown, (x, y), quartz.kCGMouseButtonLeft)
        quartz.CGEventPost(quartz.kCGHIDEventTap, down)
        time.sleep(max(1, duration_ms) / 1000.0)
        up = quartz.CGEventCreateMouseEvent(None, quartz.kCGEventLeftMouseUp, (x, y), quartz.kCGMouseButtonLeft)
        quartz.CGEventPost(quartz.kCGHIDEventTap, up)

    def swipe(self, x1: float, y1: float, x2: float, y2: float, duration_ms: int = 300, steps: int = 12) -> None:
        quartz = self._quartz
        steps = max(2, steps)
        self.move(x1, y1)
        down = quartz.CGEventCreateMouseEvent(None, quartz.kCGEventLeftMouseDown, (x1, y1), quartz.kCGMouseButtonLeft)
        quartz.CGEventPost(quartz.kCGHIDEventTap, down)

        step_delay = max(0.004, (max(50, duration_ms) / 1000.0) / steps)
        for index in range(1, steps + 1):
            ratio = index / steps
            x = x1 + (x2 - x1) * ratio
            y = y1 + (y2 - y1) * ratio
            dragged = quartz.CGEventCreateMouseEvent(
                None, quartz.kCGEventLeftMouseDragged, (x, y), quartz.kCGMouseButtonLeft
            )
            quartz.CGEventPost(quartz.kCGHIDEventTap, dragged)
            time.sleep(step_delay)

        up = quartz.CGEventCreateMouseEvent(None, quartz.kCGEventLeftMouseUp, (x2, y2), quartz.kCGMouseButtonLeft)
        quartz.CGEventPost(quartz.kCGHIDEventTap, up)

    # --------------------------------------------------------------- keyboard

    def key(self, name: str) -> None:
        quartz = self._quartz
        key = name.strip().lower()
        code = KEYCODES.get(key)
        if code is None:
            if len(key) != 1:
                raise ValueError(f"unknown key: {name}")
            self.text(name)
            return

        down = quartz.CGEventCreateKeyboardEvent(None, code, True)
        quartz.CGEventPost(quartz.kCGHIDEventTap, down)
        time.sleep(0.02)
        up = quartz.CGEventCreateKeyboardEvent(None, code, False)
        quartz.CGEventPost(quartz.kCGHIDEventTap, up)

    def text(self, value: str) -> None:
        """Type a string one character at a time.

        Sending the whole string in a single event is faster but games commonly
        drop characters without a delay between them.
        """
        quartz = self._quartz
        for char in value:
            down = quartz.CGEventCreateKeyboardEvent(None, 0, True)
            quartz.CGEventKeyboardSetUnicodeString(down, 1, char)
            quartz.CGEventPost(quartz.kCGHIDEventTap, down)
            up = quartz.CGEventCreateKeyboardEvent(None, 0, False)
            quartz.CGEventKeyboardSetUnicodeString(up, 1, char)
            quartz.CGEventPost(quartz.kCGHIDEventTap, up)
            time.sleep(0.012)

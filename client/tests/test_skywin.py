import ctypes
import unittest
from contextlib import nullcontext
from types import SimpleNamespace
from unittest.mock import patch

import numpy as np

from skycore.driver import Driver
from skycore.geometry import Rect
from skywin import SurfaceUnavailable, WindowsDriver, driver, window
from skywin._native import INPUT, KEYBDINPUT, MOUSEINPUT
from skywin.input import ABSOLUTE, KEYUP, UNICODE, VIRTUALDESK, SendInputInjector


class API:
    def __init__(self):
        self.events = []
        self.hit = 42
        self.foreground = 42
        self.user32 = SimpleNamespace(
            GetSystemMetrics=lambda n: {76: -1920, 77: -200, 78: 3840, 79: 1280}[n],
            SetForegroundWindow=lambda hwnd: 1,
            GetForegroundWindow=lambda: self.foreground,
            GetAncestor=lambda hwnd, kind: hwnd,
            WindowFromPoint=lambda point: self.hit,
        )

    def physical_pixels(self):
        return nullcontext()

    def send(self, event):
        self.events.append(event)


class Capture:
    def grab(self, rect):
        return np.zeros((int(rect.h), int(rect.w), 3), dtype=np.uint8)

    def close(self):
        pass


class WindowsDriverTests(unittest.TestCase):
    def setUp(self):
        self.api = API()
        self.patch_api = patch.object(driver, 'WinAPI', return_value=self.api)
        self.patch_capture = patch.object(driver, 'MssCapturer', return_value=Capture())
        self.patch_api.start()
        self.patch_capture.start()
        self.addCleanup(self.patch_api.stop)
        self.addCleanup(self.patch_capture.stop)
        self.current = window.Window(42, 7, 8, 'Test', 'Target', Rect(-100, 20, 80, 60))
        self.lookup = patch.object(window, 'find_window', side_effect=lambda *args: self.current)
        self.handle = patch.object(window, 'window_by_handle', side_effect=lambda *args: self.current)
        self.lookup.start()
        self.handle.start()
        self.addCleanup(self.lookup.stop)
        self.addCleanup(self.handle.stop)

    def test_enumeration_filters_and_selects_titles(self):
        self.lookup.stop()
        candidates = {
            1: window.Window(1, 11, 21, 'Class', 'Calculator', Rect(0, 0, 20, 20)),
            2: window.Window(2, 12, 22, 'Class', '', Rect(0, 0, 20, 20)),
            3: None,
        }
        self.api.WNDENUMPROC = lambda callback: callback
        def enum(callback, arg):
            for hwnd in candidates:
                callback(hwnd, arg)
            return 1
        self.api.user32.EnumWindows = enum
        with patch.object(window, 'window_by_handle', side_effect=lambda api, hwnd: candidates[hwnd]):
            self.assertEqual([w.hwnd for w in window.list_windows(self.api)], [1])
            self.assertEqual(window.find_window(self.api, 'CULA').hwnd, 1)
            self.assertIsNone(window.find_window(self.api, 'unknown'))
            with self.assertRaises(ValueError):
                window.find_window(self.api, '  ')

    def test_window_capture_and_caps(self):
        d = WindowsDriver(window_query='target')
        self.assertIsInstance(d, Driver)
        frame = d.capture()
        self.assertEqual(frame.image.shape, (60, 80, 3))
        self.assertEqual(frame.bounds.rect, Rect(-100, 20, 80, 60))
        self.assertEqual(frame.scale, 1)
        self.assertEqual(d.caps().ui_query, 'none')
        self.assertEqual(d.caps().surface, 'window')
        self.assertTrue(d.describe()['flat_image'])

    def test_window_loss_and_handle_reuse_never_fall_back(self):
        d = WindowsDriver(window_query='target')
        d.bounds()
        self.current = None
        for operation in (d.bounds, d.capture, lambda: d.tap(0, 0), lambda: d.key('return'), lambda: d.text('a')):
            with self.assertRaises(SurfaceUnavailable):
                operation()
        self.current = window.Window(42, 99, 8, 'Test', 'Other', Rect(0, 0, 80, 60))
        with self.assertRaises(SurfaceUnavailable):
            d.bounds()
        self.assertFalse(self.api.events)

    def test_window_input_requires_foreground_and_hit(self):
        d = WindowsDriver(window_query='target', activate_window=False)
        self.api.foreground = 77
        with self.assertRaises(SurfaceUnavailable):
            d.key('return')
        self.api.foreground = 42
        self.api.hit = 88
        with self.assertRaises(SurfaceUnavailable):
            d.tap(-50, 30, 0)
        self.api.hit = 42
        with self.assertRaises(ValueError):
            d.tap(10, 30, 0)
        d.tap(-50, 30, 0)
        self.assertEqual(len(self.api.events), 3)

    def test_clipping_and_moving_surface(self):
        self.current = window.Window(42, 7, 8, 'Test', 'Target', Rect(-1950, -220, 100, 60))
        d = WindowsDriver(window_query='target')
        self.assertEqual(d.bounds().rect, Rect(-1920, -200, 70, 40))
        def moving(rect):
            self.current = window.Window(42, 7, 8, 'Test', 'Target', Rect(-1900, -200, 100, 60))
            return np.zeros((int(rect.h), int(rect.w), 3), dtype=np.uint8)
        d.capturer.grab = moving
        with self.assertRaises(SurfaceUnavailable):
            d.capture()

    def test_explicit_fullscreen(self):
        with self.assertRaises(ValueError):
            WindowsDriver()
        d = WindowsDriver(full_screen=True)
        self.assertEqual(d.bounds().rect, Rect(-1920, -200, 3840, 1280))
        self.assertEqual(d.caps().surface, 'screen')

    def test_virtual_desktop_and_unicode_input(self):
        injector = SendInputInjector(self.api)
        injector.move(-1920, -200)
        e = self.api.events[-1].mi
        self.assertEqual((e.dx, e.dy), (int(0.5 * 65536 / 3840), int(0.5 * 65536 / 1280)))
        self.assertTrue(e.dwFlags & ABSOLUTE and e.dwFlags & VIRTUALDESK)
        injector.move(1919, 1079)
        e = self.api.events[-1].mi
        self.assertEqual((e.dx, e.dy), (int(3839.5 * 65536 / 3840), int(1279.5 * 65536 / 1280)))
        with self.assertRaises(ValueError):
            injector.move(1920, 0)
        self.api.events.clear()
        injector.text('A😀')
        keys = [(e.ki.wScan, e.ki.dwFlags) for e in self.api.events]
        self.assertEqual(keys, [(65, UNICODE), (65, UNICODE | KEYUP),
                                (0xd83d, UNICODE), (0xd83d, UNICODE | KEYUP),
                                (0xde00, UNICODE), (0xde00, UNICODE | KEYUP)])

    def test_drag_releases_button_if_interrupted(self):
        injector = SendInputInjector(self.api)
        def fail(x, y):
            raise SurfaceUnavailable('gone')
        with self.assertRaises(SurfaceUnavailable):
            injector.swipe(-100, 20, -50, 30, 0, check=fail)
        self.assertEqual(self.api.events[-1].mi.dwFlags, 4)

    def test_native_input_layout(self):
        self.assertEqual(ctypes.sizeof(KEYBDINPUT), 24 if ctypes.sizeof(ctypes.c_void_p) == 8 else 16)
        self.assertEqual(ctypes.sizeof(MOUSEINPUT), 32 if ctypes.sizeof(ctypes.c_void_p) == 8 else 24)
        self.assertEqual(ctypes.sizeof(INPUT), 40 if ctypes.sizeof(ctypes.c_void_p) == 8 else 28)


if __name__ == '__main__':
    unittest.main()

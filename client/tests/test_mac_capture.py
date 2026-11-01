"""macOS selected-window capture without accessing a real screen."""
import sys
from types import SimpleNamespace

import numpy as np
import pytest

from skycore.geometry import Rect
from skymac.capture import MssCapturer
from skymac.driver import MacDriver, Surface


def test_window_capture_requests_only_window_and_handles_row_padding(monkeypatch):
    calls = []
    pixels = bytes([1, 2, 3, 255, 4, 5, 6, 255, 99, 99, 99, 99,
                    7, 8, 9, 255, 10, 11, 12, 255, 99, 99, 99, 99])
    quartz = SimpleNamespace(
        kCGWindowImageBoundsIgnoreFraming=1, kCGWindowImageShouldBeOpaque=2,
        kCGWindowImageNominalResolution=16, kCGWindowListOptionIncludingWindow=8,
        CGRectMake=lambda *args: args,
        CGWindowListCreateImage=lambda *args: calls.append(args) or 'image',
        CGImageGetWidth=lambda image: 2,
        CGImageGetHeight=lambda image: 2,
        CGImageGetBytesPerRow=lambda image: 12,
        CGImageGetBitsPerPixel=lambda image: 32,
        CGImageGetDataProvider=lambda image: 'provider',
        CGDataProviderCopyData=lambda provider: pixels,
    )
    monkeypatch.setitem(sys.modules, 'Quartz', quartz)
    image = MssCapturer().grab_window(Rect(100, 200, 2, 2), 42)
    assert calls == [((100, 200, 2, 2), 8, 42, 19)]
    assert image.tolist() == [[[1, 2, 3], [4, 5, 6]], [[7, 8, 9], [10, 11, 12]]]
    assert image.flags.owndata


def test_selected_window_capture_uses_window_id_and_actual_scale(monkeypatch):
    driver = MacDriver(window_number=42, full_screen=False)
    surface = Surface(Rect(50, 80, 100, 60), 'app', 42, 123)
    monkeypatch.setattr(driver, 'surface', lambda: surface)
    calls = []
    monkeypatch.setattr(driver.capturer, 'grab_window',
                        lambda rect, number: calls.append((rect, number)) or np.zeros((120, 200, 3), np.uint8))
    monkeypatch.setattr(driver.capturer, 'grab', lambda rect: pytest.fail('must not capture desktop'))
    frame = driver.capture()
    assert calls == [(surface.rect, 42)]
    assert frame.image.shape == (120, 200, 3)
    assert frame.bounds.rect == surface.rect
    assert frame.scale == frame.bounds.scale == 2
    assert driver.bounds().scale == 2


def test_window_capture_failure_never_falls_back_to_screen(monkeypatch):
    driver = MacDriver(window_number=42, full_screen=False)
    surface = Surface(Rect(50, 80, 100, 60), 'app', 42, 123)
    monkeypatch.setattr(driver, 'surface', lambda: surface)
    def fail(rect, number):
        raise RuntimeError('capture failed')
    monkeypatch.setattr(driver.capturer, 'grab_window', fail)
    monkeypatch.setattr(driver.capturer, 'grab', lambda rect: pytest.fail('must not capture desktop'))
    with pytest.raises(RuntimeError, match='capture failed'):
        driver.capture()
    with pytest.raises(RuntimeError, match='capture failed'):
        driver.bounds()


def test_window_move_during_capture_is_rejected(monkeypatch):
    driver = MacDriver(window_number=42, full_screen=False)
    surfaces = iter([Surface(Rect(0, 0, 100, 60), 'app', 42, 123),
                     Surface(Rect(10, 0, 100, 60), 'app', 42, 123)])
    monkeypatch.setattr(driver, 'surface', lambda: next(surfaces))
    monkeypatch.setattr(driver.capturer, 'grab_window', lambda rect, number: np.zeros((60, 100, 3), np.uint8))
    with pytest.raises(RuntimeError, match='发生变化'):
        driver.capture()


def test_fullscreen_still_uses_screen_capture(monkeypatch):
    driver = MacDriver(full_screen=True)
    surface = Surface(Rect(0, 0, 100, 60), 'display')
    monkeypatch.setattr(driver, 'surface', lambda: surface)
    monkeypatch.setattr(driver.capturer, 'grab', lambda rect: np.zeros((60, 100, 3), np.uint8))
    monkeypatch.setattr(driver.capturer, 'grab_window', lambda *args: pytest.fail('not a window'))
    assert driver.capture().bounds.rect == surface.rect

"""Offline Qt integration test for the capture-to-draft recording flow."""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import numpy as np
from PySide6.QtWidgets import QApplication, QDialog

from skycore.frame import Frame
from skycore.geometry import Bounds
from skygui.main_window import MainWindow, blank_ir


def test_record_click_captures_before_input_and_saves_asset(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    events = []
    bounds = Bounds(0, 0, 100, 100)
    image = np.zeros((100, 100, 3), dtype=np.uint8)
    image[20:80, 20:80] = [10, 120, 230]
    frame = Frame(image=image, bounds=bounds)

    class Driver:
        def bounds(self): return bounds
        def activate(self): return True
        def capture(self):
            events.append('capture')
            return frame
        def ax_at(self, x, y): return None
        def tap(self, x, y): events.append('tap')

    class Dialog:
        def __init__(self, *args): pass
        def exec(self): return QDialog.DialogCode.Accepted
        def selection(self): return (20, 20, 60, 60)

    monkeypatch.setattr(gui.platform, 'system', lambda: 'test')
    monkeypatch.setattr(gui, 'CropDialog', Dialog)
    window = MainWindow()
    window.driver = Driver()
    window.script = window.store.create_script('test', blank_ir('test'))
    window.ir = blank_ir('test')
    window.last_frame = frame
    monkeypatch.setattr(window, 'task', lambda fn, done=None, failed=None: done(fn()) if done else fn())
    # 录制要求先进入编辑模式。
    # Recording requires entering edit mode first.
    window.edit_button.setChecked(True)
    window.record_click(.5, .5)
    assert events == ['capture', 'tap']
    assert window.ir['blocks']['main'][0]['op'] == 'tap'
    target = next(iter(window.ir['targets'].values()))
    assert (tmp_path / 'assets' / (target['asset'] + '.png')).exists()
    assert window.store.get_script(window.script['id'])['draft'] == window.ir
    window._quitting = True
    window.close()
    app.processEvents()

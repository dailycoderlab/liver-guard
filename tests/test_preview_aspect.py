"""预览面板按目标窗口比例调整，避免出现黑边。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import numpy as np
import pytest
from PySide6.QtWidgets import QApplication

from skycore.frame import Frame
from skycore.geometry import Bounds


@pytest.fixture(scope='module')
def app():
    return QApplication.instance() or QApplication([])


@pytest.fixture
def window(tmp_path, monkeypatch, app):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    built = gui.MainWindow()
    built.preview_timer.stop()
    built.resize(1250, 760)
    built.show()
    for _ in range(3):
        app.processEvents()
    yield built
    built.pending_save = False
    built._quitting = True
    built.close()
    app.processEvents()


def feed(window, width, height, app):
    image = np.zeros((height, width, 3), dtype=np.uint8)
    image[:, :] = (200, 180, 160)
    window.live.set_frame(Frame(image, Bounds(0, 0, width, height, 1)))
    window.fit_preview_aspect()
    for _ in range(4):
        app.processEvents()


def fill_ratio(window):
    """绘制区域占控件面积的比例；接近 1 说明没有黑边。"""
    rect = window.live.image_rect()
    total = max(1, window.live.width() * window.live.height())
    return (rect.width() * rect.height()) / total


@pytest.mark.parametrize('width,height', [(390, 844), (1920, 1080), (500, 500), (1200, 600)])
def test_preview_fills_its_area_for_common_aspects(window, app, width, height):
    feed(window, width, height, app)
    # 允许取整误差；有黑边时会明显低于这个值。
    assert fill_ratio(window) > 0.97


def test_preview_recovers_after_switching_targets(window, app):
    feed(window, 390, 844, app)   # 竖屏
    assert fill_ratio(window) > 0.97
    feed(window, 1200, 600, app)  # 宽屏
    assert fill_ratio(window) > 0.97
    feed(window, 390, 844, app)   # 再切回竖屏
    assert fill_ratio(window) > 0.97


def test_no_fit_before_the_first_frame(window):
    assert window.live.aspect() is None
    window.fit_preview_aspect()  # 不应抛错，也不应改动布局
    assert window.preview_splitter.sizes()


def test_status_pane_keeps_a_readable_height(window, app):
    feed(window, 390, 844, app)
    sizes = window.preview_splitter.sizes()
    assert sizes[1] >= 100


def test_live_view_reports_its_aspect(app):
    from skygui.live_view import LiveView
    view = LiveView()
    assert view.aspect() is None
    image = np.zeros((50, 100, 3), dtype=np.uint8)
    view.set_frame(Frame(image, Bounds(0, 0, 100, 50, 1)))
    assert view.aspect() == pytest.approx(2.0)

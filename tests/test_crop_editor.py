"""录制时的模板选区：坐标换算与红色选区框线。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import numpy as np
import pytest
from PySide6.QtCore import QPointF
from PySide6.QtGui import QColor
from PySide6.QtWidgets import QApplication

from skygui.crop_editor import (
    SELECTION_COLOR,
    SELECTION_OUTLINE,
    CropCanvas,
    CropDialog,
)


@pytest.fixture(scope='module')
def app():
    return QApplication.instance() or QApplication([])


def make_canvas(app, width=200, height=100, box=(10, 20, 40, 30)):
    image = np.zeros((height, width, 3), dtype=np.uint8)
    image[:, :] = (120, 140, 160)
    canvas = CropCanvas(image, box)
    canvas.resize(460, 320)
    canvas.show()
    app.processEvents()
    return canvas


def test_selection_uses_red_not_yellow():
    assert SELECTION_COLOR == QColor(255, 45, 45)
    assert SELECTION_COLOR.green() < 64  # 不再是黄色
    # A dark outline keeps the red readable on light and on reddish screenshots.
    assert SELECTION_OUTLINE.lightness() < 80


def test_selection_rect_maps_into_the_drawn_image(app):
    canvas = make_canvas(app)
    rect = canvas.box_rect()
    drawn = canvas.image_rect()
    assert not rect.isEmpty()
    # The selection must stay inside the drawn image regardless of letterboxing.
    assert rect.x() >= drawn.x() - 0.5
    assert rect.y() >= drawn.y() - 0.5
    assert rect.right() <= drawn.right() + 0.5
    assert rect.bottom() <= drawn.bottom() + 0.5
    # 40x30 crop out of a 200x100 image keeps its 4:3 proportion on screen.
    assert rect.width() / rect.height() == pytest.approx(40 / 30, rel=0.02)
    canvas.close()


def test_paint_marks_red_pixels_inside_the_selection(app):
    canvas = make_canvas(app)
    pixmap = canvas.grab()
    image = pixmap.toImage()
    found = False
    for y in range(image.height()):
        for x in range(image.width()):
            colour = image.pixelColor(x, y)
            if colour.red() > 200 and colour.green() < 90 and colour.blue() < 90:
                found = True
                break
        if found:
            break
    assert found, '选区框线没有画出红色像素'
    canvas.close()


def test_dragging_updates_the_selection(app):
    canvas = make_canvas(app)
    start = canvas.image_rect().topLeft() + QPointF(30, 30)
    canvas.start = canvas.point(start)
    canvas.box = type(canvas.box)(canvas.start, canvas.point(start + QPointF(60, 40))).normalized()
    canvas.start = None
    assert canvas.box.width() > 0 and canvas.box.height() > 0
    canvas.close()


def test_dialog_selection_returns_integers(app):
    image = np.zeros((80, 120, 3), dtype=np.uint8)
    dialog = CropDialog(image, (4, 6, 20, 10))
    selection = dialog.selection()
    assert len(selection) == 4
    assert all(isinstance(value, int) for value in selection)
    assert selection == (4, 6, 20, 10)
    dialog.close()

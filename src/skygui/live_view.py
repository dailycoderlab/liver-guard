"""Display a captured frame and return clicks in image-relative coordinates."""
import cv2
from PySide6.QtCore import QRectF, QSize, Qt, Signal
from PySide6.QtGui import QImage, QPainter
from PySide6.QtWidgets import QWidget


def to_qimage(bgr):
    rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
    h, w = rgb.shape[:2]
    return QImage(rgb.data, w, h, rgb.strides[0], QImage.Format.Format_RGB888).copy()


class LiveView(QWidget):
    clicked = Signal(float, float)

    def __init__(self):
        super().__init__()
        self.image = None
        # 非编辑状态下画面仍然显示，但点上去不会有任何效果，因此光标要保持
        # 普通箭头，避免让人以为可以点。
        # Outside edit mode the frame is still shown but clicks do nothing, so the
        # cursor stays a plain arrow instead of inviting a click.
        self.editable = False
        self.setCursor(Qt.CursorShape.ArrowCursor)
        # 预览会被调成目标窗口的比例，最小尺寸必须足够小：极宽的横幅窗口只需要
        # 一百多像素高，极窄的竖窗口只需要一百多像素宽，卡大了就会被迫留黑边。
        # The pane is resized to the target's aspect ratio, so the minimum size has
        # to stay small: a wide banner needs ~130 px of height and a narrow
        # portrait ~160 px of width, and a larger floor forces letterboxing.
        self.setMinimumSize(120, 90)

    def set_editable(self, editable: bool) -> None:
        if self.editable == bool(editable):
            return
        self.editable = bool(editable)
        self.setCursor(Qt.CursorShape.CrossCursor if self.editable else Qt.CursorShape.ArrowCursor)
        self.update()

    def set_frame(self, frame):
        self.image = to_qimage(frame.image)
        self.update()

    def aspect(self) -> float | None:
        """Width / height of the displayed image, or None before the first frame."""
        if self.image is None or self.image.height() <= 0:
            return None
        return self.image.width() / self.image.height()

    def sizeHint(self):
        aspect = self.aspect()
        if aspect is None:
            return super().sizeHint()
        return QSize(round(320 * aspect), 320)

    def image_rect(self):
        if self.image is None:
            return QRectF()
        size = self.image.size().scaled(self.size(), Qt.AspectRatioMode.KeepAspectRatio)
        return QRectF((self.width() - size.width()) / 2, (self.height() - size.height()) / 2, size.width(), size.height())

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), Qt.GlobalColor.black)
        if self.image is not None:
            painter.drawImage(self.image_rect(), self.image)
        else:
            painter.setPen(Qt.GlobalColor.white)
            painter.drawText(self.rect(), Qt.AlignmentFlag.AlignCenter, '请选择目标窗口并启动预览')

    def mousePressEvent(self, event):
        rect = self.image_rect()
        if event.button() == Qt.MouseButton.LeftButton and not rect.isEmpty() and rect.contains(event.position()):
            self.clicked.emit((event.position().x() - rect.x()) / rect.width(),
                              (event.position().y() - rect.y()) / rect.height())

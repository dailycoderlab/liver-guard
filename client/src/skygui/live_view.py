"""Display a captured frame and return clicks in image-relative coordinates."""
import cv2
from PySide6.QtCore import Qt, QRectF, Signal
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
        self.setMinimumSize(360, 220)

    def set_frame(self, frame):
        self.image = to_qimage(frame.image)
        self.update()

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

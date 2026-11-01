"""Interactive selection of the template inside a pre-click capture."""
from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QPainter, QPen
from PySide6.QtWidgets import QDialog, QDialogButtonBox, QLabel, QVBoxLayout, QWidget

from .live_view import to_qimage


class CropCanvas(QWidget):
    def __init__(self, image, initial):
        super().__init__()
        self.image = to_qimage(image)
        self.box = QRectF(*initial)
        self.start = None
        self.setMinimumSize(480, 320)

    def image_rect(self):
        size = self.image.size().scaled(self.size(), Qt.AspectRatioMode.KeepAspectRatio)
        return QRectF((self.width()-size.width())/2, (self.height()-size.height())/2, size.width(), size.height())

    def point(self, p):
        r = self.image_rect()
        return QPointF(max(0, min(self.image.width(), (p.x()-r.x())*self.image.width()/r.width())),
                       max(0, min(self.image.height(), (p.y()-r.y())*self.image.height()/r.height())))

    def paintEvent(self, event):
        p = QPainter(self)
        p.fillRect(self.rect(), Qt.GlobalColor.black)
        r = self.image_rect()
        p.drawImage(r, self.image)
        p.setPen(QPen(Qt.GlobalColor.yellow, 2))
        p.drawRect(QRectF(r.x()+self.box.x()*r.width()/self.image.width(),
                          r.y()+self.box.y()*r.height()/self.image.height(),
                          self.box.width()*r.width()/self.image.width(),
                          self.box.height()*r.height()/self.image.height()))

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton and self.image_rect().contains(event.position()):
            self.start = self.point(event.position())

    def mouseMoveEvent(self, event):
        if self.start is not None:
            self.box = QRectF(self.start, self.point(event.position())).normalized()
            self.update()

    def mouseReleaseEvent(self, event):
        if self.start is not None:
            self.box = QRectF(self.start, self.point(event.position())).normalized()
            self.start = None
            self.update()


class CropDialog(QDialog):
    def __init__(self, image, initial, parent=None):
        super().__init__(parent)
        self.setWindowTitle('选择模板区域')
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel('拖动鼠标重新选择模板区域。'))
        self.canvas = CropCanvas(image, initial)
        layout.addWidget(self.canvas)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def selection(self):
        rect = self.canvas.box
        return tuple(round(x) for x in (rect.x(), rect.y(), rect.width(), rect.height()))

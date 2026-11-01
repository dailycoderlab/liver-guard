"""Interactive selection of the template inside a pre-click capture."""
from PySide6.QtCore import QPointF, QRectF, Qt
from PySide6.QtGui import QColor, QPainter, QPen
from PySide6.QtWidgets import QDialog, QDialogButtonBox, QLabel, QVBoxLayout, QWidget

from .live_view import to_qimage

# 选区框线：红色主色 + 深色描边。纯红色线在浅色或红色调的界面截图里会糊成
# 一片，加一圈深色外描边后无论底下是什么颜色都能看清边界。
# Red with a dark outline: a plain red line disappears on light or reddish
# screenshots, and the outline keeps the boundary readable on any background.
SELECTION_COLOR = QColor(255, 45, 45)
SELECTION_OUTLINE = QColor(20, 20, 20)
SELECTION_WIDTH = 2.0


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

    def box_rect(self) -> QRectF:
        """选区在控件坐标里的矩形。"""
        r = self.image_rect()
        if r.isEmpty() or self.image.width() <= 0 or self.image.height() <= 0:
            return QRectF()
        sx = r.width() / self.image.width()
        sy = r.height() / self.image.height()
        return QRectF(r.x() + self.box.x() * sx, r.y() + self.box.y() * sy,
                      self.box.width() * sx, self.box.height() * sy)

    def point(self, p):
        r = self.image_rect()
        return QPointF(max(0, min(self.image.width(), (p.x()-r.x())*self.image.width()/r.width())),
                       max(0, min(self.image.height(), (p.y()-r.y())*self.image.height()/r.height())))

    def paintEvent(self, event):
        p = QPainter(self)
        p.fillRect(self.rect(), Qt.GlobalColor.black)
        p.drawImage(self.image_rect(), self.image)
        rect = self.box_rect()
        if rect.isEmpty():
            return
        p.setRenderHint(QPainter.RenderHint.Antialiasing, False)
        # 先画深色外圈再压红色内圈：两层都从同一个矩形外扩，边线不会错位。
        # Dark base first, red on top; both derive from one rect so they stay aligned.
        p.setPen(QPen(SELECTION_OUTLINE, SELECTION_WIDTH + 2))
        p.drawRect(rect)
        p.setPen(QPen(SELECTION_COLOR, SELECTION_WIDTH))
        p.drawRect(rect)

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

"""放大查看一张模板素材（点击区域截图）。"""
from __future__ import annotations

from PySide6.QtCore import QRectF, Qt
from PySide6.QtGui import QPainter, QPixmap
from PySide6.QtWidgets import QDialog, QDialogButtonBox, QLabel, QVBoxLayout, QWidget


class AssetCanvas(QWidget):
    """等比铺满画布；素材通常只有几十像素，直接按原尺寸画会小到看不清。"""

    def __init__(self, pixmap: QPixmap, parent=None):
        super().__init__(parent)
        self.pixmap = pixmap

    def target_rect(self) -> QRectF:
        if self.pixmap.isNull():
            return QRectF()
        size = self.pixmap.size().scaled(self.size(), Qt.AspectRatioMode.KeepAspectRatio)
        return QRectF((self.width() - size.width()) / 2, (self.height() - size.height()) / 2,
                      size.width(), size.height())

    def paintEvent(self, event):
        painter = QPainter(self)
        painter.fillRect(self.rect(), Qt.GlobalColor.black)
        if not self.pixmap.isNull():
            # 最近邻放大：模板本身就是像素图，线性插值会把边界糊掉，
            # 反而不利于确认裁到的是不是想要的那块控件。
            # Nearest-neighbour on purpose: smoothing blurs the exact region
            # boundary the user is trying to confirm.
            painter.setRenderHint(QPainter.RenderHint.SmoothPixmapTransform, False)
            painter.drawPixmap(self.target_rect(), self.pixmap, QRectF(self.pixmap.rect()))


class AssetViewer(QDialog):
    def __init__(self, pixmap: QPixmap, caption: str = '', parent=None):
        super().__init__(parent)
        self.setWindowTitle('点击区域')
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel(caption))
        self.canvas = AssetCanvas(pixmap)
        # 至少给到素材的 4 倍显示面积，同时不超过屏幕可用空间的合理比例。
        # At least 4x the source size, bounded so it always fits on screen.
        width = max(360, min(900, pixmap.width() * 4))
        height = max(240, min(600, pixmap.height() * 4))
        self.canvas.setMinimumSize(width, height)
        layout.addWidget(self.canvas, 1)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Close)
        buttons.rejected.connect(self.reject)
        buttons.accepted.connect(self.accept)
        layout.addWidget(buttons)
        self.resize(width + 40, height + 90)

"""List visible application windows and let the user choose one."""
from dataclasses import dataclass
import os
import platform

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog, QDialogButtonBox, QLabel, QListWidget, QListWidgetItem,
    QPushButton, QVBoxLayout,
)


@dataclass(frozen=True)
class WindowChoice:
    number: int
    pid: int
    label: str


def available_windows():
    system = platform.system()
    if system == 'Darwin':
        from skymac.window import list_windows
        return [
            WindowChoice(w.number, w.pid,
                         f"{w.owner or '未知应用'} — {w.title or '无标题'}  "
                         f"({w.bounds.w:.0f} × {w.bounds.h:.0f})  #{w.number}")
            for w in list_windows() if w.pid != os.getpid() and w.number
        ]
    if system == 'Windows':
        from skywin._native import WinAPI
        from skywin.window import list_windows
        api = WinAPI()
        with api.physical_pixels():
            windows = list_windows(api)
        return [
            WindowChoice(w.hwnd, w.pid,
                         f"{w.title}  ({w.bounds.w:.0f} × {w.bounds.h:.0f})  #{w.hwnd}")
            for w in windows if w.pid != os.getpid()
        ]
    raise RuntimeError('当前仅支持 macOS 和 Windows')


class WindowPicker(QDialog):
    def __init__(self, parent=None, provider=None):
        super().__init__(parent)
        self.provider = provider or available_windows
        self.setWindowTitle('选择目标窗口')
        self.resize(650, 430)
        layout = QVBoxLayout(self)
        layout.addWidget(QLabel('选择当前可见的窗口。打开新窗口后可点击刷新。'))
        self.windows = QListWidget()
        layout.addWidget(self.windows)
        self.message = QLabel()
        self.message.setWordWrap(True)
        layout.addWidget(self.message)
        buttons = QDialogButtonBox(QDialogButtonBox.StandardButton.Ok | QDialogButtonBox.StandardButton.Cancel)
        self.ok_button = buttons.button(QDialogButtonBox.StandardButton.Ok)
        self.ok_button.setText('选择')
        refresh = QPushButton('刷新列表')
        buttons.addButton(refresh, QDialogButtonBox.ButtonRole.ActionRole)
        refresh.clicked.connect(self.refresh)
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        self.windows.currentItemChanged.connect(self._selection_changed)
        self.windows.itemDoubleClicked.connect(lambda _: self.accept())
        layout.addWidget(buttons)
        self.refresh()

    def _selection_changed(self, current, previous):
        self.ok_button.setEnabled(current is not None)

    def selected_window(self):
        item = self.windows.currentItem()
        return item.data(Qt.ItemDataRole.UserRole) if item else None

    def refresh(self):
        selected = self.selected_window()
        self.windows.clear()
        self.ok_button.setEnabled(False)
        try:
            choices = self.provider()
        except Exception as error:
            self.message.setText(f'无法读取窗口列表：{error}')
            return
        for choice in choices:
            item = QListWidgetItem(choice.label)
            item.setData(Qt.ItemDataRole.UserRole, choice)
            self.windows.addItem(item)
            if selected and (choice.number, choice.pid) == (selected.number, selected.pid):
                self.windows.setCurrentItem(item)
        if not choices:
            self.message.setText('未找到可见窗口。请先打开目标窗口，再刷新列表。')
        elif platform.system() == 'Darwin':
            self.message.setText('如果 macOS 窗口标题显示为“无标题”，请检查屏幕录制权限。')
        else:
            self.message.setText('请选择要操作的窗口。')

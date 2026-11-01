"""Startup permission gate.

Without screen recording and accessibility access Sky cannot preview, record or
drive any window, so asking once when the app opens beats a capture failing
after the user already picked a target. This dialog stays the only visible
window until every requirement is satisfied.

缺少屏幕录制或辅助功能权限时，预览、录制和运行都无法工作。在软件打开时一次性
检查并申请，比用户选好窗口后截图才失败要友好得多；权限未满足前不进入主界面。
"""
from __future__ import annotations

import platform
import sys
from types import SimpleNamespace

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QDialog,
    QHBoxLayout,
    QLabel,
    QMessageBox,
    QPushButton,
    QVBoxLayout,
)

# Display order for the requirement list; keys match PermissionState fields.
# 展示顺序，键与 PermissionState 字段一致。
REQUIREMENTS = (
    ('accessibility', '辅助功能', '操作鼠标键盘'),
    ('screen_recording', '屏幕录制', '预览和截取目标窗口'),
)


def current_state():
    """Permission snapshot. Other platforms have nothing to ask for."""
    if platform.system() != 'Darwin':
        return SimpleNamespace(accessibility=True, screen_recording=True, ok=True)
    from skymac.permissions import current_state as mac_state
    return mac_state()


def responsible_application():
    """The app macOS will list in System Settings for this process."""
    if platform.system() != 'Darwin':
        return None
    from skymac.permissions import responsible_application as mac_responsible
    return mac_responsible()


def request(kind: str) -> None:
    """Trigger the system prompt for one permission so Sky is listed in Settings."""
    if platform.system() != 'Darwin':
        return
    from skymac import permissions
    if kind == 'accessibility':
        permissions.accessibility_trusted(prompt=True)
    else:
        permissions.screen_recording_allowed(prompt=True)


def open_settings(kind: str) -> None:
    if platform.system() != 'Darwin':
        return
    from skymac.permissions import open_settings as mac_open_settings
    mac_open_settings(kind)


def restart_application() -> None:
    """macOS only gives a newly granted screen-recording permission to a new process."""
    from PySide6.QtCore import QProcess
    # Packaged builds are their own executable; a dev run has to keep the entry
    # script, otherwise the venv Python would start with no arguments.
    # 打包后是独立可执行文件；开发态要把入口脚本一起带上，否则会起一个空 Python。
    if getattr(sys, 'frozen', False):
        QProcess.startDetached(sys.executable, sys.argv[1:])
    else:
        QProcess.startDetached(sys.executable, sys.argv)


class PermissionGate(QDialog):
    def __init__(self, parent=None, state_provider=None, requester=None, opener=None):
        super().__init__(parent)
        self.state_provider = state_provider or current_state
        self.requester = requester or request
        self.opener = opener or open_settings
        self.responsible_provider = responsible_application
        self.setWindowTitle('Sky 需要系统权限')
        self.setModal(True)
        self.setMinimumSize(600, 420)
        layout = QVBoxLayout(self)
        layout.setSpacing(10)
        title = QLabel('Sky 需要以下系统权限')
        title.setStyleSheet('font-weight: 600; font-size: 15px;')
        layout.addWidget(title)
        self.intro = QLabel()
        self.intro.setWordWrap(True)
        layout.addWidget(self.intro)
        self.owner = QLabel()
        self.owner.setWordWrap(True)
        self.owner.setTextFormat(Qt.TextFormat.RichText)
        self.owner.setStyleSheet(
            'background: #f2f4f7; border: 1px solid #dfe3e8; border-radius: 6px; padding: 8px;')
        layout.addWidget(self.owner)
        self.items = QLabel()
        self.items.setWordWrap(True)
        self.items.setTextFormat(Qt.TextFormat.RichText)
        layout.addWidget(self.items)
        self.hint = QLabel()
        self.hint.setWordWrap(True)
        self.hint.setStyleSheet('color: #8a5a00;')
        layout.addWidget(self.hint)
        layout.addStretch(1)
        row = QHBoxLayout()
        self.apply_button = QPushButton('申请授权')
        self.apply_button.clicked.connect(self.apply)
        row.addWidget(self.apply_button)
        self.recheck_button = QPushButton('重新检查')
        self.recheck_button.clicked.connect(self.recheck)
        row.addWidget(self.recheck_button)
        row.addStretch(1)
        quit_button = QPushButton('退出 Sky')
        quit_button.clicked.connect(self.reject)
        row.addWidget(quit_button)
        layout.addLayout(row)
        self.initial = self.state_provider()
        self.state = self.initial
        self.refresh()

    def granted(self) -> bool:
        return bool(getattr(self.state, 'ok', False))

    def initial_granted(self) -> bool:
        return bool(getattr(self.initial, 'ok', False))

    def needs_restart(self) -> bool:
        """Screen recording granted during this run only takes effect after a relaunch."""
        return (not getattr(self.initial, 'screen_recording', True)
                and bool(getattr(self.state, 'screen_recording', False)))

    def refresh(self) -> bool:
        self.state = self.state_provider()
        rows = []
        for name, label, purpose in REQUIREMENTS:
            granted = getattr(self.state, name, True)
            mark = '&#10003;' if granted else '&#10007;'
            color = '#1a7f37' if granted else '#b3261e'
            status = '' if granted else '<span style="color:#b3261e;">未授权</span>'
            rows.append(
                f'<p style="margin:6px 0;"><span style="color:{color}; font-weight:600;">{mark}</span>'
                f'&nbsp;<b>{label}</b>（{purpose}）&nbsp;{status}</p>'
            )
        self.items.setText(''.join(rows))
        self.owner.setText(self._owner_text())
        self.intro.setText(self._intro_text())
        problems = [] if self.granted() else self.state.problems()
        self.hint.setText('\n'.join(problems))
        self.apply_button.setEnabled(not self.granted())
        return self.granted()

    def _owner_text(self) -> str:
        """Name the exact app System Settings will list.

        A dev run has no bundle of its own, so "Sky" never appears in the list;
        saying so up front avoids the user searching for an entry that cannot
        exist there.
        """
        found = self.responsible_provider()
        self._host = found
        if found is None:
            return ('<b>系统设置中勾选：</b>你启动 Sky 的那个程序（终端 Terminal / iTerm，或打包后的 Sky.app）。')
        name, path = found
        if path.endswith('Sky.app'):
            return f'<b>系统设置中勾选：</b>{name}（{path}）'
        return (f'<b>系统设置中勾选：</b>{name}（{path}）<br>'
                '<span style="color:#5f6b7a;">当前是开发态运行，Sky 没有独立的应用条目，'
                '权限记在这个宿主应用上。若希望 Sky 单独出现在列表中，请运行打包后的 Sky.app。</span>')

    def _intro_text(self) -> str:
        if getattr(self, '_host', None) is not None and not self._host[1].endswith('Sky.app'):
            host = self._host[0]
            return (f'预览、录制和运行脚本都依赖这些权限。授权后需要完全退出 {host} 并重新启动，'
                    '再从 Sky 侧重新检查。')
        return '预览、录制和运行脚本都依赖这些权限。授权后需要重新启动 Sky 才会生效。'

    def apply(self) -> None:
        missing = [name for name, _, _ in REQUIREMENTS if not getattr(self.state, name, True)]
        # One permission at a time: stacked system prompts and settings pages
        # would leave the user guessing which switch belongs to which dialog.
        # 一次只申请一项：系统弹窗和设置页同时出现时，用户分不清开关对应哪个。
        if missing:
            self.requester(missing[0])
            self.opener(missing[0])
        self.refresh()

    def recheck(self) -> bool:
        if self.refresh():
            self.accept()
            return True
        return False


def prompt_restart(parent=None) -> bool:
    """Explain what has to be restarted for a new grant to apply.

    Screen recording is bound to the process at launch, so neither a relaunched
    Sky nor a fresh shell is enough: in a dev run the host app itself (the IDE or
    terminal that owns the TCC entry) has to be quit and reopened.
    """
    host = responsible_application()
    box = QMessageBox(parent)
    box.setWindowTitle('Sky')
    if host is not None and not host[1].endswith('Sky.app'):
        box.setText(f'权限已授权。屏幕录制权限只对新进程生效，请完全退出 {host[0]} 后重新启动它，'
                    '再重新运行 Sky。')
        box.addButton('知道了', QMessageBox.ButtonRole.AcceptRole)
        box.exec()
        return False
    box.setText('权限已全部授权，需要重新启动 Sky 才会生效。')
    restart = box.addButton('立即重启', QMessageBox.ButtonRole.AcceptRole)
    box.addButton('稍后', QMessageBox.ButtonRole.RejectRole)
    box.exec()
    if box.clickedButton() is not restart:
        return False
    restart_application()
    return True


def ensure_permissions(parent=None) -> bool:
    """Gate the application on required permissions before the main window opens.

    Returns True when everything was already granted, and False whenever the
    user left the gate unaccepted, including a fresh grant that still needs a
    relaunch - the caller must not continue operating in-process.
    """
    gate = PermissionGate(parent)
    if gate.initial_granted():
        return True
    if gate.exec() != QDialog.DialogCode.Accepted:
        return False
    if not gate.needs_restart():
        return True
    prompt_restart(parent)
    return False


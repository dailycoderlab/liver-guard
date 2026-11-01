"""Entry point for the native desktop application."""
import sys

from PySide6.QtWidgets import QApplication

from .main_window import MainWindow
from .permissions_gate import ensure_permissions


def main() -> int:
    app = QApplication.instance() or QApplication(sys.argv)
    app.setApplicationName('Sky')
    app.setQuitOnLastWindowClosed(False)
    # 权限检查放在主窗口之前：未授权时用户看不到也没法操作任何功能。
    # Checked before the main window so nothing is operable without permission.
    if not ensure_permissions():
        return 0
    window = MainWindow()
    window.show()
    return app.exec()


if __name__ == '__main__':
    raise SystemExit(main())

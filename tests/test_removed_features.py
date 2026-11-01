"""已移除的功能不应留下入口或残留代码。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import pytest
from PySide6.QtWidgets import QApplication, QPushButton

REMOVED_BUTTONS = ('权限与环境', '系统权限', '开机自启')


@pytest.fixture(scope='module')
def app():
    return QApplication.instance() or QApplication([])


@pytest.fixture
def window(tmp_path, monkeypatch, app):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    built = gui.MainWindow()
    built.preview_timer.stop()
    yield built
    built.pending_save = False
    built._quitting = True
    built.close()
    app.processEvents()


def all_buttons(window):
    return [button.text() for button in window.findChildren(QPushButton)]


@pytest.mark.parametrize('label', REMOVED_BUTTONS)
def test_removed_buttons_are_gone(window, label):
    assert label not in all_buttons(window)


@pytest.mark.parametrize('method', ('diagnose', 'show_permissions', 'configure_autostart'))
def test_removed_handlers_are_gone(window, method):
    assert not hasattr(window, method)


def test_autostart_module_is_deleted():
    # 模块本身要删掉，否则还会被误以为是可用功能。
    with pytest.raises(ModuleNotFoundError):
        import skygui.autostart  # noqa: F401


def test_startup_permission_gate_is_kept():
    # 启动阶段的权限门是必需的，只删掉运行中复查的入口。
    import skygui.permissions_gate as gate
    assert hasattr(gate, 'ensure_permissions')
    assert not hasattr(gate, 'show_permission_gate')
    # allow_close 只服务于已删除的运行中入口。
    import inspect
    assert 'allow_close' not in inspect.signature(gate.PermissionGate).parameters


def test_toolbar_keeps_the_essential_actions(window):
    labels = all_buttons(window)
    for kept in ('选择窗口', '运行历史', '运行', '停止'):
        assert kept in labels


def test_mac_driver_drops_the_unused_diagnostic_method():
    # describe() 只被已删除的「权限与环境」使用，留着就是死代码。
    from skymac.driver import MacDriver
    assert not hasattr(MacDriver, 'describe')

"""Window picker and exact-window selection without desktop access."""
import os
from types import SimpleNamespace

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

from PySide6.QtWidgets import QApplication, QDialog  # noqa: E402

from skygui.window_picker import WindowChoice, WindowPicker, available_windows  # noqa: E402


def test_picker_refresh_selection_and_errors():
    app = QApplication.instance() or QApplication([])
    choices = []

    def provider():
        return choices

    picker = WindowPicker(provider=provider)
    assert not picker.ok_button.isEnabled()
    assert '未找到' in picker.message.text()
    choices.extend([WindowChoice(1, 10, '同名窗口 #1'), WindowChoice(2, 20, '同名窗口 #2')])
    picker.refresh()
    assert picker.windows.count() == 2
    picker.windows.setCurrentRow(1)
    assert picker.ok_button.isEnabled()
    assert picker.selected_window() == choices[1]
    picker.refresh()
    assert picker.selected_window() == choices[1]
    choices.pop()
    picker.refresh()
    assert picker.selected_window() is None
    assert not picker.ok_button.isEnabled()
    picker.provider = lambda: (_ for _ in ()).throw(RuntimeError('枚举失败'))
    picker.refresh()
    assert '枚举失败' in picker.message.text()
    app.processEvents()


def test_mac_window_list_excludes_self_and_keeps_untitled(monkeypatch):
    import skygui.window_picker as picker
    import skymac.window as mac_window
    from skycore.geometry import Rect

    monkeypatch.setattr(picker.platform, 'system', lambda: 'Darwin')
    windows = [
        mac_window.WindowInfo(1, os.getpid(), 'Sky', 'Sky', Rect(0, 0, 100, 100), 0),
        mac_window.WindowInfo(2, 123, '浏览器', '', Rect(0, 0, 500, 300), 0),
        mac_window.WindowInfo(3, 123, '浏览器', '页面', Rect(0, 0, 400, 200), 0),
    ]
    monkeypatch.setattr(mac_window, 'list_windows', lambda: windows)
    choices = available_windows()
    assert [choice.number for choice in choices] == [2, 3]
    assert '无标题' in choices[0].label
    assert '浏览器' in choices[1].label


def test_windows_list_uses_physical_geometry_and_excludes_self(monkeypatch):
    from contextlib import contextmanager
    from skycore.geometry import Rect
    import skygui.window_picker as picker
    import skywin._native as native
    import skywin.window as win_window

    monkeypatch.setattr(picker.platform, 'system', lambda: 'Windows')
    states = []

    class API:
        @contextmanager
        def physical_pixels(self):
            states.append('enter')
            yield
            states.append('exit')

    windows = [
        win_window.Window(11, os.getpid(), 1, 'own', 'Sky', Rect(0, 0, 100, 100)),
        win_window.Window(12, 123, 1, 'app', '目标窗口', Rect(0, 0, 800, 600)),
    ]
    monkeypatch.setattr(native, 'WinAPI', API)
    monkeypatch.setattr(win_window, 'list_windows', lambda api: windows)
    choices = available_windows()
    assert states == ['enter', 'exit']
    assert len(choices) == 1
    assert choices[0].number == 12
    assert '目标窗口' in choices[0].label


def test_select_window_uses_exact_identifier_and_checks_identity(tmp_path, monkeypatch):
    import skygui.main_window as gui
    import skymac.driver as mac_driver

    app = QApplication.instance() or QApplication([])
    monkeypatch.setattr(gui.MainWindow, 'restore_session', lambda self: None)
    monkeypatch.setattr(gui.platform, 'system', lambda: 'Darwin')
    choice = WindowChoice(42, 123, '测试窗口')

    class Picker:
        def __init__(self, parent):
            pass

        def exec(self):
            return QDialog.DialogCode.Accepted

        def selected_window(self):
            return choice

    class Driver:
        pid = 123

        def __init__(self, **kwargs):
            self.kwargs = kwargs

        def surface(self):
            return SimpleNamespace(pid=self.pid)

    monkeypatch.setattr(gui, 'WindowPicker', Picker)
    monkeypatch.setattr(mac_driver, 'MacDriver', Driver)
    window = gui.MainWindow()
    window.preview_timer.stop()
    errors = []
    monkeypatch.setattr(window, 'error', errors.append)
    window.select_window()
    assert not errors
    assert window.driver.kwargs == {'window_number': 42, 'full_screen': False}
    assert '测试窗口' in window.status.text()
    existing = window.driver
    Driver.pid = 999
    window.select_window()
    assert window.driver is existing
    assert '重新选择' in errors[0]
    window._quitting = True
    window.close()

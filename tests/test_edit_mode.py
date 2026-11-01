"""编辑模式：必须主动进入才能通过点击画面录制步骤。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import numpy as np
import pytest
from PySide6.QtWidgets import QApplication, QPushButton

from skycore.frame import Frame
from skycore.geometry import Bounds
from skygui.main_window import blank_ir


@pytest.fixture(scope='module')
def app():
    return QApplication.instance() or QApplication([])


@pytest.fixture
def window(tmp_path, monkeypatch, app):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    built = gui.MainWindow()
    built.preview_timer.stop()
    built.show()
    app.processEvents()
    yield built
    # 关闭前清掉未保存标记，否则 close() 会弹出确认框卡住测试。
    # Clear the dirty flag before closing; otherwise close() prompts.
    built.pending_save = False
    built._quitting = True
    built.close()
    app.processEvents()


def open_script(window):
    script = window.store.create_script('测试', blank_ir('测试'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    return script


def fake_driver(monkeypatch, window):
    bounds = Bounds(0, 0, 100, 100)
    image = np.zeros((100, 100, 3), dtype=np.uint8)
    image[20:80, 20:80] = [10, 120, 230]
    frame = Frame(image, bounds)
    events = []

    class Driver:
        def bounds(self): return bounds
        def activate(self): return True
        def capture(self):
            events.append('capture')
            return frame
        def ax_at(self, x, y): return None
        def tap(self, x, y): events.append('tap')

    class Dialog:
        def __init__(self, *args): pass
        def exec(self): return 1
        def selection(self): return (20, 20, 60, 60)

    import skygui.main_window as gui
    monkeypatch.setattr(gui.platform, 'system', lambda: 'test')
    monkeypatch.setattr(gui, 'CropDialog', Dialog)
    window.driver = Driver()
    window.last_frame = frame
    monkeypatch.setattr(window, 'task',
                        lambda fn, done=None, failed=None: done(fn()) if done else fn())
    return events


def test_scripts_open_in_read_only_mode(window):
    open_script(window)
    assert window.editing is False
    assert window.edit_button.text() == '开始编辑'
    # 非编辑状态不能录，这是防误操作的核心。
    assert window.live.editable is False


def test_clicking_the_preview_outside_edit_mode_records_nothing(window, monkeypatch):
    open_script(window)
    events = fake_driver(monkeypatch, window)
    window.record_click(0.5, 0.5)
    # 没有截图、没有点击、没有新增步骤。
    assert events == []
    assert window.ir['blocks']['main'] == []


def test_entering_edit_mode_enables_recording(window, monkeypatch):
    open_script(window)
    events = fake_driver(monkeypatch, window)
    window.edit_button.setChecked(True)
    assert window.editing is True
    assert window.live.editable is True
    assert window.edit_button.text() == '结束编辑'
    window.record_click(0.5, 0.5)
    assert events == ['capture', 'tap']
    assert len(window.ir['blocks']['main']) == 1


def test_leaving_edit_mode_disables_recording_again(window, monkeypatch):
    open_script(window)
    fake_driver(monkeypatch, window)
    window.edit_button.setChecked(True)
    window.record_click(0.5, 0.5)
    before = len(window.ir['blocks']['main'])
    window.edit_button.setChecked(False)
    assert window.live.editable is False
    window.record_click(0.5, 0.5)
    assert len(window.ir['blocks']['main']) == before


def test_edit_mode_requires_an_open_script(window, monkeypatch):
    assert window.ir is None
    # error() 会弹模态框，测试里替换掉，否则会一直等用户点确定。
    # error() opens a modal dialog; stub it so the test does not block.
    messages = []
    monkeypatch.setattr(window, 'error', messages.append)
    window.edit_button.setChecked(True)
    # 没有脚本时不能进入编辑模式，按钮应该自己弹回。
    assert window.editing is False
    assert not window.edit_button.isChecked()
    assert messages and '请先选择或新建一个脚本' in messages[0]


def test_leaving_edit_mode_keeps_unsaved_changes_and_warns(window, monkeypatch):
    open_script(window)
    fake_driver(monkeypatch, window)
    window.edit_button.setChecked(True)
    window.record_click(0.5, 0.5)
    # 手动保存：录制后改动只在内存里，磁盘仍是空脚本。
    # Manual save: after recording the edit is in memory, disk is still empty.
    assert window.pending_save is True
    assert window.store.get_script(window.script['id'])['ir']['blocks']['main'] == []
    window.edit_button.setChecked(False)
    # 退出编辑不写盘，但要提示未保存。
    assert '未保存' in window.status.text()
    assert window.save_button.isEnabled()
    assert window.save_script()
    assert '已保存' in window.status.text()


def test_switching_scripts_leaves_edit_mode(window, monkeypatch):
    open_script(window)
    window.store.create_script('另一个', blank_ir('另一个'))
    window.load_scripts()
    window.edit_button.setChecked(True)
    assert window.editing is True
    # 按名称找另一个脚本：列表是按名称排序的，item(1) 不一定是它。
    # Find by name; the list is sorted, so item(1) is not necessarily the other.
    other = next(window.scripts.item(i) for i in range(window.scripts.count())
                 if window.scripts.item(i).text().startswith('另一个'))
    window.open_script(other)
    assert window.script['name'] == '另一个'
    # 打开另一个脚本时不应延续上一个脚本的编辑意图。
    assert window.editing is False
    assert window.live.editable is False


def test_new_script_enters_edit_mode(window, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui.QInputDialog, 'getText', lambda *a, **k: ('新脚本', True))
    window.create_script()
    # 新建的脚本是空的，用户就是要录，直接进入编辑模式。
    assert window.editing is True
    assert window.edit_button.isChecked()


def test_run_and_stop_live_in_the_toolbar(window):
    assert window.run_button.text() == '运行'
    assert window.stop_button.text() == '停止'
    # 停止默认不可用，避免没有运行时的无意义点击。
    assert window.stop_button.isEnabled() is False


def test_step_pane_has_no_action_buttons_below_the_list(window):
    pane = window.steps.parentWidget()
    labels = [button.text() for button in pane.findChildren(QPushButton)]
    # 只剩下编辑模式开关，操作都走右键菜单了。
    assert labels == ['开始编辑']
    for removed in ('上移', '下移', '删除', '插入等待', '保存', '运行', '停止', '删除脚本'):
        assert removed not in labels


def test_running_state_toggles_the_buttons(window):
    window.set_running(True)
    assert window.run_button.isEnabled() is False
    assert window.stop_button.isEnabled() is True
    window.set_running(False)
    assert window.run_button.isEnabled() is True
    assert window.stop_button.isEnabled() is False

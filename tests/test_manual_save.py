"""手动保存：改动只在内存里，点「保存」才写盘。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import pytest
from PySide6.QtWidgets import QApplication

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
    built.pending_save = False
    built._quitting = True
    built.close()
    app.processEvents()


def open_script(window):
    script = window.store.create_script('测试', blank_ir('测试'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    return script


def add_step(window, ms=10):
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': ms})
    window.changed()


def disk_steps(window, script_id):
    return window.store.get_script(script_id)['ir']['blocks']['main']


def test_save_button_starts_disabled(window):
    open_script(window)
    # 没有改动时保存按钮不可用，避免无意义点击。
    assert window.save_button.isEnabled() is False
    assert window.save_button.text() == '保存'


def test_editing_marks_dirty_without_writing(window):
    script = open_script(window)
    add_step(window)
    assert window.pending_save is True
    assert window.save_button.isEnabled() is True
    # 磁盘内容没变：这是手动保存的核心保证。
    assert disk_steps(window, script['id']) == []


def test_save_writes_to_disk_and_clears_dirty(window):
    script = open_script(window)
    add_step(window)
    assert window.save_script()
    assert window.pending_save is False
    assert window.save_button.isEnabled() is False
    assert disk_steps(window, script['id'])[0]['ms'] == 10
    assert '已保存' in window.status.text()


def test_discard_reverts_to_the_saved_content(window, monkeypatch):
    script = open_script(window)
    add_step(window)
    import skygui.main_window as gui
    # 有未保存改动时切换脚本，选择放弃 → 从磁盘重读。
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: gui.QMessageBox.StandardButton.Discard)
    window.reload_current_script()
    assert window.pending_save is False
    assert window.ir['blocks']['main'] == []
    assert disk_steps(window, script['id']) == []


def test_cancel_keeps_the_edit(window, monkeypatch):
    open_script(window)
    add_step(window)
    import skygui.main_window as gui
    # cancel 意味着整个动作取消，改动继续留在内存里。
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: gui.QMessageBox.StandardButton.Cancel)
    assert window.confirm_discard() is False
    assert window.pending_save is True
    assert len(window.ir['blocks']['main']) == 1


def test_run_saves_pending_changes_before_reading_disk(window, monkeypatch):
    script = open_script(window)
    add_step(window)
    assert window.pending_save is True
    saved = []
    original = window.store.save_script

    def spy(script_id, ir):
        saved.append(script_id)
        return original(script_id, ir)

    monkeypatch.setattr(window.store, 'save_script', spy)
    # 运行读磁盘内容，所以必须先把内存改动落盘；否则会跑旧版本。
    # Runs read from disk, so pending edits must reach it first.
    assert window.save_script()
    assert saved == [script['id']]
    assert disk_steps(window, script['id'])[0]['ms'] == 10


def test_closing_with_unsaved_changes_can_be_cancelled(window, monkeypatch):
    open_script(window)
    add_step(window)
    import skygui.main_window as gui
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: gui.QMessageBox.StandardButton.Cancel)

    class Event:
        def __init__(self):
            self.ignored = False
            self.accepted = False

        def ignore(self):
            self.ignored = True

        def accept(self):
            self.accepted = True

    event = Event()
    window.closeEvent(event)
    # 取消关闭：窗口留着，改动还在。
    assert event.ignored is True
    assert window.pending_save is True


def test_opening_the_same_script_does_not_prompt(window, monkeypatch):
    open_script(window)
    add_step(window)
    prompted = []
    import skygui.main_window as gui
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: prompted.append(True) or gui.QMessageBox.StandardButton.Cancel)
    # 点击已打开的脚本不该弹「是否保存」。
    window.open_script(window.scripts.item(0))
    assert prompted == []
    assert window.pending_save is True

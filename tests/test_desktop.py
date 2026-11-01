"""Desktop integration checks without a physical screen or network."""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

from PySide6.QtCore import Qt  # noqa: E402
from PySide6.QtWidgets import QApplication  # noqa: E402

from skygui.main_window import MainWindow, blank_ir  # noqa: E402


def test_preview_is_above_status_in_right_column(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    splitter = window.centralWidget().layout().itemAt(1).widget()
    assert splitter.count() == 3
    assert splitter.widget(0).isAncestorOf(window.scripts)
    assert splitter.widget(1).isAncestorOf(window.steps)
    right = splitter.widget(2)
    assert right.orientation() == Qt.Orientation.Vertical
    assert right.widget(0).isAncestorOf(window.live)
    assert right.widget(1).isAncestorOf(window.log)
    window._quitting = True
    window.close()


def test_script_edit_is_manual_then_persists(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    script = window.store.create_script('测试', blank_ir('测试'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': 100})
    window.changed()
    # 手动保存模式：改动只在内存里，磁盘还是旧内容。
    # Manual save: the edit stays in memory, disk still holds the old content.
    assert window.pending_save
    assert window.store.get_script(script['id'])['ir']['blocks']['main'] == []
    assert window.save_button.isEnabled()
    assert window.save_script()
    assert not window.pending_save
    assert not window.save_button.isEnabled()
    assert window.store.get_script(script['id'])['ir']['blocks']['main'][0]['ms'] == 100
    window._quitting = True
    window.close()
    window2 = MainWindow()
    window2.open_script(window2.scripts.item(0))
    assert window2.ir['blocks']['main'][0]['ms'] == 100
    window2._quitting = True
    window2.close()


def test_explicit_save_writes_even_without_changes(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    script = window.store.create_script('测试', blank_ir('测试'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    writes = []
    original = window.store.save_script

    def counted(script_id, ir):
        writes.append(script_id)
        return original(script_id, ir)

    monkeypatch.setattr(window.store, 'save_script', counted)
    assert window.pending_save is False
    assert window.save_script()
    assert writes == [script['id']]
    assert '已保存' in window.status.text()
    window._quitting = True
    window.close()


def test_failed_save_keeps_unsaved_changes_and_prevents_switch(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    first = window.store.create_script('a', blank_ir('a'))
    window.store.create_script('b', blank_ir('b'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    errors = []
    monkeypatch.setattr(window, 'error', errors.append)

    def failed(*args):
        raise OSError('disk full')

    monkeypatch.setattr(window.store, 'save_script', failed)
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': 10})
    window.changed()
    # 用户选择「保存」但写盘失败：不能切走，改动必须留在内存里。
    # The user chose Save but it failed: stay put, keep the edit in memory.
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: gui.QMessageBox.StandardButton.Save)
    window.open_script(window.scripts.item(1))
    assert window.script['id'] == first['id']
    assert window.pending_save
    assert 'disk full' in errors[-1]
    # 退出时不要真弹确认框：把改动标记为已丢弃再关闭。
    # Mark the edit as discarded so closing does not try to prompt.
    window.pending_save = False
    window._quitting = True
    window.close()


def test_discard_reloads_the_saved_content(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    window.store.create_script('a', blank_ir('a'))
    window.store.create_script('b', blank_ir('b'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': 10})
    window.changed()
    assert window.pending_save
    # 选择「放弃」时切走：内存改动丢掉，磁盘内容不变。
    # Discarding on switch drops the edit and leaves disk untouched.
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: gui.QMessageBox.StandardButton.Discard)
    window.open_script(window.scripts.item(1))
    assert window.script['name'] == 'b'
    assert not window.pending_save
    window._quitting = True
    window.close()


def test_cancel_keeps_the_current_script(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    window.store.create_script('a', blank_ir('a'))
    window.store.create_script('b', blank_ir('b'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': 10})
    window.changed()
    monkeypatch.setattr(gui.QMessageBox, 'question',
                        lambda *a, **k: gui.QMessageBox.StandardButton.Cancel)
    window.open_script(window.scripts.item(1))
    # 取消后停在原脚本，改动仍然待保存。
    assert window.script['name'] == 'a'
    assert window.pending_save
    window.pending_save = False
    window._quitting = True
    window.close()

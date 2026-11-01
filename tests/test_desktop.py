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


def test_script_draft_edit_persists_and_reopens(tmp_path, monkeypatch):
    import skygui.main_window as gui
    monkeypatch.setattr(gui, 'HOME', tmp_path)
    app = QApplication.instance() or QApplication([])
    window = MainWindow()
    script = window.store.create_script('测试', blank_ir('测试'))
    window.load_scripts()
    window.open_script(window.scripts.item(0))
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': 100})
    window.changed()
    assert not window.pending_save
    assert window.store.get_script(script['id'])['draft']['blocks']['main'][0]['ms'] == 100
    window.publish()
    assert window.store.get_script(script['id'])['versions']['1']['blocks']['main'][0]['ms'] == 100
    window._quitting = True
    window.close()
    window2 = MainWindow()
    window2.open_script(window2.scripts.item(0))
    assert window2.ir['blocks']['main'][0]['ms'] == 100
    assert window2.script['currentVersion'] == 1
    window2._quitting = True
    window2.close()


def test_failed_save_keeps_unsaved_draft_and_prevents_switch(tmp_path, monkeypatch):
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
    monkeypatch.setattr(window.store, 'save_draft', failed)
    window.ir['blocks']['main'].append({'op': 'sleep', 'id': 's1', 'ms': 10})
    window.changed()
    window.open_script(window.scripts.item(1))
    assert window.script['id'] == first['id']
    assert window.pending_save
    assert 'disk full' in errors[-1]
    monkeypatch.undo()
    window._quitting = True
    window.close()

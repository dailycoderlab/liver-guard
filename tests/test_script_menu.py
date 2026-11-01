"""脚本列表的右键菜单：新建/重命名/删除/刷新。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import pytest
from PySide6.QtWidgets import QApplication, QMessageBox

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
    yield built
    built.pending_save = False
    built._quitting = True
    built.close()
    app.processEvents()


def labels(menu):
    return [action.text() for action in menu.actions() if action.text()]


def seed(window, names):
    for name in names:
        window.store.create_script(name, blank_ir(name))
    window.load_scripts()


def test_empty_area_offers_create_and_refresh_only(window):
    seed(window, ['a'])
    menu = window.build_script_menu(None)
    assert labels(menu) == ['新建脚本…', '刷新列表']


def test_row_menu_offers_rename_and_delete(window):
    seed(window, ['a'])
    menu = window.build_script_menu(window.scripts.item(0))
    assert labels(menu) == ['新建脚本…', '重命名…', '删除', '刷新列表']


def find_item(window, name):
    """按名称取列表项；列表是按名称排序的，不能按下标假设顺序。"""
    for index in range(window.scripts.count()):
        item = window.scripts.item(index)
        if item.text().startswith(name):
            return item
    raise AssertionError(f'列表里没有 {name}')


def test_rename_acts_on_the_clicked_script_not_the_open_one(window, monkeypatch):
    seed(window, ['甲', '乙'])
    window.open_script(find_item(window, '甲'))
    open_id = window.script['id']
    other = find_item(window, '乙')
    other_id = window.script_id_of(other)
    assert other_id != open_id

    import skygui.main_window as gui
    monkeypatch.setattr(gui.QInputDialog, 'getText', lambda *a, **k: ('乙改', True))
    window.rename_script(other)

    names = {s['id']: s['name'] for s in window.store.list_scripts()}
    assert names[other_id] == '乙改'
    # The open script keeps its own name; renaming elsewhere must not disturb it.
    assert window.script['id'] == open_id
    assert window.script['name'] == '甲'


def test_delete_acts_on_the_clicked_script_and_keeps_the_open_one(window, monkeypatch):
    seed(window, ['甲', '乙'])
    window.open_script(find_item(window, '甲'))
    open_id = window.script['id']
    other_id = window.script_id_of(find_item(window, '乙'))

    monkeypatch.setattr(QMessageBox, 'question',
                        lambda *a, **k: QMessageBox.StandardButton.Yes)
    window.delete_script(find_item(window, '乙'))

    remaining = [s['id'] for s in window.store.list_scripts()]
    assert other_id not in remaining
    # Deleting a different script must leave the open one intact.
    assert window.script is not None and window.script['id'] == open_id


def test_delete_open_script_clears_the_editor(window, monkeypatch):
    seed(window, ['甲'])
    window.open_script(find_item(window, '甲'))
    monkeypatch.setattr(QMessageBox, 'question',
                        lambda *a, **k: QMessageBox.StandardButton.Yes)
    window.delete_script(find_item(window, '甲'))
    assert window.script is None
    assert window.ir is None
    assert window.steps.count() == 0


def test_delete_cancelled_keeps_the_script(window, monkeypatch):
    seed(window, ['甲'])
    monkeypatch.setattr(QMessageBox, 'question',
                        lambda *a, **k: QMessageBox.StandardButton.No)
    window.delete_script(find_item(window, '甲'))
    assert len(window.store.list_scripts()) == 1

"""步骤命名：默认名称、改名、清空恢复，以及改名后的显示与日志。"""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import numpy as np
import pytest
from PySide6.QtCore import Qt
from PySide6.QtWidgets import QApplication

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
    # Clear the dirty flag so close() does not prompt.
    built.pending_save = False
    built._quitting = True
    built.close()
    app.processEvents()


def seed(window):
    script = window.store.create_script('测试', blank_ir('测试'))
    window.script = script
    window.ir = blank_ir('测试')
    window.ir['targets'] = {
        't1': {'type': 'template', 'asset': 'a' * 64, 'assetBox': {'w': 40, 'h': 20}},
        't2': {'type': 'template', 'asset': 'b' * 64, 'assetBox': {'w': 30, 'h': 30}},
    }
    window.ir['blocks']['main'] = [
        {'op': 'tap', 'id': 's1', 'target': 't1'},
        {'op': 'sleep', 'id': 's2', 'ms': 500},
        {'op': 'tap', 'id': 's3', 'target': 't2'},
    ]
    window.refresh_steps()
    return window.ir


def test_each_row_shows_a_name_and_the_rows_differ(window):
    seed(window)
    captions = [window.steps.item(i).text() for i in range(window.steps.count())]
    assert len(captions) == 3
    titles = [caption.split('\n')[0] for caption in captions]
    assert titles == ['1. 点击 t1', '2. 等待 500 ms', '3. 点击 t2']
    # Two taps on different targets must not read identically.
    assert titles[0] != titles[2]


def test_rename_step_persists_and_shows_in_the_list(window, monkeypatch):
    seed(window)
    import skygui.main_window as gui
    monkeypatch.setattr(gui.QInputDialog, 'getText', lambda *a, **k: ('点击购买按钮', True))
    window.rename_step(0)
    assert window.ir['blocks']['main'][0]['name'] == '点击购买按钮'
    assert window.steps.item(0).text().split('\n')[0] == '1. 点击购买按钮'
    # 手动保存模式下改名先留在内存，保存后才落盘。
    # Manual save: the rename lives in memory until saved.
    assert window.pending_save is True
    on_disk = window.store.get_script(window.script['id'])['ir']['blocks']['main']
    assert on_disk == [] or 'name' not in on_disk[0]
    assert window.save_script()
    assert window.store.get_script(window.script['id'])['ir']['blocks']['main'][0]['name'] == '点击购买按钮'


def test_rename_prefills_the_current_name(window, monkeypatch):
    seed(window)
    seen = {}
    import skygui.main_window as gui

    def capture(parent, title, label, text=''):
        seen['text'] = text
        return ('', False)

    monkeypatch.setattr(gui.QInputDialog, 'getText', capture)
    window.ir['blocks']['main'][0]['name'] = '已有名字'
    window.rename_step(0)
    assert seen['text'] == '已有名字'
    # Cancelling changes nothing.
    assert window.ir['blocks']['main'][0]['name'] == '已有名字'


def test_blank_name_restores_the_generated_one(window, monkeypatch):
    seed(window)
    window.ir['blocks']['main'][0]['name'] = '临时名字'
    import skygui.main_window as gui
    monkeypatch.setattr(gui.QInputDialog, 'getText', lambda *a, **k: ('   ', True))
    window.rename_step(0)
    assert 'name' not in window.ir['blocks']['main'][0]
    assert window.steps.item(0).text().split('\n')[0] == '1. 点击 t1'


def test_rename_needs_a_valid_row(window, monkeypatch):
    seed(window)
    import skygui.main_window as gui
    called = []
    monkeypatch.setattr(gui.QInputDialog, 'getText',
                        lambda *a, **k: called.append(True) or ('x', False))
    window.rename_step(99)
    window.rename_step(-1)
    window.ir = None
    window.rename_step(0)
    assert called == []


def test_double_click_renames_and_ctrl_double_click_views(window, monkeypatch):
    seed(window)
    renamed, viewed = [], []
    monkeypatch.setattr(window, 'rename_step', lambda index=None: renamed.append(index))
    monkeypatch.setattr(window, 'preview_step_asset', lambda item: viewed.append(item))

    item = window.steps.item(1)
    window.on_step_double_clicked(item)
    assert renamed == [1] and viewed == []

    # Ctrl+double-click keeps the crop inspection it used to have.
    monkeypatch.setattr(QApplication, 'keyboardModifiers',
                        staticmethod(lambda: Qt.KeyboardModifier.ControlModifier))
    window.on_step_double_clicked(item)
    assert viewed == [item]
    assert renamed == [1]


def test_run_log_uses_the_step_name(window):
    seed(window)
    window.ir['blocks']['main'][0]['name'] = '点击购买按钮'
    assert window.step_label('s1') == '点击购买按钮'
    # Unnamed steps fall back to the generated name, unknown ids to the raw id.
    assert window.step_label('s2') == '等待 500 ms'
    assert window.step_label('nonexistent') == 'nonexistent'


def test_step_menu_offers_rename_and_view(window):
    seed(window)
    labels = [action.text() for action in window.build_step_menu(0).actions() if action.text()]
    assert '重命名步骤…' in labels
    assert '查看点击区域' in labels
    assert '删除步骤' in labels
    # Empty area offers insert plus save.
    empty = [action.text() for action in window.build_step_menu(-1).actions() if action.text()]
    assert empty == ['插入等待…', '保存脚本']


def test_recording_names_the_new_step(window, monkeypatch):
    seed(window)
    window.ir['blocks']['main'] = []
    bounds = Bounds(0, 0, 100, 100)
    image = np.zeros((100, 100, 3), dtype=np.uint8)
    image[20:80, 20:80] = [10, 120, 230]
    frame = Frame(image, bounds)

    class Driver:
        def bounds(self): return bounds
        def activate(self): return True
        def capture(self): return frame
        def ax_at(self, x, y): return None
        def tap(self, x, y): pass

    class Dialog:
        def __init__(self, *args): pass
        def exec(self): return 1  # QDialog.DialogCode.Accepted
        def selection(self): return (20, 20, 60, 60)

    import skygui.main_window as gui
    monkeypatch.setattr(gui.platform, 'system', lambda: 'test')
    monkeypatch.setattr(gui, 'CropDialog', Dialog)
    window.driver = Driver()
    window.last_frame = frame
    monkeypatch.setattr(window, 'task', lambda fn, done=None, failed=None: done(fn()) if done else fn())
    # 录制需要显式进入编辑模式，这是防误操作的关键。
    # Recording requires entering edit mode explicitly.
    window.edit_button.setChecked(True)
    window.record_click(.5, .5)
    step = window.ir['blocks']['main'][0]
    assert step['op'] == 'tap'
    # A freshly recorded step already carries a name, so it never renders blank.
    assert step.get('name', '').startswith('点击')

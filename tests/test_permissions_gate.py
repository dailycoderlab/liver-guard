"""Startup permission gate: blocks the app until every requirement is granted."""
import os
from types import SimpleNamespace

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

from PySide6.QtWidgets import QApplication, QDialog  # noqa: E402

from skygui import permissions_gate  # noqa: E402
from skygui.permissions_gate import PermissionGate  # noqa: E402


def state(accessibility=True, screen_recording=True):
    return SimpleNamespace(accessibility=accessibility, screen_recording=screen_recording,
                           ok=accessibility and screen_recording,
                           problems=lambda: [] if accessibility and screen_recording else ['缺少权限'])


def test_gate_lists_every_requirement_and_requests_one_at_a_time():
    app = QApplication.instance() or QApplication([])
    requested, opened = [], []
    gate = PermissionGate(state_provider=lambda: state(False, False),
                          requester=requested.append, opener=opened.append)
    assert not gate.initial_granted()
    assert gate.apply_button.isEnabled()
    assert '辅助功能' in gate.items.text() and '屏幕录制' in gate.items.text()

    gate.apply()
    # A single click must not stack two system prompts or two settings pages.
    assert requested == ['accessibility']
    assert opened == ['accessibility']
    app.processEvents()


def test_gate_refresh_reenables_apply_and_accepts_when_granted():
    app = QApplication.instance() or QApplication([])
    current = {'value': state(False, True)}
    gate = PermissionGate(state_provider=lambda: current['value'],
                          requester=lambda _: None, opener=lambda _: None)
    assert gate.apply_button.isEnabled()
    assert '屏幕录制' in gate.items.text() and '未授权' in gate.items.text()
    current['value'] = state(True, True)
    assert gate.recheck()
    assert not gate.apply_button.isEnabled()
    app.processEvents()


def test_gate_stays_open_while_a_requirement_is_missing():
    app = QApplication.instance() or QApplication([])
    gate = PermissionGate(state_provider=lambda: state(True, False),
                          requester=lambda _: None, opener=lambda _: None)
    assert not gate.recheck()
    assert gate.result() != int(QDialog.DialogCode.Accepted)
    app.processEvents()


def test_screen_recording_granted_mid_run_needs_restart():
    app = QApplication.instance() or QApplication([])
    current = {'value': state(True, False)}
    gate = PermissionGate(state_provider=lambda: current['value'],
                          requester=lambda _: None, opener=lambda _: None)
    gate.refresh()
    assert not gate.needs_restart()
    current['value'] = state(True, True)
    gate.refresh()
    assert gate.needs_restart()
    app.processEvents()


def test_ensure_permissions_skips_the_dialog_when_already_granted(monkeypatch):
    app = QApplication.instance() or QApplication([])
    monkeypatch.setattr(permissions_gate, 'current_state', lambda: state(True, True))
    executed = []
    monkeypatch.setattr(PermissionGate, 'exec', lambda self: executed.append(True))
    assert permissions_gate.ensure_permissions()
    assert executed == []
    app.processEvents()


def test_ensure_permissions_refuses_when_the_gate_is_dismissed(monkeypatch):
    app = QApplication.instance() or QApplication([])
    monkeypatch.setattr(permissions_gate, 'current_state', lambda: state(False, False))
    monkeypatch.setattr(PermissionGate, 'exec', lambda self: QDialog.DialogCode.Rejected)
    assert not permissions_gate.ensure_permissions()
    app.processEvents()


def test_gate_names_the_host_app_and_warns_about_dev_runs():
    app = QApplication.instance() or QApplication([])
    gate = PermissionGate(state_provider=lambda: state(False, False),
                          requester=lambda _: None, opener=lambda _: None)
    gate.responsible_provider = lambda: ('iTerm', '/Applications/iTerm.app')
    gate.refresh()
    assert 'iTerm' in gate.owner.text()
    assert '开发态' in gate.owner.text()
    # The restart hint has to name the host, not Sky: relaunching Sky alone
    # would still inherit the old process's screen-recording decision.
    assert 'iTerm' in gate.intro.text()
    app.processEvents()


def test_gate_points_at_sky_itself_when_packaged():
    app = QApplication.instance() or QApplication([])
    gate = PermissionGate(state_provider=lambda: state(False, False),
                          requester=lambda _: None, opener=lambda _: None)
    gate.responsible_provider = lambda: ('Sky', '/Applications/Sky.app')
    gate.refresh()
    assert 'Sky' in gate.owner.text()
    assert '开发态' not in gate.owner.text()
    assert '重新启动 Sky' in gate.intro.text()
    app.processEvents()


def test_owner_label_falls_back_when_no_bundle_is_found(monkeypatch):
    from skymac import permissions as mac
    monkeypatch.setattr(mac, 'responsible_application', lambda: None)
    assert '终端' in mac.owner_label()


def test_owner_label_uses_outermost_bundle(monkeypatch):
    from skymac import permissions as mac
    monkeypatch.setattr(mac, 'responsible_application', lambda: ('Cursor', '/Applications/Cursor.app'))
    assert mac.owner_label() == 'Cursor（/Applications/Cursor.app）'


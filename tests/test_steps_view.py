"""Step list thumbnails: each row shows the click region it targets."""
import os

os.environ.setdefault('QT_QPA_PLATFORM', 'offscreen')

import cv2
import numpy as np
import pytest
from PySide6.QtWidgets import QApplication

from skygui.steps_view import (
    StepList,
    default_step_name,
    step_asset,
    step_caption,
    step_target_name,
    step_title,
)


@pytest.fixture(scope='module')
def app():
    return QApplication.instance() or QApplication([])


@pytest.fixture
def asset_dir(tmp_path):
    directory = tmp_path / 'assets'
    directory.mkdir()
    return directory


def write_asset(directory, name, width, height, colour=(40, 120, 210)):
    image = np.zeros((height, width, 3), dtype=np.uint8)
    image[:, :] = colour
    ok, buffer = cv2.imencode('.png', image)
    assert ok
    payload = buffer.tobytes()
    import hashlib
    sha = hashlib.sha256(payload).hexdigest()
    (directory / f'{sha}.png').write_bytes(payload)
    return sha


def make_ir(sha, op='tap', extra_targets=None):
    targets = {'t1': {'type': 'template', 'asset': sha, 'assetBox': {'w': 60, 'h': 40}}}
    targets.update(extra_targets or {})
    return {'targets': targets, 'blocks': {'main': [{'op': op, 'target': 't1'}]}}


def test_step_resolves_its_click_region():
    sha, size = step_asset({'op': 'tap', 'target': 't1'},
                           {'t1': {'type': 'template', 'asset': 'abc', 'assetBox': {'w': 60, 'h': 40}}})
    assert (sha, size) == ('abc', (60.0, 40.0))


def test_swipe_uses_the_from_target():
    targets = {'t1': {'type': 'template', 'asset': 'sha-from', 'assetBox': {'w': 10, 'h': 10}},
               't2': {'type': 'template', 'asset': 'sha-to', 'assetBox': {'w': 10, 'h': 10}}}
    sha, _ = step_asset({'op': 'swipe', 'from': 't1', 'to': 't2'}, targets)
    assert sha == 'sha-from'


def test_wait_for_uses_a_candidate_target():
    targets = {'t9': {'type': 'template', 'asset': 'sha-wait', 'assetBox': {'w': 8, 'h': 8}}}
    sha, _ = step_asset({'op': 'waitFor', 'any': ['t9']}, targets)
    assert sha == 'sha-wait'


def test_point_targets_and_plain_steps_have_no_crop():
    targets = {'p1': {'type': 'point', 'norm': (0.5, 0.5), 'zone': (0.4, 0.4, 0.2, 0.2)}}
    assert step_asset({'op': 'tap', 'target': 'p1'}, targets) == ('', None)
    assert step_asset({'op': 'sleep', 'ms': 10}, {}) == ('', None)
    assert step_target_name({'op': 'tap', 'target': 'missing'}, {}) is None


def test_caption_shows_name_then_op_and_parameter():
    # 第一行是步骤名，第二行是操作类型与参数。
    # Line one is the step name; line two is op plus its parameter.
    assert step_caption({'op': 'sleep', 'ms': 800}, None) == '等待 800 ms\n等待 · 800 ms'
    assert step_caption({'op': 'tap'}, (60.0, 40.0), targets={'t1': {}}) == '点击\n点击 · 60 × 40 px'
    assert step_caption({'op': 'tap'}, (60.0, 40.0), 0) == '1. 点击\n点击 · 60 × 40 px'
    # An unknown op still renders rather than crashing the list.
    assert step_caption({'op': 'mystery'}, None) == 'mystery\nmystery'


def test_custom_name_overrides_the_generated_one():
    step = {'op': 'tap', 'target': 't1', 'name': '点击购买按钮'}
    assert step_title(step, {'t1': {}}) == '点击购买按钮'
    assert step_caption(step, None, 2) == '3. 点击购买按钮\n点击'
    # 空白名称视为没有名字，退回默认名称。
    # A blank name counts as unset and falls back to the generated one.
    assert step_title({'op': 'tap', 'target': 't1', 'name': '   '}, {'t1': {}}) == '点击 t1'


def test_default_name_distinguishes_steps_of_the_same_op():
    targets = {'t1': {'type': 'template'}, 't2': {'type': 'template'}}
    first = default_step_name({'op': 'tap', 'target': 't1'}, targets)
    second = default_step_name({'op': 'tap', 'target': 't2'}, targets)
    assert first != second
    assert first == '点击 t1'
    assert default_step_name({'op': 'sleep', 'ms': 500}, {}) == '等待 500 ms'


def test_rows_carry_the_asset_for_double_click(app, asset_dir):
    sha = write_asset(asset_dir, 'a', 60, 40)
    steps = StepList(asset_dir)
    steps.show_steps(make_ir(sha))
    assert steps.count() == 1
    assert steps.item(0).data(0x0100) == sha  # Qt.ItemDataRole.UserRole
    assert not steps.item(0).icon().isNull()


def test_missing_asset_still_produces_a_row(app, asset_dir):
    steps = StepList(asset_dir)
    steps.show_steps(make_ir('0' * 64))
    assert steps.count() == 1
    # A missing file must be visible in the list, not silently blank.
    assert not steps.item(0).icon().isNull()


def test_empty_ir_clears_the_list(app, asset_dir):
    steps = StepList(asset_dir)
    steps.show_steps(make_ir(write_asset(asset_dir, 'a', 10, 10)))
    assert steps.count() == 1
    steps.show_steps(None)
    assert steps.count() == 0
    steps.show_steps({'targets': {}, 'blocks': {}})
    assert steps.count() == 0

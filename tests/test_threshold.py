"""threshold 真正拦截：低分命中等同于没找到，走 not_found 中止。

同时固定那些"不该被拦截"的路径，避免以后把 waitGone 或条件判断也一起改坏。
"""
import numpy as np
import pytest

from skycore.frame import Frame
from skycore.geometry import Bounds, Rect
from skycore.matcher.engine import Hit
from skycore.report import Reporter
from skycore.runtime import RunContext, execute
from skycore.schema import IRDoc

BOX = Rect(100, 100, 100, 50)
ANCHOR = (150.0, 125.0)


class RecordingDriver:
    def __init__(self):
        self.taps = []
        self.swipes = []

    def bounds(self):
        return Bounds(0, 0, 800, 600, 1)

    def capture(self):
        return Frame(np.zeros((600, 800, 3), dtype=np.uint8), Bounds(0, 0, 800, 600, 1))

    def tap(self, x, y, duration_ms=60):
        self.taps.append((x, y))

    def swipe(self, x1, y1, x2, y2, duration_ms=300):
        self.swipes.append((x1, y1, x2, y2))


class ScoringEngine:
    """按分数返回命中；score 为 None 表示完全没匹配到。"""

    def __init__(self, score):
        self.score = score

    def resolve(self, target, name, frame):
        if self.score is None:
            return None
        return Hit(box=BOX, point=ANCHOR, score=self.score, strategy='tpl', elapsed_ms=1.0)


def targets(threshold=0.82, warn_threshold=0.7):
    return {
        't1': {'type': 'template', 'asset': 'a' * 64, 'assetBox': {'w': 10, 'h': 10},
               'roi': [0, 0, 1, 1], 'threshold': threshold, 'warnThreshold': warn_threshold},
        't2': {'type': 'template', 'asset': 'b' * 64, 'assetBox': {'w': 10, 'h': 10},
               'roi': [0, 0, 1, 1], 'threshold': threshold, 'warnThreshold': warn_threshold},
    }


def run(steps, score, **kwargs):
    ir = IRDoc.model_validate({
        'irVersion': 1, 'meta': {'name': 'x'}, 'entry': 'main', 'vars': {},
        'targets': kwargs.get('targets') or targets(),
        'blocks': {'main': steps},
    })
    driver = RecordingDriver()
    reporter = Reporter('r')
    ctx = RunContext(ir=ir, driver=driver, engine=ScoringEngine(score), reporter=reporter, run_id='r')
    return execute(ir, ctx), driver, reporter


def test_low_score_aborts_and_never_clicks():
    result, driver, _ = run([{'op': 'tap', 'id': 's1', 'target': 't1'}], 0.55)
    assert result.state == 'failed'
    assert result.error_code == 'not_found'
    # 低于阈值绝不允许注入输入。
    assert driver.taps == []
    # 错误信息要能看出是分数不够，而不是完全没匹配到。
    assert 'below threshold' in result.error_msg
    assert '0.550' in result.error_msg


def test_low_score_is_logged_as_a_rejection_with_context():
    _, _, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1'}], 0.55)
    rejected = [line for line in reporter.log_lines if line['event'] == 'match_rejected']
    assert rejected
    payload = rejected[0]['payload']
    assert payload['target'] == 't1'
    assert payload['score'] == 0.55
    assert payload['threshold'] == 0.82
    # 坐标一并记下，便于判断模板是在哪里误匹配的。
    assert 'x' in payload and 'y' in payload


@pytest.mark.parametrize('score', [0.82, 0.83, 0.99])
def test_scores_at_or_above_the_threshold_click(score):
    result, driver, _ = run([{'op': 'tap', 'id': 's1', 'target': 't1'}], score)
    assert result.state == 'done'
    assert driver.taps == [ANCHOR]


def test_missing_match_and_low_score_report_different_messages():
    _, _, _ = run([], None)
    missing, _, _ = run([{'op': 'tap', 'id': 's1', 'target': 't1'}], None)
    low, _, _ = run([{'op': 'tap', 'id': 's1', 'target': 't1'}], 0.5)
    assert missing.error_msg != low.error_msg
    assert 'not matched' in missing.error_msg
    assert 'below threshold' in low.error_msg


def test_swipe_is_rejected_when_either_end_is_below_threshold():
    result, driver, _ = run(
        [{'op': 'swipe', 'id': 's1', 'from': 't1', 'to': 't2'}], 0.5)
    assert result.state == 'failed'
    assert result.error_code == 'not_found'
    assert driver.swipes == []


def test_long_press_also_obeys_the_threshold():
    result, driver, _ = run(
        [{'op': 'longPress', 'id': 's1', 'target': 't1'}], 0.5)
    assert result.state == 'failed'
    assert driver.taps == []


def test_wait_gone_is_not_fooled_by_a_low_score():
    # 低分说明画面在变，不代表目标消失；判成"已消失"会让脚本提前继续。
    # A low score means the screen is moving, not that the target is gone.
    result, _, _ = run(
        [{'op': 'waitGone', 'id': 's1', 'target': 't1', 'timeoutMs': 300, 'intervalMs': 60}], 0.55)
    assert result.state == 'failed'
    assert result.error_code == 'timeout'


def test_conditions_ignore_the_threshold():
    steps = [{'op': 'if', 'id': 's1', 'when': {'missing': 't1'},
              'then': [{'op': 'log', 'id': 'a', 'msg': 'then'}],
              'else': [{'op': 'log', 'id': 'b', 'msg': 'else'}]}]
    result, _, reporter = run(steps, 0.55)
    assert result.state == 'done'
    messages = [line['payload'].get('msg') for line in reporter.log_lines if line['event'] == 'script_log']
    # 目标还在（只是分数低），应该走 else。
    assert messages == ['else']


def test_wait_for_still_times_out_on_a_low_score():
    # waitFor 期待"出现"，低分不该算出现，否则会点错位置。
    # waitFor expects the target to appear; a low score must not count.
    result, _, _ = run(
        [{'op': 'waitFor', 'id': 's1', 'any': ['t1'], 'timeoutMs': 300, 'intervalMs': 60}], 0.55)
    assert result.state == 'failed'
    assert result.error_code == 'timeout'


def test_threshold_uses_the_targets_own_value():
    # 阈值是每个目标自己的字段，改它应该直接改变结果。
    permissive = targets(threshold=0.4)
    result, driver, _ = run([{'op': 'tap', 'id': 's1', 'target': 't1'}], 0.55,
                            targets=permissive)
    assert result.state == 'done'
    assert driver.taps == [ANCHOR]


def test_point_targets_are_unaffected_by_threshold():
    # 点目标没有图像匹配分数（真实引擎固定给 1.0），阈值不应把它拦掉。
    # Point targets carry no image score (the engine reports 1.0), so the
    # threshold must not reject them.
    point_targets = {'p1': {'type': 'point', 'norm': (0.5, 0.5), 'zone': (0.4, 0.4, 0.2, 0.2)}}
    ir = IRDoc.model_validate({
        'irVersion': 1, 'meta': {'name': 'x'}, 'entry': 'main', 'vars': {},
        'targets': point_targets,
        'blocks': {'main': [{'op': 'tap', 'id': 's1', 'target': 'p1'}]},
    })
    driver = RecordingDriver()
    reporter = Reporter('r')
    from skycore.matcher import AssetStore
    from skycore.matcher.engine import MatcherEngine
    ctx = RunContext(ir=ir, driver=driver, engine=MatcherEngine(AssetStore()),
                     reporter=reporter, run_id='r')
    result = execute(ir, ctx)
    assert result.state == 'done'
    assert len(driver.taps) == 1

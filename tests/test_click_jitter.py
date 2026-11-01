"""点击的随机延迟、区域内随机偏移，以及坐标日志。"""
import re
import time

import numpy as np
import pytest

from skycore.frame import Frame
from skycore.geometry import Bounds, Rect
from skycore.matcher.engine import Hit
from skycore.report import Reporter
from skycore.runtime import Interpreter, RunContext
from skycore.schema import IRDoc


class RecordingDriver:
    def __init__(self):
        self.taps = []

    def bounds(self):
        return Bounds(0, 0, 800, 600, 1)

    def capture(self):
        return Frame(np.zeros((600, 800, 3), dtype=np.uint8), Bounds(0, 0, 800, 600, 1))

    def tap(self, x, y, duration_ms=60):
        self.taps.append((x, y, duration_ms))


class FixedEngine:
    """总是命中同一个框，便于判断偏移是否发生在框内。"""

    def __init__(self, box=Rect(100, 100, 200, 100), point=(200.0, 150.0), score=0.95):
        self.box = box
        self.point = point
        self.score = score

    def resolve(self, target, name, frame):
        return Hit(box=self.box, point=self.point, score=self.score, strategy='tpl', elapsed_ms=1.0)


def make_ir(steps):
    return IRDoc.model_validate({
        'irVersion': 1, 'meta': {'name': 'j'}, 'entry': 'main', 'vars': {},
        'targets': {'t1': {'type': 'template', 'asset': 'a' * 64,
                           'assetBox': {'w': 10, 'h': 10}, 'roi': [0, 0, 1, 1]}},
        'blocks': {'main': steps},
    })


def run(steps, engine=None):
    driver = RecordingDriver()
    reporter = Reporter('run')
    ir = make_ir(steps)
    ctx = RunContext(ir=ir, driver=driver, engine=engine or FixedEngine(), reporter=reporter, run_id='run')
    Interpreter(ctx).run()
    return driver, reporter


def tap_events(reporter):
    return [line['payload'] for line in reporter.log_lines if line['event'] == 'tap']


def test_plain_tap_hits_the_anchor_and_logs_it():
    driver, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1'}])
    assert driver.taps == [(200.0, 150.0, 60)]
    event = tap_events(reporter)[0]
    assert (event['x'], event['y']) == (200.0, 150.0)
    # 没有配置偏移时必须如实报告 0，便于确认"偏移没开"而不是"偏移没生效"。
    assert (event['dx'], event['dy']) == (0.0, 0.0)
    assert event['delayMs'] == 0


def test_offset_stays_inside_the_matched_region():
    driver, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1', 'offsetRatio': 0.4}])
    x, y, _ = driver.taps[0]
    box = Rect(100, 100, 200, 100)
    assert box.x <= x <= box.x + box.w
    assert box.y <= y <= box.y + box.h
    event = tap_events(reporter)[0]
    assert event['x'] == pytest.approx(x, abs=0.01)
    assert event['boxW'] == 200 and event['boxH'] == 100


def test_offset_varies_between_runs():
    steps = [{'op': 'tap', 'id': f's{i}', 'target': 't1', 'offsetRatio': 0.45} for i in range(12)]
    driver, _ = run(steps)
    points = {(round(x, 1), round(y, 1)) for x, y, _ in driver.taps}
    assert len(points) > 1


def test_offset_is_logged_as_a_delta_from_the_anchor():
    driver, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1', 'offsetRatio': 0.3}])
    event = tap_events(reporter)[0]
    assert event['anchorX'] == 200.0 and event['anchorY'] == 150.0
    assert event['x'] == pytest.approx(200.0 + event['dx'], abs=0.01)
    assert event['y'] == pytest.approx(150.0 + event['dy'], abs=0.01)


def test_large_offset_is_clamped_and_reported():
    # 大偏移把点推到框外时，必须夹回框内并留下可见记录。
    # A large offset is clamped back inside the box, with a visible warning.
    engine = FixedEngine(box=Rect(100, 100, 20, 20), point=(110.0, 110.0))
    driver, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1', 'offsetRatio': 0.5}], engine)
    x, y, _ = driver.taps[0]
    assert 100 <= x <= 120 and 100 <= y <= 120
    # offsetRatio > 0.5 会被收窄，并记录一次 warn。
    _, reporter2 = run([{'op': 'tap', 'id': 's2', 'target': 't1', 'offsetRatio': 3.0}], engine)
    clamped = [line for line in reporter2.log_lines if line['event'] == 'offset_clamped']
    assert clamped and clamped[0]['payload']['used'] == 0.5


def test_fixed_delay_is_waited_and_logged():
    started = time.monotonic()
    _, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1', 'delayMs': 120}])
    elapsed_ms = (time.monotonic() - started) * 1000
    assert elapsed_ms >= 110
    assert tap_events(reporter)[0]['delayMs'] >= 100


def test_random_delay_range_is_respected():
    delays = []
    for _ in range(8):
        _, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1', 'delayRangeMs': [10, 20]}])
        delays.append(tap_events(reporter)[0]['delayMs'])
    assert all(9 <= delay <= 21 for delay in delays)


def test_fixed_delay_and_range_add_up():
    _, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1',
                        'delayMs': 50, 'delayRangeMs': [10, 20]}])
    assert 59 <= tap_events(reporter)[0]['delayMs'] <= 71


def test_tap_log_carries_a_timestamp_and_score():
    _, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1'}])
    event = tap_events(reporter)[0]
    assert re.fullmatch(r'\d{2}:\d{2}:\d{2}', event['at'])
    assert event['score'] == 0.95


def test_long_press_uses_the_same_jitter_path():
    driver, reporter = run([{'op': 'longPress', 'id': 's1', 'target': 't1', 'offsetRatio': 0.2}])
    assert driver.taps[0][2] == 600
    assert tap_events(reporter)[0]['target'] == 't1'


def test_reused_hit_also_gets_the_offset():
    # 缓存命中时若不应用偏移，第二次点击会回到正中心，看起来像偏移失效。
    # A reused match must be offset too, otherwise the second tap snaps back.
    engine = FixedEngine()

    class ReusingEngine(FixedEngine):
        def resolve(self, target, name, frame):
            hit = super().resolve(target, name, frame)
            hit.reused = True
            hit.strategy = 'reuse'
            return hit

    driver, _ = run([{'op': 'tap', 'id': 's1', 'target': 't1', 'offsetRatio': 0.4}] * 2,
                    ReusingEngine())
    assert driver.taps[0][:2] != driver.taps[1][:2] or driver.taps[0][0] != 200.0


def test_bad_step_values_do_not_crash_the_run():
    # 手改 IR 写出非数字时，按未配置处理而不是让整次运行失败。
    # Hand-edited non-numeric values fall back to "not configured".
    driver, reporter = run([{'op': 'tap', 'id': 's1', 'target': 't1',
                             'delayMs': 'abc', 'offsetRatio': 'xyz'}])
    assert driver.taps == [(200.0, 150.0, 60)]
    assert tap_events(reporter)[0]['delayMs'] == 0


def test_subscriber_receives_events_without_breaking_the_run():
    seen = []
    reporter = Reporter('run')
    reporter.subscribe(lambda level, event, payload: seen.append(event))

    def boom(level, event, payload):
        raise RuntimeError('subscriber exploded')

    reporter.subscribe(boom)
    reporter.log('info', 'tap', {'x': 1})
    assert 'tap' in seen
    # 订阅者抛错不能中断记录，也不能悄悄吞掉。
    assert any(line['event'] == 'log_subscriber_failed' for line in reporter.log_lines)

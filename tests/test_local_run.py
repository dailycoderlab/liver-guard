"""Regression coverage for local execution and asset path safety."""
import threading

import pytest

from skycore.matcher import AssetStore
from skycore.runner import run_local
from skycore.schema import IRDoc


def test_local_run_uses_shared_runtime_and_reports_steps():
    ir = IRDoc.model_validate({'meta': {'name': 'test'}, 'blocks': {'main': [
        {'op': 'sleep', 'id': 'wait', 'ms': 1},
    ]}, 'targets': {}})
    steps = []
    result, reporter = run_local(ir, object(), AssetStore(), 'run', threading.Event(), steps.append)
    assert result.state == 'done'
    assert result.step_count == 1
    assert steps[0].step_id == 'wait'
    assert 'run_done' in reporter.jsonl()


def test_rejects_unsafe_asset_path():
    ir = IRDoc.model_validate({'meta': {'name': 'test'}, 'blocks': {'main': []}, 'targets': {
        't': {'type': 'template', 'asset': '../x', 'assetBox': {'w': 5, 'h': 5}, 'roi': [0, 0, 1, 1]}}})
    with pytest.raises(ValueError, match='Invalid asset'):
        run_local(ir, object(), AssetStore(), 'run')

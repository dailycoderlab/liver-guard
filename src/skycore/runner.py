"""Local script execution, independent of any GUI."""
from __future__ import annotations

import re
import threading
from typing import Callable

from .driver import Driver
from .matcher import AssetStore, MatcherEngine
from .report import Reporter
from .runtime import RunContext, RunResult, StepReport, execute
from .schema import IRDoc


def run_local(ir: IRDoc, driver: Driver, assets: AssetStore, run_id: str,
              stop_event: threading.Event | None = None,
              on_step: Callable[[StepReport], None] | None = None) -> tuple[RunResult, Reporter]:
    problems = ir.validate()
    if problems:
        raise ValueError('; '.join(problems))
    # Reject unsafe cache paths and normalize older sha256:-prefixed references.
    for target in ir.targets.values():
        if getattr(target, 'type', None) == 'template':
            sha = target.asset.removeprefix('sha256:')
            if not re.fullmatch(r'[0-9a-f]{64}', sha):
                raise ValueError('Invalid asset hash')
            target.asset = sha
    for sha in ir.asset_hashes():
        assets.get(sha)
    reporter = Reporter(run_id=run_id)
    context = RunContext(ir=ir, driver=driver, engine=MatcherEngine(assets), reporter=reporter,
                         run_id=run_id, stop_event=stop_event, on_step=on_step)
    return execute(ir, context), reporter

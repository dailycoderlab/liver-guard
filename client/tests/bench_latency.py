#!/usr/bin/env python3
"""Latency budget check on the real surface: capture and one full resolve."""

from __future__ import annotations

import asyncio
import sys
import time
from pathlib import Path

import cv2

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from skycore.matcher import AssetStore, MatcherEngine  # noqa: E402
from skycore.schema import TemplateTarget  # noqa: E402
from skymac.driver import MacDriver  # noqa: E402


async def main() -> None:
    driver = MacDriver(window_query=sys.argv[1] if len(sys.argv) > 1 else None, full_screen=len(sys.argv) <= 1)
    driver.refresh()

    for index in range(3):
        started = time.perf_counter()
        frame = driver.capture()
        print(f"capture (main thread)   {(time.perf_counter() - started) * 1000:7.1f} ms  {frame.width}x{frame.height}")

    for index in range(3):
        started = time.perf_counter()
        await asyncio.to_thread(driver.capture)
        print(f"capture (worker thread) {(time.perf_counter() - started) * 1000:7.1f} ms")

    frame = driver.capture()
    size = 140

    height, width = frame.image.shape[:2]
    crop = frame.image[height // 4 : height // 4 + size, width // 4 : width // 4 + size]
    ok, png = cv2.imencode(".png", crop)
    assert ok
    sha = "b" * 64
    store = AssetStore()
    store.put_bytes(sha, png.tobytes())
    engine = MatcherEngine(store, allow_reuse=False)
    target = TemplateTarget(
        type="template",
        asset=sha,
        assetBox={"w": size, "h": size},
        recordScale=frame.scale,
        recordWidth=frame.bounds.w,
        roi=(0.0, 0.0, 1.0, 1.0),
        anchor=(0.5, 0.5),
        strategy=["tpl", "mstpl"],
    )

    for label_roi, roi in (("full frame roi", (0.0, 0.0, 1.0, 1.0)), ("tight roi", (0.2, 0.2, 0.2, 0.2))):
        target.roi = roi
        for index in range(3):
            engine.tracker.clear()
            started = time.perf_counter()
            hit = engine.resolve(target, "x", frame)
            score = "MISS" if hit is None else f"{hit.score:.3f}"
            print(f"resolve {label_roi:<15} {(time.perf_counter() - started) * 1000:7.1f} ms  {score}")


if __name__ == "__main__":
    asyncio.run(main())

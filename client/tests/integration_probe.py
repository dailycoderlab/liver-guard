#!/usr/bin/env python3
"""Real-screen integration check: record a crop from a live window, publish it
through the platform, then locate it again with the device matcher.

This is the closest thing to the full product loop that can run without a
browser: capture -> crop -> upload -> publish -> bundle -> match.

    client/.venv/bin/python client/tests/integration_probe.py --window 阴阳师

Requires a running dev server (`wrangler dev`) and screen recording permission.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
import urllib.request
from pathlib import Path

import cv2
import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from skycore.geometry import anchor_in_box, infer_roi  # noqa: E402
from skycore.matcher import AssetStore, MatcherEngine  # noqa: E402
from skycore.schema import IRDoc  # noqa: E402
from skymac.driver import MacDriver  # noqa: E402

SERVER = "http://127.0.0.1:8787"
PASSWORD = "sky-dev"
CROP = 140


def http(method: str, path: str, body=None, token: str | None = None, raw: bytes | None = None, ctype: str | None = None):
    headers = {}
    data = raw
    if body is not None:
        data = json.dumps(body).encode()
        headers["content-type"] = "application/json"
    if raw is not None and ctype:
        headers["content-type"] = ctype
    if token:
        headers["authorization"] = f"Bearer {token}"
    request = urllib.request.Request(SERVER + path, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = response.read()
        if response.headers.get("content-type", "").startswith("application/json"):
            return json.loads(payload or b"null")
        return payload


def most_textured_patch(image: np.ndarray, size: int) -> tuple[int, int]:
    """Stand in for "the operator clicked a button": find the busiest box."""
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY).astype(np.float32)
    # Edge energy per pixel, box filtered, then take the maximum.
    gx = cv2.Sobel(gray, cv2.CV_32F, 1, 0, ksize=3)
    gy = cv2.Sobel(gray, cv2.CV_32F, 0, 1, ksize=3)
    energy = cv2.magnitude(gx, gy)
    score = cv2.boxFilter(energy, -1, (size, size), normalize=True)
    margin = size // 2 + 4
    inner = score[margin : score.shape[0] - margin, margin : score.shape[1] - margin]
    _, _, _, max_loc = cv2.minMaxLoc(inner)
    return max_loc[0] + margin, max_loc[1] + margin


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--window", default=None, help="window substring; omit for the whole display")
    args = parser.parse_args()

    driver = MacDriver(window_query=args.window, full_screen=args.window is None)
    surface = driver.refresh()
    frame = driver.capture()
    bounds = frame.bounds
    print(f"surface  {surface.label}  {bounds.w:.0f}x{bounds.h:.0f} pt @ scale {frame.scale:.2f}")

    cx, cy = most_textured_patch(frame.image, CROP)
    half = CROP // 2
    crop = frame.image[cy - half : cy + half, cx - half : cx + half].copy()
    ok, png = cv2.imencode(".png", crop)
    assert ok
    payload = png.tobytes()
    sha = hashlib.sha256(payload).hexdigest()

    logical_box = {
        "x": bounds.x + (cx - half) / frame.scale,
        "y": bounds.y + (cy - half) / frame.scale,
        "w": CROP / frame.scale,
        "h": CROP / frame.scale,
    }
    from skycore.geometry import Rect

    box = Rect(logical_box["x"], logical_box["y"], logical_box["w"], logical_box["h"])
    click = (box.x + box.w / 2, box.y + box.h / 2)
    anchor = anchor_in_box(click, box)
    roi = infer_roi(box, bounds)
    print(f"crop     {CROP}x{CROP} px at ({cx},{cy}) -> logical box {box.w:.0f}x{box.h:.0f} pt")
    print(f"roi      {roi}")

    token = http("POST", "/v1/auth/login", {"password": PASSWORD})["token"]
    uploaded = http("PUT", f"/v1/assets/{sha}?w={CROP}&h={CROP}&scale={frame.scale}", token=token, raw=payload, ctype="image/png")
    print(f"asset    {uploaded['url']}")

    script = http("POST", "/v1/scripts", {"name": "integration probe"}, token)["script"]
    ir = {
        "irVersion": 1,
        "meta": {"name": "integration probe", "maxDurationMs": 60000},
        "entry": "main",
        "blocks": {"main": [{"op": "tap", "target": "found"}]},
        "targets": {
            "found": {
                "type": "template",
                "asset": sha,
                "assetBox": {"w": CROP, "h": CROP},
                "recordScale": frame.scale,
                "recordWidth": bounds.w,
                "roi": list(roi),
                "anchor": list(anchor),
                "threshold": 0.82,
                "strategy": ["tpl", "mstpl"],
            }
        },
    }
    version = http("POST", f"/v1/scripts/{script['id']}/publish", {"ir": ir}, token)
    print(f"published v{version['version']}")

    bundle = http("GET", f"/v1/scripts/{script['id']}/bundle?v=1", token=token)
    parsed = IRDoc.model_validate(bundle["ir"])
    problems = parsed.validate()
    if problems:
        print("ir problems:", problems)
        return 3

    store = AssetStore(cache_dir=Path("/tmp/sky_integration_assets"), fetcher=lambda s: http("GET", f"/v1/assets/{s}"))
    engine = MatcherEngine(store, allow_reuse=False)
    target = parsed.targets["found"]

    failures = 0

    hit = engine.resolve(target, "found", frame)
    print(f"match    {'MISS' if hit is None else f'score {hit.score:.3f} strategy {hit.strategy} {hit.elapsed_ms:.0f}ms'}")
    if hit is None or hit.score < 0.9:
        failures += 1
    else:
        dx = hit.point[0] - click[0]
        dy = hit.point[1] - click[1]
        print(f"delta    {dx:+.1f}, {dy:+.1f} pt from the recorded click")
        if abs(dx) > 3 or abs(dy) > 3:
            failures += 1

    # The same crop has to survive a different pixel density and a different
    # surface size, which is the whole reason the normalization exists.
    for factor, label in ((0.8, "80% pixels"), (1.25, "125% pixels")):
        scaled_image = cv2.resize(
            frame.image,
            (int(frame.image.shape[1] * factor), int(frame.image.shape[0] * factor)),
            interpolation=cv2.INTER_AREA if factor < 1 else cv2.INTER_LINEAR,
        )
        from skycore.frame import Frame

        scaled = Frame(image=scaled_image, bounds=bounds, seq=2)
        engine.tracker.clear()
        scaled_hit = engine.resolve(target, "found", scaled)
        if scaled_hit is None:
            print(f"rescale  {label:<12} MISS")
            failures += 1
        else:
            print(f"rescale  {label:<12} score {scaled_hit.score:.3f} at ({scaled_hit.point[0]:.0f},{scaled_hit.point[1]:.0f})")
            if scaled_hit.score < 0.8 or abs(scaled_hit.point[0] - click[0]) > 6:
                failures += 1

    # A search region with no control in it must miss, not guess.
    empty_target = parsed.targets["found"].model_copy(deep=True)
    empty_target.roi = (0.02, 0.02, 0.06, 0.06)
    engine.tracker.clear()
    miss = engine.resolve(empty_target, "empty", frame)
    print(f"empty roi {'MISS (correct)' if miss is None or miss.score < 0.8 else f'MATCHED {miss.score:.3f} (wrong)'}")
    if miss is not None and miss.score >= 0.8:
        failures += 1

    print()
    print("PASS" if failures == 0 else f"{failures} check(s) failed")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main())

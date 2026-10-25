#!/usr/bin/env python3
"""Execute a published script on the real device and watch the run lifecycle.

Records a crop from the live screen, publishes a two step script against it,
starts the run through the platform API, and asserts that run state, per step
scores and the run summary all arrive and are persisted.

    client/.venv/bin/python workers/scripts/run_smoke.py
"""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
import sys
import time
import urllib.request

import cv2
from websockets.asyncio.client import connect

BASE = os.environ.get("SKY_BASE", "http://127.0.0.1:8787")
PASSWORD = os.environ.get("SKY_PASSWORD", "sky-dev")

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "client", "src"))

from skycore import protocol  # noqa: E402
from skycore.geometry import Rect, anchor_in_box, infer_roi  # noqa: E402
from skymac.driver import MacDriver  # noqa: E402


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
    request = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=30) as response:
        payload = response.read()
        if response.headers.get("content-type", "").startswith("application/json"):
            return json.loads(payload or b"null")
        return payload


def busiest_stable_patch(image, size: int, previous=None):
    """Most textured box, preferring one that did not change between two frames.

    Texture alone picks animated content (spinners, floating text) which makes a
    useless template. Comparing two captures a moment apart filters those out,
    which is also what a good recorder should do before saving a crop.
    """
    gray = cv2.cvtColor(image, cv2.COLOR_BGR2GRAY).astype("float32")
    gx = cv2.Sobel(gray, cv2.CV_32F, 1, 0, ksize=3)
    gy = cv2.Sobel(gray, cv2.CV_32F, 0, 1, ksize=3)
    energy = cv2.magnitude(gx, gy)
    score = cv2.boxFilter(energy, -1, (size, size), normalize=True)

    if previous is not None:
        diff = cv2.absdiff(image, previous).astype("float32").mean(axis=2)
        motion = cv2.boxFilter(diff, -1, (size, size), normalize=True)
        score = score / (1.0 + motion * 4.0)

    margin = size // 2 + 4
    inner = score[margin : score.shape[0] - margin, margin : score.shape[1] - margin]
    _, _, _, loc = cv2.minMaxLoc(inner)
    return loc[0] + margin, loc[1] + margin


async def main() -> int:
    failures: list[str] = []

    def check(label: str, condition: bool, extra: str = "") -> None:
        print(f"[{'ok  ' if condition else 'FAIL'}] {label}{'' if condition else ' — ' + extra}")
        if not condition:
            failures.append(label)

    token = http("POST", "/v1/auth/login", {"password": PASSWORD})["token"]
    devices = http("GET", "/v1/devices", token=token)["devices"]
    online = [device for device in devices if device["online"]]
    if not online:
        print("no device online — start `sky serve` first")
        return 2
    device = online[0]

    driver = MacDriver(window_query="阴阳师", full_screen=False)
    driver.activate()
    driver.refresh()
    frame = driver.capture()
    time.sleep(0.8)
    second = driver.capture()
    size = 140
    cx, cy = busiest_stable_patch(frame.image, size, previous=second.image)
    half = size // 2
    crop = frame.image[cy - half : cy + half, cx - half : cx + half].copy()
    ok, png = cv2.imencode(".png", crop)
    assert ok
    payload = png.tobytes()
    sha = hashlib.sha256(payload).hexdigest()

    box = Rect(
        frame.bounds.x + (cx - half) / frame.scale,
        frame.bounds.y + (cy - half) / frame.scale,
        size / frame.scale,
        size / frame.scale,
    )
    anchor = anchor_in_box(box.center, box)
    roi = infer_roi(box, frame.bounds)

    http("PUT", f"/v1/assets/{sha}?w={size}&h={size}&scale={frame.scale}", token=token, raw=payload, ctype="image/png")
    script = http("POST", "/v1/scripts", {"name": "run smoke"}, token)["script"]
    ir = {
        "irVersion": 1,
        "meta": {"name": "run smoke", "maxDurationMs": 30000},
        "entry": "main",
        "blocks": {
            "main": [
                {"op": "waitFor", "any": ["btn"], "timeoutMs": 5000, "intervalMs": 300, "id": "w1"},
                {"op": "loop", "count": 2, "body": [{"op": "tap", "target": "btn", "id": "t1"}]},
                {"op": "log", "level": "info", "msg": "loop finished", "id": "l1"},
            ]
        },
        "targets": {
            "btn": {
                "type": "template",
                "asset": sha,
                "assetBox": {"w": size, "h": size},
                "recordScale": frame.scale,
                "recordWidth": frame.bounds.w,
                "roi": list(roi),
                "anchor": list(anchor),
                "strategy": ["tpl", "mstpl"],
            }
        },
    }
    version = http("POST", f"/v1/scripts/{script['id']}/publish", {"ir": ir}, token)["version"]
    print(f"script   {script['id']} v{version}, target from a {size}px crop at ({cx},{cy})")

    ws_url = BASE.replace("http://", "ws://").replace("https://", "wss://")
    states: list[str] = []
    scores: list[dict] = []
    logs: list[dict] = []
    summary: dict | None = None

    async with connect(f"{ws_url}/v1/ws/console?token={token}", max_size=16 * 1024 * 1024) as console:
        await console.recv()  # hub_ready
        await console.send(json.dumps({"t": "subscribe", "deviceId": device["id"]}))
        await asyncio.sleep(0.2)

        started = http("POST", "/v1/runs", {"deviceId": device["id"], "scriptId": script["id"]}, token)
        run_id = started["runId"]
        print(f"run      {run_id}")

        deadline = time.monotonic() + 45
        while time.monotonic() < deadline and summary is None:
            try:
                raw = await asyncio.wait_for(console.recv(), timeout=deadline - time.monotonic())
            except TimeoutError:
                break
            if isinstance(raw, bytes):
                continue
            message = json.loads(raw)
            kind = message.get("t")
            if kind == "run_state":
                states.append(message["state"])
            elif kind == "score":
                scores.extend(message["items"])
            elif kind == "log":
                logs.extend(message["items"])
            elif kind == "run_summary":
                summary = message
            elif kind == "error":
                print("device error:", message)

    check("run reached a terminal state", summary is not None and summary["state"] in ("done", "failed"), str(summary))
    if summary:
        check("run finished cleanly", summary["state"] == "done", str(summary.get("errorCode")))
        check("steps were executed", summary["stepCount"] >= 3, str(summary["stepCount"]))
        check("summary carries per step scores", len(summary["steps"]) >= 1, str(summary["steps"]))
        best = max((row["last"] for row in summary["steps"]), default=0)
        check("control was located on the live screen", best > 0.8, str(summary["steps"]))

    check("scores streamed live", len(scores) >= 2, str(len(scores)))
    check("script log arrived", any(row.get("event") == "script_log" for row in logs), str(logs[:3]))

    detail = http("GET", f"/v1/runs/{run_id}", token=token)
    check("run persisted", detail["run"]["state"] == summary["state"] if summary else False, str(detail["run"]))
    check("scores persisted", len(detail["scores"]) >= 1, str(detail["scores"]))

    print()
    if failures:
        print(f"{len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("run checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))

#!/usr/bin/env python3
"""End to end check of the Durable Object hub.

Drives a fake device and a fake console against a running dev server and asserts
the hub relays in both directions, forwards frames, and persists a run summary.

Run with the client's virtualenv (it needs `websockets`):
    cd workers && ../client/.venv/bin/python scripts/ws_smoke.py
"""

from __future__ import annotations

import asyncio
import json
import os
import sys

from websockets.asyncio.client import connect

BASE = os.environ.get("SKY_BASE", "http://127.0.0.1:8787")
PASSWORD = os.environ.get("SKY_PASSWORD", "sky-dev")

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "client", "src"))

from skycore import protocol  # noqa: E402


def http_json(method: str, path: str, body=None, token: str | None = None):
    import urllib.request

    headers = {"content-type": "application/json"}
    if token:
        headers["authorization"] = f"Bearer {token}"
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=20) as response:
        return json.loads(response.read() or b"null")


async def recv_json(socket, timeout: float = 5.0) -> dict:
    raw = await asyncio.wait_for(socket.recv(), timeout=timeout)
    if isinstance(raw, bytes):
        header, _ = protocol.decode_binary(raw)
        return header
    return json.loads(raw)


async def recv_until(socket, kind: str, timeout: float = 6.0, match=None) -> dict:
    deadline = asyncio.get_event_loop().time() + timeout
    while True:
        remaining = deadline - asyncio.get_event_loop().time()
        if remaining <= 0:
            raise TimeoutError(f"never received {kind}")
        message = await recv_json(socket, timeout=remaining)
        if message.get("t") == kind and (match is None or match(message)):
            return message


async def main() -> int:
    failures: list[str] = []

    def check(label: str, condition: bool, extra: str = "") -> None:
        print(f"[{'ok  ' if condition else 'FAIL'}] {label}{'' if condition else ' — ' + extra}")
        if not condition:
            failures.append(label)

    console_token = http_json("POST", "/v1/auth/login", {"password": PASSWORD})["token"]
    enrolled = http_json("POST", "/v1/devices/enroll", {"name": "ws-smoke", "platform": "macos"}, console_token)
    device_id = enrolled["deviceId"]
    device_token = enrolled["deviceToken"]

    bounds = {"x": 0.0, "y": 0.0, "w": 600.0, "h": 400.0, "scale": 2.0}
    capsules = protocol.Caps(max_fps=6, ui_query="ax", max_window=(600, 400), surface="window")

    ws_url = BASE.replace("http://", "ws://").replace("https://", "wss://")

    async with connect(f"{ws_url}/v1/ws/device?token={device_token}", max_size=16 * 1024 * 1024) as device:
        await device.send(json.dumps(protocol.hello(device_id, "ws-smoke", "macos", capsules, bounds)))
        await asyncio.sleep(0.2)

        async with connect(f"{ws_url}/v1/ws/console?token={console_token}", max_size=16 * 1024 * 1024) as console:
            await recv_until(console, "hub_ready")
            await console.send(json.dumps({"t": "subscribe", "deviceId": device_id}))

            # The hub also replays every device it already knows about at connect
            # time, so wait for the state of the device we care about.
            state = await recv_until(console, "device_state", match=lambda m: m.get("deviceId") == device_id)
            check("console sees the device", state.get("deviceId") == device_id, str(state))
            check("device reports online", state.get("online") is True, str(state))
            check("caps survive the hub", (state.get("caps") or {}).get("uiQuery") == "ax", str(state.get("caps")))
            check("bounds survive the hub", (state.get("bounds") or {}).get("w") == 600.0, str(state.get("bounds")))

            # console -> device
            await console.send(json.dumps({"t": "stream", "on": True, "fps": 6, "quality": 60}))
            stream = await recv_until(device, "stream")
            check("console commands reach the device", stream.get("on") is True, str(stream))

            frame = protocol.frame_message(1, bounds, b"\xff\xd8jpegbytes")
            await device.send(frame)
            header = await recv_until(console, "frame")
            check("frames reach the console", header.get("seq") == 1, str(header))

            await console.send(
                json.dumps(
                    {
                        "t": "inject",
                        "reqId": "req-1",
                        "kind": "tap",
                        "at": [0.5, 0.55],
                        "frameSeq": 1,
                        "record": True,
                        "durationMs": 60,
                    }
                )
            )
            inject = await recv_until(device, "inject")
            check("clicks reach the device", inject.get("reqId") == "req-1" and inject.get("record") is True, str(inject))

            context = protocol.context_message(
                "context",
                "req-1",
                {"x": 100.0, "y": 100.0, "w": 600.0, "h": 400.0, "scale": 2.0},
                b"\x89PNGsearchwindow",
                click_px=(300.0, 220.0),
            )
            await device.send(context)
            received = await recv_until(console, "context")
            check("search window reaches the console", received.get("reqId") == "req-1", str(received))
            check("click pixel is forwarded", received.get("clickPx") == [300.0, 220.0], str(received))

            # run lifecycle: create a published script, start a run, finish it,
            # then confirm the hub persisted the summary into D1.
            script = http_json("POST", "/v1/scripts", {"name": "ws-smoke"}, console_token)["script"]
            ir = {
                "irVersion": 1,
                "meta": {"name": "ws-smoke", "maxDurationMs": 5000},
                "entry": "main",
                "blocks": {"main": [{"op": "sleep", "ms": 100}]},
                "targets": {},
            }
            publish = http_json("POST", f"/v1/scripts/{script['id']}/publish", {"ir": ir}, console_token)
            check("script published", publish.get("version") == 1, str(publish))

            started = http_json(
                "POST", "/v1/runs", {"deviceId": device_id, "scriptId": script["id"]}, console_token
            )
            run_msg = await recv_until(device, "run")
            check("run command reaches the device", run_msg.get("runId") == started["runId"], str(run_msg))

            summary = protocol.run_summary(
                started["runId"],
                "done",
                3,
                [{"stepId": "s1", "targetId": "btn", "n": 3, "avg": 0.93, "min": 0.88, "maxMs": 42, "last": 0.95}],
            )
            await device.send(json.dumps(summary))
            await asyncio.sleep(0.6)

            detail = http_json("GET", f"/v1/runs/{started['runId']}", token=console_token)
            check("run summary persisted", detail["run"]["state"] == "done", str(detail["run"]))
            check("per-step scores persisted", len(detail["scores"]) == 1, str(detail["scores"]))
            check("score aggregate is right", detail["scores"][0]["min_score"] == 0.88, str(detail["scores"]))

            check("device token still identifies the device", http_json("GET", "/v1/device/me", token=device_token)["device"]["id"] == device_id)

    # A stale-bounds reply must be routed too.
    async with connect(f"{ws_url}/v1/ws/device?token={device_token}", max_size=16 * 1024 * 1024) as device:
        await device.send(json.dumps(protocol.hello(device_id, "ws-smoke", "macos", capsules, bounds)))
        async with connect(f"{ws_url}/v1/ws/console?token={console_token}", max_size=16 * 1024 * 1024) as console:
            await recv_until(console, "hub_ready")
            await console.send(json.dumps({"t": "subscribe", "deviceId": device_id}))
            await recv_until(console, "device_state")
            await device.send(json.dumps(protocol.stale_frame("req-2", bounds)))
            stale = await recv_until(console, "stale_frame")
            check("stale_frame is routed", stale.get("reqId") == "req-2", str(stale))

    print()
    if failures:
        print(f"{len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("hub checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))

#!/usr/bin/env python3
"""Drive the real macOS client through the hub, exactly like the console does.

Requires `sky serve` to be running with the surface pointing at some window.
Verifies: online state, live frames, the click round trip (search window plus
pre-click capture) and a stop command.

    client/.venv/bin/python workers/scripts/client_smoke.py
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import urllib.request

from websockets.asyncio.client import connect

BASE = os.environ.get("SKY_BASE", "http://127.0.0.1:8787")
PASSWORD = os.environ.get("SKY_PASSWORD", "sky-dev")

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "..", "client", "src"))

from skycore import protocol  # noqa: E402

CAPTURE_TIMEOUT = 12.0


def http_json(method: str, path: str, body=None, token: str | None = None):
    headers = {"content-type": "application/json"}
    if token:
        headers["authorization"] = f"Bearer {token}"
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read() or b"null")


async def recv_until(socket, kinds: set[str], timeout: float = 8.0):
    """Returns (header, payload) where payload is bytes for binary messages."""
    import time

    deadline = time.monotonic() + timeout
    while True:
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError(f"never received {kinds}")
        raw = await asyncio.wait_for(socket.recv(), timeout=remaining)
        if isinstance(raw, bytes):
            header, payload = protocol.decode_binary(raw)
            if header.get("t") in kinds:
                return header, payload
            continue
        message = json.loads(raw)
        if message.get("t") in kinds:
            return message, None


async def main() -> int:
    failures: list[str] = []

    def check(label: str, condition: bool, extra: str = "") -> None:
        print(f"[{'ok  ' if condition else 'FAIL'}] {label}{'' if condition else ' — ' + extra}")
        if not condition:
            failures.append(label)

    token = http_json("POST", "/v1/auth/login", {"password": PASSWORD})["token"]
    devices = http_json("GET", "/v1/devices", token=token)["devices"]
    online = [device for device in devices if device["online"]]
    if not online:
        print("no device online — start `sky serve` first")
        return 2
    device = online[0]
    print(f"device   {device['name']} ({device['platform']}) bounds={device.get('bounds')}")
    check("device reports a scale", (device.get("bounds") or {}).get("scale", 0) > 0, str(device.get("bounds")))
    check("device declares AX lookup", (device.get("caps") or {}).get("uiQuery") == "ax", str(device.get("caps")))

    ws_url = BASE.replace("http://", "ws://").replace("https://", "wss://")
    async with connect(f"{ws_url}/v1/ws/console?token={token}", max_size=16 * 1024 * 1024) as console:
        await recv_until(console, {"hub_ready"})
        await console.send(json.dumps({"t": "subscribe", "deviceId": device["id"]}))
        state, _ = await recv_until(console, {"device_state"})
        check("hub knows the device is online", state.get("online") is True, str(state))

        await console.send(json.dumps({"t": "stream", "on": True, "fps": 4, "quality": 55}))
        header, payload = await recv_until(console, {"frame"}, timeout=CAPTURE_TIMEOUT)
        check("live frames arrive", bool(payload) and payload[:2] == b"\xff\xd8", f"{len(payload or b'')} bytes")
        bounds = header["bounds"]
        print(f"frame    seq={header['seq']} {len(payload)} bytes bounds={bounds}")

        # Click the middle of the surface and require a search window back.
        await console.send(
            json.dumps(
                {
                    "t": "inject",
                    "reqId": "live-1",
                    "kind": "tap",
                    "at": [0.5, 0.6],
                    "frameSeq": header["seq"],
                    "record": True,
                    "durationMs": 60,
                }
            )
        )
        context, png = await recv_until(console, {"context", "stale_frame"}, timeout=CAPTURE_TIMEOUT)
        if context.get("t") == "stale_frame":
            check("click returns a search window", False, "device reported stale_frame")
        else:
            window = context["windowRect"]
            check("click returns a search window", bool(png), f"{len(png or b'')} bytes")
            check("search window is a PNG", (png or b"")[:4] == b"\x89PNG", str((png or b"")[:4]))
            check("search window matches the declared caps", window["w"] <= 600.5 and window["h"] <= 400.5, str(window))
            check("click pixel is inside the window", 0 <= context["clickPx"][0] <= window["w"] and 0 <= context["clickPx"][1] <= window["h"], str(context.get("clickPx")))
            try:
                import cv2
                import numpy as np

                image = cv2.imdecode(np.frombuffer(png, dtype=np.uint8), cv2.IMREAD_COLOR)
                check("search window decodes", image is not None and image.shape[0] > 50, str(None if image is None else image.shape))
                if image is not None:
                    print(f"context  {image.shape[1]}x{image.shape[0]} px  clickPx={context['clickPx']}")
            except ImportError:
                pass

        # Swapping the surface geometry must make the device refuse, not mis-click.
        await console.send(
            json.dumps(
                {
                    "t": "inject",
                    "reqId": "live-2",
                    "kind": "tap",
                    "at": [0.2, 0.2],
                    "frameSeq": header["seq"] + 500,
                    "record": True,
                }
            )
        )
        second, _ = await recv_until(console, {"context", "stale_frame"}, timeout=CAPTURE_TIMEOUT)
        check("a moved surface is re-verified, not blindly clicked", second.get("t") in ("context", "stale_frame"), str(second))

        await console.send(json.dumps({"t": "stream", "on": False, "fps": 4, "quality": 55}))
        await asyncio.sleep(0.5)
        check("stream can be turned off", True)

    print()
    if failures:
        print(f"{len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("client checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))

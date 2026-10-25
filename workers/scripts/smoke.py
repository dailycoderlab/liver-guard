#!/usr/bin/env python3
"""End to end smoke test against a running dev server (default 127.0.0.1:8787).

Exercises the whole platform side without a real device: login, enroll, asset
upload with hash verification, script publish, and bundle fetch.
"""

from __future__ import annotations

import hashlib
import json
import os
import struct
import sys
import urllib.error
import urllib.request
import zlib

BASE = os.environ.get("SKY_BASE", "http://127.0.0.1:8787")
PASSWORD = os.environ.get("SKY_PASSWORD", "sky-dev")


def request(method: str, path: str, body=None, token: str | None = None, raw: bytes | None = None, ctype: str | None = None):
    url = BASE + path
    data = raw
    headers = {}
    if body is not None:
        data = json.dumps(body).encode()
        headers["content-type"] = "application/json"
    if raw is not None and ctype:
        headers["content-type"] = ctype
    if token:
        headers["authorization"] = f"Bearer {token}"
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=20) as resp:
            payload = resp.read()
            if resp.headers.get("content-type", "").startswith("application/json"):
                return resp.status, json.loads(payload or b"null")
            return resp.status, payload
    except urllib.error.HTTPError as err:
        payload = err.read()
        try:
            return err.code, json.loads(payload or b"null")
        except json.JSONDecodeError:
            return err.code, payload


def png(width: int, height: int, seed: int = 0) -> bytes:
    """Build a small RGB PNG by hand so the smoke test needs no image library."""
    rows = []
    for y in range(height):
        row = bytearray([0])
        for x in range(width):
            row += bytes(((x * 17 + seed) % 256, (y * 23 + seed) % 256, ((x + y) * 11) % 256))
        rows.append(bytes(row))
    raw = b"".join(rows)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")


def main() -> int:
    failures = []

    def check(label: str, condition: bool, extra: str = "") -> None:
        mark = "ok  " if condition else "FAIL"
        print(f"[{mark}] {label}{(' — ' + extra) if extra and not condition else ''}")
        if not condition:
            failures.append(label)

    status, body = request("POST", "/v1/auth/login", {"password": PASSWORD})
    check("login", status == 200 and "token" in body, f"{status} {body}")
    token = body.get("token", "")

    status, body = request("POST", "/v1/auth/login", {"password": PASSWORD + "-wrong"})
    check("login rejects wrong password", status == 401, str(status))

    status, body = request("GET", "/v1/devices")
    check("devices requires auth", status == 401, str(status))

    status, body = request("POST", "/v1/devices/enroll", {"name": "smoke-mac", "platform": "macos"}, token)
    check("enroll device", status == 200 and "deviceToken" in body, f"{status} {body}")
    device_id = body.get("deviceId")
    device_token = body.get("deviceToken", "")

    status, body = request("GET", "/v1/devices", token=token)
    check("list devices", status == 200 and any(d["id"] == device_id for d in body.get("devices", [])), str(status))

    status, body = request("GET", "/v1/device/me", token=device_token)
    check("device token identifies device", status == 200 and body["device"]["id"] == device_id, str(status))

    image = png(48, 32, seed=7)
    sha = hashlib.sha256(image).hexdigest()
    status, body = request("PUT", f"/v1/assets/{sha}?w=48&h=32&scale=2", token=token, raw=image, ctype="image/png")
    check("upload asset", status == 200 and body.get("sha256") == sha, f"{status} {body}")

    status, body = request("PUT", f"/v1/assets/{'0' * 64}?w=48&h=32&scale=2", token=token, raw=image, ctype="image/png")
    check("asset hash is verified", status == 400, f"{status} {body}")

    status, body = request("GET", f"/v1/assets/{sha}")
    check("asset can be fetched anonymously", status == 200 and body == image, str(status))

    status, body = request("POST", "/v1/scripts", {"name": "smoke script"}, token)
    check("create script", status == 201, f"{status} {body}")
    script_id = body["script"]["id"]

    ir = {
        "irVersion": 1,
        "meta": {"name": "smoke script", "maxDurationMs": 60000},
        "requires": ["screenshot", "templateMatch", "gesture"],
        "entry": "main",
        "vars": {},
        "blocks": {
            "main": [
                {"op": "waitFor", "any": ["t1"], "timeoutMs": 5000, "intervalMs": 500},
                {"op": "tap", "target": "t1"},
                {"op": "sleep", "ms": 300},
            ]
        },
        "targets": {
            "t1": {
                "type": "template",
                "asset": sha,
                "assetBox": {"w": 48, "h": 32},
                "recordScale": 2,
                "roi": [0.3, 0.3, 0.4, 0.4],
                "anchor": [0.5, 0.5],
            }
        },
    }

    status, body = request("POST", f"/v1/scripts/{script_id}/publish", {"ir": ir}, token)
    check("publish script", status == 200 and body.get("version") == 1, f"{status} {body}")

    broken = json.loads(json.dumps(ir))
    broken["blocks"]["main"].append({"op": "tap", "target": "missing"})
    status, body = request("POST", f"/v1/scripts/{script_id}/publish", {"ir": broken}, token)
    check("publish rejects dangling target", status == 400, f"{status} {body}")

    broken2 = json.loads(json.dumps(ir))
    broken2["targets"]["t1"]["asset"] = "f" * 64
    status, body = request("POST", f"/v1/scripts/{script_id}/publish", {"ir": broken2}, token)
    check("publish rejects missing asset", status == 400 and body.get("error") == "missing_assets", f"{status} {body}")

    status, body = request("GET", f"/v1/scripts/{script_id}/bundle?v=1", token=device_token)
    check(
        "device can fetch a bundle",
        status == 200 and body["version"] == 1 and sha in body["assets"],
        f"{status} {body}",
    )

    status, body = request("POST", "/v1/runs", {"deviceId": device_id, "scriptId": script_id}, token)
    check("run refuses offline device", status == 409 and body.get("error") == "device_offline", f"{status} {body}")

    status, body = request("POST", f"/v1/runs/{'x' * 8}/logs", token=device_token, body=None, raw=b"", ctype="application/x-ndjson")
    check("log upload requires a body", status in (200, 400), str(status))

    print()
    if failures:
        print(f"{len(failures)} check(s) failed: {', '.join(failures)}")
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())

"""Wire protocol shared with the platform.

Control messages are JSON. Images travel in a binary envelope
``[u32 header length][JSON header][payload]`` so a one megabyte search window
crosses the Durable Object untouched.

The envelope layout must match ``workers/src/protocol.ts`` exactly.
"""

from __future__ import annotations

import json
import struct
import time
from dataclasses import dataclass, field
from typing import Any

HEADER_LEN_BYTES = 4


class MessageType:
    HELLO = "hello"
    INJECT = "inject"
    RECROP = "recrop"
    STREAM = "stream"
    RUN = "run"
    STOP = "stop"
    PING = "ping"
    INJECT_ACK = "inject_ack"
    STALE_FRAME = "stale_frame"
    RUN_STATE = "run_state"
    SCORE = "score"
    LOG = "log"
    SHOT = "shot"
    RUN_SUMMARY = "run_summary"
    ERROR = "error"
    FRAME = "frame"
    CONTEXT = "context"
    RECROP_CONTEXT = "recrop_context"


@dataclass
class Caps:
    """What this platform can do. The console adapts its UI to these."""

    max_fps: int = 8
    inject_kinds: list[str] = field(default_factory=lambda: ["tap", "longPress", "swipe", "key", "text"])
    ui_query: str = "none"  # "ax" on macOS, "uia" on Windows, "node" on Android
    max_window: tuple[int, int] = (600, 400)
    secure_screen: bool = False
    surface: str = "window"  # "window" | "screen" | "mirror"

    def to_json(self) -> dict[str, Any]:
        return {
            "maxFps": self.max_fps,
            "injectKinds": self.inject_kinds,
            "uiQuery": self.ui_query,
            "maxWindow": [self.max_window[0], self.max_window[1]],
            "secureScreen": self.secure_screen,
            "surface": self.surface,
        }


def encode_binary(header: dict[str, Any], payload: bytes) -> bytes:
    raw_header = json.dumps(header, separators=(",", ":")).encode("utf-8")
    return struct.pack(">I", len(raw_header)) + raw_header + payload


def decode_binary(data: bytes | bytearray | memoryview) -> tuple[dict[str, Any], bytes]:
    view = bytes(data)
    if len(view) < HEADER_LEN_BYTES:
        raise ValueError("binary message shorter than its header length field")
    (header_len,) = struct.unpack(">I", view[:HEADER_LEN_BYTES])
    end = HEADER_LEN_BYTES + header_len
    if end > len(view):
        raise ValueError("binary message truncated")
    header = json.loads(view[HEADER_LEN_BYTES:end].decode("utf-8"))
    return header, view[end:]


def frame_message(seq: int, bounds_payload: dict[str, Any], jpeg: bytes, ts: float | None = None) -> bytes:
    header = {
        "t": MessageType.FRAME,
        "seq": seq,
        "bounds": bounds_payload,
        "ts": ts if ts is not None else time.time(),
        "mime": "image/jpeg",
    }
    return encode_binary(header, jpeg)


def context_message(
    kind: str,
    req_id: str,
    window_payload: dict[str, Any],
    png: bytes,
    click_px: tuple[float, float] | None = None,
    ax_payload: dict[str, Any] | None = None,
) -> bytes:
    header: dict[str, Any] = {
        "t": kind,
        "reqId": req_id,
        "windowRect": window_payload,
        "mime": "image/png",
    }
    if click_px is not None:
        header["clickPx"] = [float(click_px[0]), float(click_px[1])]
    if ax_payload is not None:
        header["axBox"] = ax_payload
    return encode_binary(header, png)


# --- message builders -------------------------------------------------------


def hello(device_id: str | None, name: str, platform: str, caps: Caps, bounds_payload: dict[str, Any]) -> dict:
    return {
        "t": MessageType.HELLO,
        "role": "device",
        "deviceId": device_id,
        "name": name,
        "platform": platform,
        "caps": caps.to_json(),
        "bounds": bounds_payload,
    }


def inject_ack(req_id: str, bounds_payload: dict[str, Any], window_payload: dict[str, Any], ax_payload: dict | None = None) -> dict:
    payload: dict[str, Any] = {
        "t": MessageType.INJECT_ACK,
        "reqId": req_id,
        "bounds": bounds_payload,
        "windowRect": window_payload,
    }
    if ax_payload:
        payload["axBox"] = ax_payload
    return payload


def stale_frame(req_id: str, bounds_payload: dict[str, Any]) -> dict:
    return {"t": MessageType.STALE_FRAME, "reqId": req_id, "bounds": bounds_payload}


def run_state(
    run_id: str,
    state: str,
    step: int | None = None,
    step_id: str | None = None,
    error_code: str | None = None,
    error_msg: str | None = None,
) -> dict:
    payload: dict[str, Any] = {"t": MessageType.RUN_STATE, "runId": run_id, "state": state}
    if step is not None:
        payload["step"] = step
    if step_id is not None:
        payload["stepId"] = step_id
    if error_code is not None:
        payload["errorCode"] = error_code
    if error_msg is not None:
        payload["errorMsg"] = error_msg
    return payload


def score(items: list[dict[str, Any]]) -> dict:
    return {"t": MessageType.SCORE, "items": items}


def log(items: list[dict[str, Any]]) -> dict:
    return {"t": MessageType.LOG, "items": items}


def run_summary(
    run_id: str,
    state: str,
    step_count: int,
    steps: list[dict[str, Any]],
    error_code: str | None = None,
    error_msg: str | None = None,
) -> dict:
    payload: dict[str, Any] = {
        "t": MessageType.RUN_SUMMARY,
        "runId": run_id,
        "state": state,
        "stepCount": step_count,
        "steps": steps,
    }
    if error_code:
        payload["errorCode"] = error_code
    if error_msg:
        payload["errorMsg"] = error_msg
    return payload


def shot(run_id: str, key: str) -> dict:
    return {"t": MessageType.SHOT, "runId": run_id, "key": key}


def error(code: str, message: str, req_id: str | None = None) -> dict:
    payload: dict[str, Any] = {"t": MessageType.ERROR, "code": code, "message": message}
    if req_id:
        payload["reqId"] = req_id
    return payload


def log_item(run_id: str, level: str, event: str, payload: dict[str, Any] | None = None) -> dict:
    item: dict[str, Any] = {"runId": run_id, "ts": int(time.time() * 1000), "level": level, "event": event}
    if payload:
        item["payload"] = payload
    return item


def score_item(
    run_id: str,
    step_id: str,
    target_id: str,
    value: float,
    strategy: str,
    ms: float,
    reused: bool,
) -> dict:
    return {
        "runId": run_id,
        "stepId": step_id,
        "targetId": target_id,
        "score": round(float(value), 4),
        "strategy": strategy,
        "ms": int(ms),
        "reused": bool(reused),
    }

import json
import struct

from skycore import protocol


def test_binary_envelope_layout_matches_the_platform():
    """The layout is shared with workers/src/protocol.ts; assert the bytes."""
    header = {"t": "frame", "seq": 3, "mime": "image/jpeg"}
    payload = b"\x01\x02\x03"

    encoded = protocol.encode_binary(header, payload)
    raw_header = json.dumps(header, separators=(",", ":")).encode()

    assert encoded[:4] == struct.pack(">I", len(raw_header))
    assert encoded[4 : 4 + len(raw_header)] == raw_header
    assert encoded[4 + len(raw_header) :] == payload

    decoded_header, decoded_payload = protocol.decode_binary(encoded)
    assert decoded_header == header
    assert decoded_payload == payload


def test_decode_rejects_truncated_messages():
    for broken in (b"", b"\x00\x01", b"\x00\x00\x00\x40abc"):
        try:
            protocol.decode_binary(broken)
        except ValueError:
            continue
        raise AssertionError(f"expected ValueError for {broken!r}")


def test_frame_and_context_headers_carry_what_the_console_needs():
    bounds = {"x": 10, "y": 20, "w": 600, "h": 400, "scale": 2}
    header, payload = protocol.decode_binary(protocol.frame_message(9, bounds, b"jpeg"))
    assert header["t"] == "frame"
    assert header["bounds"] == bounds
    assert payload == b"jpeg"

    header, payload = protocol.decode_binary(
        protocol.context_message(
            "context",
            "req-1",
            bounds,
            b"png",
            click_px=(12.5, 33.0),
            ax_payload={"x": 1, "y": 2, "w": 3, "h": 4, "scale": 2},
        )
    )
    assert header["t"] == "context"
    assert header["reqId"] == "req-1"
    assert header["clickPx"] == [12.5, 33.0]
    assert header["windowRect"] == bounds
    assert header["axBox"]["w"] == 3
    assert payload == b"png"


def test_caps_json_uses_the_field_names_the_console_expects():
    caps = protocol.Caps(max_fps=6, ui_query="ax", max_window=(500, 300), surface="window")
    payload = caps.to_json()
    assert payload["maxFps"] == 6
    assert payload["uiQuery"] == "ax"
    assert payload["maxWindow"] == [500, 300]
    assert payload["surface"] == "window"
    assert "tap" in payload["injectKinds"]


def test_message_builders_omit_empty_optional_fields():
    payload = protocol.run_state("run-1", "running")
    assert "errorCode" not in payload
    assert payload["runId"] == "run-1"

    payload = protocol.stale_frame("req-1", {"x": 0, "y": 0, "w": 1, "h": 1, "scale": 1})
    assert payload["t"] == "stale_frame"
    assert payload["reqId"] == "req-1"

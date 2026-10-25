"""Device client: one websocket to the hub, plus a worker thread per run.

Design notes:

* The transport loop owns asyncio; runs execute in a worker thread so a slow
  `waitFor` never blocks frame streaming or a stop request.
* Websocket ping frames keep the connection alive. No application level
  heartbeat: the platform bills per message, and protocol pings are free.
* Every recorded click captures the frame *before* injecting, so the template is
  cut from a clean, pre-interaction screenshot.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import platform as platform_module
import stat
import threading
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from skycore import protocol
from skycore.frame import Frame
from skycore.geometry import Bounds, normalized_to_global, search_window
from skycore.matcher import AssetStore, MatcherEngine
from skycore.report import Reporter
from skycore.runtime import CAPTURE_STALL_MS, RunContext, RunResult, execute
from skycore.schema import IRDoc

from . import permissions
from .driver import MacDriver

LOG = logging.getLogger("sky.client")

CONFIG_PATH = Path.home() / ".sky" / "device.json"
ASSET_DIR = Path.home() / ".sky" / "assets"
FRAME_BUFFER = 8
STREAM_MAX_WIDTH = 1280


@dataclass
class DeviceConfig:
    server: str
    device_id: str
    token: str
    name: str
    window: str | None = None

    @staticmethod
    def load(path: Path = CONFIG_PATH) -> "DeviceConfig":
        data = json.loads(path.read_text("utf-8"))
        return DeviceConfig(
            server=str(data["server"]).rstrip("/"),
            device_id=str(data["deviceId"]),
            token=str(data["token"]),
            name=str(data.get("name", "mac")),
            window=data.get("window"),
        )

    def save(self, path: Path = CONFIG_PATH) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(
                {
                    "server": self.server,
                    "deviceId": self.device_id,
                    "token": self.token,
                    "name": self.name,
                    "window": self.window,
                },
                indent=2,
            ),
            "utf-8",
        )
        try:
            path.chmod(stat.S_IRUSR | stat.S_IWUSR)
        except OSError:
            pass


class DeviceClient:
    def __init__(self, config: DeviceConfig) -> None:
        self.config = config
        self.driver = MacDriver(window_query=config.window, full_screen=config.window is None)
        self.assets = AssetStore(cache_dir=ASSET_DIR, fetcher=self._fetch_asset)
        self.engine = MatcherEngine(self.assets)
        self.caps = self.driver.caps()

        self.ws = None
        self.seq = 0
        self.stream_on = False
        self.stream_fps = 6
        self.stream_quality = 60
        self.last_bounds: Bounds | None = None

        self._frames: dict[str, tuple[Frame, tuple[float, float]]] = {}
        self._frame_order: list[str] = []
        self._reporter: Reporter | None = None
        self._run_id: str | None = None
        self._stop_event = threading.Event()
        self._run_task: asyncio.Task | None = None
        self._closing = asyncio.Event()

    # ------------------------------------------------------------- transport

    @property
    def ws_url(self) -> str:
        scheme = "wss" if self.config.server.startswith("https") else "ws"
        host = self.config.server.split("://", 1)[-1]
        return f"{scheme}://{host}/v1/ws/device?token={self.config.token}"

    async def send_json(self, payload: dict[str, Any]) -> None:
        if self.ws is not None:
            await self.ws.send(json.dumps(payload))

    async def send_binary(self, payload: bytes) -> None:
        if self.ws is not None:
            await self.ws.send(payload)

    async def run_forever(self) -> None:
        backoff = 1.0
        while not self._closing.is_set():
            try:
                await self._session()
                backoff = 1.0
            except asyncio.CancelledError:
                raise
            except Exception as error:  # noqa: BLE001 - reconnect on anything
                LOG.warning("disconnected: %s", error)
            if self._closing.is_set():
                break
            await asyncio.sleep(backoff)
            backoff = min(30.0, backoff * 2)

    async def _session(self) -> None:
        connect = _connect_factory()
        async with connect(self.ws_url, max_size=16 * 1024 * 1024, ping_interval=25, ping_timeout=20) as socket:
            self.ws = socket
            LOG.info("connected to %s", self.config.server)

            surface = await asyncio.to_thread(self.driver.refresh)
            self.last_bounds = await asyncio.to_thread(self.driver.bounds)
            await self.send_json(
                protocol.hello(
                    self.config.device_id,
                    self.config.name,
                    "macos",
                    self.caps,
                    self.last_bounds.to_json(),
                )
            )
            LOG.info("surface: %s %.0fx%.0f @%.2fx", surface.label, surface.rect.w, surface.rect.h, self.last_bounds.scale)

            tasks = [
                asyncio.create_task(self._receive_loop()),
                asyncio.create_task(self._stream_loop()),
                asyncio.create_task(self._report_loop()),
            ]
            done, pending = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
            for task in pending:
                task.cancel()
            for task in done:
                error = task.exception()
                if error:
                    raise error

    async def close(self) -> None:
        self._closing.set()
        self._stop_event.set()
        if self.ws is not None:
            await self.ws.close()

    # --------------------------------------------------------------- receive

    async def _receive_loop(self) -> None:
        assert self.ws is not None
        async for raw in self.ws:
            if isinstance(raw, bytes):
                continue
            try:
                message = json.loads(raw)
            except json.JSONDecodeError:
                continue
            kind = message.get("t")
            try:
                if kind == protocol.MessageType.INJECT:
                    await self.handle_inject(message)
                elif kind == protocol.MessageType.RECROP:
                    await self.handle_recrop(message)
                elif kind == protocol.MessageType.STREAM:
                    await self.handle_stream(message)
                elif kind == protocol.MessageType.RUN:
                    await self.handle_run(message)
                elif kind == protocol.MessageType.STOP:
                    await self.handle_stop(message)
            except Exception as error:  # noqa: BLE001 - one bad message must not kill the socket
                LOG.exception("message handling failed: %s", error)
                await self.send_json(protocol.error("handler_error", str(error), message.get("reqId")))

    # ------------------------------------------------------------ recording

    async def handle_inject(self, message: dict[str, Any]) -> None:
        req_id = str(message.get("reqId", ""))
        kind = str(message.get("kind", "tap"))

        current = await asyncio.to_thread(self.driver.bounds)
        if self.last_bounds is not None and not self.last_bounds.close_to(current, tolerance=1.5):
            await self.send_json(protocol.stale_frame(req_id, current.to_json()))
            return
        self.last_bounds = current

        nx, ny = message.get("at", [0.5, 0.5])
        click = normalized_to_global(float(nx), float(ny), current)

        if not message.get("record"):
            await self.send_json(protocol.inject_ack(req_id, current.to_json(), current.to_json()))
            await self._inject(kind, click, message)
            return

        # Capture before injecting: this is the whole point of the ordering.
        frame = await asyncio.to_thread(self.driver.capture)
        if frame.capture_ms > CAPTURE_STALL_MS:
            await self.send_json(
                protocol.error(
                    "capture_stalled",
                    f"one screenshot took {frame.capture_ms:.0f}ms; check the screen recording permission",
                    req_id,
                )
            )
            return
        self._remember_frame(req_id, frame, click)

        window = search_window(click, self.caps.max_window, current)
        crop = frame.crop_logical(window)
        png = Frame.encode_png(crop)

        scale = frame.scale
        click_px = ((click[0] - window.x) * scale, (click[1] - window.y) * scale)

        ax_box = await asyncio.to_thread(self.driver.ax_at, click[0], click[1])
        ax_payload = None
        if ax_box is not None and window.contains(*ax_box.center):
            ax_payload = {"x": ax_box.x, "y": ax_box.y, "w": ax_box.w, "h": ax_box.h, "scale": scale}

        window_bounds = Bounds(x=window.x, y=window.y, w=window.w, h=window.h, scale=scale)
        await self.send_binary(
            protocol.context_message("context", req_id, window_bounds.to_json(), png, click_px, ax_payload)
        )

        await self._inject(kind, click, message)

    async def handle_recrop(self, message: dict[str, Any]) -> None:
        req_id = str(message.get("reqId", ""))
        stored = self._frames.get(req_id)
        if stored is None:
            await self.send_json(protocol.error("frame_evicted", "the pre-click frame is no longer buffered", req_id))
            return

        frame, click = stored
        size = message.get("window") or [self.caps.max_window[0], self.caps.max_window[1]]
        window = search_window(click, (float(size[0]), float(size[1])), frame.bounds)

        crop = frame.crop_logical(window)
        png = Frame.encode_png(crop)
        scale = frame.scale
        click_px = ((click[0] - window.x) * scale, (click[1] - window.y) * scale)

        window_bounds = Bounds(x=window.x, y=window.y, w=window.w, h=window.h, scale=scale)
        await self.send_binary(
            protocol.context_message("recrop_context", req_id, window_bounds.to_json(), png, click_px)
        )

    async def _inject(self, kind: str, click: tuple[float, float], message: dict[str, Any]) -> None:
        duration = int(message.get("durationMs", 60))
        if kind in ("tap", "longPress"):
            await asyncio.to_thread(self.driver.tap, click[0], click[1], duration)
        elif kind == "swipe":
            to = message.get("to") or [0.5, 0.5]
            bounds = await asyncio.to_thread(self.driver.bounds)
            end = normalized_to_global(float(to[0]), float(to[1]), bounds)
            await asyncio.to_thread(self.driver.swipe, click[0], click[1], end[0], end[1], max(120, duration))
        elif kind == "key":
            await asyncio.to_thread(self.driver.key, str(message.get("key", "return")))
        elif kind == "text":
            await asyncio.to_thread(self.driver.text, str(message.get("text", "")))

    def _remember_frame(self, req_id: str, frame: Frame, click: tuple[float, float]) -> None:
        self._frames[req_id] = (frame, click)
        self._frame_order.append(req_id)
        while len(self._frame_order) > FRAME_BUFFER:
            evicted = self._frame_order.pop(0)
            self._frames.pop(evicted, None)

    # ------------------------------------------------------------- streaming

    async def handle_stream(self, message: dict[str, Any]) -> None:
        self.stream_on = bool(message.get("on"))
        self.stream_fps = max(1, min(int(message.get("fps", 6)), self.caps.max_fps))
        self.stream_quality = max(20, min(int(message.get("quality", 60)), 95))
        LOG.info("stream %s at %s fps", "on" if self.stream_on else "off", self.stream_fps)

    async def _stream_loop(self) -> None:
        warned = False
        while True:
            if not self.stream_on or self.ws is None:
                await asyncio.sleep(0.1)
                continue

            started = time.perf_counter()
            try:
                frame = await asyncio.to_thread(self.driver.capture)
                if frame.capture_ms > CAPTURE_STALL_MS:
                    # Blank frames arriving 30 seconds late are useless; stop the
                    # stream and say why instead of hammering the device.
                    if not warned:
                        warned = True
                        LOG.error(
                            "capture is stalling (%.0f ms per frame); turning streaming off. "
                            "Check the screen recording permission, or that no full-screen app covers the target.",
                            frame.capture_ms,
                        )
                    self.stream_on = False
                    continue
                jpeg = await asyncio.to_thread(frame.to_jpeg, self.stream_quality, STREAM_MAX_WIDTH)
                self.seq += 1
                self.last_bounds = frame.bounds
                await self.send_binary(protocol.frame_message(self.seq, frame.bounds.to_json(), jpeg))
            except Exception as error:  # noqa: BLE001 - keep the loop alive
                LOG.debug("frame failed: %s", error)

            elapsed = time.perf_counter() - started
            await asyncio.sleep(max(0.0, 1.0 / self.stream_fps - elapsed))

    # ------------------------------------------------------------------- runs

    async def handle_run(self, message: dict[str, Any]) -> None:
        if self._run_task is not None and not self._run_task.done():
            await self.send_json(protocol.error("busy", "a run is already in progress"))
            return

        run_id = str(message["runId"])
        script_id = str(message["scriptId"])
        version = int(message.get("version", 0))

        bundle = await asyncio.to_thread(self._http_json, f"/v1/scripts/{script_id}/bundle?v={version}")
        ir = IRDoc.model_validate(bundle["ir"])
        problems = ir.validate()
        if problems:
            await self.send_json(protocol.error("invalid_ir", "; ".join(problems)))
            return

        for sha in ir.asset_hashes():
            if not self.assets.has(sha):
                try:
                    await asyncio.to_thread(self.assets.get, sha)
                except Exception as error:  # noqa: BLE001
                    await self.send_json(protocol.error("missing_asset", f"{sha[:8]}: {error}"))
                    return

        self._run_id = run_id
        self._stop_event.clear()
        reporter = Reporter(run_id=run_id)
        self._reporter = reporter
        self.engine.tracker.clear()

        await self.send_json(protocol.run_state(run_id, "running", step=0))
        self._run_task = asyncio.create_task(self._run_in_thread(ir, reporter, run_id))

    async def _run_in_thread(self, ir: IRDoc, reporter: Reporter, run_id: str) -> None:
        def work() -> RunResult:
            ctx = RunContext(
                ir=ir,
                driver=self.driver,
                engine=self.engine,
                reporter=reporter,
                run_id=run_id,
                stop_event=self._stop_event,
            )
            return execute(ir, ctx)

        result = await asyncio.to_thread(work)

        # Drain anything still queued before the final summary.
        await asyncio.sleep(0.15)
        scores, logs = reporter.take(5000)
        if scores:
            await self.send_json(protocol.score(scores))
        if logs:
            await self.send_json(protocol.log(logs))

        await self._upload_logs(run_id, reporter)
        if result.state == "failed":
            await self._upload_failure_shot(run_id, result)

        await self.send_json(protocol.run_state(run_id, result.state, error_code=result.error_code, error_msg=result.error_msg))
        await self.send_json(
            protocol.run_summary(
                run_id,
                result.state,
                result.step_count,
                reporter.summary_rows(),
                result.error_code,
                result.error_msg,
            )
        )
        LOG.info("run %s %s (%s steps)", run_id, result.state, result.step_count)
        self._reporter = None
        self._run_id = None

    async def handle_stop(self, message: dict[str, Any]) -> None:
        LOG.info("stop requested")
        self._stop_event.set()

    async def _report_loop(self) -> None:
        while True:
            await asyncio.sleep(1.5)
            reporter = self._reporter
            if reporter is None or self.ws is None:
                continue
            scores, logs = reporter.take()
            if scores:
                await self.send_json(protocol.score(scores))
            if logs:
                await self.send_json(protocol.log(logs))

    async def _upload_logs(self, run_id: str, reporter: Reporter) -> None:
        body = reporter.jsonl().encode("utf-8")
        if not body:
            return
        try:
            await asyncio.to_thread(self._http_post, f"/v1/runs/{run_id}/logs", body, "application/x-ndjson")
            reporter.clear_jsonl()
        except Exception as error:  # noqa: BLE001
            LOG.warning("log upload failed: %s", error)

    async def _upload_failure_shot(self, run_id: str, result: RunResult) -> None:
        try:
            frame = await asyncio.to_thread(self.driver.capture)
            jpeg = await asyncio.to_thread(frame.to_jpeg, 70, 1280)
            key = f"{int(time.time() * 1000)}.jpg"
            await asyncio.to_thread(
                self._http_put, f"/v1/runs/{run_id}/shots/{key}", jpeg, "image/jpeg"
            )
            await self.send_json(protocol.shot(run_id, key))
        except Exception as error:  # noqa: BLE001
            LOG.debug("failure screenshot skipped: %s", error)

    # ------------------------------------------------------------------- http

    def _headers(self) -> dict[str, str]:
        return {"authorization": f"Bearer {self.config.token}"}

    def _http_json(self, path: str) -> dict[str, Any]:
        request = urllib.request.Request(self.config.server + path, headers=self._headers())
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.loads(response.read())

    def _http_post(self, path: str, body: bytes, content_type: str) -> None:
        request = urllib.request.Request(
            self.config.server + path,
            data=body,
            headers={**self._headers(), "content-type": content_type},
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            response.read()

    def _http_put(self, path: str, body: bytes, content_type: str) -> None:
        request = urllib.request.Request(
            self.config.server + path,
            data=body,
            headers={**self._headers(), "content-type": content_type},
            method="PUT",
        )
        with urllib.request.urlopen(request, timeout=60) as response:
            response.read()

    def _fetch_asset(self, sha: str) -> bytes:
        request = urllib.request.Request(self.config.server + f"/v1/assets/{sha}")
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read()


def _connect_factory():
    try:
        from websockets.asyncio.client import connect

        return connect
    except Exception:  # pragma: no cover - older websockets
        from websockets import connect

        return connect


def platform_name() -> str:
    return f"macOS {platform_module.mac_ver()[0]}"


def local_environment_report() -> dict[str, Any]:
    state = permissions.current_state()
    return {
        "platform": platform_name(),
        "python": platform_module.python_version(),
        "host": os.uname().nodename,
        "permissions": {"accessibility": state.accessibility, "screenRecording": state.screen_recording},
    }

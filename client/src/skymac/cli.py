"""Command line entry point: ``sky <command>``."""

from __future__ import annotations

import argparse
import asyncio
import getpass
import json
import logging
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

from skycore.matcher import AssetStore, MatcherEngine
from skycore.report import NullReporter
from skycore.runtime import RunContext, execute
from skycore.schema import IRDoc, TemplateTarget

from . import permissions
from . import window as window_tools
from .client import CONFIG_PATH, DeviceClient, DeviceConfig
from .driver import MacDriver

ASSET_DIR = Path.home() / ".sky" / "assets"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="sky", description="Sky device agent")
    parser.add_argument("--verbose", "-v", action="store_true", help="debug logging")
    sub = parser.add_subparsers(dest="command", required=True)

    doctor = sub.add_parser("doctor", help="check permissions, capture and coordinates")
    doctor.add_argument("--window", default=None, help="check a specific window instead of the whole display")

    enroll = sub.add_parser("enroll", help="register this device with the platform")
    enroll.add_argument("--server", required=True, help="platform base url, e.g. http://127.0.0.1:8787")
    enroll.add_argument("--name", default=None, help="device name shown in the console")
    enroll.add_argument("--window", default=None, help="substring of the window to control; omit for the whole screen")
    enroll.add_argument(
        "--force",
        action="store_true",
        help="register even though a permission is missing (pipeline testing only: clicks will not land)",
    )

    windows = sub.add_parser("windows", help="list controllable windows")
    windows.add_argument("--all", action="store_true", help="include off-screen and non-zero layers")

    capture = sub.add_parser("capture", help="save one screenshot (verifies the permission)")
    capture.add_argument("--out", default="capture.png")
    capture.add_argument("--window", default=None)

    serve = sub.add_parser("serve", help="connect to the platform and stay online")
    serve.add_argument("--server", default=None, help="override the enrolled server")

    run = sub.add_parser("run", help="run a script locally")
    run.add_argument("--script", required=True, help="script id on the platform")
    run.add_argument("--version", type=int, default=None)
    run.add_argument("--window", default=None)
    run.add_argument("--max-seconds", type=int, default=None, help="safety limit for a local run")

    probe = sub.add_parser("probe", help="dry run: locate every target, click nothing")
    probe.add_argument("--script", default=None, help="script id on the platform")
    probe.add_argument("--file", default=None, help="local IR json")
    probe.add_argument("--assets", default=None, help="local asset directory (default ~/.sky/assets)")
    probe.add_argument("--window", default=None)

    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    logging.basicConfig(
        level=logging.DEBUG if args.verbose else logging.INFO,
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    try:
        if args.command == "doctor":
            return cmd_doctor(args)
        if args.command == "enroll":
            return cmd_enroll(args)
        if args.command == "windows":
            return cmd_windows(args)
        if args.command == "capture":
            return cmd_capture(args)
        if args.command == "serve":
            return cmd_serve(args)
        if args.command == "run":
            return cmd_run(args)
        if args.command == "probe":
            return cmd_probe(args)
    except KeyboardInterrupt:
        print("\ninterrupted", file=sys.stderr)
        return 130
    except Exception as error:  # noqa: BLE001
        print(f"error: {error}", file=sys.stderr)
        if args.verbose:
            raise
        return 1

    return 2


# ------------------------------------------------------------------- doctor


def cmd_doctor(args) -> int:
    state = permissions.current_state()
    print("Sky macOS agent — doctor")
    print()

    print("权限")
    print(f"  辅助功能    {'ok' if state.accessibility else 'MISSING'}")
    print(f"  屏幕录制    {'ok' if state.screen_recording else 'MISSING'}")
    for problem in state.problems():
        print(f"  ! {problem}")
    print()

    driver = MacDriver(window_query=args.window, full_screen=args.window is None)
    surface = driver.surface()
    bounds = driver.bounds()
    print("捕获面")
    print(f"  {surface.label}")
    print(f"  逻辑尺寸 {bounds.w:.0f}x{bounds.h:.0f} pt @ ({bounds.x:.0f}, {bounds.y:.0f})")
    print(f"  scale {bounds.scale:.3f}x")
    print()

    print("截图")
    try:
        started = time.perf_counter()
        frame = driver.capture()
        elapsed = (time.perf_counter() - started) * 1000
        std = frame.content_std()
        print(f"  像素尺寸 {frame.width}x{frame.height}")
        print(f"  耗时 {elapsed:.0f} ms" + ("  <-- 超过 1 秒，通常是权限或全屏应用在挡" if elapsed > 1000 else ""))
        print(f"  内容标准差 {std:.2f}" + ("" if std > 3 else "  <-- 全黑或空白，多半是屏幕录制权限没生效"))
    except Exception as error:  # noqa: BLE001
        print(f"  失败: {error}")
    print()

    print("坐标")
    center = (bounds.x + bounds.w / 2, bounds.y + bounds.h / 2)
    element = driver.ax_at(*center)
    if element is None:
        print("  AX 查询无结果（游戏自绘 UI 正常会是这样，录制台会退回轮廓检测）")
    else:
        print(f"  屏幕中心元素 {element.w:.0f}x{element.h:.0f} pt @ ({element.x:.0f}, {element.y:.0f})")
    print()

    print("遮挡")
    surface = driver.surface()
    if surface.window_number is None:
        print("  正在捕获整块屏幕，不存在遮挡问题")
    else:
        target = window_tools.window_by_number(surface.window_number)
        covering = window_tools.occluding_windows(target) if target else []
        if not covering:
            print("  目标窗口在最前面，捕获到的就是它本身")
        else:
            print("  ! 目标窗口被下面的窗口盖住了，截图里会是它们而不是游戏：")
            for window in covering[:5]:
                print(f"      {window}")
            print("      录制前把游戏切到前台，或改用整屏捕获（去掉 --window）")
    print()

    print("配置")
    if CONFIG_PATH.exists():
        try:
            config = DeviceConfig.load()
            print(f"  已注册为 {config.name} ({config.device_id[:8]}…) → {config.server}")
        except Exception as error:  # noqa: BLE001
            print(f"  配置文件损坏: {error}")
    else:
        print("  未注册。运行 `sky enroll --server <url>`")

    return 0 if state.ok else 3


# ------------------------------------------------------------------- enroll


def cmd_enroll(args) -> int:
    state = permissions.current_state()
    if not state.ok:
        for problem in state.problems():
            print(f"! {problem}", file=sys.stderr)
        if not args.force:
            print("\n权限补齐后重新打开终端再试（或用 --force 只验证链路）。", file=sys.stderr)
            return 3
        print("! --force：继续注册，但注入的点击不会真正生效。\n", file=sys.stderr)

    server = args.server.rstrip("/")
    # SKY_PASSWORD lets CI and scripts enroll without a tty; getpass is only for
    # interactive use.
    password = os.environ.get("SKY_PASSWORD") or getpass.getpass("控制台口令: ")

    try:
        token = _post_json(server + "/v1/auth/login", {"password": password})["token"]
    except urllib.error.HTTPError as error:
        print(f"登录失败: HTTP {error.code}", file=sys.stderr)
        return 4

    driver = MacDriver(window_query=args.window, full_screen=args.window is None)
    bounds = driver.bounds()
    caps = driver.caps()

    name = args.name or f"mac-{time.strftime('%H%M')}"
    payload = {
        "name": name,
        "platform": "macos",
        "caps": caps.to_json(),
        "screen": {"w": bounds.w, "h": bounds.h, "scale": bounds.scale},
    }
    try:
        result = _post_json(server + "/v1/devices/enroll", payload, token)
    except urllib.error.HTTPError as error:
        print(f"注册失败: HTTP {error.code} {error.read()[:200]!r}", file=sys.stderr)
        return 4

    config = DeviceConfig(
        server=server,
        device_id=result["deviceId"],
        token=result["deviceToken"],
        name=name,
        window=args.window,
    )
    config.save()
    print(f"已注册设备 {name} ({config.device_id})")
    print(f"配置写入 {CONFIG_PATH}")
    print("下一步: `sky serve`")
    return 0


# ------------------------------------------------------------------ windows


def cmd_windows(args) -> int:
    windows = window_tools.list_windows(on_screen_only=not args.all, include_all_layers=args.all)
    if not windows:
        print("没有可见窗口（也可能是没给辅助功能权限）")
        return 1
    for window in windows:
        print(window)
    return 0


def cmd_capture(args) -> int:
    driver = MacDriver(window_query=args.window, full_screen=args.window is None)
    driver.activate()
    frame = driver.capture()
    Path(args.out).write_bytes(frame.to_png())
    print(f"{args.out}: {frame.width}x{frame.height} px, bounds {frame.bounds.w:.0f}x{frame.bounds.h:.0f} pt, std {frame.content_std():.2f}")
    return 0


# -------------------------------------------------------------------- serve


def cmd_serve(args) -> int:
    if not CONFIG_PATH.exists():
        print("还没有注册设备，先运行 `sky enroll --server <url>`", file=sys.stderr)
        return 2
    config = DeviceConfig.load()
    if args.server:
        config.server = args.server.rstrip("/")

    state = permissions.current_state()
    if not state.ok:
        for problem in state.problems():
            print(f"! {problem}", file=sys.stderr)

    client = DeviceClient(config)

    async def runner() -> None:
        await client.run_forever()

    try:
        asyncio.run(runner())
    except KeyboardInterrupt:
        pass
    finally:
        asyncio.run(client.close())
    return 0


# ---------------------------------------------------------------- run/probe


def _load_ir(args) -> tuple[IRDoc, str | None]:
    """Returns the IR and the server base url used to fetch assets."""
    if args.file:
        ir = IRDoc.model_validate(json.loads(Path(args.file).read_text("utf-8")))
        return ir, None

    if not args.script:
        raise SystemExit("either --script or --file is required")
    config = DeviceConfig.load()
    bundle = _get_json(config.server + f"/v1/scripts/{args.script}/bundle" + (f"?v={args.version}" if getattr(args, "version", None) else ""), config.token)
    return IRDoc.model_validate(bundle["ir"]), config.server


def _make_assets(server: str | None, token: str | None, directory: str | None) -> AssetStore:
    cache = Path(directory) if directory else ASSET_DIR

    def fetch(sha: str) -> bytes:
        if server is None:
            raise KeyError(sha)
        headers = {"authorization": f"Bearer {token}"} if token else {}
        request = urllib.request.Request(f"{server}/v1/assets/{sha}", headers=headers)
        with urllib.request.urlopen(request, timeout=30) as response:
            return response.read()

    return AssetStore(cache_dir=cache, fetcher=fetch if server else None)


def cmd_probe(args) -> int:
    ir, server = _load_ir(args)
    problems = ir.validate()
    if problems:
        for problem in problems:
            print(f"! {problem}", file=sys.stderr)
        return 4

    token = None
    if server:
        token = DeviceConfig.load().token
    assets = _make_assets(server, token, args.assets)

    driver = MacDriver(window_query=args.window, full_screen=args.window is None)
    driver.activate()
    frame = driver.capture()
    engine = MatcherEngine(assets, allow_reuse=False)

    print(f"probe {ir.meta.name} — {frame.width}x{frame.height} px @ scale {frame.scale:.2f}")
    print(f"{'target':<20}{'score':>8}{'ms':>8}  {'strategy':<16}{'box':<18}warnings")
    failures = 0

    for name, target in ir.targets.items():
        if not isinstance(target, TemplateTarget):
            print(f"{name:<20}{'-':>8}{'-':>8}  {'point':<16}{'':<18}")
            continue
        started = time.perf_counter()
        hit = engine.resolve(target, name, frame)
        elapsed = (time.perf_counter() - started) * 1000

        if hit is None:
            failures += 1
            print(f"{name:<20}{'MISS':>8}{elapsed:7.0f}  {'-':<16}{'-':<18}")
            continue

        box = f"{hit.box.w:.0f}x{hit.box.h:.0f}"
        flag = "" if hit.score >= target.threshold else "  <-- below threshold"
        if hit.score < target.warnThreshold:
            failures += 1
        print(
            f"{name:<20}{hit.score:8.3f}{hit.elapsed_ms:8.0f}  {hit.strategy:<16}{box:<18}"
            + (",".join(hit.warnings) if hit.warnings else "")
            + flag
        )

    print()
    if failures:
        print(f"{failures} target(s) missed or below the warn threshold")
        return 5
    print("all targets located")
    return 0


def cmd_run(args) -> int:
    ir, server = _load_ir(args)
    config = DeviceConfig.load() if (server or CONFIG_PATH.exists()) else None
    assets = _make_assets(server, config.token if config else None, None)

    for sha in ir.asset_hashes():
        try:
            assets.get(sha)
        except Exception as error:  # noqa: BLE001
            print(f"missing asset {sha[:8]}: {error}", file=sys.stderr)
            return 4

    driver = MacDriver(window_query=args.window, full_screen=args.window is None)
    driver.activate()

    if args.max_seconds:
        ir.meta.maxDurationMs = min(ir.meta.maxDurationMs, args.max_seconds * 1000)

    reporter = NullReporter()
    ctx = RunContext(ir=ir, driver=driver, engine=MatcherEngine(assets), reporter=reporter, run_id="local")
    result = execute(ir, ctx)

    print()
    print(f"state      {result.state}")
    print(f"steps      {result.step_count}")
    if result.error_code:
        print(f"error      {result.error_code}: {result.error_msg}")
    rows = reporter.summary_rows()
    if rows:
        print()
        print(f"{'step':<12}{'target':<20}{'n':>4}{'avg':>8}{'min':>8}{'max ms':>8}")
        for row in rows:
            print(f"{row['stepId']:<12}{row['targetId']:<20}{row['n']:>4}{row['avg']:>8.3f}{row['min']:>8.3f}{row['maxMs']:>8}")
    return 0 if result.state == "done" else 6


# ---------------------------------------------------------------- http utils


def _post_json(url: str, payload: dict, token: str | None = None) -> dict:
    headers = {"content-type": "application/json"}
    if token:
        headers["authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=json.dumps(payload).encode(), headers=headers, method="POST")
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read())


def _get_json(url: str, token: str | None = None) -> dict:
    headers = {"authorization": f"Bearer {token}"} if token else {}
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=30) as response:
        return json.loads(response.read())


if __name__ == "__main__":
    sys.exit(main())

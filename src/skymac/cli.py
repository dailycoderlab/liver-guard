"""Command line entry point: ``sky <command>``."""

from __future__ import annotations

import argparse
import json
import logging
import sys
import time
from pathlib import Path

from skycore.local_store import LocalStore
from skycore.matcher import AssetStore, MatcherEngine
from skycore.report import NullReporter
from skycore.runtime import RunContext, execute
from skycore.schema import IRDoc, TemplateTarget

from . import permissions
from . import window as window_tools
from .driver import MacDriver

ASSET_DIR = Path.home() / ".sky" / "assets"
SCRIPT_DIR = Path.home() / ".sky" / "scripts"


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="sky", description="Sky local script runner")
    parser.add_argument("--verbose", "-v", action="store_true", help="debug logging")
    sub = parser.add_subparsers(dest="command", required=True)

    doctor = sub.add_parser("doctor", help="check permissions, capture and coordinates")
    doctor.add_argument("--window", default=None, help="check a specific window instead of the whole display")

    windows = sub.add_parser("windows", help="list controllable windows")
    windows.add_argument("--all", action="store_true", help="include off-screen and non-zero layers")

    capture = sub.add_parser("capture", help="save one screenshot (verifies the permission)")
    capture.add_argument("--out", default="capture.png")
    capture.add_argument("--window", default=None)

    for name, help_text in (
        ("run", "run a script locally"),
        ("probe", "dry run: locate every target, click nothing"),
    ):
        command = sub.add_parser(name, help=help_text)
        source = command.add_mutually_exclusive_group(required=True)
        source.add_argument("--script", help="local script id in ~/.sky/scripts")
        source.add_argument("--file", help="local IR JSON file")
        command.add_argument("--assets", default=None, help="local asset directory (default ~/.sky/assets)")
        command.add_argument("--window", default=None)
        if name == "run":
            command.add_argument("--max-seconds", type=int, default=None, help="safety limit for a local run")

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
        if args.command == "windows":
            return cmd_windows(args)
        if args.command == "capture":
            return cmd_capture(args)
        if args.command == "run":
            return cmd_run(args)
        if args.command == "probe":
            return cmd_probe(args)
    except KeyboardInterrupt:
        print("\ninterrupted", file=sys.stderr)
        return 130
    except Exception as error:
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
    host = permissions.responsible_application()
    if host is None:
        print("  归属应用    未知（进程树里没有找到 app bundle）")
    else:
        name, path = host
        print(f"  归属应用    {name}（{path}）")
        if not path.endswith("Sky.app"):
            print("              开发态运行：Sky 没有独立的应用条目，权限记在上面这个宿主应用上；")
            print("              若要在系统设置里单独看到 Sky，请运行打包后的 Sky.app。")
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

    return 0 if state.ok else 3




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




# ---------------------------------------------------------------- run/probe


def _load_ir(args) -> IRDoc:
    """Read an IR file or the current content of a local script."""
    if args.file:
        data = json.loads(Path(args.file).read_text(encoding="utf-8"))
    else:
        script_id = args.script
        if not script_id or script_id in (".", "..") or "/" in script_id or "\\" in script_id:
            raise ValueError("invalid local script id")
        path = SCRIPT_DIR / f"{script_id}.json"
        script = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(script, dict) or script.get("id") != script_id:
            raise ValueError(f"invalid local script record: {path}")
        # 脚本只有一份内容；同时兼容旧文件里的草稿/版本记录。
        # A script has a single content field; older drafts/versions still load.
        data = LocalStore(SCRIPT_DIR.parent).script_ir(script)
    return IRDoc.model_validate(data)


def _make_assets(directory: str | None) -> AssetStore:
    return AssetStore(cache_dir=Path(directory) if directory else ASSET_DIR)


def cmd_probe(args) -> int:
    ir = _load_ir(args)
    problems = ir.validate()
    if problems:
        for problem in problems:
            print(f"! {problem}", file=sys.stderr)
        return 4

    assets = _make_assets(args.assets)

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
    ir = _load_ir(args)
    assets = _make_assets(args.assets)

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






if __name__ == "__main__":
    sys.exit(main())

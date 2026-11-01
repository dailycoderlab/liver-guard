import json
from types import SimpleNamespace
from typing import ClassVar

import pytest

from skymac import cli


def ir(name="test", targets=None):
    return {
        "irVersion": 1,
        "meta": {"name": name, "maxDurationMs": 20000},
        "entry": "main",
        "blocks": {"main": []},
        "targets": targets or {},
    }


def write_json(path, data):
    path.write_text(json.dumps(data), encoding="utf-8")


class FakeDriver:
    instances: ClassVar[list["FakeDriver"]] = []

    def __init__(self, window_query, full_screen):
        self.window_query = window_query
        self.full_screen = full_screen
        self.activated = False
        self.__class__.instances.append(self)

    def activate(self):
        self.activated = True

    def capture(self):
        return SimpleNamespace(width=800, height=600, scale=1.0)


@pytest.fixture(autouse=True)
def local_environment(monkeypatch, tmp_path):
    monkeypatch.setattr(cli, "SCRIPT_DIR", tmp_path / "scripts")
    monkeypatch.setattr(cli, "ASSET_DIR", tmp_path / "assets")
    monkeypatch.setattr(cli, "MacDriver", FakeDriver)
    cli.SCRIPT_DIR.mkdir()
    FakeDriver.instances.clear()


def test_parser_exposes_only_local_commands():
    parser = cli.build_parser()
    for name in ("doctor", "windows", "capture"):
        assert parser.parse_args([name]).command == name
    for name in ("enroll", "serve"):
        with pytest.raises(SystemExit):
            parser.parse_args([name])
    for name in ("run", "probe"):
        with pytest.raises(SystemExit):
            parser.parse_args([name])
        with pytest.raises(SystemExit):
            parser.parse_args([name, "--script", "abc", "--file", "x.json"])


@pytest.mark.parametrize("command", ["run", "probe"])
def test_both_commands_accept_local_ir_file(command, tmp_path, monkeypatch, capsys):
    path = tmp_path / "ir.json"
    write_json(path, ir("from file"))
    seen = []

    def execute(doc, ctx):
        seen.append(doc.meta.name)
        return SimpleNamespace(state="done", step_count=0, error_code=None)

    monkeypatch.setattr(cli, "execute", execute)
    assert cli.main([command, "--file", str(path), "--window", "Editor"]) == 0
    assert FakeDriver.instances[-1].activated
    assert FakeDriver.instances[-1].window_query == "Editor"
    if command == "run":
        assert seen == ["from file"]
    else:
        assert "probe from file" in capsys.readouterr().out


@pytest.mark.parametrize("command", ["run", "probe"])
def test_local_script_uses_its_current_content(command, monkeypatch, capsys):
    write_json(cli.SCRIPT_DIR / "s1.json", {
        "id": "s1", "name": "Script", "ir": ir("current content"),
    })
    seen = []
    def execute(doc, ctx):
        seen.append(doc.meta.name)
        return SimpleNamespace(state="done", step_count=0, error_code=None)
    monkeypatch.setattr(cli, "execute", execute)
    assert cli.main([command, "--script", "s1"]) == 0
    if command == "run":
        assert seen == ["current content"]
    else:
        assert "probe current content" in capsys.readouterr().out


@pytest.mark.parametrize("command", ["run", "probe"])
def test_legacy_script_records_still_load(command, monkeypatch, capsys):
    # 旧文件里内容躺在 draft 或 versions 里，取消发布状态后必须仍能读到。
    # Old records kept content in draft or versions; they must still be readable.
    write_json(cli.SCRIPT_DIR / "s1.json", {
        "id": "s1", "name": "Script", "currentVersion": 2,
        "draft": ir("legacy draft"),
        "versions": {"1": ir("published one"), "2": ir("published two")},
    })
    seen = []
    def execute(doc, ctx):
        seen.append(doc.meta.name)
        return SimpleNamespace(state="done", step_count=0, error_code=None)
    monkeypatch.setattr(cli, "execute", execute)
    assert cli.main([command, "--script", "s1"]) == 0
    if command == "run":
        assert seen == ["legacy draft"]
    else:
        assert "probe legacy draft" in capsys.readouterr().out


@pytest.mark.parametrize("command", ["run", "probe"])
def test_script_without_content_reports_an_error(command, capsys):
    path = cli.SCRIPT_DIR / "s1.json"
    write_json(path, {"id": "s1", "name": "empty"})
    assert cli.main([command, "--script", "s1"]) == 1
    assert "脚本内容为空" in capsys.readouterr().err
    assert not FakeDriver.instances


def test_invalid_sources_fail_cleanly(tmp_path, capsys):
    assert cli.main(["run", "--script", "../outside"]) == 1
    assert "invalid local script id" in capsys.readouterr().err
    assert cli.main(["probe", "--script", "absent"]) == 1
    assert "absent.json" in capsys.readouterr().err
    # 版本概念已移除，参数不应再被接受。
    # The version concept is gone, so the flag must not be accepted.
    with pytest.raises(SystemExit):
        cli.build_parser().parse_args(["run", "--script", "s1", "--version", "1"])


def test_probe_fails_when_a_match_is_below_threshold(tmp_path, capsys, monkeypatch):
    """probe 必须和运行时判定一致：低于 threshold 就是跑不通。"""
    sha = "a" * 64
    (tmp_path / "assets").mkdir(exist_ok=True)
    import cv2
    import numpy as np
    patch = np.zeros((20, 20, 3), dtype=np.uint8)
    patch[:, :] = (10, 120, 200)
    ok, buffer = cv2.imencode(".png", patch)
    assert ok
    (tmp_path / "assets" / f"{sha}.png").write_bytes(buffer.tobytes())

    path = tmp_path / "ir.json"
    write_json(path, ir(targets={"button": {
        "type": "template", "asset": sha, "assetBox": {"w": 20, "h": 20},
        "roi": [0, 0, 1, 1], "threshold": 0.82, "warnThreshold": 0.7,
    }}))

    from skycore.geometry import Rect
    from skycore.matcher.engine import Hit

    class LowScoreEngine:
        def __init__(self, *args, **kwargs):
            pass

        def resolve(self, target, name, frame):
            return Hit(box=Rect(10, 10, 20, 20), point=(20.0, 20.0),
                       score=0.55, strategy="tpl", elapsed_ms=1.0)

    monkeypatch.setattr(cli, "MatcherEngine", LowScoreEngine)
    assert cli.main(["probe", "--file", str(path), "--assets", str(tmp_path / "assets")]) == 5
    out = capsys.readouterr().out
    assert "below threshold" in out
    # 说明这一行不会被点击，避免误读成"找到了只是分数低"。
    assert "will not click" in out


def test_run_missing_asset_never_starts_driver(tmp_path, capsys):
    path = tmp_path / "ir.json"
    write_json(path, ir(targets={"button": {
        "type": "template", "asset": "a" * 64, "assetBox": {"w": 20, "h": 20},
        "roi": [0, 0, 1, 1],
    }}))
    assert cli.main(["run", "--file", str(path), "--assets", str(tmp_path / "custom")]) == 4
    assert "missing asset" in capsys.readouterr().err
    assert not FakeDriver.instances


def test_local_assets_only_read_the_given_directory(tmp_path):
    store = cli._make_assets(str(tmp_path / "custom"))
    assert store.cache_dir == tmp_path / "custom"
    with pytest.raises(KeyError):
        store.get("a" * 64)

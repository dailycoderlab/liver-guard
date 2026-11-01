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
        seen.append((doc.meta.name, ctx.engine.assets.fetcher))
        return SimpleNamespace(state="done", step_count=0, error_code=None)

    monkeypatch.setattr(cli, "execute", execute)
    assert cli.main([command, "--file", str(path), "--window", "Editor"]) == 0
    assert FakeDriver.instances[-1].activated
    assert FakeDriver.instances[-1].window_query == "Editor"
    if command == "run":
        assert seen == [("from file", None)]
    else:
        assert "probe from file" in capsys.readouterr().out


@pytest.mark.parametrize("command", ["run", "probe"])
@pytest.mark.parametrize("version,expected", [(None, "published two"), ("1", "published one")])
def test_local_script_uses_published_version(command, version, expected, monkeypatch, capsys):
    write_json(cli.SCRIPT_DIR / "s1.json", {
        "id": "s1", "name": "Script", "currentVersion": 2,
        "draft": ir("unpublished draft"),
        "versions": {"1": ir("published one"), "2": ir("published two")},
    })
    seen = []
    def execute(doc, ctx):
        seen.append(doc.meta.name)
        return SimpleNamespace(state="done", step_count=0, error_code=None)
    monkeypatch.setattr(cli, "execute", execute)
    args = [command, "--script", "s1"]
    if version:
        args += ["--version", version]
    assert cli.main(args) == 0
    if command == "run":
        assert seen == [expected]
    else:
        assert f"probe {expected}" in capsys.readouterr().out


@pytest.mark.parametrize("command", ["run", "probe"])
def test_unpublished_and_missing_versions_are_errors(command, capsys):
    path = cli.SCRIPT_DIR / "s1.json"
    write_json(path, {"id": "s1", "currentVersion": 0, "draft": ir("draft"), "versions": {}})
    assert cli.main([command, "--script", "s1"]) == 1
    assert "published version" in capsys.readouterr().err
    write_json(path, {"id": "s1", "currentVersion": 1, "versions": {"1": ir()}})
    assert cli.main([command, "--script", "s1", "--version", "9"]) == 1
    assert "published version" in capsys.readouterr().err
    assert not FakeDriver.instances


def test_invalid_sources_fail_cleanly(tmp_path, capsys):
    assert cli.main(["run", "--script", "../outside"]) == 1
    assert "invalid local script id" in capsys.readouterr().err
    assert cli.main(["probe", "--script", "absent"]) == 1
    assert "absent.json" in capsys.readouterr().err
    path = tmp_path / "ir.json"
    write_json(path, ir())
    assert cli.main(["run", "--file", str(path), "--version", "1"]) == 1
    assert "--version requires --script" in capsys.readouterr().err


def test_run_missing_asset_never_fetches_or_starts_driver(tmp_path, capsys):
    path = tmp_path / "ir.json"
    write_json(path, ir(targets={"button": {
        "type": "template", "asset": "a" * 64, "assetBox": {"w": 20, "h": 20},
        "roi": [0, 0, 1, 1],
    }}))
    assert cli.main(["run", "--file", str(path), "--assets", str(tmp_path / "custom")]) == 4
    assert "missing asset" in capsys.readouterr().err
    assert not FakeDriver.instances


def test_local_assets_have_no_network_fetcher(tmp_path):
    store = cli._make_assets(str(tmp_path / "custom"))
    assert store.cache_dir == tmp_path / "custom"
    assert store.fetcher is None
    with pytest.raises(KeyError):
        store.get("a" * 64)

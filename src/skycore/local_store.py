"""Local, atomically written scripts, template assets and run history."""
from __future__ import annotations

import hashlib
import json
import os
import tempfile
import threading
import uuid
from pathlib import Path


class LocalStore:
    def __init__(self, root: Path):
        self.root = Path(root)
        self._lock = threading.RLock()

    @staticmethod
    def _id(value: str) -> str:
        try:
            parsed = uuid.UUID(value)
        except (ValueError, TypeError, AttributeError) as error:
            raise ValueError('无效的本地标识') from error
        if parsed.hex != value:
            raise ValueError('无效的本地标识')
        return value

    @staticmethod
    def _write(path: Path, data: bytes) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        name = None
        try:
            with tempfile.NamedTemporaryFile(dir=path.parent, prefix='.sky-', delete=False) as file:
                name = file.name
                file.write(data)
                file.flush()
                os.fsync(file.fileno())
            os.replace(name, path)
        finally:
            if name and os.path.exists(name):
                os.unlink(name)

    def _write_json(self, path: Path, value: dict) -> None:
        self._write(path, json.dumps(value, ensure_ascii=False, indent=2).encode('utf-8'))

    def _script_path(self, script_id: str) -> Path:
        return self.root / 'scripts' / (self._id(script_id) + '.json')

    def list_scripts(self) -> list[dict]:
        with self._lock:
            directory = self.root / 'scripts'
            if not directory.exists():
                return []
            scripts = [json.loads(path.read_text('utf-8')) for path in directory.glob('*.json')]
            return sorted(scripts, key=lambda script: (script['name'].casefold(), script['id']))

    def get_script(self, script_id: str) -> dict:
        with self._lock:
            return json.loads(self._script_path(script_id).read_text('utf-8'))

    def create_script(self, name: str, ir: dict) -> dict:
        name = name.strip()
        if not name:
            raise ValueError('脚本名称不能为空')
        # 只有一个内容字段：编辑、保存就是改它，没有草稿/已发布之分。
        # A single content field: saving edits it, with no draft/published split.
        script = {'id': uuid.uuid4().hex, 'name': name, 'ir': ir}
        with self._lock:
            self._write_json(self._script_path(script['id']), script)
        return script

    def save_script(self, script_id: str, ir: dict) -> None:
        with self._lock:
            script = self.get_script(script_id)
            script['ir'] = ir
            self._write_json(self._script_path(script_id), script)

    def script_ir(self, script: dict) -> dict:
        """读取脚本内容，兼容只有旧版草稿/版本记录的脚本文件。

        旧文件里发布版本才是"能跑的内容"，但草稿是用户最后一次编辑的结果，
        取消发布状态后它就是唯一内容，因此优先取草稿。
        Old records kept a draft plus published versions; the draft is what the
        user last edited, so it becomes the single content after this change.
        """
        if isinstance(script.get('ir'), dict):
            return script['ir']
        if isinstance(script.get('draft'), dict):
            return script['draft']
        versions = script.get('versions')
        if isinstance(versions, dict) and versions:
            current = str(script.get('currentVersion'))
            if current in versions:
                return versions[current]
            return versions[max(versions, key=str)]
        raise ValueError('脚本内容为空')

    def rename(self, script_id: str, name: str) -> None:
        name = name.strip()
        if not name:
            raise ValueError('脚本名称不能为空')
        with self._lock:
            script = self.get_script(script_id)
            script['name'] = name
            content = script.get('ir') or script.get('draft')
            if isinstance(content, dict) and isinstance(content.get('meta'), dict):
                content['meta']['name'] = name
            self._write_json(self._script_path(script_id), script)

    def delete(self, script_id: str) -> None:
        with self._lock:
            self._script_path(script_id).unlink()

    def put_asset(self, sha: str, payload: bytes) -> None:
        if hashlib.sha256(payload).hexdigest() != sha:
            raise ValueError('素材校验失败')
        with self._lock:
            self._write(self.root / 'assets' / (sha + '.png'), payload)

    def save_run(self, record: dict, logs: str) -> str:
        run_id = uuid.uuid4().hex
        with self._lock:
            self._write_json(self.root / 'runs' / (run_id + '.json'),
                             {'id': run_id, 'record': record, 'logs': logs})
        return run_id

    def list_runs(self) -> list[dict]:
        with self._lock:
            directory = self.root / 'runs'
            if not directory.exists():
                return []
            runs = [json.loads(path.read_text('utf-8')) for path in directory.glob('*.json')]
            return sorted(({'id': run['id'], **run['record']} for run in runs),
                          key=lambda run: run.get('startedAt', 0), reverse=True)

    def import_legacy_drafts(self) -> tuple[int, list[str]]:
        """Recover old unsynced drafts without modifying their original files."""
        with self._lock:
            directory = self.root / 'drafts'
            marker = self.root / 'legacy-drafts-imported.json'
            imported = set(json.loads(marker.read_text('utf-8'))) if marker.exists() else set()
            count, errors = 0, []
            for path in sorted(directory.glob('*/*.json')):
                source = path.relative_to(self.root).as_posix()
                if source in imported:
                    continue
                try:
                    draft = json.loads(path.read_text('utf-8'))['draft']
                    name = draft['meta']['name']
                    if not isinstance(name, str) or not isinstance(draft.get('blocks'), dict):
                        raise ValueError('草稿格式无效')
                    script_id = uuid.uuid5(uuid.NAMESPACE_URL, 'sky-legacy-draft:' + source).hex
                    destination = self._script_path(script_id)
                    if not destination.exists():
                        self._write_json(destination, {'id': script_id, 'name': name + '（恢复的草稿）',
                            'ir': draft, 'legacySource': source})
                        count += 1
                    imported.add(source)
                    self._write_json(marker, sorted(imported))
                except (OSError, ValueError, KeyError, TypeError) as error:
                    errors.append(f'{source}: {error}')
            return count, errors

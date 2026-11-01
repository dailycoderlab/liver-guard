"""脚本只有一份内容：保存即生效，同时兼容旧版本的草稿/版本记录。"""
import json

import pytest

from skycore.local_store import LocalStore


def write(root, script_id, record):
    directory = root / 'scripts'
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f'{script_id}.json').write_text(json.dumps(record), encoding='utf-8')


def content(name):
    return {'meta': {'name': name}, 'blocks': {'main': []}, 'targets': {}}


def test_new_script_has_a_single_content_field(tmp_path):
    store = LocalStore(tmp_path)
    script = store.create_script('测试', content('测试'))
    assert script['ir']['meta']['name'] == '测试'
    # 没有草稿/版本概念。
    assert 'draft' not in script and 'versions' not in script and 'currentVersion' not in script


def test_save_overwrites_the_content(tmp_path):
    store = LocalStore(tmp_path)
    script = store.create_script('测试', content('旧'))
    store.save_script(script['id'], content('新'))
    assert store.script_ir(store.get_script(script['id']))['meta']['name'] == '新'


def test_rename_updates_the_stored_name_and_the_content(tmp_path):
    store = LocalStore(tmp_path)
    script = store.create_script('旧名', content('旧名'))
    store.rename(script['id'], '新名')
    reopened = store.get_script(script['id'])
    assert reopened['name'] == '新名'
    assert store.script_ir(reopened)['meta']['name'] == '新名'


@pytest.mark.parametrize('record,expected', [
    # 新格式
    ({'ir': content('ir')}, 'ir'),
    # 旧格式：草稿是用户最后编辑的结果，优先于已发布版本。
    ({'currentVersion': 2, 'draft': content('draft'),
      'versions': {'1': content('v1'), '2': content('v2')}}, 'draft'),
    # 只有发布版本时用 currentVersion 指向的那一版。
    ({'currentVersion': 1, 'versions': {'1': content('v1'), '2': content('v2')}}, 'v1'),
    # currentVersion 不在 versions 里时退回最后一版，而不是报错。
    ({'versions': {'3': content('v3')}}, 'v3'),
])
def test_legacy_records_still_load(tmp_path, record, expected):
    script_id = 'a' * 32
    record = {'id': script_id, 'name': 'n', **record}
    write(tmp_path, script_id, record)
    store = LocalStore(tmp_path)
    assert store.script_ir(store.get_script(script_id))['meta']['name'] == expected


def test_script_without_any_content_is_an_error(tmp_path):
    script_id = 'b' * 32
    write(tmp_path, script_id, {'id': script_id, 'name': 'empty'})
    store = LocalStore(tmp_path)
    with pytest.raises(ValueError):
        store.script_ir(store.get_script(script_id))


def test_deleted_script_is_gone(tmp_path):
    store = LocalStore(tmp_path)
    script = store.create_script('待删', content('待删'))
    store.delete(script['id'])
    assert store.list_scripts() == []

"""Opt-in startup configuration for installed desktop builds."""
from __future__ import annotations

import plistlib
import sys
from pathlib import Path

LABEL = 'com.sky.desktop'


def set_enabled(enabled: bool) -> None:
    if not getattr(sys, 'frozen', False):
        raise RuntimeError('开机自启仅支持打包后的桌面程序')
    executable = str(Path(sys.executable).resolve())
    if sys.platform == 'darwin':
        path = Path.home() / 'Library' / 'LaunchAgents' / f'{LABEL}.plist'
        if enabled:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(plistlib.dumps({'Label': LABEL, 'ProgramArguments': [executable], 'RunAtLoad': True}))
        else:
            path.unlink(missing_ok=True)
    elif sys.platform == 'win32':
        import winreg
        key_path = r'Software\Microsoft\Windows\CurrentVersion\Run'
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, key_path) as key:
            if enabled:
                winreg.SetValueEx(key, 'Sky', 0, winreg.REG_SZ, f'"{executable}"')
            else:
                try:
                    winreg.DeleteValue(key, 'Sky')
                except FileNotFoundError:
                    pass
    else:
        raise RuntimeError('当前平台不支持开机自启')

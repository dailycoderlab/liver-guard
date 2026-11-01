"""Permission checks.

macOS does not let an app grant itself either permission, and it never attributes
it to the bare interpreter: TCC stores the *responsible* process, which for
``uv run sky-gui`` or ``python -m skygui`` is the terminal or IDE that launched
it. A venv interpreter also has no app bundle, so it cannot appear in the
System Settings list at all. ``responsible_application`` reports who will
actually show up there; only a packaged ``Sky.app`` gets its own entry.
"""

from __future__ import annotations

import os
import subprocess
from dataclasses import dataclass


@dataclass
class PermissionState:
    accessibility: bool
    screen_recording: bool

    @property
    def ok(self) -> bool:
        return self.accessibility and self.screen_recording

    def problems(self, owner: str | None = None) -> list[str]:
        """Human-readable fixes; ``owner`` defaults to the app macOS will list."""
        if owner is None:
            owner = owner_label()
        issues: list[str] = []
        if not self.accessibility:
            issues.append("辅助功能：系统设置 → 隐私与安全性 → 辅助功能，勾选 " + owner)
        if not self.screen_recording:
            issues.append("屏幕录制：系统设置 → 隐私与安全性 → 屏幕录制，勾选 " + owner)
            issues.append("勾选后要完全退出该程序再重新启动，权限才会生效。")
        return issues


def _parent_pid(pid: int) -> int | None:
    try:
        output = subprocess.run(
            ["ps", "-o", "ppid=", "-p", str(pid)],
            capture_output=True, text=True, timeout=5, check=False,
        ).stdout.strip()
        return int(output) if output else None
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def responsible_application() -> tuple[str, str] | None:
    """(name, bundle path) of the app macOS attributes this process to.

    Walks the whole ancestor chain and keeps the outermost bundle: nested
    helpers such as ``Cursor Helper (Plugin).app`` live inside another app and
    are not what System Settings lists. A packaged build stops at Sky.app; a dev
    run ends at Terminal, iTerm, Cursor or whichever GUI app started the shell.
    """
    try:
        from AppKit import NSRunningApplication
    except Exception:
        return None
    found: tuple[str, str] | None = None
    pid = os.getpid()
    seen = 0
    while pid > 1 and seen < 64:
        seen += 1
        try:
            app = NSRunningApplication.runningApplicationWithProcessIdentifier_(pid)
            url = app.bundleURL() if app is not None else None
            if url is not None:
                found = (str(app.localizedName()), str(url.path()))
        except Exception:
            return found
        parent = _parent_pid(pid)
        if parent is None:
            break
        pid = parent
    return found



def owner_label() -> str:
    """Who to tick in System Settings, with a hint when no bundle is involved."""
    found = responsible_application()
    if found is None:
        return "你运行 Sky 的程序（终端 Terminal / iTerm，或打包后的 Sky.app）"
    name, path = found
    if path.endswith("Sky.app"):
        return f"Sky（{path}）"
    return f"{name}（{path}）"


def accessibility_trusted(prompt: bool = False) -> bool:
    try:
        from ApplicationServices import (
            AXIsProcessTrusted,
            AXIsProcessTrustedWithOptions,
        )

        if prompt:
            return bool(AXIsProcessTrustedWithOptions({"AXTrustedCheckOptionPrompt": True}))
        return bool(AXIsProcessTrusted())
    except Exception:
        return False


def screen_recording_allowed(prompt: bool = False) -> bool:
    try:
        from Quartz import CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess

        if prompt:
            return bool(CGRequestScreenCaptureAccess())
        return bool(CGPreflightScreenCaptureAccess())
    except Exception:
        return False


def current_state() -> PermissionState:
    return PermissionState(
        accessibility=accessibility_trusted(),
        screen_recording=screen_recording_allowed(),
    )


def request_missing() -> PermissionState:
    """Trigger the system prompts once; the user still has to flip the switches."""
    if not accessibility_trusted():
        accessibility_trusted(prompt=True)
    if not screen_recording_allowed():
        screen_recording_allowed(prompt=True)
    return current_state()


# Settings panes by permission key, so the GUI can jump straight to the right
# list instead of telling the user to navigate four levels of System Settings.
# 按权限键映射到设置面板；GUI 直接跳转，省得用户自己逐层点进去。
_PANES = {
    "accessibility": "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    "screen_recording": "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture",
}


def open_settings(kind: str) -> bool:
    """Open the System Settings pane for one permission."""
    url = _PANES.get(kind)
    if not url:
        return False
    try:
        subprocess.run(["open", url], check=True, timeout=5)
        return True
    except (OSError, subprocess.SubprocessError):
        return False

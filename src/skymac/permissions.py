"""Permission checks.

macOS does not let an app grant itself either permission, and it attributes the
request to the parent process (Terminal / iTerm), not to the Python binary. That
is why ``doctor`` has to spell out where to click.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass
class PermissionState:
    accessibility: bool
    screen_recording: bool

    @property
    def ok(self) -> bool:
        return self.accessibility and self.screen_recording

    def problems(self) -> list[str]:
        issues: list[str] = []
        if not self.accessibility:
            issues.append(
                "辅助功能未授权：系统设置 → 隐私与安全性 → 辅助功能，勾选你运行 sky 的终端（Terminal / iTerm）"
            )
        if not self.screen_recording:
            issues.append(
                "屏幕录制未授权：系统设置 → 隐私与安全性 → 屏幕录制，勾选你的终端，然后完全退出并重开终端"
            )
        return issues


def accessibility_trusted(prompt: bool = False) -> bool:
    try:
        from ApplicationServices import AXIsProcessTrusted, AXIsProcessTrustedWithOptions

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

"""Test helpers: synthetic scenes that look enough like a game UI to exercise
the matcher without needing a screen recording permission."""

from __future__ import annotations

import hashlib

import cv2
import numpy as np

from skycore.frame import Frame
from skycore.geometry import Bounds
from skycore.schema import TemplateTarget


def draw_button(image: np.ndarray, x: int, y: int, w: int, h: int, label: str = "START") -> None:
    cv2.rectangle(image, (x, y), (x + w, y + h), (196, 120, 60), -1)
    cv2.rectangle(image, (x, y), (x + w, y + h), (250, 240, 220), max(2, w // 40))
    font_scale = max(0.4, h / 90.0)
    thickness = max(1, int(round(h / 40)))
    cv2.putText(
        image,
        label,
        (x + int(w * 0.12), y + int(h * 0.66)),
        cv2.FONT_HERSHEY_SIMPLEX,
        font_scale,
        (255, 252, 245),
        thickness,
        cv2.LINE_AA,
    )


def make_scene(
    scale: float = 2.0,
    width_pt: float = 600.0,
    height_pt: float = 400.0,
    button_center_pt: tuple[float, float] = (300.0, 220.0),
    button_size_pt: tuple[float, float] = (120.0, 56.0),
    decoy_at_pt: tuple[float, float] | None = None,
) -> Frame:
    width = int(round(width_pt * scale))
    height = int(round(height_pt * scale))
    rng = np.random.default_rng(7)
    base = np.full((height, width, 3), (48, 44, 40), dtype=np.int16)
    noise = rng.integers(-5, 6, size=(height, width, 1), dtype=np.int16)
    image = np.clip(base + noise, 0, 255).astype(np.uint8)

    bw = int(round(button_size_pt[0] * scale))
    bh = int(round(button_size_pt[1] * scale))

    def place(center_pt: tuple[float, float]) -> tuple[int, int]:
        return (int(round(center_pt[0] * scale - bw / 2)), int(round(center_pt[1] * scale - bh / 2)))

    bx, by = place(button_center_pt)
    draw_button(image, bx, by, bw, bh)

    if decoy_at_pt is not None:
        dx, dy = place(decoy_at_pt)
        draw_button(image, dx, dy, bw, bh)

    bounds = Bounds(x=0.0, y=0.0, w=width_pt, h=height_pt, scale=scale)
    return Frame(image=image, bounds=bounds)


def crop_asset(image: np.ndarray, center_pt: tuple[float, float], size_pt: tuple[float, float], scale: float) -> bytes:
    x = int(round(center_pt[0] * scale - size_pt[0] * scale / 2))
    y = int(round(center_pt[1] * scale - size_pt[1] * scale / 2))
    w = int(round(size_pt[0] * scale))
    h = int(round(size_pt[1] * scale))
    patch = image[y : y + h, x : x + w]
    ok, buffer = cv2.imencode(".png", patch)
    if not ok:
        raise RuntimeError("png encode failed")
    return buffer.tobytes()


def button_target(
    asset_bytes: bytes,
    size_pt: tuple[float, float],
    record_scale: float,
    record_width: float = 600.0,
    roi: tuple[float, float, float, float] = (0.0, 0.0, 1.0, 1.0),
    threshold: float = 0.8,
    multi: str = "highest",
    anchor: tuple[float, float] = (0.5, 0.5),
) -> TemplateTarget:
    return TemplateTarget(
        type="template",
        asset=hashlib.sha256(asset_bytes).hexdigest(),
        assetBox={"w": size_pt[0] * record_scale, "h": size_pt[1] * record_scale},
        recordScale=record_scale,
        recordWidth=record_width,
        roi=roi,
        anchor=anchor,
        threshold=threshold,
        warnThreshold=0.6,
        verify="rgb",
        multi=multi,
        scale={"mode": "auto", "min": 0.7, "max": 1.5, "steps": 9},
    )


class FakeDriver:
    """Minimal Driver for interpreter tests: fixed screen, recorded input."""

    def __init__(self, frame: Frame) -> None:
        self.frame_obj = frame
        self.taps: list[tuple[float, float]] = []
        self.swipes: list[tuple[float, float, float, float]] = []
        self.keys: list[str] = []
        self.texts: list[str] = []
        self.captures = 0

    def bounds(self) -> Bounds:
        return self.frame_obj.bounds

    def capture(self) -> Frame:
        self.captures += 1
        frame = Frame(image=self.frame_obj.image, bounds=self.frame_obj.bounds, seq=self.captures)
        frame.hash()
        return frame

    def tap(self, x: float, y: float, duration_ms: int = 60) -> None:
        self.taps.append((x, y))

    def swipe(self, x1: float, y1: float, x2: float, y2: float, duration_ms: int = 300) -> None:
        self.swipes.append((x1, y1, x2, y2))

    def key(self, name: str) -> None:
        self.keys.append(name)

    def text(self, value: str) -> None:
        self.texts.append(value)

    def ax_at(self, x: float, y: float):
        return None

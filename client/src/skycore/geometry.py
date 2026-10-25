"""Coordinate maths for capture surfaces.

There are three coordinate spaces in this system and mixing them up is the
single most common source of "the click landed 40 pixels away" bugs:

1.  **canvas / normalized** - ``[0, 1]`` relative to the captured image content.
    This is what travels over the wire, so neither side needs to know the other's
    pixel density.
2.  **logical points** - what the OS input APIs want (Quartz global points on
    macOS, points on iOS, dp on Android). Also the space ``Bounds`` is expressed
    in.
3.  **device pixels** - what screenshots and image matching use.

The conversion between 2 and 3 is a single scalar, ``scale`` (pixels per point),
which is exactly why capture bounds carry it everywhere.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Rect:
    """Axis aligned rectangle. The unit is implied by the context."""

    x: float
    y: float
    w: float
    h: float

    @property
    def right(self) -> float:
        return self.x + self.w

    @property
    def bottom(self) -> float:
        return self.y + self.h

    @property
    def center(self) -> tuple[float, float]:
        return (self.x + self.w / 2.0, self.y + self.h / 2.0)

    def contains(self, px: float, py: float) -> bool:
        return self.x <= px <= self.right and self.y <= py <= self.bottom

    def intersection(self, other: "Rect") -> "Rect":
        x0 = max(self.x, other.x)
        y0 = max(self.y, other.y)
        x1 = min(self.right, other.right)
        y1 = min(self.bottom, other.bottom)
        if x1 <= x0 or y1 <= y0:
            return Rect(x0, y0, 0.0, 0.0)
        return Rect(x0, y0, x1 - x0, y1 - y0)

    def iou(self, other: "Rect") -> float:
        inter = self.intersection(other)
        area = inter.w * inter.h
        if area <= 0:
            return 0.0
        union = self.w * self.h + other.w * other.h - area
        return area / union if union > 0 else 0.0

    def as_tuple(self) -> tuple[float, float, float, float]:
        return (self.x, self.y, self.w, self.h)


@dataclass(frozen=True)
class Bounds:
    """A capture surface in logical points, in global screen coordinates."""

    x: float
    y: float
    w: float
    h: float
    scale: float = 1.0
    orientation: str | None = None

    @property
    def rect(self) -> Rect:
        return Rect(self.x, self.y, self.w, self.h)

    def to_json(self) -> dict:
        payload = {"x": self.x, "y": self.y, "w": self.w, "h": self.h, "scale": self.scale}
        if self.orientation:
            payload["orientation"] = self.orientation
        return payload

    @classmethod
    def from_json(cls, payload: dict) -> "Bounds":
        return cls(
            x=float(payload["x"]),
            y=float(payload["y"]),
            w=float(payload["w"]),
            h=float(payload["h"]),
            scale=float(payload.get("scale", 1.0)),
            orientation=payload.get("orientation"),
        )

    def close_to(self, other: "Bounds", tolerance: float = 1.0) -> bool:
        return (
            abs(self.x - other.x) <= tolerance
            and abs(self.y - other.y) <= tolerance
            and abs(self.w - other.w) <= tolerance
            and abs(self.h - other.h) <= tolerance
        )


def clamp(value: float, low: float, high: float) -> float:
    return max(low, min(high, value))


def normalized_to_global(nx: float, ny: float, bounds: Bounds) -> tuple[float, float]:
    """Normalized frame coordinates -> global logical points."""
    return (bounds.x + nx * bounds.w, bounds.y + ny * bounds.h)


def global_to_normalized(px: float, py: float, bounds: Bounds) -> tuple[float, float]:
    if bounds.w <= 0 or bounds.h <= 0:
        return (0.5, 0.5)
    return ((px - bounds.x) / bounds.w, (py - bounds.y) / bounds.h)


def anchor_in_box(click: tuple[float, float], box: Rect) -> tuple[float, float]:
    """Where inside a matched box to click, as a ratio in ``[0.1, 0.9]``.

    A ratio rather than a centre point: a click on the left third of a long
    button stays on the left third after the screen gets bigger.
    """
    ax = (click[0] - box.x) / box.w if box.w > 0 else 0.5
    ay = (click[1] - box.y) / box.h if box.h > 0 else 0.5
    return (clamp(ax, 0.1, 0.9), clamp(ay, 0.1, 0.9))


def point_in_box(anchor: tuple[float, float], box: Rect) -> tuple[float, float]:
    return (box.x + anchor[0] * box.w, box.y + anchor[1] * box.h)


def search_window(click: tuple[float, float], size: tuple[float, float], bounds: Bounds) -> Rect:
    """A window of ``size`` logical points centred on the click, clamped inside
    the capture surface so the device never captures outside its own screen."""
    w = min(size[0], bounds.w)
    h = min(size[1], bounds.h)
    x = clamp(click[0] - w / 2.0, bounds.x, bounds.x + bounds.w - w)
    y = clamp(click[1] - h / 2.0, bounds.y, bounds.y + bounds.h - h)
    return Rect(x, y, w, h)


def infer_roi(box: Rect, bounds: Bounds, cols: int = 3, rows: int = 5) -> tuple[float, float, float, float]:
    """Search region for a target: the grid cell it sits in, plus one ring.

    Returns a normalized rectangle. Constraining the search keeps matching fast
    and disambiguates controls that look alike in different parts of the screen.

    The matched box is always folded in, so a control on the very edge of the
    surface is still inside its own search region - clamping the grid cell alone
    would silently exclude it.
    """
    if bounds.w <= 0 or bounds.h <= 0:
        return (0.0, 0.0, 1.0, 1.0)

    cx, cy = box.center
    nx = clamp((cx - bounds.x) / bounds.w, 0.0, 1.0)
    ny = clamp((cy - bounds.y) / bounds.h, 0.0, 1.0)

    col = min(cols - 1, int(nx * cols))
    row = min(rows - 1, int(ny * rows))

    c0 = max(0, col - 1) / cols
    c1 = min(cols, col + 2) / cols
    r0 = max(0, row - 1) / rows
    r1 = min(rows, row + 2) / rows

    box_x0 = clamp((box.x - bounds.x) / bounds.w, 0.0, 1.0)
    box_y0 = clamp((box.y - bounds.y) / bounds.h, 0.0, 1.0)
    box_x1 = clamp((box.x + box.w - bounds.x) / bounds.w, 0.0, 1.0)
    box_y1 = clamp((box.y + box.h - bounds.y) / bounds.h, 0.0, 1.0)

    margin = 0.02
    x0 = clamp(min(c0, box_x0) - margin, 0.0, 1.0)
    y0 = clamp(min(r0, box_y0) - margin, 0.0, 1.0)
    x1 = clamp(max(c1, box_x1) + margin, 0.0, 1.0)
    y1 = clamp(max(r1, box_y1) + margin, 0.0, 1.0)

    return (round(x0, 4), round(y0, 4), round(max(0.02, x1 - x0), 4), round(max(0.02, y1 - y0), 4))


def roi_to_pixels(roi: tuple[float, float, float, float], width: int, height: int) -> tuple[int, int, int, int]:
    """Normalized roi -> integer pixel box inside an image of ``width`` x ``height``."""
    x0 = int(round(roi[0] * width))
    y0 = int(round(roi[1] * height))
    x1 = int(round((roi[0] + roi[2]) * width))
    y1 = int(round((roi[1] + roi[3]) * height))
    x0 = max(0, min(width - 1, x0))
    y0 = max(0, min(height - 1, y0))
    x1 = max(x0 + 1, min(width, x1))
    y1 = max(y0 + 1, min(height, y1))
    return (x0, y0, x1 - x0, y1 - y0)

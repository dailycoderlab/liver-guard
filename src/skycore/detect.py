"""Button candidates in image pixel coordinates.

Pass an H x W x 4 uint8 RGBA image, a pixel-space click and options to
``detect_candidates``. Alpha is ignored. Results are ordered AX, up to two
contours (descending score), color if no contour, then fallback, with later
near-duplicates removed.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from typing import Literal

import cv2
import numpy as np
from numpy.typing import NDArray

Strictness = Literal["loose", "normal", "tight"]
CandidateKind = Literal["ax", "contour", "color", "fallback"]


@dataclass(frozen=True)
class Point:
    x: float
    y: float


@dataclass(frozen=True)
class PixelBox:
    x: float
    y: float
    w: float
    h: float


@dataclass(frozen=True)
class Candidate(PixelBox):
    kind: CandidateKind
    score: float


@dataclass(frozen=True)
class DetectOptions:
    scale: float
    min_side: float = 24
    max_side: float = 220
    padding: float = 0.08
    strictness: Strictness = "normal"
    ax_box: PixelBox | None = None


@dataclass(frozen=True)
class _Component:
    x0: int
    y0: int
    x1: int
    y1: int
    area: int


def _round(value: float) -> int:
    """JavaScript Math.round, including negative half values."""
    return math.floor(value + 0.5)


def iou(a: PixelBox, b: PixelBox) -> float:
    x0, y0 = max(a.x, b.x), max(a.y, b.y)
    x1, y1 = min(a.x + a.w, b.x + b.w), min(a.y + a.h, b.y + b.h)
    if x1 <= x0 or y1 <= y0:
        return 0.0
    intersection = (x1 - x0) * (y1 - y0)
    return intersection / (a.w * a.h + b.w * b.h - intersection)


def _border_coverage(binary: NDArray[np.uint8], c: _Component) -> float:
    # Count corners twice, and overlapping strips independently, just like TS.
    hit = total = 0
    h, w = binary.shape
    for t in range(2):
        for y in (c.y0 + t, c.y1 - t):
            if 0 <= y < h:
                hit += int(binary[y, c.x0 : c.x1 + 1].sum())
                total += c.x1 - c.x0 + 1
        for x in (c.x0 + t, c.x1 - t):
            if 0 <= x < w:
                hit += int(binary[c.y0 : c.y1 + 1, x].sum())
                total += c.y1 - c.y0 + 1
    return hit / total if total else 0.0


def _dedupe(candidates: list[Candidate], width: int, height: int,
            max_side: float, padding: float) -> list[Candidate]:
    out: list[Candidate] = []
    for candidate in candidates:
        exact = candidate.kind == "ax"
        grow = 0 if exact else max(candidate.w, candidate.h) * padding
        x, y = candidate.x - grow, candidate.y - grow
        w, h = candidate.w + 2 * grow, candidate.h + 2 * grow
        if not exact and (w > max_side or h > max_side):
            shrink = max_side / max(w, h)
            cx, cy = x + w / 2, y + h / 2
            w, h = w * shrink, h * shrink
            x, y = cx - w / 2, cy - h / 2
        x = max(0, min(width - 8, x))
        y = max(0, min(height - 8, y))
        w, h = min(w, width - x), min(h, height - y)
        rounded = Candidate(_round(x), _round(y), _round(w), _round(h),
                            candidate.kind, candidate.score)
        if not any(iou(existing, rounded) > 0.75 for existing in out):
            out.append(rounded)
    return out


def _flood_region(rgb: NDArray[np.uint8], sx: int, sy: int,
                  tolerance: int, max_pixels: int) -> _Component | None:
    h, w = rgb.shape[:2]
    if sx < 0 or sy < 0 or sx >= w or sy >= h:
        return None
    # Fixed range compares each pixel to the original seed, not its neighbor.
    # MASK_ONLY leaves the source unchanged; mask coordinates have a 1px border.
    mask = np.zeros((h + 2, w + 2), dtype=np.uint8)
    area, _, _, (x, y, bw, bh) = cv2.floodFill(
        rgb, mask, (sx, sy), (0, 0, 0),
        (tolerance,) * 3, (tolerance,) * 3,
        flags=8 | cv2.FLOODFILL_FIXED_RANGE | cv2.FLOODFILL_MASK_ONLY | (1 << 8),
    )
    if area > max_pixels:
        return None
    return _Component(x, y, x + bw - 1, y + bh - 1, area)


def detect_candidates(
    rgba: NDArray[np.uint8], click: Point, options: DetectOptions,
) -> list[Candidate]:
    """Detect controls at ``click`` in an RGBA image.

    ``scale`` is device pixels per logical point; min_side and max_side are
    logical points. All click, AX and returned box coordinates are image pixels.
    Fallback is always proposed, but may be removed as a near-duplicate.
    """
    if not isinstance(rgba, np.ndarray) or rgba.dtype != np.uint8 or rgba.ndim != 3 or rgba.shape[2] != 4:
        raise ValueError("rgba must be an H x W x 4 uint8 numpy array")
    height, width = rgba.shape[:2]
    if not width or not height:
        raise ValueError("rgba must have nonzero width and height")
    if options.strictness not in ("loose", "normal", "tight"):
        raise ValueError("strictness must be loose, normal, or tight")
    scale = options.scale or 1
    min_side, max_side = options.min_side * scale, options.max_side * scale
    candidates: list[Candidate] = []
    ax = options.ax_box
    if ax is not None and ax.w > 0 and ax.h > 0:
        candidates.append(Candidate(ax.x, ax.y, ax.w, ax.h, "ax", 1))

    downsample = 2 if width * height > 2_500_000 else 1
    h, w = height // downsample, width // downsample
    # The trailing row/column is dropped for odd dimensions.
    rgb = np.ascontiguousarray(rgba[: h * downsample : downsample,
                                    : w * downsample : downsample, :3])
    click_x, click_y = click.x / downsample, click.y / downsample
    rgb16 = rgb.astype(np.uint16)
    gray = ((rgb16[:, :, 0] * 77 + rgb16[:, :, 1] * 150 + rgb16[:, :, 2] * 29) >> 8).astype(np.uint8)

    # Divide by the actual number of in-bounds pixels, truncating rather than
    # rounding, so box blur at edges and in the interior stays consistent.
    sums = cv2.boxFilter(gray, cv2.CV_32S, (3, 3), normalize=False, borderType=cv2.BORDER_CONSTANT)
    counts = cv2.boxFilter(np.ones((h, w), dtype=np.uint8), cv2.CV_32S, (3, 3),
                           normalize=False, borderType=cv2.BORDER_CONSTANT)
    blurred = (sums // counts).astype(np.uint8)
    gx = cv2.Scharr(blurred, cv2.CV_16S, 1, 0)
    gy = cv2.Scharr(blurred, cv2.CV_16S, 0, 1)
    mag = np.abs(gx.astype(np.int32)) + np.abs(gy.astype(np.int32))
    mag[0, :] = mag[-1, :] = 0
    mag[:, 0] = mag[:, -1] = 0
    maximum = int(mag.max())
    normalized = (mag * (255.0 / maximum)).astype(np.uint8) if maximum else np.zeros_like(gray)
    otsu, _ = cv2.threshold(normalized, 0, 255, cv2.THRESH_BINARY | cv2.THRESH_OTSU)
    bias = {"loose": -12, "normal": 0, "tight": 14}[options.strictness]
    binary = (normalized > max(4, min(250, otsu + bias))).astype(np.uint8)
    kernel = np.ones((3, 3), dtype=np.uint8)
    binary = cv2.dilate(binary, kernel, borderType=cv2.BORDER_CONSTANT, borderValue=0)
    binary = cv2.erode(binary, kernel, borderType=cv2.BORDER_CONSTANT, borderValue=0)
    _, _, stats, _ = cv2.connectedComponentsWithStatsWithAlgorithm(binary, 8, cv2.CV_32S, cv2.CCL_SAUF)
    min_area = max(12, (min_side / downsample) ** 2 * 0.35)

    def fit(component: _Component, kind: CandidateKind) -> Candidate | None:
        x0, y0, x1, y1 = component.x0, component.y0, component.x1, component.y1
        bw, bh = x1 - x0 + 1, y1 - y0 + 1
        if not (x0 <= click_x <= x1 and y0 <= click_y <= y1):
            return None
        if bw < min_side / downsample or bh < min_side / downsample:
            return None
        if bw > max_side / downsample or bh > max_side / downsample:
            return None
        if not 0.25 <= bw / bh <= 4:
            return None
        rectangularity = max(component.area / (bw * bh), _border_coverage(binary, component))
        required = {"loose": 0.45, "normal": 0.6, "tight": 0.75}[options.strictness]
        if rectangularity < required:
            return None
        pixels = gray[y0 : y1 + 1, x0 : x1 + 1].astype(np.float64)
        if pixels.var() < 24:
            return None
        return Candidate(x0 * downsample, y0 * downsample, bw * downsample,
                         bh * downsample, kind, min(0.95, rectangularity))

    contour_hits: list[Candidate] = []
    for x, y, bw, bh, area in stats[1:]:
        if area >= min_area:
            hit = fit(_Component(int(x), int(y), int(x + bw - 1), int(y + bh - 1), int(area)), "contour")
            if hit is not None:
                contour_hits.append(hit)
    contour_hits.sort(key=lambda candidate: -candidate.score)
    contour_hits = contour_hits[:2]
    candidates.extend(contour_hits)
    if not contour_hits:
        tolerance = {"loose": 28, "normal": 18, "tight": 10}[options.strictness]
        region = _flood_region(rgb, _round(click_x), _round(click_y), tolerance,
                               _round((max_side / downsample) ** 2))
        if region is not None:
            color = fit(region, "color")
            if color is not None:
                candidates.append(Candidate(color.x, color.y, color.w, color.h, "color", min(0.7, color.score * 0.8)))
    fallback_side = min(max_side, max(64 * scale, min_side * 2))
    candidates.append(Candidate(click.x - fallback_side / 2, click.y - fallback_side / 2,
                                fallback_side, fallback_side, "fallback", 0.2))
    return _dedupe(candidates, width, height, max_side, options.padding)

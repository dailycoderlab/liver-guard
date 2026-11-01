"""Template matching primitives: single scale, pyramid multi-scale, NMS."""

from __future__ import annotations

from dataclasses import dataclass

import cv2
import numpy as np


@dataclass
class Match:
    x: float
    y: float
    w: float
    h: float
    score: float
    scale: float = 1.0

    @property
    def center(self) -> tuple[float, float]:
        return (self.x + self.w / 2.0, self.y + self.h / 2.0)


LOW_TEXTURE_STD = 2.0


def to_match_space(image: np.ndarray, mask: str) -> np.ndarray:
    """Optionally reduce both sides to edge maps.

    Edge matching survives skin changes and colour tints because it only looks at
    structure. It is not the default because it throws away colour information
    that is otherwise useful for disambiguation.
    """
    if mask != "edges":
        return image
    gray = image if image.ndim == 2 else cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    return cv2.Canny(gray, 50, 150)


def match_at_scale(image: np.ndarray, template: np.ndarray, threshold: float) -> np.ndarray:
    """Raw correlation map. Returns an empty array when the sizes are invalid."""
    ih, iw = image.shape[:2]
    th, tw = template.shape[:2]
    if th > ih or tw > iw or th < 4 or tw < 4:
        return np.empty((0, 0), dtype=np.float32)
    if float(np.std(template)) < LOW_TEXTURE_STD:
        # A flat template has no correlation signal. Without this guard
        # TM_CCOEFF_NORMED returns 1.0 everywhere for a constant patch, which
        # would look like a perfect match at the top left corner.
        return np.empty((0, 0), dtype=np.float32)
    result = cv2.matchTemplate(image, template, cv2.TM_CCOEFF_NORMED)
    return np.nan_to_num(result, nan=0.0, posinf=0.0, neginf=0.0).astype(np.float32)


def collect_peaks(result: np.ndarray, template: tuple[int, int], threshold: float, limit: int = 5) -> list[Match]:
    """Non-maximum suppression by blacking out the neighbourhood of each hit.

    Same idea as Airtest's "find_all_results": take the global max, record it,
    paint over that area so the next iteration finds a different control.
    """
    if result.size == 0:
        return []

    work = result.copy()
    tw, th = template
    radius_x = max(2, tw // 2)
    radius_y = max(2, th // 2)
    out: list[Match] = []

    while len(out) < limit:
        _, max_val, _, max_loc = cv2.minMaxLoc(work)
        if max_val < threshold:
            break
        x, y = max_loc
        out.append(Match(float(x), float(y), float(tw), float(th), float(max_val)))
        x0 = max(0, x - radius_x)
        y0 = max(0, y - radius_y)
        x1 = min(work.shape[1], x + radius_x + 1)
        y1 = min(work.shape[0], y + radius_y + 1)
        work[y0:y1, x0:x1] = -1.0

    return out


def scale_ladder(spec: dict | None) -> list[float]:
    spec = spec or {}
    low = float(spec.get("min", 0.8))
    high = float(spec.get("max", 1.25))
    steps = int(spec.get("steps", 7))
    low = max(0.2, min(low, 3.0))
    high = max(low, min(high, 3.0))
    steps = max(1, min(steps, 21))
    if steps == 1:
        return [(low + high) / 2.0]
    step = (high - low) / (steps - 1)
    return [round(low + step * i, 4) for i in range(steps)]


def _resize(image: np.ndarray, factor: float) -> np.ndarray:
    height, width = image.shape[:2]
    new_w = max(4, int(round(width * factor)))
    new_h = max(4, int(round(height * factor)))
    if new_w == width and new_h == height:
        return image
    interpolation = cv2.INTER_AREA if factor < 1.0 else cv2.INTER_LINEAR
    return cv2.resize(image, (new_w, new_h), interpolation=interpolation)


def multi_scale_match(
    search: np.ndarray,
    template: np.ndarray,
    scales: list[float],
    threshold: float,
    max_count: int = 3,
) -> list[Match]:
    """Coarse-to-fine search over a scale ladder.

    Coarse: half resolution, every scale, pick the best.
    Fine:   full resolution, three scales around the winner, small window.
    """
    if search.size == 0 or template.size == 0:
        return []
    if float(np.std(template)) < LOW_TEXTURE_STD:
        # A flat template has no correlation signal; matching it is meaningless.
        return []

    ih, iw = search.shape[:2]
    th, tw = template.shape[:2]
    if th > ih or tw > iw:
        return []

    # Coarse pass. Budget: only downscale when the work would be expensive.
    work_area = iw * ih * tw * th
    coarse = 0.5 if work_area > 4_000_000 else 1.0
    search_coarse = _resize(search, coarse) if coarse != 1.0 else search

    best: tuple[float, float, int, int] | None = None
    for scale in scales:
        factor = scale * coarse
        tpl = _resize(template, factor)
        if tpl.shape[0] > search_coarse.shape[0] or tpl.shape[1] > search_coarse.shape[1]:
            continue
        result = match_at_scale(search_coarse, tpl, threshold)
        if result.size == 0:
            continue
        _, max_val, _, max_loc = cv2.minMaxLoc(result)
        if best is None or max_val > best[0]:
            best = (float(max_val), scale, int(max_loc[0]), int(max_loc[1]))

    if best is None:
        return []

    _, best_scale, coarse_x, coarse_y = best
    fine_scales = sorted(
        {
            max(0.2, best_scale * 0.97),
            best_scale,
            min(3.0, best_scale * 1.03),
        }
    )

    # Fine pass over a window around the coarse hit. The coarse image was scaled
    # BY `coarse`, so a coarse coordinate maps back by dividing by it.
    full_x = coarse_x / coarse
    full_y = coarse_y / coarse
    pad = 12
    tpl_w = int(round(tw * best_scale))
    tpl_h = int(round(th * best_scale))
    wx0 = max(0, int(full_x) - pad)
    wy0 = max(0, int(full_y) - pad)
    wx1 = min(iw, int(full_x) + tpl_w + pad + 1)
    wy1 = min(ih, int(full_y) + tpl_h + pad + 1)
    window = search[wy0:wy1, wx0:wx1]

    matches: list[Match] = []
    for scale in fine_scales:
        tpl = _resize(template, scale)
        if tpl.shape[0] > window.shape[0] or tpl.shape[1] > window.shape[1]:
            continue
        result = match_at_scale(window, tpl, threshold)
        if result.size == 0:
            continue
        for peak in collect_peaks(result, (tpl.shape[1], tpl.shape[0]), threshold, max_count):
            matches.append(
                Match(
                    peak.x + wx0,
                    peak.y + wy0,
                    float(tpl.shape[1]),
                    float(tpl.shape[0]),
                    peak.score,
                    scale,
                )
            )

    matches.sort(key=lambda match: match.score, reverse=True)
    return matches[:max_count]


def single_scale_match(search: np.ndarray, template: np.ndarray, threshold: float, max_count: int = 3) -> list[Match]:
    result = match_at_scale(search, template, threshold)
    if result.size == 0:
        return []
    return collect_peaks(result, (template.shape[1], template.shape[0]), threshold, max_count)

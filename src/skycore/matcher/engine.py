"""Target resolution: from a stored crop to a clickable point on this device."""

from __future__ import annotations

import time
from dataclasses import dataclass, field

import cv2
import numpy as np

from ..geometry import Rect, point_in_box, roi_to_pixels
from ..schema import PointTarget, TemplateTarget
from . import keypoint
from .assets import AssetStore
from .normalize import CANONICAL_WIDTH, Normalized, normalize_frame, template_factor
from .tpl import Match, multi_scale_match, scale_ladder, single_scale_match, to_match_space
from .track import Tracker
from .verify import MIN_SIMILARITY, color_similarity

LOW_TEXTURE_STD = 2.0


@dataclass
class Hit:
    box: Rect
    point: tuple[float, float]
    score: float
    strategy: str
    elapsed_ms: float
    reused: bool = False
    warnings: list[str] = field(default_factory=list)


class MatcherEngine:
    def __init__(
        self,
        assets: AssetStore,
        tracker: Tracker | None = None,
        canonical_width: float = CANONICAL_WIDTH,
        allow_reuse: bool = True,
    ) -> None:
        self.assets = assets
        self.tracker = tracker or Tracker()
        self.canonical_width = canonical_width
        self.allow_reuse = allow_reuse

    # ------------------------------------------------------------- public API

    def resolve(
        self,
        target: TemplateTarget | PointTarget,
        target_id: str,
        frame,
        offset: tuple[float, float] = (0.0, 0.0),
    ) -> Hit | None:
        started = time.perf_counter()

        if isinstance(target, PointTarget) or getattr(target, "type", None) == "point":
            return self._resolve_point(target, frame, offset, started)

        return self._resolve_template(target, target_id, frame, offset, started)

    # --------------------------------------------------------------- helpers

    def _resolve_point(self, target, frame, offset, started) -> Hit | None:
        nx, ny = target.norm
        x = frame.bounds.x + nx * frame.bounds.w
        y = frame.bounds.y + ny * frame.bounds.h
        zone = target.zone
        box = Rect(
            frame.bounds.x + zone[0] * frame.bounds.w,
            frame.bounds.y + zone[1] * frame.bounds.h,
            zone[2] * frame.bounds.w,
            zone[3] * frame.bounds.h,
        )
        if not box.contains(x, y):
            return None
        warnings = ["no_visual_anchor"]
        point = (x + offset[0] * box.w, y + offset[1] * box.h)
        return Hit(
            box=box,
            point=point,
            score=1.0,
            strategy="point",
            elapsed_ms=(time.perf_counter() - started) * 1000.0,
            warnings=warnings,
        )

    def _normalized(self, frame) -> Normalized:
        # Cached on the frame itself: frames are short lived, and keying a dict by
        # id() risks a recycled id returning a stale image after a GC.
        cached = getattr(frame, "_sky_normalized", None)
        if cached is not None and cached[0] == self.canonical_width:
            return cached[1]
        normalized = normalize_frame(frame.image, frame.bounds, self.canonical_width)
        setattr(frame, "_sky_normalized", (self.canonical_width, normalized))
        return normalized

    def _resolve_template(self, target: TemplateTarget, target_id: str, frame, offset, started) -> Hit | None:
        warnings: list[str] = []
        frame_hash = frame.hash()

        if self.allow_reuse:
            remembered = self.tracker.reusable(target_id, frame_hash)
            if remembered is not None:
                return Hit(
                    box=remembered.box,
                    point=remembered.point,
                    score=remembered.score,
                    strategy="reuse",
                    elapsed_ms=(time.perf_counter() - started) * 1000.0,
                    reused=True,
                )

        normalized = self._normalized(frame)
        factor = template_factor(
            target.recordWidth,
            target.recordScale,
            fallback_width=frame.bounds.w,
            canonical_width=self.canonical_width,
        )
        try:
            template_raw = self.assets.get_scaled(target.asset, factor)
        except KeyError:
            return None

        if float(np.std(template_raw)) < LOW_TEXTURE_STD:
            warnings.append("low_texture_template")

        search_raw, origin = self._search_region(normalized, target)
        if search_raw.size == 0:
            return None

        search_space = to_match_space(search_raw, target.mask)
        template_space = to_match_space(template_raw, target.mask)

        matches, strategies_tried = self._run_strategies(target, search_space, template_space)
        if not matches:
            return None

        scored = self._verify(target, matches, search_raw, template_raw, warnings)
        chosen = self._select(target, scored, search_raw, target_id)
        if chosen is None:
            return None

        box = normalized.rect_to_logical(
            origin[0] + chosen.x,
            origin[1] + chosen.y,
            chosen.w,
            chosen.h,
        )
        point = point_in_box(target.anchor, box)
        point = (point[0] + offset[0] * box.w, point[1] + offset[1] * box.h)

        if chosen.score < target.threshold:
            warnings.append("below_threshold")

        hit = Hit(
            box=box,
            point=point,
            score=float(chosen.score),
            strategy=strategies_tried,
            elapsed_ms=(time.perf_counter() - started) * 1000.0,
            warnings=warnings,
        )
        self.tracker.remember(target_id, frame_hash, hit.box, hit.score, hit.point)
        return hit

    def _search_region(self, normalized: Normalized, target: TemplateTarget) -> tuple[np.ndarray, tuple[int, int]]:
        x, y, w, h = roi_to_pixels(target.roi, normalized.width, normalized.height)
        return normalized.image[y : y + h, x : x + w], (x, y)

    def _run_strategies(self, target: TemplateTarget, search: np.ndarray, template: np.ndarray) -> tuple[list[Match], str]:
        tried: list[str] = []
        matches: list[Match] = []

        for strategy in target.strategy:
            tried.append(strategy)
            if strategy == "tpl":
                matches = single_scale_match(search, template, max(0.4, target.warnThreshold))
            elif strategy == "mstpl":
                matches = multi_scale_match(
                    search,
                    template,
                    scale_ladder(target.scale.model_dump() if hasattr(target.scale, "model_dump") else dict(target.scale)),
                    max(0.4, target.warnThreshold),
                )
            elif strategy == "sift":
                matches = keypoint.feature_match(search, template, threshold=0.5)
            if matches:
                break

        return matches, "+".join(tried)

    def _verify(
        self,
        target: TemplateTarget,
        matches: list[Match],
        search_raw: np.ndarray,
        template_raw: np.ndarray,
        warnings: list[str],
    ) -> list[Match]:
        if target.verify != "rgb":
            return matches

        kept: list[Match] = []
        for match in matches:
            x0 = max(0, int(round(match.x)))
            y0 = max(0, int(round(match.y)))
            x1 = min(search_raw.shape[1], int(round(match.x + match.w)))
            y1 = min(search_raw.shape[0], int(round(match.y + match.h)))
            patch = search_raw[y0:y1, x0:x1]
            if patch.size == 0:
                continue

            resized_template = template_raw
            if abs(match.scale - 1.0) > 0.01:
                resized_template = cv2.resize(
                    template_raw,
                    (max(4, int(round(template_raw.shape[1] * match.scale))), max(4, int(round(template_raw.shape[0] * match.scale)))),
                    interpolation=cv2.INTER_AREA if match.scale < 1.0 else cv2.INTER_LINEAR,
                )

            similarity = color_similarity(patch, resized_template)
            if similarity >= MIN_SIMILARITY:
                kept.append(match)
            else:
                warnings.append(f"color_mismatch:{similarity:.2f}")

        if kept:
            return kept

        # Nothing survived the colour check. Returning the geometric best with a
        # warning beats returning nothing: the runtime can still report a
        # low-confidence match instead of a hard failure.
        if matches:
            warnings.append("color_check_failed")
            return matches[:1]
        return []

    def _select(self, target: TemplateTarget, matches: list[Match], search: np.ndarray, target_id: str) -> Match | None:
        if not matches:
            return None

        ordered = sorted(matches, key=lambda match: match.score, reverse=True)

        if target.multi == "highest":
            return ordered[0]
        if target.multi == "nth" and target.nth:
            index = min(len(ordered), max(1, target.nth)) - 1
            return ordered[index]
        if target.multi == "center":
            cx, cy = search.shape[1] / 2.0, search.shape[0] / 2.0
            return min(ordered, key=lambda match: (match.center[0] - cx) ** 2 + (match.center[1] - cy) ** 2)

        # nearest-last: prefer the control the script clicked last time.
        last = self.tracker.last_box(target_id)
        if last is not None:
            return min(
                ordered,
                key=lambda match: (match.center[0] - last.center[0]) ** 2 + (match.center[1] - last.center[1]) ** 2,
            )
        return ordered[0]

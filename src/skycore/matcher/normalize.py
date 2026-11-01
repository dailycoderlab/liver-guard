"""Logical-size normalization: the trick that makes one crop work everywhere.

A control rendered on a 3x phone and on a 2x phone is a different number of
*pixels*, so pixel sized templates never match across devices. Normalizing both
the frame and the template to a fixed logical width removes pixel density from
the problem, leaving only genuine layout scale differences for the multi-scale
search to absorb.

With a canonical width ``L``:

* frame pixels -> normalized: factor ``L / (bounds.w * frame.scale)``
* template pixels (recorded at ``recordScale``) -> normalized: factor
  ``L / (bounds.w * recordScale)``

If the runtime device has the same density as the recorder the two factors are
equal and the template matches at scale 1.0.
"""

from __future__ import annotations

from dataclasses import dataclass

import cv2
import numpy as np

from ..geometry import Bounds

CANONICAL_WIDTH = 480.0


@dataclass
class Normalized:
    """A frame resized into canonical logical space."""

    image: np.ndarray
    width: int
    height: int
    factor: float
    bounds: Bounds

    def to_logical(self, nx: float, ny: float) -> tuple[float, float]:
        """Normalized pixel -> global logical points."""
        return (
            self.bounds.x + (nx / self.width) * self.bounds.w,
            self.bounds.y + (ny / self.height) * self.bounds.h,
        )

    def rect_to_logical(self, x: float, y: float, w: float, h: float):
        x0, y0 = self.to_logical(x, y)
        x1, y1 = self.to_logical(x + w, y + h)
        from ..geometry import Rect

        return Rect(x0, y0, x1 - x0, y1 - y0)


def normalize_frame(image: np.ndarray, bounds: Bounds, canonical_width: float = CANONICAL_WIDTH) -> Normalized:
    height, width = image.shape[:2]
    if width <= 0 or height <= 0:
        raise ValueError("empty frame")
    factor = canonical_width / float(width)
    new_w = int(round(width * factor))
    new_h = max(1, int(round(height * factor)))
    interpolation = cv2.INTER_AREA if factor < 1.0 else cv2.INTER_LINEAR
    resized = cv2.resize(image, (new_w, new_h), interpolation=interpolation)
    return Normalized(image=resized, width=new_w, height=new_h, factor=factor, bounds=bounds)


def template_factor(
    record_width: float,
    record_scale: float,
    fallback_width: float = 0.0,
    canonical_width: float = CANONICAL_WIDTH,
) -> float:
    """Pixels per normalized pixel for a template.

    Derived from the *recording surface's pixel width*, not from the current one.
    Both the template and the live frame end up normalized by their own surface
    width, so a control that kept its relative size lands at scale 1.0 no matter
    how big either screen was. Using the current width here instead would make a
    proportionally scaled UI look like a 1.5x zoom bug.
    """
    width = record_width if record_width > 0 else fallback_width
    pixel_width = width * max(record_scale, 0.01)
    if pixel_width <= 0:
        return 1.0
    return canonical_width / pixel_width


def resize_template(template: np.ndarray, factor: float) -> np.ndarray:
    height, width = template.shape[:2]
    new_w = max(4, int(round(width * factor)))
    new_h = max(4, int(round(height * factor)))
    if new_w == width and new_h == height:
        return template
    interpolation = cv2.INTER_AREA if factor < 1.0 else cv2.INTER_LINEAR
    return cv2.resize(template, (new_w, new_h), interpolation=interpolation)

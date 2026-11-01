"""A captured frame plus the metadata needed to map it back to the screen."""

from __future__ import annotations

import time
from dataclasses import dataclass, field

import cv2
import numpy as np

from .geometry import Bounds, Rect


@dataclass
class Frame:
    """One screenshot of a capture surface.

    ``image`` is BGR (OpenCV order), always at device pixel resolution. The
    mapping back to logical points is ``pixels / bounds.scale + bounds.origin``.
    """

    image: np.ndarray
    bounds: Bounds
    seq: int = 0
    ts: float = field(default_factory=time.time)
    """How long the capture itself took, in milliseconds.

    Worth tracking because macOS *stalls* instead of failing when a process has
    lost screen recording permission: the grab blocks for roughly 30 seconds and
    then hands back a blank surface. A run that silently polls for 30 seconds per
    frame is far worse than one that says "capture stalled".
    """
    capture_ms: float = 0.0
    _hash: str | None = None

    @property
    def width(self) -> int:
        return int(self.image.shape[1])

    @property
    def height(self) -> int:
        return int(self.image.shape[0])

    @property
    def scale(self) -> float:
        """Pixels per logical point, derived from the real image so a wrong
        reported scale can never silently corrupt matching."""
        if self.bounds.w <= 0:
            return self.bounds.scale
        return self.width / self.bounds.w

    def logical_rect(self) -> Rect:
        return Rect(self.bounds.x, self.bounds.y, self.bounds.w, self.bounds.h)

    def crop_logical(self, rect: Rect) -> np.ndarray:
        """Crop by logical global points."""
        scale = self.scale
        x0 = int(round((rect.x - self.bounds.x) * scale))
        y0 = int(round((rect.y - self.bounds.y) * scale))
        x1 = int(round((rect.x + rect.w - self.bounds.x) * scale))
        y1 = int(round((rect.y + rect.h - self.bounds.y) * scale))
        x0 = max(0, min(self.width - 1, x0))
        y0 = max(0, min(self.height - 1, y0))
        x1 = max(x0 + 1, min(self.width, x1))
        y1 = max(y0 + 1, min(self.height, y1))
        return self.image[y0:y1, x0:x1].copy()

    def hash(self) -> str:
        """Cheap perceptual hash used to skip matching on a static screen.

        A 16x16 mean-threshold hash: stable across tiny dithering/compression
        differences, changes as soon as the screen actually changes.
        """
        if self._hash is not None:
            return self._hash
        small = cv2.resize(self.image, (16, 16), interpolation=cv2.INTER_AREA)
        gray = cv2.cvtColor(small, cv2.COLOR_BGR2GRAY)
        bits = gray > gray.mean()
        self._hash = "".join("1" if bit else "0" for bit in bits.flatten())
        return self._hash

    def content_std(self) -> float:
        """Standard deviation of the pixels.

        Without screen recording permission macOS hands back a flat surface
        instead of failing, so this is how ``doctor`` tells "capturing" from
        "capturing a black rectangle".
        """
        return float(np.std(cv2.resize(self.image, (64, 64), interpolation=cv2.INTER_AREA)))

    def to_png(self) -> bytes:
        ok, buffer = cv2.imencode(".png", self.image)
        if not ok:
            raise RuntimeError("png encode failed")
        return buffer.tobytes()

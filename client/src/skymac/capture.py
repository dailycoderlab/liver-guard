"""Screen capture through CoreGraphics (mss).

mss grabs a rectangle in *global logical points* and returns device *pixels*, so
``Frame.scale`` (pixels per point) comes out of the real image rather than from a
configuration value. On a 2x display that factor is 2 and everything downstream
- matching, cropping, the search window - keeps working unchanged.

Note: this captures a screen region, not a window. Occluded windows therefore
capture whatever is on top of them; the recorder activates the target window
before capturing. True per window capture needs ScreenCaptureKit, which is a
drop-in replacement behind this interface later.
"""

from __future__ import annotations

import threading

import cv2
import numpy as np
from skycore.frame import Frame
from skycore.geometry import Bounds, Rect


class MssCapturer:
    def __init__(self) -> None:
        self._local = threading.local()

    def _session(self):
        existing = getattr(self._local, "session", None)
        if existing is None:
            import mss

            existing = mss.MSS()
            self._local.session = existing
        return existing

    def close(self) -> None:
        session = getattr(self._local, "session", None)
        if session is not None:
            try:
                session.close()
            except Exception:
                pass
            self._local.session = None

    def grab(self, rect: Rect) -> np.ndarray:
        """Capture a logical-point rectangle; returns BGR pixels."""
        if rect.w < 1 or rect.h < 1:
            raise ValueError("capture rectangle is empty")

        monitor = {
            "left": int(round(rect.x)),
            "top": int(round(rect.y)),
            "width": max(1, int(round(rect.w))),
            "height": max(1, int(round(rect.h))),
        }
        shot = self._session().grab(monitor)
        raw = np.frombuffer(shot.bgra, dtype=np.uint8)
        bgra = raw.reshape(shot.height, shot.width, 4)
        # BGRA -> BGR without an extra copy of the alpha plane.
        return bgra[:, :, :3].copy()

    def frame(self, bounds: Bounds, seq: int = 0) -> Frame:
        image = self.grab(bounds.rect)
        return Frame(image=image, bounds=bounds, seq=seq)


def downscale_if_needed(image: np.ndarray, max_width: int = 1280) -> np.ndarray:
    if image.shape[1] <= max_width:
        return image
    ratio = max_width / image.shape[1]
    return cv2.resize(image, (max_width, int(round(image.shape[0] * ratio))), interpolation=cv2.INTER_AREA)

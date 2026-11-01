"""macOS screen-region and individual-window capture.

Screen capture uses mss. Selected windows use CoreGraphics' window-list
capture so the preview does not include other windows covering the target.
Image sizes are measured instead of assuming a Retina scaling factor.
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

    def grab_window(self, rect: Rect, window_number: int) -> np.ndarray:
        """Capture only the selected window within global logical bounds."""
        if rect.w < 1 or rect.h < 1:
            raise ValueError("capture rectangle is empty")
        import Quartz as q

        options = (q.kCGWindowImageBoundsIgnoreFraming
                   | q.kCGWindowImageShouldBeOpaque
                   | q.kCGWindowImageNominalResolution)
        image = q.CGWindowListCreateImage(
            q.CGRectMake(rect.x, rect.y, rect.w, rect.h),
            q.kCGWindowListOptionIncludingWindow, window_number, options,
        )
        if image is None:
            raise RuntimeError('无法截取目标窗口，请检查屏幕录制权限及窗口状态')
        width = q.CGImageGetWidth(image)
        height = q.CGImageGetHeight(image)
        stride = q.CGImageGetBytesPerRow(image)
        if not width or not height or q.CGImageGetBitsPerPixel(image) != 32 or stride < width * 4:
            raise RuntimeError('目标窗口截图格式无效')
        data = q.CGDataProviderCopyData(q.CGImageGetDataProvider(image))
        if data is None:
            raise RuntimeError('无法读取目标窗口截图')
        raw = np.frombuffer(data, dtype=np.uint8)
        if raw.size < stride * height:
            raise RuntimeError('目标窗口截图数据不完整')
        rows = raw[:stride * height].reshape(height, stride)
        bgra = rows[:, :width * 4].reshape(height, width, 4)
        return bgra[:, :, :3].copy()

    def frame(self, bounds: Bounds, seq: int = 0) -> Frame:
        image = self.grab(bounds.rect)
        return Frame(image=image, bounds=bounds, seq=seq)


def downscale_if_needed(image: np.ndarray, max_width: int = 1280) -> np.ndarray:
    if image.shape[1] <= max_width:
        return image
    ratio = max_width / image.shape[1]
    return cv2.resize(image, (max_width, int(round(image.shape[0] * ratio))), interpolation=cv2.INTER_AREA)

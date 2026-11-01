"""Capture a physical screen rectangle with mss, returning BGR pixels."""
import threading

import numpy as np


class MssCapturer:
    def __init__(self):
        self._local = threading.local()

    def grab(self, rect):
        session = getattr(self._local, 'session', None)
        if session is None:
            import mss
            session = mss.MSS()
            self._local.session = session
        shot = session.grab({'left': int(rect.x), 'top': int(rect.y),
                             'width': int(rect.w), 'height': int(rect.h)})
        return np.frombuffer(shot.bgra, dtype=np.uint8).reshape(shot.height, shot.width, 4)[:, :, :3].copy()

    def close(self):
        session = getattr(self._local, 'session', None)
        if session is not None:
            session.close()
            self._local.session = None

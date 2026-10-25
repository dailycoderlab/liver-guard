"""Image matching: locate a recorded control inside a live frame.

Approach follows Airtest's ``aircv`` (Apache-2.0) in spirit - numpy/OpenCV
``matchTemplate`` with ``TM_CCOEFF_NORMED``, a colour cross check, and a keypoint
fallback - but the code here is independent, because this engine needs things
Airtest's ``Template`` does not express:

* logical-size normalization so one crop works on any pixel density,
* a per-target search region instead of full-frame matching,
* a continuous anchor instead of nine preset click positions,
* coarse-to-fine pyramid search plus frame-hash reuse for low latency.
"""

from .assets import AssetStore
from .engine import Hit, MatcherEngine

__all__ = ["AssetStore", "Hit", "MatcherEngine"]

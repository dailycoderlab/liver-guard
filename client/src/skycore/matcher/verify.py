"""Second stage colour check.

Grayscale correlation happily matches "the same shape, different colour" - a
greyed out button against an active one is the classic false positive. Comparing
mean-removed colour vectors at low resolution is cheap and kills most of them.

This mirrors Airtest's ``rgb=True`` path (``cal_rgb_confidence``), reimplemented
so it can run on a normalised patch without extra dependencies.
"""

from __future__ import annotations

import cv2
import numpy as np

SAMPLE = 16
MIN_SIMILARITY = 0.45


def color_similarity(patch: np.ndarray, template: np.ndarray) -> float:
    if patch.size == 0 or template.size == 0:
        return 0.0
    try:
        a = cv2.resize(patch, (SAMPLE, SAMPLE), interpolation=cv2.INTER_AREA).astype(np.float32)
        b = cv2.resize(template, (SAMPLE, SAMPLE), interpolation=cv2.INTER_AREA).astype(np.float32)
    except cv2.error:
        return 0.0

    if a.ndim == 2:
        a = a[:, :, None]
    if b.ndim == 2:
        b = b[:, :, None]

    va = a.reshape(-1)
    vb = b.reshape(-1)
    va = va - va.mean()
    vb = vb - vb.mean()

    denom = float(np.linalg.norm(va) * np.linalg.norm(vb))
    if denom < 1e-6:
        # Both patches are flat: fall back to comparing brightness only.
        return 1.0 if abs(float(a.mean()) - float(b.mean())) < 12 else 0.0

    correlation = float(np.dot(va, vb) / denom)
    return max(0.0, min(1.0, correlation))

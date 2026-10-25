"""Keypoint fallback.

Used when template correlation fails: scale drift, a reskinned control, slight
rotation. Slower by an order of magnitude, so it is only ever tried after the
template strategies miss, and still inside the target's search region.
"""

from __future__ import annotations

import cv2
import numpy as np

from .tpl import Match

MAX_FEATURES = 400
MIN_GOOD = 8
RATIO = 0.75


def available() -> bool:
    return hasattr(cv2, "SIFT_create")


def _prepare(image: np.ndarray) -> np.ndarray:
    gray = image if image.ndim == 2 else cv2.cvtColor(image, cv2.COLOR_BGR2GRAY)
    height, width = gray.shape[:2]
    if max(height, width) > 900:
        factor = 900.0 / max(height, width)
        gray = cv2.resize(gray, (int(width * factor), int(height * factor)), interpolation=cv2.INTER_AREA)
    return gray


def feature_match(search: np.ndarray, template: np.ndarray, threshold: float = 0.7) -> list[Match]:
    if not available() or search.size == 0 or template.size == 0:
        return []

    try:
        detector = cv2.SIFT_create(nfeatures=MAX_FEATURES)
    except cv2.error:
        return []

    search_gray = _prepare(search)
    template_gray = _prepare(template)
    if min(template_gray.shape[:2]) < 16:
        return []

    kp1, des1 = detector.detectAndCompute(template_gray, None)
    kp2, des2 = detector.detectAndCompute(search_gray, None)
    if des1 is None or des2 is None or len(kp1) < MIN_GOOD or len(kp2) < MIN_GOOD:
        return []

    matcher = cv2.BFMatcher(cv2.NORM_L2)
    try:
        knn = matcher.knnMatch(des1, des2, k=2)
    except cv2.error:
        return []

    good = [pair[0] for pair in knn if len(pair) == 2 and pair[0].distance < RATIO * pair[1].distance]
    if len(good) < MIN_GOOD:
        return []

    src = np.float32([kp1[m.queryIdx].pt for m in good]).reshape(-1, 1, 2)
    dst = np.float32([kp2[m.trainIdx].pt for m in good]).reshape(-1, 1, 2)

    homography, mask = cv2.findHomography(src, dst, cv2.RANSAC, 5.0)
    if homography is None or mask is None:
        return []

    inliers = int(mask.sum())
    if inliers < MIN_GOOD:
        return []

    th, tw = template_gray.shape[:2]
    corners = np.float32([[0, 0], [tw, 0], [tw, th], [0, th]]).reshape(-1, 1, 2)
    projected = cv2.perspectiveTransform(corners, homography).reshape(-1, 2)
    x0, y0 = projected.min(axis=0)
    x1, y1 = projected.max(axis=0)

    scale = (x1 - x0) / max(tw, 1)
    confidence = min(0.99, inliers / max(len(good), 1))
    if confidence < threshold:
        return []

    return [Match(float(x0), float(y0), float(x1 - x0), float(y1 - y0), float(confidence), float(scale))]

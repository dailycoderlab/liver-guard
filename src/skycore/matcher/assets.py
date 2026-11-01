"""Template asset store, content addressed by sha256."""

from __future__ import annotations

import hashlib
from pathlib import Path

import cv2
import numpy as np

from .normalize import resize_template


class AssetStore:
    """Keeps decoded templates in memory, keyed by hash and resize factor.

    Templates are small (tens of KB) so the cache stays tiny, and caching the
    resized variant matters: normalization resizes on every match otherwise.
    Assets are local files only; a missing hash is an error, never a download.
    """

    def __init__(self, cache_dir: Path | None = None) -> None:
        self.cache_dir = cache_dir
        self._raw: dict[str, np.ndarray] = {}
        self._resized: dict[tuple[str, int], np.ndarray] = {}
        if self.cache_dir:
            self.cache_dir.mkdir(parents=True, exist_ok=True)

    # -------------------------------------------------------------- loading

    def put(self, sha: str, image: np.ndarray) -> None:
        self._raw[sha] = image
        if self.cache_dir:
            path = self.cache_dir / f"{sha}.png"
            if not path.exists():
                cv2.imwrite(str(path), image)

    def put_bytes(self, sha: str, payload: bytes) -> None:
        array = np.frombuffer(payload, dtype=np.uint8)
        image = cv2.imdecode(array, cv2.IMREAD_COLOR)
        if image is None:
            raise ValueError(f"asset {sha[:8]} is not a decodable image")
        self.put(sha, image)

    def has(self, sha: str) -> bool:
        if sha in self._raw:
            return True
        return bool(self.cache_dir and (self.cache_dir / f"{sha}.png").exists())

    def get(self, sha: str) -> np.ndarray:
        if sha in self._raw:
            return self._raw[sha]

        if self.cache_dir:
            path = self.cache_dir / f"{sha}.png"
            if path.exists():
                image = cv2.imread(str(path), cv2.IMREAD_COLOR)
                if image is not None:
                    self._raw[sha] = image
                    return image

        raise KeyError(f"asset {sha[:8]} is not in the local asset directory")

    def get_scaled(self, sha: str, factor: float) -> np.ndarray:
        key = (sha, int(round(factor * 1000)))
        cached = self._resized.get(key)
        if cached is not None:
            return cached
        scaled = resize_template(self.get(sha), factor)
        self._resized[key] = scaled
        return scaled

    def clear_resized(self) -> None:
        self._resized.clear()

    # ------------------------------------------------------------- integrity

    @staticmethod
    def hash_bytes(payload: bytes) -> str:
        return hashlib.sha256(payload).hexdigest()

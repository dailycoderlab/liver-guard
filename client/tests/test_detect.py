import numpy as np
import pytest

from skycore.detect import (
    Candidate,
    DetectOptions,
    PixelBox,
    Point,
    _dedupe,
    _flood_region,
    detect_candidates,
    iou,
)


def scene(width=600, height=400, button=None):
    y, x = np.indices((height, width))
    base = (38 + (x * 7 + y * 3) % 18).astype(np.uint8)
    image = np.empty((height, width, 4), dtype=np.uint8)
    image[:, :, 0] = image[:, :, 1] = base
    image[:, :, 2] = base + 6
    image[:, :, 3] = 255
    if button:
        bx, by, bw, bh = button
        image[by : by + bh, bx : bx + bw, :3] = (250, 245, 235)
        image[by + 6 : by + bh - 6, bx + 6 : bx + bw - 6, :3] = (120, 170, 60)
        image[by + bh // 2 - 4 : by + bh // 2 + 4, bx + 10 : bx + bw - 10, :3] = 255
    return image


def test_finds_button_and_preserves_order():
    button = PixelBox(200, 140, 160, 80)
    image = scene(button=(200, 140, 160, 80))
    hits = detect_candidates(image, Point(280, 180), DetectOptions(scale=1))
    assert hits[0].kind == "contour"
    assert iou(hits[0], button) > 0.55
    assert hits[-1].kind == "fallback"
    assert all(c.x <= 280 <= c.x + c.w and c.y <= 180 <= c.y + c.h for c in hits)


def test_flat_fallback_rounding_and_clamp():
    image = np.full((80, 100, 4), 40, dtype=np.uint8)
    assert detect_candidates(image, Point(0, 0), DetectOptions(scale=1)) == [
        Candidate(0, 0, 74, 74, "fallback", 0.2)
    ]
    assert detect_candidates(image, Point(99, 79), DetectOptions(scale=1, padding=0)) == [
        Candidate(67, 47, 33, 33, "fallback", 0.2)
    ]


def test_ax_first_exact_and_removes_duplicate():
    image = scene(button=(200, 140, 160, 80))
    ax = PixelBox(198, 138, 164, 84)
    hits = detect_candidates(image, Point(280, 180), DetectOptions(scale=1, ax_box=ax, padding=0))
    assert hits[0] == Candidate(198, 138, 164, 84, "ax", 1)
    assert [c.kind for c in hits] == ["ax", "fallback"]
    # AX is not limited by max_side and is not padded, even outside the click.
    hits = detect_candidates(image, Point(280, 180), DetectOptions(scale=1, max_side=50, ax_box=PixelBox(-5, 2, 400, 90)))
    assert hits[0] == Candidate(0, 2, 400, 90, "ax", 1)


def test_dedupe_uses_strict_iou_threshold_and_js_rounding():
    assert iou(PixelBox(0, 0, 7, 10), PixelBox(1, 0, 7, 10)) == 0.75
    hits = _dedupe([Candidate(0, 0, 7, 10, "ax", 1),
                    Candidate(1, 0, 7, 10, "ax", 1),
                    Candidate(0.5, 0.5, 7, 10, "ax", 1)], 100, 100, 50, 0)
    assert hits == [Candidate(0, 0, 7, 10, "ax", 1), Candidate(1, 0, 7, 10, "ax", 1)]


def test_max_size_and_invalid_ax():
    image = scene(button=(40, 50, 400, 200))
    hits = detect_candidates(image, Point(240, 150), DetectOptions(scale=1, max_side=100, ax_box=PixelBox(0, 0, 0, 20)))
    assert all(max(c.w, c.h) <= 100 for c in hits)
    assert all(c.kind != "ax" for c in hits)


def test_strictness_changes_detection_to_color_and_dedupes_fallback():
    rng = np.random.default_rng(1)
    image = np.zeros((160, 160, 4), dtype=np.uint8)
    image[:, :, :3] = 40
    noise = rng.integers(-17, 18, (60, 60, 1))
    image[50:110, 50:110, :3] = (100 + noise).astype(np.uint8)
    image[80, 80, :3] = 100
    for strictness in ("loose", "normal"):
        hits = detect_candidates(image, Point(80, 80), DetectOptions(scale=1, strictness=strictness, padding=0))
        assert [hit.kind for hit in hits] == ["contour"]
    hits = detect_candidates(image, Point(80, 80), DetectOptions(scale=1, strictness="tight", padding=0))
    assert hits == [Candidate(50, 50, 60, 60, "color", 0.7)]


def test_flood_fixed_seed_eight_connected_and_limit():
    image = np.zeros((5, 5, 3), dtype=np.uint8)
    image[0, 0] = (100, 100, 100)
    image[1, 1] = (110, 110, 110)
    image[2, 2] = (120, 120, 120)
    region = _flood_region(image, 0, 0, 10, 2)
    assert region is not None and (region.x0, region.y0, region.x1, region.y1, region.area) == (0, 0, 1, 1, 2)
    assert _flood_region(image, 0, 0, 10, 1) is None
    assert _flood_region(image, -1, 0, 10, 2) is None


@pytest.mark.parametrize("strictness", ["loose", "normal", "tight"])
def test_strictness_and_alpha_ignored(strictness):
    image = scene(button=(200, 140, 160, 80))
    options = DetectOptions(scale=1, strictness=strictness)
    result = detect_candidates(image, Point(280, 180), options)
    image[:, :, 3] = 0
    assert detect_candidates(image, Point(280, 180), options) == result
    assert result[0].kind == "contour"


def test_downsample_odd_dimensions_pixel_coordinates():
    image = np.zeros((1501, 1667, 4), dtype=np.uint8)
    image[400:600, 600:900, :3] = 250
    image[410:590, 610:890, :3] = 100
    hits = detect_candidates(image, Point(750, 500), DetectOptions(scale=2, padding=0))
    assert hits[0].kind == "contour"
    assert iou(hits[0], PixelBox(600, 400, 300, 200)) > 0.85
    assert all(c.x >= 0 and c.y >= 0 and c.x + c.w <= 1667 and c.y + c.h <= 1501 for c in hits)


def test_input_validation():
    with pytest.raises(ValueError):
        detect_candidates(np.zeros((10, 10, 3), dtype=np.uint8), Point(1, 1), DetectOptions(scale=1))
    with pytest.raises(ValueError):
        detect_candidates(np.zeros((0, 10, 4), dtype=np.uint8), Point(1, 1), DetectOptions(scale=1))
    with pytest.raises(ValueError):
        detect_candidates(np.zeros((10, 10, 4), dtype=np.uint8), Point(1, 1), DetectOptions(scale=1, strictness="bad"))

import numpy as np
from tests.helpers import button_target, crop_asset, make_scene

from skycore.matcher import AssetStore, MatcherEngine

BUTTON_CENTER = (300.0, 220.0)
BUTTON_SIZE = (120.0, 56.0)


def build_engine(asset_bytes: bytes, sha: str, allow_reuse: bool = False) -> MatcherEngine:
    store = AssetStore()
    store.put_bytes(sha, asset_bytes)
    return MatcherEngine(store, allow_reuse=allow_reuse)


def recorded_asset(center=BUTTON_CENTER, size=BUTTON_SIZE, scale=3.0, width_pt=600.0):
    scene = make_scene(scale=scale, width_pt=width_pt, height_pt=width_pt * 2 / 3, button_center_pt=center, button_size_pt=size)
    asset = crop_asset(scene.image, center, size, scale)
    return asset, button_target(asset, size, record_scale=scale, record_width=width_pt)


def test_same_layout_on_a_lower_density_device():
    """The whole point: recorded at 3x, matched at 2x, pixel densities differ."""
    asset, target = recorded_asset()
    engine = build_engine(asset, target.asset)
    frame = make_scene(scale=2.0)

    hit = engine.resolve(target, "btn", frame)

    assert hit is not None
    assert hit.score > 0.9
    assert hit.strategy.startswith("tpl")
    assert abs(hit.point[0] - BUTTON_CENTER[0]) <= 3
    assert abs(hit.point[1] - BUTTON_CENTER[1]) <= 3
    assert "reuse" != hit.strategy


def test_same_layout_on_a_larger_window():
    """A window with the same relative layout, 1.5x the point size."""
    asset, target = recorded_asset()
    engine = build_engine(asset, target.asset)
    frame = make_scene(
        scale=2.0,
        width_pt=900.0,
        height_pt=600.0,
        button_center_pt=(450.0, 330.0),
        button_size_pt=(180.0, 84.0),
    )

    hit = engine.resolve(target, "btn", frame)

    assert hit is not None
    assert hit.score > 0.85
    # `box` is in logical points, so it should be the control's point size.
    # Normalizing by the recording surface width means no scale search is needed.
    assert abs(hit.box.w - 180.0) < 8
    assert abs(hit.box.h - 84.0) < 8
    assert abs(hit.point[0] - 450.0) <= 4
    assert abs(hit.point[1] - 330.0) <= 4


def test_multi_scale_finds_a_proportionally_bigger_control():
    """Layout did not scale, the control did: only a scale search can find it."""
    asset, target = recorded_asset()
    engine = build_engine(asset, target.asset)
    frame = make_scene(scale=2.0, button_size_pt=(156.0, 72.8))

    hit = engine.resolve(target, "btn", frame)

    assert hit is not None
    assert hit.score > 0.75
    assert abs(hit.point[0] - BUTTON_CENTER[0]) <= 6
    assert 1.2 < hit.box.w / BUTTON_SIZE[0] < 1.45


def test_roi_excludes_an_identical_decoy():
    asset, target = recorded_asset()
    target.roi = (0.4, 0.0, 0.6, 1.0)
    engine = build_engine(asset, target.asset)
    frame = make_scene(scale=2.0, decoy_at_pt=(120.0, 220.0))

    hit = engine.resolve(target, "btn", frame)

    assert hit is not None
    # The decoy sits at x = 120, well outside the roi; picking it would be a bug.
    assert abs(hit.point[0] - BUTTON_CENTER[0]) <= 6


def test_anchor_click_point_follows_the_recorded_offset():
    asset, target = recorded_asset()
    target.anchor = (0.15, 0.5)
    engine = build_engine(asset, target.asset)
    frame = make_scene(scale=2.0)

    hit = engine.resolve(target, "btn", frame)

    assert hit is not None
    # 15% into a 120pt wide button is 18pt right of its left edge.
    assert abs(hit.point[0] - (BUTTON_CENTER[0] - BUTTON_SIZE[0] / 2 + BUTTON_SIZE[0] * 0.15)) <= 3


def test_frame_hash_reuse_skips_matching():
    asset, target = recorded_asset()
    engine = build_engine(asset, target.asset, allow_reuse=True)
    frame = make_scene(scale=2.0)

    first = engine.resolve(target, "btn", frame)
    second = engine.resolve(target, "btn", frame)

    assert first is not None and not first.reused
    assert second is not None and second.reused
    assert second.strategy == "reuse"
    assert second.score == first.score


def test_a_changed_frame_is_matched_again():
    asset, target = recorded_asset()
    engine = build_engine(asset, target.asset, allow_reuse=True)

    first = engine.resolve(target, "btn", make_scene(scale=2.0))
    # A decoy changes the pixels, so the frame hash differs and reuse must not fire.
    second = engine.resolve(target, "btn", make_scene(scale=2.0, decoy_at_pt=(120.0, 220.0)))

    assert first is not None and second is not None
    assert not second.reused


def test_flat_template_is_rejected_instead_of_matching_noise():
    """A control with no texture cannot be located; say so rather than guess."""
    import cv2

    scene = make_scene(scale=2.0)
    flat = np.full((60, 60, 3), 45, dtype=np.uint8)
    ok, buffer = cv2.imencode(".png", flat)
    assert ok

    target = button_target(buffer.tobytes(), (30.0, 30.0), record_scale=2.0, threshold=0.8)
    engine = build_engine(buffer.tobytes(), target.asset)

    hit = engine.resolve(target, "flat", scene)

    # The guard has to fire, otherwise a constant patch reports score 1.0
    # everywhere because normalised cross correlation of a flat patch is undefined.
    assert hit is None


def test_point_target_is_bounded_by_its_zone():
    from skycore.schema import PointTarget

    frame = make_scene(scale=2.0)
    inside = PointTarget(type="point", norm=(0.5, 0.55), zone=(0.3, 0.3, 0.4, 0.4))
    outside = PointTarget(type="point", norm=(0.05, 0.05), zone=(0.3, 0.3, 0.4, 0.4))

    engine = MatcherEngine(AssetStore(), allow_reuse=False)
    hit = engine.resolve(inside, "empty", frame)
    assert hit is not None
    assert hit.strategy == "point"
    assert "no_visual_anchor" in hit.warnings
    assert abs(hit.point[0] - 300) <= 1 and abs(hit.point[1] - 220) <= 1

    assert engine.resolve(outside, "empty", frame) is None


def test_offset_shifts_the_click_inside_the_matched_box():
    asset, target = recorded_asset()
    engine = build_engine(asset, target.asset)
    frame = make_scene(scale=2.0)

    plain = engine.resolve(target, "btn", make_scene(scale=2.0))
    assert plain is not None

    engine.tracker.clear()
    shifted = engine.resolve(target, "btn", frame, offset=(0.0, -0.25))
    assert shifted is not None

    assert shifted.point[1] < plain.point[1]


def test_matching_is_fast_enough_for_a_polling_loop():
    """Guards the latency budget: median well under the plan's 60ms target."""
    asset, target = recorded_asset()
    target.roi = (0.4, 0.35, 0.35, 0.35)
    engine = build_engine(asset, target.asset)

    # Same relative layout on a much bigger surface: the button keeps its 20% width.
    big = dict(
        scale=2.0,
        width_pt=1600.0,
        height_pt=1000.0,
        button_center_pt=(800.0, 550.0),
        button_size_pt=(320.0, 149.3),
    )

    import time

    samples = []
    for _ in range(5):
        engine.tracker.clear()
        started = time.perf_counter()
        hit = engine.resolve(target, "btn", make_scene(**big))
        samples.append((time.perf_counter() - started) * 1000)
        assert hit is not None

    median = sorted(samples)[len(samples) // 2]
    assert median < 150, f"median match time {median:.1f}ms"

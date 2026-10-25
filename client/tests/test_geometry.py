from skycore.geometry import (
    Bounds,
    Rect,
    anchor_in_box,
    global_to_normalized,
    infer_roi,
    normalized_to_global,
    point_in_box,
    roi_to_pixels,
    search_window,
)


def test_normalized_round_trip():
    bounds = Bounds(x=100, y=50, w=800, h=600, scale=2)
    x, y = normalized_to_global(0.25, 0.5, bounds)
    assert x == 300
    assert y == 350
    assert global_to_normalized(x, y, bounds) == (0.25, 0.5)


def test_anchor_keeps_relative_position_inside_the_control():
    box = Rect(200, 100, 400, 100)
    # Clicked a fifth of the way across a wide button.
    anchor = anchor_in_box((280, 150), box)
    assert anchor == (0.2, 0.5)
    point = point_in_box(anchor, box)
    assert point == (280, 150)

    # The same ratio on a control twice as wide still lands at 20% across.
    wider = Rect(0, 0, 800, 100)
    assert point_in_box(anchor, wider) == (160, 50)


def test_anchor_is_clamped_away_from_the_edges():
    box = Rect(0, 0, 100, 100)
    assert anchor_in_box((0, 0), box) == (0.1, 0.1)
    assert anchor_in_box((100, 100), box) == (0.9, 0.9)


def test_search_window_is_centred_and_clamped_inside_the_surface():
    bounds = Bounds(x=0, y=0, w=600, h=400, scale=2)
    window = search_window((300, 200), (600, 400), bounds)
    assert window.as_tuple() == (0.0, 0.0, 600.0, 400.0)

    near_edge = search_window((10, 10), (300, 200), bounds)
    assert near_edge.x == 0 and near_edge.y == 0
    assert near_edge.w == 300 and near_edge.h == 200


def test_infer_roi_covers_the_grid_cell_with_a_ring():
    bounds = Bounds(x=0, y=0, w=600, h=400, scale=2)
    # Bottom middle cell (row 4 of 5, column 1 of 3).
    box = Rect(250, 380, 40, 20)
    roi = infer_roi(box, bounds)
    x, y, w, h = roi
    assert 0.0 <= x < 0.35 and x + w <= 1.0
    assert y >= 0.5 and y + h <= 1.0
    # The box centre must fall inside its own roi.
    cx = (box.x + box.w / 2) / bounds.w
    cy = (box.y + box.h / 2) / bounds.h
    assert x <= cx <= x + w
    assert y <= cy <= y + h


def test_infer_roi_corner_still_contains_the_control():
    """A control in the very corner must be inside its own search region."""
    bounds = Bounds(x=0, y=0, w=600, h=400, scale=1)
    box = Rect(0, 0, 10, 10)
    roi = infer_roi(box, bounds)
    assert roi[0] == 0.0
    assert roi[1] == 0.0
    cx = (box.x + box.w / 2) / bounds.w
    cy = (box.y + box.h / 2) / bounds.h
    assert roi[0] <= cx <= roi[0] + roi[2]
    assert roi[1] <= cy <= roi[1] + roi[3]

    bottom = infer_roi(Rect(590, 390, 10, 10), bounds)
    bcy = 395 / 400
    assert bottom[1] <= bcy <= bottom[1] + bottom[3]


def test_roi_to_pixels_stays_inside_the_image():
    x, y, w, h = roi_to_pixels((0.9, 0.9, 0.5, 0.5), 100, 100)
    assert x + w <= 100
    assert y + h <= 100
    assert w >= 1 and h >= 1


def test_iou_and_intersection():
    a = Rect(0, 0, 10, 10)
    b = Rect(5, 5, 10, 10)
    assert a.intersection(b).as_tuple() == (5.0, 5.0, 5.0, 5.0)
    assert round(a.iou(b), 4) == round(25 / 175, 4)
    assert a.iou(Rect(100, 100, 5, 5)) == 0.0


def test_contains_uses_inclusive_edges():
    box = Rect(10, 10, 10, 10)
    assert box.contains(10, 10)
    assert box.contains(20, 20)
    assert not box.contains(20.5, 20)

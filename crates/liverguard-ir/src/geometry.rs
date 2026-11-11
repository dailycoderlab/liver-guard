//! Coordinate maths for capture surfaces.
//!
//! Three coordinate spaces exist and mixing them up is the classic "the click
//! landed 40 pixels away" bug:
//!
//! 1. **normalized** - `[0, 1]` relative to the captured image. Scripts store this.
//! 2. **logical points** - what OS input APIs want; `Bounds` is expressed in it.
//! 3. **device pixels** - what screenshots and matching use.
//!
//! 2 <-> 3 is a single scalar, `Bounds::scale` (pixels per point).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Python's `round()` rounds half to even; keep that so pixel maths matches the
/// reference implementation the fixtures were generated from.
#[inline]
pub fn py_round(value: f64) -> f64 {
    value.round_ties_even()
}

#[inline]
pub fn round_to(value: f64, digits: i32) -> f64 {
    let factor = 10f64.powi(digits);
    py_round(value * factor) / factor
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        self.x <= px && px <= self.right() && self.y <= py && py <= self.bottom()
    }

    pub fn intersection(&self, other: &Rect) -> Rect {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        if x1 <= x0 || y1 <= y0 {
            return Rect::new(x0, y0, 0.0, 0.0);
        }
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn iou(&self, other: &Rect) -> f64 {
        let inter = self.intersection(other);
        let area = inter.w * inter.h;
        if area <= 0.0 {
            return 0.0;
        }
        let union = self.w * self.h + other.w * other.h - area;
        if union > 0.0 {
            area / union
        } else {
            0.0
        }
    }

    pub fn as_array(&self) -> [f64; 4] {
        [self.x, self.y, self.w, self.h]
    }
}

/// A capture surface in logical points, in global screen coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    #[serde(default = "one")]
    pub scale: f64,
}

fn one() -> f64 {
    1.0
}

impl Bounds {
    pub const fn new(x: f64, y: f64, w: f64, h: f64, scale: f64) -> Self {
        Self { x, y, w, h, scale }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x, self.y, self.w, self.h)
    }

    pub fn close_to(&self, other: &Bounds, tolerance: f64) -> bool {
        (self.x - other.x).abs() <= tolerance
            && (self.y - other.y).abs() <= tolerance
            && (self.w - other.w).abs() <= tolerance
            && (self.h - other.h).abs() <= tolerance
    }
}

pub fn clamp(value: f64, low: f64, high: f64) -> f64 {
    // Same order as the reference `max(low, min(high, value))`: `low` wins when
    // the range is inverted.
    low.max(high.min(value))
}

pub fn normalized_to_global(nx: f64, ny: f64, bounds: &Bounds) -> (f64, f64) {
    (bounds.x + nx * bounds.w, bounds.y + ny * bounds.h)
}

pub fn global_to_normalized(px: f64, py: f64, bounds: &Bounds) -> (f64, f64) {
    if bounds.w <= 0.0 || bounds.h <= 0.0 {
        return (0.5, 0.5);
    }
    ((px - bounds.x) / bounds.w, (py - bounds.y) / bounds.h)
}

/// Where inside a matched box to click, as a ratio in `[0.1, 0.9]`. A ratio
/// rather than a centre point keeps a click on the left third of a long button
/// on the left third after the screen gets bigger.
pub fn anchor_in_box(click: (f64, f64), rect: &Rect) -> (f64, f64) {
    let ax = if rect.w > 0.0 {
        (click.0 - rect.x) / rect.w
    } else {
        0.5
    };
    let ay = if rect.h > 0.0 {
        (click.1 - rect.y) / rect.h
    } else {
        0.5
    };
    (clamp(ax, 0.1, 0.9), clamp(ay, 0.1, 0.9))
}

pub fn point_in_box(anchor: (f64, f64), rect: &Rect) -> (f64, f64) {
    (rect.x + anchor.0 * rect.w, rect.y + anchor.1 * rect.h)
}

/// A window of `size` logical points centred on the click, clamped inside the
/// capture surface.
pub fn search_window(click: (f64, f64), size: (f64, f64), bounds: &Bounds) -> Rect {
    let w = size.0.min(bounds.w);
    let h = size.1.min(bounds.h);
    let x = clamp(click.0 - w / 2.0, bounds.x, bounds.x + bounds.w - w);
    let y = clamp(click.1 - h / 2.0, bounds.y, bounds.y + bounds.h - h);
    Rect::new(x, y, w, h)
}

/// Search region for a target: the grid cell it sits in, plus one ring, as a
/// normalized rectangle. The matched box is always folded in so a control on the
/// very edge of the surface stays inside its own search region.
pub fn infer_roi(rect: &Rect, bounds: &Bounds, cols: usize, rows: usize) -> [f64; 4] {
    if bounds.w <= 0.0 || bounds.h <= 0.0 {
        return [0.0, 0.0, 1.0, 1.0];
    }
    let (cx, cy) = rect.center();
    let nx = clamp((cx - bounds.x) / bounds.w, 0.0, 1.0);
    let ny = clamp((cy - bounds.y) / bounds.h, 0.0, 1.0);

    let col = (cols - 1).min((nx * cols as f64) as usize);
    let row = (rows - 1).min((ny * rows as f64) as usize);

    let c0 = col.saturating_sub(1) as f64 / cols as f64;
    let c1 = cols.min(col + 2) as f64 / cols as f64;
    let r0 = row.saturating_sub(1) as f64 / rows as f64;
    let r1 = rows.min(row + 2) as f64 / rows as f64;

    let box_x0 = clamp((rect.x - bounds.x) / bounds.w, 0.0, 1.0);
    let box_y0 = clamp((rect.y - bounds.y) / bounds.h, 0.0, 1.0);
    let box_x1 = clamp((rect.x + rect.w - bounds.x) / bounds.w, 0.0, 1.0);
    let box_y1 = clamp((rect.y + rect.h - bounds.y) / bounds.h, 0.0, 1.0);

    let margin = 0.02;
    let x0 = clamp(c0.min(box_x0) - margin, 0.0, 1.0);
    let y0 = clamp(r0.min(box_y0) - margin, 0.0, 1.0);
    let x1 = clamp(c1.max(box_x1) + margin, 0.0, 1.0);
    let y1 = clamp(r1.max(box_y1) + margin, 0.0, 1.0);

    [
        round_to(x0, 4),
        round_to(y0, 4),
        round_to((x1 - x0).max(0.02), 4),
        round_to((y1 - y0).max(0.02), 4),
    ]
}

/// Normalized roi -> integer pixel box `(x, y, w, h)` inside a `width` x `height` image.
pub fn roi_to_pixels(roi: &[f64; 4], width: i32, height: i32) -> (i32, i32, i32, i32) {
    let (w, h) = (width as f64, height as f64);
    let mut x0 = py_round(roi[0] * w) as i32;
    let mut y0 = py_round(roi[1] * h) as i32;
    let mut x1 = py_round((roi[0] + roi[2]) * w) as i32;
    let mut y1 = py_round((roi[1] + roi[3]) * h) as i32;
    x0 = x0.clamp(0, width - 1);
    y0 = y0.clamp(0, height - 1);
    x1 = (x0 + 1).max(x1.min(width));
    y1 = (y0 + 1).max(y1.min(height));
    (x0, y0, x1 - x0, y1 - y0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_orders_bounds() {
        assert_eq!(clamp(5.0, 0.0, 3.0), 3.0);
        assert_eq!(clamp(-1.0, 0.0, 3.0), 0.0);
        assert_eq!(clamp(1.5, 0.0, 3.0), 1.5);
    }

    #[test]
    fn rounding_matches_python_half_to_even() {
        assert_eq!(py_round(0.5), 0.0);
        assert_eq!(py_round(1.5), 2.0);
        assert_eq!(py_round(2.5), 2.0);
    }

    #[test]
    fn iou_of_identical_and_disjoint_rects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!((a.iou(&a) - 1.0).abs() < 1e-9);
        assert_eq!(a.iou(&Rect::new(20.0, 20.0, 5.0, 5.0)), 0.0);
    }

    #[test]
    fn anchor_is_kept_inside_the_box() {
        let b = Rect::new(100.0, 100.0, 100.0, 40.0);
        assert_eq!(anchor_in_box((100.0, 100.0), &b), (0.1, 0.1));
        assert_eq!(anchor_in_box((1000.0, 1000.0), &b), (0.9, 0.9));
    }

    #[test]
    fn search_window_stays_on_the_surface() {
        let bounds = Bounds::new(0.0, 0.0, 600.0, 400.0, 2.0);
        let w = search_window((10.0, 10.0), (600.0, 400.0), &bounds);
        assert_eq!((w.x, w.y, w.w, w.h), (0.0, 0.0, 600.0, 400.0));
        let w = search_window((590.0, 390.0), (200.0, 100.0), &bounds);
        assert_eq!((w.right(), w.bottom()), (600.0, 400.0));
    }

    #[test]
    fn roi_to_pixels_never_collapses() {
        let (x, y, w, h) = roi_to_pixels(&[0.99, 0.99, 0.5, 0.5], 480, 320);
        assert!(x < 480 && y < 320 && w >= 1 && h >= 1);
        assert_eq!(
            roi_to_pixels(&[0.0, 0.0, 1.0, 1.0], 480, 320),
            (0, 0, 480, 320)
        );
    }

    #[test]
    fn infer_roi_includes_an_edge_box() {
        let bounds = Bounds::new(0.0, 0.0, 600.0, 400.0, 2.0);
        let roi = infer_roi(&Rect::new(0.0, 0.0, 40.0, 30.0), &bounds, 3, 5);
        assert_eq!(roi[0], 0.0);
        assert_eq!(roi[1], 0.0);
    }
}

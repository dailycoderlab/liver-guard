//! Logical-size normalization: the trick that makes one crop work everywhere.
//!
//! A control rendered at 3x and at 2x is a different number of *pixels*, so
//! pixel sized templates never match across devices. Normalizing both the frame
//! and the template to a fixed logical width removes pixel density from the
//! problem, leaving only genuine layout scale differences for the multi-scale
//! search to absorb.

use opencv::core::{Mat, MatTraitConst, Size};
use opencv::imgproc;
use sky_ir::geometry::{py_round, Bounds, Rect};

use crate::util::{interpolation_for, Result, VisionError};

pub const CANONICAL_WIDTH: f64 = 480.0;

/// A frame resized into canonical logical space.
pub struct Normalized {
    pub image: Mat,
    pub width: i32,
    pub height: i32,
    pub factor: f64,
    pub bounds: Bounds,
}

impl Normalized {
    /// Normalized pixel -> global logical points.
    pub fn to_logical(&self, nx: f64, ny: f64) -> (f64, f64) {
        (
            self.bounds.x + (nx / self.width as f64) * self.bounds.w,
            self.bounds.y + (ny / self.height as f64) * self.bounds.h,
        )
    }

    pub fn rect_to_logical(&self, x: f64, y: f64, w: f64, h: f64) -> Rect {
        let (x0, y0) = self.to_logical(x, y);
        let (x1, y1) = self.to_logical(x + w, y + h);
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

pub fn normalize_frame(image: &Mat, bounds: Bounds, canonical_width: f64) -> Result<Normalized> {
    let (height, width) = (image.rows(), image.cols());
    if width <= 0 || height <= 0 {
        return Err(VisionError::Invalid("empty frame".into()));
    }
    let factor = canonical_width / width as f64;
    let new_w = py_round(width as f64 * factor) as i32;
    let new_h = (py_round(height as f64 * factor) as i32).max(1);
    let mut resized = Mat::default();
    imgproc::resize(image, &mut resized, Size::new(new_w, new_h), 0.0, 0.0, interpolation_for(factor))?;
    Ok(Normalized { image: resized, width: new_w, height: new_h, factor, bounds })
}

/// Pixels per normalized pixel for a template.
///
/// Derived from the *recording surface's pixel width*, not the current one.
/// Both the template and the live frame end up normalized by their own surface
/// width, so a control that kept its relative size lands at scale 1.0 no matter
/// how big either screen was.
pub fn template_factor(record_width: f64, record_scale: f64, fallback_width: f64, canonical_width: f64) -> f64 {
    let width = if record_width > 0.0 { record_width } else { fallback_width };
    let pixel_width = width * record_scale.max(0.01);
    if pixel_width <= 0.0 {
        return 1.0;
    }
    canonical_width / pixel_width
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_relative_layout_gives_factor_one_after_normalizing() {
        // Recorded on a 600pt surface at 3x; the live frame is 600pt at 2x.
        let template = template_factor(600.0, 3.0, 0.0, CANONICAL_WIDTH);
        let frame = CANONICAL_WIDTH / (600.0 * 2.0);
        // Template pixels shrink by `template`, frame pixels by `frame`; a control
        // that kept its relative size ends up the same normalized size.
        assert!((template * 3.0 - frame * 2.0).abs() < 1e-9);
    }

    #[test]
    fn falls_back_to_the_runtime_width() {
        assert_eq!(template_factor(0.0, 2.0, 600.0, 480.0), 480.0 / 1200.0);
        assert_eq!(template_factor(0.0, 1.0, 0.0, 480.0), 1.0);
    }
}

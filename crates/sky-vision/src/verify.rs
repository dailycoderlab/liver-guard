//! Second stage colour check.
//!
//! Grayscale correlation happily matches "the same shape, different colour" - a
//! greyed out button against an active one is the classic false positive.
//! Comparing mean-removed colour vectors at low resolution is cheap and kills
//! most of them.

use opencv::core::{Mat, MatTraitConst, MatTraitConstManual, Size};
use opencv::imgproc;

use crate::util::Result;

pub const SAMPLE: i32 = 16;
pub const MIN_SIMILARITY: f64 = 0.45;

fn sample(image: &Mat) -> Result<Vec<f32>> {
    let mut small = Mat::default();
    imgproc::resize(image, &mut small, Size::new(SAMPLE, SAMPLE), 0.0, 0.0, imgproc::INTER_AREA)?;
    Ok(small.data_bytes()?.iter().map(|&v| v as f32).collect())
}

/// Correlation of mean-removed colour vectors, clamped to `[0, 1]`.
pub fn color_similarity(patch: &Mat, template: &Mat) -> f64 {
    if patch.empty() || template.empty() {
        return 0.0;
    }
    let (Ok(a), Ok(b)) = (sample(patch), sample(template)) else { return 0.0 };

    let mean_a = a.iter().map(|&v| v as f64).sum::<f64>() / a.len() as f64;
    let mean_b = b.iter().map(|&v| v as f64).sum::<f64>() / b.len() as f64;
    let va: Vec<f64> = a.iter().map(|&v| v as f64 - mean_a).collect();
    let vb: Vec<f64> = b.iter().map(|&v| v as f64 - mean_b).collect();

    let norm_a = va.iter().map(|v| v * v).sum::<f64>().sqrt();
    let norm_b = vb.iter().map(|v| v * v).sum::<f64>().sqrt();
    let denom = norm_a * norm_b;
    if denom < 1e-6 {
        // Both patches are flat: fall back to comparing brightness only.
        return if (mean_a - mean_b).abs() < 12.0 { 1.0 } else { 0.0 };
    }

    let dot: f64 = va.iter().zip(&vb).map(|(x, y)| x * y).sum();
    (dot / denom).clamp(0.0, 1.0)
}

//! Control candidates around a click, in image pixel coordinates.
//!
//! Candidates come out ordered: native accessibility box, up to two contours
//! (descending score), a colour flood-fill when no contour fits, then a square
//! fallback, with later near-duplicates removed. Port of the reference
//! detector, including its integer arithmetic.

use opencv::core::{
    self, Mat, MatTraitConst, MatTraitConstManual, Point, Scalar, Size, CV_16S, CV_32S, CV_8U,
};
use opencv::imgproc;
use opencv::prelude::*;
use serde::Serialize;

use crate::util::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strictness {
    Loose,
    Normal,
    Tight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    Ax,
    Contour,
    Color,
    Fallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PixelBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Candidate {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub kind: CandidateKind,
    pub score: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct DetectOptions {
    /// Device pixels per logical point.
    pub scale: f64,
    pub min_side: f64,
    pub max_side: f64,
    pub padding: f64,
    pub strictness: Strictness,
    pub ax_box: Option<PixelBox>,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self {
            scale: 1.0,
            min_side: 24.0,
            max_side: 220.0,
            padding: 0.08,
            strictness: Strictness::Normal,
            ax_box: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Component {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    area: i32,
}

/// JavaScript `Math.round`, including negative half values.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn iou(a: &Candidate, b: &Candidate) -> f64 {
    let x0 = a.x.max(b.x);
    let y0 = a.y.max(b.y);
    let x1 = (a.x + a.w).min(b.x + b.w);
    let y1 = (a.y + a.h).min(b.y + b.h);
    if x1 <= x0 || y1 <= y0 {
        return 0.0;
    }
    let inter = (x1 - x0) * (y1 - y0);
    inter / (a.w * a.h + b.w * b.h - inter)
}

/// Count corners twice, and overlapping strips independently.
fn border_coverage(binary: &[u8], width: i32, height: i32, c: &Component) -> f64 {
    let at = |x: i32, y: i32| binary[(y * width + x) as usize] as i64;
    let (mut hit, mut total) = (0i64, 0i64);
    for t in 0..2 {
        for y in [c.y0 + t, c.y1 - t] {
            if (0..height).contains(&y) {
                hit += (c.x0..=c.x1).map(|x| at(x, y)).sum::<i64>();
                total += (c.x1 - c.x0 + 1) as i64;
            }
        }
        for x in [c.x0 + t, c.x1 - t] {
            if (0..width).contains(&x) {
                hit += (c.y0..=c.y1).map(|y| at(x, y)).sum::<i64>();
                total += (c.y1 - c.y0 + 1) as i64;
            }
        }
    }
    if total > 0 {
        hit as f64 / total as f64
    } else {
        0.0
    }
}

fn dedupe(
    candidates: Vec<Candidate>,
    width: i32,
    height: i32,
    max_side: f64,
    padding: f64,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    for candidate in candidates {
        let exact = candidate.kind == CandidateKind::Ax;
        let grow = if exact {
            0.0
        } else {
            candidate.w.max(candidate.h) * padding
        };
        let (mut x, mut y) = (candidate.x - grow, candidate.y - grow);
        let (mut w, mut h) = (candidate.w + 2.0 * grow, candidate.h + 2.0 * grow);
        if !exact && (w > max_side || h > max_side) {
            let shrink = max_side / w.max(h);
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            w *= shrink;
            h *= shrink;
            x = cx - w / 2.0;
            y = cy - h / 2.0;
        }
        x = x.min((width - 8) as f64).max(0.0);
        y = y.min((height - 8) as f64).max(0.0);
        w = w.min(width as f64 - x);
        h = h.min(height as f64 - y);
        let rounded = Candidate {
            x: js_round(x),
            y: js_round(y),
            w: js_round(w),
            h: js_round(h),
            kind: candidate.kind,
            score: candidate.score,
        };
        if !out.iter().any(|existing| iou(existing, &rounded) > 0.75) {
            out.push(rounded);
        }
    }
    out
}

/// Fixed-range flood fill around the seed; comparing each pixel to the original
/// seed rather than its neighbour keeps gradients from leaking.
fn flood_region(
    rgb: &Mat,
    sx: i32,
    sy: i32,
    tolerance: i32,
    max_pixels: i32,
) -> Result<Option<Component>> {
    let (h, w) = (rgb.rows(), rgb.cols());
    if sx < 0 || sy < 0 || sx >= w || sy >= h {
        return Ok(None);
    }
    let mut image = rgb.try_clone()?;
    let mut mask = Mat::zeros(h + 2, w + 2, CV_8U)?.to_mat()?;
    let mut rect = core::Rect::default();
    let tol = Scalar::all(tolerance as f64);
    let flags = 8 | imgproc::FLOODFILL_FIXED_RANGE | imgproc::FLOODFILL_MASK_ONLY | (1 << 8);
    let area = imgproc::flood_fill_mask(
        &mut image,
        &mut mask,
        Point::new(sx, sy),
        Scalar::all(0.0),
        Some(&mut rect),
        tol,
        tol,
        flags,
    )?;
    if area > max_pixels {
        return Ok(None);
    }
    Ok(Some(Component {
        x0: rect.x,
        y0: rect.y,
        x1: rect.x + rect.width - 1,
        y1: rect.y + rect.height - 1,
        area,
    }))
}

fn mat_i32(mat: &Mat) -> Result<Vec<i32>> {
    Ok(mat.data_typed::<i32>()?.to_vec())
}

/// Detect controls at `click` in a BGR image.
///
/// `options.scale` is device pixels per logical point; `min_side` and `max_side`
/// are logical points. Click, accessibility and returned boxes are image pixels.
/// The fallback is always proposed but may be removed as a near-duplicate.
pub fn detect_candidates(
    bgr: &Mat,
    click: (f64, f64),
    options: &DetectOptions,
) -> Result<Vec<Candidate>> {
    if bgr.typ() != core::CV_8UC3 {
        return Err(crate::util::VisionError::Invalid(
            "detect needs an 8-bit BGR image".into(),
        ));
    }
    let (height, width) = (bgr.rows(), bgr.cols());
    if width == 0 || height == 0 {
        return Err(crate::util::VisionError::Invalid(
            "image must have nonzero width and height".into(),
        ));
    }

    let scale = if options.scale == 0.0 {
        1.0
    } else {
        options.scale
    };
    let (min_side, max_side) = (options.min_side * scale, options.max_side * scale);
    let mut candidates: Vec<Candidate> = Vec::new();
    if let Some(ax) = options.ax_box.filter(|b| b.w > 0.0 && b.h > 0.0) {
        candidates.push(Candidate {
            x: ax.x,
            y: ax.y,
            w: ax.w,
            h: ax.h,
            kind: CandidateKind::Ax,
            score: 1.0,
        });
    }

    let downsample: i32 = if (width as i64) * (height as i64) > 2_500_000 {
        2
    } else {
        1
    };
    let (h, w) = (height / downsample, width / downsample);
    // The trailing row/column is dropped for odd dimensions.
    let src = bgr.data_bytes()?;
    let mut rgb_bytes = Vec::with_capacity((h * w * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let base = (((y * downsample) * width + x * downsample) * 3) as usize;
            rgb_bytes.extend_from_slice(&src[base..base + 3]);
        }
    }
    let rgb = Mat::new_rows_cols_with_data(h, w * 3, &rgb_bytes)?
        .reshape(3, h)?
        .try_clone()?;
    let click_x = click.0 / downsample as f64;
    let click_y = click.1 / downsample as f64;

    // gray = (R*77 + G*150 + B*29) >> 8, with the BGR channel order.
    let gray_bytes: Vec<u8> = rgb_bytes
        .chunks_exact(3)
        .map(|p| (((p[2] as u32) * 77 + (p[1] as u32) * 150 + (p[0] as u32) * 29) >> 8) as u8)
        .collect();
    let gray = Mat::new_rows_cols_with_data(h, w, &gray_bytes)?.try_clone()?;

    // Divide by the real number of in-bounds pixels, truncating, so the box blur
    // at edges and in the interior stays consistent.
    let mut sums = Mat::default();
    let mut counts = Mat::default();
    let ones = Mat::new_rows_cols_with_default(h, w, CV_8U, Scalar::all(1.0))?;
    imgproc::box_filter(
        &gray,
        &mut sums,
        CV_32S,
        Size::new(3, 3),
        Point::new(-1, -1),
        false,
        core::BORDER_CONSTANT,
    )?;
    imgproc::box_filter(
        &ones,
        &mut counts,
        CV_32S,
        Size::new(3, 3),
        Point::new(-1, -1),
        false,
        core::BORDER_CONSTANT,
    )?;
    let sums_v = mat_i32(&sums)?;
    let counts_v = mat_i32(&counts)?;
    let blurred_bytes: Vec<u8> = sums_v
        .iter()
        .zip(&counts_v)
        .map(|(s, c)| (s / c) as u8)
        .collect();
    let blurred = Mat::new_rows_cols_with_data(h, w, &blurred_bytes)?.try_clone()?;

    let mut gx = Mat::default();
    let mut gy = Mat::default();
    imgproc::scharr(
        &blurred,
        &mut gx,
        CV_16S,
        1,
        0,
        1.0,
        0.0,
        core::BORDER_DEFAULT,
    )?;
    imgproc::scharr(
        &blurred,
        &mut gy,
        CV_16S,
        0,
        1,
        1.0,
        0.0,
        core::BORDER_DEFAULT,
    )?;
    let gx_v = gx.data_typed::<i16>()?;
    let gy_v = gy.data_typed::<i16>()?;
    let mut mag: Vec<i32> = gx_v
        .iter()
        .zip(gy_v)
        .map(|(a, b)| (*a as i32).abs() + (*b as i32).abs())
        .collect();
    let (wu, hu) = (w as usize, h as usize);
    for x in 0..wu {
        mag[x] = 0;
        mag[(hu - 1) * wu + x] = 0;
    }
    for y in 0..hu {
        mag[y * wu] = 0;
        mag[y * wu + wu - 1] = 0;
    }
    let maximum = mag.iter().copied().max().unwrap_or(0);
    let normalized_bytes: Vec<u8> = if maximum > 0 {
        let k = 255.0 / maximum as f64;
        mag.iter().map(|&m| (m as f64 * k) as u8).collect()
    } else {
        vec![0; mag.len()]
    };
    let normalized = Mat::new_rows_cols_with_data(h, w, &normalized_bytes)?.try_clone()?;
    let mut otsu_dst = Mat::default();
    let otsu = imgproc::threshold(
        &normalized,
        &mut otsu_dst,
        0.0,
        255.0,
        imgproc::THRESH_BINARY | imgproc::THRESH_OTSU,
    )?;
    let bias = match options.strictness {
        Strictness::Loose => -12.0,
        Strictness::Normal => 0.0,
        Strictness::Tight => 14.0,
    };
    let cut = (otsu + bias).min(250.0).max(4.0);
    let binary_bytes: Vec<u8> = normalized_bytes
        .iter()
        .map(|&v| u8::from(v as f64 > cut))
        .collect();
    let binary0 = Mat::new_rows_cols_with_data(h, w, &binary_bytes)?.try_clone()?;
    let kernel = Mat::new_rows_cols_with_default(3, 3, CV_8U, Scalar::all(1.0))?;
    let mut dilated = Mat::default();
    let mut binary = Mat::default();
    imgproc::dilate(
        &binary0,
        &mut dilated,
        &kernel,
        Point::new(-1, -1),
        1,
        core::BORDER_CONSTANT,
        Scalar::all(0.0),
    )?;
    imgproc::erode(
        &dilated,
        &mut binary,
        &kernel,
        Point::new(-1, -1),
        1,
        core::BORDER_CONSTANT,
        Scalar::all(0.0),
    )?;
    let binary_v: Vec<u8> = binary.data_bytes()?.to_vec();

    let mut labels = Mat::default();
    let mut stats = Mat::default();
    let mut centroids = Mat::default();
    imgproc::connected_components_with_stats_with_algorithm(
        &binary,
        &mut labels,
        &mut stats,
        &mut centroids,
        8,
        CV_32S,
        imgproc::CCL_SAUF,
    )?;
    let stats_v = mat_i32(&stats)?;
    let min_area = 12f64.max((min_side / downsample as f64).powi(2) * 0.35);

    let required = match options.strictness {
        Strictness::Loose => 0.45,
        Strictness::Normal => 0.6,
        Strictness::Tight => 0.75,
    };
    let ds = downsample as f64;

    let fit = |c: &Component, kind: CandidateKind| -> Option<Candidate> {
        let (bw, bh) = ((c.x1 - c.x0 + 1) as f64, (c.y1 - c.y0 + 1) as f64);
        if !((c.x0 as f64) <= click_x
            && click_x <= c.x1 as f64
            && (c.y0 as f64) <= click_y
            && click_y <= c.y1 as f64)
        {
            return None;
        }
        if bw < min_side / ds || bh < min_side / ds {
            return None;
        }
        if bw > max_side / ds || bh > max_side / ds {
            return None;
        }
        if !(0.25..=4.0).contains(&(bw / bh)) {
            return None;
        }
        let rectangularity = (c.area as f64 / (bw * bh)).max(border_coverage(&binary_v, w, h, c));
        if rectangularity < required {
            return None;
        }
        // Population variance of the region's gray pixels.
        let (mut sum, mut count) = (0f64, 0f64);
        for y in c.y0..=c.y1 {
            for x in c.x0..=c.x1 {
                sum += gray_bytes[(y * w + x) as usize] as f64;
                count += 1.0;
            }
        }
        let mean = sum / count;
        let mut var = 0f64;
        for y in c.y0..=c.y1 {
            for x in c.x0..=c.x1 {
                var += (gray_bytes[(y * w + x) as usize] as f64 - mean).powi(2);
            }
        }
        if var / count < 24.0 {
            return None;
        }
        Some(Candidate {
            x: c.x0 as f64 * ds,
            y: c.y0 as f64 * ds,
            w: bw * ds,
            h: bh * ds,
            kind,
            score: rectangularity.min(0.95),
        })
    };

    let mut contour_hits: Vec<Candidate> = Vec::new();
    for row in stats_v.chunks_exact(5).skip(1) {
        let (x, y, bw, bh, area) = (row[0], row[1], row[2], row[3], row[4]);
        if area as f64 >= min_area {
            let component = Component {
                x0: x,
                y0: y,
                x1: x + bw - 1,
                y1: y + bh - 1,
                area,
            };
            if let Some(hit) = fit(&component, CandidateKind::Contour) {
                contour_hits.push(hit);
            }
        }
    }
    contour_hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    contour_hits.truncate(2);
    let have_contours = !contour_hits.is_empty();
    candidates.extend(contour_hits);

    if !have_contours {
        let tolerance = match options.strictness {
            Strictness::Loose => 28,
            Strictness::Normal => 18,
            Strictness::Tight => 10,
        };
        let max_pixels = js_round((max_side / ds).powi(2)) as i32;
        if let Some(region) = flood_region(
            &rgb,
            js_round(click_x) as i32,
            js_round(click_y) as i32,
            tolerance,
            max_pixels,
        )? {
            if let Some(color) = fit(&region, CandidateKind::Color) {
                candidates.push(Candidate {
                    kind: CandidateKind::Color,
                    score: (color.score * 0.8).min(0.7),
                    ..color
                });
            }
        }
    }

    let fallback_side = max_side.min((64.0 * scale).max(min_side * 2.0));
    candidates.push(Candidate {
        x: click.0 - fallback_side / 2.0,
        y: click.1 - fallback_side / 2.0,
        w: fallback_side,
        h: fallback_side,
        kind: CandidateKind::Fallback,
        score: 0.2,
    });
    Ok(dedupe(candidates, width, height, max_side, options.padding))
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencv::core::Vec3b;
    use opencv::imgproc::{rectangle, LINE_8};

    fn scene() -> Mat {
        let mut image = Mat::new_rows_cols_with_default(
            400,
            600,
            core::CV_8UC3,
            Scalar::new(40.0, 44.0, 48.0, 0.0),
        )
        .unwrap();
        let rect = core::Rect::new(240, 180, 120, 56);
        rectangle(
            &mut image,
            rect,
            Scalar::new(196.0, 120.0, 60.0, 0.0),
            -1,
            LINE_8,
            0,
        )
        .unwrap();
        rectangle(
            &mut image,
            rect,
            Scalar::new(250.0, 240.0, 220.0, 0.0),
            3,
            LINE_8,
            0,
        )
        .unwrap();
        // Texture inside the button so the variance gate passes.
        for i in 0..20 {
            let x = 255 + i * 4;
            *image.at_2d_mut::<Vec3b>(205, x).unwrap() = Vec3b::from([255, 255, 255]);
            *image.at_2d_mut::<Vec3b>(206, x).unwrap() = Vec3b::from([255, 255, 255]);
        }
        image
    }

    #[test]
    fn finds_the_button_under_the_click() {
        let found = detect_candidates(&scene(), (300.0, 208.0), &DetectOptions::default()).unwrap();
        assert!(!found.is_empty());
        let best = found
            .iter()
            .find(|c| matches!(c.kind, CandidateKind::Contour | CandidateKind::Color));
        let best = best.expect("a contour or colour candidate");
        assert!(best.x <= 245.0 && best.x + best.w >= 355.0, "{best:?}");
        assert!(best.y <= 185.0 && best.y + best.h >= 230.0, "{best:?}");
    }

    #[test]
    fn fallback_is_proposed_on_an_empty_surface() {
        let blank =
            Mat::new_rows_cols_with_default(300, 300, core::CV_8UC3, Scalar::all(30.0)).unwrap();
        let found = detect_candidates(&blank, (150.0, 150.0), &DetectOptions::default()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, CandidateKind::Fallback);
    }

    #[test]
    fn ax_box_comes_first_and_is_exact() {
        let options = DetectOptions {
            ax_box: Some(PixelBox {
                x: 10.0,
                y: 20.0,
                w: 80.0,
                h: 40.0,
            }),
            ..Default::default()
        };
        let found = detect_candidates(&scene(), (50.0, 40.0), &options).unwrap();
        assert_eq!(found[0].kind, CandidateKind::Ax);
        assert_eq!(
            (found[0].x, found[0].y, found[0].w, found[0].h),
            (10.0, 20.0, 80.0, 40.0)
        );
    }

    #[test]
    fn rejects_non_bgr_input() {
        let gray =
            Mat::new_rows_cols_with_default(10, 10, core::CV_8UC1, Scalar::all(0.0)).unwrap();
        assert!(detect_candidates(&gray, (5.0, 5.0), &DetectOptions::default()).is_err());
    }
}

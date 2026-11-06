//! Template matching primitives: single scale, pyramid multi-scale, NMS.

use opencv::prelude::*;
use opencv::core::{self, no_array, Mat, MatTraitConst, MatTraitManual, Point, Scalar, CV_32F};
use opencv::imgproc;
use sky_ir::geometry::py_round;
use sky_ir::{Mask, ScaleSpec};

use crate::util::{resize_by, std_dev, to_gray, Result};

/// Templates flatter than this carry no correlation signal.
pub const LOW_TEXTURE_STD: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Match {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub score: f64,
    pub scale: f64,
}

impl Match {
    pub fn center(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

/// Optionally reduce an image to its edge map. Edge matching survives skin
/// changes and colour tints because it only looks at structure; it is not the
/// default because it throws away colour information.
pub fn to_match_space(image: &Mat, mask: Mask) -> Result<Mat> {
    if mask != Mask::Edges {
        return Ok(image.try_clone()?);
    }
    let gray = to_gray(image)?;
    let mut edges = Mat::default();
    imgproc::canny(&gray, &mut edges, 50.0, 150.0, 3, false)?;
    Ok(edges)
}

/// Raw correlation map, or `None` when the sizes or texture make it meaningless.
pub fn match_at_scale(image: &Mat, template: &Mat) -> Result<Option<Mat>> {
    let (ih, iw) = (image.rows(), image.cols());
    let (th, tw) = (template.rows(), template.cols());
    if th > ih || tw > iw || th < 4 || tw < 4 {
        return Ok(None);
    }
    if std_dev(template)? < LOW_TEXTURE_STD {
        // TM_CCOEFF_NORMED of a constant patch is 1.0 everywhere, which would
        // look like a perfect match at the top-left corner.
        return Ok(None);
    }
    let mut result = Mat::default();
    imgproc::match_template(image, template, &mut result, imgproc::TM_CCOEFF_NORMED, &no_array())?;
    debug_assert_eq!(result.typ(), CV_32F);
    for value in result.data_typed_mut::<f32>()? {
        if !value.is_finite() {
            *value = 0.0;
        }
    }
    Ok(Some(result))
}

fn max_loc(map: &Mat) -> Result<(f64, Point)> {
    let mut max_val = 0.0;
    let mut loc = Point::new(0, 0);
    core::min_max_loc(map, None, Some(&mut max_val), None, Some(&mut loc), &no_array())?;
    Ok((max_val, loc))
}

/// Non-maximum suppression by blacking out the neighbourhood of each hit: take
/// the global max, record it, paint over that area, repeat.
pub fn collect_peaks(result: &Mat, template_w: i32, template_h: i32, threshold: f64, limit: usize) -> Result<Vec<Match>> {
    if result.empty() {
        return Ok(vec![]);
    }
    let mut work = result.try_clone()?;
    let radius_x = (template_w / 2).max(2);
    let radius_y = (template_h / 2).max(2);
    let mut out = Vec::new();

    while out.len() < limit {
        let (max_val, loc) = max_loc(&work)?;
        if max_val < threshold {
            break;
        }
        out.push(Match {
            x: loc.x as f64,
            y: loc.y as f64,
            w: template_w as f64,
            h: template_h as f64,
            score: max_val,
            scale: 1.0,
        });
        let x0 = (loc.x - radius_x).max(0);
        let y0 = (loc.y - radius_y).max(0);
        let x1 = work.cols().min(loc.x + radius_x + 1);
        let y1 = work.rows().min(loc.y + radius_y + 1);
        let mut region = Mat::roi_mut(&mut work, core::Rect::new(x0, y0, x1 - x0, y1 - y0))?;
        region.set_to(&Scalar::all(-1.0), &no_array())?;
    }
    Ok(out)
}

pub fn scale_ladder(spec: &ScaleSpec) -> Vec<f64> {
    let mut low = spec.min;
    let mut high = spec.max;
    let mut steps = spec.steps as i64;
    low = low.min(3.0).max(0.2);
    high = high.min(3.0).max(low);
    steps = steps.min(21).max(1);
    if steps == 1 {
        return vec![(low + high) / 2.0];
    }
    let step = (high - low) / (steps - 1) as f64;
    (0..steps).map(|i| sky_ir::geometry::round_to(low + step * i as f64, 4)).collect()
}

/// Coarse-to-fine search over a scale ladder.
///
/// Coarse: half resolution when the work is large, every scale, pick the best.
/// Fine: full resolution, three scales around the winner, small window.
pub fn multi_scale_match(
    search: &Mat,
    template: &Mat,
    scales: &[f64],
    threshold: f64,
    max_count: usize,
) -> Result<Vec<Match>> {
    if search.empty() || template.empty() {
        return Ok(vec![]);
    }
    if std_dev(template)? < LOW_TEXTURE_STD {
        return Ok(vec![]);
    }

    let (ih, iw) = (search.rows(), search.cols());
    let (th, tw) = (template.rows(), template.cols());
    if th > ih || tw > iw {
        return Ok(vec![]);
    }

    // Only downscale when the work would be expensive.
    let work_area = iw as f64 * ih as f64 * tw as f64 * th as f64;
    let coarse = if work_area > 4_000_000.0 { 0.5 } else { 1.0 };
    let search_coarse = if coarse != 1.0 { resize_by(search, coarse)? } else { search.try_clone()? };

    let mut best: Option<(f64, f64, i32, i32)> = None;
    for &scale in scales {
        let factor = scale * coarse;
        let tpl = resize_by(template, factor)?;
        if tpl.rows() > search_coarse.rows() || tpl.cols() > search_coarse.cols() {
            continue;
        }
        let Some(result) = match_at_scale(&search_coarse, &tpl)? else { continue };
        let (max_val, loc) = max_loc(&result)?;
        if best.map_or(true, |b| max_val > b.0) {
            best = Some((max_val, scale, loc.x, loc.y));
        }
    }

    let Some((_, best_scale, coarse_x, coarse_y)) = best else { return Ok(vec![]) };

    let mut fine_scales = vec![(best_scale * 0.97).max(0.2), best_scale, (best_scale * 1.03).min(3.0)];
    fine_scales.sort_by(|a, b| a.partial_cmp(b).unwrap());
    fine_scales.dedup();

    // The coarse image was scaled BY `coarse`, so a coarse coordinate maps back
    // by dividing by it.
    let full_x = coarse_x as f64 / coarse;
    let full_y = coarse_y as f64 / coarse;
    let pad = 12;
    let tpl_w = py_round(tw as f64 * best_scale) as i32;
    let tpl_h = py_round(th as f64 * best_scale) as i32;
    let wx0 = ((full_x as i32) - pad).max(0);
    let wy0 = ((full_y as i32) - pad).max(0);
    let wx1 = iw.min(full_x as i32 + tpl_w + pad + 1);
    let wy1 = ih.min(full_y as i32 + tpl_h + pad + 1);
    if wx1 <= wx0 || wy1 <= wy0 {
        return Ok(vec![]);
    }
    let window = crate::util::crop(search, wx0, wy0, wx1 - wx0, wy1 - wy0)?;

    let mut matches = Vec::new();
    for scale in fine_scales {
        let tpl = resize_by(template, scale)?;
        if tpl.rows() > window.rows() || tpl.cols() > window.cols() {
            continue;
        }
        let Some(result) = match_at_scale(&window, &tpl)? else { continue };
        for peak in collect_peaks(&result, tpl.cols(), tpl.rows(), threshold, max_count)? {
            matches.push(Match {
                x: peak.x + wx0 as f64,
                y: peak.y + wy0 as f64,
                w: tpl.cols() as f64,
                h: tpl.rows() as f64,
                score: peak.score,
                scale,
            });
        }
    }

    matches.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    matches.truncate(max_count);
    Ok(matches)
}

pub fn single_scale_match(search: &Mat, template: &Mat, threshold: f64, max_count: usize) -> Result<Vec<Match>> {
    let Some(result) = match_at_scale(search, template)? else { return Ok(vec![]) };
    collect_peaks(&result, template.cols(), template.rows(), threshold, max_count)
}

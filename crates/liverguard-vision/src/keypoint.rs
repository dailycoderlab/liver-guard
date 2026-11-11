//! Keypoint fallback.
//!
//! Used when template correlation fails: scale drift, a reskinned control,
//! slight rotation. Slower by an order of magnitude, so it is only tried after
//! the template strategies miss, and still inside the target's search region.

use opencv::calib3d;
use opencv::core::{
    no_array, DMatch, KeyPoint, Mat, MatTraitConst, Point2f, Size, Vector, NORM_L2,
};
use opencv::features2d::{BFMatcher, Feature2DTrait, SIFT};
use opencv::imgproc;
use opencv::prelude::*;

use crate::tpl::Match;
use crate::util::{to_gray, Result};

const MAX_FEATURES: i32 = 400;
const MIN_GOOD: usize = 8;
const RATIO: f32 = 0.75;

fn prepare(image: &Mat) -> Result<Mat> {
    let gray = to_gray(image)?;
    let (height, width) = (gray.rows(), gray.cols());
    if height.max(width) > 900 {
        let factor = 900.0 / height.max(width) as f64;
        let mut small = Mat::default();
        imgproc::resize(
            &gray,
            &mut small,
            Size::new(
                (width as f64 * factor) as i32,
                (height as f64 * factor) as i32,
            ),
            0.0,
            0.0,
            imgproc::INTER_AREA,
        )?;
        return Ok(small);
    }
    Ok(gray)
}

/// SIFT + ratio test + RANSAC homography. Returns at most one match.
pub fn feature_match(search: &Mat, template: &Mat, threshold: f64) -> Result<Vec<Match>> {
    if search.empty() || template.empty() {
        return Ok(vec![]);
    }
    // OpenCV errors in this path mean "no usable features", not a broken run.
    Ok(feature_match_inner(search, template, threshold).unwrap_or_default())
}

fn feature_match_inner(search: &Mat, template: &Mat, threshold: f64) -> Result<Vec<Match>> {
    let search_gray = prepare(search)?;
    let template_gray = prepare(template)?;
    if template_gray.rows().min(template_gray.cols()) < 16 {
        return Ok(vec![]);
    }

    let mut detector = SIFT::create_def()?;
    let mut kp1 = Vector::<KeyPoint>::new();
    let mut kp2 = Vector::<KeyPoint>::new();
    let mut des1 = Mat::default();
    let mut des2 = Mat::default();
    detector.detect_and_compute(&template_gray, &no_array(), &mut kp1, &mut des1, false)?;
    detector.detect_and_compute(&search_gray, &no_array(), &mut kp2, &mut des2, false)?;
    let _ = MAX_FEATURES;
    if des1.empty() || des2.empty() || kp1.len() < MIN_GOOD || kp2.len() < MIN_GOOD {
        return Ok(vec![]);
    }

    let matcher = BFMatcher::create(NORM_L2, false)?;
    let mut knn = Vector::<Vector<DMatch>>::new();
    matcher.knn_train_match(&des1, &des2, &mut knn, 2, &no_array(), false)?;

    let good: Vec<DMatch> = knn
        .iter()
        .filter(|pair| {
            pair.len() == 2 && pair.get(0).unwrap().distance < RATIO * pair.get(1).unwrap().distance
        })
        .map(|pair| pair.get(0).unwrap())
        .collect();
    if good.len() < MIN_GOOD {
        return Ok(vec![]);
    }

    let src: Vector<Point2f> = good
        .iter()
        .map(|m| kp1.get(m.query_idx as usize).unwrap().pt())
        .collect();
    let dst: Vector<Point2f> = good
        .iter()
        .map(|m| kp2.get(m.train_idx as usize).unwrap().pt())
        .collect();

    let mut mask = Mat::default();
    let homography = calib3d::find_homography(&src, &dst, &mut mask, calib3d::RANSAC, 5.0)?;
    if homography.empty() || mask.empty() {
        return Ok(vec![]);
    }

    let inliers = mask.data_bytes()?.iter().filter(|&&v| v != 0).count();
    if inliers < MIN_GOOD {
        return Ok(vec![]);
    }

    let (th, tw) = (template_gray.rows() as f32, template_gray.cols() as f32);
    let corners = Vector::<Point2f>::from_slice(&[
        Point2f::new(0.0, 0.0),
        Point2f::new(tw, 0.0),
        Point2f::new(tw, th),
        Point2f::new(0.0, th),
    ]);
    let mut projected = Vector::<Point2f>::new();
    opencv::core::perspective_transform(&corners, &mut projected, &homography)?;

    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in projected.iter() {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }

    let scale = (x1 - x0) as f64 / (tw as f64).max(1.0);
    let confidence = (inliers as f64 / good.len().max(1) as f64).min(0.99);
    if confidence < threshold {
        return Ok(vec![]);
    }
    Ok(vec![Match {
        x: x0 as f64,
        y: y0 as f64,
        w: (x1 - x0) as f64,
        h: (y1 - y0) as f64,
        score: confidence,
        scale,
    }])
}

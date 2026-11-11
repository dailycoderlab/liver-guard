//! Shared error type and small `Mat` helpers.

use liverguard_ir::geometry::py_round;
use opencv::core::{Mat, MatTraitConst, MatTraitConstManual, Size, CV_8UC1, CV_8UC3};
use opencv::imgproc;

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("opencv: {0}")]
    Cv(#[from] opencv::Error),
    #[error("{0}")]
    Asset(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, VisionError>;

/// `INTER_AREA` when shrinking, `INTER_LINEAR` when growing: the rule every
/// resize in the matcher follows.
pub fn interpolation_for(factor: f64) -> i32 {
    if factor < 1.0 {
        imgproc::INTER_AREA
    } else {
        imgproc::INTER_LINEAR
    }
}

/// Scale an image by `factor`, never below 4 px per side. Returns a clone when
/// the size is unchanged.
pub fn resize_by(image: &Mat, factor: f64) -> Result<Mat> {
    let width = image.cols();
    let height = image.rows();
    let new_w = (py_round(width as f64 * factor) as i32).max(4);
    let new_h = (py_round(height as f64 * factor) as i32).max(4);
    if new_w == width && new_h == height {
        return Ok(image.try_clone()?);
    }
    let mut out = Mat::default();
    imgproc::resize(
        image,
        &mut out,
        Size::new(new_w, new_h),
        0.0,
        0.0,
        interpolation_for(factor),
    )?;
    Ok(out)
}

/// A continuous copy of a sub-rectangle.
pub fn crop(image: &Mat, x: i32, y: i32, w: i32, h: i32) -> Result<Mat> {
    let roi = Mat::roi(image, opencv::core::Rect::new(x, y, w, h))?;
    Ok(roi.try_clone()?)
}

fn continuous_bytes(mat: &Mat) -> Result<Mat> {
    if mat.is_continuous() {
        Ok(mat.try_clone()?)
    } else {
        // `try_clone` of a ROI yields a fresh continuous buffer.
        Ok(mat.try_clone()?)
    }
}

/// Population standard deviation over every element (all channels), like `np.std`.
pub fn std_dev(mat: &Mat) -> Result<f64> {
    if mat.empty() {
        return Ok(0.0);
    }
    let owned = continuous_bytes(mat)?;
    let bytes = owned.data_bytes()?;
    if bytes.is_empty() {
        return Ok(0.0);
    }
    let n = bytes.len() as f64;
    let mean = bytes.iter().map(|&v| v as f64).sum::<f64>() / n;
    let var = bytes
        .iter()
        .map(|&v| (v as f64 - mean).powi(2))
        .sum::<f64>()
        / n;
    Ok(var.sqrt())
}

/// Mean over every element.
pub fn mean(mat: &Mat) -> Result<f64> {
    let owned = continuous_bytes(mat)?;
    let bytes = owned.data_bytes()?;
    if bytes.is_empty() {
        return Ok(0.0);
    }
    Ok(bytes.iter().map(|&v| v as f64).sum::<f64>() / bytes.len() as f64)
}

pub fn to_gray(image: &Mat) -> Result<Mat> {
    if image.channels() == 1 {
        return Ok(image.try_clone()?);
    }
    let mut gray = Mat::default();
    cvt_bgr_to_gray(image, &mut gray)?;
    Ok(gray)
}

pub fn cvt_bgr_to_gray(src: &Mat, dst: &mut Mat) -> Result<()> {
    imgproc::cvt_color_def(src, dst, imgproc::COLOR_BGR2GRAY)?;
    Ok(())
}

pub fn is_bgr_or_gray(image: &Mat) -> bool {
    let t = image.typ();
    t == CV_8UC3 || t == CV_8UC1
}

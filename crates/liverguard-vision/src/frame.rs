//! A captured frame plus the metadata needed to map it back to the screen.

use std::cell::{OnceCell, RefCell};
use std::rc::Rc;
use std::time::Instant;

use liverguard_ir::geometry::{py_round, Bounds, Rect};
use opencv::core::{Mat, MatTraitConst, MatTraitConstManual, Size, Vector};
use opencv::prelude::*;
use opencv::{imgcodecs, imgproc};

use crate::normalize::Normalized;
use crate::util::{self, Result, VisionError};

/// One screenshot of a capture surface. `image` is BGR at device pixel
/// resolution; the mapping back to logical points is `pixels / scale + origin`.
pub struct Frame {
    pub image: Mat,
    pub bounds: Bounds,
    pub seq: u64,
    pub captured_at: Instant,
    /// How long the capture itself took. macOS *stalls* instead of failing when
    /// screen recording permission is lost, so a run watches this value.
    pub capture_ms: f64,
    hash: OnceCell<String>,
    normalized: RefCell<Option<(f64, Rc<Normalized>)>>,
}

impl Frame {
    pub fn new(image: Mat, bounds: Bounds) -> Self {
        Self {
            image,
            bounds,
            seq: 0,
            captured_at: Instant::now(),
            capture_ms: 0.0,
            hash: OnceCell::new(),
            normalized: RefCell::new(None),
        }
    }

    pub fn width(&self) -> i32 {
        self.image.cols()
    }

    pub fn height(&self) -> i32 {
        self.image.rows()
    }

    /// Pixels per logical point, derived from the real image so a wrong
    /// reported scale can never silently corrupt matching.
    pub fn scale(&self) -> f64 {
        if self.bounds.w <= 0.0 {
            return self.bounds.scale;
        }
        self.width() as f64 / self.bounds.w
    }

    pub fn crop_logical(&self, rect: &Rect) -> Result<Mat> {
        let scale = self.scale();
        let (w, h) = (self.width(), self.height());
        let mut x0 = py_round((rect.x - self.bounds.x) * scale) as i32;
        let mut y0 = py_round((rect.y - self.bounds.y) * scale) as i32;
        let mut x1 = py_round((rect.x + rect.w - self.bounds.x) * scale) as i32;
        let mut y1 = py_round((rect.y + rect.h - self.bounds.y) * scale) as i32;
        x0 = x0.clamp(0, w - 1);
        y0 = y0.clamp(0, h - 1);
        x1 = (x0 + 1).max(x1.min(w));
        y1 = (y0 + 1).max(y1.min(h));
        util::crop(&self.image, x0, y0, x1 - x0, y1 - y0)
    }

    /// 16x16 mean-threshold hash: stable across dithering, changes as soon as
    /// the screen actually changes. Used to skip matching on a static screen.
    pub fn hash(&self) -> Result<&str> {
        if let Some(value) = self.hash.get() {
            return Ok(value);
        }
        let computed = compute_hash(&self.image)?;
        Ok(self.hash.get_or_init(|| computed))
    }

    /// Standard deviation of a 64x64 downscale. Without screen recording
    /// permission macOS hands back a flat surface instead of failing, so this is
    /// how `doctor` tells "capturing" from "capturing a black rectangle".
    pub fn content_std(&self) -> Result<f64> {
        let mut small = Mat::default();
        imgproc::resize(
            &self.image,
            &mut small,
            Size::new(64, 64),
            0.0,
            0.0,
            imgproc::INTER_AREA,
        )?;
        util::std_dev(&small)
    }

    pub fn to_png(&self) -> Result<Vec<u8>> {
        encode(&self.image, ".png", &Vector::new())
    }

    pub fn to_jpeg(&self, quality: i32, max_width: i32) -> Result<Vec<u8>> {
        let params = Vector::from_slice(&[imgcodecs::IMWRITE_JPEG_QUALITY, quality]);
        if max_width > 0 && self.width() > max_width {
            let factor = max_width as f64 / self.width() as f64;
            let mut small = Mat::default();
            let size = Size::new(
                max_width,
                ((self.height() as f64) * factor).round().max(1.0) as i32,
            );
            imgproc::resize(&self.image, &mut small, size, 0.0, 0.0, imgproc::INTER_AREA)?;
            return encode(&small, ".jpg", &params);
        }
        encode(&self.image, ".jpg", &params)
    }

    /// The frame in canonical logical space, cached because several targets
    /// are usually resolved against the same frame.
    pub fn normalized(&self, canonical_width: f64) -> Result<Rc<Normalized>> {
        if let Some((width, cached)) = self.normalized.borrow().as_ref() {
            if *width == canonical_width {
                return Ok(cached.clone());
            }
        }
        let value = Rc::new(crate::normalize::normalize_frame(
            &self.image,
            self.bounds,
            canonical_width,
        )?);
        *self.normalized.borrow_mut() = Some((canonical_width, value.clone()));
        Ok(value)
    }
}

pub fn encode(image: &Mat, ext: &str, params: &Vector<i32>) -> Result<Vec<u8>> {
    let mut buffer = Vector::<u8>::new();
    let ok = imgcodecs::imencode(ext, image, &mut buffer, params)?;
    if !ok {
        return Err(VisionError::Invalid(format!("{ext} encode failed")));
    }
    Ok(buffer.to_vec())
}

fn compute_hash(image: &Mat) -> Result<String> {
    let mut small = Mat::default();
    imgproc::resize(
        image,
        &mut small,
        Size::new(16, 16),
        0.0,
        0.0,
        imgproc::INTER_AREA,
    )?;
    let gray = util::to_gray(&small)?;
    let bytes = gray.data_bytes()?;
    let mean = bytes.iter().map(|&v| v as f64).sum::<f64>() / bytes.len() as f64;
    Ok(bytes
        .iter()
        .map(|&v| if v as f64 > mean { '1' } else { '0' })
        .collect())
}

//! Turning a rectangle drawn on a frame into a template target.

use liverguard_ir::{AssetBox, Bounds, TemplateTarget};

use crate::assets::AssetStore;
use crate::frame::{encode, Frame};
use crate::util::{self, Result, VisionError};
use opencv::core::{Mat, MatTraitConst, Vector};
use opencv::imgcodecs;

/// How far, as a fraction of the surface, the search region extends past the
/// drawn box. Small enough to avoid look-alikes elsewhere, big enough to
/// survive a control that moves a little.
pub const ROI_PADDING: f64 = 0.15;

/// A template crop and the target that refers to it.
pub struct NewTemplate {
    /// PNG bytes of the crop; store them under `target.asset`.
    pub png: Vec<u8>,
    pub target: TemplateTarget,
}

/// Largest side, in pixels, accepted for a user-supplied screenshot.
pub const MAX_IMAGE_SIDE: i32 = 10_000;

/// Decode a user-supplied screenshot (PNG / JPEG / ...) into BGR pixels and the
/// bounds that go with it. There is no real surface behind the file, so it is
/// treated as a 1x capture of the whole surface at its own pixel size: matching
/// normalises by width, so only the framing (full surface, not a crop) matters.
pub fn decode_image(bytes: &[u8]) -> Result<(Mat, Bounds)> {
    let image = imgcodecs::imdecode(&Vector::<u8>::from_slice(bytes), imgcodecs::IMREAD_COLOR)?;
    if image.empty() {
        return Err(VisionError::Invalid("the file is not a decodable image".to_string()));
    }
    let (w, h) = (image.cols(), image.rows());
    if w < 8 || h < 8 || w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE {
        return Err(VisionError::Invalid(format!("image size {w}x{h} is outside 8..{MAX_IMAGE_SIDE} px")));
    }
    Ok((image, Bounds { x: 0.0, y: 0.0, w: w as f64, h: h as f64, scale: 1.0 }))
}

/// Crop `rect` (image pixels: x, y, w, h) out of `frame`.
pub fn create_template(frame: &Frame, rect: (i32, i32, i32, i32)) -> Result<NewTemplate> {
    let (width, height) = (frame.width(), frame.height());
    let x = rect.0.clamp(0, width - 1);
    let y = rect.1.clamp(0, height - 1);
    let w = rect.2.min(width - x);
    let h = rect.3.min(height - y);
    if w < 4 || h < 4 {
        return Err(VisionError::Invalid("the selected area is too small".to_string()));
    }

    let crop = util::crop(&frame.image, x, y, w, h)?;
    let png = encode(&crop, ".png", &Vector::new())?;
    let sha = AssetStore::hash_bytes(&png);

    let (fw, fh) = (width as f64, height as f64);
    let pad_x = ROI_PADDING;
    let pad_y = ROI_PADDING;
    let left = (x as f64 / fw - pad_x).max(0.0);
    let top = (y as f64 / fh - pad_y).max(0.0);
    let right = ((x + w) as f64 / fw + pad_x).min(1.0);
    let bottom = ((y + h) as f64 / fh + pad_y).min(1.0);

    let mut target = TemplateTarget::new(sha, AssetBox { w: w as f64, h: h as f64 }, [left, top, right - left, bottom - top]);
    target.record_scale = frame.scale();
    target.record_width = frame.bounds.w;
    Ok(NewTemplate { png, target })
}

#[cfg(test)]
mod tests {
    use super::*;
    use opencv::core::{Scalar, CV_8UC3};

    fn frame(w: i32, h: i32, scale: f64) -> Frame {
        let image = Mat::new_rows_cols_with_default(h, w, CV_8UC3, Scalar::new(40.0, 50.0, 60.0, 0.0)).unwrap();
        Frame::new(image, Bounds { x: 0.0, y: 0.0, w: w as f64 / scale, h: h as f64 / scale, scale })
    }

    #[test]
    fn records_pixel_box_scale_and_surface_width() {
        let f = frame(1200, 800, 2.0);
        let made = create_template(&f, (300, 200, 120, 60)).unwrap();
        assert_eq!((made.target.asset_box.w, made.target.asset_box.h), (120.0, 60.0));
        assert_eq!(made.target.record_scale, 2.0);
        assert_eq!(made.target.record_width, 600.0);
        assert_eq!(made.target.asset.len(), 64);
        assert_eq!(made.target.asset, AssetStore::hash_bytes(&made.png));
    }

    #[test]
    fn search_region_stays_inside_the_surface_and_contains_the_box() {
        let f = frame(1000, 500, 1.0);
        let made = create_template(&f, (0, 0, 100, 50)).unwrap();
        let [x, y, w, h] = made.target.roi;
        assert_eq!((x, y), (0.0, 0.0));
        assert!(x + w <= 1.0 && y + h <= 1.0);
        assert!(w >= 0.1 && h >= 0.1);
    }

    #[test]
    fn clamps_a_box_that_overhangs_the_edge() {
        let f = frame(400, 300, 1.0);
        let made = create_template(&f, (350, 250, 100, 100)).unwrap();
        assert_eq!((made.target.asset_box.w, made.target.asset_box.h), (50.0, 50.0));
    }

    #[test]
    fn decodes_an_uploaded_image_as_a_1x_full_surface() {
        let png = frame(320, 200, 1.0).to_png().unwrap();
        let (image, bounds) = decode_image(&png).unwrap();
        assert_eq!((image.cols(), image.rows()), (320, 200));
        assert_eq!((bounds.w, bounds.h, bounds.scale), (320.0, 200.0, 1.0));
        // A template cut from it records the whole image as the surface.
        let made = create_template(&Frame::new(image, bounds), (40, 40, 80, 40)).unwrap();
        assert_eq!((made.target.record_scale, made.target.record_width), (1.0, 320.0));
    }

    #[test]
    fn rejects_bytes_that_are_not_an_image() {
        assert!(decode_image(b"definitely not an image").is_err());
        assert!(decode_image(&[]).is_err());
    }

    #[test]
    fn rejects_a_tiny_box() {
        let f = frame(400, 300, 1.0);
        assert!(create_template(&f, (10, 10, 2, 2)).is_err());
    }
}

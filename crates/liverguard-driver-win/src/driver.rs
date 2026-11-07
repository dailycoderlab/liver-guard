//! `WinDriver`: the Windows implementation of the engine's `Driver`.

use std::time::Instant;

use opencv::core::{Mat, MatTraitConst, Scalar, CV_8UC4};
use opencv::imgproc;
use liverguard_engine::{Driver, DriverError, DriverResult, PointerAction};
use liverguard_ir::{Bounds, Rect};
use liverguard_vision::Frame;

use crate::capture::{CaptureError, Capturer, RawImage, Target};
use crate::window::{self, WindowInfo};
use crate::{input, uia};

/// Which part of the screen a script works on.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SurfaceSelector {
    /// The primary monitor.
    #[default]
    FullScreen,
    Window {
        number: Option<isize>,
        query: Option<String>,
    },
}

struct Surface {
    rect: Rect,
    window: Option<WindowInfo>,
}

pub struct WinDriver {
    selector: SurfaceSelector,
    capturer: Capturer,
    activate_window: bool,
}

impl WinDriver {
    pub fn new(selector: SurfaceSelector) -> Self {
        Self { selector, capturer: Capturer::new(), activate_window: true }
    }

    pub fn full_screen() -> Self {
        Self::new(SurfaceSelector::FullScreen)
    }

    pub fn with_activation(mut self, activate: bool) -> Self {
        self.activate_window = activate;
        self
    }

    fn surface(&self) -> DriverResult<Surface> {
        match &self.selector {
            SurfaceSelector::FullScreen => Ok(Surface { rect: window::primary_display_bounds(), window: None }),
            SurfaceSelector::Window { number, query } => {
                let mut found = number.and_then(window::window_by_number);
                if found.is_none() {
                    found = query.as_deref().and_then(window::find_window);
                }
                // Never turn a missing selected window into a whole-screen target:
                // that could inject input into an unrelated application.
                let found = found.ok_or_else(|| DriverError::failed("Selected window is unavailable; select the target again"))?;
                Ok(Surface { rect: found.bounds, window: Some(found) })
            }
        }
    }

    fn grab(&mut self, surface: &Surface) -> DriverResult<RawImage> {
        let target = match &surface.window {
            Some(w) => Target::Window(w.number),
            None => Target::Monitor(window::primary_monitor().0 as isize),
        };
        self.capturer.grab(target).map_err(|error| match error {
            CaptureError::Missing => DriverError::failed("Selected window is unavailable; select the target again"),
            other => DriverError::failed(other.to_string()),
        })
    }

    fn ensure_input_target(&self, points: &[(f64, f64)]) -> DriverResult<()> {
        if matches!(self.selector, SurfaceSelector::FullScreen) {
            return Ok(());
        }
        let target = self.surface()?.window.ok_or_else(|| DriverError::failed("Selected window is unavailable"))?;
        if self.activate_window && !window::activate(&target) {
            return Err(DriverError::failed("Selected window cannot be activated"));
        }
        if points.iter().any(|&(x, y)| !target.bounds.contains(x, y)) {
            return Err(DriverError::failed("Input point is outside the selected window"));
        }
        for other in window::occluding_windows(&target, 0.000001) {
            if points.iter().any(|&(x, y)| other.bounds.contains(x, y)) {
                return Err(DriverError::failed("Selected window is covered at the input point"));
            }
        }
        Ok(())
    }
}

fn input_error(error: input::InputError) -> DriverError {
    DriverError::failed(error.to_string())
}

fn bgra_to_bgr(raw: &RawImage) -> DriverResult<Mat> {
    let to_error = |e: opencv::Error| DriverError::failed(format!("image conversion failed: {e}"));
    let mut bgra = Mat::new_rows_cols_with_default(raw.height as i32, raw.width as i32, CV_8UC4, Scalar::all(0.0)).map_err(to_error)?;
    let dst = bgra.data_bytes_mut().map_err(to_error)?;
    let n = raw.bgra.len().min(dst.len());
    dst[..n].copy_from_slice(&raw.bgra[..n]);
    let mut bgr = Mat::default();
    imgproc::cvt_color_def(&bgra, &mut bgr, imgproc::COLOR_BGRA2BGR).map_err(to_error)?;
    Ok(bgr)
}

impl Driver for WinDriver {
    fn bounds(&mut self) -> DriverResult<Bounds> {
        let surface = self.surface()?;
        let raw = self.grab(&surface)?;
        Ok(Bounds::new(surface.rect.x, surface.rect.y, surface.rect.w, surface.rect.h, raw.width as f64 / surface.rect.w))
    }

    fn capture(&mut self) -> DriverResult<Frame> {
        let surface = self.surface()?;
        let started = Instant::now();
        let raw = self.grab(&surface)?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

        if surface.window.is_some() {
            let current = self.surface()?;
            let same = current.window.as_ref().map(|w| (w.number, w.pid)) == surface.window.as_ref().map(|w| (w.number, w.pid))
                && current.rect == surface.rect;
            if !same {
                return Err(DriverError::SurfaceChanged);
            }
        }

        let scale = raw.width as f64 / surface.rect.w;
        let image = bgra_to_bgr(&raw)?;
        let mut frame = Frame::new(image, Bounds::new(surface.rect.x, surface.rect.y, surface.rect.w, surface.rect.h, scale));
        frame.capture_ms = elapsed_ms;
        Ok(frame)
    }

    fn tap(&mut self, x: f64, y: f64, duration_ms: u32) -> DriverResult<()> {
        self.ensure_input_target(&[(x, y)])?;
        input::tap(x, y, duration_ms).map_err(input_error)
    }

    fn swipe(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, duration_ms: u32) -> DriverResult<()> {
        self.ensure_input_target(&[(x1, y1), (x2, y2)])?;
        input::swipe(x1, y1, x2, y2, duration_ms).map_err(input_error)
    }

    fn pointer(&mut self, action: PointerAction, x: f64, y: f64) -> DriverResult<()> {
        match action {
            // On Windows a drag is a move with the button already held.
            PointerAction::Move | PointerAction::Drag => input::move_to(x, y),
            PointerAction::Down => input::mouse_down(x, y),
            PointerAction::Up => input::mouse_up(x, y),
        }
        .map_err(input_error)
    }

    fn pointer_position(&mut self) -> DriverResult<Option<(f64, f64)>> {
        Ok(input::cursor_position())
    }

    fn check_input(&mut self, points: &[(f64, f64)]) -> DriverResult<()> {
        self.ensure_input_target(points)
    }

    fn key(&mut self, name: &str) -> DriverResult<()> {
        self.ensure_input_target(&[])?;
        input::key(name).map_err(input_error)
    }

    fn text(&mut self, value: &str) -> DriverResult<()> {
        self.ensure_input_target(&[])?;
        input::text(value).map_err(input_error)
    }

    fn ax_at(&mut self, x: f64, y: f64) -> DriverResult<Option<Rect>> {
        Ok(uia::element_at(x, y))
    }
}

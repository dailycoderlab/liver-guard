//! Target resolution: from a stored crop to a clickable point on this device.

use std::time::Instant;

use liverguard_ir::geometry::{point_in_box, py_round, roi_to_pixels, Rect};
use liverguard_ir::{Multi, PointTarget, Strategy, Target, TemplateTarget, Verify};
use opencv::core::Mat;
use opencv::prelude::*;

use crate::assets::AssetStore;
use crate::frame::Frame;
use crate::keypoint;
use crate::normalize::{template_factor, Normalized, CANONICAL_WIDTH};
use crate::tpl::{
    multi_scale_match, scale_ladder, single_scale_match, to_match_space, Match, LOW_TEXTURE_STD,
};
use crate::track::Tracker;
use crate::util::{crop, resize_by, std_dev, Result};
use crate::verify::{color_similarity, MIN_SIMILARITY};

#[derive(Debug, Clone)]
pub struct Hit {
    /// Matched control in global logical points.
    pub rect: Rect,
    /// Click point in global logical points, after anchor and jitter.
    pub point: (f64, f64),
    pub score: f64,
    pub strategy: String,
    pub elapsed_ms: f64,
    pub reused: bool,
    pub warnings: Vec<String>,
}

/// Anything that can turn a target into a clickable hit. The interpreter only
/// sees this trait, which keeps run logic testable without real matching.
pub trait Resolver {
    fn resolve(
        &mut self,
        target: &Target,
        target_id: &str,
        frame: &Frame,
        offset: (f64, f64),
    ) -> Result<Option<Hit>>;
}

impl Resolver for MatcherEngine {
    fn resolve(
        &mut self,
        target: &Target,
        target_id: &str,
        frame: &Frame,
        offset: (f64, f64),
    ) -> Result<Option<Hit>> {
        MatcherEngine::resolve(self, target, target_id, frame, offset)
    }
}

pub struct MatcherEngine {
    pub assets: AssetStore,
    pub tracker: Tracker,
    pub canonical_width: f64,
    pub allow_reuse: bool,
}

impl MatcherEngine {
    pub fn new(assets: AssetStore) -> Self {
        Self {
            assets,
            tracker: Tracker::default(),
            canonical_width: CANONICAL_WIDTH,
            allow_reuse: true,
        }
    }

    pub fn without_reuse(assets: AssetStore) -> Self {
        Self {
            allow_reuse: false,
            ..Self::new(assets)
        }
    }

    /// `offset` is a jitter as a fraction of the matched box, applied to the
    /// click point (also on the reuse path, otherwise only the first of several
    /// taps on one target would be offset).
    pub fn resolve(
        &mut self,
        target: &Target,
        target_id: &str,
        frame: &Frame,
        offset: (f64, f64),
    ) -> Result<Option<Hit>> {
        let started = Instant::now();
        match target {
            Target::Point(point) => Ok(resolve_point(point, frame, offset, started)),
            Target::Template(template) => {
                self.resolve_template(template, target_id, frame, offset, started)
            }
        }
    }

    fn resolve_template(
        &mut self,
        target: &TemplateTarget,
        target_id: &str,
        frame: &Frame,
        offset: (f64, f64),
        started: Instant,
    ) -> Result<Option<Hit>> {
        let mut warnings: Vec<String> = Vec::new();
        let frame_hash = frame.hash()?.to_string();

        if self.allow_reuse {
            if let Some(remembered) = self.tracker.reusable(target_id, &frame_hash) {
                let mut point = remembered.point;
                if offset != (0.0, 0.0) {
                    point = (
                        point.0 + offset.0 * remembered.rect.w,
                        point.1 + offset.1 * remembered.rect.h,
                    );
                }
                return Ok(Some(Hit {
                    rect: remembered.rect,
                    point,
                    score: remembered.score,
                    strategy: "reuse".to_string(),
                    elapsed_ms: elapsed_ms(started),
                    reused: true,
                    warnings,
                }));
            }
        }

        let normalized = frame.normalized(self.canonical_width)?;
        let factor = template_factor(
            target.record_width,
            target.record_scale,
            frame.bounds.w,
            self.canonical_width,
        );
        let template_raw = match self.assets.get_scaled(&target.asset, factor) {
            Ok(template) => template,
            Err(_) => return Ok(None),
        };

        if std_dev(&template_raw)? < LOW_TEXTURE_STD {
            warnings.push("low_texture_template".to_string());
        }

        let (search_raw, origin) = search_region(&normalized, target)?;
        if search_raw.empty() {
            return Ok(None);
        }

        let search_space = to_match_space(&search_raw, target.mask)?;
        let template_space = to_match_space(&template_raw, target.mask)?;

        let (matches, tried) = run_strategies(target, &search_space, &template_space)?;
        if matches.is_empty() {
            return Ok(None);
        }

        let scored = self.verify(target, matches, &search_raw, &template_raw, &mut warnings)?;
        let Some(chosen) = self.select(target, scored, &search_raw, target_id) else {
            return Ok(None);
        };

        let rect = normalized.rect_to_logical(
            origin.0 as f64 + chosen.x,
            origin.1 as f64 + chosen.y,
            chosen.w,
            chosen.h,
        );
        let mut point = point_in_box((target.anchor[0], target.anchor[1]), &rect);
        point = (point.0 + offset.0 * rect.w, point.1 + offset.1 * rect.h);

        if chosen.score < target.threshold {
            warnings.push("below_threshold".to_string());
        }

        let hit = Hit {
            rect,
            point,
            score: chosen.score,
            strategy: tried,
            elapsed_ms: elapsed_ms(started),
            reused: false,
            warnings,
        };
        self.tracker
            .remember(target_id, &frame_hash, hit.rect, hit.score, hit.point);
        Ok(Some(hit))
    }

    fn verify(
        &self,
        target: &TemplateTarget,
        matches: Vec<Match>,
        search_raw: &Mat,
        template_raw: &Mat,
        warnings: &mut Vec<String>,
    ) -> Result<Vec<Match>> {
        if target.verify != Verify::Rgb {
            return Ok(matches);
        }

        let mut kept = Vec::new();
        for m in &matches {
            let x0 = (py_round(m.x) as i32).max(0);
            let y0 = (py_round(m.y) as i32).max(0);
            let x1 = search_raw.cols().min(py_round(m.x + m.w) as i32);
            let y1 = search_raw.rows().min(py_round(m.y + m.h) as i32);
            if x1 <= x0 || y1 <= y0 {
                continue;
            }
            let patch = crop(search_raw, x0, y0, x1 - x0, y1 - y0)?;

            let resized_template = if (m.scale - 1.0).abs() > 0.01 {
                resize_by(template_raw, m.scale)?
            } else {
                template_raw.try_clone()?
            };

            let similarity = color_similarity(&patch, &resized_template);
            if similarity >= MIN_SIMILARITY {
                kept.push(*m);
            } else {
                warnings.push(format!("color_mismatch:{similarity:.2}"));
            }
        }

        if !kept.is_empty() {
            return Ok(kept);
        }

        // Nothing survived the colour check. Returning the geometric best with a
        // warning beats returning nothing: the runtime can still report a
        // low-confidence match instead of a hard failure.
        if let Some(first) = matches.first() {
            warnings.push("color_check_failed".to_string());
            return Ok(vec![*first]);
        }
        Ok(vec![])
    }

    fn select(
        &self,
        target: &TemplateTarget,
        matches: Vec<Match>,
        search: &Mat,
        target_id: &str,
    ) -> Option<Match> {
        if matches.is_empty() {
            return None;
        }
        let mut ordered = matches;
        ordered.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        match target.multi {
            Multi::Highest => return ordered.first().copied(),
            Multi::Nth => {
                if let Some(nth) = target.nth.filter(|n| *n > 0) {
                    let index = (nth as usize).min(ordered.len()) - 1;
                    return ordered.get(index).copied();
                }
            }
            Multi::Center => {
                let (cx, cy) = (search.cols() as f64 / 2.0, search.rows() as f64 / 2.0);
                return ordered.into_iter().min_by(|a, b| {
                    dist2(a.center(), (cx, cy))
                        .partial_cmp(&dist2(b.center(), (cx, cy)))
                        .unwrap()
                });
            }
            Multi::NearestLast => {}
        }

        // nearest-last: prefer the control the script clicked last time.
        if let Some(last) = self.tracker.last_rect(target_id) {
            let anchor = last.center();
            return ordered.into_iter().min_by(|a, b| {
                dist2(a.center(), anchor)
                    .partial_cmp(&dist2(b.center(), anchor))
                    .unwrap()
            });
        }
        ordered.first().copied()
    }
}

fn dist2(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn resolve_point(
    target: &PointTarget,
    frame: &Frame,
    offset: (f64, f64),
    started: Instant,
) -> Option<Hit> {
    let b = &frame.bounds;
    let x = b.x + target.norm[0] * b.w;
    let y = b.y + target.norm[1] * b.h;
    let zone = target.zone;
    let rect = Rect::new(
        b.x + zone[0] * b.w,
        b.y + zone[1] * b.h,
        zone[2] * b.w,
        zone[3] * b.h,
    );
    if !rect.contains(x, y) {
        return None;
    }
    Some(Hit {
        rect,
        point: (x + offset.0 * rect.w, y + offset.1 * rect.h),
        score: 1.0,
        strategy: "point".to_string(),
        elapsed_ms: elapsed_ms(started),
        reused: false,
        warnings: vec!["no_visual_anchor".to_string()],
    })
}

fn search_region(normalized: &Normalized, target: &TemplateTarget) -> Result<(Mat, (i32, i32))> {
    let (x, y, w, h) = roi_to_pixels(&target.roi, normalized.width, normalized.height);
    Ok((crop(&normalized.image, x, y, w, h)?, (x, y)))
}

fn run_strategies(
    target: &TemplateTarget,
    search: &Mat,
    template: &Mat,
) -> Result<(Vec<Match>, String)> {
    let mut tried: Vec<&str> = Vec::new();
    let mut matches = Vec::new();
    let floor = target.warn_threshold.max(0.4);

    for strategy in &target.strategy {
        match strategy {
            Strategy::Tpl => {
                tried.push("tpl");
                matches = single_scale_match(search, template, floor, 3)?;
            }
            Strategy::Mstpl => {
                tried.push("mstpl");
                matches =
                    multi_scale_match(search, template, &scale_ladder(&target.scale), floor, 3)?;
            }
            Strategy::Sift => {
                tried.push("sift");
                matches = keypoint::feature_match(search, template, 0.5)?;
            }
        }
        if !matches.is_empty() {
            break;
        }
    }
    Ok((matches, tried.join("+")))
}

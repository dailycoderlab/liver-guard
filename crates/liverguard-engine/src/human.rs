//! Human-like pointer behaviour: where a click lands, how long it is held, how the
//! mouse travels to the target and how a drag is drawn.
//!
//! Everything here is pure planning on top of an RNG: it returns a list of timed
//! raw pointer events and never sleeps or touches a driver, so it is platform
//! independent and testable with a seeded RNG. The interpreter plays the plan.
//!
//! The model, in one place:
//! * landing point: 2D normal around the anchor, `sigma = size / 4.133 * spread`,
//!   cut at the box edge ([`human_point`]); `spread` is the per-node "分散度",
//!   1.0 by default;
//! * press duration: log-normal around the configured value ([`hold_ms`]);
//! * travel to the target: cubic Bezier arc with a minimum-jerk speed profile, a
//!   Fitts-law duration, hand tremor, and sometimes an overshoot that is then
//!   corrected ([`plan_click`]);
//! * a short hesitation between arriving and pressing;
//! * drags: jittered start and end, the same kind of arc and speed profile, a pause
//!   after pressing and before releasing ([`plan_drag`]).

use liverguard_ir::{Rect, DEFAULT_SPREAD, MAX_SPREAD, MIN_SPREAD};
use rand::Rng;

use crate::driver::PointerAction;

/// Width of a normal distribution that holds ~96% of its mass, in standard
/// deviations: the "effective target width" `We = 4.133 * sigma` from Fitts' law
/// (MacKenzie, ISO 9241-9). Human pointing endpoints scatter this way, so a
/// target of width `W` is hit with `sigma = W / 4.133`.
const FITTS_WIDTH_IN_SIGMAS: f64 = 4.133;

/// Resampling budget before falling back to clamping (a full box accepts ~92%
/// of draws, so exhausting this is vanishingly rare).
const HUMAN_POINT_TRIES: usize = 16;

/// Time between two pointer events along a path, like a ~125 Hz mouse.
const EVENT_INTERVAL_MS: f64 = 8.0;

/// Travel shorter than this is not drawn (the pointer is already there).
const MIN_TRAVEL_PX: f64 = 1.5;

/// One raw pointer event and how long to wait after sending it.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerStep {
    pub action: PointerAction,
    pub x: f64,
    pub y: f64,
    pub wait_ms: f64,
}

/// A timed sequence of pointer events plus the numbers worth logging.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub steps: Vec<PointerStep>,
    /// Time spent moving to the target before pressing.
    pub travel_ms: f64,
    /// How long the button is held (a click's press, or a drag's whole stroke).
    pub hold_ms: f64,
    /// The mouse ran past the target and came back.
    pub overshoot: bool,
}

impl Plan {
    /// Points where a button is pressed, held or released: the ones that must be
    /// allowed by the driver's input check. Hover moves are not included.
    pub fn press_points(&self) -> Vec<(f64, f64)> {
        self.steps
            .iter()
            .filter(|s| s.action != PointerAction::Move)
            .map(|s| (s.x, s.y))
            .collect()
    }

    fn push(&mut self, action: PointerAction, at: (f64, f64), wait_ms: f64) {
        self.steps.push(PointerStep { action, x: at.0, y: at.1, wait_ms });
    }

    /// Add `wait_ms` to the pause after the last event.
    fn linger(&mut self, wait_ms: f64) {
        if let Some(last) = self.steps.last_mut() {
            last.wait_ms += wait_ms;
        }
    }
}

// ----------------------------------------------------------------- landing

/// A spread setting made safe to use: non-numbers fall back to the default and
/// everything else is kept within the supported range.
pub fn clamp_spread(spread: f64) -> f64 {
    if spread.is_finite() {
        spread.clamp(MIN_SPREAD, MAX_SPREAD)
    } else {
        DEFAULT_SPREAD
    }
}

/// A click point scattered the way a human hand lands it: a 2D normal centred
/// on `aim` (the anchor), with `sigma = size / 4.133 * spread` per axis, so that
/// at the standard `spread` of 1.0 ~96% of raw draws fall inside `rect`. Draws
/// outside the box are discarded and redrawn, so the point never misses the
/// matched box (a large spread therefore flattens towards a uniform draw, it
/// does not leave the box). x and y are independent and scale with the box's own
/// width and height.
pub fn human_point<R: Rng + ?Sized>(rect: &Rect, aim: (f64, f64), spread: f64, rng: &mut R) -> (f64, f64) {
    let scale = clamp_spread(spread) / FITTS_WIDTH_IN_SIGMAS;
    let (sigma_x, sigma_y) = (rect.w * scale, rect.h * scale);
    let inside = |x: f64, y: f64| x >= rect.x && x <= rect.x + rect.w && y >= rect.y && y <= rect.y + rect.h;

    let mut point = aim;
    for _ in 0..HUMAN_POINT_TRIES {
        let (zx, zy) = standard_normal_pair(rng);
        point = (aim.0 + zx * sigma_x, aim.1 + zy * sigma_y);
        if inside(point.0, point.1) {
            return point;
        }
    }
    (point.0.clamp(rect.x, rect.x + rect.w), point.1.clamp(rect.y, rect.y + rect.h))
}

/// The end of a drag given only a vector: the nominal end point, shifted by a
/// normal error whose sigma is `3% * spread` of the drag length on each axis.
pub fn jitter_drag_end<R: Rng + ?Sized>(
    start: (f64, f64),
    nominal_end: (f64, f64),
    spread: f64,
    rng: &mut R,
) -> (f64, f64) {
    let length = ((nominal_end.0 - start.0).powi(2) + (nominal_end.1 - start.1).powi(2)).sqrt();
    let sigma = length * 0.03 * clamp_spread(spread);
    let (zx, zy) = standard_normal_pair(rng);
    (nominal_end.0 + zx * sigma, nominal_end.1 + zy * sigma)
}

// ------------------------------------------------------------------ timing

/// How long to hold the button for a nominal `base_ms`: log-normal around it
/// (sigma 0.22), kept within 0.6x..1.8x. Zero stays zero.
pub fn hold_ms<R: Rng + ?Sized>(base_ms: u32, rng: &mut R) -> f64 {
    if base_ms == 0 {
        return 0.0;
    }
    let base = base_ms as f64;
    lognormal(rng, base, 0.22, base * 0.6, base * 1.8).round().max(1.0)
}

/// Time to move `distance` onto a target `width` wide: Fitts' law
/// `120 + 110 * log2(1 + D / W)` ms, log-normal noise (sigma 0.15), 120..1500 ms.
pub fn travel_ms<R: Rng + ?Sized>(distance: f64, width: f64, rng: &mut R) -> f64 {
    let index_of_difficulty = (1.0 + distance / width.max(8.0)).log2();
    lognormal(rng, 120.0 + 110.0 * index_of_difficulty, 0.15, 120.0, 1500.0)
}

fn lognormal<R: Rng + ?Sized>(rng: &mut R, median: f64, sigma: f64, lo: f64, hi: f64) -> f64 {
    let z = standard_normal_pair(rng).0;
    (median * (sigma * z).exp()).clamp(lo, hi)
}

// ------------------------------------------------------------------- plans

/// Move to `to` (from `from`, when the pointer position is known), hesitate, then
/// press and release after a jittered `hold_base_ms`. `width` is the target's
/// smaller side, which sets how long the approach takes.
pub fn plan_click<R: Rng + ?Sized>(
    from: Option<(f64, f64)>,
    to: (f64, f64),
    width: f64,
    hold_base_ms: u32,
    rng: &mut R,
) -> Plan {
    let mut plan = Plan::default();
    approach(&mut plan, from, to, width, rng);

    plan.hold_ms = hold_ms(hold_base_ms, rng);
    plan.push(PointerAction::Down, to, plan.hold_ms);
    plan.push(PointerAction::Up, to, 0.0);
    plan
}

/// Move to `start`, hesitate, press, pause, drag to `end` over a jittered
/// `duration_base_ms` along an arc, pause, release.
pub fn plan_drag<R: Rng + ?Sized>(
    from: Option<(f64, f64)>,
    start: (f64, f64),
    start_width: f64,
    end: (f64, f64),
    duration_base_ms: u32,
    rng: &mut R,
) -> Plan {
    let mut plan = Plan::default();
    approach(&mut plan, from, start, start_width, rng);

    // Hands pause a beat after the button goes down before the stroke begins.
    plan.push(PointerAction::Down, start, lognormal(rng, 60.0, 0.35, 20.0, 200.0));

    let base = duration_base_ms.max(50) as f64;
    let duration = lognormal(rng, base, 0.12, base * 0.8, base * 1.35);
    plan.hold_ms = duration;
    for (x, y, wait_ms) in curve(start, end, duration, 0.05, 0.5, rng) {
        plan.push(PointerAction::Drag, (x, y), wait_ms);
    }

    // ...and rest briefly at the end before letting go.
    plan.linger(lognormal(rng, 45.0, 0.4, 15.0, 150.0));
    plan.push(PointerAction::Up, end, 0.0);
    plan
}

/// Hover moves up to `to`, ending with a hesitation before the press. With an
/// unknown start the pointer simply appears at `to`.
fn approach<R: Rng + ?Sized>(plan: &mut Plan, from: Option<(f64, f64)>, to: (f64, f64), width: f64, rng: &mut R) {
    if let Some(from) = from {
        let distance = dist(from, to);
        if distance >= MIN_TRAVEL_PX {
            travel(plan, from, to, distance, width, rng);
        }
    }
    if plan.steps.is_empty() {
        plan.push(PointerAction::Move, to, 0.0);
    }
    plan.linger(lognormal(rng, 70.0, 0.35, 25.0, 250.0));
}

/// A mouse trip to `to`. Long trips sometimes overshoot by 1.5-5% (4..30 px) and
/// come back with a short corrective move, as ballistic hand movements do.
fn travel<R: Rng + ?Sized>(plan: &mut Plan, from: (f64, f64), to: (f64, f64), distance: f64, width: f64, rng: &mut R) {
    let total = travel_ms(distance, width, rng);
    plan.travel_ms = total;

    let mut legs: Vec<((f64, f64), (f64, f64), f64, f64)> = Vec::new(); // from, to, ms, bend
    if distance >= 220.0 && rng.random_bool(0.3) {
        let past = (distance * rng.random_range(0.015..0.05)).clamp(4.0, 30.0);
        let (ux, uy) = ((to.0 - from.0) / distance, (to.1 - from.1) / distance);
        let aim = (to.0 + ux * past, to.1 + uy * past);
        let fix_ms = lognormal(rng, 110.0, 0.25, 70.0, 220.0);
        legs.push((from, aim, total * 0.9, 0.12));
        legs.push((aim, to, fix_ms, 0.02));
        plan.overshoot = true;
        plan.travel_ms = total * 0.9 + fix_ms;
    } else {
        legs.push((from, to, total, 0.12));
    }

    for (leg_from, leg_to, ms, bend) in legs {
        for (x, y, wait_ms) in curve(leg_from, leg_to, ms, bend, 0.35, rng) {
            plan.push(PointerAction::Move, (x, y), wait_ms);
        }
    }
}

// ------------------------------------------------------------------- curve

/// Points of an arc from `from` to `to` taking `duration_ms`, as `(x, y, wait_ms)`.
///
/// The path is a cubic Bezier whose control points sit off the straight line by up
/// to `bend` x length (at most 120 px) on one side, sometimes crossing over to the
/// other for an S shape. Position along it follows the minimum-jerk profile
/// (`10t^3 - 15t^4 + 6t^5`): slow start, fast middle, slow arrival. A zero-mean
/// tremor of `tremor_px` sigma fades in and out and is absent at both ends, so the
/// last point is exactly `to`. Events are about `EVENT_INTERVAL_MS` apart with a
/// +-25% wobble; points that did not move are merged into the previous wait.
fn curve<R: Rng + ?Sized>(
    from: (f64, f64),
    to: (f64, f64),
    duration_ms: f64,
    bend: f64,
    tremor_px: f64,
    rng: &mut R,
) -> Vec<(f64, f64, f64)> {
    let length = dist(from, to);
    if length < 0.5 {
        return vec![(to.0, to.1, duration_ms)];
    }
    let (ux, uy) = ((to.0 - from.0) / length, (to.1 - from.1) / length);
    let (px, py) = (-uy, ux);

    let side = if rng.random_bool(0.5) { 1.0 } else { -1.0 };
    let side2 = if rng.random_bool(0.2) { -side } else { side };
    let bend1 = side * (length * bend * rng.random_range(0.4..1.0)).min(120.0);
    let bend2 = side2 * (length * bend * rng.random_range(0.0..0.7)).min(120.0);
    let along1 = length * rng.random_range(0.2..0.4);
    let along2 = length * rng.random_range(0.6..0.85);
    let p0 = from;
    let p1 = (from.0 + ux * along1 + px * bend1, from.1 + uy * along1 + py * bend1);
    let p2 = (from.0 + ux * along2 + px * bend2, from.1 + uy * along2 + py * bend2);
    let p3 = to;

    let count = ((duration_ms / EVENT_INTERVAL_MS).round() as usize).clamp(3, 400);
    let weights: Vec<f64> = (0..count).map(|_| rng.random_range(0.75..1.25)).collect();
    let weight_sum: f64 = weights.iter().sum();

    let mut out: Vec<(f64, f64, f64)> = Vec::with_capacity(count);
    let mut last = from;
    for (i, weight) in weights.iter().enumerate() {
        let wait = duration_ms * weight / weight_sum;
        let last_event = i + 1 == count;
        let t = (i + 1) as f64 / count as f64;
        let s = min_jerk(t);
        let mut point = bezier(p0, p1, p2, p3, s);
        if last_event {
            point = to;
        } else if tremor_px > 0.0 {
            let fade = (std::f64::consts::PI * t).sin();
            let (zx, zy) = standard_normal_pair(rng);
            point = (point.0 + zx * tremor_px * fade, point.1 + zy * tremor_px * fade);
        }
        if !last_event && dist(point, last) < 0.3 {
            if let Some(prev) = out.last_mut() {
                prev.2 += wait;
                continue;
            }
        }
        out.push((point.0, point.1, wait));
        last = point;
    }
    out
}

fn min_jerk(t: f64) -> f64 {
    t * t * t * (10.0 - 15.0 * t + 6.0 * t * t)
}

fn bezier(p0: (f64, f64), p1: (f64, f64), p2: (f64, f64), p3: (f64, f64), s: f64) -> (f64, f64) {
    let u = 1.0 - s;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * s, 3.0 * u * s * s, s * s * s);
    (a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1)
}

fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

/// Two independent standard-normal samples (Box–Muller transform).
fn standard_normal_pair<R: Rng + ?Sized>(rng: &mut R) -> (f64, f64) {
    let u1 = 1.0 - rng.random::<f64>(); // (0, 1], so ln() is finite
    let u2 = rng.random::<f64>();
    let radius = (-2.0 * u1.ln()).sqrt();
    let angle = std::f64::consts::TAU * u2;
    (radius * angle.cos(), radius * angle.sin())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    fn rng(seed: u64) -> StdRng {
        StdRng::seed_from_u64(seed)
    }

    // ---- landing point

    fn samples(rect: &Rect, aim: (f64, f64), n: usize) -> Vec<(f64, f64)> {
        samples_with(rect, aim, 1.0, n)
    }

    fn samples_with(rect: &Rect, aim: (f64, f64), spread: f64, n: usize) -> Vec<(f64, f64)> {
        let mut rng = rng(7);
        (0..n).map(|_| human_point(rect, aim, spread, &mut rng)).collect()
    }

    #[test]
    fn every_point_stays_inside_the_box() {
        let rect = Rect::new(100.0, 200.0, 80.0, 30.0);
        for (x, y) in samples(&rect, (140.0, 215.0), 20_000) {
            assert!((100.0..=180.0).contains(&x) && (200.0..=230.0).contains(&y), "({x}, {y})");
        }
    }

    #[test]
    fn points_cluster_around_the_anchor_unlike_a_uniform_draw() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        let pts = samples(&rect, (50.0, 20.0), 20_000);
        let n = pts.len() as f64;

        let mean_x = pts.iter().map(|p| p.0).sum::<f64>() / n;
        let mean_y = pts.iter().map(|p| p.1).sum::<f64>() / n;
        assert!((mean_x - 50.0).abs() < 0.5 && (mean_y - 20.0).abs() < 0.3, "({mean_x}, {mean_y})");

        // Middle half of the width: 50% for a uniform draw, ~73% for the truncated normal.
        let middle = pts.iter().filter(|p| (p.0 - 50.0).abs() <= 25.0).count() as f64 / n;
        assert!(middle > 0.68 && middle < 0.78, "{middle}");

        // Still wanders: not pinned to one spot.
        assert!(pts.iter().any(|p| (p.0 - 50.0).abs() > 30.0));
    }

    #[test]
    fn scatter_scales_with_each_axis_of_the_box() {
        let rect = Rect::new(0.0, 0.0, 200.0, 20.0);
        let pts = samples(&rect, (100.0, 10.0), 20_000);
        let spread = |f: fn(&(f64, f64)) -> f64, mean: f64| {
            (pts.iter().map(|p| (f(p) - mean).powi(2)).sum::<f64>() / pts.len() as f64).sqrt()
        };
        let (sx, sy) = (spread(|p| p.0, 100.0), spread(|p| p.1, 10.0));
        // sigma = size / 4.133, shrunk to ~0.89x by the cut at the box edge.
        assert!((sx / 200.0 - 0.215).abs() < 0.01, "{sx}");
        assert!((sy / 20.0 - 0.215).abs() < 0.01, "{sy}");
    }

    #[test]
    fn an_off_centre_anchor_still_lands_inside_the_box() {
        let rect = Rect::new(0.0, 0.0, 60.0, 60.0);
        for corner in [(0.0, 0.0), (60.0, 0.0), (0.0, 60.0), (60.0, 60.0)] {
            for (x, y) in samples(&rect, corner, 2_000) {
                assert!((0.0..=60.0).contains(&x) && (0.0..=60.0).contains(&y), "({x}, {y})");
            }
        }
    }

    #[test]
    fn a_degenerate_box_returns_its_only_point() {
        let rect = Rect::new(10.0, 10.0, 0.0, 0.0);
        assert_eq!(human_point(&rect, (10.0, 10.0), 1.0, &mut rng(1)), (10.0, 10.0));
    }

    fn std_dev_x(pts: &[(f64, f64)], mean: f64) -> f64 {
        (pts.iter().map(|p| (p.0 - mean).powi(2)).sum::<f64>() / pts.len() as f64).sqrt()
    }

    #[test]
    fn a_larger_spread_scatters_wider_and_a_smaller_one_tighter() {
        let rect = Rect::new(0.0, 0.0, 200.0, 60.0);
        let at = |spread| std_dev_x(&samples_with(&rect, (100.0, 30.0), spread, 20_000), 100.0);
        let (tight, standard, wide) = (at(0.5), at(1.0), at(1.5));
        assert!(tight < standard && standard < wide, "{tight} {standard} {wide}");
        // Half the spread is a bit over half the scatter: the standard one is cut by the
        // box edge more than the tight one.
        assert!((0.5..0.62).contains(&(tight / standard)), "{}", tight / standard);
    }

    #[test]
    fn the_standard_spread_is_the_original_behaviour() {
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(samples(&rect, (50.0, 20.0), 500), samples_with(&rect, (50.0, 20.0), 1.0, 500));
        assert_eq!(DEFAULT_SPREAD, 1.0);
    }

    #[test]
    fn points_stay_inside_the_box_at_every_spread() {
        let rect = Rect::new(100.0, 200.0, 80.0, 30.0);
        for spread in [MIN_SPREAD, 1.0, MAX_SPREAD, 50.0, f64::NAN] {
            for (x, y) in samples_with(&rect, (170.0, 205.0), spread, 5_000) {
                assert!((100.0..=180.0).contains(&x) && (200.0..=230.0).contains(&y), "spread {spread}: ({x}, {y})");
            }
        }
    }

    #[test]
    fn an_out_of_range_spread_is_pulled_back_in() {
        assert_eq!(clamp_spread(0.0), MIN_SPREAD);
        assert_eq!(clamp_spread(-3.0), MIN_SPREAD);
        assert_eq!(clamp_spread(99.0), MAX_SPREAD);
        assert_eq!(clamp_spread(1.3), 1.3);
        assert_eq!(clamp_spread(f64::NAN), DEFAULT_SPREAD);
        assert_eq!(clamp_spread(f64::INFINITY), DEFAULT_SPREAD);
    }

    #[test]
    fn drag_end_jitter_follows_the_spread_too() {
        let sd = |spread| {
            let mut r = rng(8);
            let pts: Vec<(f64, f64)> = (0..20_000).map(|_| jitter_drag_end((0.0, 0.0), (0.0, -300.0), spread, &mut r)).collect();
            std_dev_x(&pts, 0.0)
        };
        assert!((sd(2.0) / sd(1.0) - 2.0).abs() < 0.1);
    }

    // ---- timing

    #[test]
    fn hold_time_varies_around_the_base_within_bounds() {
        let mut r = rng(3);
        let holds: Vec<f64> = (0..5_000).map(|_| hold_ms(100, &mut r)).collect();
        assert!(holds.iter().all(|h| (60.0..=180.0).contains(h)), "{holds:?}");
        let mut sorted = holds.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = sorted[sorted.len() / 2];
        assert!((median - 100.0).abs() < 4.0, "{median}");
        assert!(sorted[sorted.len() - 1] - sorted[0] > 40.0, "no variation");
        assert!(holds.iter().all(|h| h.fract() == 0.0), "whole milliseconds");
    }

    #[test]
    fn a_zero_hold_stays_zero_and_a_short_one_never_rounds_to_nothing() {
        assert_eq!(hold_ms(0, &mut rng(1)), 0.0);
        assert!((0..500).all(|seed| hold_ms(1, &mut rng(seed)) >= 1.0));
    }

    #[test]
    fn farther_and_smaller_targets_take_longer_to_reach() {
        let med = |distance: f64, width: f64| {
            let mut r = rng(11);
            let mut v: Vec<f64> = (0..501).map(|_| travel_ms(distance, width, &mut r)).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[250]
        };
        assert!(med(800.0, 60.0) > med(100.0, 60.0));
        assert!(med(400.0, 12.0) > med(400.0, 120.0));
        // 500 px onto a 60 px button: 120 + 110 * log2(1 + 500/60) = ~470 ms.
        assert!((med(500.0, 60.0) - 470.0).abs() < 40.0, "{}", med(500.0, 60.0));
    }

    // ---- the path itself

    fn path_of(plan: &Plan, action: PointerAction) -> Vec<(f64, f64)> {
        plan.steps.iter().filter(|s| s.action == action).map(|s| (s.x, s.y)).collect()
    }

    #[test]
    fn a_click_moves_in_many_small_hops_and_ends_exactly_on_the_point() {
        let plan = plan_click(Some((100.0, 700.0)), (600.0, 300.0), 60.0, 60, &mut rng(5));
        let moves = path_of(&plan, PointerAction::Move);
        assert!(moves.len() > 20, "{}", moves.len());
        assert_eq!(*moves.last().unwrap(), (600.0, 300.0));

        let kinds: Vec<_> = plan.steps.iter().map(|s| s.action).collect();
        assert_eq!(&kinds[kinds.len() - 2..], &[PointerAction::Down, PointerAction::Up]);
        assert!(kinds[..kinds.len() - 2].iter().all(|k| *k == PointerAction::Move));

        // The press and release are at the same place, and the press is held.
        let n = plan.steps.len();
        assert_eq!((plan.steps[n - 2].x, plan.steps[n - 2].y), (600.0, 300.0));
        assert_eq!((plan.steps[n - 1].x, plan.steps[n - 1].y), (600.0, 300.0));
        assert_eq!(plan.steps[n - 2].wait_ms, plan.hold_ms);
        assert!(plan.hold_ms >= 36.0 && plan.hold_ms <= 108.0);
    }

    #[test]
    fn the_path_is_an_arc_not_a_straight_line() {
        // Perpendicular distance of the midpoint-ish samples from the chord.
        let mut bent = 0;
        for seed in 0..40 {
            let plan = plan_click(Some((0.0, 0.0)), (600.0, 0.0), 60.0, 60, &mut rng(seed));
            let off = path_of(&plan, PointerAction::Move).iter().map(|p| p.1.abs()).fold(0.0, f64::max);
            if off > 8.0 {
                bent += 1;
            }
        }
        assert!(bent >= 30, "only {bent}/40 paths bent away from the straight line");
    }

    #[test]
    fn speed_is_slow_at_both_ends_and_fast_in_the_middle() {
        let plan = plan_click(Some((0.0, 0.0)), (400.0, 0.0), 60.0, 60, &mut rng(9));
        let times_and_points: Vec<(f64, (f64, f64))> = {
            let mut t = 0.0;
            let mut v = vec![];
            for s in plan.steps.iter().filter(|s| s.action == PointerAction::Move) {
                t += s.wait_ms;
                v.push((t, (s.x, s.y)));
            }
            v
        };
        let n = times_and_points.len();
        let speed = |a: usize, b: usize| {
            let (ta, pa) = times_and_points[a];
            let (tb, pb) = times_and_points[b];
            dist(pa, pb) / (tb - ta).max(1e-6)
        };
        let ends = speed(1, 1 + n / 8).max(speed(n - 1 - n / 8, n - 1));
        let middle = speed(n / 2 - n / 16, n / 2 + n / 16);
        assert!(middle > ends * 2.0, "middle {middle} vs ends {ends}");
    }

    #[test]
    fn the_total_travel_time_matches_the_waits() {
        for seed in 0..20 {
            let plan = plan_click(Some((0.0, 0.0)), (300.0, 200.0), 50.0, 60, &mut rng(seed));
            let moved: f64 = plan.steps.iter().filter(|s| s.action == PointerAction::Move).map(|s| s.wait_ms).sum();
            // Moves also carry the hesitation (25..250 ms) after arriving.
            assert!(moved >= plan.travel_ms - 1e-6 && moved <= plan.travel_ms + 250.0 + 1e-6, "{moved} vs {}", plan.travel_ms);
        }
    }

    #[test]
    fn long_trips_sometimes_overshoot_and_come_back_short_ones_never_do() {
        let overshoots = |from: (f64, f64), to: (f64, f64)| {
            (0..400).filter(|seed| plan_click(Some(from), to, 60.0, 60, &mut rng(*seed)).overshoot).count()
        };
        let long = overshoots((0.0, 0.0), (900.0, 0.0));
        assert!((80..=160).contains(&long), "{long}/400 overshot");
        assert_eq!(overshoots((0.0, 0.0), (150.0, 0.0)), 0);

        // An overshoot really goes past the target, by 4..30 px, and then returns.
        let plan = (0..400)
            .map(|seed| plan_click(Some((0.0, 0.0)), (900.0, 0.0), 60.0, 60, &mut rng(seed)))
            .find(|p| p.overshoot)
            .unwrap();
        let xs: Vec<f64> = path_of(&plan, PointerAction::Move).iter().map(|p| p.0).collect();
        let max = xs.iter().cloned().fold(f64::MIN, f64::max);
        assert!(max > 903.9 && max < 931.0, "{max}");
        assert_eq!(*xs.last().unwrap(), 900.0);
    }

    #[test]
    fn already_on_target_means_no_travel() {
        let plan = plan_click(Some((50.0, 50.0)), (50.5, 50.2), 60.0, 60, &mut rng(2));
        assert_eq!(plan.travel_ms, 0.0);
        assert_eq!(path_of(&plan, PointerAction::Move), vec![(50.5, 50.2)]);
    }

    #[test]
    fn an_unknown_start_appears_at_the_target_instead_of_inventing_a_path() {
        let plan = plan_click(None, (10.0, 20.0), 60.0, 60, &mut rng(2));
        assert_eq!(path_of(&plan, PointerAction::Move), vec![(10.0, 20.0)]);
    }

    #[test]
    fn it_hesitates_between_arriving_and_pressing() {
        let mut waits = vec![];
        for seed in 0..200 {
            let plan = plan_click(None, (10.0, 20.0), 60.0, 60, &mut rng(seed));
            waits.push(plan.steps[0].wait_ms);
        }
        assert!(waits.iter().all(|w| (25.0..=250.0).contains(w)));
        assert!(waits.iter().cloned().fold(f64::MAX, f64::min) < 60.0);
        assert!(waits.iter().cloned().fold(f64::MIN, f64::max) > 90.0);
    }

    #[test]
    fn a_drag_presses_strokes_along_an_arc_and_releases_at_the_end() {
        let plan = plan_drag(Some((50.0, 50.0)), (200.0, 500.0), 60.0, (200.0, 200.0), 300, &mut rng(4));
        let kinds: Vec<_> = plan.steps.iter().map(|s| s.action).collect();
        let down = kinds.iter().position(|k| *k == PointerAction::Down).unwrap();
        assert!(kinds[..down].iter().all(|k| *k == PointerAction::Move));
        assert!(kinds[down + 1..kinds.len() - 1].iter().all(|k| *k == PointerAction::Drag));
        assert_eq!(*kinds.last().unwrap(), PointerAction::Up);
        assert_eq!(plan.steps.iter().filter(|s| s.action == PointerAction::Down).count(), 1);

        assert_eq!((plan.steps[down].x, plan.steps[down].y), (200.0, 500.0));
        let up = plan.steps.last().unwrap();
        assert_eq!((up.x, up.y), (200.0, 200.0));
        let drags = path_of(&plan, PointerAction::Drag);
        assert!(drags.len() > 15, "{}", drags.len());
        assert_eq!(*drags.last().unwrap(), (200.0, 200.0));
        // Stroke time is the jittered duration: 0.8x..1.35x of 300 ms.
        assert!(plan.hold_ms >= 240.0 && plan.hold_ms <= 405.0, "{}", plan.hold_ms);
    }

    #[test]
    fn drag_durations_vary_from_run_to_run() {
        let durations: Vec<f64> = (0..100)
            .map(|seed| plan_drag(None, (0.0, 0.0), 60.0, (0.0, 300.0), 300, &mut rng(seed)).hold_ms)
            .collect();
        let spread = durations.iter().cloned().fold(f64::MIN, f64::max) - durations.iter().cloned().fold(f64::MAX, f64::min);
        assert!(spread > 30.0, "{spread}");
    }

    #[test]
    fn only_pressed_events_are_input_checked() {
        let plan = plan_click(Some((0.0, 0.0)), (300.0, 300.0), 60.0, 60, &mut rng(1));
        assert_eq!(plan.press_points(), vec![(300.0, 300.0), (300.0, 300.0)]);
    }

    #[test]
    fn drag_end_jitter_is_centred_and_proportional_to_the_length() {
        let mut r = rng(8);
        let n = 20_000;
        let pts: Vec<(f64, f64)> = (0..n).map(|_| jitter_drag_end((0.0, 0.0), (0.0, -300.0), 1.0, &mut r)).collect();
        let mx = pts.iter().map(|p| p.0).sum::<f64>() / n as f64;
        let my = pts.iter().map(|p| p.1).sum::<f64>() / n as f64;
        assert!(mx.abs() < 0.3 && (my + 300.0).abs() < 0.3, "({mx}, {my})");
        let sd = (pts.iter().map(|p| (p.0 - mx).powi(2)).sum::<f64>() / n as f64).sqrt();
        assert!((sd - 9.0).abs() < 0.5, "{sd}"); // 3% of 300 px
    }

    #[test]
    fn the_same_seed_gives_the_same_plan() {
        let a = plan_click(Some((0.0, 0.0)), (500.0, 500.0), 60.0, 60, &mut rng(42));
        let b = plan_click(Some((0.0, 0.0)), (500.0, 500.0), 60.0, 60, &mut rng(42));
        assert_eq!(a, b);
    }
}

//! A scaled ruler laid on the page: its graduations in real-world units at the page's drawing
//! scale, spaced for the zoom so they stay readable, like an architect's scale rule.

use serde::Serialize;

/// Most graduations one ruler draws.
pub const MAX_TICKS: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TickKind {
    Minor,
    Half,
    Major,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Tick {
    /// Along the ruler from its zero, in page points.
    pub at: f64,
    /// The real-world value it marks.
    pub value: f64,
    pub kind: TickKind,
}

/// The graduations of a ruler `length` points long, where one page point is `real_per_point`
/// real units and one point is `pixels_per_point` on screen. Minor graduations are at least
/// `min_gap` pixels apart, on a 1-2-5 step; every fifth or tenth is a major one, labelled.
pub fn ticks(length: f64, real_per_point: f64, pixels_per_point: f64, min_gap: f64) -> (f64, Vec<Tick>) {
    let ok = |v: f64| v.is_finite() && v > 0.0;
    if !ok(length) || !ok(real_per_point) || !ok(pixels_per_point) || !ok(min_gap) {
        return (0.0, Vec::new());
    }
    // Real units per pixel, times the gap: the smallest step allowed.
    let smallest = real_per_point / pixels_per_point * min_gap;
    let step = nice_step(smallest);
    if !ok(step) {
        return (0.0, Vec::new());
    }
    let major_every = if leading_digit(step) == 2 { 5 } else { 10 };
    let step_points = step / real_per_point;
    let count = ((length / step_points).floor() as usize).min(MAX_TICKS);
    let ticks = (0..=count)
        .map(|i| {
            let kind = if i % major_every == 0 {
                TickKind::Major
            } else if major_every == 10 && i % 5 == 0 {
                TickKind::Half
            } else {
                TickKind::Minor
            };
            Tick { at: i as f64 * step_points, value: round(i as f64 * step), kind }
        })
        .collect();
    (step, ticks)
}

/// The least of 1, 2 and 5 times a power of ten that is at least `at_least`.
pub fn nice_step(at_least: f64) -> f64 {
    if !at_least.is_finite() || at_least <= 0.0 {
        return 0.0;
    }
    let power = 10f64.powf(at_least.log10().floor());
    [1.0, 2.0, 5.0, 10.0].into_iter().map(|m| m * power).find(|s| *s >= at_least * (1.0 - 1e-12)).unwrap_or(10.0 * power)
}

fn leading_digit(step: f64) -> u8 {
    let power = 10f64.powf(step.log10().floor());
    (step / power).round() as u8
}

/// `v` without float noise (0.30000000000000004 → 0.3).
fn round(v: f64) -> f64 {
    let r = (v * 1e9).round() / 1e9;
    if r == 0.0 { 0.0 } else { r }
}

/// The point on the line through `origin` at `angle` radians nearest to `p`: what a pen held
/// against the ruler's edge draws.
pub fn project(p: [f64; 2], origin: [f64; 2], angle: f64) -> [f64; 2] {
    let (c, s) = (angle.cos(), angle.sin());
    let t = (p[0] - origin[0]) * c + (p[1] - origin[1]) * s;
    [origin[0] + t * c, origin[1] + t * s]
}

/// The distance from `p` to that line.
pub fn distance_to_edge(p: [f64; 2], origin: [f64; 2], angle: f64) -> f64 {
    let q = project(p, origin, angle);
    ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_follow_one_two_five() {
        assert_eq!(nice_step(0.7), 1.0);
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(1.2), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(6.0), 10.0);
        assert_eq!(nice_step(0.03), 0.05);
        assert_eq!(nice_step(0.0), 0.0);
        assert_eq!(nice_step(f64::NAN), 0.0);
    }

    #[test]
    fn a_one_to_hundred_plan_reads_in_metres() {
        // 1:100: one point (0.3528 mm) is 35.28 mm real = 0.03528 m.
        let real_per_point = 0.0254 / 72.0 * 100.0;
        // At 100 % zoom, 2 pixels per point, ticks at least 8 px apart.
        let (step, ticks) = ticks(72.0 / 0.0254 * 0.05, real_per_point, 2.0, 8.0);
        assert_eq!(step, 0.2);
        // A 5 cm long ruler on paper is 5 m at 1:100: 0, 0.2, … 5.0.
        assert_eq!(ticks.len(), 26);
        assert_eq!(ticks.last().map(|t| t.value), Some(5.0));
        assert_eq!(ticks[5].kind, TickKind::Major);
        assert_eq!(ticks[1].kind, TickKind::Minor);
    }

    #[test]
    fn zooming_in_refines_the_graduations() {
        let rpp = 0.0254 / 72.0 * 100.0;
        let (coarse, _) = ticks(500.0, rpp, 1.0, 8.0);
        let (fine, t) = ticks(500.0, rpp, 8.0, 8.0);
        assert!(fine < coarse);
        assert!(t.iter().any(|t| t.kind == TickKind::Half));
    }

    #[test]
    fn bad_inputs_give_no_ticks_and_counts_are_capped() {
        assert!(ticks(f64::INFINITY, 1.0, 1.0, 8.0).1.is_empty());
        assert!(ticks(100.0, 0.0, 1.0, 8.0).1.is_empty());
        assert!(ticks(100.0, 1.0, -1.0, 8.0).1.is_empty());
        assert_eq!(ticks(1e12, 1.0, 1e6, 1e-6).1.len(), MAX_TICKS + 1);
    }

    #[test]
    fn a_pen_held_to_the_edge_lands_on_it() {
        let p = project([3.0, 4.0], [0.0, 0.0], 0.0);
        assert_eq!(p, [3.0, 0.0]);
        let d = distance_to_edge([1.0, 1.0], [0.0, 0.0], std::f64::consts::FRAC_PI_4);
        assert!(d.abs() < 1e-12);
    }
}

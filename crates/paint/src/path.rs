//! Screen-space stroke path: lazy mouse, straight lines and resampling at the brush spacing.

use glam::Vec2;

/// Most stamps one segment of a stroke can make (far more than any screen or texture is long).
pub const MAX_SEGMENT_SAMPLES: usize = 1 << 18;

/// Largest smoothing weight (1 would freeze the brush).
pub const MAX_SMOOTHING: f32 = 0.95;

/// Speed-adaptive smoothing: cursor steps of this many view pixels or more get the least
/// smoothing ([`FAST_SMOOTHING_SHARE`] of the set amount).
pub const FAST_STEP_PX: f32 = 24.0;
/// Share of the smoothing left on the fastest strokes.
pub const FAST_SMOOTHING_SHARE: f32 = 0.2;

/// One resampled stamp position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathSample {
    pub pos: Vec2,
    /// Unit screen direction of travel (for follow-path angles).
    pub dir: Vec2,
}

#[derive(Clone, Debug)]
pub struct ScreenPath {
    /// Position of the last path vertex (where the brush is).
    last: Option<Vec2>,
    /// Distance travelled since the last emitted stamp.
    residual: f32,
    /// Lazy-mouse brush position.
    lazy: Option<Vec2>,
    /// Weighted-average smoothed cursor.
    smooth: Option<Vec2>,
    /// Latest raw cursor, the target of the catch-up end.
    cursor: Option<Vec2>,
    dir: Vec2,
    /// Speed-adaptive smoothing: the cursor step (in path units) that counts as fast.
    fast_step: Option<f32>,
}

impl Default for ScreenPath {
    fn default() -> Self {
        Self { last: None, residual: 0.0, lazy: None, smooth: None, cursor: None, dir: Vec2::X, fast_step: None }
    }
}

impl ScreenPath {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a cursor sample. `smoothing` (0..1) is the weight of the running average of earlier
    /// cursor samples (0 = off). With `lazy_px > 0` the brush hangs on a rope of that length: it
    /// trails the (smoothed) cursor and only moves once the rope is taut.
    pub fn push(&mut self, cursor: Vec2, spacing_px: f32, lazy_px: f32, smoothing: f32, out: &mut Vec<PathSample>) {
        let step = self.cursor.map_or(0.0, |c| c.distance(cursor));
        self.cursor = Some(cursor);
        let mut w = smoothing.clamp(0.0, MAX_SMOOTHING);
        if let Some(fast) = self.fast_step.filter(|f| *f > 0.0) {
            // Slow, shaky movement keeps the full smoothing; a fast sweep is meant as drawn.
            let t = (step / fast).clamp(0.0, 1.0);
            w *= 1.0 - t * (1.0 - FAST_SMOOTHING_SHARE);
        }
        let cursor = if w > 0.0 {
            let avg = *self.smooth.get_or_insert(cursor);
            let avg = avg + (cursor - avg) * (1.0 - w);
            self.smooth = Some(avg);
            avg
        } else {
            self.smooth = Some(cursor);
            cursor
        };
        let target = if lazy_px > 0.0 {
            let brush = *self.lazy.get_or_insert(cursor);
            let d = cursor - brush;
            let len = d.length();
            let moved = if len > lazy_px { brush + d * ((len - lazy_px) / len) } else { brush };
            self.lazy = Some(moved);
            moved
        } else {
            cursor
        };
        self.walk(target, spacing_px, out);
    }

    /// Speed-adaptive smoothing: `fast_step` is the cursor step (in the units of the pushed
    /// positions) at which smoothing drops to its least; `None` keeps it constant.
    pub fn set_adaptive(&mut self, fast_step: Option<f32>) {
        self.fast_step = fast_step;
    }

    /// Catch-up end: the brush glides the rest of the way to the last cursor position, so a
    /// smoothed or lazy stroke ends where the pen lifted.
    pub fn finish(&mut self, spacing_px: f32, out: &mut Vec<PathSample>) {
        if let Some(cursor) = self.cursor.take() {
            self.lazy = Some(cursor);
            self.smooth = Some(cursor);
            self.walk(cursor, spacing_px, out);
        }
    }

    /// Straight segment from the current brush position (ignores lazy mouse).
    pub fn line_to(&mut self, to: Vec2, spacing_px: f32, out: &mut Vec<PathSample>) {
        self.lazy = Some(to);
        self.smooth = Some(to);
        self.cursor = Some(to);
        self.walk(to, spacing_px, out);
    }

    fn walk(&mut self, to: Vec2, spacing_px: f32, out: &mut Vec<PathSample>) {
        let spacing = spacing_px.max(0.5);
        // A point at infinity has nowhere to put a stamp.
        if !to.is_finite() {
            return;
        }
        let Some(from) = self.last else {
            out.push(PathSample { pos: to, dir: self.dir });
            self.last = Some(to);
            self.residual = 0.0;
            return;
        };
        let seg = to - from;
        let len = seg.length();
        if len < 1e-6 {
            return;
        }
        let dir = seg / len;
        self.dir = dir;
        // Stamps by index, at most MAX_SEGMENT_SAMPLES: `t += spacing` stops growing once t is
        // large in f32, and a far point asked for endless stamps (a script drawing to a point
        // millions of texels away hung, then ran out of memory).
        let first = spacing - self.residual;
        let count = if first > len { 0 } else { (((len - first) / spacing).floor() as usize).saturating_add(1).min(MAX_SEGMENT_SAMPLES) };
        let mut last_emit = -self.residual;
        for k in 0..count {
            let t = first + k as f32 * spacing;
            out.push(PathSample { pos: from + dir * t, dir });
            last_emit = t;
        }
        self.residual = (len - last_emit).min(spacing);
        self.last = Some(to);
    }
}

/// Post-smoothing at full strength: the smoothing window reaches this many view pixels of
/// path along either side of each point.
pub const POST_SMOOTHING_PX: f32 = 60.0;

/// Post-smoothing (redraw the finished stroke along a smoothed path): each point becomes a
/// smooth (biweight) average of its neighbours within `radius` of path length on either side.
/// The window shrinks near the ends, so the first and last points stay where they were and the stroke
/// still starts and stops where it was drawn. Points are weighted by the path length they
/// stand for, so a slow part (dense events) does not pull harder than a fast one. Same count
/// and order out as in, so per-point pressure still lines up.
pub fn post_smooth(points: &[Vec2], radius: f32) -> Vec<Vec2> {
    if points.len() < 3 || !(radius.is_finite() && radius > 0.0) {
        return points.to_vec();
    }
    // Path length on a shaky path is mostly the shake: a wide zigzag 8 px apart has ~50 px of
    // path per point, so one pass would average only a couple of neighbours. Each pass measures
    // length on the previous, calmer result, so the window takes in more points every time.
    let mut out = points.to_vec();
    for _ in 0..POST_SMOOTHING_PASSES {
        out = post_smooth_pass(&out, radius);
    }
    out
}

/// Passes of [`post_smooth`].
const POST_SMOOTHING_PASSES: usize = 4;

fn post_smooth_pass(points: &[Vec2], radius: f32) -> Vec<Vec2> {
    let n = points.len();
    let mut s = vec![0.0f32; n];
    for i in 1..n {
        s[i] = s[i - 1] + points[i].distance(points[i - 1]);
    }
    let total = s[n - 1];
    // Path length each point stands for (half of each neighbouring segment).
    let ds: Vec<f32> = (0..n).map(|i| (s[(i + 1).min(n - 1)] - s[i.saturating_sub(1)]) * 0.5).collect();
    let mut out = Vec::with_capacity(n);
    let mut lo = 0;
    for i in 0..n {
        let r = radius.min(s[i]).min(total - s[i]);
        if r <= 1e-3 {
            out.push(points[i]);
            continue;
        }
        while s[lo] < s[i] - r {
            lo += 1;
        }
        let (mut sum, mut weight) = (Vec2::ZERO, 0.0f32);
        let mut j = lo;
        while j < n && s[j] <= s[i] + r {
            let d = (s[j] - s[i]) / r;
            let w = (1.0 - d * d).max(0.0).powi(2) * ds[j].max(1e-6);
            sum += points[j] * w;
            weight += w;
            j += 1;
        }
        out.push(if weight > 0.0 { sum / weight } else { points[i] });
    }
    out
}

/// Deterministic per-stamp random numbers in [0, 1).
#[inline]
pub fn rand01(seed: u32, stamp: u32, stream: u32) -> f32 {
    let mut x = (seed as u64) << 32 | stamp as u64;
    x ^= (stream as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    /// QA 2026-11: a segment to a very far or infinite point never ended.
    #[test]
    fn a_far_or_infinite_point_ends_the_segment() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        p.line_to(Vec2::ZERO, 1.0, &mut out);
        p.line_to(Vec2::new(f32::INFINITY, 0.0), 1.0, &mut out);
        assert_eq!(out.len(), 1, "nothing toward infinity");
        p.line_to(Vec2::new(1e30, 0.0), 1.0, &mut out);
        assert!(out.len() <= 1 + MAX_SEGMENT_SAMPLES && out.len() > 1, "{}", out.len());
        assert!(out.iter().all(|s| s.pos.is_finite()));
        // Ordinary strokes are unchanged.
        let mut q = ScreenPath::new();
        let mut short = Vec::new();
        q.line_to(Vec2::ZERO, 1.0, &mut short);
        q.line_to(Vec2::new(10.0, 0.0), 1.0, &mut short);
        assert_eq!(short.len(), 11);
        assert!((short[10].pos.x - 10.0).abs() < 1e-5);
    }

    #[test]
    fn resamples_at_constant_spacing_across_pushes() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        for x in [0.0, 3.0, 7.5, 20.0, 21.0, 40.0] {
            p.push(Vec2::new(x, 10.0), 5.0, 0.0, 0.0, &mut out);
        }
        let xs: Vec<f32> = out.iter().map(|s| s.pos.x).collect();
        assert_eq!(xs.len(), 9, "{xs:?}");
        for (i, x) in xs.iter().enumerate() {
            assert!((x - 5.0 * i as f32).abs() < 1e-4, "{xs:?}");
        }
    }

    #[test]
    fn lazy_mouse_trails_the_cursor() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        p.push(Vec2::ZERO, 1.0, 10.0, 0.0, &mut out);
        p.push(Vec2::new(8.0, 0.0), 1.0, 10.0, 0.0, &mut out);
        assert_eq!(out.len(), 1, "cursor within lazy radius: brush does not move");
        p.push(Vec2::new(30.0, 0.0), 1.0, 10.0, 0.0, &mut out);
        let last = out.last().unwrap().pos.x;
        assert!((last - 20.0).abs() < 1e-4, "brush stops 10px behind the cursor, got {last}");
    }

    #[test]
    fn smoothing_averages_and_catch_up_reaches_the_cursor() {
        let run = |catch_up: bool| {
            let mut p = ScreenPath::new();
            let mut out = Vec::new();
            for x in 0..=20 {
                // zig-zag cursor along x
                let y = if x % 2 == 0 { 4.0 } else { -4.0 };
                p.push(Vec2::new(x as f32 * 3.0, y), 1.0, 0.0, 0.8, &mut out);
            }
            let before = out.len();
            if catch_up { p.finish(1.0, &mut out); }
            (out, before)
        };
        let (out, before) = run(false);
        let max_y = out.iter().filter(|s| s.pos.x > 30.0).map(|s| s.pos.y.abs()).fold(0.0, f32::max);
        assert!(max_y < 1.0, "zig-zag is damped, got {max_y}");
        assert!(out.last().unwrap().pos.x < 59.0, "average trails the cursor");
        let (out2, _) = run(true);
        assert!(out2.len() > before);
        let end = out2.last().unwrap().pos;
        assert!((end - Vec2::new(60.0, 4.0)).length() < 1.0, "catch-up ends at the cursor, got {end}");
        assert_eq!(run(true).0, out2, "deterministic");
    }

    #[test]
    fn adaptive_smoothing_follows_fast_sweeps_and_steadies_slow_jitter() {
        // How far behind the cursor the brush is after a run of equal steps.
        let lag = |step: f32, adaptive: bool| {
            let mut p = ScreenPath::new();
            p.set_adaptive(adaptive.then_some(FAST_STEP_PX));
            let mut out = Vec::new();
            for i in 0..=10 {
                p.push(Vec2::new(i as f32 * step, 0.0), 0.5, 0.0, 0.8, &mut out);
            }
            10.0 * step - out.last().unwrap().pos.x
        };
        // Fast: much less lag than with constant smoothing.
        assert!(lag(40.0, true) < 0.35 * lag(40.0, false), "{} vs {}", lag(40.0, true), lag(40.0, false));
        // Slow (sub-pixel jitter scale): about the same smoothing as constant.
        assert!((lag(1.0, true) - lag(1.0, false)).abs() < 0.2 * lag(1.0, false));
        // Slow zig-zag stays damped.
        let mut p = ScreenPath::new();
        p.set_adaptive(Some(FAST_STEP_PX));
        let mut out = Vec::new();
        for x in 0..=40 {
            let y = if x % 2 == 0 { 1.5 } else { -1.5 };
            p.push(Vec2::new(x as f32 * 0.5, y), 0.25, 0.0, 0.8, &mut out);
        }
        let max_y = out.iter().filter(|s| s.pos.x > 8.0).map(|s| s.pos.y.abs()).fold(0.0, f32::max);
        assert!(max_y < 0.5, "slow jitter is damped: {max_y}");
    }

    #[test]
    fn straight_line_emits_evenly() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        p.line_to(Vec2::new(0.0, 0.0), 2.0, &mut out);
        p.line_to(Vec2::new(0.0, 10.0), 2.0, &mut out);
        assert_eq!(out.len(), 6);
        assert!(out.iter().all(|s| s.pos.x == 0.0));
        assert_eq!(out[3].dir, Vec2::Y);
    }

    #[test]
    fn random_is_deterministic_and_spread() {
        assert_eq!(rand01(1, 2, 3), rand01(1, 2, 3));
        let mean: f32 = (0..1000).map(|i| rand01(7, i, 0)).sum::<f32>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05);
        assert!((0..1000).all(|i| (0.0..1.0).contains(&rand01(7, i, 1))));
    }

    #[test]
    fn post_smoothing_irons_out_jitter_and_keeps_the_ends() {
        // A horizontal line with +-4 px zigzag every point, 2 px apart.
        let raw: Vec<Vec2> = (0..101).map(|i| Vec2::new(i as f32 * 2.0, if i % 2 == 0 { 4.0 } else { -4.0 })).collect();
        let out = post_smooth(&raw, 30.0);
        assert_eq!(out.len(), raw.len());
        assert_eq!(out[0], raw[0]);
        assert_eq!(out[100], raw[100]);
        // Away from the ends the zigzag is nearly gone and x keeps its order.
        assert!(out[20..80].iter().all(|p| p.y.abs() < 0.5), "{:?}", &out[20..30]);
        assert!(out.windows(2).all(|w| w[1].x >= w[0].x));
        // A clean straight line stays where it is; zero radius changes nothing.
        let line: Vec<Vec2> = (0..20).map(|i| Vec2::new(i as f32, 0.0)).collect();
        assert!(post_smooth(&line, 30.0).iter().zip(&line).all(|(a, b)| a.distance(*b) < 1e-3));
        assert_eq!(post_smooth(&raw, 0.0), raw);
        // A wide zigzag (+-25 px, 8 px apart: mostly up and down) still comes out nearly flat.
        let wide: Vec<Vec2> = (0..41).map(|i| Vec2::new(i as f32 * 8.0, if i == 0 || i == 40 { 0.0 } else if i % 2 == 0 { 25.0 } else { -25.0 })).collect();
        let out = post_smooth(&wide, POST_SMOOTHING_PX);
        assert!(out[8..33].iter().all(|p| p.y.abs() < 5.0), "{:?}", &out[8..33]);
    }
}

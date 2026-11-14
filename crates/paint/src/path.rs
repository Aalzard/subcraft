//! Screen-space stroke path: lazy mouse, straight lines and resampling at the brush spacing.

use glam::Vec2;

/// Most stamps one segment of a stroke can make (far more than any screen or texture is long).
pub const MAX_SEGMENT_SAMPLES: usize = 1 << 18;

/// Largest smoothing weight (1 would freeze the brush).
pub const MAX_SMOOTHING: f32 = 0.95;

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
}

impl Default for ScreenPath {
    fn default() -> Self {
        Self { last: None, residual: 0.0, lazy: None, smooth: None, cursor: None, dir: Vec2::X }
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
        self.cursor = Some(cursor);
        let w = smoothing.clamp(0.0, MAX_SMOOTHING);
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
}

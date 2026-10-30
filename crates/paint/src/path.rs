//! Screen-space stroke path: lazy mouse, straight lines and resampling at the brush spacing.

use glam::Vec2;

/// One resampled stamp position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathSample {
    pub pos: Vec2,
    /// Unit screen direction of travel (for follow-path angles).
    pub dir: Vec2,
    /// Pen pressure (0..1) at this stamp, interpolated between input samples; 1.0 for a mouse.
    pub pressure: f32,
}

#[derive(Clone, Debug)]
pub struct ScreenPath {
    /// Position of the last path vertex (where the brush is).
    last: Option<Vec2>,
    /// Pressure at the last path vertex.
    last_pressure: f32,
    /// Distance travelled since the last emitted stamp.
    residual: f32,
    /// Lazy-mouse brush position.
    lazy: Option<Vec2>,
    dir: Vec2,
}

impl Default for ScreenPath {
    fn default() -> Self {
        Self { last: None, last_pressure: 1.0, residual: 0.0, lazy: None, dir: Vec2::X }
    }
}

/// Clamp a device pressure to 0..1; missing / invalid readings count as full pressure.
#[inline]
pub fn sanitize_pressure(p: f32) -> f32 {
    if p.is_finite() { p.clamp(0.0, 1.0) } else { 1.0 }
}

impl ScreenPath {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a cursor sample at full pressure (mouse input).
    pub fn push(&mut self, cursor: Vec2, spacing_px: f32, lazy_px: f32, out: &mut Vec<PathSample>) {
        self.push_pressure(cursor, 1.0, spacing_px, lazy_px, out);
    }

    /// Feed a cursor sample with pen pressure. With `lazy_px > 0` the brush trails the cursor by
    /// that distance and only moves once the cursor pulls it (it takes the cursor's pressure).
    /// Stamps between two samples get linearly interpolated pressure.
    pub fn push_pressure(&mut self, cursor: Vec2, pressure: f32, spacing_px: f32, lazy_px: f32, out: &mut Vec<PathSample>) {
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
        self.walk(target, sanitize_pressure(pressure), spacing_px, out);
    }

    /// Straight segment from the current brush position (ignores lazy mouse), full pressure.
    pub fn line_to(&mut self, to: Vec2, spacing_px: f32, out: &mut Vec<PathSample>) {
        self.line_to_pressure(to, 1.0, spacing_px, out);
    }

    /// Straight segment ending at `pressure` (interpolated from the brush's current pressure).
    pub fn line_to_pressure(&mut self, to: Vec2, pressure: f32, spacing_px: f32, out: &mut Vec<PathSample>) {
        self.lazy = Some(to);
        self.walk(to, sanitize_pressure(pressure), spacing_px, out);
    }

    fn walk(&mut self, to: Vec2, pressure: f32, spacing_px: f32, out: &mut Vec<PathSample>) {
        let spacing = spacing_px.max(0.5);
        let Some(from) = self.last else {
            out.push(PathSample { pos: to, dir: self.dir, pressure });
            self.last = Some(to);
            self.last_pressure = pressure;
            self.residual = 0.0;
            return;
        };
        let seg = to - from;
        let len = seg.length();
        if len < 1e-6 {
            // The pen may press harder without moving: the next stamp starts from this pressure.
            self.last_pressure = pressure;
            return;
        }
        let dir = seg / len;
        self.dir = dir;
        let p0 = self.last_pressure;
        let mut t = spacing - self.residual;
        let mut last_emit = -self.residual;
        while t <= len {
            // Constant pressure (always the case for a mouse) is passed through exactly.
            let p = if p0 == pressure { pressure } else { p0 + (pressure - p0) * (t / len) };
            out.push(PathSample { pos: from + dir * t, dir, pressure: p });
            last_emit = t;
            t += spacing;
        }
        self.residual = len - last_emit;
        self.last = Some(to);
        self.last_pressure = pressure;
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

    #[test]
    fn resamples_at_constant_spacing_across_pushes() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        for x in [0.0, 3.0, 7.5, 20.0, 21.0, 40.0] {
            p.push(Vec2::new(x, 10.0), 5.0, 0.0, &mut out);
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
        p.push(Vec2::ZERO, 1.0, 10.0, &mut out);
        p.push(Vec2::new(8.0, 0.0), 1.0, 10.0, &mut out);
        assert_eq!(out.len(), 1, "cursor within lazy radius: brush does not move");
        p.push(Vec2::new(30.0, 0.0), 1.0, 10.0, &mut out);
        let last = out.last().unwrap().pos.x;
        assert!((last - 20.0).abs() < 1e-4, "brush stops 10px behind the cursor, got {last}");
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
    fn pressure_is_interpolated_between_samples() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        p.push_pressure(Vec2::ZERO, 0.0, 1.0, 0.0, &mut out);
        p.push_pressure(Vec2::new(10.0, 0.0), 1.0, 1.0, 0.0, &mut out);
        assert_eq!(out.len(), 11);
        for (i, s) in out.iter().enumerate() {
            assert!((s.pressure - i as f32 / 10.0).abs() < 1e-5, "{i}: {}", s.pressure);
        }
        // Pressing harder in place takes effect from the next segment; out-of-range input clamps.
        p.push_pressure(Vec2::new(10.0, 0.0), 0.5, 1.0, 0.0, &mut out);
        p.push_pressure(Vec2::new(12.0, 0.0), 7.0, 1.0, 0.0, &mut out);
        let tail: Vec<f32> = out[11..].iter().map(|s| s.pressure).collect();
        assert_eq!(tail, vec![0.75, 1.0]);
        p.line_to_pressure(Vec2::new(14.0, 0.0), f32::NAN, 1.0, &mut out);
        assert!(out[13..].iter().all(|s| s.pressure == 1.0));
    }

    #[test]
    fn mouse_samples_have_full_pressure() {
        let mut p = ScreenPath::new();
        let mut out = Vec::new();
        for x in [0.0, 4.0, 9.0] {
            p.push(Vec2::new(x, 0.0), 1.0, 0.0, &mut out);
        }
        assert!(out.iter().all(|s| s.pressure == 1.0));
    }

    #[test]
    fn random_is_deterministic_and_spread() {
        assert_eq!(rand01(1, 2, 3), rand01(1, 2, 3));
        let mean: f32 = (0..1000).map(|i| rand01(7, i, 0)).sum::<f32>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05);
        assert!((0..1000).all(|i| (0.0..1.0).contains(&rand01(7, i, 1))));
    }
}

//! Procedural scratch generator (original implementation).
//!
//! Every lattice cell owns up to [`CANDIDATES`] scratches. Whether candidate `k` exists, and its
//! centre, direction, length, width, bend and depth, come from hashes of the *wrapped* cell
//! coordinates, so the pattern tiles seamlessly for an integer cell count and is a pure function
//! of pattern-space position (resolution independent, deterministic by seed). A candidate exists
//! when its hash is below the local probability, so raising `density` only ever adds scratches.
//! Each scratch is a bent segment: distance is measured to a parabolic arc in the scratch frame,
//! with a smooth cross profile and a taper along its length.

use glam::Vec2;
use subcraft_doc::ScratchParams;

use crate::noise::{hash_f, value_xy};

/// Candidate scratches per lattice cell.
pub const CANDIDATES: u32 = 6;

#[inline]
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Scratch intensity 0..1 at `q`, in lattice-cell units (`period` cells per tile).
pub fn eval(q: Vec2, period: i32, seed: u32, sp: &ScratchParams) -> f32 {
    let period = period.max(1);
    let len_max = sp.length.max(0.01) * (1.0 + 0.5 * sp.length_var.clamp(0.0, 1.0));
    let w_max = sp.width.max(1e-4) * (1.0 + 0.8 * sp.width_var.clamp(0.0, 1.0));
    let reach = len_max * 0.5 * (1.0 + 0.5 * sp.curvature) + w_max + 0.5 * sp.curvature * len_max * 0.25;
    let r = (reach.ceil() as i32).clamp(1, 4);
    let cl_period = (period / 2).max(1);
    let base = Vec2::new(q.x.floor(), q.y.floor());
    let mut best: f32 = 0.0;
    for dy in -r..=r {
        for dx in -r..=r {
            let cx = base.x as i32 + dx;
            let cy = base.y as i32 + dy;
            let wx = cx.rem_euclid(period);
            let wy = cy.rem_euclid(period);
            // Patchiness: a coarse periodic noise gates the local probability.
            let p_exist = if sp.cluster > 0.0 {
                let g = value_xy(Vec2::new(wx as f32 + 0.5, wy as f32 + 0.5) * (cl_period as f32 / period as f32), cl_period, cl_period, seed ^ 0x3c1d);
                let gate = smooth((g - 0.35) / 0.3) * 2.2;
                sp.density * (1.0 + (gate - 1.0) * sp.cluster.clamp(0.0, 1.0))
            } else {
                sp.density
            };
            if p_exist <= 0.0 {
                continue;
            }
            for k in 0..CANDIDATES {
                let s = seed.wrapping_add(k.wrapping_mul(0x9e37_79b1)).wrapping_add(0x51ed);
                if hash_f(wx, wy, s) >= p_exist {
                    continue;
                }
                let h = |salt: u32| hash_f(wx, wy, s ^ salt);
                let c = Vec2::new(cx as f32 + h(0xa1), cy as f32 + h(0xa2));
                let ang = (sp.angle_deg + (h(0xa3) * 2.0 - 1.0) * sp.spread_deg).to_radians();
                let dir = Vec2::new(ang.cos(), ang.sin());
                let nrm = Vec2::new(-dir.y, dir.x);
                let len = (sp.length * (1.0 + (h(0xa4) - 0.5) * sp.length_var)).max(0.01);
                let hl = len * 0.5;
                let w = (sp.width * (1.0 + (h(0xa5) * 2.0 - 1.0) * 0.8 * sp.width_var)).max(1e-4);
                let bend = sp.curvature * (h(0xa6) * 2.0 - 1.0) * 0.8;
                let depth = 1.0 - sp.depth_falloff * 0.6 * h(0xa7);
                let d = q - c;
                // Quick reject by distance to the centre.
                if d.length_squared() > (hl * (1.0 + bend.abs()) + w).powi(2) {
                    continue;
                }
                let along = d.dot(dir);
                let side = d.dot(nrm);
                let sc = along.clamp(-hl, hl);
                let offset = bend * sc * sc / hl;
                let slope = 2.0 * bend * sc / hl;
                let perp = (side - offset) / (1.0 + slope * slope).sqrt();
                let past = along - sc;
                let tn = sc.abs() / hl;
                let power = 8.0 - 6.5 * sp.depth_falloff;
                let taper = 1.0 - tn.powf(power);
                let w_here = w * (1.0 - 0.6 * sp.depth_falloff * (1.0 - taper));
                let dist = (perp * perp + past * past).sqrt();
                let line = 1.0 - smooth(dist / w_here);
                best = best.max(line * taper * depth);
            }
        }
    }
    best
}

/// Weight 0..1 of curvature-map value `curv` (0.5 = flat) for scratch wear: edges and
/// grooves keep their scratches, flat areas lose them as `wear` grows.
pub fn wear_weight(curv: f32, wear: f32) -> f32 {
    let edge = smooth((curv - 0.5).abs() / 0.2);
    1.0 + (edge - 1.0) * wear.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern;
    use subcraft_doc::{ProceduralKind, ProceduralParams, ScratchPreset};

    fn params(density: f32) -> ProceduralParams {
        let mut p = ProceduralParams::new(ProceduralKind::Scratch);
        p.scale = 4.0;
        p.scratch = ScratchParams { density, width: 0.02, ..ScratchParams::default() };
        p
    }

    fn mean_coverage(p: &ProceduralParams, n: u32) -> f32 {
        let mut sum = 0.0;
        for y in 0..n {
            for x in 0..n {
                sum += pattern::eval(p, Vec2::new((x as f32 + 0.5) / n as f32, (y as f32 + 0.5) / n as f32));
            }
        }
        sum / (n * n) as f32
    }

    #[test]
    fn deterministic_and_seed_dependent() {
        let p = params(0.4);
        let a = mean_coverage(&p, 96);
        assert_eq!(a.to_bits(), mean_coverage(&p, 96).to_bits());
        let b = mean_coverage(&ProceduralParams { seed: 9, ..p }, 96);
        assert_ne!(a.to_bits(), b.to_bits());
        assert!(a > 0.0);
    }

    #[test]
    fn tiles_without_seams() {
        for preset in ScratchPreset::ALL {
            let mut p = ProceduralParams::new(ProceduralKind::Scratch);
            preset.apply(&mut p);
            for i in 0..400 {
                let u = i as f32 * 0.002_513;
                let v = (i * 7 % 400) as f32 * 0.0025;
                let a = pattern::eval(&p, Vec2::new(u, v));
                // One tile over (and back) must not change the value.
                assert!((a - pattern::eval(&p, Vec2::new(u + 1.0, v))).abs() < 1e-3, "{preset:?} u");
                assert!((a - pattern::eval(&p, Vec2::new(u, v - 2.0))).abs() < 1e-3, "{preset:?} v");
            }
            // Continuity across the border: the jump over one texel is no larger than inside.
            let n = 512.0;
            let (mut border, mut inner) = (0.0f32, 0.0f32);
            for i in 0..512 {
                let v = (i as f32 + 0.5) / n;
                border = border.max((pattern::eval(&p, Vec2::new(0.999_5, v)) - pattern::eval(&p, Vec2::new(1.000_5, v))).abs());
                inner = inner.max((pattern::eval(&p, Vec2::new(0.499_5, v)) - pattern::eval(&p, Vec2::new(0.500_5, v))).abs());
            }
            assert!(border <= inner.max(0.05) * 1.5 + 0.02, "{preset:?}: border {border} inner {inner}");
        }
    }

    #[test]
    fn resolution_independent() {
        let p = params(0.4);
        let coarse = mean_coverage(&p, 128);
        let fine = mean_coverage(&p, 512);
        assert!((coarse - fine).abs() < 0.08 * fine, "coarse {coarse} fine {fine}");
        // Same positions give the same values whatever grid they come from.
        let q = Vec2::new(0.123, 0.456);
        assert_eq!(pattern::eval(&p, q).to_bits(), pattern::eval(&p, q).to_bits());
    }

    #[test]
    fn coverage_grows_with_density() {
        let mut last = -1.0f32;
        assert!(mean_coverage(&params(0.0), 64) < 2e-3, "remap floor only");
        for d in [0.1, 0.25, 0.5, 0.75, 1.0] {
            let c = mean_coverage(&params(d), 128);
            assert!(c > last, "density {d}: {c} <= {last}");
            last = c;
        }
        assert!(last < 0.6, "full density must not flood the tile: {last}");
    }

    #[test]
    fn direction_and_spread() {
        // Spread 0 along U: neighbouring texels along V of a bright texel are dimmer than along U.
        let mut p = params(0.3);
        p.scratch.spread_deg = 0.0;
        p.scratch.curvature = 0.0;
        let n = 256u32;
        let (mut along, mut across) = (0.0f32, 0.0f32);
        for y in 2..n - 2 {
            for x in 2..n - 2 {
                let at = |x: u32, y: u32| pattern::eval(&p, Vec2::new(x as f32 / n as f32, y as f32 / n as f32));
                let c = at(x, y);
                along += (c - at(x + 1, y)).abs();
                across += (c - at(x, y + 1)).abs();
            }
        }
        assert!(along * 3.0 < across, "along {along} across {across}");
    }

    #[test]
    fn wear_weight_behaviour() {
        assert_eq!(wear_weight(0.5, 0.0), 1.0);
        assert_eq!(wear_weight(0.5, 1.0), 0.0);
        assert_eq!(wear_weight(0.8, 1.0), 1.0);
        assert!(wear_weight(0.6, 0.5) > wear_weight(0.5, 0.5));
    }
}

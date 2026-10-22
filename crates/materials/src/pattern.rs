//! Procedural pattern evaluation (grayscale 0..1).

use glam::Vec2;
use subcraft_doc::{ProceduralKind, ProceduralParams};

use crate::noise::{fbm, hash_f, value_xy, worley};

/// Raw pattern value at `p` (pattern space: one tile = 0..1).
pub fn raw(kind: ProceduralKind, p: Vec2, scale: f32, seed: u32) -> f32 {
    let period = scale.round().max(1.0) as i32;
    let q = p * period as f32;
    match kind {
        ProceduralKind::Clouds => fbm(q, period, 6, seed),
        ProceduralKind::Cells => {
            let (f1, f2) = worley(q, period, seed);
            // Cell interiors bright, borders dark.
            ((f2 - f1) * 1.6).clamp(0.0, 1.0)
        }
        ProceduralKind::Grunge => {
            let base = fbm(q, period, 6, seed);
            let detail = fbm(q * 4.0, period * 4, 3, seed ^ 0x5bd1);
            let v = base * 0.75 + detail * 0.25;
            smooth((v - 0.42) / 0.18)
        }
        ProceduralKind::Scratches => scratches(q, period, seed),
        ProceduralKind::WoodRings => {
            let warp = fbm(q * 0.5, (period / 2).max(1), 4, seed) - 0.5;
            let r = (q.y * 0.35 + warp * 2.5) * 6.0;
            let ring = (r - r.floor()).powf(0.6);
            let grain = value_xy(Vec2::new(q.x, q.y * 24.0), period, period * 24, seed ^ 0x77) * 0.25;
            (ring * 0.8 + grain).clamp(0.0, 1.0)
        }
        ProceduralKind::Brushed => {
            let fine = value_xy(Vec2::new(q.x, q.y * 64.0), period, period * 64, seed);
            let coarse = value_xy(Vec2::new(q.x, q.y * 16.0), period, period * 16, seed ^ 0x99);
            (fine * 0.6 + coarse * 0.4).clamp(0.0, 1.0)
        }
        ProceduralKind::Checker => {
            let c = (q.x.floor() as i64 + q.y.floor() as i64).rem_euclid(2);
            c as f32
        }
        ProceduralKind::GradientU => p.x.rem_euclid(1.0),
        ProceduralKind::GradientV => p.y.rem_euclid(1.0),
        ProceduralKind::Dirt => {
            // Soft grime blotches broken up by fine specks.
            let blotch = fbm(q, period, 5, seed);
            let specks = fbm(q * 5.0, period * 5, 2, seed ^ 0x2f1);
            smooth((blotch * 0.8 + specks * 0.35 - 0.5) / 0.22)
        }
        ProceduralKind::Cracks => {
            // Worley borders, bent by noise so the cracks wander; 1 on the crack.
            let warp = Vec2::new(fbm(q * 2.0, period * 2, 3, seed ^ 0x51) - 0.5, fbm(q * 2.0 + Vec2::splat(31.0), period * 2, 3, seed ^ 0x52) - 0.5);
            let w = (q + warp * 0.35).rem_euclid(Vec2::splat(period as f32));
            let (f1, f2) = worley(w, period, seed);
            1.0 - smooth((f2 - f1) / 0.07)
        }
        ProceduralKind::Streaks => {
            // Narrow in u, long in v: columns of drips that fade as they run.
            let col = value_xy(Vec2::new(q.x * 6.0, q.y), period * 6, period, seed);
            let run = value_xy(Vec2::new(q.x * 6.0 + 17.0, q.y * 2.0), period * 6, period * 2, seed ^ 0x8a);
            smooth((col * 0.7 + run * 0.4 - 0.55) / 0.2)
        }
        ProceduralKind::Speckles => {
            let n = value_xy(q * 24.0, period * 24, period * 24, seed);
            smooth((n - 0.72) / 0.1)
        }
        ProceduralKind::Pores => {
            let (f1, _) = worley(q * 3.0, period * 3, seed);
            1.0 - smooth((f1 - 0.08) / 0.12)
        }
    }
}

/// Pattern value after balance/contrast/invert.
pub fn eval(params: &ProceduralParams, p: Vec2) -> f32 {
    let v = raw(params.kind, p, params.scale, params.seed);
    let v = remap(v, params.balance, params.contrast);
    if params.invert { 1.0 - v } else { v }
}

/// Linear remap: contrast 0 = identity around balance 0.5; contrast 1 = hard threshold.
pub fn remap(v: f32, balance: f32, contrast: f32) -> f32 {
    let width = (1.0 - contrast.clamp(0.0, 1.0)) * 0.5 + 1e-3;
    ((v - balance) / (2.0 * width) + 0.5).clamp(0.0, 1.0)
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Thin random line segments, a few per lattice cell.
fn scratches(q: Vec2, period: i32, seed: u32) -> f32 {
    let i = q.floor();
    let mut best: f32 = 0.0;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let cx = i.x as i32 + dx;
            let cy = i.y as i32 + dy;
            for k in 0..3u32 {
                let s = seed.wrapping_add(k * 7919);
                let wx = cx.rem_euclid(period);
                let wy = cy.rem_euclid(period);
                let ox = hash_f(wx, wy, s);
                let oy = hash_f(wx, wy, s ^ 0xabc);
                let ang = hash_f(wx, wy, s ^ 0x123) * std::f32::consts::PI;
                let len = 0.3 + hash_f(wx, wy, s ^ 0x777) * 0.9;
                let c = Vec2::new(cx as f32 + ox, cy as f32 + oy);
                let dir = Vec2::new(ang.cos(), ang.sin());
                let d = q - c;
                let t = d.dot(dir).clamp(-len * 0.5, len * 0.5);
                let dist = (d - dir * t).length();
                let fade = 1.0 - (t.abs() / (len * 0.5)).powi(4);
                let w = 0.012 + hash_f(wx, wy, s ^ 0x999) * 0.01;
                let line = (1.0 - (dist / w).clamp(0.0, 1.0)) * fade;
                best = best.max(line);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_patterns_in_range_and_deterministic() {
        for kind in ProceduralKind::ALL {
            let params = ProceduralParams::new(kind);
            for k in 0..200 {
                let p = Vec2::new(k as f32 * 0.0137, k as f32 * 0.0291);
                let a = eval(&params, p);
                assert!((0.0..=1.0).contains(&a), "{kind:?} {a}");
                assert_eq!(a, eval(&params, p));
            }
        }
    }

    #[test]
    fn uv_patterns_tile_at_integer_scale() {
        for kind in [ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Brushed, ProceduralKind::Grunge, ProceduralKind::WoodRings, ProceduralKind::Scratches, ProceduralKind::Dirt, ProceduralKind::Cracks, ProceduralKind::Streaks, ProceduralKind::Speckles, ProceduralKind::Pores] {
            let params = ProceduralParams::new(kind);
            for k in 0..40 {
                let p = Vec2::new(0.013 * k as f32, 0.71);
                let a = eval(&params, p);
                let b = eval(&params, p + Vec2::X);
                assert!((a - b).abs() < 2e-3, "{kind:?} not tileable: {a} vs {b}");
            }
        }
    }

    /// Mean of a pattern over a regular grid of the unit tile.
    fn mean(kind: ProceduralKind, scale: f32, seed: u32) -> f32 {
        let params = ProceduralParams { scale, seed, ..ProceduralParams::new(kind) };
        let n = 64;
        (0..n * n).map(|i| eval(&params, Vec2::new((i % n) as f32 + 0.5, (i / n) as f32 + 0.5) / n as f32)).sum::<f32>() / (n * n) as f32
    }

    #[test]
    fn dirt_generators_cover_part_of_the_surface_and_depend_on_the_seed() {
        for kind in [ProceduralKind::Dirt, ProceduralKind::Cracks, ProceduralKind::Streaks, ProceduralKind::Speckles, ProceduralKind::Pores] {
            let m = mean(kind, 4.0, 3);
            assert!((0.01..0.7).contains(&m), "{kind:?} covers {m}");
            assert!((m - mean(kind, 4.0, 4)).abs() > 1e-4 || kind == ProceduralKind::Speckles, "{kind:?} ignores its seed");
            assert_eq!(m, mean(kind, 4.0, 3));
        }
    }

    #[test]
    fn streaks_run_along_v_and_cracks_are_thin_lines() {
        // Streaks vary much faster across u than along v.
        let params = ProceduralParams { scale: 4.0, seed: 5, ..ProceduralParams::new(ProceduralKind::Streaks) };
        let (mut across, mut along) = (0.0, 0.0);
        for i in 0..400 {
            let p = Vec2::new((i % 20) as f32 * 0.05 + 0.013, (i / 20) as f32 * 0.05 + 0.021);
            let v = eval(&params, p);
            across += (v - eval(&params, p + Vec2::new(0.01, 0.0))).abs();
            along += (v - eval(&params, p + Vec2::new(0.0, 0.01))).abs();
        }
        assert!(across > 2.0 * along, "across {across} vs along {along}");
        let cracks = mean(ProceduralKind::Cracks, 5.0, 7);
        assert!(cracks < 0.35, "cracks should be thin: {cracks}");
    }
}

//! Procedural pattern evaluation (grayscale 0..1).

use glam::Vec2;
use subcraft_doc::{ProceduralKind, ProceduralParams, ScratchParams};

use crate::noise::{fbm, fbm_many, hash_f, value_xy, value_xy_many, worley, worley_many};

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
        ProceduralKind::Scratch => crate::scratch::eval(q, period, seed, &ScratchParams::default()),
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
        ProceduralKind::Weave => weave(q, period, seed),
        ProceduralKind::Twill => twill(q, period, seed),
        ProceduralKind::Veins => veins(q, period, seed),
        ProceduralKind::Planks => planks(q, period, seed),
        ProceduralKind::Knots => knots(q, period, seed),
    }
}

/// Pattern value after balance/contrast/invert.
pub fn eval(params: &ProceduralParams, p: Vec2) -> f32 {
    let v = if params.kind == ProceduralKind::Scratch {
        let period = params.scale.round().max(1.0) as i32;
        crate::scratch::eval(p * period as f32, period, params.seed, &params.scratch)
    } else {
        raw(params.kind, p, params.scale, params.seed)
    };
    let v = remap(v, params.balance, params.contrast);
    if params.invert { 1.0 - v } else { v }
}

/// Whether [`eval_many`] has a batched path for these parameters (faster than [`eval`] per
/// point); the others work too, one point at a time.
pub fn batches(params: &ProceduralParams) -> bool {
    matches!(
        params.kind,
        ProceduralKind::Clouds | ProceduralKind::Grunge | ProceduralKind::Dirt | ProceduralKind::Cracks | ProceduralKind::Cells | ProceduralKind::Pores | ProceduralKind::Brushed | ProceduralKind::Speckles | ProceduralKind::Streaks
    )
}

/// [`eval`] at every point into `out`, bit-identical; the fractal noise of the kinds in
/// [`batches`] runs over all points at once ([`crate::noise::fbm_many`]).
pub fn eval_many(params: &ProceduralParams, points: &[Vec2], out: &mut [f32]) {
    assert_eq!(points.len(), out.len());
    if !batches(params) {
        for (o, &p) in out.iter_mut().zip(points) {
            *o = eval(params, p);
        }
        return;
    }
    let seed = params.seed;
    let period = params.scale.round().max(1.0) as i32;
    let q: Vec<Vec2> = points.iter().map(|&p| p * period as f32).collect();
    let scaled = |k: f32| -> Vec<Vec2> { q.iter().map(|&q| q * k).collect() };
    let mut a = vec![0.0f32; q.len()];
    let mut b = vec![0.0f32; q.len()];
    match params.kind {
        ProceduralKind::Clouds => fbm_many(&q, period, 6, seed, out),
        ProceduralKind::Grunge => {
            fbm_many(&q, period, 6, seed, &mut a);
            fbm_many(&scaled(4.0), period * 4, 3, seed ^ 0x5bd1, &mut b);
            for ((o, &base), &detail) in out.iter_mut().zip(&a).zip(&b) {
                let v = base * 0.75 + detail * 0.25;
                *o = smooth((v - 0.42) / 0.18);
            }
        }
        ProceduralKind::Dirt => {
            fbm_many(&q, period, 5, seed, &mut a);
            fbm_many(&scaled(5.0), period * 5, 2, seed ^ 0x2f1, &mut b);
            for ((o, &blotch), &specks) in out.iter_mut().zip(&a).zip(&b) {
                *o = smooth((blotch * 0.8 + specks * 0.35 - 0.5) / 0.22);
            }
        }
        ProceduralKind::Cracks => {
            let q2 = scaled(2.0);
            fbm_many(&q2, period * 2, 3, seed ^ 0x51, &mut a);
            let q2b: Vec<Vec2> = q2.iter().map(|&p| p + Vec2::splat(31.0)).collect();
            fbm_many(&q2b, period * 2, 3, seed ^ 0x52, &mut b);
            for (((o, &q), &wx), &wy) in out.iter_mut().zip(&q).zip(&a).zip(&b) {
                let warp = Vec2::new(wx - 0.5, wy - 0.5);
                let w = (q + warp * 0.35).rem_euclid(Vec2::splat(period as f32));
                let (f1, f2) = worley(w, period, seed);
                *o = 1.0 - smooth((f2 - f1) / 0.07);
            }
        }
        ProceduralKind::Cells => {
            worley_many(&q, period, seed, &mut a, &mut b);
            for ((o, &f1), &f2) in out.iter_mut().zip(&a).zip(&b) {
                *o = ((f2 - f1) * 1.6).clamp(0.0, 1.0);
            }
        }
        ProceduralKind::Pores => {
            worley_many(&scaled(3.0), period * 3, seed, &mut a, &mut b);
            for (o, &f1) in out.iter_mut().zip(&a) {
                *o = 1.0 - smooth((f1 - 0.08) / 0.12);
            }
        }
        ProceduralKind::Brushed => {
            let fine: Vec<Vec2> = q.iter().map(|q| Vec2::new(q.x, q.y * 64.0)).collect();
            value_xy_many(&fine, period, period * 64, seed, &mut a);
            let coarse: Vec<Vec2> = q.iter().map(|q| Vec2::new(q.x, q.y * 16.0)).collect();
            value_xy_many(&coarse, period, period * 16, seed ^ 0x99, &mut b);
            for ((o, &fine), &coarse) in out.iter_mut().zip(&a).zip(&b) {
                *o = (fine * 0.6 + coarse * 0.4).clamp(0.0, 1.0);
            }
        }
        ProceduralKind::Speckles => {
            value_xy_many(&scaled(24.0), period * 24, period * 24, seed, &mut a);
            for (o, &n) in out.iter_mut().zip(&a) {
                *o = smooth((n - 0.72) / 0.1);
            }
        }
        ProceduralKind::Streaks => {
            let col: Vec<Vec2> = q.iter().map(|q| Vec2::new(q.x * 6.0, q.y)).collect();
            value_xy_many(&col, period * 6, period, seed, &mut a);
            let run: Vec<Vec2> = q.iter().map(|q| Vec2::new(q.x * 6.0 + 17.0, q.y * 2.0)).collect();
            value_xy_many(&run, period * 6, period * 2, seed ^ 0x8a, &mut b);
            for ((o, &col), &run) in out.iter_mut().zip(&a).zip(&b) {
                *o = smooth((col * 0.7 + run * 0.4 - 0.55) / 0.2);
            }
        }
        _ => unreachable!("kind without a batched path"),
    }
    for v in out.iter_mut() {
        let r = remap(*v, params.balance, params.contrast);
        *v = if params.invert { 1.0 - r } else { r };
    }
}

/// Linear remap: contrast 0 = identity around balance 0.5; contrast 1 = hard threshold.
pub fn remap(v: f32, balance: f32, contrast: f32) -> f32 {
    let width = (1.0 - contrast.clamp(0.0, 1.0)) * 0.5 + 1e-3;
    ((v - balance) / (2.0 * width) + 0.5).clamp(0.0, 1.0)
}

/// Threads across one unit of pattern space (even, so the over/under order tiles).
const WEAVE_THREADS: i32 = 8;

/// Plain weave: warp and weft threads crossing over and under each other. The value is the
/// cloth's height: a thread is round across its width, highest where it passes over the other
/// thread and dipping where it goes under. Each thread has its own slight thickness.
fn weave(q: Vec2, period: i32, seed: u32) -> f32 {
    let n = period * WEAVE_THREADS;
    let t = q * WEAVE_THREADS as f32;
    let cell = crate::noise::floor2(t);
    let f = t - cell;
    let (ix, iy) = ((cell.x as i32).rem_euclid(n), (cell.y as i32).rem_euclid(n));
    // Even crossings: the vertical (warp) thread is on top; odd: the horizontal (weft) one.
    let warp_on_top = (ix + iy) % 2 == 0;
    let (across, along, thread) = if warp_on_top { (f.x, f.y, hash_f(ix, -1, seed)) } else { (f.y, f.x, hash_f(-1, iy, seed ^ 0x3c)) };
    let round = (std::f32::consts::PI * across).sin().max(0.0).powf(0.6);
    let over = 0.5 + 0.5 * (std::f32::consts::PI * along).sin();
    // Fibres along the thread.
    let fibre = if warp_on_top { value_xy(Vec2::new(t.x * 6.0, t.y * 0.5), n * 6, (n / 2).max(1), seed ^ 0x61) } else { value_xy(Vec2::new(t.x * 0.5, t.y * 6.0), (n / 2).max(1), n * 6, seed ^ 0x62) };
    (round * (0.35 + 0.55 * over) * (0.86 + 0.14 * thread) + (fibre - 0.5) * 0.08).clamp(0.0, 1.0)
}

/// 3/1 twill: every warp (vertical) thread floats over three weft threads and dips under the
/// fourth, and that dip moves one thread sideways per row, which draws the diagonal rib of
/// denim. Height of the cloth: the warp floats are high and round, the weft crossings lower.
fn twill(q: Vec2, period: i32, seed: u32) -> f32 {
    let n = period * WEAVE_THREADS;
    let t = q * WEAVE_THREADS as f32;
    let cell = crate::noise::floor2(t);
    let f = t - cell;
    let (ix, iy) = ((cell.x as i32).rem_euclid(n), (cell.y as i32).rem_euclid(n));
    // `n` is a multiple of 4 (8 threads per unit), so the step tiles.
    let phase = (ix - iy).rem_euclid(4);
    let pi = std::f32::consts::PI;
    let thread = hash_f(ix, -1, seed);
    let v = if phase == 0 {
        // The weft is on top here: a short, lower crossing running along x.
        (pi * f.y).sin().max(0.0).powf(0.6) * (0.3 + 0.2 * (pi * f.x).sin())
    } else {
        // Warp float: round across x; it dips towards the crossing at the end of the float.
        let dip = match phase {
            1 => 0.75 + 0.25 * (0.5 * pi * f.y).sin(),
            3 => 0.75 + 0.25 * (0.5 * pi * (1.0 - f.y)).sin(),
            _ => 1.0,
        };
        (pi * f.x).sin().max(0.0).powf(0.6) * dip * (0.88 + 0.12 * thread)
    };
    let fibre = value_xy(Vec2::new(t.x * 6.0, t.y * 0.5), n * 6, (n / 2).max(1), seed ^ 0x61);
    (v + (fibre - 0.5) * 0.08).clamp(0.0, 1.0)
}

/// Mineral veins: the level lines of a noise field that is itself bent by noise, so the lines
/// wander, fork and fade instead of forming a net. A second, finer set runs between them.
fn veins(q: Vec2, period: i32, seed: u32) -> f32 {
    let line = |q: Vec2, period: i32, seed: u32, width: f32| {
        let warp = Vec2::new(fbm(q, period, 4, seed ^ 0x1b3) - 0.5, fbm(q + Vec2::splat(13.0), period, 4, seed ^ 0x2c7) - 0.5);
        let n = fbm(q + warp * 1.6, period, 5, seed);
        // Distance from the 0.5 level, in units of the line width.
        let d = (n - 0.5).abs() / width;
        // Veins fade in and out along their length.
        let fade = smooth((fbm(q + Vec2::splat(41.0), period, 3, seed ^ 0x3d1) - 0.35) / 0.25);
        (1.0 - d).clamp(0.0, 1.0) * fade
    };
    let main = line(q, period, seed, 0.035);
    let fine = line(q * 2.0, period * 2, seed ^ 0x9e37, 0.03) * 0.55;
    main.max(fine)
}

/// Boards per unit across (V); a board is one unit long (U), so boards are 5 times longer
/// than wide before the projection's own scale.
const PLANK_ROWS: i32 = 5;

/// Boards in staggered rows. 0 in the gap between boards, rising over a narrow bevel to the
/// board's own brightness (0.35 to 1, one value per board), so the same pattern gives a
/// height with rounded edges, a tone per board, and (inverted, with contrast) the gaps.
fn planks(q: Vec2, period: i32, seed: u32) -> f32 {
    let rows = period * PLANK_ROWS;
    let y = q.y * PLANK_ROWS as f32;
    let row_f = crate::noise::floor(y);
    let row = (row_f as i32).rem_euclid(rows);
    // Each row starts its boards somewhere else.
    let x = q.x + hash_f(row, 7, seed) * period as f32;
    let col_f = crate::noise::floor(x);
    let col = (col_f as i32).rem_euclid(period.max(1));
    let (fx, fy) = (x - col_f, y - row_f);
    // Distance to the nearest edge, in board widths (the ends are measured in the same unit).
    let edge = fy.min(1.0 - fy).min(fx.min(1.0 - fx) * PLANK_ROWS as f32);
    let gap = 0.03;
    let bevel = 0.035;
    let rise = smooth((edge - gap) / bevel);
    rise * (0.35 + 0.65 * hash_f(col, row, seed ^ 0x51f))
}

/// Wood knots: in under half of the cells, one oval stretched along U (the grain). A solid core
/// (1) with growth rings fading out around it; 0 away from knots. A knot never reaches its
/// cell's border, so the pattern tiles.
fn knots(q: Vec2, period: i32, seed: u32) -> f32 {
    let cell = crate::noise::floor2(q);
    let f = q - cell;
    let p = period.max(1);
    let (ix, iy) = ((cell.x as i32).rem_euclid(p), (cell.y as i32).rem_euclid(p));
    if hash_f(ix, iy, seed) > 0.45 {
        return 0.0;
    }
    let centre = Vec2::new(0.35 + 0.3 * hash_f(ix, iy, seed ^ 0xa1), 0.3 + 0.4 * hash_f(ix, iy, seed ^ 0xa2));
    let size = 0.07 + 0.08 * hash_f(ix, iy, seed ^ 0xa3);
    let d = f - centre;
    // Not a perfect ellipse: the outline wobbles a little, differently per knot.
    let wobble = 1.0 + 0.12 * (d.y.atan2(d.x) * 3.0 + hash_f(ix, iy, seed ^ 0xa4) * std::f32::consts::TAU).sin();
    let r = Vec2::new(d.x / 1.8, d.y).length() / (size * wobble);
    if r >= 1.0 {
        return 0.0;
    }
    let core = 1.0 - smooth((r - 0.22) / 0.1);
    let rings = (0.5 + 0.5 * (r * 4.0 * std::f32::consts::TAU).cos()) * (1.0 - smooth(r)) * 0.7;
    core.max(rings)
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One line segment of [`scratches`]: centre, direction, half length and width.
#[derive(Clone, Copy, Default)]
struct Line {
    c: Vec2,
    dir: Vec2,
    half: f32,
    w: f32,
    /// Squared distance from `c` beyond which the segment contributes exactly 0 (with a margin).
    reach2: f32,
}

/// Last cells' segments on this thread, keyed by `(cx, cy, period, seed)`: neighboring texels
/// share their 9 cells, so the hashes and trig run once per cell instead of once per texel.
#[derive(Clone, Copy)]
struct LineCell {
    key: [i32; 4],
    lines: [Line; 3],
}

const LINE_SLOTS: usize = 64;

thread_local! {
    // Period 0 never occurs in a key (`rem_euclid` would have panicked), so the initial entries never match.
    static LINES: std::cell::RefCell<Box<[LineCell]>> = std::cell::RefCell::new(vec![LineCell { key: [0; 4], lines: [Line::default(); 3] }; LINE_SLOTS].into_boxed_slice());
}

fn line_cell(cx: i32, cy: i32, period: i32, seed: u32) -> [Line; 3] {
    std::array::from_fn(|k| {
        let s = seed.wrapping_add(k as u32 * 7919);
        let wx = cx.rem_euclid(period);
        let wy = cy.rem_euclid(period);
        let ox = hash_f(wx, wy, s);
        let oy = hash_f(wx, wy, s ^ 0xabc);
        let ang = hash_f(wx, wy, s ^ 0x123) * std::f32::consts::PI;
        let len = 0.3 + hash_f(wx, wy, s ^ 0x777) * 0.9;
        let w = 0.012 + hash_f(wx, wy, s ^ 0x999) * 0.01;
        let half = len * 0.5;
        // A point farther from the centre than `half + w` is farther than `w` from the segment
        // (which reaches `half` from its centre), so its line value is `(1 - 1) * fade = 0`.
        // The 1% margin (0.5% in distance) is far beyond any rounding of these sums.
        Line { c: Vec2::new(cx as f32 + ox, cy as f32 + oy), dir: Vec2::new(ang.cos(), ang.sin()), half, w, reach2: (half + w) * (half + w) * 1.01 }
    })
}

/// Thin random line segments, a few per lattice cell.
fn scratches(q: Vec2, period: i32, seed: u32) -> f32 {
    let i = crate::noise::floor2(q);
    LINES.with(|cells| {
        let cells = &mut *cells.borrow_mut();
        let mut best: f32 = 0.0;
        for dy in -1..=1 {
            for dx in -1..=1 {
                let cx = i.x as i32 + dx;
                let cy = i.y as i32 + dy;
                let key = [cx, cy, period, seed as i32];
                let slot = ((cx as u32).wrapping_mul(0x9e37_79b9) ^ (cy as u32).wrapping_mul(0x85eb_ca6b) ^ seed) >> 26;
                let cell = &mut cells[slot as usize & (LINE_SLOTS - 1)];
                if cell.key != key {
                    *cell = LineCell { key, lines: line_cell(cx, cy, period, seed) };
                }
                for l in &cell.lines {
                    let d = q - l.c;
                    // Out of reach: the value below would be exactly 0, which never raises `best`.
                    if d.length_squared() > l.reach2 {
                        continue;
                    }
                    let t = d.dot(l.dir).clamp(-l.half, l.half);
                    let dist = (d - l.dir * t).length();
                    let fade = 1.0 - (t.abs() / l.half).powi(4);
                    let line = (1.0 - (dist / l.w).clamp(0.0, 1.0)) * fade;
                    best = best.max(line);
                }
            }
        }
        best
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`scratches`] before cells were cached.
    fn scratches_reference(q: Vec2, period: i32, seed: u32) -> f32 {
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

    #[test]
    fn cached_scratch_lines_are_bit_identical_to_the_reference() {
        for (period, seed) in [(4, 0u32), (1, 5), (17, u32::MAX)] {
            for k in 0..20000 {
                let q = Vec2::new(k as f32 * 0.00731 - 5.0, (k % 409) as f32 * 0.0213 - 2.0);
                assert_eq!(scratches(q, period, seed).to_bits(), scratches_reference(q, period, seed).to_bits(), "{q} {period} {seed}");
                assert_eq!(scratches(q, period + 1, seed).to_bits(), scratches_reference(q, period + 1, seed).to_bits());
            }
        }
    }

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
        for kind in [ProceduralKind::Knots, ProceduralKind::Twill, ProceduralKind::Veins, ProceduralKind::Planks, ProceduralKind::Weave, ProceduralKind::Clouds, ProceduralKind::Cells, ProceduralKind::Brushed, ProceduralKind::Grunge, ProceduralKind::WoodRings, ProceduralKind::Scratches, ProceduralKind::Dirt, ProceduralKind::Cracks, ProceduralKind::Streaks, ProceduralKind::Speckles, ProceduralKind::Pores] {
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

    /// Weave: threads alternate over and under (neighbouring crossings differ), every thread is
    /// highest along its middle, and the cloth repeats `WEAVE_THREADS` times per unit.
    #[test]
    fn weave_alternates_over_and_under() {
        let at = |x: f32, y: f32| raw(ProceduralKind::Weave, Vec2::new(x, y), 1.0, 3);
        let step = 1.0 / WEAVE_THREADS as f32;
        let (mut middle, mut edge) = (0.0, 0.0);
        for i in 0..WEAVE_THREADS {
            for j in 0..WEAVE_THREADS {
                let (x, y) = ((i as f32 + 0.5) * step, (j as f32 + 0.5) * step);
                middle += at(x, y);
                // The corner between four crossings is the gap between threads.
                edge += at(i as f32 * step + 1e-4, j as f32 * step + 1e-4);
            }
        }
        let n = (WEAVE_THREADS * WEAVE_THREADS) as f32;
        assert!(middle / n > 0.7 && edge / n < 0.15, "threads are raised, gaps are low: {} {}", middle / n, edge / n);
        // On a warp-on-top crossing the profile is round across x; on the next one, across y.
        let quarter = step * 0.15;
        let c = step * 0.5;
        let drop_x = |x: f32, y: f32| at(x, y) - at(x + c - quarter, y);
        let drop_y = |x: f32, y: f32| at(x, y) - at(x, y + c - quarter);
        assert!(drop_x(c, c) > 0.2 && drop_x(c, c) > drop_y(c, c) + 0.1, "crossing (0,0) is a vertical thread");
        assert!(drop_y(c + step, c) > 0.2 && drop_y(c + step, c) > drop_x(c + step, c) + 0.1, "crossing (1,0) is a horizontal thread");
    }

    /// Twill: the weft crossing moves one thread across per row (a diagonal), and three of
    /// every four crossings are high warp floats.
    #[test]
    fn twill_steps_diagonally() {
        let step = 1.0 / WEAVE_THREADS as f32;
        let at = |i: i32, j: i32| raw(ProceduralKind::Twill, Vec2::new((i as f32 + 0.5) * step, (j as f32 + 0.5) * step), 1.0, 3);
        let mut low = 0;
        for j in 0..WEAVE_THREADS {
            for i in 0..WEAVE_THREADS {
                let weft = (i - j).rem_euclid(4) == 0;
                assert_eq!(at(i, j) < 0.6, weft, "crossing {i},{j}: {}", at(i, j));
                low += weft as i32;
                // The same kind of crossing one step along the diagonal.
                assert_eq!(at(i, j) < 0.6, at(i + 1, j + 1) < 0.6);
            }
        }
        assert_eq!(low, WEAVE_THREADS * WEAVE_THREADS / 4);
    }

    /// Veins are thin lines (they cover little) that depend on the seed, and they are not a
    /// closed net: long stretches of a straight walk cross no vein at all.
    #[test]
    fn veins_are_thin_wandering_lines() {
        let m = mean(ProceduralKind::Veins, 4.0, 3);
        assert!((0.01..0.2).contains(&m), "veins cover {m}");
        assert!((m - mean(ProceduralKind::Veins, 4.0, 4)).abs() > 1e-4);
        let params = ProceduralParams { scale: 4.0, seed: 3, ..ProceduralParams::new(ProceduralKind::Veins) };
        let on: Vec<bool> = (0..2000).map(|i| eval(&params, Vec2::new(i as f32 / 2000.0, 0.37)) > 0.5).collect();
        let crossings = on.windows(2).filter(|w| !w[0] && w[1]).count();
        assert!((1..40).contains(&crossings), "{crossings} veins crossed on one line");
        assert!(on.iter().filter(|v| **v).count() < 400, "lines, not areas");
    }

    /// Knots are sparse (most of the wood has none), each has a solid core, and rings around it.
    #[test]
    fn knots_are_sparse_cores_with_rings() {
        let n = 256;
        let vals: Vec<f32> = (0..n * n).map(|i| raw(ProceduralKind::Knots, Vec2::new((i % n) as f32 + 0.5, (i / n) as f32 + 0.5) / n as f32, 4.0, 5)).collect();
        let empty = vals.iter().filter(|v| **v == 0.0).count() as f32 / vals.len() as f32;
        assert!((0.8..0.995).contains(&empty), "empty share {empty}");
        assert!(vals.iter().any(|v| *v > 0.99), "a knot has a solid core");
        let rings = vals.iter().filter(|v| (0.05..0.6).contains(*v)).count();
        assert!(rings > 200, "rings around the cores: {rings}");
        assert!(vals.iter().all(|v| (0.0..=1.0).contains(v)));
        let other: Vec<f32> = (0..n * n).map(|i| raw(ProceduralKind::Knots, Vec2::new((i % n) as f32 + 0.5, (i / n) as f32 + 0.5) / n as f32, 4.0, 6)).collect();
        assert!(vals != other, "the seed moves the knots");
    }

    /// Planks: gaps are 0 and thin, a board has one brightness, and neighbouring boards differ.
    #[test]
    fn planks_have_gaps_and_a_tone_per_board() {
        let at = |x: f32, y: f32| raw(ProceduralKind::Planks, Vec2::new(x, y), 1.0, 9);
        let row = 1.0 / PLANK_ROWS as f32;
        for r in 0..PLANK_ROWS {
            assert_eq!(at(0.3, r as f32 * row + 1e-4), 0.0, "the line between rows is a gap");
        }
        let tones: Vec<f32> = (0..PLANK_ROWS).map(|r| at(0.31, (r as f32 + 0.5) * row)).collect();
        assert!(tones.iter().all(|t| (0.35..=1.0).contains(t)), "{tones:?}");
        assert!(tones.windows(2).any(|w| (w[0] - w[1]).abs() > 0.05), "boards differ: {tones:?}");
        let covered = (0..4096).filter(|i| at((i % 64) as f32 / 64.0 + 0.003, (i / 64) as f32 / 64.0 + 0.003) == 0.0).count();
        assert!((40..700).contains(&covered), "gaps are thin: {covered} of 4096");
    }
}

//! Non-periodic volume breakup: identical at identical surface positions across UV seams,
//! with coarse domain variation so opposite faces do not reuse the same projected tile.
use glam::Vec3;
use subcraft_doc::ProceduralParams;

/// The eight corner values of lattice cell `key = (x, y, z, seed)`, in `z, y, x` loop order.
#[derive(Clone, Copy)]
struct Cell {
    key: [i32; 4],
    valid: bool,
    v: [f32; 8],
}

const CELL_SLOTS: usize = 256;

thread_local! {
    /// Cells evaluated last on this thread: neighboring texels share cells at every octave, so
    /// the 16 hashes of a cell run once for many texels (values are exactly the same).
    static CELLS: std::cell::RefCell<Box<[Cell]>> = std::cell::RefCell::new(vec![Cell { key: [0; 4], valid: false, v: [0.0; 8] }; CELL_SLOTS].into_boxed_slice());
}

fn corners(cx: i32, cy: i32, cz: i32, seed: u32) -> [f32; 8] {
    let mut v = [0.0; 8];
    let mut k = 0;
    for z in 0..=1 { for y in 0..=1 { for x in 0..=1 {
        let h = crate::noise::hash2(cx + x, cy + y, seed);
        v[k] = crate::noise::hash_f(h as i32, cz + z, seed ^ 0x6d2b_79f5);
        k += 1;
    }}}
    v
}

fn value(p: Vec3, seed: u32) -> f32 {
    let cell = Vec3::new(crate::noise::floor(p.x), crate::noise::floor(p.y), crate::noise::floor(p.z));
    let f = p - cell;
    let u = f * f * f * (f * (f * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));
    let (cx, cy, cz) = (cell.x as i32, cell.y as i32, cell.z as i32);
    let key = [cx, cy, cz, seed as i32];
    let slot = ((cx as u32).wrapping_mul(0x9e37_79b9) ^ (cy as u32).wrapping_mul(0x85eb_ca6b) ^ (cz as u32).wrapping_mul(0xc2b2_ae35) ^ seed.wrapping_mul(0x27d4_eb2f)) >> 24;
    let v = CELLS.with(|cells| {
        let cell = &mut cells.borrow_mut()[slot as usize & (CELL_SLOTS - 1)];
        if !cell.valid || cell.key != key {
            *cell = Cell { key, valid: true, v: corners(cx, cy, cz, seed) };
        }
        cell.v
    });
    let mut out = 0.;
    let mut k = 0;
    for z in 0..=1 { for y in 0..=1 { for x in 0..=1 {
        let w = |axis: f32, side| if side == 0 { 1. - axis } else { axis };
        out += v[k] * w(u.x, x) * w(u.y, y) * w(u.z, z);
        k += 1;
    }}}
    out
}

pub fn eval(params: &ProceduralParams, position: [f32; 3]) -> f32 {
    let p = Vec3::from_array(position);
    let seed = params.seed;
    let macro_value = value(p * 1.7 + Vec3::new(7.3, 3.1, 5.7), seed ^ 0x132b);
    let warp = Vec3::new(macro_value, value(p * 1.3, seed ^ 0x572d), value(p * 1.9, seed ^ 0x781f)) - Vec3::splat(0.5);
    let q = p * params.scale.clamp(0.01, 1024.) + warp * 1.4;
    let (mut fine, mut norm, mut amplitude, mut frequency) = (0., 0., 0.5, 1.);
    for octave in 0..4 {
        fine += value(q * frequency, seed.wrapping_add(octave * 1013)) * amplitude;
        norm += amplitude; amplitude *= 0.5; frequency *= 2.;
    }
    let v = crate::pattern::remap(fine / norm * 0.75 + macro_value * 0.25, params.balance, params.contrast);
    if params.invert { 1. - v } else { v }
}

#[cfg(test)]
mod tests {
    use super::*;
    use subcraft_doc::ProceduralKind;

    /// [`value`] before cells were cached.
    fn value_reference(p: Vec3, seed: u32) -> f32 {
        let cell = p.floor();
        let f = p - cell;
        let u = f * f * f * (f * (f * 6.0 - Vec3::splat(15.0)) + Vec3::splat(10.0));
        let mut out = 0.;
        for z in 0..=1 { for y in 0..=1 { for x in 0..=1 {
            let h = crate::noise::hash2(cell.x as i32 + x, cell.y as i32 + y, seed);
            let v = crate::noise::hash_f(h as i32, cell.z as i32 + z, seed ^ 0x6d2b_79f5);
            let w = |axis: f32, side| if side == 0 { 1. - axis } else { axis };
            out += v * w(u.x, x) * w(u.y, y) * w(u.z, z);
        }}}
        out
    }

    #[test]
    fn cached_cells_are_bit_identical_to_the_reference() {
        for k in 0..30000 {
            let p = Vec3::new(k as f32 * 0.0137 - 50.0, (k % 331) as f32 * -0.0291 + 3.0, (k % 97) as f32 * 0.051 - 2.0);
            for seed in [0u32, 17, 0x132b, u32::MAX] {
                assert_eq!(value(p, seed).to_bits(), value_reference(p, seed).to_bits(), "{p} {seed}");
            }
        }
    }

    #[test]
    fn volume_breakup_is_continuous_deterministic_and_does_not_repeat_at_unit_offsets() {
        let p = ProceduralParams { scale: 8., seed: 17, ..ProceduralParams::new(ProceduralKind::Clouds) };
        let mut repeated_difference = 0.;
        let mut seed_difference = 0.;
        let other = ProceduralParams { seed: 18, ..p };
        for i in 0..100 {
            let point = Vec3::new(i as f32 * 0.0137, 0.37, 0.81);
            let a = eval(&p, point.to_array());
            assert_eq!(a, eval(&p, point.to_array()));
            assert!((0.0..=1.0).contains(&a));
            for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
                assert!((a - eval(&p, (point + axis * 1e-6).to_array())).abs() < 0.001);
                repeated_difference += (a - eval(&p, (point + axis).to_array())).abs();
            }
            seed_difference += (a - eval(&other, point.to_array())).abs();
        }
        assert!(repeated_difference / 300. > 0.03);
        assert!(seed_difference / 100. > 0.03);
        let inverted = ProceduralParams { invert: true, ..p };
        assert!((eval(&p, [0.2; 3]) + eval(&inverted, [0.2; 3]) - 1.).abs() < 1e-6);
    }
}

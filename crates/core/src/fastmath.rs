//! Exact replacements for float helpers that compile to library calls on the portable x86-64
//! baseline (no SSE4.1 `roundps`). Same bits as the standard functions for every input.

/// Exactly `x.floor()` (same bits, including -0.0, NaN, infinities and huge values) without the
/// library call `floor` becomes on x86-64 CPUs without SSE4.1.
#[inline]
pub fn floor(x: f32) -> f32 {
    if x.abs() < 8_388_608.0 && x != 0.0 {
        let t = x as i32 as f32;
        if t > x { t - 1.0 } else { t }
    } else {
        x.floor()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_is_bit_identical() {
        let specials = [0.0f32, -0.0, 0.5, -0.5, 1.0, -1.0, 8_388_607.5, -8_388_607.5, 8_388_608.0, -8_388_609.0, 1e20, -1e20, f32::MAX, f32::MIN, f32::MIN_POSITIVE, -f32::MIN_POSITIVE, f32::INFINITY, f32::NEG_INFINITY, f32::EPSILON, -f32::EPSILON, 2.999_999_8, -2.999_999_8];
        for x in specials {
            assert_eq!(floor(x).to_bits(), x.floor().to_bits(), "{x}");
        }
        assert!(floor(f32::NAN).is_nan());
        let mut bits = 0u32;
        for _ in 0..4_000_000 {
            bits = bits.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = f32::from_bits(bits);
            if !x.is_nan() {
                assert_eq!(floor(x).to_bits(), x.floor().to_bits(), "{x}");
            }
        }
    }
}

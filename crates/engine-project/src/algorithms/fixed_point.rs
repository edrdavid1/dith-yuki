//! Q8.8 fixed-point helpers for GPU dither families (`ED-on-GPU.md` §3.0).
//!
//! Error is `i32` in Q8.8 (8 integer bits, 8 fractional). Color `u8` maps as
//! `value << 8`. No floats in the algorithm path.

/// Fractional bits in Q8.8.
pub const Q8: i32 = 8;
/// 1.0 in Q8.8.
pub const ONE_Q8: i32 = 1 << Q8;
/// 255.0 in Q8.8.
pub const MAX_U8_Q8: i32 = 255 << Q8;

/// Floyd–Steinberg weights in Q8.8 (sum to 256).
pub const W7: i32 = (7 << Q8) / 16; // 112
pub const W3: i32 = (3 << Q8) / 16; // 48
pub const W5: i32 = (5 << Q8) / 16; // 80
pub const W1: i32 = (1 << Q8) / 16; // 16

/// Convert `u8` color to Q8.8.
#[inline]
pub fn u8_to_q8(v: u8) -> i32 {
    (v as i32) << Q8
}

/// Round Q8.8 to nearest `u8` (ties away from −∞ via +128).
#[inline]
pub fn q8_to_u8_rounded(v: i32) -> u8 {
    let clamped = v.clamp(0, MAX_U8_Q8);
    ((clamped + 128) >> Q8) as u8
}

/// Clamp adjusted Q8.8 value to the representable `u8` range.
#[inline]
pub fn clamp_adjusted_q8(adjusted: i32) -> i32 {
    adjusted.clamp(0, MAX_U8_Q8)
}

/// Remainder-preserving FS weight split (`ED-on-GPU.md` §3.0.1).
///
/// After distribution: `e7 + e3 + e5 + e1 == error` exactly.
#[inline]
pub fn distribute_fs(error: i32) -> (i32, i32, i32, i32) {
    let e7 = (error * 7) >> 4;
    let e3 = (error * 3) >> 4;
    let e5 = (error * 5) >> 4;
    let e1 = error - e7 - e3 - e5;
    (e7, e3, e5, e1)
}

/// Blue-noise shift in Q8.8 from a 16-bit rank texture sample.
///
/// `rank` is in `0..16384` (permutation). Mapped to `t_q16 ∈ [0, 65532]`, then
/// `shift_q8 = ((t_q16 - 32768) * spread) >> 8`.
///
/// Prefer [`blue_noise_shift_scaled_q8`] when an interval width `A` is known
/// (uniform ±A/2 at `spread=256`).
#[inline]
pub fn blue_noise_shift_q8(rank: u16, spread: u32) -> i32 {
    let t_q16 = (rank as i32) * 4;
    ((t_q16 - 32768) * (spread as i32)) >> 8
}

/// Rank → Q8.8 shift scaled to interval width `a_u8` (display-u8).
///
/// At `spread=256`: `shift_q8 = ((rank*4 − 32768) * (a_u8<<8)) >> 16`
/// (= `((t−32768) * a_u8) >> 8`), so the distribution is uniform on ±`a_u8/2`
/// (in u8). Other spreads multiply by `spread/256` afterward.
#[inline]
pub fn blue_noise_shift_scaled_q8(rank: u16, spread: u32, a_u8: u32) -> i32 {
    let t_q16 = (rank as i32) * 4;
    let a = a_u8.min(255) as i32;
    let at_default = ((t_q16 - 32768) * a) >> 8;
    ((at_default as i64 * spread as i64) >> 8) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_256() {
        assert_eq!(W7 + W3 + W5 + W1, 256);
        assert_eq!(W7, 112);
        assert_eq!(W3, 48);
        assert_eq!(W5, 80);
        assert_eq!(W1, 16);
    }

    #[test]
    fn remainder_preserving_distribution() {
        for e in -65536i32..=65536 {
            let (e7, e3, e5, e1) = distribute_fs(e);
            assert_eq!(e7 + e3 + e5 + e1, e, "e={e}");
        }
    }

    #[test]
    fn clamp_bounds_correct() {
        for input_q8 in (0..=65280).step_by(256) {
            for err in [-32768i32, -1, 0, 1, 32767] {
                let adjusted = input_q8 + err;
                let clamped = clamp_adjusted_q8(adjusted);
                assert!((0..=MAX_U8_Q8).contains(&clamped));
                let color = q8_to_u8_rounded(clamped);
                let error_q8 = clamped - u8_to_q8(color);
                debug_assert!(error_q8.abs() <= 65536);
                let _ = error_q8;
            }
        }
    }
}

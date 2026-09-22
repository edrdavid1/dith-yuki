//! Zhou–Fang threshold-modulation table (SIGGRAPH 2003).
//!
//! Key amplitudes from the published parameter set (also used by `dithr` /
//! Cris Luengo-style implementations). Linearly interpolated over 0..=255.
//! Diffusion weights reuse [`super::ostromoukhov_table`] (same 3-tap family).

/// Key gray levels for modulation amplitude.
const KEY_LEVELS: [u8; 10] = [0, 32, 64, 96, 112, 128, 144, 160, 192, 255];
/// Modulation amplitudes at [`KEY_LEVELS`] (0..=255 scale).
const KEY_AMPLITUDES: [u8; 10] = [0, 87, 133, 163, 184, 199, 184, 163, 133, 87];

const fn interp_u8(x: u16, x0: u16, y0: u16, x1: u16, y1: u16) -> u8 {
    if x1 <= x0 {
        return y0 as u8;
    }
    let dx = x1 - x0;
    let tx = x - x0;
    let dy = y1 as i32 - y0 as i32;
    let num = dy * tx as i32;
    let den = dx as i32;
    let delta = if num >= 0 {
        (num + den / 2) / den
    } else {
        (num - den / 2) / den
    };
    let value = y0 as i32 + delta;
    if value < 0 {
        0
    } else if value > 255 {
        255
    } else {
        value as u8
    }
}

const fn generate_modulation() -> [u8; 256] {
    let mut out = [0_u8; 256];
    let mut i = 0_usize;
    while i < 256 {
        let x = i as u16;
        let mut seg = 0_usize;
        while seg + 1 < KEY_LEVELS.len() {
            let x0 = KEY_LEVELS[seg] as u16;
            let x1 = KEY_LEVELS[seg + 1] as u16;
            if x >= x0 && x <= x1 {
                let y0 = KEY_AMPLITUDES[seg] as u16;
                let y1 = KEY_AMPLITUDES[seg + 1] as u16;
                out[i] = interp_u8(x, x0, y0, x1, y1);
                break;
            }
            seg += 1;
        }
        if seg + 1 == KEY_LEVELS.len() {
            out[i] = KEY_AMPLITUDES[KEY_AMPLITUDES.len() - 1];
        }
        i += 1;
    }
    out
}

/// Per-tone threshold modulation amplitude (0..=255).
pub const MODULATION: [u8; 256] = generate_modulation();

#[inline]
pub fn amplitude_unit(tone: u8) -> f32 {
    MODULATION[tone as usize] as f32 / 255.0
}

/// Deterministic unit noise in `[0, 1)` from global pixel coordinates.
///
/// Hash matches the `dithr` Zhou–Fang path so results are reproducible across
/// tiles (no process RNG).
#[inline]
pub fn noise_unit(gx: i32, gy: i32) -> f32 {
    let mut state = (gx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (gy as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ 0x94D0_49BB_1331_11EB_u64;
    state ^= state >> 30;
    state = state.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    state ^= state >> 27;
    state = state.wrapping_mul(0x94D0_49BB_1331_11EB);
    state ^= state >> 31;
    (state >> 40) as f32 / 16_777_215.0
}

/// Apply Zhou–Fang threshold modulation: `value + (noise−0.5) × amplitude`.
#[inline]
pub fn modulate_unit(value: f32, tone: u8, gx: i32, gy: i32) -> f32 {
    let amp = amplitude_unit(tone);
    let noise = noise_unit(gx, gy);
    (value + (noise - 0.5) * amp).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_amplitudes_match_published() {
        assert_eq!(MODULATION[0], 0);
        assert_eq!(MODULATION[64], 133);
        assert_eq!(MODULATION[128], 199);
        assert_eq!(MODULATION[255], 87);
    }

    #[test]
    fn noise_is_deterministic_and_in_unit_range() {
        let a = noise_unit(10, 20);
        let b = noise_unit(10, 20);
        assert_eq!(a, b);
        assert!((0.0..1.0).contains(&a));
        assert_ne!(noise_unit(10, 20), noise_unit(11, 20));
    }
}

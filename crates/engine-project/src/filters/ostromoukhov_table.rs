//! Ostromoukhov variable-coefficient error diffusion (SIGGRAPH 2001).
//!
//! Appendix I of Victor Ostromoukhov, "A Simple and Efficient Error-Diffusion
//! Algorithm". Entries are `(A10, A₋₁₁, A01)` for tones `0..=127`; tones
//! `128..=255` mirror via `255 - tone`. Neighbors: right `(1,0)`, down-left
//! `(-1,1)`, down `(0,1)`.

/// Published integer coefficients for tones 0..=127 (Appendix I).
pub const COEFFICIENTS: [[u16; 3]; 128] = [
    [13, 0, 5],
    [13, 0, 5],
    [21, 0, 10],
    [7, 0, 4],
    [8, 0, 5],
    [47, 3, 28],
    [23, 3, 13],
    [15, 3, 8],
    [22, 6, 11],
    [43, 15, 20],
    [7, 3, 3],
    [501, 224, 211],
    [249, 116, 103],
    [165, 80, 67],
    [123, 62, 49],
    [489, 256, 191],
    [81, 44, 31],
    [483, 272, 181],
    [60, 35, 22],
    [53, 32, 19],
    [237, 148, 83],
    [471, 304, 161],
    [3, 2, 1],
    [481, 314, 185],
    [354, 226, 155],
    [1389, 866, 685],
    [227, 138, 125],
    [267, 158, 163],
    [327, 188, 220],
    [61, 34, 45],
    [627, 338, 505],
    [1227, 638, 1075],
    [20, 10, 19],
    [1937, 1000, 1767],
    [977, 520, 855],
    [657, 360, 551],
    [71, 40, 57],
    [2005, 1160, 1539],
    [337, 200, 247],
    [2039, 1240, 1425],
    [257, 160, 171],
    [691, 440, 437],
    [1045, 680, 627],
    [301, 200, 171],
    [177, 120, 95],
    [2141, 1480, 1083],
    [1079, 760, 513],
    [725, 520, 323],
    [137, 100, 57],
    [2209, 1640, 855],
    [53, 40, 19],
    [2243, 1720, 741],
    [565, 440, 171],
    [759, 600, 209],
    [1147, 920, 285],
    [2311, 1880, 513],
    [97, 80, 19],
    [335, 280, 57],
    [1181, 1000, 171],
    [793, 680, 95],
    [599, 520, 57],
    [2413, 2120, 171],
    [405, 360, 19],
    [2447, 2200, 57],
    [11, 10, 0],
    [158, 151, 3],
    [178, 179, 7],
    [1030, 1091, 63],
    [248, 277, 21],
    [318, 375, 35],
    [458, 571, 63],
    [878, 1159, 147],
    [5, 7, 1],
    [172, 181, 37],
    [97, 76, 22],
    [72, 41, 17],
    [119, 47, 29],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [4, 1, 1],
    [65, 18, 17],
    [95, 29, 26],
    [185, 62, 53],
    [30, 11, 9],
    [35, 14, 11],
    [85, 37, 28],
    [55, 26, 19],
    [80, 41, 29],
    [155, 86, 59],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [5, 3, 2],
    [305, 176, 119],
    [155, 86, 59],
    [105, 56, 39],
    [80, 41, 29],
    [65, 32, 23],
    [55, 26, 19],
    [335, 152, 113],
    [85, 37, 28],
    [115, 48, 37],
    [35, 14, 11],
    [355, 136, 109],
    [30, 11, 9],
    [365, 128, 107],
    [185, 62, 53],
    [25, 8, 7],
    [95, 29, 26],
    [385, 112, 103],
    [65, 18, 17],
    [395, 104, 101],
    [4, 1, 1],
];

/// Integer coefficients for a tone in `0..=255` (mirrored above 127).
#[inline]
pub fn coefficients(tone: u8) -> [u16; 3] {
    let index = if tone > 127 { 255 - tone } else { tone };
    COEFFICIENTS[index as usize]
}

/// Normalized `(dx, dy, weight)` offsets for `distribute_kernel`.
///
/// Layout: right, down-left, down — matches Appendix I / libpipi / ditherlib.
#[inline]
pub fn normalized_offsets(tone: u8) -> [(i32, i32, f32); 3] {
    let [a10, a_11, a01] = coefficients(tone);
    let m = (a10 as f32) + (a_11 as f32) + (a01 as f32);
    let m = if m > 0.0 { m } else { 1.0 };
    [
        (1, 0, a10 as f32 / m),
        (-1, 1, a_11 as f32 / m),
        (0, 1, a01 as f32 / m),
    ]
}

/// Map linear RGB (or equal-channel luma) in `0..=1` to an 8-bit tone index.
#[inline]
pub fn tone_from_unit(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appendix_i_key_levels_match_published_integers() {
        // Highlighted key levels from SIGGRAPH 2001 Appendix I / ditherlib.
        assert_eq!(coefficients(0), [13, 0, 5]);
        assert_eq!(coefficients(64), [11, 10, 0]);
        assert_eq!(coefficients(127), [4, 1, 1]);
        assert_eq!(coefficients(255), [13, 0, 5]); // mirror of 0
        assert_eq!(coefficients(191), [11, 10, 0]); // mirror of 64
    }

    #[test]
    fn normalized_weights_sum_to_one() {
        for tone in [0u8, 22, 64, 85, 127, 200, 255] {
            let offs = normalized_offsets(tone);
            let sum: f32 = offs.iter().map(|o| o.2).sum();
            assert!((sum - 1.0).abs() < 1e-5, "tone {tone}: weight sum {sum}");
        }
    }
}

//! Colour targets and ANSI / xterm palette snapping (Oklab).

use engine_color::oklab::{linear_to_oklab, oklab_dist_sq, LinRgb};
use engine_color::palette::{linear_to_srgb, srgb_to_linear};

use crate::grid::CellColor;

/// How cell colours are quantized for storage / exporters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorTarget {
    /// Keep full sRGB8 (`CellColor::Rgb`).
    TrueColor,
    /// Snap to the xterm 256-colour cube (`CellColor::Indexed`).
    Xterm256,
    /// Snap to a 16-colour ANSI palette (`CellColor::Indexed` 0..=15).
    Ansi16(Ansi16Palette),
}

/// Built-in ANSI-16 palettes (§3.6 / §9.3 — default VGA).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Ansi16Palette {
    #[default]
    Vga,
    Xterm,
    Windows10,
}

impl Ansi16Palette {
    /// Classic 16 sRGB colours.
    pub fn colours(self) -> [[u8; 3]; 16] {
        match self {
            Self::Vga => VGA_16,
            Self::Xterm => XTERM_16,
            Self::Windows10 => WINDOWS10_16,
        }
    }
}

/// IBM / VGA text-mode colours (CP437 / BBS look).
pub const VGA_16: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xAA, 0x00, 0x00],
    [0x00, 0xAA, 0x00],
    [0xAA, 0x55, 0x00],
    [0x00, 0x00, 0xAA],
    [0xAA, 0x00, 0xAA],
    [0x00, 0xAA, 0xAA],
    [0xAA, 0xAA, 0xAA],
    [0x55, 0x55, 0x55],
    [0xFF, 0x55, 0x55],
    [0x55, 0xFF, 0x55],
    [0xFF, 0xFF, 0x55],
    [0x55, 0x55, 0xFF],
    [0xFF, 0x55, 0xFF],
    [0x55, 0xFF, 0xFF],
    [0xFF, 0xFF, 0xFF],
];

/// xterm default 16.
pub const XTERM_16: [[u8; 3]; 16] = [
    [0x00, 0x00, 0x00],
    [0xCD, 0x00, 0x00],
    [0x00, 0xCD, 0x00],
    [0xCD, 0xCD, 0x00],
    [0x00, 0x00, 0xEE],
    [0xCD, 0x00, 0xCD],
    [0x00, 0xCD, 0xCD],
    [0xE5, 0xE5, 0xE5],
    [0x7F, 0x7F, 0x7F],
    [0xFF, 0x00, 0x00],
    [0x00, 0xFF, 0x00],
    [0xFF, 0xFF, 0x00],
    [0x5C, 0x5C, 0xFF],
    [0xFF, 0x00, 0xFF],
    [0x00, 0xFF, 0xFF],
    [0xFF, 0xFF, 0xFF],
];

/// Windows 10 console defaults.
pub const WINDOWS10_16: [[u8; 3]; 16] = [
    [0x0C, 0x0C, 0x0C],
    [0xC5, 0x0F, 0x1F],
    [0x13, 0xA1, 0x0E],
    [0xC1, 0x9C, 0x00],
    [0x00, 0x37, 0xDA],
    [0x88, 0x17, 0x98],
    [0x3A, 0x96, 0xDD],
    [0xCC, 0xCC, 0xCC],
    [0x76, 0x76, 0x76],
    [0xE7, 0x48, 0x56],
    [0x16, 0xC6, 0x0C],
    [0xF9, 0xF1, 0xA5],
    [0x3B, 0x78, 0xFF],
    [0xB4, 0x00, 0x9E],
    [0x61, 0xD6, 0xD6],
    [0xF2, 0xF2, 0xF2],
];

/// Resolve a cell colour to sRGB8 using an optional ANSI-16 / xterm table.
pub fn resolve_rgb(c: CellColor, target: ColorTarget) -> [u8; 3] {
    match c {
        CellColor::Rgb(rgb) => rgb,
        CellColor::Indexed(i) => match target {
            ColorTarget::Ansi16(p) => p.colours()[i as usize % 16],
            ColorTarget::Xterm256 => xterm256_colour(i),
            ColorTarget::TrueColor => [i, i, i],
        },
    }
}

/// Snap an sRGB8 colour to the given target.
pub fn quantize_rgb(rgb: [u8; 3], target: ColorTarget) -> CellColor {
    match target {
        ColorTarget::TrueColor => CellColor::Rgb(rgb),
        ColorTarget::Ansi16(p) => CellColor::Indexed(nearest_palette(rgb, &p.colours())),
        ColorTarget::Xterm256 => CellColor::Indexed(nearest_xterm256(rgb)),
    }
}

/// Snap both fg and bg (independent nearest).
pub fn quantize_pair(fg: [u8; 3], bg: [u8; 3], target: ColorTarget) -> (CellColor, CellColor) {
    (quantize_rgb(fg, target), quantize_rgb(bg, target))
}

fn rgb8_to_oklab(rgb: [u8; 3]) -> engine_color::Oklab {
    linear_to_oklab(LinRgb {
        r: srgb_to_linear(rgb[0]),
        g: srgb_to_linear(rgb[1]),
        b: srgb_to_linear(rgb[2]),
    })
}

fn nearest_palette(rgb: [u8; 3], palette: &[[u8; 3]]) -> u8 {
    let lab = rgb8_to_oklab(rgb);
    let mut best_i = 0u8;
    let mut best_d = f32::MAX;
    for (i, &c) in palette.iter().enumerate() {
        let d = oklab_dist_sq(lab, rgb8_to_oklab(c));
        let i = i as u8;
        if d < best_d || (d == best_d && i < best_i) {
            best_d = d;
            best_i = i;
        }
    }
    best_i
}

/// Build the standard xterm-256 colour at index `i`.
pub fn xterm256_colour(i: u8) -> [u8; 3] {
    if i < 16 {
        return XTERM_16[i as usize];
    }
    if i < 232 {
        let n = i - 16;
        let r = n / 36;
        let g = (n / 6) % 6;
        let b = n % 6;
        let level = |v: u8| -> u8 {
            if v == 0 {
                0
            } else {
                55 + 40 * v
            }
        };
        [level(r), level(g), level(b)]
    } else {
        let v = 8 + 10 * (i - 232);
        [v, v, v]
    }
}

fn nearest_xterm256(rgb: [u8; 3]) -> u8 {
    // Direct cube estimate + grayscale + system-16; then refine by Oklab among candidates.
    let mut candidates = Vec::with_capacity(20);
    for i in 0..16u8 {
        candidates.push(i);
    }
    let to_cube = |c: u8| -> u8 {
        if c < 48 {
            0
        } else if c < 115 {
            1
        } else {
            ((c - 35) / 40).min(5)
        }
    };
    let ri = to_cube(rgb[0]);
    let gi = to_cube(rgb[1]);
    let bi = to_cube(rgb[2]);
    candidates.push(16 + 36 * ri + 6 * gi + bi);
    // Greyscale estimate.
    let gray = ((rgb[0] as u16 + rgb[1] as u16 + rgb[2] as u16) / 3) as i32;
    let gray_i = ((gray - 8) / 10).clamp(0, 23) as u8;
    candidates.push(232 + gray_i);
    if gray_i > 0 {
        candidates.push(232 + gray_i - 1);
    }
    if gray_i < 23 {
        candidates.push(232 + gray_i + 1);
    }
    // Neighbour cube cells.
    for dr in [-1i8, 0, 1] {
        for dg in [-1i8, 0, 1] {
            for db in [-1i8, 0, 1] {
                let r = ri as i8 + dr;
                let g = gi as i8 + dg;
                let b = bi as i8 + db;
                if (0..6).contains(&r) && (0..6).contains(&g) && (0..6).contains(&b) {
                    candidates.push(16 + 36 * r as u8 + 6 * g as u8 + b as u8);
                }
            }
        }
    }
    candidates.sort_unstable();
    candidates.dedup();

    let lab = rgb8_to_oklab(rgb);
    let mut best_i = candidates[0];
    let mut best_d = f32::MAX;
    for &i in &candidates {
        let d = oklab_dist_sq(lab, rgb8_to_oklab(xterm256_colour(i)));
        if d < best_d || (d == best_d && i < best_i) {
            best_d = d;
            best_i = i;
        }
    }
    best_i
}

/// Linear RGB mean of a cell → sRGB8.
pub fn mean_cell_srgb8(cell_rgb: &[[f32; 3]]) -> [u8; 3] {
    if cell_rgb.is_empty() {
        return [0, 0, 0];
    }
    let mut sum = [0.0f64; 3];
    for p in cell_rgb {
        sum[0] += p[0] as f64;
        sum[1] += p[1] as f64;
        sum[2] += p[2] as f64;
    }
    let n = cell_rgb.len() as f64;
    [
        linear_to_srgb((sum[0] / n) as f32),
        linear_to_srgb((sum[1] / n) as f32),
        linear_to_srgb((sum[2] / n) as f32),
    ]
}

/// Alpha-weighted mean linear RGB of a cell → sRGB8.
/// Fully transparent pixels are ignored; empty weight → black.
pub fn mean_cell_srgb8_alpha(
    rgba: &[f32],
    width: u32,
    height: u32,
    ox: u32,
    oy: u32,
    cell_w: u32,
    cell_h: u32,
) -> [u8; 3] {
    debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
    let mut sum = [0.0f64; 3];
    let mut wsum = 0.0f64;
    for ly in 0..cell_h {
        for lx in 0..cell_w {
            let gx = ox + lx;
            let gy = oy + ly;
            if gx >= width || gy >= height {
                continue;
            }
            let i = ((gy * width + gx) * 4) as usize;
            let a = rgba[i + 3].clamp(0.0, 1.0) as f64;
            if a <= 0.0 {
                continue;
            }
            sum[0] += rgba[i] as f64 * a;
            sum[1] += rgba[i + 1] as f64 * a;
            sum[2] += rgba[i + 2] as f64 * a;
            wsum += a;
        }
    }
    if wsum <= 0.0 {
        return [0, 0, 0];
    }
    [
        linear_to_srgb((sum[0] / wsum) as f32),
        linear_to_srgb((sum[1] / wsum) as f32),
        linear_to_srgb((sum[2] / wsum) as f32),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vga_black_white_exact() {
        assert_eq!(
            quantize_rgb([0, 0, 0], ColorTarget::Ansi16(Ansi16Palette::Vga)),
            CellColor::Indexed(0)
        );
        assert_eq!(
            quantize_rgb([255, 255, 255], ColorTarget::Ansi16(Ansi16Palette::Vga)),
            CellColor::Indexed(15)
        );
    }

    #[test]
    fn xterm256_cube_round_trip() {
        // Pure cube primary at index 196 = #FF0000 in xterm (16 + 36*5).
        // System bright-red (index 9) is the same RGB — either index is correct.
        let c = xterm256_colour(196);
        assert_eq!(c, [255, 0, 0]);
        let i = nearest_xterm256(c);
        assert_eq!(xterm256_colour(i), c);
        assert!(i == 9 || i == 196, "got {i}");
    }

    #[test]
    fn resolve_indexed_vga() {
        let c = CellColor::Indexed(4);
        assert_eq!(
            resolve_rgb(c, ColorTarget::Ansi16(Ansi16Palette::Vga)),
            [0x00, 0x00, 0xAA]
        );
    }
}

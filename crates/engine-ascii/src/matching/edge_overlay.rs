//! Edge overlay (Acerola): DoG → Sobel → orientation vote → `| — / \` glyphs.

use crate::atlas::GlyphAtlas;

/// Edge-overlay parameters. When enabled, cells with mean edge strength above
/// `tau` (0..=255) replace the base match with an orientation glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeOverlay {
    pub enabled: bool,
    /// Mean Sobel magnitude threshold (0..=255).
    pub tau: u8,
}

impl Default for EdgeOverlay {
    fn default() -> Self {
        Self {
            enabled: false,
            tau: 40,
        }
    }
}

/// Four orientation bins → preferred codepoints (tried in order).
const ORIENT_CHARS: [[char; 3]; 4] = [
    ['—', '─', '-'],    // 0° horizontal
    ['/', '/', '/'],    // 45°
    ['|', '│', '|'],    // 90° vertical
    ['\\', '\\', '\\'], // 135°
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeOrient {
    Horizontal = 0,
    DiagUp = 1,
    Vertical = 2,
    DiagDown = 3,
}

/// Precomputed edge field for an image (luma DoG + Sobel).
pub struct EdgeField {
    pub width: u32,
    pub height: u32,
    /// Per-pixel Sobel magnitude 0..=255.
    pub mag: Vec<u8>,
    /// Per-pixel orientation bin 0..=3.
    pub orient: Vec<u8>,
}

impl EdgeField {
    /// Build from linear RGBA. Cheap 3×3 box blurs approximate DoG.
    pub fn from_rgba(rgba: &[f32], width: u32, height: u32) -> Self {
        let n = (width * height) as usize;
        let mut luma = vec![0u8; n];
        for i in 0..n {
            let r = rgba[i * 4];
            let g = rgba[i * 4 + 1];
            let b = rgba[i * 4 + 2];
            let y = (0.2126 * r + 0.7152 * g + 0.0722 * b).clamp(0.0, 1.0);
            luma[i] = (y * 255.0).round() as u8;
        }
        let blur1 = box_blur_3(&luma, width, height);
        let blur2 = box_blur_3(&blur1, width, height);
        let blur2 = box_blur_3(&blur2, width, height);
        let mut dog = vec![0i16; n];
        for i in 0..n {
            dog[i] = blur1[i] as i16 - blur2[i] as i16;
        }
        let mut mag = vec![0u8; n];
        let mut orient = vec![0u8; n];
        for y in 1..height.saturating_sub(1) {
            for x in 1..width.saturating_sub(1) {
                let at = |dx: i32, dy: i32| -> i32 {
                    let xx = (x as i32 + dx) as u32;
                    let yy = (y as i32 + dy) as u32;
                    dog[(yy * width + xx) as usize] as i32
                };
                let gx =
                    -at(-1, -1) + at(1, -1) - 2 * at(-1, 0) + 2 * at(1, 0) - at(-1, 1) + at(1, 1);
                let gy =
                    -at(-1, -1) - 2 * at(0, -1) - at(1, -1) + at(-1, 1) + 2 * at(0, 1) + at(1, 1);
                let m = ((gx * gx + gy * gy) as f32).sqrt().min(255.0) as u8;
                // Angle of the gradient normal; edge direction is perpendicular.
                let angle = (gy as f32).atan2(gx as f32); // −π..=π
                let edge = angle + std::f32::consts::FRAC_PI_2;
                let deg = edge.to_degrees().rem_euclid(180.0);
                // Bins centred on 0, 45, 90, 135.
                let bin = if !(22.5..157.5).contains(&deg) {
                    0
                } else if deg < 67.5 {
                    1
                } else if deg < 112.5 {
                    2
                } else {
                    3
                };
                let i = (y * width + x) as usize;
                mag[i] = m;
                orient[i] = bin;
            }
        }
        Self {
            width,
            height,
            mag,
            orient,
        }
    }

    /// Majority orientation for a cell; `None` if mean magnitude < `tau`.
    pub fn cell_orient(
        &self,
        ox: u32,
        oy: u32,
        cell_w: u32,
        cell_h: u32,
        tau: u8,
    ) -> Option<EdgeOrient> {
        let mut votes = [0u32; 4];
        let mut sum_m = 0u32;
        let mut n = 0u32;
        for ly in 0..cell_h {
            for lx in 0..cell_w {
                let x = ox + lx;
                let y = oy + ly;
                if x >= self.width || y >= self.height {
                    continue;
                }
                let i = (y * self.width + x) as usize;
                let m = self.mag[i] as u32;
                sum_m += m;
                votes[self.orient[i] as usize] += m.max(1);
                n += 1;
            }
        }
        if n == 0 {
            return None;
        }
        let mean = (sum_m / n) as u8;
        if mean < tau {
            return None;
        }
        let mut best = 0usize;
        for i in 1..4 {
            if votes[i] > votes[best] || (votes[i] == votes[best] && i < best) {
                best = i;
            }
        }
        Some(match best {
            0 => EdgeOrient::Horizontal,
            1 => EdgeOrient::DiagUp,
            2 => EdgeOrient::Vertical,
            _ => EdgeOrient::DiagDown,
        })
    }
}

/// Resolve an orientation to an atlas glyph index, or `None` if no candidate exists.
pub fn orient_glyph(atlas: &GlyphAtlas, orient: EdgeOrient) -> Option<u16> {
    let chars = &ORIENT_CHARS[orient as usize];
    for &ch in chars {
        if let Some(i) = atlas.glyphs.iter().position(|g| g.ch == ch) {
            return Some(i as u16);
        }
    }
    None
}

fn box_blur_3(src: &[u8], w: u32, h: u32) -> Vec<u8> {
    let mut tmp = vec![0u8; src.len()];
    let mut out = vec![0u8; src.len()];
    // Horizontal.
    for y in 0..h {
        for x in 0..w {
            let mut s = 0u32;
            let mut n = 0u32;
            for dx in -1i32..=1 {
                let xx = x as i32 + dx;
                if xx >= 0 && xx < w as i32 {
                    s += src[(y * w + xx as u32) as usize] as u32;
                    n += 1;
                }
            }
            tmp[(y * w + x) as usize] = (s / n) as u8;
        }
    }
    // Vertical.
    for y in 0..h {
        for x in 0..w {
            let mut s = 0u32;
            let mut n = 0u32;
            for dy in -1i32..=1 {
                let yy = y as i32 + dy;
                if yy >= 0 && yy < h as i32 {
                    s += tmp[(yy as u32 * w + x) as usize] as u32;
                    n += 1;
                }
            }
            out[(y * w + x) as usize] = (s / n) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::font::{BundledFont, FontFace};
    use crate::symbols::SymbolSet;

    #[test]
    fn vertical_edge_votes_vertical() {
        let w = 32u32;
        let h = 32u32;
        let mut rgba = vec![1.0f32; (w * h * 4) as usize];
        // Left half black, right half white → vertical edge in the middle.
        for y in 0..h {
            for x in 0..w / 2 {
                let i = ((y * w + x) * 4) as usize;
                rgba[i] = 0.0;
                rgba[i + 1] = 0.0;
                rgba[i + 2] = 0.0;
            }
        }
        let field = EdgeField::from_rgba(&rgba, w, h);
        let o = field.cell_orient(w / 2 - 4, 8, 8, 16, 10);
        assert_eq!(o, Some(EdgeOrient::Vertical));
    }

    #[test]
    fn flat_region_below_tau() {
        let w = 16u32;
        let h = 16u32;
        let rgba = vec![0.5f32; (w * h * 4) as usize];
        let field = EdgeField::from_rgba(&rgba, w, h);
        assert_eq!(field.cell_orient(0, 0, 8, 8, 40), None);
    }

    #[test]
    fn printable_ascii_has_slash_pipe_backslash() {
        let atlas = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::IbmPlexMono),
            &SymbolSet::PrintableAscii,
            &AtlasOptions {
                size: FontSize::Px(16.0),
                antialias: true,
                hinting: true,
            },
        )
        .unwrap();
        assert!(orient_glyph(&atlas, EdgeOrient::Vertical).is_some());
        assert!(orient_glyph(&atlas, EdgeOrient::DiagUp).is_some());
        assert!(orient_glyph(&atlas, EdgeOrient::DiagDown).is_some());
        assert!(orient_glyph(&atlas, EdgeOrient::Horizontal).is_some());
    }
}

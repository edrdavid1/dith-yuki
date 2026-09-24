//! Symbol sets: which characters the matcher may choose from.
//!
//! Block elements, quadrants, sextants, octants and braille are drawn from
//! exact geometry ([`Procedural`]) regardless of the font, because fonts
//! disagree on these glyphs and many lack sextants/octants entirely. Every
//! other character is rasterized from the font.

mod block_tables;

pub use block_tables::{CP437, OCTANTS, SEXTANTS};

use crate::fnv1a64;

/// Classic 10-character density ramp (Paul Bourke).
pub const BOURKE_10: &str = " .:-=+*#%@";
/// Classic 70-character density ramp (Paul Bourke).
pub const BOURKE_70: &str =
    "$@B%8&WM#*oahkbdpqwmZO0QLCJUYXzcvunxrjft/\\|()1{}[]?-_+~<>i!lI;:,\"^`'. ";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SymbolSet {
    Bourke10,
    Bourke70,
    PrintableAscii,
    AsciiBoxDrawing,
    Blocks,
    Quadrants,
    Sextants,
    Octants,
    Braille,
    Cp437,
    Custom(String),
}

/// How a symbol's coverage raster is produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GlyphSource {
    Font,
    Procedural(Procedural),
}

/// Exact geometric definition of a block-type character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Procedural {
    /// `cols × rows` grid of equal sub-cells; bit `n` fills sub-cell `n`
    /// (row-major, left column first).
    SubCells { cols: u8, rows: u8, mask: u8 },
    /// Filled rectangle in eighths of the cell, `x0 < x1`, `y0 < y1`, all `0..=8`.
    Eighths { x0: u8, y0: u8, x1: u8, y1: u8 },
    /// Braille cell: bit `n` raises dot `n + 1` (Unicode dot numbering).
    Braille(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol {
    pub ch: char,
    pub source: GlyphSource,
}

impl SymbolSet {
    /// Characters in this set, deduplicated, in definition order.
    pub fn chars(&self) -> Vec<char> {
        let raw: Vec<char> = match self {
            SymbolSet::Bourke10 => BOURKE_10.chars().collect(),
            SymbolSet::Bourke70 => BOURKE_70.chars().collect(),
            SymbolSet::PrintableAscii => (0x20u8..=0x7E).map(char::from).collect(),
            SymbolSet::AsciiBoxDrawing => (0x20u32..=0x7E)
                .chain(0x2500..=0x257F)
                .filter_map(char::from_u32)
                .collect(),
            SymbolSet::Blocks => " █▀▄▌▐▁▂▃▅▆▇▏▎▍▋▊▉▔▕░▒▓".chars().collect(),
            SymbolSet::Quadrants => " ▘▝▀▖▌▞▛▗▚▐▜▄▙▟█".chars().collect(),
            SymbolSet::Sextants => SEXTANTS.to_vec(),
            SymbolSet::Octants => OCTANTS.to_vec(),
            SymbolSet::Braille => (0x2800u32..=0x28FF).filter_map(char::from_u32).collect(),
            SymbolSet::Cp437 => CP437.to_vec(),
            SymbolSet::Custom(s) => s.chars().filter(|c| !c.is_control()).collect(),
        };
        let mut seen = std::collections::HashSet::with_capacity(raw.len());
        raw.into_iter().filter(|c| seen.insert(*c)).collect()
    }

    pub fn symbols(&self) -> Vec<Symbol> {
        self.chars()
            .into_iter()
            .map(|ch| Symbol {
                ch,
                source: procedural_for(ch).map_or(GlyphSource::Font, GlyphSource::Procedural),
            })
            .collect()
    }

    /// Stable key for atlas caching.
    pub fn cache_key(&self) -> u64 {
        let mut bytes = Vec::new();
        for ch in self.chars() {
            bytes.extend_from_slice(&(ch as u32).to_le_bytes());
        }
        fnv1a64(&bytes)
    }
}

/// Geometry for characters that are always drawn procedurally.
pub fn procedural_for(ch: char) -> Option<Procedural> {
    let cp = ch as u32;
    let eighths = |x0, y0, x1, y1| Some(Procedural::Eighths { x0, y0, x1, y1 });
    match cp {
        0x2588 => eighths(0, 0, 8, 8),
        0x2580 => eighths(0, 0, 8, 4),
        0x2581..=0x2587 => eighths(0, 8 - (cp - 0x2580) as u8, 8, 8),
        0x2589..=0x258F => eighths(0, 0, 8 - (cp - 0x2588) as u8, 8),
        0x2590 => eighths(4, 0, 8, 8),
        0x2594 => eighths(0, 0, 8, 1),
        0x2595 => eighths(7, 0, 8, 8),
        0x1FB82 => eighths(0, 0, 8, 2),
        0x1FB85 => eighths(0, 0, 8, 6),
        0x2596..=0x259F => {
            // Bits: 0 upper-left, 1 upper-right, 2 lower-left, 3 lower-right.
            const MASKS: [u8; 10] = [
                0b0100, 0b1000, 0b0001, 0b1101, 0b1001, 0b0111, 0b1011, 0b0010, 0b0110, 0b1110,
            ];
            Some(Procedural::SubCells {
                cols: 2,
                rows: 2,
                mask: MASKS[(cp - 0x2596) as usize],
            })
        }
        0x2800..=0x28FF => Some(Procedural::Braille((cp - 0x2800) as u8)),
        _ => {
            if let Some(mask) = pattern_in(&SEXTANTS, ch) {
                return Some(Procedural::SubCells {
                    cols: 2,
                    rows: 3,
                    mask,
                });
            }
            if let Some(mask) = pattern_in(&OCTANTS, ch) {
                return Some(Procedural::SubCells {
                    cols: 2,
                    rows: 4,
                    mask,
                });
            }
            None
        }
    }
}

/// Pattern index of `ch` in a generated table, skipping the space entry.
fn pattern_in(table: &[char], ch: char) -> Option<u8> {
    if ch == ' ' {
        return None;
    }
    table.iter().position(|&c| c == ch).map(|p| p as u8)
}

/// Integer sub-cell edge: `round(len * k / parts)`, halves rounded up.
#[inline]
pub(crate) fn edge(len: u32, k: u32, parts: u32) -> u32 {
    (len * k * 2 + parts) / (parts * 2)
}

impl Procedural {
    /// Rasterize into a `w × h` coverage buffer (0 = empty, 255 = full).
    pub fn rasterize(self, w: u32, h: u32, out: &mut [u8]) {
        debug_assert_eq!(out.len(), (w * h) as usize);
        out.fill(0);
        match self {
            Procedural::SubCells { cols, rows, mask } => {
                let (cols, rows) = (cols as u32, rows as u32);
                for n in 0..cols * rows {
                    if mask >> n & 1 == 0 {
                        continue;
                    }
                    let (c, r) = (n % cols, n / cols);
                    fill_rect(
                        out,
                        w,
                        edge(w, c, cols),
                        edge(h, r, rows),
                        edge(w, c + 1, cols),
                        edge(h, r + 1, rows),
                    );
                }
            }
            Procedural::Eighths { x0, y0, x1, y1 } => fill_rect(
                out,
                w,
                edge(w, x0 as u32, 8),
                edge(h, y0 as u32, 8),
                edge(w, x1 as u32, 8),
                edge(h, y1 as u32, 8),
            ),
            Procedural::Braille(dots) => rasterize_braille(dots, w, h, out),
        }
    }
}

fn fill_rect(out: &mut [u8], w: u32, x0: u32, y0: u32, x1: u32, y1: u32) {
    for y in y0..y1 {
        let row = (y * w) as usize;
        out[row + x0 as usize..row + x1 as usize].fill(255);
    }
}

/// Unicode braille dot `n` (1-based) → `(column, row)` in the 2×4 grid.
const BRAILLE_DOTS: [(u32, u32); 8] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (1, 2),
    (0, 3),
    (1, 3),
];

/// Round dots centred in each 2×4 sub-cell, radius 35% of the smaller sub-cell
/// side, antialiased with 4×4 supersampling (integer coverage counts).
fn rasterize_braille(dots: u8, w: u32, h: u32, out: &mut [u8]) {
    const SS: u32 = 4;
    let sub_w = w as f32 / 2.0;
    let sub_h = h as f32 / 4.0;
    let r = 0.35 * sub_w.min(sub_h);
    let r2 = r * r;
    for (bit, &(c, row)) in BRAILLE_DOTS.iter().enumerate() {
        if dots >> bit & 1 == 0 {
            continue;
        }
        let cx = (c as f32 + 0.5) * sub_w;
        let cy = (row as f32 + 0.5) * sub_h;
        let x_lo = (cx - r).floor().max(0.0) as u32;
        let x_hi = ((cx + r).ceil() as u32).min(w);
        let y_lo = (cy - r).floor().max(0.0) as u32;
        let y_hi = ((cy + r).ceil() as u32).min(h);
        for y in y_lo..y_hi {
            for x in x_lo..x_hi {
                let mut hits = 0u32;
                for sy in 0..SS {
                    for sx in 0..SS {
                        let px = x as f32 + (sx as f32 + 0.5) / SS as f32 - cx;
                        let py = y as f32 + (sy as f32 + 0.5) / SS as f32 - cy;
                        if px * px + py * py <= r2 {
                            hits += 1;
                        }
                    }
                }
                let v = (hits * 255 + SS * SS / 2) / (SS * SS);
                let i = (y * w + x) as usize;
                out[i] = out[i].max(v as u8);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(p: Procedural, w: u32, h: u32) -> Vec<u8> {
        let mut out = vec![0; (w * h) as usize];
        p.rasterize(w, h, &mut out);
        out
    }

    #[test]
    fn tables_have_expected_shape() {
        assert_eq!(SEXTANTS[0], ' ');
        assert_eq!(SEXTANTS[63], '█');
        assert_eq!(SEXTANTS[0b010101], '▌');
        assert_eq!(SEXTANTS[0b101010], '▐');
        assert_eq!(SEXTANTS[1], '\u{1FB00}');
        assert_eq!(OCTANTS[0], ' ');
        assert_eq!(OCTANTS[255], '█');
        assert_eq!(OCTANTS[0b0000_1111], '▀');
        assert_eq!(OCTANTS[0b0000_0100], '\u{1CD00}');
        assert_eq!(CP437[0xDB], '█');
        assert_eq!(CP437[0x01], '☺');
        assert_eq!(CP437[0x7F], '⌂');
    }

    #[test]
    fn every_sextant_and_octant_pattern_round_trips() {
        // Characters shared with other blocks (halves, quadrants, quarters) resolve to
        // equivalent geometry, so compare rasters rather than enum variants.
        for (cols, rows, table) in [(2u8, 3u8, &SEXTANTS[..]), (2, 4, &OCTANTS[..])] {
            for (pattern, &ch) in table.iter().enumerate() {
                let expected = filled(
                    Procedural::SubCells {
                        cols,
                        rows,
                        mask: pattern as u8,
                    },
                    8,
                    24,
                );
                let got = match procedural_for(ch) {
                    Some(p) => filled(p, 8, 24),
                    None => {
                        assert_eq!(ch, ' ', "only space is non-procedural");
                        vec![0; 8 * 24]
                    }
                };
                assert_eq!(got, expected, "{ch:?} pattern {pattern:#b}");
            }
        }
    }

    #[test]
    fn quadrants_cover_all_sixteen_patterns() {
        let mut rasters: Vec<Vec<u8>> = SymbolSet::Quadrants
            .symbols()
            .into_iter()
            .map(|s| match s.source {
                GlyphSource::Procedural(p) => filled(p, 8, 16),
                GlyphSource::Font => vec![0; 8 * 16],
            })
            .collect();
        rasters.sort();
        rasters.dedup();
        assert_eq!(rasters.len(), 16);
    }

    #[test]
    fn eighth_blocks_have_exact_extent() {
        // 8×16 cell: one eighth is 1 px wide / 2 px tall.
        let lower_three_eighths = filled(procedural_for('▃').unwrap(), 8, 16);
        assert_eq!(lower_three_eighths.iter().filter(|&&v| v == 255).count(), 8 * 6);
        assert!(lower_three_eighths[..8 * 10].iter().all(|&v| v == 0));
        let left_quarter = filled(procedural_for('▎').unwrap(), 8, 16);
        for y in 0..16 {
            assert_eq!(&left_quarter[y * 8..y * 8 + 8], &[255, 255, 0, 0, 0, 0, 0, 0]);
        }
    }

    #[test]
    fn complementary_patterns_tile_the_cell() {
        for (w, h) in [(7, 14), (10, 20), (9, 17)] {
            for p in 0..=255u8 {
                let a = filled(Procedural::SubCells { cols: 2, rows: 4, mask: p }, w, h);
                let b = filled(Procedural::SubCells { cols: 2, rows: 4, mask: !p }, w, h);
                assert!(a.iter().zip(&b).all(|(x, y)| (*x as u32 + *y as u32) == 255));
            }
        }
    }

    #[test]
    fn braille_ink_grows_with_dot_count() {
        let ink = |d: u8| filled(Procedural::Braille(d), 10, 20).iter().map(|&v| v as u32).sum::<u32>();
        assert_eq!(ink(0), 0);
        assert!(ink(0b1) < ink(0b11));
        assert!(ink(0b1111) < ink(0xFF));
        // Mirror symmetry: dot 1 (left column) vs dot 4 (right column).
        let left = filled(Procedural::Braille(0b0000_0001), 10, 20);
        let right = filled(Procedural::Braille(0b0000_1000), 10, 20);
        for y in 0..20 {
            for x in 0..10 {
                assert_eq!(left[y * 10 + x], right[y * 10 + (9 - x)]);
            }
        }
    }

    #[test]
    fn sets_deduplicate_and_classify() {
        let cp437 = SymbolSet::Cp437.chars();
        assert_eq!(cp437.len(), 255, "0x00 and 0x20 are both space");
        let bourke = SymbolSet::Bourke70.chars();
        assert_eq!(bourke.len(), 70);
        let custom = SymbolSet::Custom("aab\n█".into()).symbols();
        assert_eq!(custom.len(), 3);
        assert_eq!(custom[2].source, GlyphSource::Procedural(procedural_for('█').unwrap()));
        assert_eq!(custom[0].source, GlyphSource::Font);
    }

    #[test]
    fn cache_keys_distinguish_sets() {
        assert_ne!(SymbolSet::Bourke10.cache_key(), SymbolSet::Bourke70.cache_key());
        assert_eq!(
            SymbolSet::Custom(" .:-=+*#%@".into()).cache_key(),
            SymbolSet::Bourke10.cache_key()
        );
    }

    #[test]
    fn edges_round_half_up() {
        assert_eq!(edge(7, 1, 2), 4);
        assert_eq!(edge(14, 1, 3), 5);
        assert_eq!(edge(14, 2, 3), 9);
        assert_eq!(edge(8, 8, 8), 8);
    }
}

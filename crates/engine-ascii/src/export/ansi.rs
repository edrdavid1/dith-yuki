//! ANSI / ECMA-48 SGR exporter.

use crate::atlas::GlyphAtlas;
use crate::color::{resolve_rgb, Ansi16Palette, ColorTarget};
use crate::grid::{AsciiGrid, CellColor, GridColorMode};

/// Colour depth for SGR sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnsiDepth {
    /// SGR 30–37 / 90–97 (+ background 40–47 / 100–107).
    Ansi16(Ansi16Palette),
    /// `38;5;n` / `48;5;n`.
    Xterm256,
    /// `38;2;r;g;b` / `48;2;r;g;b`.
    TrueColor,
}

impl AnsiDepth {
    fn as_target(self) -> ColorTarget {
        match self {
            Self::Ansi16(p) => ColorTarget::Ansi16(p),
            Self::Xterm256 => ColorTarget::Xterm256,
            Self::TrueColor => ColorTarget::TrueColor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnsiOptions {
    pub depth: AnsiDepth,
    /// Emit `\x1b[0m` at end of each line.
    pub reset_at_eol: bool,
    pub mono_fg: CellColor,
    pub mono_bg: CellColor,
}

impl Default for AnsiOptions {
    fn default() -> Self {
        Self {
            depth: AnsiDepth::Ansi16(Ansi16Palette::Vga),
            reset_at_eol: true,
            mono_fg: CellColor::WHITE,
            mono_bg: CellColor::BLACK,
        }
    }
}

/// Emit an ANSI document (UTF-8 with SGR). Run-length: SGR only when style changes.
pub fn to_ansi(atlas: &GlyphAtlas, grid: &AsciiGrid, opts: AnsiOptions) -> String {
    let target = opts.depth.as_target();
    let mut out = String::new();
    let mut prev_fg: Option<[u8; 3]>;
    let mut prev_bg: Option<[u8; 3]>;
    let mut prev_idx_fg: Option<u8>;
    let mut prev_idx_bg: Option<u8>;

    for row in 0..grid.rows {
        prev_fg = None;
        prev_bg = None;
        prev_idx_fg = None;
        prev_idx_bg = None;
        for col in 0..grid.cols {
            let cell = &grid.cells[(row * grid.cols + col) as usize];
            let (fg_c, bg_c) = match grid.color {
                GridColorMode::Mono => (opts.mono_fg, opts.mono_bg),
                GridColorMode::Fg => (cell.fg, opts.mono_bg),
                GridColorMode::FgBg => (cell.fg, cell.bg),
            };

            match opts.depth {
                AnsiDepth::Ansi16(_) | AnsiDepth::Xterm256 => {
                    let fi = to_index(fg_c, target);
                    let bi = to_index(bg_c, target);
                    if prev_idx_fg != Some(fi) || prev_idx_bg != Some(bi) {
                        out.push_str(&sgr_indexed(opts.depth, fi, bi));
                        prev_idx_fg = Some(fi);
                        prev_idx_bg = Some(bi);
                    }
                }
                AnsiDepth::TrueColor => {
                    let fg = resolve_rgb(fg_c, target);
                    let bg = resolve_rgb(bg_c, target);
                    if prev_fg != Some(fg) || prev_bg != Some(bg) {
                        out.push_str(&format!(
                            "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                            fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]
                        ));
                        prev_fg = Some(fg);
                        prev_bg = Some(bg);
                    }
                }
            }
            out.push(atlas.glyphs[cell.glyph as usize].ch);
        }
        if opts.reset_at_eol {
            out.push_str("\x1b[0m");
        }
        out.push('\n');
    }
    out
}

fn to_index(c: CellColor, target: ColorTarget) -> u8 {
    match c {
        CellColor::Indexed(i) => i,
        CellColor::Rgb(rgb) => match crate::color::quantize_rgb(rgb, target) {
            CellColor::Indexed(i) => i,
            CellColor::Rgb(_) => 7,
        },
    }
}

fn sgr_indexed(depth: AnsiDepth, fg: u8, bg: u8) -> String {
    match depth {
        AnsiDepth::Ansi16(_) => {
            let (fg_code, bright_fg) = ansi16_fg(fg);
            let (bg_code, bright_bg) = ansi16_bg(bg);
            // Prefer a single SGR with both; bright flags via 90/100 families.
            let _ = (bright_fg, bright_bg);
            format!("\x1b[{fg_code};{bg_code}m")
        }
        AnsiDepth::Xterm256 => format!("\x1b[38;5;{fg};48;5;{bg}m"),
        AnsiDepth::TrueColor => unreachable!(),
    }
}

fn ansi16_fg(i: u8) -> (u8, bool) {
    let i = i & 15;
    if i < 8 {
        (30 + i, false)
    } else {
        (90 + (i - 8), true)
    }
}

fn ansi16_bg(i: u8) -> (u8, bool) {
    let i = i & 15;
    if i < 8 {
        (40 + i, false)
    } else {
        (100 + (i - 8), true)
    }
}

/// Parse a minimal subset of SGR for round-trip tests.
/// Returns `(fg_index, bg_index, text_lines)` for Ansi16/`38;5` docs.
pub fn parse_ansi_indices(s: &str) -> Vec<Vec<(u8, u8, char)>> {
    let mut rows = Vec::new();
    let mut fg = 7u8;
    let mut bg = 0u8;
    let mut row: Vec<(u8, u8, char)> = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            let start = i;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            let params = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            i += 1; // skip m
            apply_sgr(params, &mut fg, &mut bg);
            continue;
        }
        if bytes[i] == b'\n' {
            rows.push(std::mem::take(&mut row));
            fg = 7;
            bg = 0;
            i += 1;
            continue;
        }
        // Decode one UTF-8 char
        let ch = s[i..].chars().next().unwrap();
        i += ch.len_utf8();
        row.push((fg, bg, ch));
    }
    if !row.is_empty() {
        rows.push(row);
    }
    rows
}

fn apply_sgr(params: &str, fg: &mut u8, bg: &mut u8) {
    if params.is_empty() || params == "0" {
        *fg = 7;
        *bg = 0;
        return;
    }
    let parts: Vec<&str> = params.split(';').collect();
    let mut i = 0;
    while i < parts.len() {
        let p: u8 = parts[i].parse().unwrap_or(0);
        match p {
            0 => {
                *fg = 7;
                *bg = 0;
            }
            30..=37 => *fg = p - 30,
            40..=47 => *bg = p - 40,
            90..=97 => *fg = 8 + (p - 90),
            100..=107 => *bg = 8 + (p - 100),
            38 if i + 2 < parts.len() && parts[i + 1] == "5" => {
                *fg = parts[i + 2].parse().unwrap_or(0);
                i += 2;
            }
            48 if i + 2 < parts.len() && parts[i + 1] == "5" => {
                *bg = parts[i + 2].parse().unwrap_or(0);
                i += 2;
            }
            38 if i + 4 < parts.len() && parts[i + 1] == "2" => {
                // truecolor — encode as 0 for index parse tests
                *fg = 0;
                i += 4;
            }
            48 if i + 4 < parts.len() && parts[i + 1] == "2" => {
                *bg = 0;
                i += 4;
            }
            _ => {}
        }
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::color::{quantize_rgb, Ansi16Palette, ColorTarget};
    use crate::convert::{convert, ConvertOptions, MatchMode};
    use crate::font::{BundledFont, FontFace};
    use crate::grid::GridColorMode;
    use crate::symbols::SymbolSet;

    #[test]
    fn ansi16_parse_back_indices() {
        let atlas = GlyphAtlas::build(
            &FontFace::bundled(BundledFont::DepartureMono),
            &SymbolSet::Bourke10,
            &AtlasOptions {
                size: FontSize::Px(11.0),
                antialias: false,
                hinting: false,
            },
        )
        .unwrap();
        let (cw, ch) = (atlas.cell_w(), atlas.cell_h());
        // Red cell
        let mut rgba = vec![0.0f32; (cw * ch * 4) as usize];
        for i in 0..(cw * ch) as usize {
            rgba[i * 4] = 1.0;
            rgba[i * 4 + 3] = 1.0;
        }
        let grid = convert(
            &rgba,
            cw,
            ch,
            &atlas,
            ConvertOptions {
                match_mode: MatchMode::Tone,
                color_mode: GridColorMode::Fg,
                color_target: ColorTarget::Ansi16(Ansi16Palette::Vga),
                mono_bg: CellColor::Indexed(0),
                ..ConvertOptions::default()
            },
        );
        let ansi = to_ansi(
            &atlas,
            &grid,
            AnsiOptions {
                depth: AnsiDepth::Ansi16(Ansi16Palette::Vga),
                reset_at_eol: true,
                mono_fg: CellColor::Indexed(7),
                mono_bg: CellColor::Indexed(0),
            },
        );
        let parsed = parse_ansi_indices(&ansi);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].len(), 1);
        let (fg, bg, ch) = parsed[0][0];
        assert_eq!(ch, atlas.glyphs[grid.cells[0].glyph as usize].ch);
        assert_eq!(bg, 0);
        // Red → VGA index 1 or 9.
        assert!(fg == 1 || fg == 9, "fg={fg}");
        let expected = match grid.cells[0].fg {
            CellColor::Indexed(i) => i,
            CellColor::Rgb(rgb) => match quantize_rgb(rgb, ColorTarget::Ansi16(Ansi16Palette::Vga))
            {
                CellColor::Indexed(i) => i,
                _ => 0,
            },
        };
        assert_eq!(fg, expected);
    }
}

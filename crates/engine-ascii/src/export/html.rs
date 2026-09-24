//! Standalone HTML `<pre>` exporter.

use crate::atlas::GlyphAtlas;
use crate::color::{resolve_rgb, ColorTarget};
use crate::grid::{AsciiGrid, CellColor, GridColorMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HtmlOptions {
    pub target: ColorTarget,
    pub mono_fg: CellColor,
    pub mono_bg: CellColor,
    /// Include a minimal HTML document wrapper.
    pub standalone: bool,
}

impl Default for HtmlOptions {
    fn default() -> Self {
        Self {
            target: ColorTarget::TrueColor,
            mono_fg: CellColor::WHITE,
            mono_bg: CellColor::BLACK,
            standalone: true,
        }
    }
}

/// HTML with style-merged `<span>` runs inside `<pre>`.
pub fn to_html(atlas: &GlyphAtlas, grid: &AsciiGrid, opts: HtmlOptions) -> String {
    let mut body = String::from("<pre style=\"margin:0;line-height:1;letter-spacing:0\">");
    for row in 0..grid.rows {
        let mut run = String::new();
        let mut run_style: Option<(String, String)> = None;
        let flush = |body: &mut String, run: &mut String, style: &mut Option<(String, String)>| {
            if run.is_empty() {
                return;
            }
            if let Some((fg, bg)) = style.take() {
                body.push_str(&format!(
                    "<span style=\"color:{fg};background:{bg}\">{}</span>",
                    escape(run)
                ));
            } else {
                body.push_str(&escape(run));
            }
            run.clear();
        };
        for col in 0..grid.cols {
            let cell = &grid.cells[(row * grid.cols + col) as usize];
            let (fg_c, bg_c) = match grid.color {
                GridColorMode::Mono => (opts.mono_fg, opts.mono_bg),
                GridColorMode::Fg => (cell.fg, opts.mono_bg),
                GridColorMode::FgBg => (cell.fg, cell.bg),
            };
            let fg = css_color(resolve_rgb(fg_c, opts.target));
            let bg = css_color(resolve_rgb(bg_c, opts.target));
            let style = (fg, bg);
            if run_style.as_ref() != Some(&style) {
                flush(&mut body, &mut run, &mut run_style);
                run_style = Some(style);
            }
            run.push(atlas.glyphs[cell.glyph as usize].ch);
        }
        flush(&mut body, &mut run, &mut run_style);
        body.push('\n');
    }
    body.push_str("</pre>");

    if opts.standalone {
        format!(
            "<!DOCTYPE html>\n<html><head><meta charset=\"utf-8\"><title>ASCII</title></head>\n\
             <body style=\"background:#000;color:#fff;margin:0\">{}</body></html>\n",
            body
        )
    } else {
        body
    }
}

fn css_color(rgb: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atlas::{AtlasOptions, FontSize, GlyphAtlas};
    use crate::convert::{MatchMode, convert_mono};
    use crate::font::{BundledFont, FontFace};
    use crate::symbols::SymbolSet;

    #[test]
    fn html_is_well_formed_pre() {
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
        let rgba = vec![1.0f32; (cw * ch * 4) as usize];
        let grid = convert_mono(&rgba, cw, ch, &atlas, MatchMode::Tone);
        let html = to_html(&atlas, &grid, HtmlOptions::default());
        assert!(html.contains("<pre"));
        assert!(html.contains("</pre>"));
        assert!(html.contains("<!DOCTYPE html>"));
    }
}

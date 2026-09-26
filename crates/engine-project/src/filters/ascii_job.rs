//! ASCII full-document job: RGBA → [`engine_ascii::AsciiGrid`] → rendered RGBA.
//!
//! Runs as [`ExecutionScope::FullDocument`]. The preview raster is published as
//! Processed tiles (same path as Riemersma). A dedicated `CacheStage::Ascii` for
//! the Image|ASCII view switch is deferred to the frontend integration step.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use engine_ascii::{
    convert, render_rgba, to_ansi, to_html, to_json, to_png, to_svg, to_txt, Ansi16Palette,
    AnsiDepth, AnsiOptions, AsciiGrid, AtlasOptions, BundledFont, CellColor, CellDither,
    ColorTarget, ConvertOptions, EdgeOverlay, FontFace, FontSize, GlyphAtlas, GridColorMode,
    HtmlOptions, MatchMode, SvgOptions, SymbolSet, TXT_EXPORT_MAX_COLS,
};

use crate::error::EngineError;
use crate::filter::AsciiParams;

/// Result of one ASCII conversion (grid + atlas + doc-sized preview raster).
pub struct AsciiJobResult {
    pub grid: AsciiGrid,
    pub atlas: Arc<GlyphAtlas>,
    pub rgba_f32: Vec<f32>,
    pub width: u32,
    pub height: u32,
    pub color_target: ColorTarget,
}

/// Apply ASCII conversion in-place on a document-sized linear RGBA buffer.
pub fn apply_ascii_rgba(
    rgba: &mut [f32],
    width: u32,
    height: u32,
    params: &AsciiParams,
    should_cancel: &AtomicBool,
) -> Result<(), EngineError> {
    let result = run_ascii_job(rgba, width, height, params, should_cancel)?;
    rgba.copy_from_slice(&result.rgba_f32);
    Ok(())
}

/// Convert an RGBA buffer with ASCII settings; returns grid + preview raster.
pub fn run_ascii_job(
    rgba: &[f32],
    width: u32,
    height: u32,
    params: &AsciiParams,
    should_cancel: &AtomicBool,
) -> Result<AsciiJobResult, EngineError> {
    if should_cancel.load(Ordering::Relaxed) {
        return Err(EngineError::invalid_state("ascii cancelled"));
    }
    if rgba.len() != (width as usize) * (height as usize) * 4 {
        return Err(EngineError::invalid_filter_params("ascii invalid buffer"));
    }

    let font = match params.font.as_str() {
        "ibm_plex_mono" => FontFace::bundled(BundledFont::IbmPlexMono),
        "departure_mono" => FontFace::bundled(BundledFont::DepartureMono),
        other => {
            return Err(EngineError::invalid_filter_params(format!(
                "ascii unknown font {other}"
            )));
        }
    };
    let set = symbol_set_from_id(&params.symbol_set)?;
    let size = match params.size_mode.as_str() {
        "columns" => FontSize::Columns {
            cols: params.columns.max(1),
            doc_width: width.max(1),
        },
        _ => FontSize::Px(params.font_px),
    };
    let opts = AtlasOptions {
        size,
        antialias: params.antialias,
        hinting: params.hinting,
    };
    let atlas = Arc::new(
        GlyphAtlas::build(&font, &set, &opts)
            .map_err(|e| EngineError::invalid_filter_params(format!("ascii atlas: {e}")))?,
    );

    if should_cancel.load(Ordering::Relaxed) {
        return Err(EngineError::invalid_state("ascii cancelled"));
    }

    let color_target = color_target_from_params(params);
    let convert_opts = ConvertOptions {
        match_mode: match_mode_from_params(params),
        color_mode: color_mode_from_params(params),
        color_target,
        dither: dither_from_params(params),
        mono_fg: CellColor::BLACK,
        mono_bg: CellColor::WHITE,
        edge: EdgeOverlay {
            enabled: params.edge_overlay,
            tau: params.edge_tau,
        },
    };

    if should_cancel.load(Ordering::Relaxed) {
        return Err(EngineError::invalid_state("ascii cancelled"));
    }
    let grid = convert(rgba, width, height, &atlas, convert_opts);
    if should_cancel.load(Ordering::Relaxed) {
        return Err(EngineError::invalid_state("ascii cancelled"));
    }

    let (rw, rh, rendered) = render_rgba(
        &atlas,
        &grid,
        color_target,
        CellColor::BLACK,
        CellColor::WHITE,
    );

    // Blit rendered grid into a document-sized buffer (crop/pad).
    // Pad with fully transparent so we do not invent opaque paper outside the grid.
    let mut out = vec![0.0f32; (width as usize) * (height as usize) * 4];
    for y in 0..height {
        for x in 0..width {
            if x < rw && y < rh {
                let dst = ((y * width + x) * 4) as usize;
                let src = ((y * rw + x) * 4) as usize;
                out[dst] = rendered[src];
                out[dst + 1] = rendered[src + 1];
                out[dst + 2] = rendered[src + 2];
                out[dst + 3] = rendered[src + 3];
            }
        }
    }

    Ok(AsciiJobResult {
        grid,
        atlas,
        rgba_f32: out,
        width,
        height,
        color_target,
    })
}

/// Format codes for [`export_ascii_bytes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsciiExportFormat {
    Txt,
    Ansi,
    Html,
    Svg,
    Png,
    Json,
}

impl AsciiExportFormat {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "txt" | "text" => Some(Self::Txt),
            "ansi" | "ans" => Some(Self::Ansi),
            "html" | "htm" => Some(Self::Html),
            "svg" => Some(Self::Svg),
            "png" => Some(Self::Png),
            "json" => Some(Self::Json),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Ansi => "ans",
            Self::Html => "html",
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Json => "json",
        }
    }
}

/// Encode an [`AsciiJobResult`] to bytes for the chosen format.
pub fn export_ascii_bytes(
    result: &AsciiJobResult,
    format: AsciiExportFormat,
) -> Result<Vec<u8>, EngineError> {
    let atlas = result.atlas.as_ref();
    let grid = &result.grid;
    match format {
        AsciiExportFormat::Txt => Ok(to_txt(atlas, grid, false).into_bytes()),
        AsciiExportFormat::Ansi => {
            let depth = match result.color_target {
                ColorTarget::Ansi16(p) => AnsiDepth::Ansi16(p),
                ColorTarget::Xterm256 => AnsiDepth::Xterm256,
                ColorTarget::TrueColor => AnsiDepth::TrueColor,
            };
            Ok(to_ansi(
                atlas,
                grid,
                AnsiOptions {
                    depth,
                    reset_at_eol: true,
                    mono_fg: CellColor::BLACK,
                    mono_bg: CellColor::WHITE,
                },
            )
            .into_bytes())
        }
        AsciiExportFormat::Html => Ok(to_html(
            atlas,
            grid,
            HtmlOptions {
                target: result.color_target,
                mono_fg: CellColor::BLACK,
                mono_bg: CellColor::WHITE,
                standalone: true,
            },
        )
        .into_bytes()),
        AsciiExportFormat::Svg => Ok(to_svg(
            atlas,
            grid,
            SvgOptions {
                target: result.color_target,
                mono_fg: CellColor::BLACK,
                mono_bg: CellColor::WHITE,
            },
        )
        .into_bytes()),
        AsciiExportFormat::Png => to_png(
            atlas,
            grid,
            1,
            result.color_target,
            CellColor::BLACK,
            CellColor::WHITE,
        )
        .map_err(|e| EngineError::invalid_state(format!("ascii png: {e}"))),
        AsciiExportFormat::Json => to_json(atlas, grid)
            .map(|s| s.into_bytes())
            .map_err(|e| EngineError::invalid_state(format!("ascii json: {e}"))),
    }
}

/// Plain-text editors soft-wrap long lines, which stacks vertical strips of the
/// art. For TXT export we force a column-limited layout and an ASCII symbol set
/// so one image row stays one text line in typical viewers.
pub fn prepare_layer_for_txt_export(layer: &mut crate::layer::Layer) {
    use crate::filter::FilterParams;
    for filter in &mut layer.filters {
        let FilterParams::Ascii(ref mut p) = filter.params else {
            continue;
        };
        p.size_mode = "columns".into();
        p.columns = TXT_EXPORT_MAX_COLS;
        // Wide Unicode (blocks/braille/…) double-width in many editors → same strip bug.
        match p.symbol_set.as_str() {
            "bourke_10" | "bourke_70" | "printable_ascii" => {}
            _ => p.symbol_set = "bourke_70".into(),
        }
    }
}

fn symbol_set_from_id(id: &str) -> Result<SymbolSet, EngineError> {
    Ok(match id {
        "bourke_10" => SymbolSet::Bourke10,
        "bourke_70" => SymbolSet::Bourke70,
        "printable_ascii" => SymbolSet::PrintableAscii,
        "ascii_box_drawing" => SymbolSet::AsciiBoxDrawing,
        "blocks" => SymbolSet::Blocks,
        "quadrants" => SymbolSet::Quadrants,
        "sextants" => SymbolSet::Sextants,
        "octants" => SymbolSet::Octants,
        "braille" => SymbolSet::Braille,
        "cp437" => SymbolSet::Cp437,
        other => {
            return Err(EngineError::invalid_filter_params(format!(
                "ascii unknown symbol_set {other}"
            )));
        }
    })
}

fn match_mode_from_params(p: &AsciiParams) -> MatchMode {
    match p.match_mode.as_str() {
        "tone" => MatchMode::Tone,
        "shape_contrast" => MatchMode::ShapeContrast {
            gamma_8_8: (p.contrast.clamp(0.25, 4.0) * 256.0).round() as u16,
        },
        "mask_two_color" => MatchMode::MaskTwoColor,
        _ => MatchMode::Shape,
    }
}

fn color_mode_from_params(p: &AsciiParams) -> GridColorMode {
    match p.color_mode.as_str() {
        "fg" => GridColorMode::Fg,
        "fg_bg" => GridColorMode::FgBg,
        _ => GridColorMode::Mono,
    }
}

fn color_target_from_params(p: &AsciiParams) -> ColorTarget {
    match p.color_target.as_str() {
        "xterm256" => ColorTarget::Xterm256,
        "ansi16_xterm" => ColorTarget::Ansi16(Ansi16Palette::Xterm),
        "ansi16_win10" => ColorTarget::Ansi16(Ansi16Palette::Windows10),
        "ansi16" | "ansi16_vga" => ColorTarget::Ansi16(Ansi16Palette::Vga),
        _ => ColorTarget::TrueColor,
    }
}

fn dither_from_params(p: &AsciiParams) -> CellDither {
    match p.cell_dither.as_str() {
        "bayer2" => CellDither::Bayer2,
        "bayer4" => CellDither::Bayer4,
        "bayer8" => CellDither::Bayer8,
        "floyd_steinberg" => CellDither::FloydSteinberg {
            serpentine: p.serpentine,
        },
        _ => CellDither::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::AsciiParams;

    #[test]
    fn solid_white_becomes_spaces() {
        let w = 28u32;
        let h = 28u32;
        let mut rgba = vec![1.0f32; (w * h * 4) as usize];
        let params = AsciiParams {
            font: "departure_mono".into(),
            size_mode: "px".into(),
            font_px: 11.0,
            symbol_set: "bourke_10".into(),
            match_mode: "tone".into(),
            ..AsciiParams::default()
        };
        let cancel = AtomicBool::new(false);
        apply_ascii_rgba(&mut rgba, w, h, &params, &cancel).unwrap();
        let i = ((h / 2 * w + w / 2) * 4) as usize;
        assert!(rgba[i] > 0.9, "got {}", rgba[i]);
        assert!((rgba[i + 3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn transparent_pixels_stay_transparent() {
        let w = 28u32;
        let h = 28u32;
        let mut rgba = vec![0.0f32; (w * h * 4) as usize];
        let params = AsciiParams {
            font: "departure_mono".into(),
            size_mode: "px".into(),
            font_px: 11.0,
            symbol_set: "bourke_10".into(),
            match_mode: "tone".into(),
            ..AsciiParams::default()
        };
        let cancel = AtomicBool::new(false);
        apply_ascii_rgba(&mut rgba, w, h, &params, &cancel).unwrap();
        for i in 0..(w * h) as usize {
            assert_eq!(rgba[i * 4 + 3], 0.0, "pixel {i} alpha");
        }
    }

    #[test]
    fn cancel_before_work_errors() {
        let mut rgba = vec![0.0f32; 4];
        let cancel = AtomicBool::new(true);
        let err = apply_ascii_rgba(&mut rgba, 1, 1, &AsciiParams::default(), &cancel).unwrap_err();
        assert!(err.to_string().contains("cancel"));
    }

    #[test]
    fn export_txt_from_job() {
        let w = 14u32;
        let h = 14u32;
        let rgba = vec![1.0f32; (w * h * 4) as usize];
        let cancel = AtomicBool::new(false);
        let result = run_ascii_job(&rgba, w, h, &AsciiParams::default(), &cancel).unwrap();
        let bytes = export_ascii_bytes(&result, AsciiExportFormat::Txt).unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.contains('\n'));
    }
}

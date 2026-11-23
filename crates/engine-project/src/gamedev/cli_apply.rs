//! F7: thin CLI apply path over headless + GamedevContext.
//!
//! `dither-yuki apply` reads the same settings as the GUI so outputs match.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use engine_color::palette::Palette;
use engine_tiles::TileCache;
use thiserror::Error;

use crate::document::Document;
use crate::filter::FilterInstance;
use crate::headless::{
    expand_output_name, run_headless_batch, run_headless_job_cancellable, HeadlessError,
    HeadlessJob, HeadlessJobResult, HeadlessOutputFormat,
};
use crate::layer::{Layer, LayerNode};
use crate::serialize::open_project_from_bytes;
use crate::serialize::pattern::import_pattern_into_document;
use crate::types::{DocumentId, LayerId, LayerKind};

use super::collect_frame_paths;
use super::context::{CellSettings, GamedevContext, GamedevJob};
use super::should_lock_pattern_phase;

#[derive(Debug, Error)]
pub enum CliApplyError {
    #[error("{0}")]
    Msg(String),
    #[error(transparent)]
    Headless(#[from] HeadlessError),
}

impl From<String> for CliApplyError {
    fn from(s: String) -> Self {
        Self::Msg(s)
    }
}

#[derive(Debug, Clone)]
pub struct CliApplyOptions {
    /// Optional `.dyuki` pattern pack.
    pub pattern: Option<PathBuf>,
    /// Optional `.dyproj` for filters + gamedev context.
    pub project: Option<PathBuf>,
    /// Optional JSON file that deserializes to [`GamedevContext`].
    pub context_json: Option<PathBuf>,
    pub cell: Option<(u32, u32)>,
    pub tile: bool,
    pub png8: bool,
    pub lock_pattern_phase: bool,
    pub name_template: String,
}

impl Default for CliApplyOptions {
    fn default() -> Self {
        Self {
            pattern: None,
            project: None,
            context_json: None,
            cell: None,
            tile: false,
            png8: false,
            lock_pattern_phase: true,
            name_template: "{name}_dithered.png".into(),
        }
    }
}

/// Resolve filters, palettes, and effective gamedev context for a CLI apply.
pub fn resolve_cli_preset(
    opts: &CliApplyOptions,
) -> Result<(Vec<FilterInstance>, Vec<Palette>, GamedevContext), CliApplyError> {
    let mut filters = Vec::new();
    let mut palettes = Vec::new();
    let mut ctx = GamedevContext::default();

    if let Some(proj) = &opts.project {
        let bytes = std::fs::read(proj).map_err(|e| format!("read project: {e}"))?;
        let staging = TileCache::new(64 * 1024 * 1024);
        let opened = open_project_from_bytes(&bytes, &staging, DocumentId::new(1))
            .map_err(|e| format!("open project: {e}"))?;
        ctx = opened.document.gamedev.clone();
        palettes = opened.document.palettes.clone();
        filters = first_layer_filters(&opened.document.root);
    }

    if let Some(pat) = &opts.pattern {
        let bytes = std::fs::read(pat).map_err(|e| format!("read pattern: {e}"))?;
        let mut doc = Document::new(DocumentId::new(1), 8, 8);
        let layer_id = LayerId::new(1);
        doc.root.push(LayerNode::Leaf(Layer::new(
            layer_id,
            LayerKind::Raster,
            8,
            8,
        )));
        import_pattern_into_document(&bytes, &mut doc, layer_id, env!("CARGO_PKG_VERSION"))
            .map_err(|e| format!("import pattern: {e}"))?;
        filters = first_layer_filters(&doc.root);
        palettes = doc.palettes.clone();
    }

    if let Some(json_path) = &opts.context_json {
        let text = std::fs::read_to_string(json_path).map_err(|e| format!("read context: {e}"))?;
        let loaded: GamedevContext =
            serde_json::from_str(&text).map_err(|e| format!("parse context json: {e}"))?;
        ctx = loaded;
    }

    if opts.tile {
        ctx.tile.enabled = true;
        ctx.job = GamedevJob::Tile;
    }
    if let Some((w, h)) = opts.cell {
        ctx.cell = Some(CellSettings {
            w,
            h,
            padding: ctx.cell.as_ref().map(|c| c.padding).unwrap_or(0),
            independent: ctx.cell.as_ref().map(|c| c.independent).unwrap_or(true),
        });
        if matches!(ctx.job, GamedevJob::None) {
            ctx.job = GamedevJob::Sprite;
        }
    }
    if opts.png8 {
        ctx.export.png8 = true;
    }

    if filters.is_empty() {
        return Err(CliApplyError::Msg(
            "no filters: pass --pattern or --project with a dither stack".into(),
        ));
    }

    Ok((filters, palettes, ctx))
}

fn first_layer_filters(root: &[LayerNode]) -> Vec<FilterInstance> {
    let mut fallback = Vec::new();
    for node in root {
        match node {
            LayerNode::Leaf(layer) => {
                if !layer.filters.is_empty() {
                    return layer.filters.clone();
                }
                if fallback.is_empty() {
                    fallback = layer.filters.clone();
                }
            }
            LayerNode::Group(g) => {
                let nested = first_layer_filters(&g.children);
                if !nested.is_empty() {
                    return nested;
                }
            }
        }
    }
    fallback
}

fn collect_inputs(input: &Path) -> Result<Vec<PathBuf>, CliApplyError> {
    if input.is_file() {
        return Ok(vec![input.to_path_buf()]);
    }
    if input.is_dir() {
        return collect_frame_paths(input).map_err(|e| CliApplyError::Msg(e.to_string()));
    }
    Err(CliApplyError::Msg(format!(
        "input not found: {}",
        input.display()
    )))
}

/// Apply resolved preset to `input` (file or folder) → `output` (file or folder).
pub fn run_cli_apply(
    opts: &CliApplyOptions,
    input: &Path,
    output: &Path,
) -> Result<Vec<HeadlessJobResult>, CliApplyError> {
    let (filters, palettes, ctx) = resolve_cli_preset(opts)?;
    let format = if opts.png8 || ctx.export.png8 {
        HeadlessOutputFormat::Png8
    } else {
        HeadlessOutputFormat::Png
    };
    let lock = opts.lock_pattern_phase || should_lock_pattern_phase(&ctx);
    let template = if opts.name_template.is_empty() {
        ctx.export
            .naming
            .clone()
            .unwrap_or_else(|| "{name}_dithered.png".into())
    } else {
        opts.name_template.clone()
    };

    let inputs = collect_inputs(input)?;
    let cancel = AtomicBool::new(false);

    if inputs.len() == 1 && input.is_file() && !output_is_directory_hint(output) {
        if let Some(parent) = output.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| format!("create output parent: {e}"))?;
            }
        }
        let job = HeadlessJob {
            input: inputs[0].clone(),
            output: output.to_path_buf(),
            format,
            filters,
            palettes,
            lock_pattern_phase: lock,
            png8_palette_id: None,
        };
        run_headless_job_cancellable(&job, &cancel)?;
        return Ok(vec![HeadlessJobResult {
            input: job.input,
            output: Some(job.output),
            error: None,
        }]);
    }

    std::fs::create_dir_all(output).map_err(|e| format!("create output dir: {e}"))?;
    let jobs: Vec<HeadlessJob> = inputs
        .iter()
        .map(|inp| {
            let name = expand_output_name(&template, inp);
            HeadlessJob {
                input: inp.clone(),
                output: output.join(name),
                format,
                filters: filters.clone(),
                palettes: palettes.clone(),
                lock_pattern_phase: lock,
                png8_palette_id: None,
            }
        })
        .collect();

    Ok(run_headless_batch(&jobs, &cancel, |_, _, _| {}))
}

fn output_is_directory_hint(output: &Path) -> bool {
    output.is_dir()
        || output
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| ext.is_empty())
            .unwrap_or(true)
}

/// Parse `WxH` cell size.
pub fn parse_cell_wh(s: &str) -> Result<(u32, u32), CliApplyError> {
    let parts: Vec<_> = s.split(['x', 'X', '×']).collect();
    if parts.len() != 2 {
        return Err(CliApplyError::Msg(format!(
            "expected WxH cell size, got {s}"
        )));
    }
    let w: u32 = parts[0]
        .parse()
        .map_err(|_| CliApplyError::Msg(format!("bad cell width in {s}")))?;
    let h: u32 = parts[1]
        .parse()
        .map_err(|_| CliApplyError::Msg(format!("bad cell height in {s}")))?;
    if w == 0 || h == 0 {
        return Err(CliApplyError::Msg("cell size must be ≥ 1".into()));
    }
    Ok((w, h))
}

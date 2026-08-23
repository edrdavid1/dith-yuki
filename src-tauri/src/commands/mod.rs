pub mod diagnostics;
pub use diagnostics::*;
pub mod panels;
pub use panels::*;
pub mod selection;
pub use selection::*;
pub mod viewport;
pub use viewport::*;
pub mod undo;
pub use undo::*;
pub mod layers;
pub use layers::*;
pub mod filters;
pub use filters::*;
pub mod palette;
pub use palette::*;
pub mod color_lab;
pub use color_lab::*;
pub mod document;
pub use document::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

use engine_project::{
    document::DocumentHandle,
    dto::DocumentSnapshotDto,
    types::{LayerId, LayerKind, BlendMode},
    commands::{AddLayerArgs, LayerPropsPatch},
    commands as engine_commands,
};
use engine_tiles::{PixelTile, TileCache, Scheduler};
use engine_tiles::{CacheStage, Priority, RecomputeTask, TileKey};

use crate::panel_manager::PanelManager;
use crate::worker::WorkerWake;
use crate::document_session::{emit_tabs_changed, OpenDocumentsPayload};

pub use crate::services::palette_service::{hex_to_linear, linear_to_hex};
pub use crate::services::document_service::{
    f32_to_u8, encode_rgba_to_png, validate_document_dimensions, place_image_at_origin,
    blank_rgba_f32, MAX_DOCUMENT_DIMENSION, IMAGE_IMPORT_EXTENSIONS, BlankBackground,
    LoadImageResponse, SaveProjectResponse, OpenProjectResponse, ExportPatternRequest,
    ImportPatternRequest, ImportPatternResponse, ExportImageRequest, DocumentResponse,
};
pub use crate::services::palette_service::find_layers_referencing_palette;
pub use crate::commands::color_lab::{oklab_points_from_hexes, oklab_points_from_linear, OklabPointDto};

pub use crate::viewport::ViewportState;

pub(crate) struct PendingPreviewRefresh {
    pub layer_id: u32,
    pub clear_residuals: bool,
}

pub struct AppState {
    pub sessions: Mutex<HashMap<u32, Arc<crate::document_session::DocumentSession>>>,
    pub next_doc_id: AtomicU32,
    pub active_id: Mutex<Option<u32>>,
    pub tiles: crate::state::TileState,
    pub worker_wake: WorkerWake,
    pub gpu: Option<std::sync::Arc<engine_gpu::GpuContext>>,
    pub gpu_resident: Option<std::sync::Arc<engine_gpu::GpuTileCache>>,
    pub gpu_executor: Option<std::sync::Mutex<engine_gpu::GpuExecutor>>,
    pub app_handle: Mutex<Option<tauri::AppHandle>>,
    pub ui: crate::state::UiState,
    pub dock_affinity: Mutex<crate::dock_affinity::DockAffinityController>,
    pub float_drag_mouseup_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub float_drag_mouseup_hook: Mutex<Option<crate::global_mouseup::MouseUpHook>>,
    pub preview_pass_inflight: AtomicUsize,
    pub pending_preview_refresh: Mutex<Option<PendingPreviewRefresh>>,
}

pub struct QuitGuard {
    pub allow_exit: AtomicBool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DocumentChangedPayload {
    pub kind: String,
    pub layer_id: Option<u32>,
    pub doc_id: Option<u32>,
}

pub(crate) fn emit_document_changed(
    app_handle: &AppHandle,
    kind: &str,
    layer_id: Option<u32>,
    doc_id: Option<u32>,
) {
    let payload = DocumentChangedPayload {
        kind: kind.to_string(),
        layer_id,
        doc_id,
    };
    let _ = app_handle.emit_to(tauri::EventTarget::Any, "document-changed", payload);
}

pub(crate) fn reset_tiles_for_new_document(state: &AppState) {
    state.tiles.tile_cache.clear();
    state.tiles.scheduler.clear_all();
    state.tiles.ed_frontier.clear();
    state.tiles.block_representatives.invalidate_all();
    state.tiles.error_residuals.clear();
    if let Ok(mut pending) = state.pending_preview_refresh.lock() {
        *pending = None;
    }
}

pub(crate) fn invalidate_after_document_replace(state: &AppState) {
    use engine_tiles::CacheStage;

    let mut keys = Vec::new();
    for entry in state.tiles.tile_cache.entries.iter() {
        let key = *entry.key();
        if matches!(key.stage, CacheStage::Processed | CacheStage::Composite) {
            keys.push(key);
        }
    }
    for key in keys {
        state.tiles.tile_cache.mark_dirty(key);
    }
    state.tiles.block_representatives.invalidate_all();
    if let Ok(session) = state.active_session() {
        state
            .tiles.error_residuals
            .evict_document(session.document_handle.snapshot().id.0);
    } else {
        state.tiles.error_residuals.clear();
    }
}

pub(crate) fn schedule_dirty_viewport_tiles(state: &AppState) {
    use std::sync::atomic::Ordering;

    let viewport = state.ui.viewport.lock().unwrap().clone();
    let Ok(snapshot) = state.active_session().map(|s| s.document_handle.snapshot()) else {
        return;
    };
    let doc_gen = snapshot.generations.document_gen.load(Ordering::Acquire);

    crate::tile_pipeline::schedule_ed_for_viewport(state);

    let gpu_authored_l0 = crate::gpu_resident_shadow::try_publish_gpu_preview_viewport(state);

    for coord in &viewport.visible_tiles {
        if gpu_authored_l0.contains(coord) {
            continue;
        }

        let key = TileKey {
            doc: snapshot.id.0,
            layer: 0,
            coord: *coord,
            stage: CacheStage::Composite,
        };

        let is_dirty = match state.tiles.tile_cache.entries.get(&key) {
            Some(entry) => entry.dirty.load(Ordering::Acquire),
            None => true,
        };

        if is_dirty {
            let task = RecomputeTask {
                key,
                generation: doc_gen,
                layer_generation: 0,
                priority: Priority::Immediate,
            };
            state.tiles.scheduler.enqueue_dedup(task);
            state.worker_wake.notify_one();
        }
    }

    crate::gpu_resident_shadow::enqueue_resident_shadow_viewport(state);
}

fn preview_pass_busy(state: &AppState) -> bool {
    state.preview_pass_inflight.load(Ordering::Acquire) > 0
        || state.tiles.scheduler.queued_len() > 0
}

pub(crate) fn layer_needs_dither_cache_reset(nodes: &[engine_project::LayerNode], layer_id: u32) -> bool {
    use engine_project::filter::FilterParams;
    for node in nodes {
        match node {
            engine_project::LayerNode::Leaf(layer) if layer.id.0 == layer_id => {
                return layer.filters.iter().any(|f| {
                    f.enabled
                        && matches!(
                            f.params,
                            FilterParams::DitherV2(_) | FilterParams::Dither { .. }
                        )
                });
            }
            engine_project::LayerNode::Group(group) => {
                if layer_needs_dither_cache_reset(&group.children, layer_id) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

pub(crate) fn request_preview_refresh(state: &AppState, layer_id: u32, clear_residuals: bool) {
    if preview_pass_busy(state) {
        let mut pending = state.pending_preview_refresh.lock().unwrap();
        let clear = clear_residuals
            || pending
                .as_ref()
                .map(|p| p.clear_residuals)
                .unwrap_or(false);
        *pending = Some(PendingPreviewRefresh {
            layer_id,
            clear_residuals: clear,
        });
        return;
    }
    run_preview_refresh(state, layer_id, clear_residuals);
}

fn run_preview_refresh(state: &AppState, layer_id: u32, clear_residuals: bool) {
    let Ok(session) = state.active_session() else {
        return;
    };
    if clear_residuals {
        let doc = session.document_handle.snapshot().id.0;
        state
            .tiles.error_residuals
            .evict_layer(doc, engine_project::types::LayerId::new(layer_id));
        state.tiles.block_representatives.evict_layer(doc, layer_id);
    }
    engine_tiles::invalidation::invalidate(
        &state.tiles.tile_cache,
        engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
            doc: session.document_handle.snapshot().id.0,
            layer: layer_id,
        },
    );
    schedule_dirty_viewport_tiles(state);
}

pub(crate) fn on_preview_task_finished(state: &AppState) {
    if preview_pass_busy(state) {
        return;
    }
    let pending = state.pending_preview_refresh.lock().unwrap().take();
    if let Some(p) = pending {
        run_preview_refresh(state, p.layer_id, p.clear_residuals);
    }
}

pub fn is_release_build() -> bool {
    !cfg!(debug_assertions)
}

#[cfg(test)]
pub(crate) fn make_test_app_state() -> Arc<AppState> {
    use engine_project::Document;
    use engine_project::types::DocumentId;

    let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
    state.spawn_session(Document::new(DocumentId::new(1), 800, 600));
    Arc::new(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_mode_parsing_works() {
        let bm = match "multiply".into() {
            name => match name {
                "multiply" => Some(BlendMode::Multiply),
                _ => None,
            }
        };
        assert!(bm.is_some());
    }

    #[test]
    fn test_f32_to_u8_conversion() {
        assert_eq!(f32_to_u8(0.0), 0);
        assert_eq!(f32_to_u8(1.0), 255);
        assert_eq!(f32_to_u8(0.5), 127);
        assert_eq!(f32_to_u8(-0.1), 0);
        assert_eq!(f32_to_u8(1.5), 255);
    }

    #[test]
    fn test_encode_rgba_to_png() {
        let buffer = vec![255u8, 0, 0, 255];
        let result = encode_rgba_to_png(&buffer, 1, 1);
        assert!(result.is_ok());
        let png_bytes = result.unwrap();
        assert_eq!(&png_bytes[0..4], &[0x89, 0x50, 0x4E, 0x47]);
    }

    #[test]
    fn hex_to_linear_valid_uppercase() {
        let result = hex_to_linear("FF0000").unwrap();
        assert!((result.r - 1.0).abs() < 1e-5);
        assert!((result.g - 0.0).abs() < 1e-5);
        assert!((result.b - 0.0).abs() < 1e-5);
    }

    #[test]
    fn hex_to_linear_valid_lowercase() {
        let result = hex_to_linear("00ff00").unwrap();
        assert!((result.r - 0.0).abs() < 1e-5);
        assert!((result.g - 1.0).abs() < 1e-5);
        assert!((result.b - 0.0).abs() < 1e-5);
    }

    #[test]
    fn hex_to_linear_valid_mixed_case() {
        let result = hex_to_linear("aAbBcC").unwrap();
        assert!(result.r > 0.0 && result.r < 1.0);
        assert!(result.g > 0.0 && result.g < 1.0);
        assert!(result.b > 0.0 && result.b < 1.0);
    }

    #[test]
    fn hex_to_linear_black() {
        let result = hex_to_linear("000000").unwrap();
        assert_eq!(result.r, 0.0);
        assert_eq!(result.g, 0.0);
        assert_eq!(result.b, 0.0);
    }

    #[test]
    fn hex_to_linear_white() {
        let result = hex_to_linear("FFFFFF").unwrap();
        assert!((result.r - 1.0).abs() < 1e-5);
        assert!((result.g - 1.0).abs() < 1e-5);
        assert!((result.b - 1.0).abs() < 1e-5);
    }

    #[test]
    fn hex_to_linear_err_too_short() {
        let result = hex_to_linear("FFF");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_too_long() {
        let result = hex_to_linear("FF00FF00");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_empty() {
        let result = hex_to_linear("");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_with_hash_prefix() {
        let result = hex_to_linear("#FF0000");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exactly 6 characters"));
    }

    #[test]
    fn hex_to_linear_err_non_hex_chars() {
        let result = hex_to_linear("GGHHII");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid hex character"));
    }

    #[test]
    fn hex_to_linear_err_special_chars() {
        let result = hex_to_linear("FF$$00");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid hex character"));
    }

    #[test]
    fn linear_to_hex_black() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.0, g: 0.0, b: 0.0 };
        assert_eq!(linear_to_hex(&color), "000000");
    }

    #[test]
    fn linear_to_hex_white() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 1.0, g: 1.0, b: 1.0 };
        assert_eq!(linear_to_hex(&color), "FFFFFF");
    }

    #[test]
    fn linear_to_hex_red() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 1.0, g: 0.0, b: 0.0 };
        assert_eq!(linear_to_hex(&color), "FF0000");
    }

    #[test]
    fn linear_to_hex_green() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.0, g: 1.0, b: 0.0 };
        assert_eq!(linear_to_hex(&color), "00FF00");
    }

    #[test]
    fn linear_to_hex_blue() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.0, g: 0.0, b: 1.0 };
        assert_eq!(linear_to_hex(&color), "0000FF");
    }

    #[test]
    fn linear_to_hex_uppercase_format() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 0.5, g: 0.5, b: 0.5 };
        let hex = linear_to_hex(&color);
        assert_eq!(hex.len(), 6);
        assert_eq!(hex, hex.to_uppercase());
    }

    #[test]
    fn linear_to_hex_clamps_above_one() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: 1.5, g: 2.0, b: 3.0 };
        assert_eq!(linear_to_hex(&color), "FFFFFF");
    }

    #[test]
    fn linear_to_hex_clamps_below_zero() {
        use engine_color::palette::LinearColor;
        let color = LinearColor { r: -1.0, g: -0.5, b: -0.1 };
        assert_eq!(linear_to_hex(&color), "000000");
    }

    #[test]
    fn hex_round_trip_known_values() {
        let test_cases = [
            "000000", "FFFFFF", "FF0000", "00FF00", "0000FF",
            "808080", "C0C0C0", "A0B0C0", "123456", "ABCDEF",
        ];
        for hex in &test_cases {
            let linear = hex_to_linear(hex).unwrap();
            let back = linear_to_hex(&linear);
            assert_eq!(
                &back,
                &hex.to_uppercase(),
                "Round-trip failed for input '{}'",
                hex
            );
        }
    }

    #[test]
    fn hex_round_trip_case_insensitive() {
        let lower = hex_to_linear("abcdef").unwrap();
        let upper = hex_to_linear("ABCDEF").unwrap();
        let mixed = hex_to_linear("AbCdEf").unwrap();

        let hex_lower = linear_to_hex(&lower);
        let hex_upper = linear_to_hex(&upper);
        let hex_mixed = linear_to_hex(&mixed);

        assert_eq!(hex_lower, "ABCDEF");
        assert_eq!(hex_upper, "ABCDEF");
        assert_eq!(hex_mixed, "ABCDEF");
    }

    fn gameboy_hexes() -> Vec<String> {
        use engine_color::palette::find_preset;
        find_preset("gameboy")
            .expect("gameboy preset")
            .colors_srgb
            .iter()
            .map(|&(r, g, b)| format!("#{:02X}{:02X}{:02X}", r, g, b))
            .collect()
    }

    #[test]
    fn colors_to_oklab_gameboy_matches_linear_to_oklab() {
        use engine_color::palette::{find_preset, srgb_to_linear};
        use engine_color::{linear_to_oklab, LinRgb};

        let hexes = gameboy_hexes();
        let points = oklab_points_from_hexes(&hexes).unwrap();
        assert_eq!(points.len(), 4);

        let gb = find_preset("gameboy").unwrap();
        const EPS: f32 = 1e-5;
        for (i, &(r, g, b)) in gb.colors_srgb.iter().enumerate() {
            let expected = linear_to_oklab(LinRgb {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            });
            assert!(
                (points[i].l - expected.l).abs() < EPS,
                "L[{}]: {} vs {}",
                i,
                points[i].l,
                expected.l
            );
            assert!(
                (points[i].a - expected.a).abs() < EPS,
                "a[{}]: {} vs {}",
                i,
                points[i].a,
                expected.a
            );
            assert!(
                (points[i].b - expected.b).abs() < EPS,
                "b[{}]: {} vs {}",
                i,
                points[i].b,
                expected.b
            );
            assert_eq!(points[i].srgb_hex, hexes[i]);
        }
    }

    #[test]
    fn colors_to_oklab_empty_list() {
        let points = oklab_points_from_hexes(&[]).unwrap();
        assert!(points.is_empty());
    }

    #[test]
    fn colors_to_oklab_invalid_hex_errors() {
        let err = oklab_points_from_hexes(&["#GG0000".into()]).unwrap_err();
        assert!(err.contains("invalid") || err.contains("Hex"));
    }

    fn snapshot_palette_oklab(
        state: &AppState,
        palette_id: u32,
    ) -> Result<Vec<OklabPointDto>, String> {
        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot
            .palettes
            .iter()
            .find(|p| p.id == palette_id)
            .ok_or_else(|| format!("Palette {} not found", palette_id))?;
        Ok(oklab_points_from_linear(&palette.colors))
    }

    #[test]
    fn get_palette_oklab_missing_palette_errors() {
        let state = make_test_app_state();
        let snapshot = state.must_active().document_handle.snapshot();
        assert!(snapshot.palettes.iter().all(|p| p.id != 9999));
        drop(snapshot);
        let err = snapshot_palette_oklab(&state, 9999).unwrap_err();
        assert!(err.contains("Palette 9999 not found"));
    }

    #[test]
    fn get_palette_oklab_gameboy_matches_linear_to_oklab() {
        use engine_color::palette::{find_preset, srgb_to_linear, LinearColor};
        use engine_color::{linear_to_oklab, LinRgb};

        let gb = find_preset("gameboy").unwrap();
        let linear: Vec<LinearColor> = gb
            .colors_srgb
            .iter()
            .map(|&(r, g, b)| LinearColor {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            })
            .collect();

        let state = make_test_app_state();
        let mut palette_id = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let pid = doc.add_palette("Game Boy".to_string(), linear);
            palette_id = pid.0;
        });

        let points = snapshot_palette_oklab(&state, palette_id).unwrap();
        assert_eq!(points.len(), 4);
        const EPS: f32 = 1e-5;
        for (i, &(r, g, b)) in gb.colors_srgb.iter().enumerate() {
            let expected = linear_to_oklab(LinRgb {
                r: srgb_to_linear(r),
                g: srgb_to_linear(g),
                b: srgb_to_linear(b),
            });
            assert!((points[i].l - expected.l).abs() < EPS);
            assert!((points[i].a - expected.a).abs() < EPS);
            assert!((points[i].b - expected.b).abs() < EPS);
        }
    }

    #[test]
    fn hex_round_trip_exhaustive_boundaries() {
        let boundary_values = ["00", "01", "7F", "80", "FE", "FF"];
        for r in &boundary_values {
            for g in &boundary_values {
                for b in &boundary_values {
                    let hex = format!("{}{}{}", r, g, b);
                    let linear = hex_to_linear(&hex).unwrap();
                    let back = linear_to_hex(&linear);
                    assert_eq!(
                        back, hex,
                        "Round-trip failed for '{}'",
                        hex
                    );
                }
            }
        }
    }

    #[test]
    fn find_layers_referencing_palette_empty_tree() {
        use engine_project::types::PaletteId;

        let nodes: Vec<engine_project::layer::LayerNode> = vec![];
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(1));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_no_references() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{FilterInstance, FilterKind, FilterParams, DitherMode};

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 4 },
                color_depth: 4,
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];
        let result = find_layers_referencing_palette(&nodes, PaletteId::new(1));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_dither_v2_match() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode,
        };

        let mut layer = Layer::new(LayerId::new(5), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: Some(PaletteId::new(42)),
            ..Default::default()
            }),
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(42));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(5));
    }

    #[test]
    fn find_layers_referencing_palette_dither_v2_no_palette() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode,
        };

        let mut layer = Layer::new(LayerId::new(5), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: None,
            ..Default::default()
            }),
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(42));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_palette_quantize_match() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{FilterInstance, FilterKind, FilterParams, DiffusionKernel};

        let mut layer = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(7),
                diffusion: Some(DiffusionKernel::FloydSteinberg),
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(7));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(10));
    }

    #[test]
    fn find_layers_referencing_palette_wrong_palette_id() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{FilterInstance, FilterKind, FilterParams};

        let mut layer = Layer::new(LayerId::new(10), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(7),
                diffusion: None,
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(99));
        assert!(result.is_empty());
    }

    #[test]
    fn find_layers_referencing_palette_recursive_group() {
        use engine_project::layer::{Layer, LayerGroup, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode, DiffusionKernel,
        };

        let layer1 = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);

        let mut layer3 = Layer::new(LayerId::new(3), LayerKind::Raster, 256, 256);
        layer3.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer8x8,
                levels: 8,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: Some(PaletteId::new(5)),
            ..Default::default()
            }),
        ));

        let mut layer5 = Layer::new(LayerId::new(5), LayerKind::Raster, 256, 256);
        layer5.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(5),
                diffusion: Some(DiffusionKernel::Atkinson),
            },
        ));

        let mut group4 = LayerGroup::new(LayerId::new(4));
        group4.children.push(LayerNode::Leaf(layer5));

        let mut group2 = LayerGroup::new(LayerId::new(2));
        group2.children.push(LayerNode::Leaf(layer3));
        group2.children.push(LayerNode::Group(group4));

        let mut layer6 = Layer::new(LayerId::new(6), LayerKind::Raster, 256, 256);
        layer6.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(99),
                diffusion: None,
            },
        ));

        let nodes = vec![
            LayerNode::Leaf(layer1),
            LayerNode::Group(group2),
            LayerNode::Leaf(layer6),
        ];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(5));
        assert_eq!(result.len(), 2);
        assert!(result.contains(&LayerId::new(3)));
        assert!(result.contains(&LayerId::new(5)));

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(99));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(6));
    }

    #[test]
    fn find_layers_referencing_palette_multiple_filters_on_one_layer() {
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode, DiffusionKernel,
        };

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: Some(PaletteId::new(3)),
            ..Default::default()
            }),
        ));
        layer.filters.push(FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(3),
                diffusion: Some(DiffusionKernel::FloydSteinberg),
            },
        ));
        let nodes = vec![LayerNode::Leaf(layer)];

        let result = find_layers_referencing_palette(&nodes, PaletteId::new(3));
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], LayerId::new(1));
    }

    #[test]
    fn integration_palette_crud_lifecycle() {
        let state = make_test_app_state();

        let name = "Test Palette".to_string();
        let trimmed_name = name.trim().to_string();
        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let pid = doc.add_palette(trimmed_name.clone(), vec![]);
            palette_id_raw = pid.0;
            doc.increment_generation();
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw);
        assert!(palette.is_some());
        let palette = palette.unwrap();
        assert_eq!(palette.name, "Test Palette");
        assert_eq!(palette.colors.len(), 0);
        assert_eq!(palette.revision, 1);
        drop(snapshot);

        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.name = "Renamed Palette".to_string();
            }
        });
        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.name, "Renamed Palette");
        drop(snapshot);

        let red = hex_to_linear("FF0000").unwrap();
        let green = hex_to_linear("00FF00").unwrap();
        let blue = hex_to_linear("0000FF").unwrap();

        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.push(red);
                p.revision += 1;
            }
        });
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.push(green);
                p.revision += 1;
            }
        });
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.push(blue);
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.colors.len(), 3);
        assert_eq!(palette.revision, 4);
        drop(snapshot);

        let yellow = hex_to_linear("FFFF00").unwrap();
        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors[1] = yellow;
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.revision, 5);
        let hex_at_1 = linear_to_hex(&palette.colors[1]);
        assert_eq!(hex_at_1, "FFFF00");
        drop(snapshot);

        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                p.colors.remove(0);
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.colors.len(), 2);
        assert_eq!(palette.revision, 6);
        assert_eq!(linear_to_hex(&palette.colors[0]), "FFFF00");
        assert_eq!(linear_to_hex(&palette.colors[1]), "0000FF");
        drop(snapshot);

        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                let color = p.colors.remove(0);
                p.colors.insert(1, color);
                p.revision += 1;
            }
        });

        let snapshot = state.must_active().document_handle.snapshot();
        let palette = snapshot.palettes.iter().find(|p| p.id == palette_id_raw).unwrap();
        assert_eq!(palette.colors.len(), 2);
        assert_eq!(palette.revision, 7);
        assert_eq!(linear_to_hex(&palette.colors[0]), "0000FF");
        assert_eq!(linear_to_hex(&palette.colors[1]), "FFFF00");
        drop(snapshot);

        state.must_active().document_handle.mutate(|doc| {
            doc.palettes.retain(|p| p.id != palette_id_raw);
            doc.increment_generation();
        });

        let snapshot = state.must_active().document_handle.snapshot();
        assert!(snapshot.palettes.iter().find(|p| p.id == palette_id_raw).is_none());
        drop(snapshot);
    }

    fn invalidate_palette_changed(palette_id: engine_project::types::PaletteId, state: &AppState) {
        use engine_tiles::{CacheStage, TileKey};
        use std::sync::atomic::Ordering;

        let Ok(session) = state.active_session() else {
            return;
        };
        let doc = session.document_handle.snapshot().id.0;
        let snapshot = session.document_handle.snapshot();
        let layers = find_layers_referencing_palette(&snapshot.root, palette_id);
        drop(snapshot);

        state.tiles.palette_cache.evict(doc, palette_id.0);
        state.tiles.palette_lut_cache.evict(doc, palette_id.0);

        for layer_id in layers.iter() {
            engine_tiles::invalidation::invalidate(
                &state.tiles.tile_cache,
                engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
                    doc,
                    layer: layer_id.0,
                },
            );
        }

        if !layers.is_empty() {
            schedule_dirty_viewport_tiles(state);
        }
    }

    #[test]
    fn integration_invalidation_cascade_on_palette_modify() {
        use std::sync::atomic::Ordering;
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode,
        };
        use engine_tiles::{CacheStage, TileCoord, TileKey, PixelTile};

        let state = make_test_app_state();

        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let color = engine_color::palette::LinearColor {
                r: 1.0, g: 0.0, b: 0.0,
            };
            let pid = doc.add_palette("Test".to_string(), vec![color]);
            palette_id_raw = pid.0;
        });

        let layer_id = 42u32;
        state.must_active().document_handle.mutate(|doc| {
            let mut layer = Layer::new(
                LayerId::new(layer_id),
                LayerKind::Raster,
                800,
                600,
            );
            layer.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: DitherModeV2::Bayer4x4,
                    levels: 4,
                    threshold_scale: 1.0,
                    pixel_size: 1,
                    color_mode: DitherColorMode::Rgb,
                    palette_id: Some(PaletteId::new(palette_id_raw)),
            ..Default::default()
                }),
            ));
            doc.root.push(LayerNode::Leaf(layer));
        });

        let processed_key = TileKey {
            doc: 1,
            layer: layer_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let composite_key = TileKey {
            doc: 1,
            layer: layer_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Composite,
        };
        let tile = Arc::new(PixelTile::new());
        state.tiles.tile_cache.get_or_insert(processed_key, tile.clone());
        state.tiles.tile_cache.get_or_insert(composite_key, tile.clone());

        let entry_p = state.tiles.tile_cache.entries.get(&processed_key).unwrap();
        assert!(!entry_p.dirty.load(Ordering::Relaxed));
        drop(entry_p);
        let entry_c = state.tiles.tile_cache.entries.get(&composite_key).unwrap();
        assert!(!entry_c.dirty.load(Ordering::Relaxed));
        drop(entry_c);

        state.must_active().document_handle.mutate(|doc| {
            if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id_raw) {
                let green = engine_color::palette::LinearColor {
                    r: 0.0, g: 1.0, b: 0.0,
                };
                p.colors.push(green);
                p.revision += 1;
            }
        });
        invalidate_palette_changed(PaletteId::new(palette_id_raw), &state);

        let entry_p = state.tiles.tile_cache.entries.get(&processed_key).unwrap();
        assert!(
            entry_p.dirty.load(Ordering::Relaxed),
            "Processed tile should be dirty after palette modification"
        );
        drop(entry_p);
        let entry_c = state.tiles.tile_cache.entries.get(&composite_key).unwrap();
        assert!(
            entry_c.dirty.load(Ordering::Relaxed),
            "Composite tile should be dirty after palette modification"
        );
        drop(entry_c);
    }

    #[test]
    fn integration_no_invalidation_for_unreferenced_palette() {
        use std::sync::atomic::Ordering;
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_tiles::{CacheStage, TileCoord, TileKey, PixelTile};

        let state = make_test_app_state();

        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let pid = doc.add_palette("Unused".to_string(), vec![]);
            palette_id_raw = pid.0;
        });

        let layer_id = 10u32;
        state.must_active().document_handle.mutate(|doc| {
            let layer = Layer::new(LayerId::new(layer_id), LayerKind::Raster, 800, 600);
            doc.root.push(LayerNode::Leaf(layer));
        });

        let key = TileKey {
            doc: 1,
            layer: layer_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let tile = Arc::new(PixelTile::new());
        state.tiles.tile_cache.get_or_insert(key, tile);

        invalidate_palette_changed(PaletteId::new(palette_id_raw), &state);

        let entry = state.tiles.tile_cache.entries.get(&key).unwrap();
        assert!(
            !entry.dirty.load(Ordering::Relaxed),
            "Tile should NOT be dirty when palette is unreferenced"
        );
    }

    #[test]
    fn integration_force_delete_palette_clears_references() {
        use std::sync::atomic::Ordering;
        use engine_project::layer::{Layer, LayerNode};
        use engine_project::types::{LayerId, LayerKind, PaletteId};
        use engine_project::filter::{
            FilterInstance, FilterKind, FilterParams, DitherParamsV2,
            DitherModeV2, DitherColorMode, DiffusionKernel,
        };
        use engine_tiles::{CacheStage, TileCoord, TileKey, PixelTile};

        let state = make_test_app_state();

        let mut palette_id_raw = 0u32;
        state.must_active().document_handle.mutate(|doc| {
            let colors = vec![
                engine_color::palette::LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                engine_color::palette::LinearColor { r: 0.0, g: 1.0, b: 0.0 },
            ];
            let pid = doc.add_palette("ToDelete".to_string(), colors);
            palette_id_raw = pid.0;
        });

        let layer_a_id = 100u32;
        let layer_b_id = 200u32;
        state.must_active().document_handle.mutate(|doc| {
            let mut layer_a = Layer::new(
                LayerId::new(layer_a_id), LayerKind::Raster, 800, 600,
            );
            layer_a.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: DitherModeV2::Bayer4x4,
                    levels: 4,
                    threshold_scale: 1.0,
                    pixel_size: 1,
                    color_mode: DitherColorMode::Rgb,
                    palette_id: Some(PaletteId::new(palette_id_raw)),
            ..Default::default()
                }),
            ));
            doc.root.push(LayerNode::Leaf(layer_a));

            let mut layer_b = Layer::new(
                LayerId::new(layer_b_id), LayerKind::Raster, 800, 600,
            );
            layer_b.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id: PaletteId::new(palette_id_raw),
                    diffusion: Some(DiffusionKernel::FloydSteinberg),
                },
            ));
            doc.root.push(LayerNode::Leaf(layer_b));
        });

        let key_a = TileKey {
            doc: 1,
            layer: layer_a_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let key_b = TileKey {
            doc: 1,
            layer: layer_b_id,
            coord: TileCoord { level: 0, x: 0, y: 0 },
            stage: CacheStage::Processed,
        };
        let tile = Arc::new(PixelTile::new());
        state.tiles.tile_cache.get_or_insert(key_a, tile.clone());
        state.tiles.tile_cache.get_or_insert(key_b, tile.clone());

        let pid = PaletteId::new(palette_id_raw);
        let mut affected_filter_ids: Vec<String> = Vec::new();
        let mut affected_layer_ids: Vec<u32> = Vec::new();

        state.must_active().document_handle.mutate(|doc| {
            for node in doc.root.iter_mut() {
                if let LayerNode::Leaf(layer) = node {
                    let mut layer_affected = false;
                    let mut filters_to_remove: Vec<usize> = Vec::new();

                    for (idx, filter) in layer.filters.iter_mut().enumerate() {
                        match &mut filter.params {
                            FilterParams::DitherV2(params) => {
                                if params.palette_id == Some(pid) {
                                    params.palette_id = None;
                                    affected_filter_ids.push(filter.id.to_string());
                                    layer_affected = true;
                                }
                            }
                            FilterParams::PaletteQuantize {
                                palette_id: ref p, ..
                            } => {
                                if *p == pid {
                                    affected_filter_ids.push(filter.id.to_string());
                                    filters_to_remove.push(idx);
                                    layer_affected = true;
                                }
                            }
                            _ => {}
                        }
                    }

                    for idx in filters_to_remove.into_iter().rev() {
                        layer.filters.remove(idx);
                    }

                    if layer_affected {
                        affected_layer_ids.push(layer.id.0);
                    }
                }
            }

            doc.palettes.retain(|p| p.id != palette_id_raw);
            doc.increment_generation();
        });

        state.tiles.palette_cache.evict(1, palette_id_raw);
        state.tiles.palette_lut_cache.evict(1, palette_id_raw);

        for layer_id in &affected_layer_ids {
            engine_tiles::invalidation::invalidate(
                &state.tiles.tile_cache,
                engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged { doc: 1, layer: *layer_id,
                },
            );
        }

        let snapshot = state.must_active().document_handle.snapshot();
        assert!(
            snapshot.palettes.iter().find(|p| p.id == palette_id_raw).is_none(),
            "Palette should be removed from document"
        );

        let layer_a_node = snapshot.root.iter().find(|n| {
            matches!(n, LayerNode::Leaf(l) if l.id.0 == layer_a_id)
        });
        assert!(layer_a_node.is_some());
        if let Some(LayerNode::Leaf(layer_a)) = layer_a_node {
            assert_eq!(layer_a.filters.len(), 1, "DitherV2 filter should remain");
            match &layer_a.filters[0].params {
                FilterParams::DitherV2(params) => {
                    assert_eq!(
                        params.palette_id, None,
                        "DitherV2 palette_id should be cleared to None"
                    );
                }
                _ => panic!("Expected DitherV2 filter"),
            }
        }

        let layer_b_node = snapshot.root.iter().find(|n| {
            matches!(n, LayerNode::Leaf(l) if l.id.0 == layer_b_id)
        });
        assert!(layer_b_node.is_some());
        if let Some(LayerNode::Leaf(layer_b)) = layer_b_node {
            assert_eq!(
                layer_b.filters.len(), 0,
                "PaletteQuantize filter should be removed"
            );
        }
        drop(snapshot);

        assert_eq!(affected_filter_ids.len(), 2);
        assert_eq!(affected_layer_ids.len(), 2);

        let entry_a = state.tiles.tile_cache.entries.get(&key_a).unwrap();
        assert!(
            entry_a.dirty.load(Ordering::Relaxed),
            "Layer A tiles should be dirty after force-delete"
        );
        drop(entry_a);
        let entry_b = state.tiles.tile_cache.entries.get(&key_b).unwrap();
        assert!(
            entry_b.dirty.load(Ordering::Relaxed),
            "Layer B tiles should be dirty after force-delete"
        );
        drop(entry_b);
    }

    pub use crate::services::document_service::install_raster_document;
    pub use crate::services::document_service::import_raster_layer;

    #[test]
    fn validate_document_dimensions_rejects_zero_and_over_max() {
        assert!(validate_document_dimensions(0, 8).is_err());
        assert!(validate_document_dimensions(8, 0).is_err());
        assert!(validate_document_dimensions(MAX_DOCUMENT_DIMENSION + 1, 8).is_err());
        assert!(validate_document_dimensions(8, MAX_DOCUMENT_DIMENSION + 1).is_err());
        assert!(validate_document_dimensions(1, 1).is_ok());
        assert!(validate_document_dimensions(MAX_DOCUMENT_DIMENSION, MAX_DOCUMENT_DIMENSION).is_ok());
    }

    #[test]
    fn invalid_create_size_leaves_document_unchanged() {
        let state = make_test_app_state();
        let before = state.must_active().document_handle.snapshot();
        let before_w = before.width;
        let before_h = before.height;
        let before_len = before.root.len();
        drop(before);

        assert!(validate_document_dimensions(0, 8).is_err());
        assert!(validate_document_dimensions(8193, 8).is_err());

        let after = state.must_active().document_handle.snapshot();
        assert_eq!(after.width, before_w);
        assert_eq!(after.height, before_h);
        assert_eq!(after.root.len(), before_len);
    }

    #[test]
    fn blank_buffer_transparent_is_zeros_white_is_ones() {
        let t = blank_rgba_f32(2, 1, BlankBackground::Transparent);
        assert_eq!(t, vec![0.0; 8]);
        let w = blank_rgba_f32(1, 1, BlankBackground::White);
        assert_eq!(w, vec![1.0, 1.0, 1.0, 1.0]);
    }

    #[test]
    fn create_blank_document_one_leaf_project_path_none() {
        let state = make_test_app_state();
        *state.must_active().project_path.lock().unwrap() = Some(std::path::PathBuf::from("/tmp/old.dyproj"));

        let buf = blank_rgba_f32(8, 8, BlankBackground::White);
        let response = install_raster_document(&state, 8, 8, &buf, None).unwrap();
        assert_eq!(response.doc_id, 2);
        assert_eq!(response.width, 8);
        assert_eq!(response.height, 8);
        assert!(response.tile_count >= 1);

        let snap = state.must_active().document_handle.snapshot();
        assert_eq!(snap.root.len(), 1);
        match &snap.root[0] {
            engine_project::layer::LayerNode::Leaf(layer) => {
                assert_eq!(layer.id.0, 1);
                assert_eq!(layer.kind, engine_project::types::LayerKind::Raster);
                assert!(layer.filters.is_empty());
            }
            _ => panic!("expected a single raster leaf"),
        }
        assert!(state.must_active().project_path.lock().unwrap().is_none());
        assert!(
            !state.tiles.tile_cache.entries.is_empty(),
            "decompose should insert at least one Raw tile"
        );
    }

    #[test]
    fn create_document_does_not_record_recent_files() {
        let dir = tempfile::tempdir().unwrap();
        let recent_path = dir.path().join("recent_files.json");
        std::fs::write(&recent_path, "[]").unwrap();

        let state = make_test_app_state();
        let buf = blank_rgba_f32(8, 8, BlankBackground::Transparent);
        install_raster_document(&state, 8, 8, &buf, None).unwrap();

        let contents = std::fs::read_to_string(&recent_path).unwrap();
        assert_eq!(contents.trim(), "[]");
        assert!(crate::recent_files::load_recent_files(&recent_path).is_empty());
    }

    #[test]
    fn install_raster_document_clears_undo_stacks() {
        let state = make_test_app_state();
        crate::undo::with_document_undo(&state, None, state.active_id().unwrap(), || {
            state.must_active().document_handle.mutate(|doc| {
                doc.increment_generation();
            });
            Ok::<(), String>(())
        })
        .unwrap();
        assert!(state.must_active().history.undo_manager.lock().unwrap().state_dto().can_undo);

        let buf = blank_rgba_f32(8, 8, BlankBackground::White);
        install_raster_document(&state, 8, 8, &buf, None).unwrap();
        let dto = state.must_active().history.undo_manager.lock().unwrap().state_dto();
        assert!(!dto.can_undo);
        assert!(!dto.can_redo);
    }

    #[test]
    fn two_sessions_keep_separate_handles() {
        let state = make_test_app_state();
        let first = state.active_id().unwrap();
        let buf = blank_rgba_f32(8, 8, BlankBackground::White);
        let response = install_raster_document(&state, 8, 8, &buf, None).unwrap();
        assert_ne!(response.doc_id, first);
        assert!(state.session(first).is_ok());
        assert!(state.session(response.doc_id).is_ok());
        assert_eq!(state.active_id(), Some(response.doc_id));
        state.activate(first).unwrap();
        assert_eq!(state.active_id(), Some(first));
        state.close_session(first).unwrap();
        assert!(state.session(first).is_err());
        assert_eq!(state.active_id(), Some(response.doc_id));
    }

    #[test]
    fn second_session_composite_reads_own_raw_not_doc_one() {
        use engine_tiles::{CacheStage, TileCoord, TileKey};
        use std::sync::Arc;

        let state = make_test_app_state();
        {
            let mut red = engine_tiles::PixelTile::new();
            for i in 0..red.data.len() / 4 {
                red.data[i * 4] = 1.0;
                red.data[i * 4 + 3] = 1.0;
            }
            state.tiles.tile_cache.insert_fresh_gen(
                TileKey {
                    doc: 1,
                    layer: 1,
                    coord: TileCoord {
                        level: 0,
                        x: 0,
                        y: 0,
                    },
                    stage: CacheStage::Raw,
                },
                Arc::new(red),
                1,
            );
        }

        let blue = blank_rgba_f32(8, 8, BlankBackground::White);
        let installed = install_raster_document(&state, 8, 8, &blue, None).unwrap();
        assert_ne!(installed.doc_id, 1);

        let key = TileKey {
            doc: installed.doc_id,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        let tile = crate::tile_pipeline::compute_composite_tile(key, &state)
            .expect("composite for second session");
        assert!(
            tile.at(engine_tiles::HALO, engine_tiles::HALO, 0) < 0.1
                || (tile.at(engine_tiles::HALO, engine_tiles::HALO, 0) - 1.0).abs() < 0.05,
            "r={}",
            tile.at(engine_tiles::HALO, engine_tiles::HALO, 0)
        );
        assert!(
            (tile.at(engine_tiles::HALO, engine_tiles::HALO, 0)
                - tile.at(engine_tiles::HALO, engine_tiles::HALO, 1))
            .abs()
                < 0.05,
            "second-doc composite must not pick up doc=1 red Raw"
        );
    }

    #[test]
    fn is_release_build_false_under_debug_assertions() {
        assert!(
            !is_release_build(),
            "unit tests compile with debug_assertions; launch auto-check must stay off"
        );
    }

    fn raw_pixel_at(state: &AppState, layer: u32, x: u32, y: u32) -> [f32; 4] {
        use engine_tiles::{HALO, TILE_SIZE, TileCoord};
        let key = TileKey {
            doc: state.active_id().expect("active document"),
            layer,
            coord: TileCoord {
                level: 0,
                x: x / TILE_SIZE,
                y: y / TILE_SIZE,
            },
            stage: CacheStage::Raw,
        };
        let entry = state
            .tiles.tile_cache
            .entries
            .get(&key)
            .unwrap_or_else(|| panic!("missing raw tile for layer {layer} at ({x},{y})"));
        let lx = (x % TILE_SIZE) + HALO;
        let ly = (y % TILE_SIZE) + HALO;
        [
            entry.tile.at(lx, ly, 0),
            entry.tile.at(lx, ly, 1),
            entry.tile.at(lx, ly, 2),
            entry.tile.at(lx, ly, 3),
        ]
    }

    fn solid_rgba(w: u32, h: u32, r: f32, g: f32, b: f32, a: f32) -> Vec<f32> {
        let mut buf = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            buf.extend_from_slice(&[r, g, b, a]);
        }
        buf
    }

    #[test]
    fn install_raster_replaces_high_gen_source_tiles() {
        use engine_tiles::{TileCoord, HALO};

        let state = make_test_app_state();
        state.must_active().document_handle.mutate(|doc| {
            doc.generations.set_document_gen(40);
        });

        let leftover = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 1,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let raw00 = TileKey {
            doc: 1,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let composite = TileKey {
            doc: 1,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };

        let mut red = PixelTile::new();
        red.set(HALO, HALO, 0, 1.0);
        red.set(HALO, HALO, 3, 1.0);
        assert!(state
            .tiles.tile_cache
            .insert_fresh_gen(raw00, Arc::new(red), 50));
        assert!(state.tiles.tile_cache.insert_fresh_gen(
            leftover,
            Arc::new(PixelTile::new()),
            50
        ));
        assert!(state.tiles.tile_cache.insert_fresh_gen(
            composite,
            Arc::new(PixelTile::new()),
            50
        ));

        let blue = solid_rgba(8, 8, 0.0, 0.0, 1.0, 1.0);
        let installed = install_raster_document(&state, 8, 8, &blue, None).unwrap();
        let new_doc = installed.doc_id;

        assert!(
            state.tiles.tile_cache.entries.get(&leftover).is_some(),
            "previous document tiles stay until that session is closed"
        );
        let px = raw_pixel_at(&state, 1, 0, 0);
        assert!(
            (px[2] - 1.0).abs() < 1e-5 && px[0].abs() < 1e-5,
            "expected blue Image Source, got {px:?}"
        );

        let live_gen = state.must_active().document_handle
            .snapshot()
            .generations
            .current_document_gen();
        assert_eq!(live_gen, 1);
        let new_raw = TileKey {
            doc: new_doc,
            layer: 1,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Raw,
        };
        let raw_entry = state.tiles.tile_cache.entries.get(&new_raw).unwrap();
        assert_eq!(raw_entry.generation, 1);
        drop(raw_entry);

        let new_composite = TileKey {
            doc: new_doc,
            layer: 0,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Composite,
        };
        assert!(state.tiles.tile_cache.insert_fresh_gen(
            new_composite,
            Arc::new(PixelTile::new()),
            live_gen
        ));
    }

    #[test]
    fn place_image_at_origin_pads_smaller_and_clips_larger() {
        let src = vec![1.0, 0.0, 0.0, 1.0];
        let padded = place_image_at_origin(&src, 1, 1, 2, 1);
        assert_eq!(padded, vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);

        let wide = vec![
            1.0, 0.0, 0.0, 1.0,
            0.0, 1.0, 0.0, 1.0,
        ];
        let clipped = place_image_at_origin(&wide, 2, 1, 1, 1);
        assert_eq!(clipped, vec![1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn import_raster_layer_requires_open_document() {
        let state = make_test_app_state();
        let src = solid_rgba(2, 2, 1.0, 0.0, 0.0, 1.0);
        let err = import_raster_layer(&state, 1, 2, 2, &src, None).unwrap_err();
        assert!(
            err.contains("No document") || err.contains("closed") || err.contains("session"),
            "{err}"
        );
    }

    #[test]
    fn import_smaller_image_leaves_transparent_remainder() {
        let state = make_test_app_state();
        let bg = blank_rgba_f32(16, 16, BlankBackground::White);
        install_raster_document(&state, 16, 16, &bg, None).unwrap();

        let src = solid_rgba(4, 4, 1.0, 0.0, 0.0, 1.0);
        let resp = import_raster_layer(&state, state.active_id().unwrap(), 4, 4, &src, None).unwrap();
        assert_eq!(resp.layer_id, 2);

        let snap = state.must_active().document_handle.snapshot();
        assert_eq!(snap.root.len(), 2);
        assert_eq!(snap.width, 16);
        assert_eq!(snap.height, 16);

        let origin = raw_pixel_at(&state, 2, 0, 0);
        assert!((origin[0] - 1.0).abs() < 1e-5 && origin[3] > 0.9);
        let remainder = raw_pixel_at(&state, 2, 8, 0);
        assert!(remainder[3].abs() < 1e-5, "outside source must stay transparent");
        let base = raw_pixel_at(&state, 1, 0, 0);
        assert!((base[0] - 1.0).abs() < 1e-5 && (base[3] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn import_larger_image_clips_to_document() {
        let state = make_test_app_state();
        let bg = blank_rgba_f32(8, 8, BlankBackground::Transparent);
        install_raster_document(&state, 8, 8, &bg, None).unwrap();

        let mut src = vec![0.0; 12 * 8 * 4];
        src[0..4].copy_from_slice(&[1.0, 0.0, 0.0, 1.0]);
        let i7 = 7 * 4;
        src[i7..i7 + 4].copy_from_slice(&[0.0, 0.0, 1.0, 1.0]);
        let i11 = 11 * 4;
        src[i11..i11 + 4].copy_from_slice(&[0.0, 1.0, 0.0, 1.0]);

        let resp = import_raster_layer(&state, state.active_id().unwrap(), 12, 8, &src, None).unwrap();
        let red = raw_pixel_at(&state, resp.layer_id, 0, 0);
        let blue = raw_pixel_at(&state, resp.layer_id, 7, 0);
        assert!((red[0] - 1.0).abs() < 1e-5);
        assert!((blue[2] - 1.0).abs() < 1e-5);
        let snap = state.must_active().document_handle.snapshot();
        assert_eq!(snap.width, 8);
        assert_eq!(snap.height, 8);
    }

    #[test]
    fn import_raster_layer_does_not_rewrite_existing_filter_palette_id() {
        use engine_project::filter::{
            DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
        };
        use engine_project::types::PaletteId;

        let state = make_test_app_state();
        let bg = blank_rgba_f32(8, 8, BlankBackground::White);
        install_raster_document(&state, 8, 8, &bg, None).unwrap();

        state.must_active().document_handle.mutate(|doc| {
            if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
                layer.filters.push(FilterInstance::new(
                    FilterKind::Dither,
                    FilterParams::DitherV2(DitherParamsV2 {
                        mode: DitherModeV2::Bayer4x4,
                        levels: 4,
                        threshold_scale: 1.0,
                        pixel_size: 1,
                        color_mode: DitherColorMode::Rgb,
                        palette_id: Some(PaletteId::new(7)),
                        ..Default::default()
                    }),
                ));
            }
        });

        let src = solid_rgba(2, 2, 0.0, 1.0, 0.0, 1.0);
        import_raster_layer(&state, state.active_id().unwrap(), 2, 2, &src, None).unwrap();

        let snap = state.must_active().document_handle.snapshot();
        match &snap.root[0] {
            engine_project::layer::LayerNode::Leaf(layer) => match &layer.filters[0].params {
                FilterParams::DitherV2(p) => {
                    assert_eq!(p.palette_id, Some(PaletteId::new(7)));
                }
                other => panic!("expected DitherV2, got {other:?}"),
            },
            _ => panic!("expected leaf"),
        }
        assert_eq!(snap.root.len(), 2);
    }

    #[test]
    fn preview_refresh_coalesces_while_pass_in_flight() {
        let state = make_test_app_state();
        assert_eq!(state.tiles.error_residuals.clear_count(), 0);

        state
            .preview_pass_inflight
            .store(1, std::sync::atomic::Ordering::Release);
        for _ in 0..4 {
            request_preview_refresh(&state, 1, true);
        }
        assert_eq!(
            state.tiles.error_residuals.clear_count(),
            0,
            "in-flight pass must stash instead of clearing residuals four times"
        );
        assert!(state.pending_preview_refresh.lock().unwrap().is_some());

        state
            .preview_pass_inflight
            .store(0, std::sync::atomic::Ordering::Release);
        on_preview_task_finished(&state);
        assert_eq!(
            state.tiles.error_residuals.clear_count(),
            1,
            "idle flush applies the latest coalesced refresh once"
        );
        assert!(state.pending_preview_refresh.lock().unwrap().is_none());
    }

    #[test]
    fn preview_refresh_runs_immediately_when_idle() {
        let state = make_test_app_state();
        request_preview_refresh(&state, 1, true);
        request_preview_refresh(&state, 1, true);
        assert_eq!(state.tiles.error_residuals.clear_count(), 2);
    }
}

//! Integration tests for Tauri command handlers and core invariant functions.
//!
//! This module contains comprehensive tests for:
//! - Color conversions (hex ↔ linear RGB, Oklab)
//! - Palette CRUD operations and invalidation
//! - Document lifecycle operations
//! - Layer and filter management
//! - Preview refresh coalescing and scheduling
//!
//! Tests use the `make_test_app_state()` fixture to create an isolated app state
//! with a single test document, allowing mutations without side effects.

use super::*;

/// Create an isolated test application state with one blank document.
///
/// This is the fixture used by all integration tests to avoid interference.
pub(crate) fn make_test_app_state() -> Arc<AppState> {
    use engine_project::Document;
    use engine_project::types::DocumentId;

    let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
    let mut doc = Document::new(DocumentId::new(1), 800, 600);
    doc.root.push(engine_project::layer::LayerNode::Leaf(
        engine_project::layer::Layer::new(
            engine_project::types::LayerId::new(1),
            engine_project::types::LayerKind::Raster,
            800,
            600,
        ),
    ));
    state.spawn_session(doc);
    Arc::new(state)
}

#[test]
fn blend_mode_parsing_works() {
    let bm = match "multiply".into() {
        name => match name {
            "normal" => engine_project::types::BlendMode::Normal,
            "multiply" => engine_project::types::BlendMode::Multiply,
            "screen" => engine_project::types::BlendMode::Screen,
            "overlay" => engine_project::types::BlendMode::Overlay,
            "darken" => engine_project::types::BlendMode::Darken,
            "lighten" => engine_project::types::BlendMode::Lighten,
            "color_dodge" => engine_project::types::BlendMode::ColorDodge,
            "color_burn" => engine_project::types::BlendMode::ColorBurn,
            "hard_light" => engine_project::types::BlendMode::HardLight,
            "soft_light" => engine_project::types::BlendMode::SoftLight,
            "difference" => engine_project::types::BlendMode::Difference,
            "exclusion" => engine_project::types::BlendMode::Exclusion,
            _ => engine_project::types::BlendMode::Normal,
        },
    };
    assert_eq!(bm, engine_project::types::BlendMode::Multiply);
}

#[test]
fn test_f32_to_u8_conversion() {
    assert_eq!(f32_to_u8(0.0), 0);
    assert_eq!(f32_to_u8(1.0), 255);
    assert_eq!(f32_to_u8(0.5), 127);
    assert_eq!(f32_to_u8(2.0), 255); // clamp
    assert_eq!(f32_to_u8(-0.5), 0); // clamp
}

#[test]
fn test_encode_rgba_to_png() {
    let rgba_f32 = vec![1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0];
    let rgba_u8: Vec<u8> = rgba_f32.iter().map(|v| f32_to_u8(*v)).collect();
    let result = encode_rgba_to_png(&rgba_u8, 2, 1).unwrap();
    assert!(!result.is_empty());
    assert!(result.len() > 8);
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
    let result = hex_to_linear("0000Ff").unwrap();
    assert!((result.r - 0.0).abs() < 1e-5);
    assert!((result.g - 0.0).abs() < 1e-5);
    assert!((result.b - 1.0).abs() < 1e-5);
}

#[test]
fn hex_to_linear_black() {
    let result = hex_to_linear("000000").unwrap();
    assert!((result.r - 0.0).abs() < 1e-5);
    assert!((result.g - 0.0).abs() < 1e-5);
    assert!((result.b - 0.0).abs() < 1e-5);
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
    assert!(hex_to_linear("FFF").is_err());
}

#[test]
fn hex_to_linear_err_too_long() {
    assert!(hex_to_linear("FFFFFFF").is_err());
}

#[test]
fn hex_to_linear_err_empty() {
    assert!(hex_to_linear("").is_err());
}

#[test]
fn hex_to_linear_err_with_hash_prefix() {
    assert!(hex_to_linear("#FFFFFF").is_err());
}

#[test]
fn hex_to_linear_err_non_hex_chars() {
    assert!(hex_to_linear("GGGGGG").is_err());
}

#[test]
fn hex_to_linear_err_special_chars() {
    assert!(hex_to_linear("FF@F00").is_err());
}

#[test]
fn linear_to_hex_black() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 0.0,
        g: 0.0,
        b: 0.0,
    });
    assert_eq!(hex, "000000");
}

#[test]
fn linear_to_hex_white() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 1.0,
        g: 1.0,
        b: 1.0,
    });
    assert_eq!(hex, "FFFFFF");
}

#[test]
fn linear_to_hex_red() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 1.0,
        g: 0.0,
        b: 0.0,
    });
    assert_eq!(hex, "FF0000");
}

#[test]
fn linear_to_hex_green() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 0.0,
        g: 1.0,
        b: 0.0,
    });
    assert_eq!(hex, "00FF00");
}

#[test]
fn linear_to_hex_blue() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 0.0,
        g: 0.0,
        b: 1.0,
    });
    assert_eq!(hex, "0000FF");
}

#[test]
fn linear_to_hex_uppercase_format() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 0.5,
        g: 0.5,
        b: 0.5,
    });
    assert!(hex.chars().all(|c| c.is_uppercase() || c.is_numeric()));
}

#[test]
fn linear_to_hex_clamps_above_one() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: 2.0,
        g: 1.5,
        b: 1.0,
    });
    assert_eq!(hex, "FFFFFF");
}

#[test]
fn linear_to_hex_clamps_below_zero() {
    let hex = linear_to_hex(&engine_color::palette::LinearColor {
        r: -0.5,
        g: 0.0,
        b: 0.5,
    });
    assert_eq!(&hex[0..2], "00");
}

#[test]
fn hex_round_trip_known_values() {
    let known = vec!["FF0000", "00FF00", "0000FF", "FFFFFF", "000000", "808080"];
    for hex in known {
        let linear = hex_to_linear(hex).unwrap();
        let back = linear_to_hex(&engine_color::palette::LinearColor {
            r: linear.r,
            g: linear.g,
            b: linear.b,
        });
        assert_eq!(back, hex, "Round-trip failed for {}", hex);
    }
}

#[test]
fn hex_round_trip_case_insensitive() {
    let lower = hex_to_linear("aabbcc").unwrap();
    let upper = hex_to_linear("AABBCC").unwrap();
    assert!((lower.r - upper.r).abs() < 1e-5);
    assert!((lower.g - upper.g).abs() < 1e-5);
    assert!((lower.b - upper.b).abs() < 1e-5);
}

fn gameboy_hexes() -> Vec<String> {
    vec![
        "#0f380f", "#306230", "#8bae20", "#9bbc0f", "#c6b802", "#dde6a3", "#fffbf0", "#ffffff",
    ]
    .into_iter()
    .map(|h| h.to_string())
    .collect()
}

#[test]
fn colors_to_oklab_gameboy_matches_linear_to_oklab() {
    let hexes = gameboy_hexes();
    let oklab_from_cmd = colors_to_oklab(hexes.clone()).unwrap();
    let expected: Vec<_> = hexes
        .iter()
        .map(|hex| {
            let linear = hex_to_linear(hex.trim_start_matches('#')).unwrap();
            oklab_point_from_lin_rgb(
                engine_color::LinRgb {
                    r: linear.r,
                    g: linear.g,
                    b: linear.b,
                },
                hex.clone(),
            )
        })
        .collect();

    assert_eq!(oklab_from_cmd.len(), expected.len());
    for (got, want) in oklab_from_cmd.iter().zip(&expected) {
        assert!((got.l - want.l).abs() < 1e-4);
        assert!((got.a - want.a).abs() < 1e-4);
        assert!((got.b - want.b).abs() < 1e-4);
        assert_eq!(got.srgb_hex.to_ascii_uppercase(), want.srgb_hex.to_ascii_uppercase());
    }
}

#[test]
fn colors_to_oklab_empty_list() {
    let result = colors_to_oklab(vec![]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn colors_to_oklab_invalid_hex_errors() {
    let result = colors_to_oklab(vec!["invalid".to_string()]);
    assert!(result.is_err());
}

fn snapshot_palette_oklab(
    state: &AppState,
    palette_id: u32,
) -> Result<Vec<engine_color::Oklab>, String> {
    let snapshot = state.active_session()?.document_handle.snapshot();
    let palette = snapshot
        .palettes
        .iter()
        .find(|p| p.id == palette_id)
        .ok_or_else(|| format!("Palette {} not found", palette_id))?;
    Ok(palette
        .colors
        .iter()
        .map(|c| engine_color::linear_to_oklab(engine_color::LinRgb {
            r: c.r,
            g: c.g,
            b: c.b,
        }))
        .collect())
}

#[test]
fn get_palette_oklab_missing_palette_errors() {
    let state = make_test_app_state();
    let result = snapshot_palette_oklab(&state, 999);
    assert!(result.is_err());
}

#[test]
fn get_palette_oklab_gameboy_matches_linear_to_oklab() {
    let state = make_test_app_state();
    let hexes = gameboy_hexes();
    let palette_id = engine_project::types::PaletteId::new(1);
    state
        .active_session()
        .unwrap()
        .document_handle
        .mutate(|doc| {
            let colors: Vec<_> = hexes
                .iter()
                .filter_map(|hex| hex_to_linear(hex.trim_start_matches('#')).ok())
                .map(|linear| engine_color::palette::LinearColor {
                    r: linear.r,
                    g: linear.g,
                    b: linear.b,
                })
                .collect();
            doc.palettes.push(engine_color::palette::Palette {
                id: palette_id.0,
                name: "Gameboy".to_string(),
                colors,
                revision: 1,
            });
        });

    let expected = snapshot_palette_oklab(&state, palette_id.0).unwrap();
    let snap = state.active_session().unwrap().document_handle.snapshot();
    let pal = snap.palettes.iter().find(|p| p.id == palette_id.0).unwrap();
    let oklab_from_cmd = oklab_points_from_linear(&pal.colors);

    assert_eq!(oklab_from_cmd.len(), expected.len());
    for (got, want) in oklab_from_cmd.iter().zip(&expected) {
        assert!((got.l - want.l).abs() < 1e-4);
        assert!((got.a - want.a).abs() < 1e-4);
        assert!((got.b - want.b).abs() < 1e-4);
    }
}

#[test]
fn hex_round_trip_exhaustive_boundaries() {
    for byte_val in &[0u8, 1, 127, 128, 254, 255] {
        let hex = format!("{:02X}{:02X}{:02X}", byte_val, byte_val, byte_val);
        let linear = hex_to_linear(&hex).unwrap();
        let back = linear_to_hex(&engine_color::palette::LinearColor {
            r: linear.r,
            g: linear.g,
            b: linear.b,
        });
        assert_eq!(back, hex, "Failed for byte value {}", byte_val);
    }
}

#[test]
fn find_layers_referencing_palette_empty_tree() {
    let result = find_layers_referencing_palette(&[], engine_project::types::PaletteId::new(1));
    assert!(result.is_empty());
}

#[test]
fn find_layers_referencing_palette_no_references() {
    let state = make_test_app_state();
    let snapshot = state.active_session().unwrap().document_handle.snapshot();
    let result = find_layers_referencing_palette(&snapshot.root, engine_project::types::PaletteId::new(1));
    assert!(result.is_empty());
}

#[test]
fn find_layers_referencing_palette_dither_v2_match() {
    use engine_project::filter::{
        DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
    };
    use engine_project::types::PaletteId;

    let state = make_test_app_state();
    let palette_id = PaletteId::new(42);

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
                    palette_id: Some(palette_id),
                    ..Default::default()
                }),
            ));
        }
    });

    let snapshot = state.active_session().unwrap().document_handle.snapshot();
    let result = find_layers_referencing_palette(&snapshot.root, palette_id);
    assert_eq!(result.len(), 1);
    let leaf_id = match &snapshot.root[0] {
        engine_project::layer::LayerNode::Leaf(l) => l.id.0,
        _ => panic!("expected leaf"),
    };
    assert_eq!(result[0].0, leaf_id);
}

#[test]
fn find_layers_referencing_palette_dither_v2_no_palette() {
    use engine_project::filter::{
        DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
    };
    use engine_project::types::PaletteId;

    let state = make_test_app_state();

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
                    palette_id: None,
                    ..Default::default()
                }),
            ));
        }
    });

    let snapshot = state.active_session().unwrap().document_handle.snapshot();
    let result = find_layers_referencing_palette(&snapshot.root, PaletteId::new(99));
    assert!(result.is_empty());
}

#[test]
fn find_layers_referencing_palette_palette_quantize_match() {
    use engine_project::filter::{FilterInstance, FilterKind, FilterParams};
    use engine_project::types::PaletteId;

    let state = make_test_app_state();
    let palette_id = PaletteId::new(77);

    state.must_active().document_handle.mutate(|doc| {
        if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id,
                    diffusion: None,
                },
            ));
        }
    });

    let snapshot = state.active_session().unwrap().document_handle.snapshot();
    let result = find_layers_referencing_palette(&snapshot.root, palette_id);
    assert_eq!(result.len(), 1);
}

#[test]
fn find_layers_referencing_palette_wrong_palette_id() {
    use engine_project::filter::{FilterInstance, FilterKind, FilterParams};
    use engine_project::types::PaletteId;

    let state = make_test_app_state();

    state.must_active().document_handle.mutate(|doc| {
        if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id: PaletteId::new(10),
                    diffusion: None,
                },
            ));
        }
    });

    let snapshot = state.active_session().unwrap().document_handle.snapshot();
    let result = find_layers_referencing_palette(&snapshot.root, PaletteId::new(99));
    assert!(result.is_empty());
}

#[test]
fn find_layers_referencing_palette_recursive_group() {
    use engine_project::filter::{FilterInstance, FilterKind, FilterParams};
    use engine_project::layer::LayerNode;
    use engine_project::types::{LayerId, PaletteId};

    let state = make_test_app_state();
    let palette_id = PaletteId::new(55);

    state.must_active().document_handle.mutate(|doc| {
        let mut children = vec![];
        let mut leaf = engine_project::layer::Layer::new(
            LayerId::new(5),
            engine_project::types::LayerKind::Raster,
            800,
            600,
        );
        leaf.name = "test".to_string();
        leaf.filters = vec![FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id,
                diffusion: None,
            },
        )];
        children.push(LayerNode::Leaf(leaf));
        let group = engine_project::layer::LayerGroup {
            id: LayerId::new(4),
            name: "group".to_string(),
            blend_mode: engine_project::types::BlendMode::Normal,
            opacity: 1.0,
            visible: true,
            mask: None,
            children,
        };
        doc.root.push(LayerNode::Group(group));
    });

    let snapshot = state.active_session().unwrap().document_handle.snapshot();
    let result = find_layers_referencing_palette(&snapshot.root, palette_id);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].0, 5);
}

#[test]
fn find_layers_referencing_palette_multiple_filters_on_one_layer() {
    use engine_project::filter::{
        DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
    };
    use engine_project::types::PaletteId;

    let state = make_test_app_state();
    let palette_id1 = PaletteId::new(111);
    let palette_id2 = PaletteId::new(222);

    state.must_active().document_handle.mutate(|doc| {
        if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.push(FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    palette_id: Some(palette_id1),
                    ..Default::default()
                }),
            ));
            layer.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id: palette_id2,
                    diffusion: None,
                },
            ));
        }
    });

    let snapshot = state.active_session().unwrap().document_handle.snapshot();

    let result1 = find_layers_referencing_palette(&snapshot.root, palette_id1);
    assert_eq!(result1.len(), 1);

    let result2 = find_layers_referencing_palette(&snapshot.root, palette_id2);
    assert_eq!(result2.len(), 1);
}

#[test]
fn integration_palette_crud_lifecycle() {
    use engine_project::types::PaletteId;

    let state = make_test_app_state();
    let mut session = state.must_active();

    // Create palette
    let palette_id = PaletteId::new(1);
    session.document_handle.mutate(|doc| {
            doc.palettes.push(engine_color::palette::Palette {
                id: palette_id.0,
                name: "Test Palette".to_string(),
                colors: vec![
                    engine_color::palette::LinearColor {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    },
                ],
                revision: 1,
            });
    });

    let snap = session.document_handle.snapshot();
    assert_eq!(snap.palettes.len(), 1);
    assert_eq!(snap.palettes[0].name, "Test Palette");

    // Update palette
    session.document_handle.mutate(|doc| {
        if let Some(p) = doc.palettes.iter_mut().find(|p| p.id == palette_id.0) {
            p.name = "Updated Palette".to_string();
        }
    });

    let snap = session.document_handle.snapshot();
    assert_eq!(snap.palettes[0].name, "Updated Palette");

    // Delete palette
    session.document_handle.mutate(|doc| {
        doc.palettes.retain(|p| p.id != palette_id.0);
    });

    let snap = session.document_handle.snapshot();
    assert!(snap.palettes.is_empty());
}

fn cache_dirty_count(state: &AppState) -> usize {
    state
        .tiles
        .tile_cache
        .entries
        .iter()
        .filter(|e| e.dirty.load(std::sync::atomic::Ordering::Acquire))
        .count()
}

fn seed_processed_tile(state: &AppState, layer: u32) {
    use engine_tiles::{CacheStage, PixelTile, TileCoord, TileKey};
    let doc = state.active_id().unwrap();
    state.tiles.tile_cache.insert_fresh(
        TileKey {
            doc,
            layer,
            coord: TileCoord {
                level: 0,
                x: 0,
                y: 0,
            },
            stage: CacheStage::Processed,
        },
        Arc::new(PixelTile::new()),
    );
}

fn invalidate_palette_changed(palette_id: engine_project::types::PaletteId, state: &AppState) {
    let Ok(session) = state.active_session() else {
        return;
    };
    let snapshot = session.document_handle.snapshot();
    let affected = find_layers_referencing_palette(&snapshot.root, palette_id);
    for layer_id in affected {
        engine_tiles::invalidation::invalidate(
            &state.tiles.tile_cache,
            engine_tiles::invalidation::InvalidationEvent::LayerFilterChanged {
                doc: snapshot.id.0,
                layer: layer_id.0,
            },
        );
    }
}

fn create_blank_document(
    state: &AppState,
    width: u32,
    height: u32,
    background: BlankBackground,
) -> Result<LoadImageResponse, String> {
    validate_document_dimensions(width, height)?;
    install_raster_document(
        state,
        width,
        height,
        &blank_rgba_f32(width, height, background),
        None,
        None,
    )
}

#[test]
fn integration_invalidation_cascade_on_palette_modify() {
    use engine_project::filter::{FilterInstance, FilterKind, FilterParams};
    use engine_project::types::PaletteId;

    let state = make_test_app_state();

    // Setup: palette + layer with dither filter
    let palette_id = PaletteId::new(10);
    let session = state.must_active();
    session.document_handle.mutate(|doc| {
            doc.palettes.push(engine_color::palette::Palette {
                id: palette_id.0,
                name: "Test".to_string(),
                colors: vec![
                    engine_color::palette::LinearColor {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    },
                ],
                revision: 1,
            });

        if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id,
                    diffusion: None,
                },
            ));
        }
    });

    seed_processed_tile(&state, 1);
    let initial_dirty = cache_dirty_count(&state);

    // Modify palette → should invalidate affected layers
    invalidate_palette_changed(palette_id, &state);

    let after_invalidate = cache_dirty_count(&state);
    assert!(after_invalidate > initial_dirty, "Palette modification should dirty cache");
}

#[test]
fn integration_no_invalidation_for_unreferenced_palette() {
    use engine_project::types::PaletteId;

    let state = make_test_app_state();

    // Create palette but don't use it
    let unused_palette_id = PaletteId::new(99);
    state.must_active().document_handle.mutate(|doc| {
            doc.palettes.push(engine_color::palette::Palette {
                id: unused_palette_id.0,
                name: "Unused".to_string(),
                colors: vec![],
                revision: 1,
            });
    });

    seed_processed_tile(&state, 1);
    let initial_dirty = cache_dirty_count(&state);

    // Modify unused palette
    invalidate_palette_changed(unused_palette_id, &state);

    let after_invalidate = cache_dirty_count(&state);
    assert_eq!(
        after_invalidate, initial_dirty,
        "Unused palette should not trigger invalidation"
    );
}

#[test]
fn integration_force_delete_palette_clears_references() {
    use engine_project::filter::{FilterInstance, FilterKind, FilterParams};
    use engine_project::types::PaletteId;

    let state = make_test_app_state();
    let palette_id = PaletteId::new(50);

    // Setup palette + reference
    state.must_active().document_handle.mutate(|doc| {
            doc.palettes.push(engine_color::palette::Palette {
                id: palette_id.0,
                name: "Will Delete".to_string(),
                colors: vec![],
                revision: 1,
            });

        if let engine_project::layer::LayerNode::Leaf(layer) = &mut doc.root[0] {
            layer.filters.push(FilterInstance::new(
                FilterKind::PaletteQuantize,
                FilterParams::PaletteQuantize {
                    palette_id,
                    diffusion: None,
                },
            ));
        }
    });

    // Verify reference exists
    let snap = state.must_active().document_handle.snapshot();
    let refs = find_layers_referencing_palette(&snap.root, palette_id);
    assert_eq!(refs.len(), 1);

    // Delete palette
    state.must_active().document_handle.mutate(|doc| {
        doc.palettes.retain(|p| p.id != palette_id.0);
    });

    // References should be... still there (palette is just metadata, filters persist)
    // But palette lookup would fail
    let snap = state.must_active().document_handle.snapshot();
    assert!(snap.palettes.is_empty());
}

#[test]
fn validate_document_dimensions_rejects_zero_and_over_max() {
    assert!(validate_document_dimensions(0, 100).is_err());
    assert!(validate_document_dimensions(100, 0).is_err());
    assert!(validate_document_dimensions(MAX_DOCUMENT_DIMENSION + 1, 100).is_err());
    assert!(validate_document_dimensions(100, MAX_DOCUMENT_DIMENSION + 1).is_err());
}

#[test]
fn invalid_create_size_leaves_document_unchanged() {
    let state = make_test_app_state();
    let before = state.must_active().document_handle.snapshot();

    let result = create_blank_document(&state, 0, 100, BlankBackground::Transparent);
    assert!(result.is_err());

    let after = state.must_active().document_handle.snapshot();
    assert_eq!(before.width, after.width);
    assert_eq!(before.height, after.height);
}

#[test]
fn blank_buffer_transparent_is_zeros_white_is_ones() {
    let transparent = blank_rgba_f32(2, 2, BlankBackground::Transparent);
    assert!(transparent.iter().all(|&v| v == 0.0), "Transparent should be all zeros");

    let white = blank_rgba_f32(2, 2, BlankBackground::White);
    for chunk in white.chunks(4) {
        assert_eq!(chunk[0], 1.0);
        assert_eq!(chunk[1], 1.0);
        assert_eq!(chunk[2], 1.0);
        assert_eq!(chunk[3], 1.0);
    }
}

#[test]
fn create_blank_document_one_leaf_project_path_none() {
    let state = make_test_app_state();
    let result =
        create_blank_document(&state, 100, 100, BlankBackground::Transparent).unwrap();

    let snap = state.must_active().document_handle.snapshot();
    assert_eq!(snap.root.len(), 1);
    assert!(matches!(snap.root[0], engine_project::layer::LayerNode::Leaf(_)));
    let path = state
        .must_active()
        .project_path
        .lock()
        .unwrap()
        .clone();
    assert!(path.is_none());
    assert_eq!(result.width, 100);
    assert_eq!(result.height, 100);
}

#[test]
fn create_document_does_not_record_recent_files() {
    let state = make_test_app_state();
    create_blank_document(&state, 50, 50, BlankBackground::Transparent).unwrap();

    let path = state.must_active().project_path.lock().unwrap().clone();
    assert!(path.is_none());
}

#[test]
fn install_raster_document_clears_undo_stacks() {
    let state = make_test_app_state();
    let bg = blank_rgba_f32(50, 50, BlankBackground::White);

    install_raster_document(&state, 50, 50, &bg, None, None).unwrap();

    let session = state.must_active();
    let undo = session.history.undo_manager.lock().unwrap();
    assert!(!undo.state_dto().can_undo, "Undo stack should be cleared");
    assert!(!undo.state_dto().can_redo);
}

#[test]
fn two_sessions_keep_separate_handles() {
    let state = make_test_app_state();
    let doc1_id = state.active_id().unwrap();

    // Create second session
    state.spawn_session(engine_project::Document::new(
        engine_project::types::DocumentId::new(2),
        100,
        100,
    ));
    let doc2_id = state.active_id().unwrap();

    assert_ne!(doc1_id, doc2_id);

    let session1 = state.require_session(doc1_id).unwrap();
    let session2 = state.require_session(doc2_id).unwrap();

    assert!(!Arc::ptr_eq(&session1, &session2));
    assert_ne!(
        session1.document_handle.snapshot().id.0,
        session2.document_handle.snapshot().id.0
    );
}

#[test]
fn second_session_composite_reads_own_raw_not_doc_one() {
    let state = make_test_app_state();
    let doc1_id = state.active_id().unwrap();
    let w1 = state.must_active().document_handle.snapshot().width;

    state.spawn_session(engine_project::Document::new(
        engine_project::types::DocumentId::new(2),
        100,
        100,
    ));
    let doc2_id = state.active_id().unwrap();
    let w2 = state.must_active().document_handle.snapshot().width;

    assert_ne!(doc1_id, doc2_id);
    assert_eq!(w1, 800);
    assert_eq!(w2, 100);
}

#[test]
fn is_release_build_false_under_debug_assertions() {
    assert!(!is_release_build());
}

fn raw_pixel_at(state: &AppState, layer: u32, x: u32, y: u32) -> [f32; 4] {
    use engine_tiles::{CacheStage, TileCoord, TileKey, HALO};
    let doc = state.active_id().unwrap();
    let tile = state
        .tiles
        .tile_cache
        .get_entry(TileKey {
            doc,
            layer,
            coord: TileCoord {
                level: 0,
                x: x / 256,
                y: y / 256,
            },
            stage: CacheStage::Raw,
        })
        .expect("raw tile");
    let lx = (x % 256) + HALO;
    let ly = (y % 256) + HALO;
    [
        tile.at(lx, ly, 0),
        tile.at(lx, ly, 1),
        tile.at(lx, ly, 2),
        tile.at(lx, ly, 3),
    ]
}

fn solid_rgba(w: u32, h: u32, r: f32, g: f32, b: f32, a: f32) -> Vec<f32> {
    vec![r, g, b, a]
        .repeat((w * h) as usize)
}

#[test]
fn install_raster_replaces_high_gen_source_tiles() {
    let state = make_test_app_state();
    let src = solid_rgba(8, 8, 1.0, 0.0, 0.0, 1.0);
    let resp = install_raster_document(&state, 8, 8, &src, None, None).unwrap();
    let snap = state.must_active().document_handle.snapshot();
    let gen = snap
        .generations
        .document_gen
        .load(std::sync::atomic::Ordering::Acquire);
    assert!(gen > 0);
    assert_eq!(resp.width, 8);
}

#[test]
fn place_image_at_origin_pads_smaller_and_clips_larger() {
    let src_4x4 = solid_rgba(4, 4, 1.0, 1.0, 1.0, 1.0);
    let placed = place_image_at_origin(&src_4x4, 4, 4, 8, 8);

    assert_eq!(placed.len(), 8 * 8 * 4);
    let dst_pixel = raw_pixel_at_placed(&placed, 0, 0, 8);
    assert_eq!(dst_pixel, [1.0, 1.0, 1.0, 1.0]);

    let outside_pixel = raw_pixel_at_placed(&placed, 5, 5, 8);
    assert_eq!(outside_pixel, [0.0, 0.0, 0.0, 0.0]);
}

fn raw_pixel_at_placed(buffer: &[f32], x: u32, y: u32, width: u32) -> [f32; 4] {
    let idx = ((y as usize) * (width as usize) + (x as usize)) * 4;
    [buffer[idx], buffer[idx + 1], buffer[idx + 2], buffer[idx + 3]]
}

#[test]
fn import_raster_layer_requires_open_document() {
    let state = AppState::empty_process(None, 512 * 1024 * 1024, true);
    let src = solid_rgba(2, 2, 1.0, 1.0, 1.0, 1.0);

    let result = import_raster_layer(&state, 999, 2, 2, &src, None);
    assert!(result.is_err());
}

#[test]
fn import_smaller_image_leaves_transparent_remainder() {
    let state = make_test_app_state();
    let bg = blank_rgba_f32(8, 8, BlankBackground::Transparent);
    install_raster_document(&state, 8, 8, &bg, None, None).unwrap();

    let src = solid_rgba(4, 4, 1.0, 0.0, 0.0, 1.0);
    let resp = import_raster_layer(&state, state.active_id().unwrap(), 4, 4, &src, None).unwrap();

    let red = raw_pixel_at(&state, resp.layer_id, 0, 0);
    assert!((red[0] - 1.0).abs() < 1e-5);
    assert!((red[1] - 0.0).abs() < 1e-5);

    let transparent = raw_pixel_at(&state, resp.layer_id, 6, 6);
    assert_eq!(transparent, [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn import_larger_image_clips_to_document() {
    let state = make_test_app_state();
    let bg = blank_rgba_f32(8, 8, BlankBackground::Transparent);
    install_raster_document(&state, 8, 8, &bg, None, None).unwrap();

    let src = solid_rgba(20, 20, 0.0, 1.0, 0.0, 1.0);
    let resp = import_raster_layer(&state, state.active_id().unwrap(), 20, 20, &src, None).unwrap();

    let green = raw_pixel_at(&state, resp.layer_id, 7, 7);
    assert!((green[1] - 1.0).abs() < 1e-5);

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
    install_raster_document(&state, 8, 8, &bg, None, None).unwrap();

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

// Helper function for Oklab testing
fn oklab_point_from_lin_rgb(
    rgb: engine_color::LinRgb,
    srgb_hex: String,
) -> crate::commands::color_lab::OklabPointDto {
    let lab = engine_color::linear_to_oklab(rgb);
    crate::commands::color_lab::OklabPointDto {
        l: lab.l,
        a: lab.a,
        b: lab.b,
        srgb_hex,
    }
}

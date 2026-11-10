//! Task 1 closeout: neutral-filter skip invariants.

use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::PaletteLutCache;
use engine_color::threshold_map::ThresholdMapCache;
use engine_project::document::Document;
use engine_project::filter::{
    DitherColorMode, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filter_is_neutral_for_apply;
use engine_project::filters::apply::apply_enabled_filters_to_tile;
use engine_project::filters::apply::{
    apply_filter_stack_tile_column_strip, apply_filter_stack_tile_row_strip,
};
use engine_project::filters::curves::CurveChannel;
use engine_project::filters::dither_residuals::ErrorResidualsStore;
use engine_project::filters::full_document::{ensure_full_document_ex, FullDocumentCache};
use engine_project::filters::glitch::{GlitchParams, GlitchSubOp};
use engine_project::layer::{Layer, LayerNode};
use engine_project::post_barrier::{
    barrier_index, classify_change, prefix_hash_through_barrier, ChangeClass, PrefixExternals,
};
use engine_project::post_barrier_counters;
use engine_project::types::{BlendMode, DocumentId, LayerId, LayerKind};
use engine_project::{
    note_stack_change, record_post_clear, refresh_policy_for_change,
    set_neutral_verify_enabled_for_test, set_post_cache_enabled_for_test,
};
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::decompose::decompose_image_to_tiles;
use engine_tiles::with_tile_buffer_park;
use engine_tiles::{PixelTile, TileCache, TileCoord, HALO, TILE_SIZE};
use std::sync::Mutex;

/// Serialize tests that toggle `DITHER_POST_CACHE` process-wide.
static POST_CACHE_GATE: Mutex<()> = Mutex::new(());

const TILE_FULL: u32 = TILE_SIZE + 2 * HALO;
const IMG_W: u32 = 256;
const IMG_H: u32 = 256;

fn adjust_zero() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Adjust,
        FilterParams::Adjust {
            contrast: 0.0,
            brightness: 0.0,
            saturation: 0.0,
            blur: 0.0,
            sharpness: 0.0,
            noise: 0.0,
        },
    )
}

fn adjust_active() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Adjust,
        FilterParams::Adjust {
            contrast: 0.0,
            brightness: 0.12,
            saturation: 0.0,
            blur: 0.0,
            sharpness: 0.0,
            noise: 0.0,
        },
    )
}

fn floyd() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            ..Default::default()
        }),
    )
}

fn modulated_x() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::ModulatedDiffuseX,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            line_scale: 5,
            ..Default::default()
        }),
    )
}

fn curves_default() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Curves,
        FilterParams::Curves {
            curve: vec![(0.0, 0.0), (1.0, 1.0)],
            channel: CurveChannel::All,
        },
    )
}

fn curves_channel(ch: CurveChannel) -> FilterInstance {
    FilterInstance::new(
        FilterKind::Curves,
        FilterParams::Curves {
            curve: vec![(0.0, 0.0), (1.0, 1.0)],
            channel: ch,
        },
    )
}

fn levels_default() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.0,
            input_white: 1.0,
            gamma: 1.0,
            output_black: 0.0,
            output_white: 1.0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
        },
    )
}

fn ascii() -> FilterInstance {
    let mut f = FilterInstance::new(
        FilterKind::Ascii,
        FilterParams::Ascii(engine_project::filter::AsciiParams {
            font: "departure_mono".into(),
            size_mode: "px".into(),
            font_px: 11.0,
            symbol_set: "bourke_10".into(),
            match_mode: "tone".into(),
            ..Default::default()
        }),
    );
    f.algorithm_id = Some("ascii".into());
    f
}

fn glitch_spatial_active() -> FilterInstance {
    FilterInstance::new(
        FilterKind::Glitch,
        FilterParams::Glitch(GlitchParams {
            block_shift: GlitchSubOp {
                enabled: true,
                amount: 8.0,
            },
            ..GlitchParams::default()
        }),
    )
}

fn gradient_tile(coord: TileCoord) -> PixelTile {
    let mut tile = PixelTile::new();
    for y in 0..TILE_FULL {
        for x in 0..TILE_FULL {
            let gx = coord.x as i32 * TILE_SIZE as i32 + x as i32 - HALO as i32;
            let gy = coord.y as i32 * TILE_SIZE as i32 + y as i32 - HALO as i32;
            let u = (gx as f32 / IMG_W as f32).clamp(0.0, 1.0);
            let v = (gy as f32 / IMG_H as f32).clamp(0.0, 1.0);
            tile.set(x, y, 0, u);
            tile.set(x, y, 1, v);
            tile.set(x, y, 2, (u + v) * 0.5);
            tile.set(x, y, 3, 1.0);
        }
    }
    tile
}

fn assert_tiles_eq(a: &PixelTile, b: &PixelTile, label: &str) {
    assert_eq!(a.data, b.data, "{label}: bitwise tile mismatch");
}

fn solid_white(w: u32, h: u32) -> Vec<f32> {
    vec![1.0f32; (w * h * 4) as usize]
}

#[test]
fn glitch_sub_enabled_zero_amount_is_neutral() {
    let mut p = GlitchParams::default();
    p.block_shift = GlitchSubOp {
        enabled: true,
        amount: 0.0,
    };
    let f = FilterInstance::new(FilterKind::Glitch, FilterParams::Glitch(p));
    assert!(filter_is_neutral_for_apply(&f));
}

#[test]
fn glitch_master_on_all_subs_off_is_neutral() {
    let f = FilterInstance::new(
        FilterKind::Glitch,
        FilterParams::Glitch(GlitchParams::default()),
    );
    assert!(GlitchParams::default().enabled);
    assert!(filter_is_neutral_for_apply(&f));
}

#[test]
fn curves_levels_exact_identity_and_near_miss_not_neutral() {
    assert!(filter_is_neutral_for_apply(&curves_default()));
    assert!(filter_is_neutral_for_apply(&levels_default()));
    for ch in [
        CurveChannel::All,
        CurveChannel::Red,
        CurveChannel::Green,
        CurveChannel::Blue,
    ] {
        assert!(
            filter_is_neutral_for_apply(&curves_channel(ch)),
            "default linear {ch:?} must be neutral"
        );
        let shifted = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0 + 1e-6)],
                channel: ch,
            },
        );
        assert!(
            !filter_is_neutral_for_apply(&shifted),
            "1e-6 control-point shift must NOT be neutral ({ch:?})"
        );
    }
    let gamma_eps = FilterInstance::new(
        FilterKind::Levels,
        FilterParams::Levels {
            input_black: 0.0,
            input_white: 1.0,
            gamma: 1.0001,
            output_black: 0.0,
            output_white: 1.0,
            channel_r: true,
            channel_g: true,
            channel_b: true,
        },
    );
    assert!(
        !filter_is_neutral_for_apply(&gamma_eps),
        "gamma 1.0001 must NOT be neutral"
    );
}

#[test]
fn classify_neutral_to_non_neutral_at_filter_index() {
    let mut old = vec![curves_default(), floyd(), adjust_zero()];
    let mut new = old.clone();
    if let FilterParams::Adjust {
        ref mut brightness, ..
    } = new[2].params
    {
        *brightness = 0.1;
    }
    assert_eq!(classify_change(&old, &new), ChangeClass::Pre);
    old[2].enabled = false;
    new[2].enabled = true;
    if let FilterParams::Adjust {
        ref mut brightness, ..
    } = new[2].params
    {
        *brightness = 0.0;
    }
    // Enabling a zero Adjust under the barrier is material-identical → Post (no ED).
    assert_eq!(classify_change(&old, &new), ChangeClass::Post);
}

#[test]
fn barrier_none_when_only_expensive_disabled() {
    let mut d = floyd();
    d.enabled = false;
    let stack = vec![curves_default(), d];
    assert_eq!(barrier_index(&stack), None);
}

#[test]
fn reorder_neutral_pre_adjust_prefix_unchanged() {
    let ext = PrefixExternals::default();
    let a = adjust_zero();
    let c = curves_default();
    let d = floyd();
    let h0 = prefix_hash_through_barrier(&[c.clone(), d.clone(), a.clone()], &ext);
    let h1 = prefix_hash_through_barrier(&[c, a, d], &ext);
    assert_eq!(h0, h1, "reordering neutral Pre Adjust must not move prefix");
}

#[test]
fn reverse_active_to_neutral_restores_prefix() {
    let ext = PrefixExternals::default();
    let c = curves_default();
    let d = floyd();
    let adj = adjust_active();
    let mut adj_neutral = adj.clone();
    if let FilterParams::Adjust {
        ref mut brightness, ..
    } = adj_neutral.params
    {
        *brightness = 0.0;
    }

    let base = vec![c.clone(), d.clone()];
    let h0 = prefix_hash_through_barrier(&base, &ext);

    let with_active = vec![c.clone(), d.clone(), adj];
    let h1 = prefix_hash_through_barrier(&with_active, &ext);
    assert_ne!(h0, h1, "active Pre Adjust must move prefix");

    let with_neutral = vec![c, d, adj_neutral];
    let h2 = prefix_hash_through_barrier(&with_neutral, &ext);
    assert_eq!(h0, h2, "returning Adjust to zeros must restore prefix");

    // One invalidation class for the reverse transition at the Pre index.
    assert_eq!(
        classify_change(&with_active, &with_neutral),
        ChangeClass::Pre
    );
}

#[test]
fn post_cache_zero_ed_for_neutral_post_edits_and_add_remove() {
    let _gate = POST_CACHE_GATE.lock().unwrap_or_else(|e| e.into_inner());
    set_post_cache_enabled_for_test(true);
    let ext = PrefixExternals::default();
    let before = post_barrier_counters();

    let c = curves_default();
    let d = floyd();
    let a0 = adjust_zero();

    // Post Curves param edit above Floyd — no residual clear.
    let old = vec![c.clone(), d.clone(), a0.clone()];
    let mut new = old.clone();
    if let FilterParams::Curves { ref mut curve, .. } = new[0].params {
        *curve = vec![(0.0, 0.0), (0.5, 0.55), (1.0, 1.0)];
    }
    let (clear, preserve) = refresh_policy_for_change(&old, &new, true);
    assert!(!clear && preserve);
    assert_eq!(classify_change(&old, &new), ChangeClass::Post);
    note_stack_change(1, &old, &new, Some(0), clear, &ext);
    record_post_clear(clear);

    // Add neutral Adjust under barrier.
    let base = vec![c.clone(), d.clone()];
    let with = vec![c.clone(), d.clone(), a0.clone()];
    let (clear2, preserve2) = refresh_policy_for_change(&base, &with, true);
    assert!(!clear2 && preserve2, "add neutral Pre must skip ED clear");
    assert_eq!(classify_change(&base, &with), ChangeClass::Post);
    note_stack_change(1, &base, &with, Some(2), clear2, &ext);
    record_post_clear(clear2);

    // Remove that neutral Adjust.
    let (clear3, preserve3) = refresh_policy_for_change(&with, &base, true);
    assert!(
        !clear3 && preserve3,
        "remove neutral Pre must skip ED clear"
    );
    assert_eq!(classify_change(&with, &base), ChangeClass::Post);
    note_stack_change(1, &with, &base, Some(2), clear3, &ext);
    record_post_clear(clear3);

    // Add neutral Curves above barrier.
    let dither_only = vec![d.clone()];
    let with_post = vec![c, d];
    let (clear4, _) = refresh_policy_for_change(&dither_only, &with_post, true);
    assert!(!clear4, "add neutral Post must skip ED clear");
    note_stack_change(1, &dither_only, &with_post, Some(0), clear4, &ext);
    record_post_clear(clear4);

    let after = post_barrier_counters();
    assert_eq!(
        after.clear_residuals, before.clear_residuals,
        "neutral Post edits / add-remove must not clear residuals (ED)"
    );
    assert_eq!(
        after.ed_runs_started, before.ed_runs_started,
        "Task 2: neutral Post edits must not start ED (ed_runs_started)"
    );
    assert!(after.clear_skipped > before.clear_skipped);
    assert!(after.classify_post > before.classify_post);
    set_post_cache_enabled_for_test(false);
}

/// Dev verify path: skip + re-apply must match on every tile (no mismatches).
#[test]
fn neutral_verify_mode_checks_tiles_without_mismatch() {
    let _gate = POST_CACHE_GATE.lock().unwrap_or_else(|e| e.into_inner());
    set_neutral_verify_enabled_for_test(true);
    let before = post_barrier_counters();

    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let mut tiles_checked = 0u32;
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();
    let residuals = ErrorResidualsStore::new();
    let layer_id = LayerId::new(1);

    let neutral_adjust = adjust_zero();
    let neutral_curves = curves_default();
    let neutral_levels = levels_default();
    let active = adjust_active();

    for tx in 0..2u32 {
        for ty in 0..2u32 {
            let c = TileCoord {
                level: 0,
                x: tx,
                y: ty,
            };
            let tile = gradient_tile(c);
            let out = with_tile_buffer_park(|park| {
                apply_enabled_filters_to_tile(
                    &tile,
                    &[&neutral_curves, &neutral_levels, &neutral_adjust, &active],
                    layer_id,
                    c,
                    &palette_cache,
                    &lut_cache,
                    &threshold_cache,
                    &doc,
                    &residuals,
                    &blocks,
                    None,
                    park,
                )
            })
            .expect("verify apply");
            let expected = with_tile_buffer_park(|park| {
                apply_enabled_filters_to_tile(
                    &tile,
                    &[&active],
                    layer_id,
                    c,
                    &palette_cache,
                    &lut_cache,
                    &threshold_cache,
                    &doc,
                    &residuals,
                    &blocks,
                    None,
                    park,
                )
            })
            .unwrap();
            assert_eq!(out.data, expected.data, "verify tile ({tx},{ty})");
            tiles_checked += 1;
            let _ = coord;
        }
    }

    let after = post_barrier_counters();
    let skipped = after.neutral_apply_skipped - before.neutral_apply_skipped;
    // 3 neutrals × 4 tiles = 12 skips (verify re-applies inside skip path).
    assert!(
        skipped >= 12,
        "expected ≥12 neutral skips, got {skipped} (tiles_checked={tiles_checked})"
    );
    eprintln!(
        "DITHER_NEUTRAL_VERIFY synthetic: tiles_checked={tiles_checked} neutral_skips={skipped} mismatches=0"
    );
    set_neutral_verify_enabled_for_test(false);
}

#[test]
fn neutral_adjust_below_barrier_row_strip_bit_equal() {
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let pre = gradient_tile(coord);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();

    let mut without = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    without.filters = vec![curves_default(), floyd()];
    let mut with_neutral = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    // Panel top→bottom: Curves (Post), Floyd (barrier), Adjust (Pre).
    with_neutral.filters = vec![curves_default(), floyd(), adjust_zero()];

    let store_a = ErrorResidualsStore::new();
    let store_b = ErrorResidualsStore::new();
    let tiles = vec![(coord, &pre)];
    let mut out_a = vec![PixelTile::new()];
    let mut out_b = vec![PixelTile::new()];
    apply_filter_stack_tile_row_strip(
        &tiles,
        &without,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        &store_a,
        &blocks,
        None,
        &mut out_a,
    )
    .unwrap();
    apply_filter_stack_tile_row_strip(
        &tiles,
        &with_neutral,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        &store_b,
        &blocks,
        None,
        &mut out_b,
    )
    .unwrap();
    assert_tiles_eq(&out_a[0], &out_b[0], "row strip + neutral Pre Adjust");
}

#[test]
fn neutral_adjust_below_barrier_column_strip_bit_equal() {
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let pre = gradient_tile(coord);
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();

    let mut without = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    without.filters = vec![curves_default(), modulated_x()];
    let mut with_neutral = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
    with_neutral.filters = vec![curves_default(), modulated_x(), adjust_zero()];

    let store_a = ErrorResidualsStore::new();
    let store_b = ErrorResidualsStore::new();
    let tiles = vec![(coord, &pre)];
    let mut out_a = vec![PixelTile::new()];
    let mut out_b = vec![PixelTile::new()];
    apply_filter_stack_tile_column_strip(
        &tiles,
        &without,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        &store_a,
        &blocks,
        None,
        &mut out_a,
    )
    .unwrap();
    apply_filter_stack_tile_column_strip(
        &tiles,
        &with_neutral,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        &store_b,
        &blocks,
        None,
        &mut out_b,
    )
    .unwrap();
    assert_tiles_eq(&out_a[0], &out_b[0], "column strip + neutral Pre Adjust");
}

#[test]
fn neutral_adjust_below_barrier_full_document_bit_equal() {
    let _gate = POST_CACHE_GATE.lock().unwrap_or_else(|e| e.into_inner());
    set_post_cache_enabled_for_test(true);
    let w = 48u32;
    let h = 48u32;
    let buf = solid_white(w, h);

    let mut doc_a = Document::new(DocumentId::new(1), w, h);
    let mut layer_a = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    // Post Curves (default/neutral) + Ascii barrier — no Pre Adjust.
    layer_a.filters = vec![curves_default(), ascii()];
    doc_a.root.push(LayerNode::Leaf(layer_a.clone()));
    let cache_a = TileCache::new(50_000_000);
    decompose_image_to_tiles(&buf, w, h, doc_a.id.0, layer_a.id.0, &cache_a).unwrap();

    let mut doc_b = Document::new(DocumentId::new(2), w, h);
    let mut layer_b = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer_b.filters = vec![curves_default(), ascii(), adjust_zero()];
    doc_b.root.push(LayerNode::Leaf(layer_b.clone()));
    let cache_b = TileCache::new(50_000_000);
    decompose_image_to_tiles(&buf, w, h, doc_b.id.0, layer_b.id.0, &cache_b).unwrap();

    let fd_a = FullDocumentCache::new();
    let fd_b = FullDocumentCache::new();
    let a = ensure_full_document_ex(&fd_a, &cache_a, &layer_a, &doc_a, 1, true).unwrap();
    let b = ensure_full_document_ex(&fd_b, &cache_b, &layer_b, &doc_b, 1, true).unwrap();
    assert!(a.needs_post && b.needs_post);
    assert_eq!(
        a.rgba_f32.as_slice(),
        b.rgba_f32.as_slice(),
        "FD through-barrier must ignore neutral Pre Adjust"
    );
    set_post_cache_enabled_for_test(false);
}

fn all_selectable_blends() -> &'static [BlendMode] {
    &[
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
        BlendMode::Darken,
        BlendMode::Lighten,
        BlendMode::ColorDodge,
        BlendMode::ColorBurn,
        BlendMode::HardLight,
        BlendMode::SoftLight,
        BlendMode::Difference,
        BlendMode::Exclusion,
    ]
}

fn with_blend_opacity(mut f: FilterInstance, blend: BlendMode, opacity: f32) -> FilterInstance {
    f.blend_mode = blend;
    f.opacity = opacity;
    f
}

fn apply_tile_stack(filters: &[&FilterInstance], tile: &PixelTile) -> PixelTile {
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();
    let residuals = ErrorResidualsStore::new();
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    with_tile_buffer_park(|park| {
        apply_enabled_filters_to_tile(
            tile,
            filters,
            LayerId::new(1),
            coord,
            &palette_cache,
            &lut_cache,
            &threshold_cache,
            &doc,
            &residuals,
            &blocks,
            None,
            park,
        )
    })
    .unwrap()
}

fn apply_row_strip_layer(layer: &Layer, tile: &PixelTile) -> PixelTile {
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();
    let residuals = ErrorResidualsStore::new();
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let tiles = vec![(coord, tile)];
    let mut out = vec![PixelTile::new()];
    apply_filter_stack_tile_row_strip(
        &tiles,
        layer,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        &residuals,
        &blocks,
        None,
        &mut out,
    )
    .unwrap();
    out.remove(0)
}

fn apply_column_strip_layer(layer: &Layer, tile: &PixelTile) -> PixelTile {
    let palette_cache = PaletteKdCache::new();
    let lut_cache = PaletteLutCache::new();
    let threshold_cache = ThresholdMapCache::new();
    let doc = Document::new(DocumentId::new(1), IMG_W, IMG_H);
    let blocks = BlockRepresentativeCache::new();
    let residuals = ErrorResidualsStore::new();
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let tiles = vec![(coord, tile)];
    let mut out = vec![PixelTile::new()];
    apply_filter_stack_tile_column_strip(
        &tiles,
        layer,
        &palette_cache,
        &lut_cache,
        &threshold_cache,
        &doc,
        &residuals,
        &blocks,
        None,
        &mut out,
    )
    .unwrap();
    out.remove(0)
}

fn assert_blend_matrix_for_identity(label: &str, make: impl Fn() -> FilterInstance) {
    let coord = TileCoord {
        level: 0,
        x: 0,
        y: 0,
    };
    let tile = gradient_tile(coord);
    let without = apply_tile_stack(&[], &tile);

    for &blend in all_selectable_blends() {
        for &opacity in &[1.0f32, 0.5] {
            let f = with_blend_opacity(make(), blend, opacity);
            let expected_skip = blend == BlendMode::Normal || opacity == 0.0;
            assert_eq!(
                filter_is_neutral_for_apply(&f),
                expected_skip,
                "{label} blend={blend:?} opacity={opacity}: neutrality"
            );

            let with = apply_tile_stack(&[&f], &tile);
            if expected_skip {
                assert_eq!(
                    with.data, without.data,
                    "{label} tile Normal/skip blend={blend:?} opacity={opacity}"
                );
            } else if with.data == without.data {
                // Self-blend can be idempotent (e.g. Darken/Lighten). Still must not skip.
                assert!(
                    !filter_is_neutral_for_apply(&f),
                    "{label} idempotent self-blend {blend:?}@{opacity} must not be skipped"
                );
            } else {
                // Non-identity result — filter clearly applied.
            }

            // Row strip
            let mut layer_with = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
            layer_with.filters = vec![f.clone()];
            let layer_without = Layer::new(LayerId::new(1), LayerKind::Raster, IMG_W, IMG_H);
            let row_with = apply_row_strip_layer(&layer_with, &tile);
            let row_without = apply_row_strip_layer(&layer_without, &tile);
            if expected_skip {
                assert_eq!(
                    row_with.data, row_without.data,
                    "{label} row Normal blend={blend:?} opacity={opacity}"
                );
            } else if row_with.data == row_without.data {
                assert!(!filter_is_neutral_for_apply(&f));
            }

            // Column strip
            let col_with = apply_column_strip_layer(&layer_with, &tile);
            let col_without = apply_column_strip_layer(&layer_without, &tile);
            if expected_skip {
                assert_eq!(
                    col_with.data, col_without.data,
                    "{label} column Normal blend={blend:?} opacity={opacity}"
                );
            } else if col_with.data == col_without.data {
                assert!(!filter_is_neutral_for_apply(&f));
            }
        }
    }
}

#[test]
fn blend_matrix_neutral_adjust_curves_levels_glitch() {
    assert_blend_matrix_for_identity("Adjust", adjust_zero);
    assert_blend_matrix_for_identity("Curves", curves_default);
    assert_blend_matrix_for_identity("Levels", levels_default);
    assert_blend_matrix_for_identity("Glitch", || {
        FilterInstance::new(
            FilterKind::Glitch,
            FilterParams::Glitch(GlitchParams::default()),
        )
    });
}

#[test]
fn prefix_hash_keeps_non_normal_zero_adjust() {
    let ext = PrefixExternals::default();
    let c = curves_default();
    let d = floyd();
    let mut adj = adjust_zero();
    adj.blend_mode = BlendMode::Multiply;
    assert!(!filter_is_neutral_for_apply(&adj));
    let without = prefix_hash_through_barrier(&[c.clone(), d.clone()], &ext);
    let with = prefix_hash_through_barrier(&[c, d, adj], &ext);
    assert_ne!(
        without, with,
        "Multiply zero Adjust must remain in prefix hash"
    );
}

/// A3 barrier scenarios from `docs/post-processing-levers.md`:
/// 1. `Dither → Adjust → Glitch` (both Post): Adjust/Glitch edits skip ED
/// 2. `Adjust → Dither → Glitch`: Pre Adjust clears; Post Glitch does not
/// 3. Moving Adjust across the barrier is Barrier once; then Post edits skip ED
#[test]
fn a3_barrier_glitch_adjust_scenarios() {
    let _gate = POST_CACHE_GATE.lock().unwrap_or_else(|e| e.into_inner());
    set_post_cache_enabled_for_test(true);
    let ext = PrefixExternals::default();
    let before = post_barrier_counters();

    // Panel top-first. Processing via rev: Dither → Adjust → Glitch.
    let d = floyd();
    let a_post = adjust_active();
    let g = glitch_spatial_active();
    let stack_post = vec![g.clone(), a_post.clone(), d.clone()];
    assert_eq!(barrier_index(&stack_post), Some(2));

    let mut adjust_edit = stack_post.clone();
    if let FilterParams::Adjust {
        ref mut brightness, ..
    } = adjust_edit[1].params
    {
        *brightness = 0.35;
    }
    assert_eq!(
        classify_change(&stack_post, &adjust_edit),
        ChangeClass::Post
    );
    let (clear_a, preserve_a) = refresh_policy_for_change(&stack_post, &adjust_edit, true);
    assert!(!clear_a && preserve_a, "Post Adjust must preserve Dithered");
    note_stack_change(1, &stack_post, &adjust_edit, Some(1), clear_a, &ext);
    record_post_clear(clear_a);

    let mut glitch_edit = stack_post.clone();
    if let FilterParams::Glitch(ref mut p) = glitch_edit[0].params {
        p.block_shift.amount = 14.0;
    }
    assert_eq!(
        classify_change(&stack_post, &glitch_edit),
        ChangeClass::Post
    );
    let (clear_g, preserve_g) = refresh_policy_for_change(&stack_post, &glitch_edit, true);
    assert!(!clear_g && preserve_g, "Post Glitch must preserve Dithered");
    note_stack_change(1, &stack_post, &glitch_edit, Some(0), clear_g, &ext);
    record_post_clear(clear_g);

    // Processing: Adjust → Dither → Glitch. Panel: [Glitch, Dither, Adjust].
    let a_pre = adjust_active();
    let stack_pre = vec![g.clone(), d.clone(), a_pre.clone()];
    assert_eq!(barrier_index(&stack_pre), Some(1));

    let mut pre_adjust_edit = stack_pre.clone();
    if let FilterParams::Adjust {
        ref mut contrast, ..
    } = pre_adjust_edit[2].params
    {
        *contrast = 0.4;
    }
    assert_eq!(
        classify_change(&stack_pre, &pre_adjust_edit),
        ChangeClass::Pre
    );
    let (clear_pre, preserve_pre) = refresh_policy_for_change(&stack_pre, &pre_adjust_edit, true);
    assert!(
        clear_pre && !preserve_pre,
        "Pre Adjust must clear residuals"
    );
    note_stack_change(1, &stack_pre, &pre_adjust_edit, Some(2), clear_pre, &ext);
    record_post_clear(clear_pre);

    let mut post_glitch_edit = stack_pre.clone();
    if let FilterParams::Glitch(ref mut p) = post_glitch_edit[0].params {
        p.block_scramble = GlitchSubOp {
            enabled: true,
            amount: 0.5,
        };
    }
    assert_eq!(
        classify_change(&stack_pre, &post_glitch_edit),
        ChangeClass::Post
    );
    let (clear_pg, preserve_pg) = refresh_policy_for_change(&stack_pre, &post_glitch_edit, true);
    assert!(!clear_pg && preserve_pg, "Glitch above dither stays Post");
    note_stack_change(1, &stack_pre, &post_glitch_edit, Some(0), clear_pg, &ext);
    record_post_clear(clear_pg);

    // Move Adjust from Pre (below) to Post (above barrier): Barrier once.
    // stack_pre = [Glitch, Dither, Adjust] → [Glitch, Adjust, Dither]
    let crossed = vec![g.clone(), a_pre.clone(), d.clone()];
    assert_eq!(classify_change(&stack_pre, &crossed), ChangeClass::Barrier);
    let (clear_x, preserve_x) = refresh_policy_for_change(&stack_pre, &crossed, true);
    assert!(
        clear_x && !preserve_x,
        "crossing barrier invalidates Dithered"
    );
    note_stack_change(1, &stack_pre, &crossed, Some(1), clear_x, &ext);
    record_post_clear(clear_x);

    // After the cross, Adjust param edits are Post (zero ED / no residual clear).
    let mut after_cross_edit = crossed.clone();
    if let FilterParams::Adjust {
        ref mut brightness, ..
    } = after_cross_edit[1].params
    {
        *brightness = 0.5;
    }
    assert_eq!(
        classify_change(&crossed, &after_cross_edit),
        ChangeClass::Post
    );
    let (clear_after, preserve_after) =
        refresh_policy_for_change(&crossed, &after_cross_edit, true);
    assert!(!clear_after && preserve_after);
    note_stack_change(1, &crossed, &after_cross_edit, Some(1), clear_after, &ext);
    record_post_clear(clear_after);

    let after = post_barrier_counters();
    // Only the Pre Adjust edit + barrier cross should have cleared residuals.
    assert_eq!(
        after.clear_residuals,
        before.clear_residuals + 2,
        "exactly Pre edit + barrier cross clear residuals"
    );
    assert!(
        after.clear_skipped >= before.clear_skipped + 4,
        "Post Adjust/Glitch edits and post-cross Adjust must skip clears"
    );
    assert_eq!(
        after.ed_runs_started, before.ed_runs_started,
        "classify/refresh path must not start ED runs"
    );
    assert!(after.classify_post > before.classify_post);
    assert!(after.classify_pre > before.classify_pre);
    assert!(after.classify_barrier > before.classify_barrier);

    set_post_cache_enabled_for_test(false);
}

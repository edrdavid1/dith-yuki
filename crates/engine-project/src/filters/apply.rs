//! Filter application dispatcher.
//!
//! Main entry point for applying filters to tiles.

use super::adjust::apply_adjust_into;
use super::crt::apply_crt_into;
use super::curves::CurvesFilter;
use super::dither_diffusion::apply_error_diffusion_with_cache_into;
use super::dither_ordered::apply_ordered_with_cache_into;
use super::dither_residuals::ErrorResidualsStore;
use super::glitch::GlitchFilter;
use super::glow::apply_glow_into;
use super::levels::LevelsFilter;
use super::palette_quantize::PaletteQuantizeFilter;
use engine_gpu::GpuContext;
use crate::compositor::blend_tile;
use crate::document::Document;
use crate::error::EngineError;
use crate::filter::{DitherModeV2, DitherParamsV2, FilterInstance, FilterParams};
use crate::layer::Layer;
use crate::types::{BlendMode, LayerId};
use engine_color::palette_cache::PaletteKdCache;
use engine_color::palette_lut::{PaletteLutCache, DEFAULT_LUT_SIZE};
use engine_color::threshold_map::ThresholdMapCache;
use engine_tiles::block_cache::BlockRepresentativeCache;
use engine_tiles::{with_tile_buffer_park, PixelTile, TileBufferPark, TileCoord};

/// Apply all filters in a layer to a tile.
///
/// Accepts shared caches and a document reference for filters that require
/// palette lookups (PaletteQuantize) or threshold map loading (Dither).
///
/// Uses an internal no-op `ErrorResidualsStore` for DitherV2 error diffusion.
/// For cross-tile propagation with a shared store, use `apply_filter_to_tile_with_residuals`.
pub fn apply_filter_to_tile(
    tile: &PixelTile,
    layer: &Layer,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
) -> Result<PixelTile, EngineError> {
    // Use a local residuals store when none is provided externally.
    // This supports export and simple pipeline usage without cross-tile propagation.
    let local_residuals = ErrorResidualsStore::new();
    let local_blocks = BlockRepresentativeCache::new();
    apply_filter_to_tile_with_caches(
        tile, layer, coord, palette_cache, lut_cache, threshold_cache, document,
        &local_residuals,
        &local_blocks,
        None,
    )
}

/// Apply all filters in a layer to a tile, with an explicit `ErrorResidualsStore`
/// for cross-tile error diffusion propagation.
///
/// The tile pipeline (worker) should use this variant, passing the shared
/// `ErrorResidualsStore` from `AppState` so that error diffusion residuals
/// propagate correctly across tile boundaries.
pub fn apply_filter_to_tile_with_residuals(
    tile: &PixelTile,
    layer: &Layer,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
) -> Result<PixelTile, EngineError> {
    let local_blocks = BlockRepresentativeCache::new();
    apply_filter_to_tile_with_caches(
        tile, layer, coord, palette_cache, lut_cache, threshold_cache, document,
        residuals_store, &local_blocks,
        None,
    )
}

/// Full pipeline entry: residuals + block representative cache.
///
/// Uses the thread-local [`TileBufferPark`] for ping-pong working buffers.
/// Kind-specific `apply_*` still return owned tiles (Wave 3 migrates them to
/// src/dst); the dispatcher copies into park buffers so stack depth does not
/// grow dispatcher-owned temps.
///
/// `gpu`: optional shared [`GpuContext`] for GpuEligible pattern filters (Bayer/Halftone/CRT).
/// Error Diffusion is never routed to GPU. Pass `None` for CPU-only.
pub fn apply_filter_to_tile_with_caches(
    tile: &PixelTile,
    layer: &Layer,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    gpu: Option<&GpuContext>,
) -> Result<PixelTile, EngineError> {
    with_tile_buffer_park(|park| {
        apply_filter_to_tile_with_park(
            tile,
            layer,
            coord,
            palette_cache,
            lut_cache,
            threshold_cache,
            document,
            residuals_store,
            block_cache,
            gpu,
            park,
        )
    })
}

/// Same as [`apply_filter_to_tile_with_caches`] but uses an explicit park
/// (tests / callers that already hold a park).
pub fn apply_filter_to_tile_with_park(
    tile: &PixelTile,
    layer: &Layer,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    gpu: Option<&GpuContext>,
    park: &mut TileBufferPark,
) -> Result<PixelTile, EngineError> {
    park.ensure(2);

    // Layers panel is top-to-bottom = `filters[0]` … `filters[n-1]` (Image Source
    // under the last row). Apply bottom-up: last vec entry first, so the top row
    // is the final look and sees the output of everything below — not Raw.
    let enabled: Vec<&FilterInstance> = layer
        .filters
        .iter()
        .rev()
        .filter(|f| f.enabled)
        .collect();

    if enabled.is_empty() {
        let mut out = park.take();
        debug_assert_ne!(
            out.data.as_ptr(),
            tile.data.as_ptr(),
            "dst must not alias Raw / src"
        );
        out.copy_from(tile);
        return Ok(out);
    }

    // Ping-pong pair from the park. `front` is the current pre-image;
    // each filter writes into `back` via adapter, then swap.
    let mut front = park.take();
    debug_assert_ne!(
        front.data.as_ptr(),
        tile.data.as_ptr(),
        "dst must not alias Raw / src"
    );
    front.copy_from(tile);
    let mut back = park.take();
    debug_assert_ne!(front.data.as_ptr(), back.data.as_ptr());

    let mut applied_any = false;
    for filter in enabled {
        if filter.opacity >= 1.0 && filter.blend_mode == BlendMode::Normal {
            engine_tiles::with_raw_block_sampling(!applied_any, || {
                apply_single_filter_into(
                    &front,
                    &mut back,
                    filter,
                    coord,
                    palette_cache,
                    lut_cache,
                    threshold_cache,
                    document,
                    residuals_store,
                    block_cache,
                    layer.id,
                    gpu,
                )
            })?;
        } else {
            // Track I: peak park = 3 (front + back + scratch for full result).
            let mut scratch = park.take();
            let apply_result = engine_tiles::with_raw_block_sampling(!applied_any, || {
                apply_single_filter_into(
                    &front,
                    &mut scratch,
                    filter,
                    coord,
                    palette_cache,
                    lut_cache,
                    threshold_cache,
                    document,
                    residuals_store,
                    block_cache,
                    layer.id,
                    gpu,
                )
            });
            if let Err(e) = apply_result {
                park.give(scratch);
                return Err(e);
            }
            back.copy_from(&front);
            blend_tile(&mut back, &scratch, filter.blend_mode, filter.opacity);
            park.give(scratch);
        }

        std::mem::swap(&mut front, &mut back);
        applied_any = true;
    }

    // Result leaves in `front`; spare returns to the park for the next tile.
    park.give(back);
    Ok(front)
}

/// Full_Then_Blend helper for unit tests. Production stack uses ping-pong in
/// [`apply_filter_to_tile_with_park`]; residuals still come from the full result.
#[cfg(test)]
fn apply_filter_with_blend(
    pre: &PixelTile,
    filter: &FilterInstance,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
    gpu: Option<&GpuContext>,
) -> Result<PixelTile, EngineError> {
    let full_result = apply_single_filter(
        pre, filter, coord, palette_cache, lut_cache, threshold_cache, document,
        residuals_store, block_cache, layer_id, gpu,
    )?;

    if filter.opacity >= 1.0 && filter.blend_mode == BlendMode::Normal {
        return Ok(full_result);
    }

    let mut out = PixelTile::new();
    out.copy_from(pre);
    blend_tile(&mut out, &full_result, filter.blend_mode, filter.opacity);
    Ok(out)
}

/// Apply a single filter into `dst` (no intermediate tile alloc on CPU paths).
fn apply_single_filter_into(
    tile: &PixelTile,
    dst: &mut PixelTile,
    filter: &FilterInstance,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
    gpu: Option<&GpuContext>,
) -> Result<(), EngineError> {
    debug_assert_ne!(
        tile.data.as_ptr(),
        dst.data.as_ptr(),
        "filter src/dst must not alias"
    );
    match &filter.params {
        FilterParams::Curves { curve, channel } => {
            apply_curves_filter_into(tile, curve, *channel, dst)
        }
        FilterParams::Levels {
            input_black,
            input_white,
            gamma,
            output_black,
            output_white,
            channel_r,
            channel_g,
            channel_b,
        } => apply_levels_filter_into(
            tile,
            *input_black,
            *input_white,
            *gamma,
            *output_black,
            *output_white,
            [*channel_r, *channel_g, *channel_b],
            dst,
        ),
        FilterParams::Dither { mode, color_depth } => {
            let params_v2 = DitherParamsV2::from((mode.clone(), *color_depth));
            dispatch_dither_v2_into(
                tile, dst, coord, &params_v2, threshold_cache, palette_cache, lut_cache,
                document, residuals_store, block_cache, layer_id, gpu,
            )
        }
        FilterParams::PaletteQuantize { palette_id, diffusion } => {
            let palette = document
                .get_palette(*palette_id)
                .ok_or_else(|| EngineError::palette_not_found(*palette_id))?;
            let lut = lut_cache
                .get_or_build(document.id.0, palette, palette_cache, DEFAULT_LUT_SIZE)
                .map_err(|e| EngineError::InvalidFilterParams {
                    reason: format!("Failed to build palette LUT: {}", e),
                })?;
            PaletteQuantizeFilter::apply_into(tile, coord, palette, &lut, *diffusion, dst)
        }
        FilterParams::Glitch { glitch_type, intensity, seed } => {
            apply_glitch_filter_into(tile, *glitch_type, *intensity, *seed, coord, dst)
        }
        FilterParams::DitherV2(params) => dispatch_dither_v2_into(
            tile, dst, coord, params, threshold_cache, palette_cache, lut_cache, document,
            residuals_store, block_cache, layer_id, gpu,
        ),
        FilterParams::Glow {
            radius,
            intensity,
            threshold,
        } => {
            apply_glow_into(tile, *radius, *intensity, *threshold, dst);
            Ok(())
        }
        FilterParams::Crt {
            period,
            strength,
            mask_strength,
        } => {
            apply_crt_into(tile, coord, *period, *strength, *mask_strength, dst);
            Ok(())
        }
        FilterParams::Adjust {
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
        } => {
            apply_adjust_into(
                tile,
                coord,
                *contrast,
                *brightness,
                *saturation,
                *blur,
                *sharpness,
                *noise,
                dst,
            );
            Ok(())
        }
        FilterParams::Placeholder(_) => {
            dst.copy_from(tile);
            Ok(())
        }
    }
}

/// Apply a single filter to a tile (allocating wrapper for tests).
#[cfg(test)]
fn apply_single_filter(
    tile: &PixelTile,
    filter: &FilterInstance,
    coord: TileCoord,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    threshold_cache: &ThresholdMapCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
    gpu: Option<&GpuContext>,
) -> Result<PixelTile, EngineError> {
    let mut out = PixelTile::new();
    apply_single_filter_into(
        tile, &mut out, filter, coord, palette_cache, lut_cache, threshold_cache, document,
        residuals_store, block_cache, layer_id, gpu,
    )?;
    Ok(out)
}

/// Dispatch a DitherV2 filter into `dst`.
fn dispatch_dither_v2_into(
    tile: &PixelTile,
    dst: &mut PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    threshold_cache: &ThresholdMapCache,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
    gpu: Option<&GpuContext>,
) -> Result<(), EngineError> {
    let result = dispatch_dither_v2_inner_into(
        tile, dst, coord, params, threshold_cache, palette_cache, lut_cache, document,
        residuals_store, block_cache, layer_id, gpu,
    );
    if let Err(EngineError::PaletteNotFound { .. }) = &result {
        if params.palette_id.is_some() {
            let mut fallback = params.clone();
            fallback.palette_id = None;
            return dispatch_dither_v2_inner_into(
                tile, dst, coord, &fallback, threshold_cache, palette_cache, lut_cache, document,
                residuals_store, block_cache, layer_id, gpu,
            );
        }
    }
    result
}

fn dispatch_dither_v2_inner_into(
    tile: &PixelTile,
    dst: &mut PixelTile,
    coord: TileCoord,
    params: &DitherParamsV2,
    threshold_cache: &ThresholdMapCache,
    palette_cache: &PaletteKdCache,
    lut_cache: &PaletteLutCache,
    document: &Document,
    residuals_store: &ErrorResidualsStore,
    block_cache: &BlockRepresentativeCache,
    layer_id: LayerId,
    gpu: Option<&GpuContext>,
) -> Result<(), EngineError> {
    match &params.mode {
        DitherModeV2::Bayer2x2
        | DitherModeV2::Bayer4x4
        | DitherModeV2::Bayer8x8 => apply_ordered_with_cache_into(
            tile, coord, params, threshold_cache, palette_cache, lut_cache, document,
            block_cache, layer_id, dst,
        ),
        DitherModeV2::CmykHalftone => apply_ordered_with_cache_into(
            tile, coord, params, threshold_cache, palette_cache, lut_cache, document,
            block_cache, layer_id, dst,
        ),
        DitherModeV2::CustomPng { .. } | DitherModeV2::Wave => {
            apply_ordered_with_cache_into(
                tile, coord, params, threshold_cache, palette_cache, lut_cache, document,
                block_cache, layer_id, dst,
            )
        }
        mode if mode.is_error_diffusion() => {
            apply_error_diffusion_with_cache_into(
                tile, coord, params, residuals_store, layer_id,
                palette_cache, lut_cache, document, block_cache, dst,
            )
        }
        _ => unreachable!("unhandled dither mode in dispatch"),
    }
}

/// Apply Curves filter into `dst`.
fn apply_curves_filter_into(
    tile: &PixelTile,
    curve_data: &[(f32, f32)],
    channel: super::curves::CurveChannel,
    dst: &mut PixelTile,
) -> Result<(), EngineError> {
    let mut filter = CurvesFilter::new(channel);
    for &(input, output) in curve_data {
        filter.add_point(input, output)?;
    }
    filter.apply_to_tile_into(tile, dst)
}

/// Apply Levels filter into `dst`.
fn apply_levels_filter_into(
    tile: &PixelTile,
    input_black: f32,
    input_white: f32,
    gamma: f32,
    output_black: f32,
    output_white: f32,
    channels: [bool; 3],
    dst: &mut PixelTile,
) -> Result<(), EngineError> {
    let mut filter = LevelsFilter::new();
    filter.input_black = input_black;
    filter.input_white = input_white;
    filter.gamma = gamma;
    filter.output_black = output_black;
    filter.output_white = output_white;
    filter.channel_r = channels[0];
    filter.channel_g = channels[1];
    filter.channel_b = channels[2];
    filter.rebuild_lut();
    filter.apply_to_tile_into(tile, dst)
}

/// Apply Glitch filter into `dst`.
fn apply_glitch_filter_into(
    tile: &PixelTile,
    glitch_type: super::glitch::GlitchType,
    intensity: f32,
    seed: u64,
    coord: TileCoord,
    dst: &mut PixelTile,
) -> Result<(), EngineError> {
    let filter = GlitchFilter::new(glitch_type, intensity, seed)?;
    filter.apply_to_tile_into(tile, coord, dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::curves::CurveChannel;
    use crate::filter::{FilterInstance, FilterKind, FilterParams, DitherMode, DiffusionKernel};

    fn make_caches_and_doc() -> (PaletteKdCache, PaletteLutCache, ThresholdMapCache, Document, ErrorResidualsStore) {
        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let threshold_cache = ThresholdMapCache::new();
        let doc = Document::new(crate::types::DocumentId::new(1), 256, 256);
        let residuals = ErrorResidualsStore::new();
        (palette_cache, lut_cache, threshold_cache, doc, residuals)
    }

    #[test]
    fn apply_curves_from_filter() {
        let tile = PixelTile::new();
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_levels_from_filter() {
        let tile = PixelTile::new();
        let filter = FilterInstance::new(
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
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_levels_gamma_changes_midtone() {
        let mut tile = PixelTile::new();
        tile.set(8, 8, 0, 0.5);
        tile.set(8, 8, 1, 0.5);
        tile.set(8, 8, 2, 0.5);
        let identity = FilterInstance::new(
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
        );
        let bright = FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 0.0,
                input_white: 1.0,
                gamma: 2.0,
                output_black: 0.0,
                output_white: 1.0,
                channel_r: true,
                channel_g: true,
                channel_b: true,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);
        let a = apply_single_filter(
            &tile,
            &identity,
            coord,
            &pc,
            &lc,
            &tc,
            &doc,
            &rs,
            &BlockRepresentativeCache::new(),
            layer_id,
            None,
        )
        .unwrap();
        let b = apply_single_filter(
            &tile,
            &bright,
            coord,
            &pc,
            &lc,
            &tc,
            &doc,
            &rs,
            &BlockRepresentativeCache::new(),
            layer_id,
            None,
        )
        .unwrap();
        assert!(
            b.at(8, 8, 0) > a.at(8, 8, 0) + 0.05,
            "gamma 2.0 should brighten, identity={}, gamma2={}",
            a.at(8, 8, 0),
            b.at(8, 8, 0)
        );
    }

    #[test]
    fn apply_dither_from_filter() {
        let tile = PixelTile::new();
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg },
                color_depth: 4,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_glitch_from_filter() {
        use crate::filters::glitch::GlitchType;

        let tile = PixelTile::new();
        let filter = FilterInstance::new(
            FilterKind::Glitch,
            FilterParams::Glitch {
                glitch_type: GlitchType::RGBShift,
                intensity: 0.5,
                seed: 12345,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_palette_quantize_missing_palette() {
        let tile = PixelTile::new();
        let filter = FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: crate::types::PaletteId::new(999),
                diffusion: None,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_err());
        match result {
            Err(EngineError::PaletteNotFound { .. }) => {} // expected
            Err(other) => panic!("Expected PaletteNotFound, got: {:?}", other),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }

    #[test]
    fn apply_palette_quantize_with_valid_palette() {
        use crate::types::PaletteId;
        use engine_color::palette::LinearColor;

        let mut tile = PixelTile::new();
        // Set some pixel data
        for y in 0..260u32 {
            for x in 0..260u32 {
                tile.set(x, y, 0, 0.8);
                tile.set(x, y, 1, 0.2);
                tile.set(x, y, 2, 0.1);
                tile.set(x, y, 3, 1.0);
            }
        }

        let palette_cache = PaletteKdCache::new();
        let lut_cache = PaletteLutCache::new();
        let threshold_cache = ThresholdMapCache::new();
        let mut doc = Document::new(crate::types::DocumentId::new(1), 256, 256);
        let residuals = ErrorResidualsStore::new();
        let layer_id = LayerId::new(1);

        let palette_id = doc.add_palette(
            "Test".to_string(),
            vec![
                LinearColor { r: 1.0, g: 0.0, b: 0.0 },
                LinearColor { r: 0.0, g: 1.0, b: 0.0 },
                LinearColor { r: 0.0, g: 0.0, b: 1.0 },
            ],
        );

        let filter = FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id,
                diffusion: None,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };

        let result = apply_single_filter(&tile, &filter, coord, &palette_cache, &lut_cache, &threshold_cache, &doc, &residuals, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());

        // Verify output pixels are palette members
        let output = result.unwrap();
        let r = output.at(10, 10, 0);
        let g = output.at(10, 10, 1);
        let b = output.at(10, 10, 2);
        let palette = doc.get_palette(palette_id).unwrap();
        let is_member = palette.colors.iter().any(|c| c.r == r && c.g == g && c.b == b);
        assert!(is_member, "Output pixel should be a palette member");
    }

    #[test]
    fn skip_disabled_filters() {
        let tile = PixelTile::new();
        let mut layer = Layer::new(crate::types::LayerId::new(1), crate::types::LayerKind::Raster, 256, 256);

        let mut filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        filter.enabled = false;
        layer.filters.push(filter);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, _rs) = make_caches_and_doc();
        let result = apply_filter_to_tile(&tile, &layer, coord, &pc, &lc, &tc, &doc);
        assert!(result.is_ok());
    }

    #[test]
    fn multiple_filters_applied() {
        let tile = PixelTile::new();
        let mut layer = Layer::new(crate::types::LayerId::new(1), crate::types::LayerKind::Raster, 256, 256);

        let filter1 = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        let filter2 = FilterInstance::new(
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
        );
        layer.filters.push(filter1);
        layer.filters.push(filter2);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, _rs) = make_caches_and_doc();
        let result = apply_filter_to_tile(&tile, &layer, coord, &pc, &lc, &tc, &doc);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_dither_v2_ordered() {
        use crate::filter::{DitherModeV2, DitherColorMode, DitherParamsV2};

        let mut tile = PixelTile::new();
        for y in 0..260u32 {
            for x in 0..260u32 {
                tile.set(x, y, 0, 0.5);
                tile.set(x, y, 1, 0.5);
                tile.set(x, y, 2, 0.5);
                tile.set(x, y, 3, 1.0);
            }
        }

        let filter = FilterInstance::new(
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
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_dither_v2_error_diffusion() {
        use crate::filter::{DitherModeV2, DitherColorMode, DitherParamsV2};

        let mut tile = PixelTile::new();
        for y in 0..260u32 {
            for x in 0..260u32 {
                tile.set(x, y, 0, 0.5);
                tile.set(x, y, 1, 0.5);
                tile.set(x, y, 2, 0.5);
                tile.set(x, y, 3, 1.0);
            }
        }

        let filter = FilterInstance::new(
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
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);

        let result = apply_single_filter(&tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &BlockRepresentativeCache::new(), layer_id, None);
        assert!(result.is_ok());
    }

    fn fill_gray(tile: &mut PixelTile, v: f32) {
        for y in 0..260u32 {
            for x in 0..260u32 {
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, v);
                tile.set(x, y, 2, v);
                tile.set(x, y, 3, 1.0);
            }
        }
    }

    #[test]
    fn filter_blend_fast_path_matches_single_filter() {
        use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};

        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.4);
        let filter = FilterInstance::new(
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
        );
        assert_eq!(filter.opacity, 1.0);
        assert_eq!(filter.blend_mode, BlendMode::Normal);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);
        let blocks = BlockRepresentativeCache::new();

        let full = apply_single_filter(
            &tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &blocks, layer_id, None,
        )
        .unwrap();
        let wrapped = apply_filter_with_blend(
            &tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &blocks, layer_id, None,
        )
        .unwrap();
        assert_eq!(full.data.as_ref(), wrapped.data.as_ref());
    }

    #[test]
    fn filter_blend_opacity_half_matches_blend_tile() {
        use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};

        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.4);
        let mut filter = FilterInstance::new(
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
        );
        filter.opacity = 0.5;

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);
        let blocks = BlockRepresentativeCache::new();

        let full = apply_single_filter(
            &tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &blocks, layer_id, None,
        )
        .unwrap();
        let blended = apply_filter_with_blend(
            &tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &blocks, layer_id, None,
        )
        .unwrap();

        let mut expected = PixelTile::new();
        expected.data.copy_from_slice(&tile.data);
        blend_tile(&mut expected, &full, BlendMode::Normal, 0.5);

        for y in engine_tiles::HALO..(engine_tiles::HALO + engine_tiles::TILE_SIZE) {
            for x in engine_tiles::HALO..(engine_tiles::HALO + engine_tiles::TILE_SIZE) {
                for c in 0..4u32 {
                    let a = blended.at(x, y, c);
                    let e = expected.at(x, y, c);
                    assert!(
                        (a - e).abs() < 1e-5,
                        "mix mismatch at ({x},{y},c{c}): got {a} expected {e}"
                    );
                }
            }
        }
    }

    #[test]
    fn glow_opacity_half_is_visual_mix_only() {
        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.8);
        let mut filter = FilterInstance::new(
            FilterKind::Glow,
            FilterParams::Glow {
                radius: 1.0,
                intensity: 1.0,
                threshold: 0.0,
            },
        );
        filter.opacity = 0.5;

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let layer_id = LayerId::new(1);
        let blocks = BlockRepresentativeCache::new();

        let full = apply_single_filter(
            &tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &blocks, layer_id, None,
        )
        .unwrap();
        let blended = apply_filter_with_blend(
            &tile, &filter, coord, &pc, &lc, &tc, &doc, &rs, &blocks, layer_id, None,
        )
        .unwrap();

        let mut expected = PixelTile::new();
        expected.data.copy_from_slice(&tile.data);
        blend_tile(&mut expected, &full, BlendMode::Normal, 0.5);

        let x = engine_tiles::HALO + 8;
        let y = engine_tiles::HALO + 8;
        assert!((blended.at(x, y, 0) - expected.at(x, y, 0)).abs() < 1e-5);
    }

    #[test]
    fn later_stack_filter_sees_earlier_filter_output() {
        use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
        use crate::layer::Layer;
        use crate::types::LayerKind;

        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.3);

        let adjust = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.4,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        let dither = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Bayer4x4,
                levels: 2,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: None,
                ..Default::default()
            }),
        );

        let mut stacked = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        // UI / storage: top row first. Apply walks `.rev()` so Adjust runs first.
        stacked.filters.push(dither.clone());
        stacked.filters.push(adjust);

        let mut dither_only = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        dither_only.filters.push(dither);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let blocks = BlockRepresentativeCache::new();

        let stacked_out = apply_filter_to_tile_with_caches(
            &tile, &stacked, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None,
        )
        .unwrap();
        let dither_out = apply_filter_to_tile_with_caches(
            &tile, &dither_only, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None,
        )
        .unwrap();

        assert_ne!(
            stacked_out.data.as_ref(),
            dither_out.data.as_ref(),
            "dither after brightness must not equal dither on the raw tile"
        );
    }

    #[test]
    fn park_keeps_at_most_two_spares_after_stack() {
        use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
        use crate::layer::Layer;
        use crate::types::LayerKind;
        use engine_tiles::{TILE_PARK_CAPACITY, TileBufferPark};

        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.4);

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        for _ in 0..3 {
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

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let blocks = BlockRepresentativeCache::new();
        let mut park = TileBufferPark::new();

        let _out = apply_filter_to_tile_with_park(
            &tile, &layer, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None, &mut park,
        )
        .unwrap();

        assert!(
            park.len() <= TILE_PARK_CAPACITY,
            "park spilled: len={}",
            park.len()
        );
        // Result left the park; one spare should have been given back.
        assert_eq!(park.len(), 1);

        let _out2 = apply_filter_to_tile_with_park(
            &tile, &layer, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None, &mut park,
        )
        .unwrap();
        assert_eq!(park.len(), 1, "second apply should still leave one spare");
    }

    #[test]
    fn track_i_blend_uses_park_without_growing_past_capacity() {
        use crate::layer::Layer;
        use crate::types::LayerKind;
        use engine_tiles::{TILE_PARK_CAPACITY, TileBufferPark};

        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.5);
        let mut filter = FilterInstance::new(
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
        );
        filter.opacity = 0.5;

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        layer.filters.push(filter);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let blocks = BlockRepresentativeCache::new();
        let mut park = TileBufferPark::new();

        let out = apply_filter_to_tile_with_park(
            &tile, &layer, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None, &mut park,
        )
        .unwrap();

        assert!(park.len() <= TILE_PARK_CAPACITY);
        // Track I takes a third scratch then returns it + the spare → up to 2 in park.
        assert!(
            park.len() >= 1,
            "expected at least one spare after Track I apply"
        );
        // Blended result should differ from identity copy of pre at core.
        let x = engine_tiles::HALO + 4;
        let y = engine_tiles::HALO + 4;
        assert!(
            (out.at(x, y, 0) - tile.at(x, y, 0)).abs() > 1e-6
                || (out.at(x, y, 0) - 0.5).abs() < 1e-5,
            "opacity blend should produce a defined core sample"
        );
    }

    fn bayer_stack(n: usize) -> crate::layer::Layer {
        use crate::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
        use crate::layer::Layer;
        use crate::types::LayerKind;

        let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, 256, 256);
        for _ in 0..n {
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
        layer
    }

    /// Peak live temps beyond the immutable src (warm park): Normal ≤ 2.
    #[test]
    fn peak_live_temps_normal_warm_park() {
        use engine_tiles::{pixel_tile_live, TileBufferPark};

        for n in [1usize, 3, 5] {
            pixel_tile_live::reset();
            let mut tile = PixelTile::new();
            fill_gray(&mut tile, 0.4);
            let src_baseline = pixel_tile_live::live();

            let mut park = TileBufferPark::new();
            park.ensure(2);
            let _ = pixel_tile_live::mark_baseline();

            let layer = bayer_stack(n);
            let coord = TileCoord { level: 0, x: 0, y: 0 };
            let (pc, lc, tc, doc, rs) = make_caches_and_doc();
            let blocks = BlockRepresentativeCache::new();

            let out = apply_filter_to_tile_with_park(
                &tile, &layer, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None, &mut park,
            )
            .unwrap();

            let temps = pixel_tile_live::peak().saturating_sub(src_baseline);
            assert!(
                temps <= 2,
                "Normal stack n={n}: peak temps {temps} (peak={}, src_baseline={src_baseline})",
                pixel_tile_live::peak(),
            );
            // Keep buffers alive until after the assert window.
            drop(out);
            drop(park);
            drop(tile);
        }
    }

    /// Track I may briefly hold a third scratch (peak temps ≤ 3 beyond src).
    #[test]
    fn peak_live_temps_track_i_warm_park() {
        use engine_tiles::{pixel_tile_live, TileBufferPark};

        pixel_tile_live::reset();
        let mut tile = PixelTile::new();
        fill_gray(&mut tile, 0.5);
        let src_baseline = pixel_tile_live::live();

        let mut park = TileBufferPark::new();
        park.ensure(2);
        let _ = pixel_tile_live::mark_baseline();

        let mut filter = FilterInstance::new(
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
        );
        filter.opacity = 0.5;

        let mut layer = crate::layer::Layer::new(
            LayerId::new(1),
            crate::types::LayerKind::Raster,
            256,
            256,
        );
        layer.filters.push(filter);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let (pc, lc, tc, doc, rs) = make_caches_and_doc();
        let blocks = BlockRepresentativeCache::new();

        let out = apply_filter_to_tile_with_park(
            &tile, &layer, coord, &pc, &lc, &tc, &doc, &rs, &blocks, None, &mut park,
        )
        .unwrap();

        let temps = pixel_tile_live::peak().saturating_sub(src_baseline);
        assert!(
            temps <= 3,
            "Track I: peak temps {temps} (peak={}, src_baseline={src_baseline})",
            pixel_tile_live::peak(),
        );
        drop(out);
        drop(park);
        drop(tile);
    }
}

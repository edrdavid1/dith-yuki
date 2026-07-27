//! Filter application dispatcher.
//!
//! Main entry point for applying filters to tiles.

use super::curves::CurvesFilter;
use super::dither::DitherFilter;
use super::glitch::GlitchFilter;
use super::levels::LevelsFilter;
use crate::error::EngineError;
use crate::filter::{FilterInstance, FilterKind, FilterParams};
use crate::layer::Layer;
use engine_tiles::{PixelTile, TileCoord};

/// Apply all filters in a layer to a tile.
pub fn apply_filter_to_tile(
    tile: &PixelTile,
    layer: &Layer,
    coord: TileCoord,
) -> Result<PixelTile, EngineError> {
    let mut result = PixelTile::new();
    
    // Copy source tile to result
    for y in 0u32..260 {
        for x in 0u32..260 {
            for c in 0..4 {
                result.set(x, y, c, tile.at(x, y, c));
            }
        }
    }

    // Apply each filter in the layer's filter stack
    for filter in &layer.filters {
        if !filter.enabled {
            continue; // Skip disabled filters
        }

        result = apply_single_filter(&result, filter, coord)?;
    }

    Ok(result)
}

/// Apply a single filter to a tile.
fn apply_single_filter(
    tile: &PixelTile,
    filter: &FilterInstance,
    coord: TileCoord,
) -> Result<PixelTile, EngineError> {
    match filter.kind {
        FilterKind::Curves => apply_curves_filter(tile, &filter.params),
        FilterKind::Levels => apply_levels_filter(tile, &filter.params),
        FilterKind::Dither => apply_dither_filter(tile, &filter.params, coord),
        FilterKind::Glitch => apply_glitch_filter(tile, &filter.params, coord),
        FilterKind::Placeholder => {
            // Placeholder: return unchanged tile
            let mut result = PixelTile::new();
            for y in 0u32..260 {
                for x in 0u32..260 {
                    for c in 0..4 {
                        result.set(x, y, c, tile.at(x, y, c));
                    }
                }
            }
            Ok(result)
        }
    }
}

/// Apply Curves filter.
fn apply_curves_filter(tile: &PixelTile, params: &FilterParams) -> Result<PixelTile, EngineError> {
    let (curve_data, channel) = match params {
        FilterParams::Curves { curve, channel } => (curve.clone(), *channel),
        _ => return Err(EngineError::InvalidFilterParams {
            reason: "Wrong params for Curves filter".to_string(),
        }),
    };

    let mut filter = CurvesFilter::new(channel);
    for (input, output) in curve_data {
        filter.add_point(input, output)?;
    }

    filter.apply_to_tile(tile)
}

/// Apply Levels filter.
fn apply_levels_filter(tile: &PixelTile, params: &FilterParams) -> Result<PixelTile, EngineError> {
    let (input_black, input_white, gamma, output_black, output_white) = match params {
        FilterParams::Levels {
            input_black,
            input_white,
            gamma,
            output_black,
            output_white,
        } => (*input_black, *input_white, *gamma, *output_black, *output_white),
        _ => return Err(EngineError::InvalidFilterParams {
            reason: "Wrong params for Levels filter".to_string(),
        }),
    };

    let mut filter = LevelsFilter::new();
    filter.input_black = input_black;
    filter.input_white = input_white;
    filter.gamma = gamma;
    filter.output_black = output_black;
    filter.output_white = output_white;

    filter.apply_to_tile(tile)
}

/// Apply Dither filter.
fn apply_dither_filter(tile: &PixelTile, params: &FilterParams, coord: TileCoord) -> Result<PixelTile, EngineError> {
    let (algorithm, color_depth) = match params {
        FilterParams::Dither { algorithm, color_depth } => (*algorithm, *color_depth),
        _ => return Err(EngineError::InvalidFilterParams {
            reason: "Wrong params for Dither filter".to_string(),
        }),
    };

    let filter = DitherFilter::new(algorithm, color_depth)?;
    filter.apply_to_tile(tile, coord)
}

/// Apply Glitch filter.
fn apply_glitch_filter(tile: &PixelTile, params: &FilterParams, coord: TileCoord) -> Result<PixelTile, EngineError> {
    let (glitch_type, intensity, seed) = match params {
        FilterParams::Glitch { glitch_type, intensity, seed } => (*glitch_type, *intensity, *seed),
        _ => return Err(EngineError::InvalidFilterParams {
            reason: "Wrong params for Glitch filter".to_string(),
        }),
    };

    let filter = GlitchFilter::new(glitch_type, intensity, seed)?;
    filter.apply_to_tile(tile, coord)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::curves::CurveChannel;

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

        let result = apply_single_filter(&tile, &filter, coord);
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
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };

        let result = apply_single_filter(&tile, &filter, coord);
        assert!(result.is_ok());
    }

    #[test]
    fn apply_dither_from_filter() {
        use crate::filters::dither::DitherAlgorithm;

        let tile = PixelTile::new();
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                algorithm: DitherAlgorithm::FloydSteinberg,
                color_depth: 4,
            },
        );
        let coord = TileCoord { level: 0, x: 0, y: 0 };

        let result = apply_single_filter(&tile, &filter, coord);
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

        let result = apply_single_filter(&tile, &filter, coord);
        assert!(result.is_ok());
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
        let result = apply_filter_to_tile(&tile, &layer, coord);
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
            },
        );
        layer.filters.push(filter1);
        layer.filters.push(filter2);

        let coord = TileCoord { level: 0, x: 0, y: 0 };
        let result = apply_filter_to_tile(&tile, &layer, coord);
        assert!(result.is_ok());
    }
}


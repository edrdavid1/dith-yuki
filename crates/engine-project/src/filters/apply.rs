//! Filter application dispatcher.
//!
//! Main entry point for applying filters to tiles.

use super::curves::{CurvesFilter, CurveChannel};
use super::levels::LevelsFilter;
use crate::error::EngineError;
use crate::filter::{FilterInstance, FilterKind, FilterParams};
use crate::layer::Layer;
use engine_tiles::{PixelTile, TileCoord};

/// Apply all filters in a layer to a tile.
pub fn apply_filter_to_tile(
    tile: &PixelTile,
    layer: &Layer,
    _coord: TileCoord,
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

        result = apply_single_filter(&result, filter)?;
    }

    Ok(result)
}

/// Apply a single filter to a tile.
fn apply_single_filter(
    tile: &PixelTile,
    filter: &FilterInstance,
) -> Result<PixelTile, EngineError> {
    match filter.kind {
        FilterKind::Curves => apply_curves_filter(tile, &filter.params),
        FilterKind::Levels => apply_levels_filter(tile, &filter.params),
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
        FilterParams::Curves { curve } => (curve.clone(), CurveChannel::All),
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
    let (input_black, input_white, output_black, output_white) = match params {
        FilterParams::Levels {
            input_black,
            input_white,
            output_black,
            output_white,
        } => (*input_black, *input_white, *output_black, *output_white),
        _ => return Err(EngineError::InvalidFilterParams {
            reason: "Wrong params for Levels filter".to_string(),
        }),
    };

    let mut filter = LevelsFilter::new();
    filter.input_black = input_black;
    filter.input_white = input_white;
    filter.output_black = output_black;
    filter.output_white = output_white;

    filter.apply_to_tile(tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_curves_from_filter() {
        let tile = PixelTile::new();
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
            },
        );

        let result = apply_single_filter(&tile, &filter);
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
                output_black: 0.0,
                output_white: 1.0,
            },
        );

        let result = apply_single_filter(&tile, &filter);
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
            },
        );
        let filter2 = FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 0.0,
                input_white: 1.0,
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


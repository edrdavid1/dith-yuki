//! Filter instance model and application.

use crate::error::EngineError;
use crate::filters::dither::DitherAlgorithm;
use crate::filters::glitch::GlitchType;
use crate::filters::curves::CurveChannel;
use crate::types::FilterInstanceId;
use engine_tiles::types::CacheStage;
use engine_tiles::tile::PixelTile;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Filter kind enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    Curves,
    Levels,
    Dither,
    Glitch,
    Placeholder,
}

impl std::fmt::Display for FilterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterKind::Curves => write!(f, "Curves"),
            FilterKind::Levels => write!(f, "Levels"),
            FilterKind::Dither => write!(f, "Dither"),
            FilterKind::Glitch => write!(f, "Glitch"),
            FilterKind::Placeholder => write!(f, "Placeholder"),
        }
    }
}

/// Filter parameters, specific to each FilterKind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FilterParams {
    /// Curves: control points for tone adjustment
    Curves {
        /// Vector of (x, y) control points, normalized 0.0–1.0
        curve: Vec<(f32, f32)>,
        /// Which channel to apply the curve to
        channel: CurveChannel,
    },
    /// Levels: input and output range adjustment
    Levels {
        input_black: f32,
        input_white: f32,
        gamma: f32,
        output_black: f32,
        output_white: f32,
    },
    /// Dither: color reduction with various algorithms
    Dither {
        /// Dithering algorithm selection
        algorithm: DitherAlgorithm,
        /// Target color depth (bits per channel, 1-8)
        color_depth: u8,
    },
    /// Glitch: creative distortion effects
    Glitch {
        /// Glitch effect type
        glitch_type: GlitchType,
        /// Effect intensity (0.0-1.0)
        intensity: f32,
        /// Random seed for reproducibility
        seed: u64,
    },
    /// Placeholder for future filters
    Placeholder(String),
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams::Placeholder("default".to_string())
    }
}

/// A filter instance attached to a layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilterInstance {
    /// Stable identifier for this filter
    pub id: FilterInstanceId,

    /// Which filter to apply
    pub kind: FilterKind,

    /// Filter-specific parameters
    pub params: FilterParams,

    /// Whether this filter is active
    pub enabled: bool,

    /// If true, this filter requires full-row processing (not tiled)
    pub requires_full_row: bool,
}

impl FilterInstance {
    /// Create a new filter instance.
    pub fn new(kind: FilterKind, params: FilterParams) -> Self {
        FilterInstance {
            id: FilterInstanceId::new(),
            kind,
            params,
            enabled: true,
            requires_full_row: false,
        }
    }

    /// Validate the filter parameters.
    pub fn validate(&self) -> Result<(), EngineError> {
        match &self.params {
            FilterParams::Curves { curve, .. } => {
                for (x, y) in curve {
                    if *x < 0.0 || *x > 1.0 || *y < 0.0 || *y > 1.0 {
                        return Err(EngineError::invalid_filter_params(
                            "Curve control point out of [0, 1] range",
                        ));
                    }
                }
                Ok(())
            }
            FilterParams::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            } => {
                if input_black >= input_white {
                    return Err(EngineError::invalid_filter_params(
                        "input_black must be < input_white",
                    ));
                }
                if output_black >= output_white {
                    return Err(EngineError::invalid_filter_params(
                        "output_black must be < output_white",
                    ));
                }
                if *gamma < 0.1 || *gamma > 10.0 {
                    return Err(EngineError::invalid_filter_params(
                        "gamma must be in range [0.1, 10.0]",
                    ));
                }
                Ok(())
            }
            FilterParams::Dither { color_depth, .. } => {
                if !(1..=8).contains(color_depth) {
                    return Err(EngineError::invalid_filter_params(
                        "Color depth must be 1-8 bits",
                    ));
                }
                Ok(())
            }
            FilterParams::Glitch { intensity, .. } => {
                if !(0.0..=1.0).contains(intensity) {
                    return Err(EngineError::invalid_filter_params(
                        "Intensity must be in range [0.0, 1.0]",
                    ));
                }
                Ok(())
            }
            FilterParams::Placeholder(_) => Ok(()),
        }
    }
}

/// Apply a filter to a tile at a specific cache stage.
///
/// # Panics
/// Panics if `requires_full_row` is true (must be handled separately).
pub fn apply_filter_to_tile(
    _tile: &PixelTile,
    filter: &FilterInstance,
    stage: CacheStage,
) -> Arc<PixelTile> {
    // If filter is disabled or at Composite stage, return wrapped in Arc
    if !filter.enabled || stage == CacheStage::Composite {
        return Arc::new(PixelTile::new());
    }

    // Panic if requires_full_row
    if filter.requires_full_row {
        panic!(
            "Filter {:?} requires full-row processing, cannot apply in tiled context",
            filter.kind
        );
    }

    // For now, placeholder implementations return empty tile
    // Phase 3 will add actual filter algorithms
    match filter.kind {
        FilterKind::Curves => Arc::new(PixelTile::new()),
        FilterKind::Levels => Arc::new(PixelTile::new()),
        FilterKind::Dither => Arc::new(PixelTile::new()),
        FilterKind::Glitch => Arc::new(PixelTile::new()),
        FilterKind::Placeholder => Arc::new(PixelTile::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::curves::CurveChannel;

    #[test]
    fn filter_instance_new_is_enabled() {
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves { curve: vec![], channel: CurveChannel::All },
        );
        assert!(filter.enabled);
        assert!(!filter.requires_full_row);
    }

    #[test]
    fn filter_validate_curves() {
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        assert!(filter.validate().is_ok());

        let invalid_filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(1.5, 0.5)],
                channel: CurveChannel::All,
            },
        );
        assert!(invalid_filter.validate().is_err());
    }

    #[test]
    fn filter_validate_levels() {
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
        assert!(filter.validate().is_ok());

        let invalid_filter = FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 1.0,
                input_white: 0.0,
                gamma: 1.0,
                output_black: 0.0,
                output_white: 1.0,
            },
        );
        assert!(invalid_filter.validate().is_err());
    }

    #[test]
    fn filter_validate_dither() {
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                algorithm: DitherAlgorithm::FloydSteinberg,
                color_depth: 4,
            },
        );
        assert!(filter.validate().is_ok());

        let invalid_filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                algorithm: DitherAlgorithm::Ordered,
                color_depth: 0,
            },
        );
        assert!(invalid_filter.validate().is_err());

        let invalid_filter2 = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                algorithm: DitherAlgorithm::Threshold,
                color_depth: 9,
            },
        );
        assert!(invalid_filter2.validate().is_err());
    }

    #[test]
    fn filter_validate_glitch() {
        let filter = FilterInstance::new(
            FilterKind::Glitch,
            FilterParams::Glitch {
                glitch_type: GlitchType::RGBShift,
                intensity: 0.5,
                seed: 12345,
            },
        );
        assert!(filter.validate().is_ok());

        let invalid_filter = FilterInstance::new(
            FilterKind::Glitch,
            FilterParams::Glitch {
                glitch_type: GlitchType::BlockDisplace,
                intensity: 1.5,
                seed: 0,
            },
        );
        assert!(invalid_filter.validate().is_err());
    }

    #[test]
    fn filter_disabled_returns_wrapped() {
        let tile = PixelTile::default();
        let mut filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves { curve: vec![], channel: CurveChannel::All },
        );
        filter.enabled = false;

        let result = apply_filter_to_tile(&tile, &filter, CacheStage::Raw);
        assert!(result.data.len() > 0);
    }

    #[test]
    fn filter_composite_stage_returns_wrapped() {
        let tile = PixelTile::default();
        let filter =
            FilterInstance::new(FilterKind::Curves, FilterParams::Curves { curve: vec![], channel: CurveChannel::All });

        let result = apply_filter_to_tile(&tile, &filter, CacheStage::Composite);
        assert!(result.data.len() > 0);
    }

    #[test]
    #[should_panic(expected = "requires full-row processing")]
    fn filter_requires_full_row_panics() {
        let tile = PixelTile::default();
        let mut filter =
            FilterInstance::new(FilterKind::Curves, FilterParams::Curves { curve: vec![], channel: CurveChannel::All });
        filter.requires_full_row = true;

        apply_filter_to_tile(&tile, &filter, CacheStage::Raw);
    }
}

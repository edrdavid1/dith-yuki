//! Filter instance model and application.

use crate::error::EngineError;
use crate::filters::glitch::GlitchType;
use crate::filters::curves::CurveChannel;
use crate::types::{BlendMode, FilterInstanceId, PaletteId};

fn default_filter_opacity() -> f32 {
    1.0
}
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
    PaletteQuantize,
    Glitch,
    Glow,
    Crt,
    Placeholder,
}

impl std::fmt::Display for FilterKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FilterKind::Curves => write!(f, "Curves"),
            FilterKind::Levels => write!(f, "Levels"),
            FilterKind::Dither => write!(f, "Dither"),
            FilterKind::PaletteQuantize => write!(f, "PaletteQuantize"),
            FilterKind::Glitch => write!(f, "Glitch"),
            FilterKind::Glow => write!(f, "Glow"),
            FilterKind::Crt => write!(f, "Crt"),
            FilterKind::Placeholder => write!(f, "Placeholder"),
        }
    }
}

/// Dither modes for the Dither filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DitherMode {
    /// Bayer ordered dithering with specified matrix size (2, 4, or 8).
    Bayer { matrix_size: u8 },
    /// Custom PNG threshold map loaded from a file path.
    ThresholdMap { path: String },
    /// Error diffusion using a specified kernel.
    ErrorDiffusion { kernel: DiffusionKernel },
}

/// Error diffusion kernel variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffusionKernel {
    FloydSteinberg,
    Atkinson,
    JarvisJudiceNinke,
    Stucki,
    Burkes,
    Sierra,
}

impl DiffusionKernel {
    /// Standard published (dx, dy, weight) tables. Reach is at most 2 px.
    pub fn offsets(self) -> &'static [(i32, i32, f32)] {
        match self {
            Self::FloydSteinberg => &[
                (1, 0, 7.0 / 16.0),
                (-1, 1, 3.0 / 16.0),
                (0, 1, 5.0 / 16.0),
                (1, 1, 1.0 / 16.0),
            ],
            Self::Atkinson => &[
                (1, 0, 1.0 / 8.0),
                (2, 0, 1.0 / 8.0),
                (-1, 1, 1.0 / 8.0),
                (0, 1, 1.0 / 8.0),
                (1, 1, 1.0 / 8.0),
                (0, 2, 1.0 / 8.0),
            ],
            Self::JarvisJudiceNinke => &[
                (1, 0, 7.0 / 48.0),
                (2, 0, 5.0 / 48.0),
                (-2, 1, 3.0 / 48.0),
                (-1, 1, 5.0 / 48.0),
                (0, 1, 7.0 / 48.0),
                (1, 1, 5.0 / 48.0),
                (2, 1, 3.0 / 48.0),
                (-2, 2, 1.0 / 48.0),
                (-1, 2, 3.0 / 48.0),
                (0, 2, 5.0 / 48.0),
                (1, 2, 3.0 / 48.0),
                (2, 2, 1.0 / 48.0),
            ],
            Self::Stucki => &[
                (1, 0, 8.0 / 42.0),
                (2, 0, 4.0 / 42.0),
                (-2, 1, 2.0 / 42.0),
                (-1, 1, 4.0 / 42.0),
                (0, 1, 8.0 / 42.0),
                (1, 1, 4.0 / 42.0),
                (2, 1, 2.0 / 42.0),
                (-2, 2, 1.0 / 42.0),
                (-1, 2, 2.0 / 42.0),
                (0, 2, 4.0 / 42.0),
                (1, 2, 2.0 / 42.0),
                (2, 2, 1.0 / 42.0),
            ],
            Self::Burkes => &[
                (1, 0, 8.0 / 32.0),
                (2, 0, 4.0 / 32.0),
                (-2, 1, 2.0 / 32.0),
                (-1, 1, 4.0 / 32.0),
                (0, 1, 8.0 / 32.0),
                (1, 1, 4.0 / 32.0),
                (2, 1, 2.0 / 32.0),
            ],
            Self::Sierra => &[
                (1, 0, 5.0 / 32.0),
                (2, 0, 3.0 / 32.0),
                (-2, 1, 2.0 / 32.0),
                (-1, 1, 4.0 / 32.0),
                (0, 1, 5.0 / 32.0),
                (1, 1, 4.0 / 32.0),
                (2, 1, 2.0 / 32.0),
                (-1, 2, 2.0 / 32.0),
                (0, 2, 3.0 / 32.0),
                (1, 2, 2.0 / 32.0),
            ],
        }
    }

    /// Parse the PascalCase UI / IPC name used by PaletteQuantize.
    pub fn from_ui_name(s: &str) -> Option<Self> {
        match s {
            "FloydSteinberg" => Some(Self::FloydSteinberg),
            "Atkinson" => Some(Self::Atkinson),
            "JarvisJudiceNinke" => Some(Self::JarvisJudiceNinke),
            "Stucki" => Some(Self::Stucki),
            "Burkes" => Some(Self::Burkes),
            "Sierra" => Some(Self::Sierra),
            _ => None,
        }
    }
}

// ─── Dither V2 types (redesign) ───────────────────────────────────────────────

/// Redesigned dither mode with full parameter set.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherModeV2 {
    #[serde(rename = "bayer_2x2")]
    Bayer2x2,
    #[serde(rename = "bayer_4x4")]
    Bayer4x4,
    #[serde(rename = "bayer_8x8")]
    Bayer8x8,
    CustomPng { path: String },
    FloydSteinberg,
    Atkinson,
    JarvisJudiceNinke,
    Stucki,
    Burkes,
    Sierra,
    /// CMYK angled-screen halftone (ordered path, no ED).
    CmykHalftone,
    /// Sinusoidal / line-modulated threshold (ordered path).
    Wave,
}

impl DitherModeV2 {
    /// Error-diffusion modes that use the residual / full-row path.
    pub fn is_error_diffusion(&self) -> bool {
        matches!(
            self,
            Self::FloydSteinberg
                | Self::Atkinson
                | Self::JarvisJudiceNinke
                | Self::Stucki
                | Self::Burkes
                | Self::Sierra
        )
    }

    /// Matching [`DiffusionKernel`] for ED modes.
    pub fn diffusion_kernel(&self) -> Option<DiffusionKernel> {
        match self {
            Self::FloydSteinberg => Some(DiffusionKernel::FloydSteinberg),
            Self::Atkinson => Some(DiffusionKernel::Atkinson),
            Self::JarvisJudiceNinke => Some(DiffusionKernel::JarvisJudiceNinke),
            Self::Stucki => Some(DiffusionKernel::Stucki),
            Self::Burkes => Some(DiffusionKernel::Burkes),
            Self::Sierra => Some(DiffusionKernel::Sierra),
            _ => None,
        }
    }
}

/// Color processing mode for dithering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherColorMode {
    Rgb,
    Grayscale,
}

/// Full dither filter parameters (V2 redesign).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DitherParamsV2 {
    pub mode: DitherModeV2,
    /// Quantization levels per channel (2–256).
    pub levels: u16,
    /// Threshold scale multiplier (0.1–4.0, default 1.0).
    #[serde(default = "default_threshold_scale")]
    pub threshold_scale: f32,
    /// Pixel block size for retro effects (1–32, default 1).
    #[serde(default = "default_pixel_size")]
    pub pixel_size: u8,
    /// Color processing mode (default Rgb).
    #[serde(default = "default_color_mode")]
    pub color_mode: DitherColorMode,
    /// Optional palette reference for palette-constrained quantization.
    #[serde(default)]
    pub palette_id: Option<PaletteId>,
    /// CMYK halftone cell size in px (2–64, default 8). Used when mode is `CmykHalftone`.
    #[serde(default = "default_halftone_cell_size")]
    pub halftone_cell_size: u8,
    /// Wave wavelength in px (2–256, default 8). Used when mode is `Wave`.
    #[serde(default = "default_wave_wavelength")]
    pub wave_wavelength: f32,
    /// Wave amplitude (0–1, default 1). Used when mode is `Wave`.
    #[serde(default = "default_wave_amplitude")]
    pub wave_amplitude: f32,
    /// Wave phase in radians (default 0). Used when mode is `Wave`.
    #[serde(default)]
    pub wave_phase: f32,
    /// Wave band angle in degrees (default 0 = vertical bands). Used when mode is `Wave`.
    #[serde(default)]
    pub wave_angle: f32,
    /// Additive ordered-threshold shift (default 0). Range `[-0.5, 0.5]`.
    /// Applied as `T' = clamp01(T + bias)` on Bayer / CustomPng / Wave / CmykHalftone.
    /// Error-diffusion modes ignore this field.
    #[serde(default)]
    pub threshold_bias: f32,
    /// Pattern sampling angle in degrees (default 0). Bayer / CustomPng only.
    /// Applied after `aligned(pixel_size)` (Block_Then_Rotate). Periodic via `rem_euclid(360)`.
    #[serde(default)]
    pub pattern_angle: f32,
    /// Serpentine scanning for error-diffusion modes (default false = L→R identity).
    /// Odd **global** rows run R→L with the kernel mirrored in X.
    #[serde(default)]
    pub serpentine: bool,
}

fn default_threshold_scale() -> f32 {
    1.0
}

fn default_pixel_size() -> u8 {
    1
}

fn default_color_mode() -> DitherColorMode {
    DitherColorMode::Rgb
}

fn default_halftone_cell_size() -> u8 {
    8
}

fn default_wave_wavelength() -> f32 {
    8.0
}

fn default_wave_amplitude() -> f32 {
    1.0
}

impl Default for DitherParamsV2 {
    fn default() -> Self {
        Self {
            mode: DitherModeV2::Bayer4x4,
            levels: 4,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            halftone_cell_size: default_halftone_cell_size(),
            wave_wavelength: default_wave_wavelength(),
            wave_amplitude: default_wave_amplitude(),
            wave_phase: 0.0,
            wave_angle: 0.0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
            serpentine: false,
        }
    }
}

/// Convert a legacy `(DitherMode, color_depth)` tuple to the V2 parameter model.
///
/// Maps `color_depth` (1–8 bits) to `levels = 2^color_depth` and translates
/// the legacy `DitherMode` variant to the corresponding `DitherModeV2` variant.
/// All other parameters use defaults (threshold_scale=1.0, pixel_size=1, color_mode=Rgb, palette_id=None).
impl From<(DitherMode, u8)> for DitherParamsV2 {
    fn from((mode, color_depth): (DitherMode, u8)) -> Self {
        let levels = 1u16 << color_depth.min(8); // 2^color_depth, capped at 256
        let new_mode = match mode {
            DitherMode::Bayer { matrix_size: 2 } => DitherModeV2::Bayer2x2,
            DitherMode::Bayer { matrix_size: 4 } => DitherModeV2::Bayer4x4,
            DitherMode::Bayer { matrix_size: 8 } => DitherModeV2::Bayer8x8,
            DitherMode::Bayer { .. } => DitherModeV2::Bayer4x4, // fallback
            DitherMode::ThresholdMap { path } => DitherModeV2::CustomPng { path },
            DitherMode::ErrorDiffusion { kernel } => match kernel {
                DiffusionKernel::FloydSteinberg => DitherModeV2::FloydSteinberg,
                DiffusionKernel::Atkinson => DitherModeV2::Atkinson,
                DiffusionKernel::JarvisJudiceNinke => DitherModeV2::JarvisJudiceNinke,
                DiffusionKernel::Stucki => DitherModeV2::Stucki,
                DiffusionKernel::Burkes => DitherModeV2::Burkes,
                DiffusionKernel::Sierra => DitherModeV2::Sierra,
            },
        };
        DitherParamsV2 {
            mode: new_mode,
            levels,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            halftone_cell_size: default_halftone_cell_size(),
            wave_wavelength: default_wave_wavelength(),
            wave_amplitude: default_wave_amplitude(),
            wave_phase: 0.0,
            wave_angle: 0.0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
            serpentine: false,
        }
    }
}

impl DitherParamsV2 {
    /// Convert a legacy `(DitherMode, color_depth)` pair to the V2 parameter model.
    ///
    /// This is a convenience wrapper around the `From<(DitherMode, u8)>` trait impl.
    pub fn from_legacy(mode: DitherMode, color_depth: u8) -> Self {
        Self::from((mode, color_depth))
    }

    /// Validate all dither parameters are within acceptable ranges.
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(2..=256).contains(&self.levels) {
            return Err(EngineError::invalid_filter_params(
                "levels must be in range [2, 256]",
            ));
        }
        if !(0.1..=4.0).contains(&self.threshold_scale) {
            return Err(EngineError::invalid_filter_params(
                "threshold_scale must be in range [0.1, 4.0]",
            ));
        }
        if !(1..=32).contains(&self.pixel_size) {
            return Err(EngineError::invalid_filter_params(
                "pixel_size must be in range [1, 32]",
            ));
        }
        if let DitherModeV2::CustomPng { ref path } = self.mode {
            if path.is_empty() {
                return Err(EngineError::invalid_filter_params(
                    "custom_path must not be empty for CustomPng mode",
                ));
            }
        }
        if matches!(self.mode, DitherModeV2::CmykHalftone)
            && !(2..=64).contains(&self.halftone_cell_size)
        {
            return Err(EngineError::invalid_filter_params(
                "halftone_cell_size must be in range [2, 64]",
            ));
        }
        if matches!(self.mode, DitherModeV2::Wave) {
            if !(2.0..=256.0).contains(&self.wave_wavelength) {
                return Err(EngineError::invalid_filter_params(
                    "wave_wavelength must be in range [2, 256]",
                ));
            }
            if !(0.0..=1.0).contains(&self.wave_amplitude) {
                return Err(EngineError::invalid_filter_params(
                    "wave_amplitude must be in range [0, 1]",
                ));
            }
        }
        if !(-0.5..=0.5).contains(&self.threshold_bias) {
            return Err(EngineError::invalid_filter_params(
                "threshold_bias must be in range [-0.5, 0.5]",
            ));
        }
        if !self.pattern_angle.is_finite() {
            return Err(EngineError::invalid_filter_params(
                "pattern_angle must be finite",
            ));
        }
        Ok(())
    }
}

// ─── End Dither V2 types ─────────────────────────────────────────────────────

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
    /// Dither: palette-free channel quantization with various modes
    Dither {
        /// Dithering mode selection
        mode: DitherMode,
        /// Target color depth (bits per channel, 1-8)
        color_depth: u8,
    },
    /// PaletteQuantize: Oklab-based palette quantization
    PaletteQuantize {
        /// Reference to the palette to quantize against
        palette_id: PaletteId,
        /// Optional error diffusion kernel (None = nearest-only)
        diffusion: Option<DiffusionKernel>,
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
    /// Redesigned dither with full artistic parameters (V2)
    DitherV2(DitherParamsV2),
    /// Soft glow / bloom (blur + composite). Radius capped to HALO in v1.
    Glow {
        /// Blur radius in px (0.5 .. HALO).
        radius: f32,
        /// Additive bloom strength (0 .. 4).
        intensity: f32,
        /// Luminance threshold; pixels below contribute 0 to the glow mask (0 .. 1).
        threshold: f32,
    },
    /// CRT-style scanlines (+ optional RGB triad mask).
    Crt {
        /// Scanline period in px (2 .. 8).
        period: u8,
        /// Dark-line strength (0 .. 1).
        strength: f32,
        /// RGB subpixel mask strength (0 .. 1, default 0).
        #[serde(default)]
        mask_strength: f32,
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

    /// Visual mix of this filter's full result over its input (`0.0..=1.0`).
    /// Residual ED always uses the full result; opacity is a post-step.
    #[serde(default = "default_filter_opacity")]
    pub opacity: f32,

    /// Blend of full filter output over the pre-filter tile. Default Normal.
    #[serde(default)]
    pub blend_mode: BlendMode,
}

impl FilterInstance {
    /// Create a new filter instance.
    ///
    /// Automatically sets `requires_full_row = true` for error diffusion modes
    /// (DitherV2 ED kernels), signaling the scheduler to process tiles in
    /// wavefront order to satisfy cross-tile error dependencies.
    pub fn new(kind: FilterKind, params: FilterParams) -> Self {
        let requires_full_row = Self::params_require_full_row(&params);
        FilterInstance {
            id: FilterInstanceId::new(),
            kind,
            params,
            enabled: true,
            requires_full_row,
            opacity: 1.0,
            blend_mode: BlendMode::Normal,
        }
    }

    /// Whether these params need the residual / full-row scheduler path.
    pub fn params_require_full_row(params: &FilterParams) -> bool {
        match params {
            FilterParams::DitherV2(p) => p.mode.is_error_diffusion(),
            FilterParams::Dither { mode, .. } => {
                matches!(mode, DitherMode::ErrorDiffusion { .. })
            }
            _ => false,
        }
    }

    /// Validate the filter parameters.
    pub fn validate(&self) -> Result<(), EngineError> {
        if !(0.0..=1.0).contains(&self.opacity) {
            return Err(EngineError::invalid_filter_params(
                "Filter opacity must be in range [0.0, 1.0]",
            ));
        }
        if self.blend_mode.is_reserved() {
            return Err(EngineError::invalid_filter_params(
                "Reserved blend modes are not allowed on filters",
            ));
        }
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
            FilterParams::Dither { mode, color_depth } => {
                if !(1..=8).contains(color_depth) {
                    return Err(EngineError::invalid_filter_params(
                        "Color depth must be 1-8 bits",
                    ));
                }
                match mode {
                    DitherMode::Bayer { matrix_size } => {
                        if !matches!(matrix_size, 2 | 4 | 8) {
                            return Err(EngineError::invalid_filter_params(
                                "Bayer matrix_size must be 2, 4, or 8",
                            ));
                        }
                    }
                    DitherMode::ThresholdMap { path } => {
                        if path.is_empty() {
                            return Err(EngineError::invalid_filter_params(
                                "ThresholdMap path must not be empty",
                            ));
                        }
                    }
                    DitherMode::ErrorDiffusion { .. } => {
                        // All DiffusionKernel variants are valid
                    }
                }
                Ok(())
            }
            FilterParams::PaletteQuantize { .. } => {
                // palette_id validity is checked at apply-time (requires document context)
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
            FilterParams::DitherV2(params) => params.validate(),
            FilterParams::Glow {
                radius,
                intensity,
                threshold,
            } => {
                // v1: radius capped to HALO (2) so blur stays within tile halo.
                if !(0.5..=2.0).contains(radius) {
                    return Err(EngineError::invalid_filter_params(
                        "Glow radius must be in range [0.5, 2.0] (HALO cap)",
                    ));
                }
                if !(0.0..=4.0).contains(intensity) {
                    return Err(EngineError::invalid_filter_params(
                        "Glow intensity must be in range [0.0, 4.0]",
                    ));
                }
                if !(0.0..=1.0).contains(threshold) {
                    return Err(EngineError::invalid_filter_params(
                        "Glow threshold must be in range [0.0, 1.0]",
                    ));
                }
                Ok(())
            }
            FilterParams::Crt {
                period,
                strength,
                mask_strength,
            } => {
                if !(2..=8).contains(period) {
                    return Err(EngineError::invalid_filter_params(
                        "CRT period must be in range [2, 8]",
                    ));
                }
                if !(0.0..=1.0).contains(strength) {
                    return Err(EngineError::invalid_filter_params(
                        "CRT strength must be in range [0.0, 1.0]",
                    ));
                }
                if !(0.0..=1.0).contains(mask_strength) {
                    return Err(EngineError::invalid_filter_params(
                        "CRT mask_strength must be in range [0.0, 1.0]",
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
        FilterKind::PaletteQuantize => Arc::new(PixelTile::new()),
        FilterKind::Glitch => Arc::new(PixelTile::new()),
        FilterKind::Glow => Arc::new(PixelTile::new()),
        FilterKind::Crt => Arc::new(PixelTile::new()),
        FilterKind::Placeholder => Arc::new(PixelTile::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::curves::CurveChannel;
    use serde_json;

    #[test]
    fn filter_instance_new_is_enabled() {
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves { curve: vec![], channel: CurveChannel::All },
        );
        assert!(filter.enabled);
        assert!(!filter.requires_full_row);
        assert_eq!(filter.opacity, 1.0);
        assert_eq!(filter.blend_mode, BlendMode::Normal);
    }

    #[test]
    fn filter_instance_serde_missing_opacity_blend_defaults() {
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        let mut value = serde_json::to_value(&filter).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("opacity");
        obj.remove("blend_mode");
        let restored: FilterInstance = serde_json::from_value(value).unwrap();
        assert_eq!(restored.opacity, 1.0);
        assert_eq!(restored.blend_mode, BlendMode::Normal);
    }

    #[test]
    fn filter_validate_rejects_opacity_and_reserved_blend() {
        let mut filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        filter.opacity = 1.5;
        assert!(filter.validate().is_err());
        filter.opacity = 1.0;
        filter.blend_mode = BlendMode::Reserved12;
        assert!(filter.validate().is_err());
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
                mode: DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg },
                color_depth: 4,
            },
        );
        assert!(filter.validate().is_ok());

        let invalid_filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 4 },
                color_depth: 0,
            },
        );
        assert!(invalid_filter.validate().is_err());

        let invalid_filter2 = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 4 },
                color_depth: 9,
            },
        );
        assert!(invalid_filter2.validate().is_err());
    }

    #[test]
    fn filter_validate_dither_bayer_matrix_size() {
        let valid = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 2 },
                color_depth: 4,
            },
        );
        assert!(valid.validate().is_ok());

        let invalid = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 3 },
                color_depth: 4,
            },
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn filter_validate_dither_threshold_map_empty_path() {
        let invalid = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::ThresholdMap { path: String::new() },
                color_depth: 4,
            },
        );
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn filter_validate_palette_quantize() {
        let filter = FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(1),
                diffusion: Some(DiffusionKernel::Atkinson),
            },
        );
        assert!(filter.validate().is_ok());

        let filter_no_diffusion = FilterInstance::new(
            FilterKind::PaletteQuantize,
            FilterParams::PaletteQuantize {
                palette_id: PaletteId::new(1),
                diffusion: None,
            },
        );
        assert!(filter_no_diffusion.validate().is_ok());
    }

    #[test]
    fn filter_validate_glitch() {
        use crate::filters::glitch::GlitchType;

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

    #[test]
    fn filter_kind_display() {
        assert_eq!(FilterKind::Curves.to_string(), "Curves");
        assert_eq!(FilterKind::Levels.to_string(), "Levels");
        assert_eq!(FilterKind::Dither.to_string(), "Dither");
        assert_eq!(FilterKind::PaletteQuantize.to_string(), "PaletteQuantize");
        assert_eq!(FilterKind::Glitch.to_string(), "Glitch");
        assert_eq!(FilterKind::Glow.to_string(), "Glow");
        assert_eq!(FilterKind::Crt.to_string(), "Crt");
        assert_eq!(FilterKind::Placeholder.to_string(), "Placeholder");
    }

    // ─── DitherModeV2 Serialization Tests (Requirement 11.1) ─────────────────

    #[test]
    fn dither_mode_v2_simple_variants_serialize_as_snake_case_strings() {
        assert_eq!(serde_json::to_value(&DitherModeV2::Bayer2x2).unwrap(), serde_json::json!("bayer_2x2"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Bayer4x4).unwrap(), serde_json::json!("bayer_4x4"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Bayer8x8).unwrap(), serde_json::json!("bayer_8x8"));
        assert_eq!(serde_json::to_value(&DitherModeV2::FloydSteinberg).unwrap(), serde_json::json!("floyd_steinberg"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Atkinson).unwrap(), serde_json::json!("atkinson"));
        assert_eq!(serde_json::to_value(&DitherModeV2::JarvisJudiceNinke).unwrap(), serde_json::json!("jarvis_judice_ninke"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Stucki).unwrap(), serde_json::json!("stucki"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Burkes).unwrap(), serde_json::json!("burkes"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Sierra).unwrap(), serde_json::json!("sierra"));
        assert_eq!(serde_json::to_value(&DitherModeV2::CmykHalftone).unwrap(), serde_json::json!("cmyk_halftone"));
        assert_eq!(serde_json::to_value(&DitherModeV2::Wave).unwrap(), serde_json::json!("wave"));
    }

    #[test]
    fn dither_mode_v2_custom_png_serializes_as_object() {
        let mode = DitherModeV2::CustomPng { path: "/some/path.png".to_string() };
        let value = serde_json::to_value(&mode).unwrap();
        assert_eq!(value, serde_json::json!({"custom_png": {"path": "/some/path.png"}}));
    }

    #[test]
    fn dither_mode_v2_roundtrip_simple_variants() {
        let variants = vec![
            DitherModeV2::Bayer2x2,
            DitherModeV2::Bayer4x4,
            DitherModeV2::Bayer8x8,
            DitherModeV2::FloydSteinberg,
            DitherModeV2::Atkinson,
            DitherModeV2::JarvisJudiceNinke,
            DitherModeV2::Stucki,
            DitherModeV2::Burkes,
            DitherModeV2::Sierra,
            DitherModeV2::CmykHalftone,
            DitherModeV2::Wave,
        ];
        for variant in variants {
            let json = serde_json::to_string(&variant).unwrap();
            let deserialized: DitherModeV2 = serde_json::from_str(&json).unwrap();
            // Verify round-trip by re-serializing
            let json2 = serde_json::to_string(&deserialized).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn dither_mode_v2_roundtrip_custom_png() {
        let mode = DitherModeV2::CustomPng { path: "/Users/artist/patterns/halftone.png".to_string() };
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: DitherModeV2 = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn dither_mode_v2_deserialization_from_string() {
        let mode: DitherModeV2 = serde_json::from_str(r#""bayer_2x2""#).unwrap();
        assert_eq!(serde_json::to_value(&mode).unwrap(), serde_json::json!("bayer_2x2"));

        let mode: DitherModeV2 = serde_json::from_str(r#""floyd_steinberg""#).unwrap();
        assert_eq!(serde_json::to_value(&mode).unwrap(), serde_json::json!("floyd_steinberg"));
    }

    #[test]
    fn serpentine_missing_field_defaults_false() {
        let p: DitherParamsV2 = serde_json::from_str(
            r#"{"mode":"floyd_steinberg","levels":4}"#,
        )
        .unwrap();
        assert!(!p.serpentine);
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["serpentine"], serde_json::json!(false));
    }

    #[test]
    fn dither_mode_v2_deserialization_from_object() {
        let mode: DitherModeV2 = serde_json::from_str(r#"{"custom_png": {"path": "/tmp/test.png"}}"#).unwrap();
        assert_eq!(
            serde_json::to_value(&mode).unwrap(),
            serde_json::json!({"custom_png": {"path": "/tmp/test.png"}})
        );
    }

    // ─── Task 6.3: requires_full_row Tests ───────────────────────────────

    #[test]
    fn dither_v2_floyd_steinberg_requires_full_row() {
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
        assert!(filter.requires_full_row, "FloydSteinberg should require full row processing");
    }

    #[test]
    fn dither_v2_atkinson_requires_full_row() {
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::DitherV2(DitherParamsV2 {
                mode: DitherModeV2::Atkinson,
                levels: 4,
                threshold_scale: 1.0,
                pixel_size: 1,
                color_mode: DitherColorMode::Rgb,
                palette_id: None,
            ..Default::default()
            }),
        );
        assert!(filter.requires_full_row, "Atkinson should require full row processing");
    }

    #[test]
    fn dither_v2_m1_kernels_require_full_row() {
        for mode in [
            DitherModeV2::JarvisJudiceNinke,
            DitherModeV2::Stucki,
            DitherModeV2::Burkes,
            DitherModeV2::Sierra,
        ] {
            let filter = FilterInstance::new(
                FilterKind::Dither,
                FilterParams::DitherV2(DitherParamsV2 {
                    mode: mode.clone(),
                    levels: 4,
                    ..Default::default()
                }),
            );
            assert!(
                filter.requires_full_row,
                "{mode:?} should require full row processing"
            );
        }
    }

    #[test]
    fn dither_v2_ordered_does_not_require_full_row() {
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
        assert!(!filter.requires_full_row, "Bayer4x4 should NOT require full row processing");
    }

    #[test]
    fn legacy_dither_error_diffusion_requires_full_row() {
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg },
                color_depth: 4,
            },
        );
        assert!(filter.requires_full_row, "Legacy error diffusion should require full row processing");
    }

    #[test]
    fn legacy_dither_bayer_does_not_require_full_row() {
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::Bayer { matrix_size: 4 },
                color_depth: 4,
            },
        );
        assert!(!filter.requires_full_row, "Legacy Bayer should NOT require full row processing");
    }

    // ─── DitherParamsV2::from_legacy / From<(DitherMode, u8)> Tests ─────

    #[test]
    fn from_trait_bayer2() {
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 2 }, 3u8));
        assert!(matches!(params.mode, DitherModeV2::Bayer2x2));
        assert_eq!(params.levels, 8); // 2^3
        assert_eq!(params.threshold_scale, 1.0);
        assert_eq!(params.pixel_size, 1);
        assert_eq!(params.color_mode, DitherColorMode::Rgb);
        assert!(params.palette_id.is_none());
    }

    #[test]
    fn from_trait_bayer4() {
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 4 }, 5u8));
        assert!(matches!(params.mode, DitherModeV2::Bayer4x4));
        assert_eq!(params.levels, 32); // 2^5
    }

    #[test]
    fn from_trait_bayer8() {
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 8 }, 1u8));
        assert!(matches!(params.mode, DitherModeV2::Bayer8x8));
        assert_eq!(params.levels, 2); // 2^1
    }

    #[test]
    fn from_trait_bayer_fallback() {
        // Unknown matrix size falls back to Bayer4x4
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 16 }, 4u8));
        assert!(matches!(params.mode, DitherModeV2::Bayer4x4));
        assert_eq!(params.levels, 16); // 2^4
    }

    #[test]
    fn from_legacy_bayer2() {
        let params = DitherParamsV2::from_legacy(DitherMode::Bayer { matrix_size: 2 }, 3);
        assert!(matches!(params.mode, DitherModeV2::Bayer2x2));
        assert_eq!(params.levels, 8); // 2^3
        assert_eq!(params.threshold_scale, 1.0);
        assert_eq!(params.pixel_size, 1);
        assert_eq!(params.color_mode, DitherColorMode::Rgb);
        assert!(params.palette_id.is_none());
    }

    #[test]
    fn from_legacy_floyd_steinberg() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg },
            4,
        );
        assert!(matches!(params.mode, DitherModeV2::FloydSteinberg));
        assert_eq!(params.levels, 16); // 2^4
    }

    #[test]
    fn from_legacy_atkinson() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Atkinson },
            2,
        );
        assert!(matches!(params.mode, DitherModeV2::Atkinson));
        assert_eq!(params.levels, 4); // 2^2
    }

    #[test]
    fn from_legacy_jjn_maps_to_jjn() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::JarvisJudiceNinke },
            3,
        );
        assert!(matches!(params.mode, DitherModeV2::JarvisJudiceNinke));
        assert_eq!(params.levels, 8); // 2^3
    }

    #[test]
    fn from_legacy_stucki_maps_to_stucki() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Stucki },
            6,
        );
        assert!(matches!(params.mode, DitherModeV2::Stucki));
        assert_eq!(params.levels, 64); // 2^6
    }

    #[test]
    fn from_legacy_burkes_and_sierra() {
        let burkes = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Burkes },
            2,
        );
        assert!(matches!(burkes.mode, DitherModeV2::Burkes));
        let sierra = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Sierra },
            2,
        );
        assert!(matches!(sierra.mode, DitherModeV2::Sierra));
    }

    #[test]
    fn from_legacy_threshold_map() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ThresholdMap { path: "/path/to/map.png".to_string() },
            2,
        );
        assert!(matches!(params.mode, DitherModeV2::CustomPng { .. }));
        if let DitherModeV2::CustomPng { path } = &params.mode {
            assert_eq!(path, "/path/to/map.png");
        }
        assert_eq!(params.levels, 4); // 2^2
    }

    #[test]
    fn from_legacy_color_depth_boundary() {
        // color_depth=1 → levels=2
        let params = DitherParamsV2::from_legacy(DitherMode::Bayer { matrix_size: 4 }, 1);
        assert_eq!(params.levels, 2);

        // color_depth=8 → levels=256
        let params = DitherParamsV2::from_legacy(DitherMode::Bayer { matrix_size: 4 }, 8);
        assert_eq!(params.levels, 256);
    }

    #[test]
    fn from_legacy_color_depth_overflow_capped() {
        // color_depth > 8 is capped at 8, so levels = 2^8 = 256
        let params = DitherParamsV2::from_legacy(DitherMode::Bayer { matrix_size: 4 }, 10);
        assert_eq!(params.levels, 256);
    }

    #[test]
    fn from_legacy_produces_valid_params() {
        // Every legacy conversion should produce params that pass validation
        let test_cases = vec![
            (DitherMode::Bayer { matrix_size: 2 }, 1u8),
            (DitherMode::Bayer { matrix_size: 4 }, 4u8),
            (DitherMode::Bayer { matrix_size: 8 }, 8u8),
            (DitherMode::ThresholdMap { path: "/test.png".to_string() }, 3u8),
            (DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg }, 5u8),
            (DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Atkinson }, 2u8),
        ];
        for (mode, depth) in test_cases {
            let params = DitherParamsV2::from((mode, depth));
            assert!(params.validate().is_ok(), "Converted params should be valid");
        }
    }

    // ─── Track H: threshold_bias / pattern_angle ─────────────────────────

    #[test]
    fn missing_bias_and_angle_fields_deserialize_as_zero() {
        let params: DitherParamsV2 = serde_json::from_str(
            r#"{"mode":"bayer_4x4","levels":4}"#,
        )
        .unwrap();
        assert_eq!(params.threshold_bias, 0.0);
        assert_eq!(params.pattern_angle, 0.0);
        assert!(params.validate().is_ok());
    }

    #[test]
    fn threshold_bias_range_validation() {
        let mut params = DitherParamsV2::default();
        params.threshold_bias = -0.5;
        assert!(params.validate().is_ok());
        params.threshold_bias = 0.5;
        assert!(params.validate().is_ok());
        params.threshold_bias = -0.51;
        assert!(params.validate().is_err());
        params.threshold_bias = 0.51;
        assert!(params.validate().is_err());
        params.threshold_bias = f32::NAN;
        assert!(params.validate().is_err());
    }

    #[test]
    fn pattern_angle_rejects_non_finite() {
        let mut params = DitherParamsV2::default();
        params.pattern_angle = 720.0;
        assert!(params.validate().is_ok());
        params.pattern_angle = f32::INFINITY;
        assert!(params.validate().is_err());
        params.pattern_angle = f32::NAN;
        assert!(params.validate().is_err());
    }
}

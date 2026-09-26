//! Filter instance model and application.

use crate::error::EngineError;
use crate::filters::curves::CurveChannel;
use crate::filters::glitch::GlitchType;
use crate::types::{BlendMode, FilterInstanceId, PaletteId};
use serde::{Deserialize, Serialize};

fn default_channel_enabled() -> bool {
    true
}

fn default_filter_opacity() -> f32 {
    1.0
}

/// Filter kind enumeration.
///
/// Legacy serde alias — use AlgorithmId for new code. Kept for
/// `FilterInstanceFile.kind` backwards compatibility (task 4.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterKind {
    Curves,
    Levels,
    Dither,
    PaletteQuantize,
    Glitch,
    Glow,
    Crt,
    Adjust,
    /// Text-art / ASCII (not a dither mode).
    Ascii,
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
            FilterKind::Adjust => write!(f, "Adjust"),
            FilterKind::Ascii => write!(f, "Ascii"),
            FilterKind::Placeholder => write!(f, "Placeholder"),
        }
    }
}

impl FilterKind {
    /// Tone/correction ops. Newly added instances go at the bottom of the
    /// Layers list (end of `filters`) so they run before stylize filters.
    pub fn inserts_under_stylize(self) -> bool {
        matches!(self, Self::Adjust | Self::Curves | Self::Levels)
    }
}

/// Dither modes for the Dither filter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DitherMode {
    /// Bayer ordered dithering with specified matrix size (2, 4, 8, or 16).
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
    /// Zhigang Fan (1993) FS derivative (divisor 16).
    Fan93,
    Sierra,
    /// Frankie Sierra two-row filter (divisor 16).
    SierraTwoRow,
    /// Sierra Lite / Sierra-2-4A (divisor 4).
    SierraLite,
    /// Shiau–Fan (SPIE 1996 / US5353127 preferred, divisor 16). Cell reach 3.
    ShiauFan,
    /// Stevenson–Arce hexagonal filter (divisor 200). Cell reach 3.
    StevensonArce,
    /// Ostromoukhov (SIGGRAPH 2001) variable-coefficient 3-tap. Cell reach 1.
    Ostromoukhov,
    /// Zhou–Fang (SIGGRAPH 2003): Ostromoukhov weights + threshold modulation.
    ZhouFang,
}

impl DiffusionKernel {
    /// Standard published (dx, dy, weight) tables. Most kernels reach at most
    /// 2 (JJN / Atkinson / Sierra); Stevenson–Arce reaches 3. Cross-tile
    /// residual margin is `pixel_size × max_offset()` because hops are
    /// `dx × pixel_size`.
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
            // Fan (1993): * 7 / 1 3 5  (÷16). Sum = 1.0. FS with left-shifted bottom.
            // Sources: ionathanch Error-Diffusion-Dither-Kernels; caca study part3.
            Self::Fan93 => &[
                (1, 0, 7.0 / 16.0),
                (-1, 1, 1.0 / 16.0),
                (0, 1, 3.0 / 16.0),
                (1, 1, 5.0 / 16.0),
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
            // Sierra2 / two-row: * 4 3 / 1 2 3 2 1  (÷16). Sum = 1.0.
            // Sources: bisqwit error_diffusion.txt; ImageSharp Sierra2; coolbutuseless/dithr.
            Self::SierraTwoRow => &[
                (1, 0, 4.0 / 16.0),
                (2, 0, 3.0 / 16.0),
                (-2, 1, 1.0 / 16.0),
                (-1, 1, 2.0 / 16.0),
                (0, 1, 3.0 / 16.0),
                (1, 1, 2.0 / 16.0),
                (2, 1, 1.0 / 16.0),
            ],
            // Sierra Lite / 2-4A: * 2 / 1 1  (÷4). Sum = 1.0.
            // Sources: bisqwit error_diffusion.txt; ImageSharp SierraLite.
            Self::SierraLite => &[(1, 0, 2.0 / 4.0), (-1, 1, 1.0 / 4.0), (0, 1, 1.0 / 4.0)],
            // Shiau–Fan (preferred / ShiauFan2): * 8 / 1 1 2 4  (÷16). Sum = 1.0.
            // Sources: US5353127; DitherPunk SHIAU_FAN_2; libpipi shiaufan2;
            // ionathanch Error-Diffusion-Dither-Kernels wiki.
            Self::ShiauFan => &[
                (1, 0, 8.0 / 16.0),
                (-3, 1, 1.0 / 16.0),
                (-2, 1, 1.0 / 16.0),
                (-1, 1, 2.0 / 16.0),
                (0, 1, 4.0 / 16.0),
            ],
            // Stevenson–Arce (hexagonal): * . 32 / 12 . 26 . 30 . 16 / …
            // ÷200. Sum = 1.0. Offset column = 3 in a 7-wide matrix.
            // Sources: bisqwit error_diffusion.txt; ImageSharp StevensonArce.
            Self::StevensonArce => &[
                (2, 0, 32.0 / 200.0),
                (-3, 1, 12.0 / 200.0),
                (-1, 1, 26.0 / 200.0),
                (1, 1, 30.0 / 200.0),
                (3, 1, 16.0 / 200.0),
                (-2, 2, 12.0 / 200.0),
                (0, 2, 26.0 / 200.0),
                (2, 2, 12.0 / 200.0),
                (-3, 3, 5.0 / 200.0),
                (-1, 3, 12.0 / 200.0),
                (1, 3, 12.0 / 200.0),
                (3, 3, 5.0 / 200.0),
            ],
            // Positions for margin; live weights from [`crate::filters::ostromoukhov_table`].
            Self::Ostromoukhov => &[(1, 0, 13.0 / 18.0), (-1, 1, 0.0), (0, 1, 5.0 / 18.0)],
            Self::ZhouFang => &[(1, 0, 13.0 / 18.0), (-1, 1, 0.0), (0, 1, 5.0 / 18.0)],
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
            "Fan93" => Some(Self::Fan93),
            "Sierra" => Some(Self::Sierra),
            "SierraTwoRow" => Some(Self::SierraTwoRow),
            "SierraLite" => Some(Self::SierraLite),
            "ShiauFan" => Some(Self::ShiauFan),
            "StevensonArce" => Some(Self::StevensonArce),
            "Ostromoukhov" => Some(Self::Ostromoukhov),
            "ZhouFang" => Some(Self::ZhouFang),
            _ => None,
        }
    }

    /// Max |dx| / |dy| in kernel cells (FS / Ostromoukhov = 1, Atkinson / JJN /
    /// … = 2, Shiau–Fan / Stevenson–Arce = 3).
    pub fn max_offset(self) -> usize {
        self.offsets()
            .iter()
            .map(|&(dx, dy, _)| dx.unsigned_abs().max(dy.unsigned_abs()) as usize)
            .max()
            .unwrap_or(1)
    }

    /// Cross-tile residual columns/rows: `pixel_size × max_offset()`.
    pub fn edge_margin(self, pixel_size: u32) -> usize {
        pixel_size.max(1) as usize * self.max_offset()
    }
}

// ─── Dither V2 types (redesign) ───────────────────────────────────────────────

/// Redesigned dither mode with full parameter set.
///
/// Legacy serde alias — use AlgorithmId for new code. Kept as the
/// `DitherParamsV2.mode` field (task 4.4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DitherModeV2 {
    #[serde(rename = "bayer_2x2")]
    Bayer2x2,
    #[serde(rename = "bayer_4x4")]
    Bayer4x4,
    #[serde(rename = "bayer_8x8")]
    Bayer8x8,
    #[serde(rename = "bayer_16x16")]
    Bayer16x16,
    /// Classical clustered-dot (newspaper-style) ordered dither, 8×8 diagonal.
    #[serde(rename = "clustered_dot_ordered")]
    ClusteredDotOrdered,
    /// Ulichney dispersed-dot ordered dither (16×16, non-Bayer).
    #[serde(rename = "dispersed_dot_ordered")]
    DispersedDotOrdered,
    CustomPng {
        path: String,
    },
    FloydSteinberg,
    Atkinson,
    JarvisJudiceNinke,
    Stucki,
    Burkes,
    #[serde(rename = "fan93")]
    Fan93,
    Sierra,
    #[serde(rename = "sierra_two_row")]
    SierraTwoRow,
    #[serde(rename = "sierra_lite")]
    SierraLite,
    #[serde(rename = "shiau_fan")]
    ShiauFan,
    #[serde(rename = "stevenson_arce")]
    StevensonArce,
    #[serde(rename = "ostromoukhov")]
    Ostromoukhov,
    #[serde(rename = "zhou_fang")]
    ZhouFang,
    /// Hilbert-curve error diffusion (Riemersma). Full-document scope — not
    /// row-major ED; see `ExecutionScope::FullDocument`.
    #[serde(rename = "riemersma")]
    Riemersma,
    /// Ulichney void-and-cluster blue-noise ordered dither (64×64 rank matrix).
    #[serde(rename = "void_and_cluster")]
    VoidAndCluster,
    /// Engraving-style multi-layer hatch dither.
    #[serde(rename = "crosshatch_dither")]
    CrosshatchDither,
    /// Parallel line-screen halftone (stripe width tracks tone).
    #[serde(rename = "line_screen")]
    LineScreen,
    /// Jittered-lattice Voronoi stipple (density ∝ tone).
    #[serde(rename = "voronoi_stipple")]
    VoronoiStipple,
    /// Random / Bernoulli dot stipple (density ∝ tone).
    #[serde(rename = "random_dot_stipple")]
    RandomDotStipple,
    /// CMYK angled-screen halftone (ordered path, no ED).
    CmykHalftone,
    /// CMYK screens with user-rotatable base angle (`pattern_angle` offset).
    #[serde(rename = "halftone_screen_angled")]
    HalftoneScreenAngled,
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
                | Self::Fan93
                | Self::Sierra
                | Self::SierraTwoRow
                | Self::SierraLite
                | Self::ShiauFan
                | Self::StevensonArce
                | Self::Ostromoukhov
                | Self::ZhouFang
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
            Self::Fan93 => Some(DiffusionKernel::Fan93),
            Self::Sierra => Some(DiffusionKernel::Sierra),
            Self::SierraTwoRow => Some(DiffusionKernel::SierraTwoRow),
            Self::SierraLite => Some(DiffusionKernel::SierraLite),
            Self::ShiauFan => Some(DiffusionKernel::ShiauFan),
            Self::StevensonArce => Some(DiffusionKernel::StevensonArce),
            Self::Ostromoukhov => Some(DiffusionKernel::Ostromoukhov),
            Self::ZhouFang => Some(DiffusionKernel::ZhouFang),
            _ => None,
        }
    }

    /// Stable registry id for this mode, if the mode is a built-in algorithm.
    /// `CustomPng` is file-backed and is not in `ALGORITHM_ID_REGISTRY.txt`.
    pub fn algorithm_id(&self) -> Option<&'static str> {
        match self {
            Self::Bayer2x2 => Some("bayer_2x2"),
            Self::Bayer4x4 => Some("bayer_4x4"),
            Self::Bayer8x8 => Some("bayer_8x8"),
            Self::Bayer16x16 => Some("bayer_16x16"),
            Self::ClusteredDotOrdered => Some("clustered_dot_ordered"),
            Self::DispersedDotOrdered => Some("dispersed_dot_ordered"),
            Self::FloydSteinberg => Some("floyd_steinberg"),
            Self::Atkinson => Some("atkinson"),
            Self::JarvisJudiceNinke => Some("jarvis_judice_ninke"),
            Self::Stucki => Some("stucki"),
            Self::Burkes => Some("burkes"),
            Self::Fan93 => Some("fan93"),
            Self::Sierra => Some("sierra"),
            Self::SierraLite => Some("sierra_lite"),
            Self::SierraTwoRow => Some("sierra_two_row"),
            Self::ShiauFan => Some("shiau_fan"),
            Self::StevensonArce => Some("stevenson_arce"),
            Self::Ostromoukhov => Some("ostromoukhov"),
            Self::ZhouFang => Some("zhou_fang"),
            Self::Riemersma => Some("riemersma"),
            Self::VoidAndCluster => Some("void_and_cluster"),
            Self::CrosshatchDither => Some("crosshatch_dither"),
            Self::LineScreen => Some("line_screen"),
            Self::VoronoiStipple => Some("voronoi_stipple"),
            Self::RandomDotStipple => Some("random_dot_stipple"),
            Self::CmykHalftone => Some("cmyk_halftone"),
            Self::HalftoneScreenAngled => Some("halftone_screen_angled"),
            Self::Wave => Some("wave"),
            Self::CustomPng { .. } => None,
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

/// How a bound palette constrains dither (Track Q).
///
/// `Strict` (default) picks exact palette colors via two-nearest Oklab.
/// `Guided` quantizes each RGB channel in the palette's min/max range;
/// output need not match a palette entry.
/// `Mixed` Guided-quantizes each channel, then applies the same two-nearest
/// Oklab dither as Strict to that guided RGB (exact swatches, calmer steps).
/// `Simple` matches old Dither Yuki (`findClosestColor` in sRGB bytes):
/// Bayer adds `(T-0.5)*threshold_scale*64` then nearest; ED residual is
/// `(old−new)*threshold_scale` in sRGB. Palettes are document swatches.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaletteDitherMode {
    #[default]
    Strict,
    Guided {
        #[serde(default)]
        channel_levels: Option<u8>,
    },
    Mixed {
        #[serde(default)]
        channel_levels: Option<u8>,
    },
    Simple,
}

impl PaletteDitherMode {
    pub fn is_guided(self) -> bool {
        matches!(self, Self::Guided { .. } | Self::Mixed { .. })
    }

    pub fn channel_levels(self) -> Option<u8> {
        match self {
            Self::Guided { channel_levels } | Self::Mixed { channel_levels } => channel_levels,
            Self::Strict | Self::Simple => None,
        }
    }
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
    /// Strict (exact palette colors) vs Guided (per-channel range). Default Strict.
    /// Ignored when `palette_id` is `None`. Missing JSON → Strict.
    #[serde(default)]
    pub palette_dither_mode: PaletteDitherMode,
    /// When true (Strict/Mixed only), two-nearest palette pick uses Oklab `L`
    /// alone instead of full Oklab distance. Default false; missing JSON → false.
    #[serde(default)]
    pub match_by_brightness: bool,
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
    /// Applied after `aligned(pixel_size)` (Block_Then_Rotate), then Bayer/CustomPng
    /// index the map in block units (`div_euclid(pixel_size)`). Periodic via `rem_euclid(360)`.
    #[serde(default)]
    pub pattern_angle: f32,
    /// Serpentine scanning for error-diffusion modes (default false = L→R identity).
    /// Odd **global** rows run R→L with the kernel mirrored in X.
    #[serde(default)]
    pub serpentine: bool,
    /// When true, alpha is sampled per `pixel_size` block and quantized to 0/1
    /// (pixel-art silhouette on transparent PNGs). When false, source alpha is
    /// copied per pixel. Default true; missing JSON field deserializes as true.
    #[serde(default = "default_dither_alpha")]
    pub dither_alpha: bool,
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

fn default_dither_alpha() -> bool {
    true
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
            palette_dither_mode: PaletteDitherMode::Strict,
            match_by_brightness: false,
            halftone_cell_size: default_halftone_cell_size(),
            wave_wavelength: default_wave_wavelength(),
            wave_amplitude: default_wave_amplitude(),
            wave_phase: 0.0,
            wave_angle: 0.0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
            serpentine: false,
            dither_alpha: true,
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
            DitherMode::Bayer { matrix_size: 16 } => DitherModeV2::Bayer16x16,
            DitherMode::Bayer { .. } => DitherModeV2::Bayer4x4, // fallback
            DitherMode::ThresholdMap { path } => DitherModeV2::CustomPng { path },
            DitherMode::ErrorDiffusion { kernel } => match kernel {
                DiffusionKernel::FloydSteinberg => DitherModeV2::FloydSteinberg,
                DiffusionKernel::Atkinson => DitherModeV2::Atkinson,
                DiffusionKernel::JarvisJudiceNinke => DitherModeV2::JarvisJudiceNinke,
                DiffusionKernel::Stucki => DitherModeV2::Stucki,
                DiffusionKernel::Burkes => DitherModeV2::Burkes,
                DiffusionKernel::Fan93 => DitherModeV2::Fan93,
                DiffusionKernel::Sierra => DitherModeV2::Sierra,
                DiffusionKernel::SierraTwoRow => DitherModeV2::SierraTwoRow,
                DiffusionKernel::SierraLite => DitherModeV2::SierraLite,
                DiffusionKernel::ShiauFan => DitherModeV2::ShiauFan,
                DiffusionKernel::StevensonArce => DitherModeV2::StevensonArce,
                DiffusionKernel::Ostromoukhov => DitherModeV2::Ostromoukhov,
                DiffusionKernel::ZhouFang => DitherModeV2::ZhouFang,
            },
        };
        DitherParamsV2 {
            mode: new_mode,
            levels,
            threshold_scale: 1.0,
            pixel_size: 1,
            color_mode: DitherColorMode::Rgb,
            palette_id: None,
            palette_dither_mode: PaletteDitherMode::Strict,
            match_by_brightness: false,
            halftone_cell_size: default_halftone_cell_size(),
            wave_wavelength: default_wave_wavelength(),
            wave_amplitude: default_wave_amplitude(),
            wave_phase: 0.0,
            wave_angle: 0.0,
            threshold_bias: 0.0,
            pattern_angle: 0.0,
            serpentine: false,
            dither_alpha: true,
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
        if matches!(
            self.mode,
            DitherModeV2::CmykHalftone | DitherModeV2::HalftoneScreenAngled
        ) && !(2..=64).contains(&self.halftone_cell_size)
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
        if let Some(n) = self.palette_dither_mode.channel_levels() {
            if !(2..=16).contains(&n) {
                return Err(EngineError::invalid_filter_params(
                    "channel_levels must be in range [2, 16]",
                ));
            }
        }
        Ok(())
    }

    /// Copy or binary-dither alpha.
    ///
    /// Fully transparent (`<= 0`) and fully opaque (`>= 1`) stay 0/1 so solid
    /// pixels are never eaten by threshold scale. Soft edges compare `src_a`
    /// against `threshold` (ordered T, or `0.5` for error diffusion / CMYK).
    #[inline]
    pub fn map_alpha(&self, src_a: f32, threshold: f32) -> f32 {
        if !self.dither_alpha {
            return src_a;
        }
        if src_a <= 0.0 {
            0.0
        } else if src_a >= 1.0 || src_a > threshold {
            1.0
        } else {
            0.0
        }
    }

    /// RGB + binary alpha. Transparent output is cleared so zoom/PNG cannot
    /// show the pre-dither (Adjust) fringe as a contour.
    #[inline]
    pub fn dithered_rgba(&self, r: f32, g: f32, b: f32, src_a: f32, threshold: f32) -> [f32; 4] {
        let a = self.map_alpha(src_a, threshold);
        if self.dither_alpha && a <= 0.0 {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [r, g, b, a]
        }
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
        /// When false, that channel is forced to 0. Missing in old files → on.
        #[serde(default = "default_channel_enabled")]
        channel_r: bool,
        #[serde(default = "default_channel_enabled")]
        channel_g: bool,
        #[serde(default = "default_channel_enabled")]
        channel_b: bool,
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
    /// Contrast / brightness / saturation / blur / sharpness / noise.
    Adjust {
        /// Contrast around mid-gray (−1 .. 1). 0 = identity.
        contrast: f32,
        /// Additive brightness (−1 .. 1). 0 = identity.
        brightness: f32,
        /// Saturation (−1 .. 1). −1 = grayscale, 0 = identity.
        saturation: f32,
        /// Blur amount (0 .. 2). Mapped to an in-tile pixel radius in apply.
        blur: f32,
        /// Unsharp-mask amount (0 .. 2). 0 = skip.
        sharpness: f32,
        /// Deterministic RGB noise amount (0 .. 1). 0 = skip.
        noise: f32,
    },
    /// Text-art / ASCII output stage (`engine-ascii`). Not a dither mode.
    Ascii(AsciiParams),
    /// Placeholder for unknown or future filters.
    ///
    /// Old JSON `{"Placeholder": "x"}` still deserialises (`raw_params: None`).
    Placeholder(PlaceholderParams),
}

/// Parameters for [`FilterParams::Ascii`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AsciiParams {
    /// Bundled font id: `ibm_plex_mono` | `departure_mono`.
    #[serde(default = "default_ascii_font")]
    pub font: String,
    /// `px` or `columns`.
    #[serde(default = "default_ascii_size_mode")]
    pub size_mode: String,
    /// Font size in px when `size_mode == "px"`.
    #[serde(default = "default_ascii_font_px")]
    pub font_px: f32,
    /// Column count when `size_mode == "columns"`.
    #[serde(default = "default_ascii_columns")]
    pub columns: u32,
    #[serde(default = "default_false")]
    pub antialias: bool,
    #[serde(default = "default_false")]
    pub hinting: bool,
    /// Symbol set id (e.g. `bourke_10`, `printable_ascii`, `blocks`).
    #[serde(default = "default_ascii_symbol_set")]
    pub symbol_set: String,
    /// `tone` | `shape` | `shape_contrast` | `mask_two_color`.
    #[serde(default = "default_ascii_match_mode")]
    pub match_mode: String,
    /// Contrast gamma for shape_contrast (1.0 = identity).
    #[serde(default = "default_ascii_contrast")]
    pub contrast: f32,
    /// `mono` | `fg` | `fg_bg`.
    #[serde(default = "default_ascii_color_mode")]
    pub color_mode: String,
    /// `truecolor` | `xterm256` | `ansi16_vga` | `ansi16_xterm` | `ansi16_win10`.
    #[serde(default = "default_ascii_color_target")]
    pub color_target: String,
    /// `none` | `bayer2` | `bayer4` | `bayer8` | `floyd_steinberg`.
    #[serde(default = "default_ascii_cell_dither")]
    pub cell_dither: String,
    #[serde(default = "default_false")]
    pub serpentine: bool,
    #[serde(default = "default_false")]
    pub edge_overlay: bool,
    /// Edge strength threshold 0..=255 when `edge_overlay` is on.
    #[serde(default = "default_ascii_edge_tau")]
    pub edge_tau: u8,
}

fn default_ascii_font() -> String {
    "departure_mono".into()
}
fn default_ascii_size_mode() -> String {
    "px".into()
}
fn default_ascii_font_px() -> f32 {
    11.0
}
fn default_ascii_columns() -> u32 {
    120
}
fn default_false() -> bool {
    false
}
fn default_ascii_symbol_set() -> String {
    "bourke_70".into()
}
fn default_ascii_match_mode() -> String {
    "shape".into()
}
fn default_ascii_contrast() -> f32 {
    1.0
}
fn default_ascii_color_mode() -> String {
    "mono".into()
}
fn default_ascii_color_target() -> String {
    "truecolor".into()
}
fn default_ascii_cell_dither() -> String {
    "none".into()
}
fn default_ascii_edge_tau() -> u8 {
    40
}

impl Default for AsciiParams {
    fn default() -> Self {
        Self {
            font: default_ascii_font(),
            size_mode: default_ascii_size_mode(),
            font_px: default_ascii_font_px(),
            columns: default_ascii_columns(),
            antialias: false,
            hinting: false,
            symbol_set: default_ascii_symbol_set(),
            match_mode: default_ascii_match_mode(),
            contrast: default_ascii_contrast(),
            color_mode: default_ascii_color_mode(),
            color_target: default_ascii_color_target(),
            cell_dither: default_ascii_cell_dither(),
            serpentine: false,
            edge_overlay: false,
            edge_tau: default_ascii_edge_tau(),
        }
    }
}

impl AsciiParams {
    pub fn validate(&self) -> Result<(), EngineError> {
        match self.font.as_str() {
            "ibm_plex_mono" | "departure_mono" => {}
            other => {
                return Err(EngineError::invalid_filter_params(format!(
                    "ascii font must be ibm_plex_mono or departure_mono, got {other}"
                )));
            }
        }
        match self.size_mode.as_str() {
            "px" | "columns" => {}
            other => {
                return Err(EngineError::invalid_filter_params(format!(
                    "ascii size_mode must be px or columns, got {other}"
                )));
            }
        }
        if !(4.0..=256.0).contains(&self.font_px) {
            return Err(EngineError::invalid_filter_params(
                "ascii font_px must be in [4, 256]",
            ));
        }
        if !(1..=1024).contains(&self.columns) {
            return Err(EngineError::invalid_filter_params(
                "ascii columns must be in [1, 1024]",
            ));
        }
        const SETS: &[&str] = &[
            "bourke_10",
            "bourke_70",
            "printable_ascii",
            "ascii_box_drawing",
            "blocks",
            "quadrants",
            "sextants",
            "octants",
            "braille",
            "cp437",
        ];
        if !SETS.contains(&self.symbol_set.as_str()) {
            return Err(EngineError::invalid_filter_params(format!(
                "unknown ascii symbol_set: {}",
                self.symbol_set
            )));
        }
        match self.match_mode.as_str() {
            "tone" | "shape" | "shape_contrast" | "mask_two_color" => {}
            other => {
                return Err(EngineError::invalid_filter_params(format!(
                    "ascii match_mode invalid: {other}"
                )));
            }
        }
        if !(0.25..=4.0).contains(&self.contrast) {
            return Err(EngineError::invalid_filter_params(
                "ascii contrast must be in [0.25, 4]",
            ));
        }
        match self.color_mode.as_str() {
            "mono" | "fg" | "fg_bg" => {}
            other => {
                return Err(EngineError::invalid_filter_params(format!(
                    "ascii color_mode invalid: {other}"
                )));
            }
        }
        match self.color_target.as_str() {
            "truecolor" | "xterm256" | "ansi16" | "ansi16_vga" | "ansi16_xterm"
            | "ansi16_win10" => {}
            other => {
                return Err(EngineError::invalid_filter_params(format!(
                    "ascii color_target invalid: {other}"
                )));
            }
        }
        match self.cell_dither.as_str() {
            "none" | "bayer2" | "bayer4" | "bayer8" | "floyd_steinberg" => {}
            other => {
                return Err(EngineError::invalid_filter_params(format!(
                    "ascii cell_dither invalid: {other}"
                )));
            }
        }
        Ok(())
    }
}

/// Wire format for [`FilterParams::Placeholder`].
///
/// **Not dead code.** Track E Registry owns known algorithms; Placeholder remains
/// the intentional unknown-kind / forward-compat arm:
/// - `filter_service` maps unrecognized `FilterKind` strings here
/// - `serialize` / `id_remap` preserve unknown filters across load/save
/// - `apply` no-ops Placeholder rather than failing the tile pipeline
///
/// Do not delete until product policy is "reject unknown filters on load"
/// (would break older `.dyproj` / `.dyuki` with experimental kinds).
///
/// Accepts a legacy JSON string or the v2 `{ label, raw_params }` object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "PlaceholderDe", into = "PlaceholderDe")]
pub struct PlaceholderParams {
    pub label: String,
    pub raw_params: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum PlaceholderDe {
    Legacy(String),
    Record {
        label: String,
        #[serde(default)]
        raw_params: Option<serde_json::Value>,
    },
}

impl From<PlaceholderDe> for PlaceholderParams {
    fn from(value: PlaceholderDe) -> Self {
        match value {
            PlaceholderDe::Legacy(label) => Self {
                label,
                raw_params: None,
            },
            PlaceholderDe::Record { label, raw_params } => Self { label, raw_params },
        }
    }
}

impl From<PlaceholderParams> for PlaceholderDe {
    fn from(value: PlaceholderParams) -> Self {
        PlaceholderDe::Record {
            label: value.label,
            raw_params: value.raw_params,
        }
    }
}

impl Default for FilterParams {
    fn default() -> Self {
        FilterParams::Placeholder(PlaceholderParams {
            label: "default".to_string(),
            raw_params: None,
        })
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

    /// Stable algorithm identity when this instance is dispatched via the
    /// Registry. `None` keeps the legacy `match &filter.params` path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm_id: Option<String>,

    /// Parameter schema version written next to `algorithm_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<u32>,
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
            algorithm_id: None,
            schema_version: None,
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
                channel_r: _,
                channel_g: _,
                channel_b: _,
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
                        if !matches!(matrix_size, 2 | 4 | 8 | 16) {
                            return Err(EngineError::invalid_filter_params(
                                "Bayer matrix_size must be 2, 4, 8, or 16",
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
            FilterParams::Adjust {
                contrast,
                brightness,
                saturation,
                blur,
                sharpness,
                noise,
            } => {
                if !(-1.0..=1.0).contains(contrast) {
                    return Err(EngineError::invalid_filter_params(
                        "Adjust contrast must be in range [-1.0, 1.0]",
                    ));
                }
                if !(-1.0..=1.0).contains(brightness) {
                    return Err(EngineError::invalid_filter_params(
                        "Adjust brightness must be in range [-1.0, 1.0]",
                    ));
                }
                if !(-1.0..=1.0).contains(saturation) {
                    return Err(EngineError::invalid_filter_params(
                        "Adjust saturation must be in range [-1.0, 1.0]",
                    ));
                }
                if !(0.0..=2.0).contains(blur) {
                    return Err(EngineError::invalid_filter_params(
                        "Adjust blur must be in range [0.0, 2.0] (HALO cap)",
                    ));
                }
                if !(0.0..=2.0).contains(sharpness) {
                    return Err(EngineError::invalid_filter_params(
                        "Adjust sharpness must be in range [0.0, 2.0]",
                    ));
                }
                if !(0.0..=1.0).contains(noise) {
                    return Err(EngineError::invalid_filter_params(
                        "Adjust noise must be in range [0.0, 1.0]",
                    ));
                }
                Ok(())
            }
            FilterParams::Ascii(p) => p.validate(),
            FilterParams::Placeholder(_) => Ok(()),
        }
    }
}

/// Infer a registry id from persisted params when `FilterInstance.algorithm_id` is unset.
pub fn algorithm_id_for_params(params: &FilterParams) -> Option<&'static str> {
    match params {
        FilterParams::DitherV2(p) => p.mode.algorithm_id(),
        FilterParams::Dither { mode, color_depth } => {
            DitherParamsV2::from((mode.clone(), *color_depth))
                .mode
                .algorithm_id()
        }
        FilterParams::PaletteQuantize { .. } => Some("palette_quantize"),
        FilterParams::Crt { .. } => Some("crt"),
        FilterParams::Glow { .. } => Some("glow"),
        FilterParams::Adjust { .. } => Some("adjust"),
        FilterParams::Curves { .. } => Some("curves"),
        FilterParams::Glitch { .. } => Some("glitch"),
        FilterParams::Ascii(_) => Some("ascii"),
        FilterParams::Levels { .. } | FilterParams::Placeholder(_) => None,
    }
}

/// Map a built-in `AlgorithmId` to the serde `FilterKind` alias.
pub fn filter_kind_for_algorithm_id(id: &str) -> Option<FilterKind> {
    match id {
        "bayer_2x2"
        | "bayer_4x4"
        | "bayer_8x8"
        | "bayer_16x16"
        | "clustered_dot_ordered"
        | "dispersed_dot_ordered"
        | "floyd_steinberg"
        | "atkinson"
        | "jarvis_judice_ninke"
        | "stucki"
        | "burkes"
        | "fan93"
        | "sierra"
        | "sierra_lite"
        | "sierra_two_row"
        | "shiau_fan"
        | "stevenson_arce"
        | "ostromoukhov"
        | "zhou_fang"
        | "riemersma"
        | "void_and_cluster"
        | "crosshatch_dither"
        | "line_screen"
        | "voronoi_stipple"
        | "random_dot_stipple"
        | "cmyk_halftone"
        | "halftone_screen_angled"
        | "wave" => Some(FilterKind::Dither),
        "palette_quantize" => Some(FilterKind::PaletteQuantize),
        "crt" => Some(FilterKind::Crt),
        "glow" => Some(FilterKind::Glow),
        "adjust" => Some(FilterKind::Adjust),
        "curves" => Some(FilterKind::Curves),
        "glitch" => Some(FilterKind::Glitch),
        "ascii" => Some(FilterKind::Ascii),
        _ => None,
    }
}

/// Explicit `algorithm_id`, or an inference from [`FilterParams`].
///
/// For [`FilterParams::DitherV2`], **`params.mode` wins** over a stored
/// `algorithm_id`. The Effects UI edits `mode` in place; if `algorithm_id` is
/// left stale (e.g. still `floyd_steinberg` after switching to Bayer), preferring
/// the id would run ED without `requires_full_row` wavefront ordering and show
/// tile-boundary seams.
pub fn resolve_algorithm_id(filter: &FilterInstance) -> Option<String> {
    if let FilterParams::DitherV2(p) = &filter.params {
        return p.mode.algorithm_id().map(str::to_string);
    }
    if let Some(id) = filter.algorithm_id.as_deref() {
        return Some(id.to_string());
    }
    algorithm_id_for_params(&filter.params).map(str::to_string)
}

/// Serialise params to the JSON object `FilterAlgorithm::apply` expects (inner, not tagged).
pub fn filter_params_to_json(params: &FilterParams) -> Result<serde_json::Value, EngineError> {
    match params {
        FilterParams::DitherV2(p) => serde_json::to_value(p),
        FilterParams::Dither { mode, color_depth } => {
            serde_json::to_value(DitherParamsV2::from((mode.clone(), *color_depth)))
        }
        FilterParams::PaletteQuantize {
            palette_id,
            diffusion,
        } => Ok(serde_json::json!({
            "palette_id": palette_id,
            "diffusion": diffusion,
        })),
        FilterParams::Crt {
            period,
            strength,
            mask_strength,
        } => Ok(serde_json::json!({
            "period": period,
            "strength": strength,
            "mask_strength": mask_strength,
        })),
        FilterParams::Glow {
            radius,
            intensity,
            threshold,
        } => Ok(serde_json::json!({
            "radius": radius,
            "intensity": intensity,
            "threshold": threshold,
        })),
        FilterParams::Adjust {
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
        } => Ok(serde_json::json!({
            "contrast": contrast,
            "brightness": brightness,
            "saturation": saturation,
            "blur": blur,
            "sharpness": sharpness,
            "noise": noise,
        })),
        FilterParams::Curves { curve, channel } => Ok(serde_json::json!({
            "curve": curve,
            "channel": channel,
        })),
        FilterParams::Glitch {
            glitch_type,
            intensity,
            seed,
        } => Ok(serde_json::json!({
            "glitch_type": glitch_type,
            "intensity": intensity,
            "seed": seed,
        })),
        FilterParams::Ascii(p) => serde_json::to_value(p),
        other => serde_json::to_value(other),
    }
    .map_err(|e| EngineError::invalid_filter_params(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filters::curves::CurveChannel;
    use serde_json;

    #[test]
    fn placeholder_roundtrip() {
        let legacy = serde_json::json!({"Placeholder": "x"});
        let decoded: FilterParams = serde_json::from_value(legacy).unwrap();
        match decoded {
            FilterParams::Placeholder(p) => {
                assert_eq!(p.label, "x");
                assert!(p.raw_params.is_none());
            }
            other => panic!("expected Placeholder, got {other:?}"),
        }

        let full = FilterParams::Placeholder(PlaceholderParams {
            label: "y".into(),
            raw_params: Some(serde_json::json!({"custom_key": 42})),
        });
        let json = serde_json::to_value(&full).unwrap();
        let round: FilterParams = serde_json::from_value(json).unwrap();
        match round {
            FilterParams::Placeholder(p) => {
                assert_eq!(p.label, "y");
                assert_eq!(p.raw_params, Some(serde_json::json!({"custom_key": 42})));
            }
            other => panic!("expected Placeholder, got {other:?}"),
        }
    }

    #[test]
    fn filter_instance_new_is_enabled() {
        let filter = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![],
                channel: CurveChannel::All,
            },
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
                channel_r: true,
                channel_g: true,
                channel_b: true,
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
                channel_r: true,
                channel_g: true,
                channel_b: true,
            },
        );
        assert!(invalid_filter.validate().is_err());
    }

    #[test]
    fn levels_legacy_json_enables_all_channels() {
        let params: FilterParams = serde_json::from_str(
            r#"{"Levels":{"input_black":0.0,"input_white":1.0,"gamma":1.0,"output_black":0.0,"output_white":1.0}}"#,
        )
        .unwrap();
        match params {
            FilterParams::Levels {
                channel_r,
                channel_g,
                channel_b,
                ..
            } => {
                assert!(channel_r && channel_g && channel_b);
            }
            other => panic!("expected Levels, got {:?}", other),
        }
    }

    #[test]
    fn filter_validate_dither() {
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::ErrorDiffusion {
                    kernel: DiffusionKernel::FloydSteinberg,
                },
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
                mode: DitherMode::ThresholdMap {
                    path: String::new(),
                },
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
    fn filter_kind_display() {
        assert_eq!(FilterKind::Curves.to_string(), "Curves");
        assert_eq!(FilterKind::Levels.to_string(), "Levels");
        assert_eq!(FilterKind::Dither.to_string(), "Dither");
        assert_eq!(FilterKind::PaletteQuantize.to_string(), "PaletteQuantize");
        assert_eq!(FilterKind::Glitch.to_string(), "Glitch");
        assert_eq!(FilterKind::Glow.to_string(), "Glow");
        assert_eq!(FilterKind::Crt.to_string(), "Crt");
        assert_eq!(FilterKind::Adjust.to_string(), "Adjust");
        assert_eq!(FilterKind::Placeholder.to_string(), "Placeholder");
    }

    // ─── DitherModeV2 Serialization Tests (Requirement 11.1) ─────────────────

    #[test]
    fn dither_mode_v2_simple_variants_serialize_as_snake_case_strings() {
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Bayer2x2).unwrap(),
            serde_json::json!("bayer_2x2")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Bayer4x4).unwrap(),
            serde_json::json!("bayer_4x4")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Bayer8x8).unwrap(),
            serde_json::json!("bayer_8x8")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Bayer16x16).unwrap(),
            serde_json::json!("bayer_16x16")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::ClusteredDotOrdered).unwrap(),
            serde_json::json!("clustered_dot_ordered")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::DispersedDotOrdered).unwrap(),
            serde_json::json!("dispersed_dot_ordered")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::FloydSteinberg).unwrap(),
            serde_json::json!("floyd_steinberg")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Atkinson).unwrap(),
            serde_json::json!("atkinson")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::JarvisJudiceNinke).unwrap(),
            serde_json::json!("jarvis_judice_ninke")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Stucki).unwrap(),
            serde_json::json!("stucki")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Burkes).unwrap(),
            serde_json::json!("burkes")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Fan93).unwrap(),
            serde_json::json!("fan93")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Sierra).unwrap(),
            serde_json::json!("sierra")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::SierraLite).unwrap(),
            serde_json::json!("sierra_lite")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::ShiauFan).unwrap(),
            serde_json::json!("shiau_fan")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::StevensonArce).unwrap(),
            serde_json::json!("stevenson_arce")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Ostromoukhov).unwrap(),
            serde_json::json!("ostromoukhov")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::ZhouFang).unwrap(),
            serde_json::json!("zhou_fang")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Riemersma).unwrap(),
            serde_json::json!("riemersma")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::VoidAndCluster).unwrap(),
            serde_json::json!("void_and_cluster")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::CrosshatchDither).unwrap(),
            serde_json::json!("crosshatch_dither")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::LineScreen).unwrap(),
            serde_json::json!("line_screen")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::VoronoiStipple).unwrap(),
            serde_json::json!("voronoi_stipple")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::RandomDotStipple).unwrap(),
            serde_json::json!("random_dot_stipple")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::SierraTwoRow).unwrap(),
            serde_json::json!("sierra_two_row")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::CmykHalftone).unwrap(),
            serde_json::json!("cmyk_halftone")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::HalftoneScreenAngled).unwrap(),
            serde_json::json!("halftone_screen_angled")
        );
        assert_eq!(
            serde_json::to_value(&DitherModeV2::Wave).unwrap(),
            serde_json::json!("wave")
        );
    }

    #[test]
    fn dither_mode_v2_custom_png_serializes_as_object() {
        let mode = DitherModeV2::CustomPng {
            path: "/some/path.png".to_string(),
        };
        let value = serde_json::to_value(&mode).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"custom_png": {"path": "/some/path.png"}})
        );
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
        let mode = DitherModeV2::CustomPng {
            path: "/Users/artist/patterns/halftone.png".to_string(),
        };
        let json = serde_json::to_string(&mode).unwrap();
        let deserialized: DitherModeV2 = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&deserialized).unwrap();
        assert_eq!(json, json2);
    }

    #[test]
    fn dither_mode_v2_deserialization_from_string() {
        let mode: DitherModeV2 = serde_json::from_str(r#""bayer_2x2""#).unwrap();
        assert_eq!(
            serde_json::to_value(&mode).unwrap(),
            serde_json::json!("bayer_2x2")
        );

        let mode: DitherModeV2 = serde_json::from_str(r#""floyd_steinberg""#).unwrap();
        assert_eq!(
            serde_json::to_value(&mode).unwrap(),
            serde_json::json!("floyd_steinberg")
        );
    }

    #[test]
    fn serpentine_missing_field_defaults_false() {
        let p: DitherParamsV2 =
            serde_json::from_str(r#"{"mode":"floyd_steinberg","levels":4}"#).unwrap();
        assert!(!p.serpentine);
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["serpentine"], serde_json::json!(false));
    }

    #[test]
    fn dither_mode_v2_deserialization_from_object() {
        let mode: DitherModeV2 =
            serde_json::from_str(r#"{"custom_png": {"path": "/tmp/test.png"}}"#).unwrap();
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
        assert!(
            filter.requires_full_row,
            "FloydSteinberg should require full row processing"
        );
    }

    #[test]
    fn resolve_algorithm_id_prefers_dither_mode_over_stale_id() {
        let mut filter = FilterInstance::new(
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
        filter.algorithm_id = Some("floyd_steinberg".into());
        assert_eq!(
            resolve_algorithm_id(&filter).as_deref(),
            Some("bayer_4x4"),
            "stale ED algorithm_id must not override live Bayer mode (seam bug)"
        );
        assert!(
            !filter.requires_full_row,
            "scheduler flag must follow Bayer params, not stale id"
        );
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
        assert!(
            filter.requires_full_row,
            "Atkinson should require full row processing"
        );
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
        assert!(
            !filter.requires_full_row,
            "Bayer4x4 should NOT require full row processing"
        );
    }

    #[test]
    fn legacy_dither_error_diffusion_requires_full_row() {
        let filter = FilterInstance::new(
            FilterKind::Dither,
            FilterParams::Dither {
                mode: DitherMode::ErrorDiffusion {
                    kernel: DiffusionKernel::FloydSteinberg,
                },
                color_depth: 4,
            },
        );
        assert!(
            filter.requires_full_row,
            "Legacy error diffusion should require full row processing"
        );
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
        assert!(
            !filter.requires_full_row,
            "Legacy Bayer should NOT require full row processing"
        );
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
    fn from_trait_bayer_16() {
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 16 }, 4u8));
        assert!(matches!(params.mode, DitherModeV2::Bayer16x16));
        assert_eq!(params.levels, 16); // 2^4
    }

    #[test]
    fn from_trait_bayer_fallback() {
        // Unknown matrix size falls back to Bayer4x4
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 3 }, 4u8));
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
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::FloydSteinberg,
            },
            4,
        );
        assert!(matches!(params.mode, DitherModeV2::FloydSteinberg));
        assert_eq!(params.levels, 16); // 2^4
    }

    #[test]
    fn from_legacy_atkinson() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::Atkinson,
            },
            2,
        );
        assert!(matches!(params.mode, DitherModeV2::Atkinson));
        assert_eq!(params.levels, 4); // 2^2
    }

    #[test]
    fn from_legacy_jjn_maps_to_jjn() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::JarvisJudiceNinke,
            },
            3,
        );
        assert!(matches!(params.mode, DitherModeV2::JarvisJudiceNinke));
        assert_eq!(params.levels, 8); // 2^3
    }

    #[test]
    fn from_legacy_stucki_maps_to_stucki() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::Stucki,
            },
            6,
        );
        assert!(matches!(params.mode, DitherModeV2::Stucki));
        assert_eq!(params.levels, 64); // 2^6
    }

    #[test]
    fn from_legacy_burkes_and_sierra() {
        let burkes = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::Burkes,
            },
            2,
        );
        assert!(matches!(burkes.mode, DitherModeV2::Burkes));
        let sierra = DitherParamsV2::from_legacy(
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::Sierra,
            },
            2,
        );
        assert!(matches!(sierra.mode, DitherModeV2::Sierra));
    }

    #[test]
    fn from_legacy_threshold_map() {
        let params = DitherParamsV2::from_legacy(
            DitherMode::ThresholdMap {
                path: "/path/to/map.png".to_string(),
            },
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
            (
                DitherMode::ThresholdMap {
                    path: "/test.png".to_string(),
                },
                3u8,
            ),
            (
                DitherMode::ErrorDiffusion {
                    kernel: DiffusionKernel::FloydSteinberg,
                },
                5u8,
            ),
            (
                DitherMode::ErrorDiffusion {
                    kernel: DiffusionKernel::Atkinson,
                },
                2u8,
            ),
        ];
        for (mode, depth) in test_cases {
            let params = DitherParamsV2::from((mode, depth));
            assert!(
                params.validate().is_ok(),
                "Converted params should be valid"
            );
        }
    }

    // ─── Track H: threshold_bias / pattern_angle ─────────────────────────

    #[test]
    fn missing_bias_and_angle_fields_deserialize_as_zero() {
        let params: DitherParamsV2 =
            serde_json::from_str(r#"{"mode":"bayer_4x4","levels":4}"#).unwrap();
        assert_eq!(params.threshold_bias, 0.0);
        assert_eq!(params.pattern_angle, 0.0);
        assert!(
            params.dither_alpha,
            "missing dither_alpha deserializes as true"
        );
        assert!(params.validate().is_ok());
    }

    #[test]
    fn dither_v2_legacy_document_defaults_to_strict_palette_mode() {
        let params: DitherParamsV2 =
            serde_json::from_str(r#"{"mode":"bayer_4x4","levels":4}"#).unwrap();
        assert_eq!(params.palette_dither_mode, PaletteDitherMode::Strict);
        assert!(
            !params.match_by_brightness,
            "missing match_by_brightness deserializes as false"
        );
        let mut guided = DitherParamsV2::default();
        guided.palette_dither_mode = PaletteDitherMode::Guided {
            channel_levels: Some(1),
        };
        assert!(guided.validate().is_err());
        guided.palette_dither_mode = PaletteDitherMode::Guided {
            channel_levels: Some(3),
        };
        assert!(guided.validate().is_ok());
        let mut mixed = DitherParamsV2::default();
        mixed.palette_dither_mode = PaletteDitherMode::Mixed {
            channel_levels: Some(1),
        };
        assert!(mixed.validate().is_err());
        mixed.palette_dither_mode = PaletteDitherMode::Mixed {
            channel_levels: Some(4),
        };
        assert!(mixed.validate().is_ok());
        let simple: DitherParamsV2 = serde_json::from_str(
            r#"{"mode":"bayer_4x4","levels":4,"palette_dither_mode":"simple"}"#,
        )
        .unwrap();
        assert_eq!(simple.palette_dither_mode, PaletteDitherMode::Simple);
    }

    #[test]
    fn map_alpha_binary_keeps_solid_and_thresholds_soft() {
        let mut params = DitherParamsV2::default();
        params.dither_alpha = true;
        assert_eq!(params.map_alpha(0.0, 0.25), 0.0);
        assert_eq!(params.map_alpha(1.0, 0.99), 1.0);
        assert_eq!(params.map_alpha(0.4, 0.25), 1.0);
        assert_eq!(params.map_alpha(0.4, 0.5), 0.0);
        params.dither_alpha = false;
        assert_eq!(params.map_alpha(0.42, 0.5), 0.42);
    }

    #[test]
    fn dithered_rgba_clears_rgb_when_alpha_punched() {
        let mut params = DitherParamsV2::default();
        params.dither_alpha = true;
        assert_eq!(
            params.dithered_rgba(0.8, 0.2, 0.1, 0.0, 0.5),
            [0.0, 0.0, 0.0, 0.0]
        );
        let on = params.dithered_rgba(0.8, 0.2, 0.1, 1.0, 0.5);
        assert_eq!(on, [0.8, 0.2, 0.1, 1.0]);
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

    #[test]
    fn sierra_two_row_kernel_matches_published_coefficients() {
        let offs = DiffusionKernel::SierraTwoRow.offsets();
        let expected: &[(i32, i32, f32)] = &[
            (1, 0, 4.0 / 16.0),
            (2, 0, 3.0 / 16.0),
            (-2, 1, 1.0 / 16.0),
            (-1, 1, 2.0 / 16.0),
            (0, 1, 3.0 / 16.0),
            (1, 1, 2.0 / 16.0),
            (2, 1, 1.0 / 16.0),
        ];
        assert_eq!(offs.len(), expected.len());
        let mut sum = 0.0f32;
        for (got, exp) in offs.iter().zip(expected.iter()) {
            assert_eq!(got.0, exp.0);
            assert_eq!(got.1, exp.1);
            assert!((got.2 - exp.2).abs() < 1e-6, "{} vs {}", got.2, exp.2);
            sum += got.2;
        }
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "Sierra Two-Row weights must sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn sierra_lite_kernel_matches_published_coefficients() {
        let offs = DiffusionKernel::SierraLite.offsets();
        let expected: &[(i32, i32, f32)] =
            &[(1, 0, 2.0 / 4.0), (-1, 1, 1.0 / 4.0), (0, 1, 1.0 / 4.0)];
        assert_eq!(offs.len(), expected.len());
        let mut sum = 0.0f32;
        for (got, exp) in offs.iter().zip(expected.iter()) {
            assert_eq!(got.0, exp.0);
            assert_eq!(got.1, exp.1);
            assert!((got.2 - exp.2).abs() < 1e-6, "{} vs {}", got.2, exp.2);
            sum += got.2;
        }
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "Sierra Lite weights must sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn fan93_kernel_matches_published_coefficients() {
        let offs = DiffusionKernel::Fan93.offsets();
        let expected: &[(i32, i32, f32)] = &[
            (1, 0, 7.0 / 16.0),
            (-1, 1, 1.0 / 16.0),
            (0, 1, 3.0 / 16.0),
            (1, 1, 5.0 / 16.0),
        ];
        assert_eq!(offs.len(), expected.len());
        let mut sum = 0.0f32;
        for (got, exp) in offs.iter().zip(expected.iter()) {
            assert_eq!(got.0, exp.0);
            assert_eq!(got.1, exp.1);
            assert!((got.2 - exp.2).abs() < 1e-6, "{} vs {}", got.2, exp.2);
            sum += got.2;
        }
        assert_eq!(DiffusionKernel::Fan93.max_offset(), 1);
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "Fan93 weights must sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn shiau_fan_kernel_matches_published_coefficients() {
        let offs = DiffusionKernel::ShiauFan.offsets();
        let expected: &[(i32, i32, f32)] = &[
            (1, 0, 8.0 / 16.0),
            (-3, 1, 1.0 / 16.0),
            (-2, 1, 1.0 / 16.0),
            (-1, 1, 2.0 / 16.0),
            (0, 1, 4.0 / 16.0),
        ];
        assert_eq!(offs.len(), expected.len());
        let mut sum = 0.0f32;
        for (got, exp) in offs.iter().zip(expected.iter()) {
            assert_eq!(got.0, exp.0);
            assert_eq!(got.1, exp.1);
            assert!((got.2 - exp.2).abs() < 1e-6, "{} vs {}", got.2, exp.2);
            sum += got.2;
        }
        assert_eq!(DiffusionKernel::ShiauFan.max_offset(), 3);
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "Shiau–Fan weights must sum to 1.0, got {sum}"
        );
    }

    #[test]
    fn stevenson_arce_kernel_matches_published_coefficients() {
        let offs = DiffusionKernel::StevensonArce.offsets();
        let expected: &[(i32, i32, f32)] = &[
            (2, 0, 32.0 / 200.0),
            (-3, 1, 12.0 / 200.0),
            (-1, 1, 26.0 / 200.0),
            (1, 1, 30.0 / 200.0),
            (3, 1, 16.0 / 200.0),
            (-2, 2, 12.0 / 200.0),
            (0, 2, 26.0 / 200.0),
            (2, 2, 12.0 / 200.0),
            (-3, 3, 5.0 / 200.0),
            (-1, 3, 12.0 / 200.0),
            (1, 3, 12.0 / 200.0),
            (3, 3, 5.0 / 200.0),
        ];
        assert_eq!(offs.len(), expected.len());
        let mut sum = 0.0f32;
        for (got, exp) in offs.iter().zip(expected.iter()) {
            assert_eq!(got.0, exp.0);
            assert_eq!(got.1, exp.1);
            assert!((got.2 - exp.2).abs() < 1e-6, "{} vs {}", got.2, exp.2);
            sum += got.2;
        }
        assert_eq!(DiffusionKernel::StevensonArce.max_offset(), 3);
        assert!(
            (sum - 1.0).abs() < 1e-6,
            "Stevenson–Arce weights must sum to 1.0, got {sum}"
        );
    }
}

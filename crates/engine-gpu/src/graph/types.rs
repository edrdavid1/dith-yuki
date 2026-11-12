//! Compute graph types — mirrors CPU filter stack order.

use std::sync::Arc;

/// Lightweight filter description for graph compile (no engine-project dependency).
#[derive(Clone, Debug, PartialEq)]
pub enum GraphLayerFilter {
    /// Disabled filter entry — omitted from graph.
    Skip,
    Bayer(BayerPassParams),
    /// Ulichney void-and-cluster ordered dither (static 64² matrix).
    VoidAndCluster(VoidAndClusterPassParams),
    Halftone(HalftonePassParams),
    LineScreen(LineScreenPassParams),
    Crosshatch(CrosshatchPassParams),
    Wave(WavePassParams),
    Crt(CrtPassParams),
    Glow(GlowPassParams),
    /// Premultiplied box blur + hash grain (clamp path).
    BlurNoise(BlurNoisePassParams),
    /// Clarity + unsharp (clamp path).
    Detail(DetailPassParams),
    /// Invert / Threshold / Posterize / Adjust / Levels / Curves pointwise grade.
    Pointwise(PointwisePassParams),
    /// Adobe `.cube` 3D LUT.
    CubeLut(CubeLutPassParams),
    /// Nearest-color snap via `PaletteLut3D` (no ED). Payload carries LUT + palette RGB.
    PaletteQuantize(PaletteQuantizePassParams),
    /// Ordered Bayer + per-channel Guided quantize (palette ranges).
    PaletteGuided(PaletteGuidedPassParams),
    /// Guided then two-nearest Oklab snap (two passes, scratch A→B).
    PaletteMixed(PaletteMixedPassParams),
    CpuCheckpoint(CpuCheckpointKind),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GpuPipelineKey {
    Bayer2,
    Bayer4,
    Bayer8,
    Bayer16,
    ClusteredDot,
    DispersedDot,
    VoidAndCluster,
    Halftone,
    LineScreen,
    Crosshatch,
    Wave,
    Crt,
    Glow,
    BlurNoise,
    Detail,
    Pointwise,
    CubeLut,
    PaletteQuantize,
    PaletteGuided,
    PaletteMixed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BayerPassParams {
    pub pipeline: GpuPipelineKey,
    pub levels: u16,
    pub threshold_scale: f32,
    /// 0=rgb, 1=gray, 2=rgb+dither_alpha, 3=gray+dither_alpha
    pub color_mode: u32,
    pub threshold_bias: f32,
    pub pattern_angle: f32,
}

/// Void-and-cluster pass (same quantize knobs as Bayer; matrix is fixed 64²).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VoidAndClusterPassParams {
    pub levels: u16,
    pub threshold_scale: f32,
    /// 0=rgb, 1=gray, 2=rgb+dither_alpha, 3=gray+dither_alpha
    pub color_mode: u32,
    pub threshold_bias: f32,
    pub pattern_angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalftonePassParams {
    pub cell_size: u8,
    pub threshold_scale: f32,
    pub dither_alpha: bool,
    pub grayscale: bool,
    /// Degrees added to classic CMYK plate angles (`halftone_screen_angled`).
    pub angle_offset_deg: f32,
}

/// Parallel line-screen stripes (luminance → ink width).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineScreenPassParams {
    pub cell_size: u8,
    pub threshold_scale: f32,
    pub pattern_angle: f32,
    pub dither_alpha: bool,
}

/// Engraving-style crosshatch ladder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrosshatchPassParams {
    pub spacing: f32,
    pub threshold_scale: f32,
    pub pattern_angle: f32,
    pub dither_alpha: bool,
}

/// Sinusoidal wave ordered dither (Bayer-style quantize).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WavePassParams {
    pub levels: u16,
    pub threshold_scale: f32,
    /// 0=rgb, 1=gray, 2=rgb+dither_alpha, 3=gray+dither_alpha
    pub color_mode: u32,
    pub threshold_bias: f32,
    pub wavelength: f32,
    pub amplitude: f32,
    pub phase: f32,
    pub wave_angle: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrtPassParams {
    pub period: u8,
    pub strength: f32,
    pub mask_strength: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlowPassParams {
    pub radius: f32,
    pub intensity: f32,
    pub threshold: f32,
    pub softness: u8,
}

/// Blur/Noise pass (`blur` slider 0..=2 → px; `noise` 0..=1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurNoisePassParams {
    pub blur: f32,
    pub noise: f32,
}

/// Detail pass (clarity −1..=1, sharpness 0..=2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetailPassParams {
    pub clarity: f32,
    pub sharpness: f32,
}

/// Pointwise Adjust grade params (spatial knobs must be zero on the CPU side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdjustPointwisePassParams {
    pub exposure: f32,
    pub contrast: f32,
    pub brightness: f32,
    pub saturation: f32,
    pub shadows: f32,
    pub highlights: f32,
    pub temperature: f32,
    pub tint: f32,
    pub vibrance: f32,
    pub hue: f32,
}

/// Levels remap (matches CPU `LevelsFilter::apply_to_value` + channel mask).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LevelsPassParams {
    pub input_black: f32,
    pub input_white: f32,
    pub gamma: f32,
    pub output_black: f32,
    pub output_white: f32,
    /// Bit0=R, bit1=G, bit2=B. Disabled channel → 0.
    pub channel_mask: u32,
}

/// Curves 1D LUT (4096 samples) + channel select.
#[derive(Clone, Debug)]
pub struct CurvesPassParams {
    /// 0=R, 1=G, 2=B, 3=All, 4=Luminance (CPU maps luminance→green).
    pub channel: u32,
    pub lut: Arc<[f32]>,
}

impl PartialEq for CurvesPassParams {
    fn eq(&self, other: &Self) -> bool {
        self.channel == other.channel && Arc::ptr_eq(&self.lut, &other.lut)
    }
}

/// Cube LUT 3D (Adobe order, R fastest) + mix amount.
#[derive(Clone, Debug)]
pub struct CubeLutPassParams {
    pub size: u32,
    pub amount: f32,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    /// Interleaved RGB, length `size³ * 3`.
    pub data: Arc<[f32]>,
}

impl PartialEq for CubeLutPassParams {
    fn eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.amount == other.amount
            && self.domain_min == other.domain_min
            && self.domain_max == other.domain_max
            && Arc::ptr_eq(&self.data, &other.data)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PointwiseOp {
    Invert,
    Threshold { threshold: f32 },
    Posterize { levels: u32 },
    Adjust(AdjustPointwisePassParams),
    Levels(LevelsPassParams),
    Curves(CurvesPassParams),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointwisePassParams {
    pub op: PointwiseOp,
}

/// GPU payload for nearest palette snap (Strict LUT path / PaletteQuantize filter).
#[derive(Clone, Debug)]
pub struct PaletteQuantizePassParams {
    pub lut_size: u32,
    pub l_range: (f32, f32),
    pub a_range: (f32, f32),
    pub b_range: (f32, f32),
    pub lut_grid: Arc<[u16]>,
    pub palette_rgb: Arc<[[f32; 3]]>,
}

impl PartialEq for PaletteQuantizePassParams {
    fn eq(&self, other: &Self) -> bool {
        self.lut_size == other.lut_size
            && self.l_range == other.l_range
            && self.a_range == other.a_range
            && self.b_range == other.b_range
            && Arc::ptr_eq(&self.lut_grid, &other.lut_grid)
            && Arc::ptr_eq(&self.palette_rgb, &other.palette_rgb)
    }
}

/// Ordered Bayer + Guided channel quantize (CPU `PaletteDitherMode::Guided`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaletteGuidedPassParams {
    /// Must be Bayer2 / Bayer4 / Bayer8.
    pub matrix: GpuPipelineKey,
    pub channel_levels: u8,
    pub threshold_scale: f32,
    /// 0=rgb, 1=gray, 2=rgb+dither_alpha, 3=gray+dither_alpha
    pub color_mode: u32,
    pub threshold_bias: f32,
    pub pattern_angle: f32,
    /// Per-channel `[min, max]` linear RGB ranges from the palette.
    pub ranges: [[f32; 2]; 3],
}

/// Mixed = Guided pass then ordered two-nearest snap.
#[derive(Clone, Debug)]
pub struct PaletteMixedPassParams {
    pub guided: PaletteGuidedPassParams,
    pub palette_rgb: Arc<[[f32; 3]]>,
    pub palette_lab: Arc<[[f32; 3]]>,
}

impl PartialEq for PaletteMixedPassParams {
    fn eq(&self, other: &Self) -> bool {
        self.guided == other.guided
            && Arc::ptr_eq(&self.palette_rgb, &other.palette_rgb)
            && Arc::ptr_eq(&self.palette_lab, &other.palette_lab)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuPass {
    pub pipeline: GpuPipelineKey,
    pub bayer: Option<BayerPassParams>,
    pub void_and_cluster: Option<VoidAndClusterPassParams>,
    pub halftone: Option<HalftonePassParams>,
    pub line_screen: Option<LineScreenPassParams>,
    pub crosshatch: Option<CrosshatchPassParams>,
    pub wave: Option<WavePassParams>,
    pub crt: Option<CrtPassParams>,
    pub glow: Option<GlowPassParams>,
    pub blur_noise: Option<BlurNoisePassParams>,
    pub detail: Option<DetailPassParams>,
    pub pointwise: Option<PointwisePassParams>,
    pub cube_lut: Option<CubeLutPassParams>,
    pub palette_quantize: Option<PaletteQuantizePassParams>,
    pub palette_guided: Option<PaletteGuidedPassParams>,
    pub palette_mixed: Option<PaletteMixedPassParams>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CpuCheckpointKind {
    ErrorDiffusion,
    BlockGranularity,
    IneligibleDither,
    AdjustBlur,
    UnsupportedFilter,
    FullStackFallback,
    SequentialGlobalDependency,
}

#[derive(Clone, Debug, PartialEq)]
#[allow(clippy::large_enum_variant)] // GpuPass carries wgpu resource handles by value.
pub enum GraphNode {
    Gpu(GpuPass),
    CpuCheckpoint(CpuCheckpointKind),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ComputeGraph {
    pub nodes: Vec<GraphNode>,
    pub content_hash: u64,
}

impl ComputeGraph {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn gpu_only_bayer4(&self) -> bool {
        self.nodes.len() == 1
            && matches!(
                self.nodes.first(),
                Some(GraphNode::Gpu(GpuPass {
                    pipeline: GpuPipelineKey::Bayer4,
                    ..
                }))
            )
    }

    pub fn is_gpu_only(&self) -> bool {
        !self.nodes.is_empty() && self.nodes.iter().all(|n| matches!(n, GraphNode::Gpu(_)))
    }
}

pub fn hash_graph_nodes(nodes: &[GraphNode]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for n in nodes {
        h = h.wrapping_mul(0x100000001b3);
        match n {
            GraphNode::CpuCheckpoint(k) => {
                h ^= (*k as u64).wrapping_add(1);
            }
            GraphNode::Gpu(p) => {
                h ^= p.pipeline as u64;
                if let Some(b) = &p.bayer {
                    h ^= (b.levels as u64) << 8;
                    h ^= b.color_mode as u64;
                    h ^= b.threshold_scale.to_bits() as u64;
                    h ^= b.threshold_bias.to_bits() as u64;
                    h ^= b.pattern_angle.to_bits() as u64;
                }
                if let Some(v) = &p.void_and_cluster {
                    h ^= (v.levels as u64) << 8;
                    h ^= v.color_mode as u64;
                    h ^= v.threshold_scale.to_bits() as u64;
                    h ^= v.threshold_bias.to_bits() as u64;
                    h ^= v.pattern_angle.to_bits() as u64;
                    h ^= 0x7661_635f; // "vac_" tag vs Bayer with same levels
                }
                if let Some(ht) = &p.halftone {
                    h ^= ht.cell_size as u64;
                    h ^= ht.threshold_scale.to_bits() as u64;
                    h ^= ht.angle_offset_deg.to_bits() as u64;
                    if ht.dither_alpha {
                        h ^= 0x100;
                    }
                    if ht.grayscale {
                        h ^= 0x200;
                    }
                }
                if let Some(ls) = &p.line_screen {
                    h ^= ls.cell_size as u64;
                    h ^= ls.threshold_scale.to_bits() as u64;
                    h ^= ls.pattern_angle.to_bits() as u64;
                    if ls.dither_alpha {
                        h ^= 0x300;
                    }
                    h ^= 0x6c73_5f5f; // "ls__"
                }
                if let Some(ch) = &p.crosshatch {
                    h ^= ch.spacing.to_bits() as u64;
                    h ^= ch.threshold_scale.to_bits() as u64;
                    h ^= ch.pattern_angle.to_bits() as u64;
                    if ch.dither_alpha {
                        h ^= 0x400;
                    }
                    h ^= 0x6368_5f5f; // "ch__"
                }
                if let Some(w) = &p.wave {
                    h ^= (w.levels as u64) << 8;
                    h ^= w.color_mode as u64;
                    h ^= w.threshold_scale.to_bits() as u64;
                    h ^= w.threshold_bias.to_bits() as u64;
                    h ^= w.wavelength.to_bits() as u64;
                    h ^= w.amplitude.to_bits() as u64;
                    h ^= w.phase.to_bits() as u64;
                    h ^= w.wave_angle.to_bits() as u64;
                    h ^= 0x7761_7665; // "wave"
                }
                if let Some(c) = &p.crt {
                    h ^= (c.period as u64) << 16;
                    h ^= c.strength.to_bits() as u64;
                    h ^= c.mask_strength.to_bits() as u64;
                }
                if let Some(g) = &p.glow {
                    h ^= g.radius.to_bits() as u64;
                    h ^= g.intensity.to_bits() as u64;
                    h ^= g.threshold.to_bits() as u64;
                    h ^= g.softness as u64;
                    h ^= 0x676c_6f77; // "glow"
                }
                if let Some(bn) = &p.blur_noise {
                    h ^= bn.blur.to_bits() as u64;
                    h ^= bn.noise.to_bits() as u64;
                    h ^= 0x626e_5f5f; // "bn__"
                }
                if let Some(d) = &p.detail {
                    h ^= d.clarity.to_bits() as u64;
                    h ^= d.sharpness.to_bits() as u64;
                    h ^= 0x6474_5f5f; // "dt__"
                }
                if let Some(pw) = &p.pointwise {
                    h ^= 0x7077_5f5f; // "pw__"
                    match &pw.op {
                        PointwiseOp::Invert => h ^= 1,
                        PointwiseOp::Threshold { threshold } => {
                            h ^= 2;
                            h ^= threshold.to_bits() as u64;
                        }
                        PointwiseOp::Posterize { levels } => {
                            h ^= 3;
                            h ^= *levels as u64;
                        }
                        PointwiseOp::Adjust(a) => {
                            h ^= 4;
                            h ^= a.exposure.to_bits() as u64;
                            h ^= a.contrast.to_bits() as u64;
                            h ^= a.brightness.to_bits() as u64;
                            h ^= a.saturation.to_bits() as u64;
                            h ^= a.shadows.to_bits() as u64;
                            h ^= a.highlights.to_bits() as u64;
                            h ^= a.temperature.to_bits() as u64;
                            h ^= a.tint.to_bits() as u64;
                            h ^= a.vibrance.to_bits() as u64;
                            h ^= a.hue.to_bits() as u64;
                        }
                        PointwiseOp::Levels(l) => {
                            h ^= 5;
                            h ^= l.input_black.to_bits() as u64;
                            h ^= l.input_white.to_bits() as u64;
                            h ^= l.gamma.to_bits() as u64;
                            h ^= l.output_black.to_bits() as u64;
                            h ^= l.output_white.to_bits() as u64;
                            h ^= l.channel_mask as u64;
                        }
                        PointwiseOp::Curves(c) => {
                            h ^= 6;
                            h ^= c.channel as u64;
                            h ^= c.lut.len() as u64;
                            if let Some(v) = c.lut.first() {
                                h ^= v.to_bits() as u64;
                            }
                            if let Some(v) = c.lut.last() {
                                h ^= v.to_bits() as u64;
                            }
                        }
                    }
                }
                if let Some(c) = &p.cube_lut {
                    h ^= 0x6362_5f5f; // "cb__"
                    h ^= c.size as u64;
                    h ^= c.amount.to_bits() as u64;
                    h ^= c.data.len() as u64;
                    h ^= c.domain_min[0].to_bits() as u64;
                    if let Some(v) = c.data.first() {
                        h ^= v.to_bits() as u64;
                    }
                }
                if let Some(pq) = &p.palette_quantize {
                    h ^= pq.lut_size as u64;
                    h ^= pq.palette_rgb.len() as u64;
                    h ^= pq.lut_grid.len() as u64;
                    h ^= pq.l_range.0.to_bits() as u64;
                    h ^= pq.a_range.0.to_bits() as u64;
                    if let Some(c0) = pq.palette_rgb.first() {
                        h ^= c0[0].to_bits() as u64;
                        h ^= c0[1].to_bits() as u64;
                        h ^= c0[2].to_bits() as u64;
                    }
                }
                if let Some(g) = &p.palette_guided {
                    h ^= g.matrix as u64;
                    h ^= g.channel_levels as u64;
                    h ^= g.color_mode as u64;
                    h ^= g.threshold_scale.to_bits() as u64;
                    h ^= g.threshold_bias.to_bits() as u64;
                    h ^= g.pattern_angle.to_bits() as u64;
                    h ^= g.ranges[0][0].to_bits() as u64;
                    h ^= g.ranges[2][1].to_bits() as u64;
                }
                if let Some(m) = &p.palette_mixed {
                    h ^= m.guided.matrix as u64;
                    h ^= m.guided.channel_levels as u64;
                    h ^= m.palette_rgb.len() as u64;
                    if let Some(c0) = m.palette_rgb.first() {
                        h ^= c0[0].to_bits() as u64;
                    }
                }
            }
        }
    }
    h
}

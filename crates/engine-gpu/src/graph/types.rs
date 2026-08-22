//! Compute graph types — mirrors CPU filter stack order.

use std::sync::Arc;

/// Lightweight filter description for graph compile (no engine-project dependency).
#[derive(Clone, Debug, PartialEq)]
pub enum GraphLayerFilter {
    /// Disabled filter entry — omitted from graph.
    Skip,
    Bayer(BayerPassParams),
    Halftone(HalftonePassParams),
    Crt(CrtPassParams),
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
    Halftone,
    Crt,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HalftonePassParams {
    pub cell_size: u8,
    pub threshold_scale: f32,
    pub dither_alpha: bool,
    pub grayscale: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrtPassParams {
    pub period: u8,
    pub strength: f32,
    pub mask_strength: f32,
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
    pub halftone: Option<HalftonePassParams>,
    pub crt: Option<CrtPassParams>,
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
}

#[derive(Clone, Debug, PartialEq)]
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
        !self.nodes.is_empty()
            && self
                .nodes
                .iter()
                .all(|n| matches!(n, GraphNode::Gpu(_)))
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
                if let Some(ht) = &p.halftone {
                    h ^= ht.cell_size as u64;
                    h ^= ht.threshold_scale.to_bits() as u64;
                    if ht.dither_alpha {
                        h ^= 0x100;
                    }
                    if ht.grayscale {
                        h ^= 0x200;
                    }
                }
                if let Some(c) = &p.crt {
                    h ^= (c.period as u64) << 16;
                    h ^= c.strength.to_bits() as u64;
                    h ^= c.mask_strength.to_bits() as u64;
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

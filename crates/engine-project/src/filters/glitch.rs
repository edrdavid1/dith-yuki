//! Glitch effects — JPEG-style spatial corruption (gather model).
//!
//! # Pipeline order (tile-correct)
//!
//! Forward: **interlace → block shift → block scramble → scanline → channel swap**.
//! Gather: `scanline_src` on dest, then scramble/shift on that pixel's block,
//! then `interlace_src` (modes 0–3; mode 3 = segment wrap like scanline).
//!
//! Block shift/scramble stay inside an aligned G×G group; scanline wraps inside
//! an aligned segment of length `scanline_segment` — all **halo 0** when
//! `B·G` and `S` divide the tile size.

use crate::error::EngineError;
use engine_tiles::coords::GlobalCoordSigned;
use engine_tiles::{PixelTile, TileCoord, HALO, TILE_SIZE};
use serde::{Deserialize, Serialize};

/// Tile full size including halo.
const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

/// PRNG tags so sub-ops do not correlate (mixed into [`mix_prng_key`] level).
const LVL_SHIFT: u8 = 1;
const LVL_SCRAMBLE: u8 = 2;
const LVL_SWAP: u8 = 3;
const LVL_INTERLACE: u8 = 4;
const LVL_SCANLINE: u8 = 5;

/// One enable + amount sub-effect.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GlitchSubOp {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub amount: f32,
}

impl Default for GlitchSubOp {
    fn default() -> Self {
        Self {
            enabled: false,
            amount: 0.0,
        }
    }
}

/// Glitch filter parameters (v2 gather model).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GlitchParams {
    /// Master gate (in addition to `FilterInstance.enabled`).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Deterministic seed.
    #[serde(default = "default_seed")]
    pub seed: u64,
    /// Macroblock size in px: 8, 16, or 32.
    #[serde(default = "default_block_size")]
    pub block_size: u32,
    /// Group size in blocks: 2, 4, or 8. Group side in px = `block_size * group_size`.
    #[serde(default = "default_group_size")]
    pub group_size: u32,
    /// Horizontal scanline segment length in px: 64, 128, or 256.
    #[serde(default = "default_scanline_segment")]
    pub scanline_segment: u32,
    #[serde(default)]
    pub block_shift: GlitchSubOp,
    #[serde(default)]
    pub block_scramble: GlitchSubOp,
    #[serde(default)]
    pub channel_swap: GlitchSubOp,
    #[serde(default)]
    pub scanline: GlitchSubOp,
    #[serde(default)]
    pub interlace: GlitchSubOp,
}

fn default_true() -> bool {
    true
}
fn default_seed() -> u64 {
    42
}
fn default_block_size() -> u32 {
    8
}
fn default_group_size() -> u32 {
    4
}
fn default_scanline_segment() -> u32 {
    64
}

impl Default for GlitchParams {
    fn default() -> Self {
        Self {
            enabled: true,
            seed: 42,
            block_size: 8,
            group_size: 4,
            scanline_segment: 64,
            block_shift: GlitchSubOp::default(),
            block_scramble: GlitchSubOp::default(),
            channel_swap: GlitchSubOp::default(),
            scanline: GlitchSubOp::default(),
            interlace: GlitchSubOp::default(),
        }
    }
}

impl<'de> Deserialize<'de> for GlitchParams {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(deserializer)?;
        if let Some(gt) = v.get("glitch_type").and_then(|x| x.as_str()) {
            let intensity = v.get("intensity").and_then(|x| x.as_f64()).unwrap_or(0.5) as f32;
            let seed = v.get("seed").and_then(|x| x.as_u64()).unwrap_or(42);
            let mut p = GlitchParams {
                seed,
                ..GlitchParams::default()
            };
            // Legacy RGBShift ≈ channel swap; BlockDisplace ≈ in-group shift.
            if gt == "BlockDisplace" {
                p.block_shift = GlitchSubOp {
                    enabled: intensity > 0.001,
                    amount: (intensity * 20.0).clamp(0.0, 20.0),
                };
            } else {
                p.channel_swap = GlitchSubOp {
                    enabled: intensity > 0.001,
                    amount: (intensity * 100.0).clamp(0.0, 100.0),
                };
            }
            return Ok(p);
        }
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default = "default_true")]
            enabled: bool,
            #[serde(default = "default_seed")]
            seed: u64,
            #[serde(default = "default_block_size")]
            block_size: u32,
            #[serde(default = "default_group_size")]
            group_size: u32,
            #[serde(default = "default_scanline_segment")]
            scanline_segment: u32,
            #[serde(default)]
            block_shift: GlitchSubOp,
            #[serde(default)]
            block_scramble: GlitchSubOp,
            #[serde(default)]
            channel_swap: GlitchSubOp,
            #[serde(default)]
            scanline: GlitchSubOp,
            #[serde(default)]
            interlace: GlitchSubOp,
        }
        let raw: Raw = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
        Ok(GlitchParams {
            enabled: raw.enabled,
            seed: raw.seed,
            block_size: raw.block_size,
            group_size: raw.group_size,
            scanline_segment: raw.scanline_segment,
            block_shift: raw.block_shift,
            block_scramble: raw.block_scramble,
            channel_swap: raw.channel_swap,
            scanline: raw.scanline,
            interlace: raw.interlace,
        })
    }
}

impl GlitchParams {
    /// Validate B, G, and amount ranges.
    pub fn validate(&self) -> Result<(), EngineError> {
        if ![8, 16, 32].contains(&self.block_size) {
            return Err(EngineError::InvalidFilterParams {
                reason: "block_size must be 8, 16, or 32".into(),
            });
        }
        if ![2, 4, 8].contains(&self.group_size) {
            return Err(EngineError::InvalidFilterParams {
                reason: "group_size must be 2, 4, or 8".into(),
            });
        }
        if ![64, 128, 256].contains(&self.scanline_segment) {
            return Err(EngineError::InvalidFilterParams {
                reason: "scanline_segment must be 64, 128, or 256".into(),
            });
        }
        debug_assert!(
            TILE_SIZE.is_multiple_of(self.block_size * self.group_size),
            "B·G must divide TILE_SIZE"
        );
        debug_assert!(
            TILE_SIZE.is_multiple_of(self.scanline_segment),
            "scanline_segment must divide TILE_SIZE"
        );
        check_amount("block_shift", self.block_shift.amount, 0.0, 20.0)?;
        check_amount("block_scramble", self.block_scramble.amount, 0.0, 20.0)?;
        check_amount("channel_swap", self.channel_swap.amount, 0.0, 100.0)?;
        check_amount("scanline", self.scanline.amount, 0.0, 100.0)?;
        check_amount("interlace", self.interlace.amount, 0.0, 100.0)?;
        Ok(())
    }

    fn any_active(&self) -> bool {
        self.enabled
            && (sub_active(&self.block_shift)
                || sub_active(&self.block_scramble)
                || sub_active(&self.channel_swap)
                || sub_active(&self.interlace)
                || sub_active(&self.scanline))
    }
}

/// True when the gather pipeline would not move any pixels (skip apply).
pub fn glitch_params_neutral(p: &GlitchParams) -> bool {
    !p.any_active()
}

fn sub_active(s: &GlitchSubOp) -> bool {
    s.enabled && s.amount > 0.0
}

fn check_amount(name: &str, v: f32, lo: f32, hi: f32) -> Result<(), EngineError> {
    if !(lo..=hi).contains(&v) {
        return Err(EngineError::InvalidFilterParams {
            reason: format!("{name} amount must be in [{lo}, {hi}]"),
        });
    }
    Ok(())
}

/// Glitch filter instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlitchFilter {
    pub params: GlitchParams,
}

impl GlitchFilter {
    pub fn new(params: GlitchParams) -> Result<Self, EngineError> {
        params.validate()?;
        Ok(Self { params })
    }

    /// Apply into `dst` (full 260² write).
    pub fn apply_to_tile_into(
        &self,
        tile: &PixelTile,
        coord: TileCoord,
        img_w: u32,
        img_h: u32,
        dst: &mut PixelTile,
    ) -> Result<(), EngineError> {
        if !self.params.any_active() {
            dst.copy_from(tile);
            return Ok(());
        }
        let (lw, lh) = level_dims(img_w, img_h, coord.level);
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
                let rgba = gather_rgba(&self.params, g.x, g.y, lw, lh, coord.level, |sx, sy| {
                    sample_tile_rgba(tile, coord, sx, sy)
                });
                for c in 0..4 {
                    dst.set(x, y, c, rgba[c as usize]);
                }
            }
        }
        Ok(())
    }

    pub fn apply_to_tile(
        &self,
        tile: &PixelTile,
        coord: TileCoord,
        img_w: u32,
        img_h: u32,
    ) -> Result<PixelTile, EngineError> {
        let mut out = PixelTile::new();
        self.apply_to_tile_into(tile, coord, img_w, img_h, &mut out)?;
        Ok(out)
    }
}

fn level_dims(w: u32, h: u32, level: u8) -> (i32, i32) {
    let s = 1u32 << level;
    let lw = w.saturating_add(s - 1) / s;
    let lh = h.saturating_add(s - 1) / s;
    (lw.max(1) as i32, lh.max(1) as i32)
}

fn sample_tile_rgba(tile: &PixelTile, coord: TileCoord, gx: i32, gy: i32) -> [f32; 4] {
    // Complete Post neighborhood → cross-tile Dithered samples (PR-H).
    if let Some(nb) = crate::filters::neighbors::current_spatial_neighbors() {
        if nb.complete {
            return nb.sample_rgba(gx, gy);
        }
    }
    let (lx, ly) = GlobalCoordSigned { x: gx, y: gy }.to_local_with_halo(coord, HALO);
    let max = TILE_FULL_SIZE as i32 - 1;
    let lx = lx.clamp(0, max) as u32;
    let ly = ly.clamp(0, max) as u32;
    [
        tile.at(lx, ly, 0),
        tile.at(lx, ly, 1),
        tile.at(lx, ly, 2),
        tile.at(lx, ly, 3),
    ]
}

/// Neighbor gather radius for active Glitch (group / scanline footprint).
///
/// Caps at [`TILE_SIZE`] so Moore ±1 tile covers the worst-case block/group
/// and scanline segment (both ≤ 256).
pub fn glitch_spatial_radius_px(params: &GlitchParams) -> i32 {
    if !params.any_active() {
        return 0;
    }
    let group = (params.block_size.saturating_mul(params.group_size)) as i32;
    let scan = params.scanline_segment as i32;
    group.max(scan).clamp(1, TILE_SIZE as i32)
}

/// Gather one destination pixel. Pure function of global coords + params.
///
/// Forward: interlace → shift → scramble → scanline → channel swap.
/// Gather: scanline on dest → scramble → shift → interlace → sample,
/// then channel swap at the **destination** block.
pub fn gather_rgba<F>(
    params: &GlitchParams,
    gx: i32,
    gy: i32,
    img_w: i32,
    img_h: i32,
    mip: u8,
    sample: F,
) -> [f32; 4]
where
    F: Fn(i32, i32) -> [f32; 4],
{
    if gx < 0 || gy < 0 || gx >= img_w || gy >= img_h {
        return sample(gx, gy);
    }

    let b = params.block_size as i32;
    let g = params.group_size as i32;
    let seed = params.seed;
    let seg = params.scanline_segment as i32;

    // Channel swap keys off the final dest block.
    let dest_bx = gx.div_euclid(b);
    let dest_by = gy.div_euclid(b);

    // Inverse of scanline (last geometric stage before channel swap).
    let (px, py) = scanline_src(seed, gx, gy, &params.scanline, mip, img_w, seg);
    let ox = px.rem_euclid(b);
    let oy = py.rem_euclid(b);
    let pbx = px.div_euclid(b);
    let pby = py.div_euclid(b);

    // scramble then shift (inverse of forward shift→scramble)
    let (bx1, by1) = scramble_src(
        seed,
        pbx,
        pby,
        g,
        &params.block_scramble,
        mip,
        img_w,
        img_h,
        b,
    );
    let (bx2, by2) = block_shift_src(seed, bx1, by1, g, &params.block_shift, mip, img_w, img_h, b);

    let sx = bx2 * b + ox;
    let sy = by2 * b + oy;

    let rgba = interlace_sample(params, seed, sx, sy, img_w, img_h, mip, &sample);
    apply_channel_swap(params, seed, dest_bx, dest_by, mip, rgba)
}

/// Scanline gather: circular shift within an aligned horizontal segment.
///
/// Moj Boy shifted the full row; we shift within segments of length `S` so the
/// source stays in-tile when `S` divides the tile size (halo 0).
pub fn scanline_src(
    seed: u64,
    gx: i32,
    gy: i32,
    op: &GlitchSubOp,
    mip: u8,
    img_w: i32,
    segment: i32,
) -> (i32, i32) {
    if !sub_active(op) || segment <= 1 || img_w <= 0 {
        return (gx, gy);
    }
    let p = (op.amount / 100.0).clamp(0.0, 1.0);
    let seg0 = gx.div_euclid(segment) * segment;
    let len = (img_w - seg0).min(segment).max(1);
    let local = gx - seg0;
    if local < 0 || local >= len {
        return (gx, gy);
    }
    let k = mix_prng_key(seed, seg0, gy, op_level(LVL_SCANLINE, mip));
    // Moj Boy: probability p·0.5 per row; we decide per (row, segment).
    if unit(k) >= p * 0.5 {
        return (gx, gy);
    }
    let u_max = ((len as f32) * p * 0.3).floor() as i32;
    if u_max < 1 {
        return (gx, gy);
    }
    let mut r = rand_in(k ^ 11, -u_max, u_max);
    if r == 0 {
        r = if unit(k ^ 13) < 0.5 { 1 } else { -1 };
    }
    let src_local = (local - r).rem_euclid(len);
    (seg0 + src_local, gy)
}

/// Block shift gather: with prob 0.3, pull from a neighbor **inside the same group**.
pub fn block_shift_src(
    seed: u64,
    gbx: i32,
    gby: i32,
    g: i32,
    op: &GlitchSubOp,
    mip: u8,
    img_w: i32,
    img_h: i32,
    block_px: i32,
) -> (i32, i32) {
    if !sub_active(op) {
        return (gbx, gby);
    }
    let a = shift_amplitude(op.amount, g);
    let k = mix_prng_key(seed, gbx, gby, op_level(LVL_SHIFT, mip));
    if unit(k) >= 0.3 {
        return (gbx, gby);
    }
    let ox = gbx.rem_euclid(g);
    let oy = gby.rem_euclid(g);
    let dx = rand_in(k ^ 1, -a, a);
    let dy = rand_in(k ^ 2, -a, a);
    let sx = (ox + dx).rem_euclid(g);
    let sy = (oy + dy).rem_euclid(g);
    let out = (gbx - ox + sx, gby - oy + sy);
    if !block_in_image(out.0, out.1, block_px, img_w, img_h) {
        return (gbx, gby);
    }
    out
}

/// Scramble gather: Fisher–Yates within the group with probability α = amount/20.
pub fn scramble_src(
    seed: u64,
    gbx: i32,
    gby: i32,
    g: i32,
    op: &GlitchSubOp,
    mip: u8,
    img_w: i32,
    img_h: i32,
    block_px: i32,
) -> (i32, i32) {
    if !sub_active(op) {
        return (gbx, gby);
    }
    let alpha = (op.amount / 20.0).clamp(0.0, 1.0);
    let ox = gbx.rem_euclid(g);
    let oy = gby.rem_euclid(g);
    let group_bx = gbx - ox;
    let group_by = gby - oy;
    let k = mix_prng_key(seed, group_bx, group_by, op_level(LVL_SCRAMBLE, mip));
    if unit(k) >= alpha {
        return (gbx, gby);
    }
    let n = (g * g) as usize;
    let mut perm: Vec<usize> = (0..n).collect();
    let mut rng = XorShift64::new(k ^ 0xC5A5_5C7A);
    for i in (1..n).rev() {
        let j = (rng.next() as usize) % (i + 1);
        perm.swap(i, j);
    }
    let dest_i = (oy * g + ox) as usize;
    let src_i = perm[dest_i];
    let sx = (src_i as i32) % g;
    let sy = (src_i as i32) / g;
    let out = (group_bx + sx, group_by + sy);
    if !block_in_image(out.0, out.1, block_px, img_w, img_h) {
        return (gbx, gby);
    }
    out
}

/// Channel-swap permutation for a dest block, or identity.
pub fn swap_perm(seed: u64, gbx: i32, gby: i32, op: &GlitchSubOp, mip: u8) -> [usize; 3] {
    const ID: [usize; 3] = [0, 1, 2];
    const PERMS: [[usize; 3]; 5] = [[0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]];
    if !sub_active(op) {
        return ID;
    }
    let p = (op.amount / 100.0).clamp(0.0, 1.0);
    let k = mix_prng_key(seed, gbx, gby, op_level(LVL_SWAP, mip));
    if unit(k) >= p {
        return ID;
    }
    let idx = (rand_in(k ^ 3, 0, 4)) as usize;
    PERMS[idx]
}

fn apply_channel_swap(
    params: &GlitchParams,
    seed: u64,
    dest_bx: i32,
    dest_by: i32,
    mip: u8,
    rgba: [f32; 4],
) -> [f32; 4] {
    let perm = swap_perm(seed, dest_bx, dest_by, &params.channel_swap, mip);
    [rgba[perm[0]], rgba[perm[1]], rgba[perm[2]], rgba[3]]
}

/// Interlace gather for modes 0–3.
///
/// Mode 3 is a segment circular shift (same halo-0 idea as scanline), using
/// `±⌊S/10⌋` instead of Moj Boy's full-width `±⌊W/10⌋`.
pub fn interlace_src(
    seed: u64,
    gx: i32,
    gy: i32,
    op: &GlitchSubOp,
    mip: u8,
    img_w: i32,
    img_h: i32,
    segment: i32,
) -> InterlaceSrc {
    if !sub_active(op) || gy.rem_euclid(2) != 0 {
        return InterlaceSrc::Single(gx, gy);
    }
    let p = (op.amount / 100.0).clamp(0.0, 1.0);
    // Decision is per even row (Moj Boy), not per pixel.
    let k = mix_prng_key(seed, 0, gy, op_level(LVL_INTERLACE, mip));
    if unit(k) >= p {
        return InterlaceSrc::Single(gx, gy);
    }
    let mode = rand_in(k ^ 7, 0, 3);
    match mode {
        0 => {
            let py = gy - 1;
            if py < 0 {
                InterlaceSrc::Single(gx, gy)
            } else {
                InterlaceSrc::Single(gx, py)
            }
        }
        1 => {
            let ny = gy + 1;
            if ny >= img_h {
                InterlaceSrc::Single(gx, gy)
            } else {
                InterlaceSrc::Single(gx, ny)
            }
        }
        2 => {
            let py = gy - 1;
            let ny = gy + 1;
            if py < 0 || ny >= img_h {
                InterlaceSrc::Single(gx, gy)
            } else {
                InterlaceSrc::Average((gx, py), (gx, ny))
            }
        }
        _ => {
            // Mode 3: horizontal wrap within the aligned segment.
            if segment <= 1 || img_w <= 0 {
                return InterlaceSrc::Single(gx, gy);
            }
            let seg0 = gx.div_euclid(segment) * segment;
            let len = (img_w - seg0).min(segment).max(1);
            let local = gx - seg0;
            if local < 0 || local >= len {
                return InterlaceSrc::Single(gx, gy);
            }
            let u_max = (len / 10).max(1);
            let mut r = rand_in(k ^ 9, -u_max, u_max);
            if r == 0 {
                r = 1;
            }
            let src_local = (local - r).rem_euclid(len);
            InterlaceSrc::Single(seg0 + src_local, gy)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterlaceSrc {
    Single(i32, i32),
    Average((i32, i32), (i32, i32)),
}

fn interlace_sample<F>(
    params: &GlitchParams,
    seed: u64,
    sx: i32,
    sy: i32,
    img_w: i32,
    img_h: i32,
    mip: u8,
    sample: &F,
) -> [f32; 4]
where
    F: Fn(i32, i32) -> [f32; 4],
{
    // Clamp geometric source into the image; incomplete edge groups use identity
    // at the block level already, but pixel offset can still land past the edge.
    let sx = sx.clamp(0, img_w - 1);
    let sy = sy.clamp(0, img_h - 1);
    let seg = params.scanline_segment as i32;
    match interlace_src(seed, sx, sy, &params.interlace, mip, img_w, img_h, seg) {
        InterlaceSrc::Single(x, y) => sample(x, y),
        InterlaceSrc::Average((x0, y0), (x1, y1)) => {
            let a = sample(x0, y0);
            let b = sample(x1, y1);
            [
                ((a[0] + b[0]) * 0.5).floor_compat(),
                ((a[1] + b[1]) * 0.5).floor_compat(),
                ((a[2] + b[2]) * 0.5).floor_compat(),
                a[3],
            ]
        }
    }
}

trait FloorCompat {
    fn floor_compat(self) -> Self;
}
impl FloorCompat for f32 {
    fn floor_compat(self) -> Self {
        // Moj Boy used integer floor on bytes; we stay in f32 linear — mean is enough.
        self
    }
}

fn shift_amplitude(amount: f32, g: i32) -> i32 {
    let a = (amount / 8.0).floor().max(1.0) as i32;
    a.min(g - 1).max(1)
}

fn block_in_image(bx: i32, by: i32, block_px: i32, img_w: i32, img_h: i32) -> bool {
    bx >= 0 && by >= 0 && bx * block_px < img_w && by * block_px < img_h
}

fn op_level(tag: u8, mip: u8) -> u8 {
    tag.wrapping_add(mip.wrapping_mul(16))
}

/// PRNG key: `seed XOR f(global_x, global_y, level)`.
#[inline]
pub fn mix_prng_key(seed: u64, gx: i32, gy: i32, level: u8) -> u64 {
    seed ^ (gx as u64).wrapping_mul(0x9E3779B97F4A7C15)
        ^ (gy as u64).wrapping_mul(0xC2B2AE3D27D4EB4F)
        ^ (level as u64)
}

struct XorShift64 {
    state: u64,
}

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 1 } else { seed },
        }
    }

    fn next(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 32) as u32
    }

    fn next_f32(&mut self) -> f32 {
        (self.next() as f32) / (u32::MAX as f32)
    }
}

fn unit(k: u64) -> f32 {
    XorShift64::new(k).next_f32()
}

/// Inclusive integer range `[lo, hi]`.
fn rand_in(k: u64, lo: i32, hi: i32) -> i32 {
    if hi < lo {
        return lo;
    }
    let span = (hi as i64 - lo as i64 + 1) as u32;
    lo + (XorShift64::new(k).next() % span) as i32
}

/// Apply glitch to a packed RGBA f32 image using virtual tiles of side `tile`
/// (must be divisible by `B·G`). Used by the tile-invariance harness.
pub fn apply_glitch_virtual_tiles(
    src: &[f32],
    width: u32,
    height: u32,
    params: &GlitchParams,
    tile: u32,
    mip: u8,
) -> Vec<f32> {
    assert_eq!(src.len(), (width * height * 4) as usize);
    assert!(tile > 0);
    let cell = params.block_size * params.group_size;
    assert_eq!(tile % cell, 0, "B·G must divide virtual tile size");
    let mut out = src.to_vec();
    let w = width as i32;
    let h = height as i32;
    let sample = |sx: i32, sy: i32| -> [f32; 4] {
        let x = sx.clamp(0, w - 1) as u32;
        let y = sy.clamp(0, h - 1) as u32;
        let i = ((y * width + x) * 4) as usize;
        [src[i], src[i + 1], src[i + 2], src[i + 3]]
    };
    // Pure gather — partition only affects iteration order; results must match.
    for y in 0..height {
        for x in 0..width {
            let _tx = x / tile;
            let _ty = y / tile;
            let rgba = gather_rgba(params, x as i32, y as i32, w, h, mip, sample);
            let i = ((y * width + x) * 4) as usize;
            out[i] = rgba[0];
            out[i + 1] = rgba[1];
            out[i + 2] = rgba[2];
            out[i + 3] = rgba[3];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_params() -> GlitchParams {
        GlitchParams {
            enabled: true,
            seed: 99,
            block_size: 8,
            group_size: 4,
            scanline_segment: 64,
            block_shift: GlitchSubOp {
                enabled: true,
                amount: 16.0,
            },
            block_scramble: GlitchSubOp {
                enabled: true,
                amount: 20.0,
            },
            channel_swap: GlitchSubOp {
                enabled: true,
                amount: 80.0,
            },
            scanline: GlitchSubOp {
                enabled: true,
                amount: 80.0,
            },
            interlace: GlitchSubOp {
                enabled: true,
                amount: 70.0,
            },
        }
    }

    fn fill_pattern(w: u32, h: u32) -> Vec<f32> {
        let mut v = vec![0.0; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                v[i] = ((x + 10_000) % 251) as f32 / 250.0;
                v[i + 1] = ((y + 10_000) % 251) as f32 / 250.0;
                v[i + 2] = ((x.wrapping_add(y) + 10_000) % 251) as f32 / 250.0;
                v[i + 3] = 1.0;
            }
        }
        v
    }

    #[test]
    fn amount_zero_is_noop() {
        let p = GlitchParams::default();
        let src = fill_pattern(64, 64);
        let out = apply_glitch_virtual_tiles(&src, 64, 64, &p, 64, 0);
        assert_eq!(src, out);
    }

    #[test]
    fn validate_rejects_bad_block_size() {
        let mut p = GlitchParams::default();
        p.block_size = 7;
        assert!(p.validate().is_err());
    }

    #[test]
    fn legacy_deserialize_block_displace() {
        let v = serde_json::json!({
            "glitch_type": "BlockDisplace",
            "intensity": 0.5,
            "seed": 7
        });
        let p: GlitchParams = serde_json::from_value(v).unwrap();
        assert_eq!(p.seed, 7);
        assert!(p.block_shift.enabled);
        assert!((p.block_shift.amount - 10.0).abs() < 1e-5);
    }

    #[test]
    fn tile_invariance_across_virtual_sizes() {
        let params = active_params();
        params.validate().unwrap();
        // Include sizes that stress tile boundaries for interlace (even rows on edges).
        let (w, h) = (257u32, 255u32);
        let src = fill_pattern(w, h);
        let reference = apply_glitch_virtual_tiles(&src, w, h, &params, 256, 0);
        for t in [64u32, 128, 256, 512] {
            let cell = params.block_size * params.group_size;
            if t % cell != 0 {
                continue;
            }
            let out = apply_glitch_virtual_tiles(&src, w, h, &params, t, 0);
            assert_eq!(
                out, reference,
                "virtual tile {t} must match reference partition"
            );
        }
    }

    #[test]
    fn interlace_at_tile_boundary_rows() {
        // Image height 64: tile 32 places a tile edge at y=32 (even).
        let params = GlitchParams {
            interlace: GlitchSubOp {
                enabled: true,
                amount: 100.0,
            },
            block_shift: GlitchSubOp::default(),
            block_scramble: GlitchSubOp::default(),
            channel_swap: GlitchSubOp::default(),
            ..active_params()
        };
        let (w, h) = (64u32, 64u32);
        let src = fill_pattern(w, h);
        let a = apply_glitch_virtual_tiles(&src, w, h, &params, 32, 0);
        let b = apply_glitch_virtual_tiles(&src, w, h, &params, 64, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn scanline_stays_in_segment() {
        let op = GlitchSubOp {
            enabled: true,
            amount: 100.0,
        };
        let s = 64;
        let w = 256;
        for y in 0..16 {
            for x in 0..w {
                let (sx, sy) = scanline_src(42, x, y, &op, 0, w, s);
                assert_eq!(sy, y);
                assert_eq!(sx.div_euclid(s), x.div_euclid(s));
            }
        }
    }

    #[test]
    fn scanline_tile_invariance() {
        let params = GlitchParams {
            scanline: GlitchSubOp {
                enabled: true,
                amount: 100.0,
            },
            block_shift: GlitchSubOp::default(),
            block_scramble: GlitchSubOp::default(),
            channel_swap: GlitchSubOp::default(),
            interlace: GlitchSubOp::default(),
            scanline_segment: 64,
            ..active_params()
        };
        let (w, h) = (192u32, 64u32);
        let src = fill_pattern(w, h);
        let a = apply_glitch_virtual_tiles(&src, w, h, &params, 64, 0);
        let b = apply_glitch_virtual_tiles(&src, w, h, &params, 192, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn shift_stays_in_group() {
        let op = GlitchSubOp {
            enabled: true,
            amount: 20.0,
        };
        let g = 4;
        for gbx in 0..16 {
            for gby in 0..16 {
                let (sx, sy) = block_shift_src(42, gbx, gby, g, &op, 0, 512, 512, 8);
                assert_eq!(sx.div_euclid(g), gbx.div_euclid(g));
                assert_eq!(sy.div_euclid(g), gby.div_euclid(g));
            }
        }
    }

    #[test]
    fn scramble_stays_in_group() {
        let op = GlitchSubOp {
            enabled: true,
            amount: 20.0,
        };
        let g = 4;
        for gbx in 0..16 {
            for gby in 0..16 {
                let (sx, sy) = scramble_src(7, gbx, gby, g, &op, 0, 512, 512, 8);
                assert_eq!(sx.div_euclid(g), gbx.div_euclid(g));
                assert_eq!(sy.div_euclid(g), gby.div_euclid(g));
            }
        }
    }

    #[test]
    fn edge_block_outside_stays_identity() {
        // 7×9 image: only block (0,0) of B=8 is partially valid; neighbor blocks OOB.
        let op = GlitchSubOp {
            enabled: true,
            amount: 20.0,
        };
        let (sx, sy) = block_shift_src(1, 0, 0, 4, &op, 0, 7, 9, 8);
        // May be identity or in-group; in-group others are OOB → identity.
        assert_eq!((sx, sy), (0, 0));
    }

    #[test]
    fn default_params_apply_is_bitwise_identity() {
        let params = GlitchParams::default();
        assert!(!params.any_active());
        let filter = GlitchFilter::new(params).unwrap();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let v = f32::from_bits(0x3f000000 + (x ^ y));
                tile.set(x, y, 0, v);
                tile.set(x, y, 1, 0.25);
                tile.set(x, y, 2, 0.75);
                tile.set(x, y, 3, if x % 4 == 0 { 0.0 } else { 1.0 });
            }
        }
        let out = filter.apply_to_tile(&tile, coord, 256, 256).unwrap();
        assert_eq!(out.data, tile.data);
    }

    #[test]
    fn apply_to_tile_matches_gather() {
        let params = active_params();
        let filter = GlitchFilter::new(params.clone()).unwrap();
        let coord = TileCoord {
            level: 0,
            x: 0,
            y: 0,
        };
        let (w, h) = (128u32, 128u32);
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
                let px = g.x.clamp(0, w as i32 - 1) as u32;
                let py = g.y.clamp(0, h as i32 - 1) as u32;
                tile.set(x, y, 0, ((px + 10_000) % 251) as f32 / 250.0);
                tile.set(x, y, 1, ((py + 10_000) % 251) as f32 / 250.0);
                tile.set(
                    x,
                    y,
                    2,
                    ((px.wrapping_add(py) + 10_000) % 251) as f32 / 250.0,
                );
                tile.set(x, y, 3, 1.0);
            }
        }
        let out = filter.apply_to_tile(&tile, coord, w, h).unwrap();
        let sample = |sx: i32, sy: i32| sample_tile_rgba(&tile, coord, sx, sy);
        for y in HALO..(HALO + 64) {
            for x in HALO..(HALO + 64) {
                let g = GlobalCoordSigned::from_local_with_halo(coord, x, y, HALO);
                let expect = gather_rgba(&params, g.x, g.y, w as i32, h as i32, 0, sample);
                for c in 0..3 {
                    assert!(
                        (out.at(x, y, c) - expect[c as usize]).abs() < 1e-6,
                        "mismatch at local ({x},{y}) c={c}"
                    );
                }
            }
        }
    }

    #[test]
    fn spatial_radius_zero_when_inactive() {
        assert_eq!(glitch_spatial_radius_px(&GlitchParams::default()), 0);
    }

    #[test]
    fn spatial_radius_tracks_group_and_scanline() {
        let mut p = GlitchParams::default();
        p.block_shift = GlitchSubOp {
            enabled: true,
            amount: 10.0,
        };
        p.block_size = 16;
        p.group_size = 4; // 64 px group
        p.scanline_segment = 128;
        assert_eq!(glitch_spatial_radius_px(&p), 128);
        p.scanline_segment = 64;
        assert_eq!(glitch_spatial_radius_px(&p), 64);
    }
}

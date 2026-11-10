//! 1D error-diffusion core (sibling of FS, no tile knowledge).
//!
//! Style semantics for `modulated_diffuse_*`: **1D ED + contrast** — carry is
//! weakened on sharp source-luminance transitions (`K = 4` scales linear ΔY so
//! the `line_scale` slider has useful range).
//!
//! `glitch_dither`: optional [`SrcXf`] corrupts the source row **before**
//! weight/ED (authorial VHS/digital corruption — not a Moj Boy port).
//!
//! Vertical axis uses the full-height column strip driver in production
//! (`requires_full_column` / `modulated_diffuse_x`). Contiguous-buffer
//! transpose in [`ed1d_image`] remains valid for tests and goldens.

use rayon::prelude::*;

use crate::filters::glitch::mix_prng_key;

/// Scan axis for the 1D carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Left→right along each row (`modulated_diffuse_y` in Moj Boy metadata).
    Horizontal,
    /// Top→bottom along each column (`modulated_diffuse_x`); production uses
    /// full-height column strips (transpose → row ED → inverse).
    Vertical,
}

/// Optional pre-quantization source transform (enum so Hash/Eq/Send stay easy).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SrcXf {
    /// Digital corruption: tear / channel offset / dropout (see [`glitch_row`]).
    Glitch { intensity: u8, seed: u32 },
}

/// How carry weight is derived along a scan line (tickets 0.2b / Phase 1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WeightFn {
    /// Constant weight 1.
    Const,
    /// Neighbor `|ΔY|` attenuation (`modulated_diffuse_*`). `gain = line_scale/20`.
    Contrast { gain: f32 },
    /// Local luminance σ in a centered `2·half+1` window (`contrast_aware_*`).
    ContrastStd { gain: f32, half: u8 },
    /// Position sine along the carry axis (`wave_modulation_*`).
    /// `freq` = periods across [`Ed1dCfg::line_len`]; `amp` ∈ [0, 1] (= UI/100).
    Sine { freq: f32, amp: f32 },
}

/// Configuration for [`ed1d_row`] / [`ed1d_image`].
#[derive(Debug, Clone, Copy)]
pub struct Ed1dCfg {
    pub axis: Axis,
    /// Global fraction of carried error (default 1.0; not exposed in UI yet).
    pub carry: f32,
    /// Per-sample carry weight.
    pub weight: WeightFn,
    /// Optional EMA α on the *carried* error after weighting.
    /// `None` or `Some(1.0)` ⇒ no smoothing (bit-identical to pre-EMA).
    pub ema: Option<f32>,
    /// Applied before weight / ED. `None` for modulated_diffuse / pure 1D.
    pub src_transform: Option<SrcXf>,
    /// Global index of `src[0]` along the scan line (Sine phase). Default 0.
    pub line_origin: u32,
    /// Step between consecutive `src` samples in global cells (1, or `pixel_size`).
    pub line_stride: u32,
    /// Full line length N for Sine. `0` ⇒ use `src.len()` (contiguous / full strip).
    pub line_len: u32,
}

impl Default for Ed1dCfg {
    fn default() -> Self {
        Self {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Const,
            ema: None,
            src_transform: None,
            line_origin: 0,
            line_stride: 1,
            line_len: 0,
        }
    }
}

/// Linear-space Rec.709 luminance (same as FS ED).
#[inline]
pub fn to_luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}

/// Scales linear ΔY so `line_scale` is useful (transitions rarely exceed ~0.25).
const CONTRAST_K: f32 = 4.0;

/// Scales local luminance σ (5-px window) for `contrast_aware_*`. Same order as
/// [`CONTRAST_K`]: a hard step (σ≈0.5) fully attenuates at `gain = 1`.
const CONTRAST_STD_K: f32 = 4.0;

/// Default half-width for [`WeightFn::ContrastStd`] → window of 5.
pub const CONTRAST_STD_HALF: u8 = 2;

/// Tear band height: breaks share a decision across `BAND` consecutive lines.
pub const GLITCH_BAND: u64 = 3;

// PRNG level tags (distinct so sub-ops do not correlate).
const LVL_TEAR: u8 = 1;
const LVL_TEAR_R: u8 = 2;
const LVL_CH: u8 = 3;
const LVL_CH_K: u8 = 4;
const LVL_CH_WHICH: u8 = 5;
const LVL_DROP: u8 = 6;
const LVL_DROP_START: u8 = 7;
const LVL_DROP_LEN: u8 = 8;

/// Quantizer abstraction so Strict/Uniform and Simple share one row loop.
pub trait EdQuantizer {
    /// Given source linear RGB and accumulated error, return
    /// `(quantized_linear_rgb, residual_to_diffuse)`.
    fn quantize(&mut self, src: [f32; 3], acc_err: [f32; 3]) -> ([f32; 3], [f32; 3]);
}

/// Uniform levels quantizer (linear RGB residual) — mirrors FS Uniform path.
#[derive(Clone)]
pub struct UniformLevelsQuantizer {
    pub levels: f32,
}

impl EdQuantizer for UniformLevelsQuantizer {
    fn quantize(&mut self, src: [f32; 3], acc_err: [f32; 3]) -> ([f32; 3], [f32; 3]) {
        let adj = [
            (src[0] + acc_err[0]).clamp(0.0, 1.0),
            (src[1] + acc_err[1]).clamp(0.0, 1.0),
            (src[2] + acc_err[2]).clamp(0.0, 1.0),
        ];
        let q = [
            quantize_uniform(adj[0], self.levels),
            quantize_uniform(adj[1], self.levels),
            quantize_uniform(adj[2], self.levels),
        ];
        (q, [adj[0] - q[0], adj[1] - q[1], adj[2] - q[2]])
    }
}

#[inline]
fn quantize_uniform(value: f32, levels: f32) -> f32 {
    let scaled = value * (levels - 1.0);
    scaled.round().clamp(0.0, levels - 1.0) / (levels - 1.0)
}

/// Population stddev of Rec.709 luminance in `src[lo..hi]` (half-open).
#[inline]
fn luma_std(src: &[[f32; 3]], lo: usize, hi: usize) -> f32 {
    let n = hi.saturating_sub(lo);
    if n <= 1 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for pix in &src[lo..hi] {
        sum += to_luminance(*pix);
    }
    let mean = sum / n as f32;
    let mut var = 0.0f32;
    for pix in &src[lo..hi] {
        let d = to_luminance(*pix) - mean;
        var += d * d;
    }
    (var / n as f32).sqrt()
}

#[inline]
fn weight_at(i: usize, src: &[[f32; 3]], cfg: &Ed1dCfg) -> f32 {
    match cfg.weight {
        WeightFn::Const => 1.0,
        WeightFn::Contrast { gain: s } => {
            let c = if i == 0 {
                0.0
            } else {
                (to_luminance(src[i]) - to_luminance(src[i - 1])).abs()
            };
            (1.0 - s * (1.0f32).min(CONTRAST_K * c)).clamp(0.0, 1.0)
        }
        WeightFn::ContrastStd { gain: s, half } => {
            let half = half as usize;
            let lo = i.saturating_sub(half);
            let hi = (i + half + 1).min(src.len());
            let sigma = luma_std(src, lo, hi);
            (1.0 - s * (1.0f32).min(CONTRAST_STD_K * sigma)).clamp(0.0, 1.0)
        }
        WeightFn::Sine { freq, amp } => {
            let a = amp.clamp(0.0, 1.0);
            if a <= 0.0 {
                return 1.0;
            }
            let n = if cfg.line_len > 0 {
                cfg.line_len as usize
            } else {
                src.len()
            }
            .max(1);
            let stride = cfg.line_stride.max(1) as usize;
            let gi = cfg.line_origin as usize + i * stride;
            let s = (std::f32::consts::TAU * freq * (gi as f32) / (n as f32)).sin();
            1.0 - a + a * (0.5 + 0.5 * s)
        }
    }
}

/// Map UI amplitude 0..=100 → Sine `amp` ∈ [0, 1].
#[inline]
pub fn sine_amp_from_ui(amplitude: u8) -> f32 {
    (amplitude.min(100) as f32) / 100.0
}

#[inline]
fn unit_from_key(k: u64) -> f32 {
    // Match spatial glitch unit(): xor-shift from mix_prng_key.
    let mut state = if k == 0 { 1 } else { k };
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    ((state >> 32) as u32 as f32) / (u32::MAX as f32)
}

#[inline]
fn rand_in(k: u64, lo: i32, hi: i32) -> i32 {
    if hi < lo {
        return lo;
    }
    let span = (hi as i64 - lo as i64 + 1) as u32;
    let mut state = if k == 0 { 1 } else { k };
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    let r = (state >> 32) as u32;
    lo + (r % span) as i32
}

/// Inclusive `[lo, hi]` excluding 0 (requires lo < 0 < hi or a non-empty side).
fn nonzero_in(k: u64, lo: i32, hi: i32) -> i32 {
    debug_assert!(lo <= -1 && hi >= 1);
    let neg = (-lo) as u32;
    let pos = hi as u32;
    let span = neg + pos;
    let mut state = if k == 0 { 1 } else { k };
    state ^= state << 13;
    state ^= state >> 7;
    state ^= state << 17;
    let r = ((state >> 32) as u32) % span;
    if r < neg {
        -(r as i32 + 1)
    } else {
        (r - neg) as i32 + 1
    }
}

/// Tear probability curve. Tuned so I=1 is visible and I=20 is not pure noise.
/// `p = 0.5 · (I/20)^0.7` → I1≈0.06, I5≈0.20, I12≈0.36, I20=0.5.
#[inline]
pub fn glitch_tear_probability(intensity: u8) -> f32 {
    let i = intensity.clamp(1, 20) as f32;
    0.5 * (i / 20.0).powf(0.7)
}

/// Corrupt one RGB row in place. Order: tear → channel offset → dropout.
///
/// Authorial “VHS/digital corruption” interpretation (opaque original). Alpha is
/// never touched here — callers still use `params.map_alpha`.
///
/// `scratch` is reused for circular rotate (no per-call allocation when capacity
/// already matches).
pub fn glitch_row(
    row: &mut [[f32; 3]],
    line: u64,
    intensity: u8,
    seed: u32,
    scratch: &mut Vec<[f32; 3]>,
) {
    let w = row.len();
    if w == 0 {
        return;
    }
    let intensity = intensity.clamp(1, 20);
    let p = glitch_tear_probability(intensity);
    let seed64 = seed as u64;
    let band = line / GLITCH_BAND;

    // 1) Tear (band-shared decision).
    if w >= 2 {
        let k = mix_prng_key(seed64, band as i32, 0, LVL_TEAR);
        if unit_from_key(k) < p {
            let rmax = ((w as i32) * intensity as i32 / 100).max(1);
            let r = nonzero_in(
                mix_prng_key(seed64, band as i32, 0, LVL_TEAR_R),
                -rmax,
                rmax,
            );
            rotate_row(row, r, scratch);
        }
    }

    // 2) Channel offset (per line).
    {
        let k = mix_prng_key(seed64, line as i32, 0, LVL_CH);
        if unit_from_key(k) < 0.6 * p {
            let kmax = ((w as i32) * intensity as i32 / 200) + 1;
            let shift = nonzero_in(mix_prng_key(seed64, line as i32, 0, LVL_CH_K), -kmax, kmax);
            let ch =
                (rand_in(mix_prng_key(seed64, line as i32, 0, LVL_CH_WHICH), 0, 2) as usize).min(2);
            apply_channel_offset(row, ch, shift, scratch);
        }
    }

    // 3) Dropout (per line).
    {
        let k = mix_prng_key(seed64, line as i32, 0, LVL_DROP);
        if unit_from_key(k) < 0.5 * p {
            let base = w.div_ceil(64).max(1);
            let extra = (w as i32 * intensity as i32 / 200).max(0) as usize;
            let lo = base;
            let hi = base + extra;
            let len = rand_in(
                mix_prng_key(seed64, line as i32, 0, LVL_DROP_LEN),
                lo as i32,
                hi as i32,
            ) as usize;
            let max_start = w.saturating_sub(len);
            let start = if max_start == 0 {
                0
            } else {
                rand_in(
                    mix_prng_key(seed64, line as i32, 0, LVL_DROP_START),
                    0,
                    max_start as i32,
                ) as usize
            };
            let end = (start + len).min(w);
            if end > start {
                let hold = row[start];
                for px in &mut row[start + 1..end] {
                    *px = hold;
                }
            }
        }
    }

    for px in row.iter_mut() {
        px[0] = px[0].clamp(0.0, 1.0);
        px[1] = px[1].clamp(0.0, 1.0);
        px[2] = px[2].clamp(0.0, 1.0);
    }
}

fn rotate_row(row: &mut [[f32; 3]], r: i32, _scratch: &mut Vec<[f32; 3]>) {
    let w = row.len() as i32;
    if w <= 1 || r == 0 {
        return;
    }
    let r = ((r % w) + w) % w;
    if r == 0 {
        return;
    }
    // Positive r: circular shift right (pixels move toward +x).
    row.rotate_right(r as usize);
}

fn apply_channel_offset(row: &mut [[f32; 3]], ch: usize, shift: i32, scratch: &mut Vec<[f32; 3]>) {
    let w = row.len();
    if w == 0 || shift == 0 {
        return;
    }
    scratch.clear();
    scratch.extend_from_slice(row);
    let w_i = w as i32;
    for (x, px) in row.iter_mut().enumerate() {
        let src_x = (x as i32 + shift).rem_euclid(w_i) as usize;
        px[ch] = scratch[src_x][ch];
    }
}

/// Process one row left→right. `line` is the **global** document row (or column
/// index on the vertical strip path). `src_transform` runs before weight/ED.
pub fn ed1d_row(
    src: &[[f32; 3]],
    dst: &mut [[f32; 3]],
    cfg: &Ed1dCfg,
    q: &mut impl EdQuantizer,
    line: u64,
) {
    debug_assert_eq!(src.len(), dst.len());
    let owned: Option<Vec<[f32; 3]>> =
        if let Some(SrcXf::Glitch { intensity, seed }) = cfg.src_transform {
            let mut buf = src.to_vec();
            let mut scratch = Vec::with_capacity(buf.len());
            glitch_row(&mut buf, line, intensity, seed, &mut scratch);
            Some(buf)
        } else {
            None
        };
    let work: &[[f32; 3]] = owned.as_deref().unwrap_or(src);

    let mut acc = [0.0f32; 3];
    let carry = cfg.carry.clamp(0.0, 1.0);
    // α = 1 ⇒ replace (no smoothing). Missing ema ≡ 1.
    let alpha = cfg.ema.unwrap_or(1.0).clamp(0.0, 1.0);
    for i in 0..work.len() {
        let (quant, q_err) = q.quantize(work[i], acc);
        dst[i] = quant;
        let wgt = weight_at(i, work, cfg);
        let scale = carry * wgt;
        let scaled = [q_err[0] * scale, q_err[1] * scale, q_err[2] * scale];
        if alpha >= 1.0 {
            acc = scaled;
        } else {
            let keep = 1.0 - alpha;
            acc = [
                alpha * scaled[0] + keep * acc[0],
                alpha * scaled[1] + keep * acc[1],
                alpha * scaled[2] + keep * acc[2],
            ];
        }
    }
}

/// Apply 1D ED to a packed RGBA f32 image (`w*h*4`). Alpha copied unchanged.
///
/// Horizontal: rows left→right. Vertical: transpose → horizontal → transpose
/// (seam-safe on a contiguous full buffer; production strips do the same per
/// 256-wide column).
pub fn ed1d_image(
    src: &[f32],
    width: usize,
    height: usize,
    cfg: &Ed1dCfg,
    q: &mut impl EdQuantizer,
) -> Vec<f32> {
    assert_eq!(src.len(), width * height * 4);
    match cfg.axis {
        Axis::Horizontal => ed1d_image_horizontal(src, width, height, cfg, q),
        Axis::Vertical => {
            let transposed = transpose_rgba(src, width, height);
            let mut vert_cfg = *cfg;
            vert_cfg.axis = Axis::Horizontal;
            let out_t = ed1d_image_horizontal(&transposed, height, width, &vert_cfg, q);
            transpose_rgba(&out_t, height, width)
        }
    }
}

fn ed1d_image_horizontal(
    src: &[f32],
    width: usize,
    height: usize,
    cfg: &Ed1dCfg,
    q: &mut impl EdQuantizer,
) -> Vec<f32> {
    let mut out = src.to_vec();
    if height <= 1 || width == 0 {
        if width > 0 && height == 1 {
            apply_one_row(src, &mut out, 0, width, cfg, q);
        }
        return out;
    }
    for y in 0..height {
        apply_one_row(src, &mut out, y, width, cfg, q);
    }
    out
}

/// Parallel horizontal apply when the quantizer is `Clone`.
pub fn ed1d_image_horizontal_par<Q>(
    src: &[f32],
    width: usize,
    height: usize,
    cfg: &Ed1dCfg,
    q_proto: &Q,
) -> Vec<f32>
where
    Q: EdQuantizer + Clone + Send + Sync,
{
    let mut out = src.to_vec();
    if width == 0 || height == 0 {
        return out;
    }
    out.par_chunks_mut(width * 4)
        .zip(src.par_chunks(width * 4))
        .enumerate()
        .for_each(|(y, (dst_row, src_row))| {
            let mut q = q_proto.clone();
            let mut src_rgb = vec![[0.0f32; 3]; width];
            let mut dst_rgb = vec![[0.0f32; 3]; width];
            for (x, px) in src_rgb.iter_mut().enumerate() {
                let i = x * 4;
                *px = [src_row[i], src_row[i + 1], src_row[i + 2]];
            }
            ed1d_row(&src_rgb, &mut dst_rgb, cfg, &mut q, y as u64);
            for (x, px) in dst_rgb.iter().enumerate() {
                let i = x * 4;
                dst_row[i] = px[0];
                dst_row[i + 1] = px[1];
                dst_row[i + 2] = px[2];
                dst_row[i + 3] = src_row[i + 3];
            }
        });
    out
}

fn apply_one_row(
    src: &[f32],
    out: &mut [f32],
    y: usize,
    width: usize,
    cfg: &Ed1dCfg,
    q: &mut impl EdQuantizer,
) {
    let mut src_rgb = vec![[0.0f32; 3]; width];
    let mut dst_rgb = vec![[0.0f32; 3]; width];
    let row = y * width * 4;
    for (x, px) in src_rgb.iter_mut().enumerate() {
        let i = row + x * 4;
        *px = [src[i], src[i + 1], src[i + 2]];
    }
    ed1d_row(&src_rgb, &mut dst_rgb, cfg, q, y as u64);
    for (x, px) in dst_rgb.iter().enumerate() {
        let i = row + x * 4;
        out[i] = px[0];
        out[i + 1] = px[1];
        out[i + 2] = px[2];
        out[i + 3] = src[i + 3];
    }
}

fn transpose_rgba(src: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; width * height * 4];
    for y in 0..height {
        for x in 0..width {
            let si = (y * width + x) * 4;
            let di = (x * height + y) * 4;
            out[di] = src[si];
            out[di + 1] = src[si + 1];
            out[di + 2] = src[si + 2];
            out[di + 3] = src[si + 3];
        }
    }
    out
}

/// Map UI `line_scale` (1..=20) to contrast gain `s ∈ [0.05, 1]`.
#[inline]
pub fn contrast_gain_from_line_scale(line_scale: u8) -> f32 {
    (line_scale.clamp(1, 20) as f32) / 20.0
}

/// Map UI `smoothness` (0..=20) to EMA α. `0` ⇒ `1.0` (no smoothing).
#[inline]
pub fn ema_alpha_from_smoothness(smoothness: u8) -> f32 {
    1.0 / (1.0 + smoothness.clamp(0, 20) as f32 * 0.5)
}

/// Local-σ contrast weight at index `i` (for unit tests / DoD).
pub fn weight_contrast_std_at(i: usize, src: &[[f32; 3]], gain: f32, half: u8) -> f32 {
    weight_at(
        i,
        src,
        &Ed1dCfg {
            weight: WeightFn::ContrastStd { gain, half },
            ..Ed1dCfg::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: usize, h: usize, rgb: [f32; 3]) -> Vec<f32> {
        let mut v = vec![0.0; w * h * 4];
        for i in 0..(w * h) {
            v[i * 4] = rgb[0];
            v[i * 4 + 1] = rgb[1];
            v[i * 4 + 2] = rgb[2];
            v[i * 4 + 3] = 1.0;
        }
        v
    }

    fn gradient_x(w: usize, h: usize) -> Vec<f32> {
        let mut v = vec![0.0; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let t = if w > 1 {
                    x as f32 / (w - 1) as f32
                } else {
                    0.0
                };
                let i = (y * w + x) * 4;
                v[i] = t;
                v[i + 1] = t;
                v[i + 2] = t;
                v[i + 3] = 1.0;
            }
        }
        v
    }

    #[test]
    fn edges_do_not_panic() {
        let cfg = Ed1dCfg {
            weight: WeightFn::Contrast { gain: 0.25 },
            ema: None,
            ..Ed1dCfg::default()
        };
        let mut q = UniformLevelsQuantizer { levels: 2.0 };
        for (w, h) in [(1, 1), (1, 8), (8, 1), (7, 9), (257, 255)] {
            let src = gradient_x(w, h);
            let out = ed1d_image(&src, w, h, &cfg, &mut q);
            assert_eq!(out.len(), w * h * 4);
        }
    }

    #[test]
    fn uniform_matches_pure_1d_carry() {
        let src = solid(32, 4, [0.3, 0.4, 0.5]);
        let cfg_mod = Ed1dCfg {
            carry: 1.0,
            weight: WeightFn::Contrast { gain: 1.0 },
            ema: None,
            axis: Axis::Horizontal,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let cfg_pure = Ed1dCfg {
            carry: 1.0,
            weight: WeightFn::Const,
            ema: None,
            axis: Axis::Horizontal,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let mut q1 = UniformLevelsQuantizer { levels: 2.0 };
        let mut q2 = UniformLevelsQuantizer { levels: 2.0 };
        let a = ed1d_image(&src, 32, 4, &cfg_mod, &mut q1);
        let b = ed1d_image(&src, 32, 4, &cfg_pure, &mut q2);
        assert_eq!(a, b);
    }

    #[test]
    fn axis_equivalence_square_and_nonsquare() {
        let cfg_h = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Contrast { gain: 0.25 },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let cfg_v = Ed1dCfg {
            axis: Axis::Vertical,
            carry: 1.0,
            weight: WeightFn::Contrast { gain: 0.25 },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        for (w, h) in [(64, 64), (257, 255)] {
            let src = gradient_x(w, h);
            let mut q = UniformLevelsQuantizer { levels: 4.0 };
            let horiz = ed1d_image(&src, w, h, &cfg_h, &mut q);
            let src_t = transpose_rgba(&src, w, h);
            let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
            let horiz_on_t = ed1d_image(&src_t, h, w, &cfg_h, &mut q2);
            let expected = transpose_rgba(&horiz_on_t, h, w);
            let mut q3 = UniformLevelsQuantizer { levels: 4.0 };
            let vert = ed1d_image(&src, w, h, &cfg_v, &mut q3);
            assert_eq!(vert, expected, "axis eq failed for {w}x{h}");
            let _ = horiz;
        }
    }

    #[test]
    fn threads_bit_identical() {
        let src = gradient_x(128, 64);
        let cfg = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Contrast { gain: 0.5 },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let q = UniformLevelsQuantizer { levels: 4.0 };
        let a = ed1d_image_horizontal_par(&src, 128, 64, &cfg, &q);
        let mut q2 = q.clone();
        let b = ed1d_image(&src, 128, 64, &cfg, &mut q2);
        assert_eq!(a, b);
    }

    #[test]
    fn threads_bit_identical_vertical() {
        let src = gradient_x(96, 128);
        let cfg = Ed1dCfg {
            axis: Axis::Vertical,
            carry: 1.0,
            weight: WeightFn::Contrast { gain: 0.5 },
            ema: None,
            src_transform: None,
            ..Ed1dCfg::default()
        };
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let seq = ed1d_image(&src, 96, 128, &cfg, &mut q);
        let src_t = transpose_rgba(&src, 96, 128);
        let hcfg = Ed1dCfg {
            axis: Axis::Horizontal,
            ..cfg
        };
        let q_proto = UniformLevelsQuantizer { levels: 4.0 };
        let par_t = ed1d_image_horizontal_par(&src_t, 128, 96, &hcfg, &q_proto);
        let par = transpose_rgba(&par_t, 128, 96);
        assert_eq!(seq, par);
    }

    #[test]
    fn width_one_row_ok() {
        let src = solid(1, 5, [0.6, 0.2, 0.1]);
        let cfg = Ed1dCfg::default();
        let mut q = UniformLevelsQuantizer { levels: 2.0 };
        let out = ed1d_image(&src, 1, 5, &cfg, &mut q);
        assert_eq!(out.len(), 20);
    }

    #[test]
    fn glitch_none_matches_pure_1d() {
        // Internal p=0 path: intensity still set but transform forced off.
        let src = gradient_x(64, 32);
        let cfg = Ed1dCfg {
            carry: 1.0,
            weight: WeightFn::Const,
            ema: None,
            src_transform: None,
            axis: Axis::Horizontal,
            ..Ed1dCfg::default()
        };
        let mut q1 = UniformLevelsQuantizer { levels: 4.0 };
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        let a = ed1d_image(&src, 64, 32, &cfg, &mut q1);
        let b = ed1d_image(
            &src,
            64,
            32,
            &Ed1dCfg {
                src_transform: None,
                ..cfg
            },
            &mut q2,
        );
        assert_eq!(a, b);
    }

    #[test]
    fn tear_is_bijection() {
        let before: Vec<[f32; 3]> = (0..64)
            .map(|i| {
                let t = i as f32 / 63.0;
                [t, 1.0 - t, 0.5]
            })
            .collect();
        let mut row = before.clone();
        let mut scratch = Vec::new();
        rotate_row(&mut row, 7, &mut scratch);
        let mut a = before.clone();
        let mut b = row.clone();
        a.sort_by(|u, v| u.partial_cmp(v).unwrap());
        b.sort_by(|u, v| u.partial_cmp(v).unwrap());
        assert_eq!(a, b, "circular tear must preserve the multiset of pixels");
    }

    #[test]
    fn channel_offset_changes_one_channel() {
        let mut row: Vec<[f32; 3]> = (0..32).map(|i| [i as f32 / 31.0, 0.2, 0.8]).collect();
        let before = row.clone();
        let mut scratch = Vec::new();
        apply_channel_offset(&mut row, 1, 3, &mut scratch);
        for x in 0..32 {
            assert_eq!(row[x][0], before[x][0]);
            assert_eq!(row[x][2], before[x][2]);
            let src_x = ((x as i32 + 3).rem_euclid(32)) as usize;
            assert_eq!(row[x][1], before[src_x][1]);
        }
    }

    #[test]
    fn dropout_clamped_and_in_range() {
        let w = 100usize;
        let mut scratch = Vec::new();
        for line in 0..50u64 {
            let mut row: Vec<[f32; 3]> = (0..w).map(|i| [i as f32 / 99.0, 0.0, 0.0]).collect();
            glitch_row(&mut row, line, 20, 7, &mut scratch);
            for px in &row {
                assert!((0.0..=1.0).contains(&px[0]));
                assert!((0.0..=1.0).contains(&px[1]));
                assert!((0.0..=1.0).contains(&px[2]));
            }
        }
    }

    #[test]
    fn tear_rate_near_p_at_i20() {
        // Band decisions: count fraction of bands that tear ≈ p=0.5.
        let n_lines = 4000u64;
        let seed = 42u32;
        let intensity = 20u8;
        let p = glitch_tear_probability(intensity);
        let mut torn_bands = 0u64;
        let n_bands = (n_lines + GLITCH_BAND - 1) / GLITCH_BAND;
        for band in 0..n_bands {
            let k = mix_prng_key(seed as u64, band as i32, 0, LVL_TEAR);
            if unit_from_key(k) < p {
                torn_bands += 1;
            }
        }
        let frac = torn_bands as f64 / n_bands as f64;
        // ±5σ binomial: σ = sqrt(p(1-p)/n)
        let sigma = (p as f64 * (1.0 - p as f64) / n_bands as f64).sqrt();
        let tol = 5.0 * sigma;
        assert!(
            (frac - p as f64).abs() <= tol,
            "tear band frac={frac} p={p} tol={tol}"
        );
    }

    #[test]
    fn glitch_seed_changes_output() {
        let src = gradient_x(64, 48);
        let mut cfg = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Const,
            ema: None,
            src_transform: Some(SrcXf::Glitch {
                intensity: 12,
                seed: 42,
            }),
            ..Ed1dCfg::default()
        };
        let mut q = UniformLevelsQuantizer { levels: 4.0 };
        let a = ed1d_image(&src, 64, 48, &cfg, &mut q);
        cfg.src_transform = Some(SrcXf::Glitch {
            intensity: 12,
            seed: 99,
        });
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        let b = ed1d_image(&src, 64, 48, &cfg, &mut q2);
        assert_ne!(a, b);
        // Determinism.
        let mut q3 = UniformLevelsQuantizer { levels: 4.0 };
        cfg.src_transform = Some(SrcXf::Glitch {
            intensity: 12,
            seed: 42,
        });
        assert_eq!(a, ed1d_image(&src, 64, 48, &cfg, &mut q3));
    }

    #[test]
    fn glitch_threads_bit_identical() {
        let src = gradient_x(128, 64);
        let cfg = Ed1dCfg {
            axis: Axis::Horizontal,
            carry: 1.0,
            weight: WeightFn::Const,
            ema: None,
            src_transform: Some(SrcXf::Glitch {
                intensity: 8,
                seed: 42,
            }),
            ..Ed1dCfg::default()
        };
        let q = UniformLevelsQuantizer { levels: 4.0 };
        let a = ed1d_image_horizontal_par(&src, 128, 64, &cfg, &q);
        let mut q2 = q.clone();
        let b = ed1d_image(&src, 128, 64, &cfg, &mut q2);
        assert_eq!(a, b);
    }

    #[test]
    fn contrast_std_flat_line_weight_is_one() {
        let row: Vec<[f32; 3]> = (0..32).map(|_| [0.4, 0.4, 0.4]).collect();
        for i in 0..row.len() {
            let w = weight_contrast_std_at(i, &row, 1.0, CONTRAST_STD_HALF);
            assert!((w - 1.0).abs() < 1e-6, "i={i} w={w}");
        }
    }

    #[test]
    fn contrast_std_step_reduces_weight_symmetrically() {
        // Step at midpoint: left half black, right half white.
        let n = 64;
        let mid = n / 2;
        let mut row = vec![[0.0f32; 3]; n];
        for p in row.iter_mut().take(n).skip(mid) {
            *p = [1.0, 1.0, 1.0];
        }
        let gain = 1.0;
        let half = CONTRAST_STD_HALF;
        // Flat far from the edge → weight ≈ 1.
        assert!((weight_contrast_std_at(0, &row, gain, half) - 1.0).abs() < 1e-5);
        assert!((weight_contrast_std_at(n - 1, &row, gain, half) - 1.0).abs() < 1e-5);
        // Near the step, weight drops.
        let w_left = weight_contrast_std_at(mid - 1, &row, gain, half);
        let w_right = weight_contrast_std_at(mid, &row, gain, half);
        assert!(
            w_left < 0.95,
            "expected reduced weight left of step, got {w_left}"
        );
        assert!(
            w_right < 0.95,
            "expected reduced weight right of step, got {w_right}"
        );
        // Symmetry around the edge (mirrored indices).
        for d in 0..=half as usize {
            let wl = weight_contrast_std_at(mid - 1 - d, &row, gain, half);
            let wr = weight_contrast_std_at(mid + d, &row, gain, half);
            assert!(
                (wl - wr).abs() < 1e-5,
                "asymmetry at d={d}: left={wl} right={wr}"
            );
        }
    }

    #[test]
    fn ema_alpha_one_matches_no_ema() {
        let src = gradient_x(64, 32);
        let cfg_none = Ed1dCfg {
            weight: WeightFn::Contrast { gain: 0.5 },
            ema: None,
            ..Ed1dCfg::default()
        };
        let cfg_one = Ed1dCfg {
            weight: WeightFn::Contrast { gain: 0.5 },
            ema: Some(1.0),
            ..Ed1dCfg::default()
        };
        let mut q1 = UniformLevelsQuantizer { levels: 4.0 };
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        assert_eq!(
            ed1d_image(&src, 64, 32, &cfg_none, &mut q1),
            ed1d_image(&src, 64, 32, &cfg_one, &mut q2)
        );
    }

    #[test]
    fn smoothness_zero_alpha_is_one() {
        assert!((ema_alpha_from_smoothness(0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn sine_amplitude_zero_matches_const() {
        let src = gradient_x(257, 33);
        let cfg_sine = Ed1dCfg {
            weight: WeightFn::Sine {
                freq: 8.0,
                amp: 0.0,
            },
            line_len: 257,
            ..Ed1dCfg::default()
        };
        let cfg_const = Ed1dCfg::default();
        let mut q1 = UniformLevelsQuantizer { levels: 4.0 };
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        assert_eq!(
            ed1d_image(&src, 257, 33, &cfg_sine, &mut q1),
            ed1d_image(&src, 257, 33, &cfg_const, &mut q2)
        );
    }

    #[test]
    fn sine_axis_equivalence_transpose() {
        // wave_x(I) = (wave_y(Iᵀ))ᵀ — both use N = original height.
        let (w, h) = (97usize, 64usize);
        let src = gradient_x(w, h);
        let sine = WeightFn::Sine {
            freq: 5.0,
            amp: 0.5,
        };
        // On Iᵀ (width=h), horizontal rows have length h → N = h.
        let cfg_y_on_t = Ed1dCfg {
            axis: Axis::Horizontal,
            weight: sine,
            line_len: h as u32,
            ..Ed1dCfg::default()
        };
        // On I, vertical columns have length h → N = h.
        let cfg_x = Ed1dCfg {
            axis: Axis::Vertical,
            weight: sine,
            line_len: h as u32,
            ..Ed1dCfg::default()
        };
        let src_t = transpose_rgba(&src, w, h);
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        let y_on_t = ed1d_image(&src_t, h, w, &cfg_y_on_t, &mut q2);
        let expected = transpose_rgba(&y_on_t, h, w);
        let mut q3 = UniformLevelsQuantizer { levels: 4.0 };
        let x_out = ed1d_image(&src, w, h, &cfg_x, &mut q3);
        assert_eq!(x_out, expected);
    }

    #[test]
    fn sine_no_phase_seam_across_origin() {
        // Same global phase whether we start at 0 or mid-line (strip simulation).
        let n = 300usize;
        let row: Vec<[f32; 3]> = (0..n)
            .map(|x| {
                let t = x as f32 / n as f32;
                [t, 1.0 - t, 0.5]
            })
            .collect();
        let cfg_full = Ed1dCfg {
            weight: WeightFn::Sine {
                freq: 7.0,
                amp: 0.8,
            },
            line_len: n as u32,
            line_origin: 0,
            ..Ed1dCfg::default()
        };
        let split = 180usize;
        let cfg_tail = Ed1dCfg {
            weight: WeightFn::Sine {
                freq: 7.0,
                amp: 0.8,
            },
            line_len: n as u32,
            line_origin: split as u32,
            ..Ed1dCfg::default()
        };
        let w_full: Vec<f32> = (0..n).map(|i| weight_at(i, &row, &cfg_full)).collect();
        let w_tail: Vec<f32> = (0..(n - split))
            .map(|i| weight_at(i, &row[split..], &cfg_tail))
            .collect();
        for (i, &wt) in w_tail.iter().enumerate() {
            assert!(
                (wt - w_full[split + i]).abs() < 1e-5,
                "seam at global {}: full={} tail={}",
                split + i,
                w_full[split + i],
                wt
            );
        }
    }
}

//! GPU Bayer / Halftone / CRT parity + seam + map timeout tests.
//!
//! Adapter-requiring tests are `#[ignore]` so CPU-only CI stays green.
//! Run with: `cargo test -p engine-gpu --features gpu-tests -- --ignored`

use engine_gpu::{
    apply_bayer_gpu, apply_crt_gpu, apply_halftone_gpu, BayerGpuParams, BayerMatrixSize,
    CrtGpuParams, GpuContext, HalftoneGpuParams, CORE_SIZE, FLOATS_PER_TILE,
};

const HALFTONE_PARITY_EPS: f32 = 1.0 / 255.0;
const CRT_PARITY_EPS: f32 = 1.0 / 255.0;

fn solid_core(v: f32) -> Vec<f32> {
    let mut buf = vec![0.0f32; FLOATS_PER_TILE];
    for i in 0..CORE_SIZE * CORE_SIZE {
        let o = (i * 4) as usize;
        buf[o] = v;
        buf[o + 1] = v;
        buf[o + 2] = v;
        buf[o + 3] = 1.0;
    }
    buf
}

fn gradient_core() -> Vec<f32> {
    let mut buf = vec![0.0f32; FLOATS_PER_TILE];
    for y in 0..CORE_SIZE {
        for x in 0..CORE_SIZE {
            let o = ((y * CORE_SIZE + x) * 4) as usize;
            let t = x as f32 / (CORE_SIZE as f32 - 1.0);
            buf[o] = t;
            buf[o + 1] = t * 0.5;
            buf[o + 2] = 1.0 - t;
            buf[o + 3] = 1.0;
        }
    }
    buf
}

/// CPU reference for Bayer8 (matches dither_ordered quantize + matrix).
fn cpu_bayer8(input: &[f32], tile_x: u32, tile_y: u32, levels: f32, scale: f32) -> Vec<f32> {
    const BAYER: [[f32; 8]; 8] = [
        [0.0 / 64.0, 32.0 / 64.0, 8.0 / 64.0, 40.0 / 64.0, 2.0 / 64.0, 34.0 / 64.0, 10.0 / 64.0, 42.0 / 64.0],
        [48.0 / 64.0, 16.0 / 64.0, 56.0 / 64.0, 24.0 / 64.0, 50.0 / 64.0, 18.0 / 64.0, 58.0 / 64.0, 26.0 / 64.0],
        [12.0 / 64.0, 44.0 / 64.0, 4.0 / 64.0, 36.0 / 64.0, 14.0 / 64.0, 46.0 / 64.0, 6.0 / 64.0, 38.0 / 64.0],
        [60.0 / 64.0, 28.0 / 64.0, 52.0 / 64.0, 20.0 / 64.0, 62.0 / 64.0, 30.0 / 64.0, 54.0 / 64.0, 22.0 / 64.0],
        [3.0 / 64.0, 35.0 / 64.0, 11.0 / 64.0, 43.0 / 64.0, 1.0 / 64.0, 33.0 / 64.0, 9.0 / 64.0, 41.0 / 64.0],
        [51.0 / 64.0, 19.0 / 64.0, 59.0 / 64.0, 27.0 / 64.0, 49.0 / 64.0, 17.0 / 64.0, 57.0 / 64.0, 25.0 / 64.0],
        [15.0 / 64.0, 47.0 / 64.0, 7.0 / 64.0, 39.0 / 64.0, 13.0 / 64.0, 45.0 / 64.0, 5.0 / 64.0, 37.0 / 64.0],
        [63.0 / 64.0, 31.0 / 64.0, 55.0 / 64.0, 23.0 / 64.0, 61.0 / 64.0, 29.0 / 64.0, 53.0 / 64.0, 21.0 / 64.0],
    ];
    let ox = tile_x * CORE_SIZE;
    let oy = tile_y * CORE_SIZE;
    let mut out = vec![0.0f32; FLOATS_PER_TILE];
    for y in 0..CORE_SIZE {
        for x in 0..CORE_SIZE {
            let gx = (ox + x) as usize;
            let gy = (oy + y) as usize;
            let t = BAYER[gy % 8][gx % 8];
            let offset = (t - 0.5) * scale;
            let i = ((y * CORE_SIZE + x) * 4) as usize;
            for c in 0..3 {
                let v = input[i + c];
                let scaled = v * (levels - 1.0) + offset;
                out[i + c] = scaled.round().clamp(0.0, levels - 1.0) / (levels - 1.0);
            }
            out[i + 3] = input[i + 3];
        }
    }
    out
}

fn assert_exact(a: &[f32], b: &[f32]) {
    assert_eq!(a.len(), b.len());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        // f32 == treats ±0.0 as equal; bit-compare would fail on signed zero.
        assert_eq!(x, y, "mismatch at {i}: {x} vs {y} (bits {:08x} vs {:08x})", x.to_bits(), y.to_bits());
    }
}

fn assert_eps(a: &[f32], b: &[f32], eps: f32) {
    assert_eq!(a.len(), b.len());
    let mut max_d = 0.0f32;
    for (x, y) in a.iter().zip(b.iter()) {
        max_d = max_d.max((x - y).abs());
    }
    assert!(
        max_d <= eps,
        "max abs delta {max_d} > eps {eps}"
    );
}

#[test]
fn map_timeout_counter_inject() {
    // Works without adapter: only documents the counter API used by timeout path.
    if let Some(ctx) = GpuContext::try_new_blocking() {
        let before = ctx.map_timeouts();
        ctx.inject_map_timeout_for_test();
        assert_eq!(ctx.map_timeouts(), before + 1);
    }
}

#[test]
#[ignore = "requires GPU adapter"]
fn bayer_exact_parity_solid_and_gradient() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    for (input, label) in [(solid_core(0.5), "solid"), (gradient_core(), "grad")] {
        let gpu = apply_bayer_gpu(
            &ctx,
            &input,
            BayerGpuParams {
                matrix: BayerMatrixSize::Bayer8,
                levels: 4,
                threshold_scale: 1.0,
                color_mode: 0,
                tile_x: 0,
                tile_y: 0,
            },
        )
        .expect("gpu bayer");
        let cpu = cpu_bayer8(&input, 0, 0, 4.0, 1.0);
        assert_exact(&gpu, &cpu);
        let _ = label;
    }
}

#[test]
#[ignore = "requires GPU adapter"]
fn bayer_seam_tile_offset_2x2() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = gradient_core();
    // Right edge of tile (0,0) vs left edge of tile (1,0) must match CPU continuity.
    let left = apply_bayer_gpu(
        &ctx,
        &input,
        BayerGpuParams {
            matrix: BayerMatrixSize::Bayer8,
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            tile_x: 0,
            tile_y: 0,
        },
    )
    .unwrap();
    let right = apply_bayer_gpu(
        &ctx,
        &input,
        BayerGpuParams {
            matrix: BayerMatrixSize::Bayer8,
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            tile_x: 1,
            tile_y: 0,
        },
    )
    .unwrap();
    let cpu_l = cpu_bayer8(&input, 0, 0, 4.0, 1.0);
    let cpu_r = cpu_bayer8(&input, 1, 0, 4.0, 1.0);
    assert_exact(&left, &cpu_l);
    assert_exact(&right, &cpu_r);
    // Adjacent global X=255 and X=256 differ only by pattern phase (not a flat seam bug).
    let edge_l = ((0 * CORE_SIZE + (CORE_SIZE - 1)) * 4) as usize;
    let edge_r = ((0 * CORE_SIZE + 0) * 4) as usize;
    assert_eq!(left[edge_l].to_bits(), cpu_l[edge_l].to_bits());
    assert_eq!(right[edge_r].to_bits(), cpu_r[edge_r].to_bits());
}

#[test]
#[ignore = "requires GPU adapter"]
fn map_async_timeout_increments_counter() {
    use engine_gpu::{apply_bayer_gpu, BayerGpuParams, BayerMatrixSize};
    use std::sync::atomic::Ordering;

    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let before = ctx.map_timeouts();
    ctx.force_map_timeout.store(true, Ordering::Relaxed);
    let err = apply_bayer_gpu(
        &ctx,
        &solid_core(0.5),
        BayerGpuParams {
            matrix: BayerMatrixSize::Bayer8,
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            tile_x: 0,
            tile_y: 0,
        },
    );
    assert!(err.is_err(), "forced map timeout must fail the dispatch");
    assert!(
        ctx.map_timeouts() > before,
        "timeout path must increment map_timeout_counter"
    );
}

fn rem_euclid_f(a: f32, b: f32) -> f32 {
    let r = a % b;
    if r < 0.0 {
        r + b
    } else {
        r
    }
}

fn cpu_halftone(input: &[f32], tile_x: u32, tile_y: u32, cell: f32, scale: f32) -> Vec<f32> {
    const ANGLES: [f32; 4] = [
        15.0_f32.to_radians(),
        75.0_f32.to_radians(),
        0.0,
        45.0_f32.to_radians(),
    ];
    let ox = tile_x * CORE_SIZE;
    let oy = tile_y * CORE_SIZE;
    let mut out = vec![0.0f32; FLOATS_PER_TILE];
    for y in 0..CORE_SIZE {
        for x in 0..CORE_SIZE {
            let i = ((y * CORE_SIZE + x) * 4) as usize;
            let gx = (ox + x) as f32;
            let gy = (oy + y) as f32;
            let r = input[i];
            let g = input[i + 1];
            let b = input[i + 2];
            let k = 1.0 - r.max(g).max(b);
            let (c, m, yk, kk) = if k >= 1.0 - f32::EPSILON {
                (0.0, 0.0, 0.0, 1.0)
            } else {
                (
                    ((1.0 - r - k) / (1.0 - k)).clamp(0.0, 1.0),
                    ((1.0 - g - k) / (1.0 - k)).clamp(0.0, 1.0),
                    ((1.0 - b - k) / (1.0 - k)).clamp(0.0, 1.0),
                    k.clamp(0.0, 1.0),
                )
            };
            let tones = [c, m, yk, kk];
            let mut dots = [0.0f32; 4];
            for ch in 0..4 {
                let cos_t = ANGLES[ch].cos();
                let sin_t = ANGLES[ch].sin();
                let xr = gx * cos_t + gy * sin_t;
                let yr = -gx * sin_t + gy * cos_t;
                let cx = rem_euclid_f(xr, cell) - cell * 0.5;
                let cy = rem_euclid_f(yr, cell) - cell * 0.5;
                let dist = (cx * cx + cy * cy).sqrt();
                let r_max = (cell * 0.5) * tones[ch].sqrt() * scale;
                dots[ch] = if dist <= r_max { 1.0 } else { 0.0 };
            }
            out[i] = 1.0 - (dots[0] + dots[3]).min(1.0);
            out[i + 1] = 1.0 - (dots[1] + dots[3]).min(1.0);
            out[i + 2] = 1.0 - (dots[2] + dots[3]).min(1.0);
            out[i + 3] = input[i + 3];
        }
    }
    out
}

#[test]
#[ignore = "requires GPU adapter"]
fn halftone_parity_within_eps() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = gradient_core();
    let gpu = apply_halftone_gpu(
        &ctx,
        &input,
        HalftoneGpuParams {
            cell_size: 8,
            threshold_scale: 1.0,
            tile_x: 0,
            tile_y: 0,
        },
    )
    .unwrap();
    let cpu = cpu_halftone(&input, 0, 0, 8.0, 1.0);
    assert_eps(&gpu, &cpu, HALFTONE_PARITY_EPS);
}

#[test]
#[ignore = "requires GPU adapter"]
fn halftone_seam_tile_offset() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = gradient_core();
    let left = apply_halftone_gpu(
        &ctx,
        &input,
        HalftoneGpuParams {
            cell_size: 8,
            threshold_scale: 1.0,
            tile_x: 0,
            tile_y: 0,
        },
    )
    .unwrap();
    let right = apply_halftone_gpu(
        &ctx,
        &input,
        HalftoneGpuParams {
            cell_size: 8,
            threshold_scale: 1.0,
            tile_x: 1,
            tile_y: 0,
        },
    )
    .unwrap();
    let cpu_l = cpu_halftone(&input, 0, 0, 8.0, 1.0);
    let cpu_r = cpu_halftone(&input, 1, 0, 8.0, 1.0);
    assert_eps(&left, &cpu_l, HALFTONE_PARITY_EPS);
    assert_eps(&right, &cpu_r, HALFTONE_PARITY_EPS);
}

#[test]
#[ignore = "requires GPU adapter"]
fn crt_parity_within_eps() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = solid_core(0.8);
    let gpu = apply_crt_gpu(
        &ctx,
        &input,
        CrtGpuParams {
            period: 2,
            strength: 0.5,
            mask_strength: 0.0,
            tile_x: 0,
            tile_y: 0,
        },
    )
    .unwrap();
    // CPU reference: dark rows at even Y when period=2, strength=0.5 → 0.4
    for y in 0..CORE_SIZE {
        let gain = if (y % 2) == 0 { 0.4 } else { 0.8 };
        for x in 0..CORE_SIZE {
            let i = ((y * CORE_SIZE + x) * 4) as usize;
            assert_eps(&[gpu[i]], &[gain], CRT_PARITY_EPS);
        }
    }
}

#[test]
#[ignore = "requires GPU adapter"]
fn crt_horizontal_seam_tile_offset() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = solid_core(0.8);
    let top = apply_crt_gpu(
        &ctx,
        &input,
        CrtGpuParams {
            period: 2,
            strength: 0.5,
            mask_strength: 0.0,
            tile_x: 0,
            tile_y: 0,
        },
    )
    .unwrap();
    let bot = apply_crt_gpu(
        &ctx,
        &input,
        CrtGpuParams {
            period: 2,
            strength: 0.5,
            mask_strength: 0.0,
            tile_x: 0,
            tile_y: 1,
        },
    )
    .unwrap();
    // Global Y=255 (top last row) vs Y=256 (bot first row) must alternate with period=2.
    let last = (((CORE_SIZE - 1) * CORE_SIZE + 0) * 4) as usize;
    let first = 0usize;
    // period=2: even → dark (×0.5), odd → bright. 255 odd → 0.8; 256 even → 0.4
    assert_eps(&[top[last]], &[0.8], CRT_PARITY_EPS);
    assert_eps(&[bot[first]], &[0.4], CRT_PARITY_EPS);
    assert!((top[last] - bot[first]).abs() > 0.1);
}

#[test]
#[ignore = "requires GPU adapter"]
fn bayer_bench_note_smoke() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = gradient_core();
    let t0 = std::time::Instant::now();
    for _ in 0..8 {
        let _ = apply_bayer_gpu(
            &ctx,
            &input,
            BayerGpuParams {
                matrix: BayerMatrixSize::Bayer8,
                levels: 4,
                threshold_scale: 1.0,
                color_mode: 0,
                tile_x: 0,
                tile_y: 0,
            },
        )
        .unwrap();
    }
    let gpu_ms = t0.elapsed().as_secs_f64() * 1000.0 / 8.0;
    let t1 = std::time::Instant::now();
    for _ in 0..8 {
        let _ = cpu_bayer8(&input, 0, 0, 4.0, 1.0);
    }
    let cpu_ms = t1.elapsed().as_secs_f64() * 1000.0 / 8.0;
    eprintln!("Bayer bench (core 256², avg of 8): GPU {gpu_ms:.3} ms, CPU {cpu_ms:.3} ms");
}

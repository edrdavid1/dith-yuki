//! Void-and-cluster GPU parity / cold apply bench.
//!
//! Adapter-requiring tests are `#[ignore]` so CPU-only CI stays green.
//! Run with: `cargo test -p engine-gpu --features gpu-tests -- --ignored vac`

use engine_gpu::{
    apply_void_and_cluster_gpu, void_and_cluster_ranks, GpuContext, VoidAndClusterGpuParams,
    CORE_SIZE, FLOATS_PER_TILE,
};

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

/// CPU reference matching `dither_ordered` VAC threshold + Bayer-style quantize.
fn cpu_vac(input: &[f32], tile_x: u32, tile_y: u32, levels: f32, scale: f32) -> Vec<f32> {
    let ranks = void_and_cluster_ranks();
    let n = (ranks.len()) as f32;
    let ox = tile_x * CORE_SIZE;
    let oy = tile_y * CORE_SIZE;
    let mut out = vec![0.0f32; FLOATS_PER_TILE];
    for y in 0..CORE_SIZE {
        for x in 0..CORE_SIZE {
            let gx = (ox + x) as usize;
            let gy = (oy + y) as usize;
            let t = ranks[(gy % 64) * 64 + (gx % 64)] as f32 / n;
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
        assert_eq!(
            x,
            y,
            "mismatch at {i}: {x} vs {y} (bits {:08x} vs {:08x})",
            x.to_bits(),
            y.to_bits()
        );
    }
}

#[test]
fn vac_ranks_are_permutation() {
    let r = void_and_cluster_ranks();
    assert_eq!(r.len(), 64 * 64);
    let mut seen = vec![false; r.len()];
    for &v in r {
        assert!(!seen[v as usize], "duplicate rank {v}");
        seen[v as usize] = true;
    }
}

#[test]
#[ignore = "requires GPU adapter"]
fn vac_exact_parity_solid_and_gradient() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    for (input, label) in [(solid_core(0.5), "solid"), (gradient_core(), "grad")] {
        let gpu = apply_void_and_cluster_gpu(
            &ctx,
            &input,
            VoidAndClusterGpuParams {
                levels: 4,
                threshold_scale: 1.0,
                color_mode: 0,
                tile_x: 0,
                tile_y: 0,
            },
        )
        .expect("gpu vac");
        let cpu = cpu_vac(&input, 0, 0, 4.0, 1.0);
        assert_exact(&gpu, &cpu);
        let _ = label;
    }
}

#[test]
#[ignore = "requires GPU adapter"]
fn vac_seam_tile_offset() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = gradient_core();
    let left = apply_void_and_cluster_gpu(
        &ctx,
        &input,
        VoidAndClusterGpuParams {
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            tile_x: 0,
            tile_y: 0,
        },
    )
    .unwrap();
    let right = apply_void_and_cluster_gpu(
        &ctx,
        &input,
        VoidAndClusterGpuParams {
            levels: 4,
            threshold_scale: 1.0,
            color_mode: 0,
            tile_x: 1,
            tile_y: 0,
        },
    )
    .unwrap();
    assert_exact(&left, &cpu_vac(&input, 0, 0, 4.0, 1.0));
    assert_exact(&right, &cpu_vac(&input, 1, 0, 4.0, 1.0));
}

#[test]
#[ignore = "requires GPU adapter"]
fn vac_bench_vs_cpu() {
    let ctx = GpuContext::try_new_blocking().expect("adapter");
    let input = gradient_core();
    let params = VoidAndClusterGpuParams {
        levels: 4,
        threshold_scale: 1.0,
        color_mode: 0,
        tile_x: 0,
        tile_y: 0,
    };
    // Warmup
    let _ = apply_void_and_cluster_gpu(&ctx, &input, params).unwrap();
    let n = 8;
    let t0 = std::time::Instant::now();
    for _ in 0..n {
        let _ = apply_void_and_cluster_gpu(&ctx, &input, params).unwrap();
    }
    let gpu_ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
    let t1 = std::time::Instant::now();
    for _ in 0..n {
        let _ = cpu_vac(&input, 0, 0, 4.0, 1.0);
    }
    let cpu_ms = t1.elapsed().as_secs_f64() * 1000.0 / n as f64;
    eprintln!("VAC bench (core 256², avg of {n}): GPU {gpu_ms:.3} ms, CPU {cpu_ms:.3} ms");
}

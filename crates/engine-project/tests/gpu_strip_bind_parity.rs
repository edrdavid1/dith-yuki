//! Strip-sized STORAGE bindings: force a tiny max_bind and compare to
//! single-buffer GPU + CPU on 4K. Also smoke 8192² / 16384².
//!
//! ```bash
//! cargo test -p engine-project --test gpu_strip_bind_parity --release -- --ignored --nocapture --test-threads=1
//! ```

use std::sync::atomic::AtomicBool;

use engine_gpu::{
    apply_blocked_fs_gpu, apply_blocked_fs_gpu_opts, apply_blue_noise_gpu,
    apply_blue_noise_gpu_opts, BlockedFsGpuOpts, BlueNoiseGpuOpts,
};
use engine_project::algorithms::gpu_dither_cpu::{
    blocked_fs_cpu, blue_noise_cpu, blue_noise_ranks, PaletteRgb, Rgba8, CORE, DEFAULT_SPREAD, PAD,
};

fn gray_ramp(w: usize, h: usize) -> Vec<Rgba8> {
    let mut v = Vec::with_capacity(w * h);
    let denom = (w.max(2) - 1) as u32;
    for _y in 0..h {
        for x in 0..w {
            let g = ((x as u32 * 255) / denom) as u8;
            v.push(Rgba8::new(g, g, g, 255));
        }
    }
    v
}

fn to_bytes(px: &[Rgba8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(px.len() * 4);
    for p in px {
        v.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    v
}

fn bw_pal() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn pal_rgba(pal: &[PaletteRgb]) -> Vec<[u8; 4]> {
    pal.iter().map(|p| [p.r, p.g, p.b, 255]).collect()
}

const FORCE_BIND_4MIB: u64 = 4 * 1024 * 1024;

#[test]
#[ignore = "requires GPU"]
fn blocked_fs_4k_forced_4mib_strips_match_full_and_cpu() {
    let Some((device, queue, backend)) = engine_gpu::request_blocked_fs_device() else {
        eprintln!("skip: no GPU");
        return;
    };
    eprintln!("backend={backend}");
    let side = 4096u32;
    let input = gray_ramp(side as usize, side as usize);
    let pal = bw_pal();
    let in_b = to_bytes(&input);
    let pal_b = pal_rgba(&pal);
    let cancel = AtomicBool::new(false);

    let cpu = to_bytes(&blocked_fs_cpu(&input, side, side, &pal, CORE, CORE, PAD).expect("cpu"));
    let full = apply_blocked_fs_gpu(&device, &queue, &in_b, side, side, &pal_b, &cancel)
        .expect("full gpu");
    let stripped = apply_blocked_fs_gpu_opts(
        &device,
        &queue,
        &in_b,
        side,
        side,
        &pal_b,
        &cancel,
        BlockedFsGpuOpts {
            force_strip_cores: None,
            force_max_bind: Some(FORCE_BIND_4MIB),
        },
    )
    .expect("strip gpu")
    .rgba8;

    assert_eq!(cpu, full, "full GPU ≠ CPU");
    assert_eq!(cpu, stripped, "4 MiB-stripped GPU ≠ CPU");
    assert_eq!(full, stripped, "4 MiB-stripped GPU ≠ full GPU");
    eprintln!("blocked_fs 4K: full == strips(4MiB) == CPU OK");
}

#[test]
#[ignore = "requires GPU"]
fn blue_noise_4k_forced_4mib_strips_match_full_and_cpu() {
    let Some((device, queue, backend)) = engine_gpu::request_blocked_fs_device() else {
        eprintln!("skip: no GPU");
        return;
    };
    eprintln!("backend={backend}");
    let side = 4096u32;
    let input = gray_ramp(side as usize, side as usize);
    let pal = bw_pal();
    let in_b = to_bytes(&input);
    let pal_b = pal_rgba(&pal);
    let ranks = blue_noise_ranks();
    let cancel = AtomicBool::new(false);

    let cpu =
        to_bytes(&blue_noise_cpu(&input, side, side, &pal, ranks, DEFAULT_SPREAD).expect("cpu"));
    let full = apply_blue_noise_gpu(
        &device,
        &queue,
        &in_b,
        side,
        side,
        &pal_b,
        ranks,
        DEFAULT_SPREAD,
        &cancel,
    )
    .expect("full gpu");
    let stripped = apply_blue_noise_gpu_opts(
        &device,
        &queue,
        &in_b,
        side,
        side,
        &pal_b,
        ranks,
        DEFAULT_SPREAD,
        &cancel,
        BlueNoiseGpuOpts {
            force_strip_h: None,
            force_max_bind: Some(FORCE_BIND_4MIB),
        },
    )
    .expect("strip gpu")
    .rgba8;

    assert_eq!(cpu, full, "full GPU ≠ CPU");
    assert_eq!(cpu, stripped, "4 MiB-stripped GPU ≠ CPU");
    assert_eq!(full, stripped, "4 MiB-stripped GPU ≠ full GPU");
    eprintln!("blue_noise 4K: full == strips(4MiB) == CPU OK");
}

fn huge_parity(side: u32) {
    let Some((device, queue, backend)) = engine_gpu::request_blocked_fs_device() else {
        eprintln!("skip: no GPU");
        return;
    };
    eprintln!("backend={backend} side={side}");
    let input = gray_ramp(side as usize, side as usize);
    let pal = bw_pal();
    let in_b = to_bytes(&input);
    let pal_b = pal_rgba(&pal);
    let cancel = AtomicBool::new(false);

    let cpu = to_bytes(&blocked_fs_cpu(&input, side, side, &pal, CORE, CORE, PAD).expect("cpu"));
    let gpu =
        apply_blocked_fs_gpu(&device, &queue, &in_b, side, side, &pal_b, &cancel).expect("gpu");
    assert_eq!(cpu, gpu, "blocked_fs {side}² GPU ≠ CPU");

    let ranks = blue_noise_ranks();
    let cpu_bn =
        to_bytes(&blue_noise_cpu(&input, side, side, &pal, ranks, DEFAULT_SPREAD).expect("cpu bn"));
    let gpu_bn = apply_blue_noise_gpu(
        &device,
        &queue,
        &in_b,
        side,
        side,
        &pal_b,
        ranks,
        DEFAULT_SPREAD,
        &cancel,
    )
    .expect("gpu bn");
    assert_eq!(cpu_bn, gpu_bn, "blue_noise {side}² GPU ≠ CPU");
    eprintln!("{side}² both families OK");
}

#[test]
#[ignore = "requires GPU; large"]
fn gpu_parity_8192() {
    huge_parity(8192);
}

#[test]
#[ignore = "requires GPU; very large"]
fn gpu_parity_16384() {
    huge_parity(16384);
}

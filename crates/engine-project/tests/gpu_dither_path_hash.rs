//! Export-equivalent path: `DITHER_GPU_PATH=gpu` vs `cpu` must hash-match
//! (main promise for the minimum enablement checklist).

use std::sync::atomic::AtomicBool;

use engine_project::algorithms::gpu_dither::{apply_gpu_dither_rgba, last_gpu_dither_status};
use engine_project::algorithms::gpu_dither_cpu::PaletteRgb;
use engine_project::filter::{DitherModeV2, DitherParamsV2};

fn gray_ramp_f32(w: u32, h: u32) -> Vec<f32> {
    let n = (w * h) as usize;
    let mut v = Vec::with_capacity(n * 4);
    let denom = w.max(2) - 1;
    for y in 0..h {
        for x in 0..w {
            let g = (x * 255) / denom;
            let _ = y;
            let lin = engine_color::palette::srgb_to_linear(g as u8);
            v.extend_from_slice(&[lin, lin, lin, 1.0]);
        }
    }
    v
}

fn bilevel() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

fn f32_to_rgba8(rgba: &[f32]) -> Vec<u8> {
    rgba.chunks_exact(4)
        .flat_map(|c| {
            [
                engine_color::palette::linear_to_srgb(c[0]),
                engine_color::palette::linear_to_srgb(c[1]),
                engine_color::palette::linear_to_srgb(c[2]),
                (c[3].clamp(0.0, 1.0) * 255.0).round() as u8,
            ]
        })
        .collect()
}

fn run_path(path: &str, mode: DitherModeV2) -> (Vec<u8>, String, String) {
    if path.eq_ignore_ascii_case("gpu") {
        engine_project::algorithms::gpu_selftest::set_state(
            engine_project::algorithms::gpu_selftest::SelftestState::Pass,
        );
    }
    std::env::set_var("DITHER_GPU_PATH", path);
    let w = 256u32;
    let h = 256u32;
    let mut rgba = gray_ramp_f32(w, h);
    let params = DitherParamsV2 {
        mode,
        ..DitherParamsV2::default()
    };
    let cancel = AtomicBool::new(false);
    apply_gpu_dither_rgba(&mut rgba, w, h, &params, &bilevel(), &cancel).expect("apply");
    let status = last_gpu_dither_status().expect("status");
    (f32_to_rgba8(&rgba), status.path, status.backend)
}

#[test]
fn blocked_fs_gpu_path_matches_cpu_path_hash() {
    if engine_gpu::cached_blocked_fs_device().is_none() {
        eprintln!("skip: no GPU adapter");
        return;
    }
    let (gpu_px, gpu_path, gpu_backend) = run_path("gpu", DitherModeV2::BlockedFs);
    assert_eq!(
        gpu_path, "GPU",
        "expected GPU path, got {gpu_path} ({gpu_backend})"
    );
    let (cpu_px, cpu_path, _) = run_path("cpu", DitherModeV2::BlockedFs);
    assert_eq!(cpu_path, "CPU");
    let hg = sha256_hex(&gpu_px);
    let hc = sha256_hex(&cpu_px);
    assert_eq!(
        hg, hc,
        "DITHER_GPU_PATH=gpu vs cpu hash mismatch — breaks the main promise"
    );
    eprintln!("blocked_fs sha256={hg} gpu_backend={gpu_backend}");
}

#[test]
fn blue_noise_gpu_path_matches_cpu_path_hash() {
    if engine_gpu::cached_blocked_fs_device().is_none() {
        eprintln!("skip: no GPU adapter");
        return;
    }
    let (gpu_px, gpu_path, gpu_backend) = run_path("gpu", DitherModeV2::BlueNoise);
    assert_eq!(
        gpu_path, "GPU",
        "expected GPU path, got {gpu_path} ({gpu_backend})"
    );
    let (cpu_px, cpu_path, _) = run_path("cpu", DitherModeV2::BlueNoise);
    assert_eq!(cpu_path, "CPU");
    let hg = sha256_hex(&gpu_px);
    let hc = sha256_hex(&cpu_px);
    assert_eq!(
        hg, hc,
        "DITHER_GPU_PATH=gpu vs cpu hash mismatch — breaks the main promise"
    );
    eprintln!("blue_noise sha256={hg} gpu_backend={gpu_backend}");
}

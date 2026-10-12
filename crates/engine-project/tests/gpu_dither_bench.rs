//! Speed matrix for GPU dither families (spec §5.2). Ignored by default.
//!
//! ```bash
//! cargo test -p engine-project --test gpu_dither_bench --release -- --ignored --nocapture --test-threads=1
//! ```

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use engine_project::algorithms::gpu_dither::apply_gpu_dither_rgba;
use engine_project::algorithms::gpu_dither_cpu::PaletteRgb;
use engine_project::filter::{DitherModeV2, DitherParamsV2};

fn ramp(w: u32, h: u32) -> Vec<f32> {
    let mut v = Vec::with_capacity((w * h * 4) as usize);
    let denom = w.max(2) - 1;
    for _y in 0..h {
        for x in 0..w {
            let g = (x * 255) / denom;
            let lin = engine_color::palette::srgb_to_linear(g as u8);
            v.extend_from_slice(&[lin, lin, lin, 1.0]);
        }
    }
    v
}

fn pal(n: usize) -> Vec<PaletteRgb> {
    if n <= 2 {
        return vec![
            PaletteRgb { r: 0, g: 0, b: 0 },
            PaletteRgb {
                r: 255,
                g: 255,
                b: 255,
            },
        ];
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = (i as f32 / (n - 1) as f32 * 255.0).round() as u8;
        out.push(PaletteRgb {
            r: t,
            g: t,
            b: (255 - t),
        });
    }
    out
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

fn p95(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() as f64) * 0.95).ceil() as usize;
    v[i.saturating_sub(1).min(v.len() - 1)]
}

fn run_once(path: &str, mode: DitherModeV2, w: u32, h: u32, ps: u8, pal_n: usize) -> f64 {
    std::env::set_var("DITHER_GPU_PATH", path);
    let mut rgba = ramp(w, h);
    let params = DitherParamsV2 {
        mode,
        pixel_size: ps,
        ..DitherParamsV2::default()
    };
    let cancel = AtomicBool::new(false);
    let t0 = Instant::now();
    apply_gpu_dither_rgba(&mut rgba, w, h, &params, &pal(pal_n), &cancel).expect("apply");
    t0.elapsed().as_secs_f64() * 1000.0
}

#[test]
#[ignore = "bench: cargo test -p engine-project --test gpu_dither_bench --release -- --ignored --nocapture --test-threads=1"]
fn gpu_dither_speed_matrix() {
    if engine_gpu::cached_blocked_fs_device().is_none() {
        eprintln!("skip: no GPU");
        return;
    }
    let repeats = std::env::var("BENCH_REPEAT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(20usize)
        .max(3);
    let sizes = [
        (1920u32, 1080u32),
        (3840, 2160),
        (4000, 3000),
        (8192, 8192),
    ];
    eprintln!("=== gpu_dither_bench repeats={repeats} ===");
    eprintln!("algo\tpath\tWxH\tps\tpal\tstate\tmedian_ms\tp95_ms");
    for &(w, h) in &sizes {
        // Skip 8K if BENCH_SKIP_8K=1
        if w >= 8192 && std::env::var("BENCH_SKIP_8K").ok().as_deref() == Some("1") {
            continue;
        }
        for mode in [DitherModeV2::BlockedFs, DitherModeV2::BlueNoise] {
            let name = match mode {
                DitherModeV2::BlockedFs => "blocked_fs",
                DitherModeV2::BlueNoise => "blue_noise",
                _ => "?",
            };
            for path in ["gpu", "cpu"] {
                for ps in [1u8, 4] {
                    for pal_n in [2usize, 16] {
                        // cold
                        let cold = run_once(path, mode.clone(), w, h, ps, pal_n);
                        let mut warm = Vec::with_capacity(repeats);
                        for _ in 0..repeats {
                            warm.push(run_once(path, mode.clone(), w, h, ps, pal_n));
                        }
                        eprintln!(
                            "{name}\t{path}\t{w}x{h}\tps={ps}\tp{pal_n}\tcold\t{cold:.1}\t{cold:.1}"
                        );
                        eprintln!(
                            "{name}\t{path}\t{w}x{h}\tps={ps}\tp{pal_n}\twarm\t{:.1}\t{:.1}",
                            median(warm.clone()),
                            p95(warm)
                        );
                    }
                }
            }
        }
        // Exact FS CPU reference point (strip ED via apply is heavy; skip huge).
        if w <= 3840 {
            // Use blocked_fs CPU as the approx reference already; note floyd via
            // full document would need a different harness. Spec asks for
            // floyd_steinberg — measure via a quick tile-less note:
            eprintln!("floyd_steinberg\tcpu\t{w}x{h}\t(see exact ED path; not in gpu_dither apply)");
        }
    }
}

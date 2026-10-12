//! Geometry variants A–D for blocked_fs (next-stage spec §5).
//!
//! Apple M3 reports `max_compute_workgroup_storage_size=32768`. Variant A needs
//! ~33 KiB of workgroup arrays (barely admitted). B/C/D need ≥32 KiB for the
//! color tile alone and **cannot** ship on this adapter without moving tile/err
//! out of workgroup memory (architecture change — owner decision).
//!
//! This spike: CPU timings + seam metrics for A–D; GPU only for A; storage table.
//! PNGs land under `target/geometry_spike/` when the ignored timing test runs.
//!
//! **STOP after this spike — do not change default geometry without owner pick.**

use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use engine_project::algorithms::gpu_dither_cpu::{
    blocked_fs_cpu, with_pixel_size_blocks, PaletteRgb, Rgba8, CORE, PAD,
};

#[derive(Clone, Copy)]
struct Geom {
    name: &'static str,
    core_w: u32,
    core_h: u32,
    pad: u32,
    threads: u32,
}

const VARIANTS: &[Geom] = &[
    Geom {
        name: "A",
        core_w: 56,
        core_h: 56,
        pad: 8,
        threads: 64,
    },
    Geom {
        name: "B",
        core_w: 120,
        core_h: 56,
        pad: 8,
        threads: 64,
    },
    Geom {
        name: "C",
        core_w: 248,
        core_h: 56,
        pad: 8,
        threads: 64,
    },
    Geom {
        name: "D",
        core_w: 248,
        core_h: 120,
        pad: 8,
        threads: 128,
    },
];

fn workgroup_bytes(g: Geom) -> u64 {
    let wg_w = (g.core_w + g.pad) as u64;
    let wg_h = (g.core_h + g.pad) as u64;
    let tile = wg_w * wg_h * 4; // u32 RGBA
    let err = (wg_w + 2) * (wg_h + 2) * 4; // atomic i32
    tile + err
}

fn ramp(w: u32, h: u32) -> Vec<Rgba8> {
    let mut v = Vec::with_capacity((w * h) as usize);
    let denom = w.max(2) - 1;
    for _y in 0..h {
        for x in 0..w {
            let g = ((x * 255) / denom) as u8;
            v.push(Rgba8::new(g, g, g, 255));
        }
    }
    v
}

fn mid_gray(w: u32, h: u32) -> Vec<Rgba8> {
    vec![Rgba8::new(128, 128, 128, 255); (w * h) as usize]
}

fn line_art(w: u32, h: u32) -> Vec<Rgba8> {
    let mut v = vec![Rgba8::new(255, 255, 255, 255); (w * h) as usize];
    for y in 0..h {
        for x in 0..w {
            if x == y || x + y == w.min(h) || x % 32 == 0 {
                v[(y * w + x) as usize] = Rgba8::new(0, 0, 0, 255);
            }
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

fn seam_metric(rgba: &[Rgba8], w: u32, h: u32, core: u32) -> f64 {
    // Mean luminance in 4px bands around horizontal tile seams vs elsewhere.
    let mut seam = 0.0f64;
    let mut seam_n = 0u64;
    let mut other = 0.0f64;
    let mut other_n = 0u64;
    let band = 4i32;
    for y in 0..h as i32 {
        let near_seam = {
            let mut near = false;
            let mut t = core as i32;
            while t < h as i32 {
                if (y - t).abs() < band {
                    near = true;
                    break;
                }
                t += core as i32;
            }
            near
        };
        for x in 0..w as i32 {
            let p = &rgba[(y as u32 * w + x as u32) as usize];
            let yv = (p.r as f64 + p.g as f64 + p.b as f64) / 3.0;
            if near_seam {
                seam += yv;
                seam_n += 1;
            } else {
                other += yv;
                other_n += 1;
            }
        }
    }
    if seam_n == 0 || other_n == 0 {
        return 0.0;
    }
    (seam / seam_n as f64) - (other / other_n as f64)
}

/// Box blur 8×8 then mean abs diff (blocked_fs vs a second image).
fn blur8_mad(a: &[Rgba8], b: &[Rgba8], w: u32, h: u32) -> f64 {
    let r = 4i32;
    let mut acc = 0.0f64;
    let mut n = 0u64;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let mut sa = 0.0f64;
            let mut sb = 0.0f64;
            let mut c = 0.0f64;
            for dy in -r..=r {
                for dx in -r..=r {
                    let xx = (x + dx).clamp(0, w as i32 - 1) as u32;
                    let yy = (y + dy).clamp(0, h as i32 - 1) as u32;
                    let ia = (yy * w + xx) as usize;
                    sa += (a[ia].r as f64 + a[ia].g as f64 + a[ia].b as f64) / 3.0;
                    sb += (b[ia].r as f64 + b[ia].g as f64 + b[ia].b as f64) / 3.0;
                    c += 1.0;
                }
            }
            acc += ((sa / c) - (sb / c)).abs();
            n += 1;
        }
    }
    acc / n as f64
}

fn write_png(path: &PathBuf, rgba: &[Rgba8], w: u32, h: u32) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut bytes = Vec::with_capacity(rgba.len() * 4);
    for p in rgba {
        bytes.extend_from_slice(&[p.r, p.g, p.b, p.a]);
    }
    let _ = image::save_buffer(path, &bytes, w, h, image::ColorType::Rgba8);
}

fn out_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/geometry_spike")
}

#[test]
fn geometry_workgroup_storage_table() {
    eprintln!("=== geometry workgroup storage (declared arrays) ===");
    eprintln!("variant\tcore\tpad\tthreads\tbytes\tvs_32KiB");
    for g in VARIANTS {
        let b = workgroup_bytes(*g);
        eprintln!(
            "{}\t{}x{}\t{}\t{}\t{}\t{}",
            g.name,
            g.core_w,
            g.core_h,
            g.pad,
            g.threads,
            b,
            if b <= 32768 { "OK" } else { "EXCEEDS" }
        );
    }
    assert!(workgroup_bytes(VARIANTS[0]) <= 40_000); // A near the floor
    assert!(workgroup_bytes(VARIANTS[1]) > 32768); // B cannot fit on M3
}

#[test]
#[ignore = "spike: cargo test -p engine-project --test gpu_geometry_spike --release -- --ignored --nocapture"]
fn geometry_cpu_timing_and_seams() {
    let pals = bilevel();
    let sizes = [(1920u32, 1080u32), (3840, 2160), (4000, 3000)];
    let dir = out_dir();
    eprintln!("=== geometry CPU spike → {} ===", dir.display());
    eprintln!("variant\tsize\tps\tmedian_ms\tp95_ms\tseam_dY\tblur8_vs_A");
    for &(w, h) in &sizes {
        let mut ref_a: Option<Vec<Rgba8>> = None;
        for g in VARIANTS {
            for ps in [1u32, 3, 7] {
                let input = ramp(w, h);
                let mut samples = Vec::new();
                let mut last = Vec::new();
                for i in 0..20 {
                    let t0 = Instant::now();
                    let out = with_pixel_size_blocks(&input, w, h, ps, |small, sw, sh| {
                        blocked_fs_cpu(small, sw, sh, &pals, g.core_w, g.core_h, g.pad)
                    })
                    .expect("cpu");
                    let ms = t0.elapsed().as_secs_f64() * 1000.0;
                    if i > 0 {
                        samples.push(ms);
                    }
                    last = out;
                }
                samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let med = samples[samples.len() / 2];
                let p95 = samples[((samples.len() as f64) * 0.95).ceil() as usize - 1];
                let seam = seam_metric(&last, w, h, g.core_h);
                let vs_a = if g.name == "A" {
                    ref_a = Some(last.clone());
                    0.0
                } else if let Some(ref a) = ref_a {
                    blur8_mad(a, &last, w, h)
                } else {
                    0.0
                };
                eprintln!(
                    "{}\t{}x{}\tps={}\t{med:.1}\t{p95:.1}\t{seam:.3}\t{vs_a:.3}",
                    g.name, w, h, ps
                );
                if ps == 1 && matches!(w, 1920 | 4000) {
                    write_png(
                        &dir.join(format!("{}_{}x{}_ramp.png", g.name, w, h)),
                        &last,
                        w,
                        h,
                    );
                }
            }
        }
        // Fixture suite at ≤512 for seam + PNG budget.
        let gw = w.min(512);
        let gh = h.min(512);
        for (tag, src) in [
            ("gray", mid_gray(gw, gh)),
            ("ramp", ramp(gw, gh)),
            ("lines", line_art(gw, gh)),
        ] {
            for g in VARIANTS {
                let out =
                    blocked_fs_cpu(&src, gw, gh, &pals, g.core_w, g.core_h, g.pad).unwrap();
                let seam = seam_metric(&out, gw, gh, g.core_h);
                eprintln!("seam_{tag}\t{}\t{}x{}\t{seam:.3}", g.name, gw, gh);
                write_png(
                    &dir.join(format!("{}_{}_{}x{}.png", g.name, tag, gw, gh)),
                    &out,
                    gw,
                    gh,
                );
            }
        }
    }
    // Photo fixture if present.
    let photo_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/gpu_parity/photos/photo_grad.png");
    if let Ok(img) = image::open(&photo_path) {
        let rgba = img.to_rgba8();
        let (pw, ph) = rgba.dimensions();
        let mut input = Vec::with_capacity((pw * ph) as usize);
        for p in rgba.pixels() {
            input.push(Rgba8::new(p[0], p[1], p[2], p[3]));
        }
        for g in VARIANTS {
            let out = blocked_fs_cpu(&input, pw, ph, &pals, g.core_w, g.core_h, g.pad).unwrap();
            let seam = seam_metric(&out, pw, ph, g.core_h);
            eprintln!("seam_photo\t{}\t{}x{}\t{seam:.3}", g.name, pw, ph);
            write_png(
                &dir.join(format!("{}_photo_{}x{}.png", g.name, pw, ph)),
                &out,
                pw,
                ph,
            );
        }
    }
    eprintln!("note: GPU B–D not run — workgroup storage exceeds 32768 on Apple M3");
    eprintln!("default CORE={} PAD={} still variant A — owner must pick before Task 5", CORE, PAD);
    eprintln!("STOP: awaiting owner geometry choice (spec §5.5)");
}

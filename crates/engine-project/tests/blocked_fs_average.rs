//! blocked_fs correctness: preserve average, identity, ring negatives.

use engine_project::algorithms::gpu_dither_cpu::{
    blocked_fs_cpu, blocked_fs_cpu_per_channel_subst, blocked_fs_wavefront_ring_apply,
    blocked_fs_wavefront_ring_apply_ex, levels_palette, PaletteRgb, Rgba8, CORE, PAD,
};
use engine_project::filter::DitherColorMode;

fn solid(g: u8, w: u32, h: u32) -> Vec<Rgba8> {
    vec![Rgba8::new(g, g, g, 255); (w * h) as usize]
}

fn solid_rgb(r: u8, g: u8, b: u8, w: u32, h: u32) -> Vec<Rgba8> {
    vec![Rgba8::new(r, g, b, 255); (w * h) as usize]
}

fn bw() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn mid3() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 128,
            g: 128,
            b: 128,
        },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn white_frac(out: &[Rgba8]) -> f64 {
    out.iter().filter(|p| p.r >= 128).count() as f64 / out.len() as f64
}

fn mean_rgb(out: &[Rgba8]) -> (f64, f64, f64) {
    let n = out.len() as f64;
    let mut sr = 0.0;
    let mut sg = 0.0;
    let mut sb = 0.0;
    for p in out {
        sr += p.r as f64;
        sg += p.g as f64;
        sb += p.b as f64;
    }
    (sr / n, sg / n, sb / n)
}

#[test]
fn blocked_fs_preserve_average_bw_step8() {
    let side = 1024u32;
    let pal = bw();
    for g in (0u8..=255).step_by(8) {
        // Include 255: step_by on ..=255 stops at 248; add explicitly below.
        let input = solid(g, side, side);
        let out = blocked_fs_cpu(&input, side, side, &pal, CORE, CORE, PAD).unwrap();
        let wf = white_frac(&out);
        let expect = g as f64 / 255.0;
        assert!(
            (wf - expect).abs() <= 0.01,
            "g={g}: white_frac={wf:.4} expect={expect:.4}"
        );
    }
    let input = solid(255, side, side);
    let out = blocked_fs_cpu(&input, side, side, &pal, CORE, CORE, PAD).unwrap();
    assert!((white_frac(&out) - 1.0).abs() <= 0.01);
}

#[test]
fn blocked_fs_preserve_average_mid3_and_16() {
    let side = 1024u32;
    let pals = [mid3(), levels_palette(16, DitherColorMode::Grayscale)];
    for pal in &pals {
        for g in (0u8..=255).step_by(8) {
            let input = solid(g, side, side);
            let out = blocked_fs_cpu(&input, side, side, pal, CORE, CORE, PAD).unwrap();
            let (mr, mg, mb) = mean_rgb(&out);
            let expect = g as f64;
            assert!(
                (mr - expect).abs() <= 255.0 * 0.01
                    && (mg - expect).abs() <= 255.0 * 0.01
                    && (mb - expect).abs() <= 255.0 * 0.01,
                "pal_len={} g={g}: mean=({mr:.1},{mg:.1},{mb:.1})",
                pal.len()
            );
        }
    }
}

#[test]
fn blocked_fs_solid_palette_color_identity() {
    let side = 256u32;
    let pal = mid3();
    for &c in &[0u8, 128, 255] {
        let input = solid(c, side, side);
        let out = blocked_fs_cpu(&input, side, side, &pal, CORE, CORE, PAD).unwrap();
        assert!(
            out.iter().all(|p| p.r == c && p.g == c && p.b == c),
            "solid {c} must stay {c}"
        );
    }
    // Non-gray palette entry.
    let pal2 = vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 200,
            g: 40,
            b: 40,
        },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ];
    let input = solid_rgb(200, 40, 40, side, side);
    let out = blocked_fs_cpu(&input, side, side, &pal2, CORE, CORE, PAD).unwrap();
    assert!(out
        .iter()
        .all(|p| p.r == 200 && p.g == 40 && p.b == 40));
}

#[test]
fn blocked_fs_legacy_per_channel_fails_preserve_average() {
    let side = 1024u32;
    let pal = bw();
    let g = 192u8;
    let input = solid(g, side, side);
    let out = blocked_fs_cpu_per_channel_subst(&input, side, side, &pal, CORE, CORE, PAD).unwrap();
    let wf = white_frac(&out);
    let expect = g as f64 / 255.0;
    assert!(
        (wf - expect).abs() > 0.01,
        "legacy per-channel must FAIL preserve-average at g=192: wf={wf} expect={expect}"
    );
    assert!(
        (wf - 1.0).abs() < 1e-9,
        "legacy at g=192 is expected 100% white, got {wf}"
    );
}

#[test]
fn blocked_fs_err_ring4_matches_full_plane_ring3_erasure_diverges() {
    let pw = 64usize;
    let ph = 64usize;
    let pal = bw();
    let base: Vec<[u8; 3]> = (0..pw * ph)
        .map(|i| {
            let g = ((i * 1103515245 + 12345) % 256) as u8;
            [g, g, g]
        })
        .collect();
    let mut ring4 = base.clone();
    let mut ring3_ok = base.clone();
    let mut ring3_erase = base.clone();
    blocked_fs_wavefront_ring_apply(&mut ring4, pw, ph, &pal, 4).unwrap();
    // With correct clear, ring=3 is mathematically enough (t freed before t+3 write).
    blocked_fs_wavefront_ring_apply(&mut ring3_ok, pw, ph, &pal, 3).unwrap();
    // Missing clear → t and t+3 alias without erasure → DROP parity (catches the bug class).
    blocked_fs_wavefront_ring_apply_ex(&mut ring3_erase, pw, ph, &pal, 3, false).unwrap();

    let input: Vec<Rgba8> = base
        .iter()
        .map(|c| Rgba8::new(c[0], c[1], c[2], 255))
        .collect();
    let full = blocked_fs_cpu(&input, pw as u32, ph as u32, &pal, 64, 64, 0).unwrap();
    let full_rgb: Vec<[u8; 3]> = full.iter().map(|p| [p.r, p.g, p.b]).collect();

    assert_eq!(ring4, full_rgb, "ring=4 must match full err plane");
    assert_eq!(
        ring3_ok, full_rgb,
        "ring=3 with clear is equivalent (pigeonhole after free)"
    );
    assert_ne!(
        ring3_erase, full_rgb,
        "ring=3 without clear must DROP parity (tests catch erasures)"
    );
}

/// Print measurement tables for the report (always runs with unit tests).
#[test]
fn print_blocked_fs_measurement_tables() {
    let side = 1024u32;
    eprintln!("=== blocked_fs white% vs g/255 (BW, step 8, 1024²) ===");
    eprintln!("g\tg/255\twhite%\tdelta");
    for g in (0u8..=255).step_by(8).chain(std::iter::once(255)) {
        let out = blocked_fs_cpu(&solid(g, side, side), side, side, &bw(), CORE, CORE, PAD).unwrap();
        let wf = white_frac(&out);
        let e = g as f64 / 255.0;
        eprintln!("{g}\t{e:.4}\t{wf:.4}\t{:.4}", wf - e);
    }
    eprintln!("=== blocked_fs mean vs g (mid3 / gray16) ===");
    for (label, pal) in [
        ("mid3", mid3()),
        ("gray16", levels_palette(16, DitherColorMode::Grayscale)),
    ] {
        eprintln!("-- {label} --");
        for g in (0u8..=255).step_by(8).chain(std::iter::once(255)) {
            let out =
                blocked_fs_cpu(&solid(g, side, side), side, side, &pal, CORE, CORE, PAD).unwrap();
            let (mr, mg, mb) = mean_rgb(&out);
            eprintln!("{g}\t{mr:.2}\t{mg:.2}\t{mb:.2}");
        }
    }
}

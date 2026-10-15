//! blocked_fs + blue_noise correctness: preserve average, identity, negatives.

use engine_project::algorithms::gpu_dither_cpu::{
    blocked_fs_cpu, blocked_fs_cpu_per_channel_subst, blocked_fs_wavefront_ring_apply,
    blocked_fs_wavefront_ring_apply_ex, blue_noise_cpu, blue_noise_ranks, levels_palette,
    nearest_palette, palette_gray_interval_lut, palette_half_nn_dist, PaletteRgb, Rgba8, CORE,
    DEFAULT_SPREAD, PAD,
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

fn gray_0_50_255() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 50,
            g: 50,
            b: 50,
        },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
    ]
}

fn color6() -> Vec<PaletteRgb> {
    vec![
        PaletteRgb { r: 0, g: 0, b: 0 },
        PaletteRgb {
            r: 255,
            g: 255,
            b: 255,
        },
        PaletteRgb { r: 255, g: 0, b: 0 },
        PaletteRgb { r: 0, g: 255, b: 0 },
        PaletteRgb { r: 0, g: 0, b: 255 },
        PaletteRgb {
            r: 128,
            g: 128,
            b: 128,
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
    assert!(out.iter().all(|p| p.r == 200 && p.g == 40 && p.b == 40));
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

#[test]
fn blue_noise_half_nn_and_gray_interval_a() {
    let bw_h = palette_half_nn_dist(&bw());
    assert_eq!(bw_h, vec![127, 127]); // floor(255/2)
    let mid_h = palette_half_nn_dist(&mid3());
    // 0→128 d=128 →64; 128→255 d=127 →63; 255→128 d=127 →63
    assert_eq!(mid_h, vec![64, 63, 63]);
    let close = vec![
        PaletteRgb {
            r: 100,
            g: 100,
            b: 100,
        },
        PaletteRgb {
            r: 101,
            g: 101,
            b: 101,
        },
    ];
    assert_eq!(palette_half_nn_dist(&close), vec![0, 0]); // dist 1 → half 0

    let lut = palette_gray_interval_lut(&mid3()).expect("gray");
    assert_eq!(lut[8], 128, "g=8 in [0,128] → A=128");
    assert_eq!(lut[200], 127, "g=200 in [128,255] → A=127");
    // Exact mid: fallback 2*half_nn(128)=126
    assert_eq!(lut[128], 126);
}

#[test]
fn blue_noise_gray_preserve_average_and_identity() {
    let side = 512u32;
    let tex = blue_noise_ranks();
    let max_spread = 1024u32;
    let gray_pals = [
        ("bw", bw()),
        ("mid3", mid3()),
        ("gray16", levels_palette(16, DitherColorMode::Grayscale)),
        ("g050255", gray_0_50_255()),
    ];
    for (label, pal) in &gray_pals {
        for g in (0u8..=255).step_by(8).chain(std::iter::once(255)) {
            let out = blue_noise_cpu(&solid(g, side, side), side, side, pal, tex, DEFAULT_SPREAD)
                .unwrap();
            if pal.len() == 2 && pal[0].r == 0 && pal[1].r == 255 {
                let wf = white_frac(&out);
                let expect = g as f64 / 255.0;
                assert!(
                    (wf - expect).abs() <= 0.02,
                    "blue_noise {label} BW g={g}: wf={wf:.4} expect={expect:.4}"
                );
            } else {
                let (mr, mg, mb) = mean_rgb(&out);
                let expect = g as f64;
                assert!(
                    (mr - expect).abs() <= 255.0 * 0.02
                        && (mg - expect).abs() <= 255.0 * 0.02
                        && (mb - expect).abs() <= 255.0 * 0.02,
                    "blue_noise {label} g={g}: mean=({mr:.1},{mg:.1},{mb:.1}) expect={expect}"
                );
            }
        }
        for p in pal {
            let c = (p.r, p.g, p.b);
            for spread in [DEFAULT_SPREAD, max_spread] {
                let out = blue_noise_cpu(
                    &solid_rgb(c.0, c.1, c.2, 128, 128),
                    128,
                    128,
                    pal,
                    tex,
                    spread,
                )
                .unwrap();
                assert!(
                    out.iter().all(|q| q.r == c.0 && q.g == c.1 && q.b == c.2),
                    "identity {label} RGB{:?} spread={spread}",
                    c
                );
            }
        }
    }
    // Dist-1 gray pair: shift≡0 → exact identity at max spread.
    let close = vec![
        PaletteRgb {
            r: 100,
            g: 100,
            b: 100,
        },
        PaletteRgb {
            r: 101,
            g: 101,
            b: 101,
        },
    ];
    let out = blue_noise_cpu(&solid(100, 64, 64), 64, 64, &close, tex, max_spread).unwrap();
    assert!(out.iter().all(|q| q.r == 100 && q.g == 100 && q.b == 100));
}

#[test]
fn blue_noise_color_identity_in_palette_mean_vs_nearest() {
    let side = 256u32;
    let tex = blue_noise_ranks();
    let pal = color6();
    let max_spread = 1024u32;
    // Solid palette colors unchanged.
    for p in &pal {
        for spread in [DEFAULT_SPREAD, max_spread] {
            let out = blue_noise_cpu(&solid_rgb(p.r, p.g, p.b, 64, 64), 64, 64, &pal, tex, spread)
                .unwrap();
            assert!(
                out.iter().all(|q| q.r == p.r && q.g == p.g && q.b == p.b),
                "color identity {:?} spread={spread}",
                (p.r, p.g, p.b)
            );
        }
    }
    // Solids: output ∈ palette; mean-RGB bias ≤ plain nearest (preserve average).
    let probes = [
        (80u8, 80, 80),
        (200, 40, 40),
        (40, 200, 40),
        (40, 40, 200),
        (180, 180, 100),
    ];
    for &(r, g, b) in &probes {
        let input = solid_rgb(r, g, b, side, side);
        let out = blue_noise_cpu(&input, side, side, &pal, tex, DEFAULT_SPREAD).unwrap();
        assert!(
            out.iter()
                .all(|p| pal.iter().any(|q| q.r == p.r && q.g == p.g && q.b == p.b)),
            "output not in palette for solid {:?}",
            (r, g, b)
        );
        let (nr, ng, nb) = nearest_palette((r, g, b), &pal).unwrap();
        let (br, bg, bb) = mean_rgb(&out);
        let err_bn = (r as f64 - br).abs() + (g as f64 - bg).abs() + (b as f64 - bb).abs();
        let err_near = (r as f64 - nr as f64).abs()
            + (g as f64 - ng as f64).abs()
            + (b as f64 - nb as f64).abs();
        assert!(
            err_bn <= err_near + 1.0,
            "solid {:?} BN bias {err_bn:.2} > nearest {err_near:.2} (mean=({br:.1},{bg:.1},{bb:.1}))",
            (r, g, b)
        );
    }
}

#[test]
fn print_blue_noise_mid3_g8_table() {
    let side = 512u32;
    let out = blue_noise_cpu(
        &solid(8, side, side),
        side,
        side,
        &mid3(),
        blue_noise_ranks(),
        DEFAULT_SPREAD,
    )
    .unwrap();
    let (mr, mg, mb) = mean_rgb(&out);
    eprintln!("=== blue_noise mid3 g=8 (expect mean≈8) ===");
    eprintln!("mean=({mr:.2},{mg:.2},{mb:.2})");
    assert!(
        (mr - 8.0).abs() <= 255.0 * 0.02,
        "g=8 mean={mr:.2} (clip-tail bug was ~35.8)"
    );
}

/// Print measurement tables for the report (always runs with unit tests).
#[test]
fn print_blocked_fs_measurement_tables() {
    let side = 1024u32;
    eprintln!("=== blocked_fs white% vs g/255 (BW, step 8, 1024²) ===");
    eprintln!("g\tg/255\twhite%\tdelta");
    for g in (0u8..=255).step_by(8).chain(std::iter::once(255)) {
        let out =
            blocked_fs_cpu(&solid(g, side, side), side, side, &bw(), CORE, CORE, PAD).unwrap();
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

//! F1 step 1: seam fixtures lock `SEAM_K` / `SEAM_EPSILON` from data.
//!
//! Minimum set (spec):
//! 1. smooth gradient (seamless edges)
//! 2. noisy photo-like (seamless edges)
//! 3. flat fills + enough detail (seamless edges)
//! 4. high-frequency texture (seamless edges)
//! 5. known non-seamless control (must fail at any reasonable k)
//!
//! Plus: anisotropic stitch (bad H only) and flat + ε-floor guard.

use engine_project::{
    validate_seams_rgba8, SeamMode, SeamReport, SEAM_EPSILON, SEAM_K,
};

const W: u32 = 128;
const H: u32 = 96;

fn put(buf: &mut [u8], w: u32, x: u32, y: u32, rgba: [u8; 4]) {
    let i = ((y * w + x) * 4) as usize;
    buf[i..i + 4].copy_from_slice(&rgba);
}

fn get(buf: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

/// Force wrap edges equal so the buffer is bit-periodic (R_boundary → 0).
fn force_seamless(buf: &mut [u8], w: u32, h: u32) {
    for y in 0..h {
        let p = get(buf, w, 0, y);
        put(buf, w, w - 1, y, p);
    }
    for x in 0..w {
        let p = get(buf, w, x, 0);
        put(buf, w, x, h - 1, p);
    }
}

fn fixture_gradient_seamless() -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        let v = ((y as u64 * 255) / (H as u64 - 1)) as u8;
        for x in 0..W {
            // Mild horizontal variation so H-internal ≠ 0.
            let u = ((x as u64 * 40) / (W as u64)) as u8;
            put(&mut buf, W, x, y, [v.saturating_add(u / 4), v, v.saturating_sub(u / 5), 255]);
        }
    }
    force_seamless(&mut buf, W, H);
    buf
}

fn fixture_noisy_seamless() -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            // Soft noise: neighbors often close → R_internal mid, not ~1.
            let n = x
                .wrapping_mul(374761393)
                .wrapping_add(y.wrapping_mul(668265263))
                .wrapping_add(0x9E37_79B9);
            let base = 120u8;
            let r = base.wrapping_add(((n >> 16) as u8) / 8);
            let g = base.wrapping_add(((n >> 8) as u8) / 8);
            let b = base.wrapping_add((n as u8) / 8);
            put(&mut buf, W, x, y, [r, g, b, 255]);
        }
    }
    force_seamless(&mut buf, W, H);
    buf
}

/// Blocky tile (8×8 constant cells). Low R_internal; used for control / anisotropic.
fn fixture_blocky(seed: u32) -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    const BS: u32 = 8;
    for y in 0..H {
        for x in 0..W {
            let bx = x / BS;
            let by = y / BS;
            let n = bx
                .wrapping_mul(747796405)
                .wrapping_add(by.wrapping_mul(2891336453))
                .wrapping_add(seed);
            let c = (n ^ (n >> 9)) as u8;
            put(&mut buf, W, x, y, [c, c.wrapping_add(20), c.wrapping_add(40), 255]);
        }
    }
    buf
}

fn fixture_blocky_seamless() -> Vec<u8> {
    let mut buf = fixture_blocky(1);
    force_seamless(&mut buf, W, H);
    buf
}

fn fixture_flat_with_detail_seamless() -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            put(&mut buf, W, x, y, [180, 180, 180, 255]);
        }
    }
    // Detail block away from edges so force_seamless does not erase it.
    for y in 20..40 {
        for x in 30..60 {
            let on = ((x + y) % 3) == 0;
            let c = if on { 40 } else { 220 };
            put(&mut buf, W, x, y, [c, c, c, 255]);
        }
    }
    force_seamless(&mut buf, W, H);
    buf
}

fn fixture_hfreq_seamless() -> Vec<u8> {
    // 2×2 micro-blocks (not 1px checker): R_internal ~0.5, still structured.
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            let on = ((x / 2) + (y / 2)) % 2 == 0;
            let c = if on { 0 } else { 255 };
            put(&mut buf, W, x, y, [c, c, c, 255]);
        }
    }
    force_seamless(&mut buf, W, H);
    buf
}

/// Control: blocky interior (low R_internal) with fully broken wrap edges.
/// Relative ratio ≫ k because R_boundary≈1 while R_internal stays ~block-boundary rate.
fn fixture_non_seamless_control() -> Vec<u8> {
    let mut buf = fixture_blocky(2);
    for y in 0..H {
        let mut p = get(&buf, W, 0, y);
        p[0] = p[0].wrapping_add(128);
        p[1] = p[1].wrapping_add(64);
        p[2] = p[2].wrapping_add(32);
        put(&mut buf, W, W - 1, y, p);
    }
    for x in 0..W {
        let mut p = get(&buf, W, x, 0);
        p[0] = p[0].wrapping_add(90);
        p[1] = p[1].wrapping_add(50);
        p[2] = p[2].wrapping_add(70);
        put(&mut buf, W, x, H - 1, p);
    }
    buf
}

/// Anisotropic: only horizontal seam bad (stitch) on blocky content.
fn fixture_anisotropic_h_seam() -> Vec<u8> {
    let mut buf = fixture_blocky_seamless();
    for y in 0..H {
        let mut p = get(&buf, W, 0, y);
        p[0] ^= 0xFF;
        p[1] ^= 0xFF;
        p[2] ^= 0xFF;
        put(&mut buf, W, W - 1, y, p);
    }
    for x in 0..W {
        let p = get(&buf, W, x, 0);
        put(&mut buf, W, x, H - 1, p);
    }
    buf
}

/// Flat image with a tiny H seam (~1/H of pairs) — ε floor must not false-fail
/// when boundary is clean; when seam is above ε it must fail.
fn fixture_flat_clean() -> Vec<u8> {
    let mut buf = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            put(&mut buf, W, x, y, [50, 50, 50, 255]);
        }
    }
    buf
}

fn fixture_flat_one_seam_pixel() -> Vec<u8> {
    let mut buf = fixture_flat_clean();
    // One mismatched wrap pair on H → R_boundary = 1/H ≈ 1.04%.
    put(&mut buf, W, W - 1, H / 2, [200, 200, 200, 255]);
    buf
}

fn report(buf: &[u8], mode: SeamMode) -> SeamReport {
    validate_seams_rgba8(buf, W, H, mode).unwrap()
}

#[test]
fn locked_constants_match_adr() {
    assert_eq!(SEAM_K, 1.25);
    assert_eq!(SEAM_EPSILON, 0.003);
}

#[test]
fn fixtures_seamless_pass_relative_and_exact_boundary_zero() {
    for (name, buf) in [
        ("gradient", fixture_gradient_seamless()),
        ("noisy", fixture_noisy_seamless()),
        ("flat_detail", fixture_flat_with_detail_seamless()),
        ("hfreq", fixture_hfreq_seamless()),
    ] {
        let exact = report(&buf, SeamMode::Exact);
        assert!(
            exact.ok,
            "{name}: forced-seamless must be exact ok; H={:?} V={:?}",
            exact.horizontal, exact.vertical
        );
        assert_eq!(exact.horizontal.r_boundary, 0.0, "{name} H boundary");
        assert_eq!(exact.vertical.r_boundary, 0.0, "{name} V boundary");

        let rel = report(&buf, SeamMode::Relative);
        assert!(
            rel.ok,
            "{name}: relative must pass with k={SEAM_K} ε={SEAM_EPSILON}; H={:?} V={:?}",
            rel.horizontal, rel.vertical
        );
    }
}

#[test]
fn control_non_seamless_fails_exact_and_relative() {
    let buf = fixture_non_seamless_control();
    let exact = report(&buf, SeamMode::Exact);
    assert!(!exact.ok, "control must fail exact");
    assert!(exact.horizontal.r_boundary > 0.5 || exact.vertical.r_boundary > 0.5);

    let rel = report(&buf, SeamMode::Relative);
    assert!(
        !rel.ok,
        "control must fail relative at k={SEAM_K}; H ratio={:?} V ratio={:?} RbH={} RbV={} RiH={} RiV={}",
        rel.horizontal.ratio,
        rel.vertical.ratio,
        rel.horizontal.r_boundary,
        rel.vertical.r_boundary,
        rel.horizontal.r_internal,
        rel.vertical.r_internal
    );
    // Blocky control: R_internal ≪ 1 so ratio ≫ k (noise would give ratio≈1 and false-pass).
    for axis in [&rel.horizontal, &rel.vertical] {
        assert!(
            axis.r_internal < 0.5,
            "control interior must be structured, not white-noise: Ri={}",
            axis.r_internal
        );
        assert!(
            axis.r_boundary > (SEAM_K * axis.r_internal).max(SEAM_EPSILON),
            "axis must exceed relative gate"
        );
    }
}

#[test]
fn anisotropic_h_seam_fails_only_horizontal() {
    let buf = fixture_anisotropic_h_seam();
    let rel = report(&buf, SeamMode::Relative);
    assert!(!rel.horizontal.ok, "H stitch must fail");
    assert!(rel.vertical.ok, "V must stay ok: {:?}", rel.vertical);
    assert!(!rel.ok, "overall fails if either axis fails");
}

#[test]
fn epsilon_floor_flat_clean_ok_one_pixel_seam_fails() {
    let clean = report(&fixture_flat_clean(), SeamMode::Relative);
    assert!(clean.ok);
    assert_eq!(clean.horizontal.r_internal, 0.0);

    let seamed = report(&fixture_flat_one_seam_pixel(), SeamMode::Relative);
    let rb = seamed.horizontal.r_boundary;
    assert!(
        (rb - 1.0 / H as f64).abs() < 1e-9,
        "expected one row seam, got {rb}"
    );
    // 1/H ≈ 0.0104 > ε=0.003 → fail even with R_internal=0.
    assert!(rb > SEAM_EPSILON);
    assert!(!seamed.horizontal.ok);
    assert!(!seamed.ok);
}

#[test]
fn k_boundary_invisible_vs_control() {
    // Document the lock: all good fixtures have ratio 0 (boundary forced);
    // control sits well above k. This is the data-backed accept criterion.
    let mut max_good_ratio = 0.0f64;
    for buf in [
        fixture_gradient_seamless(),
        fixture_noisy_seamless(),
        fixture_flat_with_detail_seamless(),
        fixture_hfreq_seamless(),
    ] {
        let r = report(&buf, SeamMode::Relative);
        for axis in [r.horizontal, r.vertical] {
            if let Some(ratio) = axis.ratio {
                max_good_ratio = max_good_ratio.max(ratio);
            }
        }
        assert!(r.ok);
    }
    assert!(
        max_good_ratio <= SEAM_K,
        "good fixtures must sit ≤ k; got max ratio {max_good_ratio}"
    );

    let control = report(&fixture_non_seamless_control(), SeamMode::Relative);
    let min_bad = [control.horizontal, control.vertical]
        .into_iter()
        .filter_map(|a| a.ratio)
        .fold(f64::INFINITY, f64::min);
    assert!(
        min_bad > SEAM_K,
        "control min axis ratio {min_bad} must exceed k={SEAM_K}"
    );
}

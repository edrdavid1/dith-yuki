//! Golden harness: moxcms vs lcms2 two-step proof transforms (dev-dep only).
//!
//! Never linked into the Tauri product binary.
//!
//! Relative (no BPC) uses a tight ΔE2000 budget — both engines share the same
//! chain shape. Relative+BPC compares moxcms (app Lab BPC) against lcms2 with
//! native BLACKPOINT_COMPENSATION; budget starts looser because BPC is app-owned.

use engine_color::preview_encode::delta_e2000_srgb8;
use engine_color::soft_proof::{
    ProofProfile, SoftProofIntent, SoftProofTransform, BUILTIN_FOGRA51_ICC,
};
use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};

/// Plan start threshold for Relative FOGRA51 (no BPC) — hard engine-agreement gate.
const MAX_DELTA_E2000_NO_BPC: f64 = 2.0;
/// App-owned XYZ BPC vs lcms2 native PCS BPC (provisional; tighten toward 2).
const MAX_DELTA_E2000_WITH_BPC: f64 = 9.0;
const MEAN_DELTA_E2000_WITH_BPC: f64 = 2.0;

fn moxcms_proof(intent: SoftProofIntent, bpc: bool) -> SoftProofTransform {
    let proof = ProofProfile::from_icc_bytes(BUILTIN_FOGRA51_ICC).expect("FOGRA51");
    SoftProofTransform::new_exact(&proof, intent, bpc).expect("moxcms transform")
}

/// Mirror product chain: sRGB8 → CMYK8 → sRGB8 with the same intent on both legs.
fn lcms_two_step_rgb8(src: &[u8], intent: Intent, bpc: bool) -> Vec<u8> {
    let srgb = Profile::new_srgb();
    let proof = Profile::new_icc(BUILTIN_FOGRA51_ICC).expect("lcms FOGRA51");
    let mut flags = Flags::default();
    if bpc {
        flags = flags | Flags::BLACKPOINT_COMPENSATION;
    }
    let to_cmyk = Transform::new_flags(
        &srgb,
        PixelFormat::RGB_8,
        &proof,
        PixelFormat::CMYK_8,
        intent,
        flags,
    )
    .expect("lcms sRGB→CMYK");
    let to_srgb = Transform::new_flags(
        &proof,
        PixelFormat::CMYK_8,
        &srgb,
        PixelFormat::RGB_8,
        intent,
        flags,
    )
    .expect("lcms CMYK→sRGB");

    let pixels = src.len() / 3;
    let mut cmyk = vec![0u8; pixels * 4];
    to_cmyk.transform_pixels(src, &mut cmyk);
    let mut dst = vec![0u8; src.len()];
    to_srgb.transform_pixels(&cmyk, &mut dst);
    dst
}

fn sample_grid() -> Vec<[u8; 3]> {
    let mut out = Vec::new();
    for r in [0u8, 64, 128, 192, 255] {
        for g in [0u8, 64, 128, 192, 255] {
            for b in [0u8, 64, 128, 192, 255] {
                out.push([r, g, b]);
            }
        }
    }
    for i in 0..32 {
        let t = ((i as f32 / 31.0) * 255.0) as u8;
        out.push([t, t, t]);
        out.push([t, 128, 255u8.saturating_sub(t)]);
    }
    out
}

fn stats(mox: &SoftProofTransform, bpc_for_lcms: bool) -> (f64, f64, usize) {
    let samples = sample_grid();
    let mut max_de = 0.0f64;
    let mut sum_de = 0.0f64;
    let mut n = 0usize;

    for rgb in samples {
        let src = rgb.to_vec();
        let mut mox_out = [0u8; 3];
        mox.apply_srgb8(&src, &mut mox_out).expect("moxcms apply");
        let lcms_out = lcms_two_step_rgb8(&src, Intent::RelativeColorimetric, bpc_for_lcms);
        let de = delta_e2000_srgb8(mox_out, [lcms_out[0], lcms_out[1], lcms_out[2]]);
        max_de = max_de.max(de);
        sum_de += de;
        n += 1;
    }
    (max_de, sum_de / n as f64, n)
}

#[test]
fn fogra51_relative_no_bpc_max_delta_e2000() {
    let mox = moxcms_proof(SoftProofIntent::Relative, false);
    let (max_de, mean, n) = stats(&mox, false);
    eprintln!(
        "lcms golden FOGRA51 Relative (no BPC): n={n} maxΔE2000={max_de:.3} meanΔE2000={mean:.3}"
    );
    assert!(
        max_de < MAX_DELTA_E2000_NO_BPC,
        "max ΔE2000={max_de:.3} exceeds {MAX_DELTA_E2000_NO_BPC}"
    );
}

#[test]
fn fogra51_relative_bpc_max_delta_e2000() {
    let mox = moxcms_proof(SoftProofIntent::Relative, true);
    let (max_de, mean, n) = stats(&mox, true);
    eprintln!("lcms golden FOGRA51 Relative+BPC: n={n} maxΔE2000={max_de:.3} meanΔE2000={mean:.3}");
    assert!(
        mean < MEAN_DELTA_E2000_WITH_BPC,
        "mean ΔE2000={mean:.3} exceeds {MEAN_DELTA_E2000_WITH_BPC}"
    );
    assert!(
        max_de < MAX_DELTA_E2000_WITH_BPC,
        "max ΔE2000={max_de:.3} exceeds {MAX_DELTA_E2000_WITH_BPC}"
    );
}

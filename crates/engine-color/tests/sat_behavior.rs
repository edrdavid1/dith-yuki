//! Soft-proof should pull colors toward the printer gamut (usually less chroma),
//! not boost saturation vs proof-off.

use engine_color::preview_encode::{delta_e2000_srgb8, encode_preview_rgb};
use engine_color::soft_proof::{
    ProofProfile, SoftProofIntent, SoftProofTransform, BUILTIN_FOGRA51_ICC,
};

/// Distance from equal-luma gray — correlates with perceived chroma.
fn chroma_vs_gray(rgb: [u8; 3]) -> f64 {
    let r = rgb[0] as f64 / 255.0;
    let g = rgb[1] as f64 / 255.0;
    let b = rgb[2] as f64 / 255.0;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let gray = [(y * 255.0) as u8; 3];
    delta_e2000_srgb8(rgb, gray)
}

#[test]
fn fogra51_relative_reduces_average_chroma() {
    let proof = ProofProfile::from_icc_bytes(BUILTIN_FOGRA51_ICC).unwrap();
    let xform = SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true).unwrap();

    let mut up = 0usize;
    let mut down = 0usize;
    let mut sum_dc = 0.0f64;
    let mut n = 0usize;

    for r in [0u8, 64, 128, 192, 255] {
        for g in [0u8, 64, 128, 192, 255] {
            for b in [0u8, 64, 128, 192, 255] {
                if r == g && g == b {
                    continue;
                }
                let enc = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0];
                let mut off = [0u8; 3];
                let mut on = [0u8; 3];
                encode_preview_rgb(&enc, &mut off, None).unwrap();
                encode_preview_rgb(&enc, &mut on, Some(&xform)).unwrap();
                let dc = chroma_vs_gray(on) - chroma_vs_gray(off);
                sum_dc += dc;
                n += 1;
                if dc > 0.5 {
                    up += 1;
                } else if dc < -0.5 {
                    down += 1;
                }
            }
        }
    }
    let mean = sum_dc / n as f64;
    assert!(
        mean < 0.0,
        "soft-proof should reduce average chroma (mean ΔC={mean:.3})"
    );
    assert!(
        down > up,
        "more colors should lose chroma than gain: down={down} up={up}"
    );
}

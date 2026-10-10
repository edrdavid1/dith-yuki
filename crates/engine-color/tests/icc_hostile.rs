//! Hostile / truncated ICC smoke (release-safe).
//!
//! Complements `catch_unwind` + structural precheck. Run under `--release` in CI
//! to prove panics unwind into `SoftProofError` rather than aborting the process.

use engine_color::soft_proof::{
    ProofProfile, SoftProofIntent, SoftProofTransform, BUILTIN_FOGRA51_ICC,
};
use engine_color::{precheck_icc_bytes, DisplayRgbF32};

fn mutate(seed: u64, bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    let mut x = seed;
    for _ in 0..64 {
        x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
        let idx = (x as usize) % out.len().max(1);
        let bit = ((x >> 32) as u8) & 7;
        if !out.is_empty() {
            out[idx] ^= 1u8 << bit;
        }
    }
    // Occasional truncation.
    if seed % 5 == 0 && out.len() > 200 {
        out.truncate(128 + (seed as usize % 400));
    }
    out
}

#[test]
fn corrupt_icc_never_panics_process() {
    // Empty / tiny
    for sample in [Vec::new(), vec![0u8; 16], vec![0u8; 127], vec![0xFFu8; 256]] {
        let _ = ProofProfile::from_icc_bytes(&sample);
    }

    // Mutated FOGRA51 corpus (seeded “fuzz”).
    for seed in 0..64u64 {
        let hostile = mutate(seed, BUILTIN_FOGRA51_ICC);
        let _ = precheck_icc_bytes(&hostile);
        if let Ok(proof) = ProofProfile::from_icc_bytes(&hostile) {
            if let Ok(xform) =
                SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true)
            {
                let src = [DisplayRgbF32::new(1.0, 0.0, 0.0)];
                let mut dst = [DisplayRgbF32::new(0.0, 0.0, 0.0)];
                let _ = xform.apply_display_rgb(&src, &mut dst);
            }
        }
    }
}

#[test]
fn real_profile_parse_and_apply_ok() {
    let proof = ProofProfile::from_icc_bytes(BUILTIN_FOGRA51_ICC).expect("FOGRA51");
    let xform = SoftProofTransform::new(&proof, SoftProofIntent::Relative, true).expect("xform");
    let src = [DisplayRgbF32::new(0.0, 0.0, 1.0)];
    let mut dst = [DisplayRgbF32::new(0.0, 0.0, 0.0)];
    xform.apply_display_rgb(&src, &mut dst).expect("apply");
    assert!(dst[0].b() < 1.0 || dst[0].r() > 0.0 || dst[0].g() > 0.0);
}

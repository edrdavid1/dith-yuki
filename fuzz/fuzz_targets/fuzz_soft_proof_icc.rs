//! Fuzz: ICC parse + soft-proof transform build (hostile / truncated profiles).

#![no_main]
use engine_color::soft_proof::{ProofProfile, SoftProofIntent, SoftProofTransform};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Soft limit mirrors product import cap.
    if data.len() > 8 * 1024 * 1024 || data.is_empty() {
        return;
    }
    let Ok(proof) = ProofProfile::from_icc_bytes(data) else {
        return;
    };
    let _ = SoftProofTransform::new_exact(&proof, SoftProofIntent::Relative, true);
    let _ = SoftProofTransform::new_exact(&proof, SoftProofIntent::Perceptual, false);
    let _ = SoftProofTransform::new_exact(&proof, SoftProofIntent::Absolute, false);
});

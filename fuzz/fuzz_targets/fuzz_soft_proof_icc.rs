//! Fuzz: ICC precheck + parse + soft-proof transform build + one pixel apply.

#![no_main]
use engine_color::soft_proof::{ProofProfile, SoftProofIntent, SoftProofTransform};
use engine_color::{precheck_icc_bytes, DisplayRgbF32};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Soft limit mirrors product import cap.
    if data.len() > 8 * 1024 * 1024 || data.is_empty() {
        return;
    }
    let _ = precheck_icc_bytes(data);
    let Ok(proof) = ProofProfile::from_icc_bytes(data) else {
        return;
    };
    for (intent, bpc) in [
        (SoftProofIntent::Relative, true),
        (SoftProofIntent::Perceptual, false),
        (SoftProofIntent::Absolute, false),
    ] {
        let Ok(xform) = SoftProofTransform::new_exact(&proof, intent, bpc) else {
            continue;
        };
        let src = [DisplayRgbF32::new(0.5, 0.25, 0.75)];
        let mut dst = [DisplayRgbF32::new(0.0, 0.0, 0.0)];
        let _ = xform.apply_display_rgb(&src, &mut dst);
    }
});

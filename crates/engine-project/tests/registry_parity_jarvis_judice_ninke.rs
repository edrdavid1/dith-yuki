//! Parity: Registry `jarvis_judice_ninke` vs the legacy error-diffusion path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_jarvis_judice_ninke() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::JarvisJudiceNinke);
    env.assert_gpu(
        "jarvis_judice_ninke",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion),
    );
    env.assert_ed("jarvis_judice_ninke", params);
}

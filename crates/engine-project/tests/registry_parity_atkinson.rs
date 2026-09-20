//! Parity: Registry `atkinson` vs the legacy error-diffusion path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_atkinson() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::Atkinson);
    env.assert_gpu(
        "atkinson",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion),
    );
    env.assert_ed("atkinson", params);
}

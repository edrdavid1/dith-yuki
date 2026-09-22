//! Parity: Registry `ostromoukhov` vs the error-diffusion path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_ostromoukhov() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::Ostromoukhov);
    env.assert_gpu(
        "ostromoukhov",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion),
    );
    env.assert_ed("ostromoukhov", params);
}

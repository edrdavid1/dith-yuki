//! Parity: Registry `floyd_steinberg` vs the legacy error-diffusion path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_floyd_steinberg() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::FloydSteinberg);
    env.assert_gpu(
        "floyd_steinberg",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion),
    );
    env.assert_ed("floyd_steinberg", params);
}

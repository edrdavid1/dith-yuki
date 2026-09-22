//! Parity: Registry `stevenson_arce` vs the error-diffusion path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_stevenson_arce() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::StevensonArce);
    env.assert_gpu(
        "stevenson_arce",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion),
    );
    env.assert_ed("stevenson_arce", params);
}

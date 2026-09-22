//! Parity: Registry `sierra_two_row` vs the error-diffusion path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_sierra_two_row() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::SierraTwoRow);
    env.assert_gpu(
        "sierra_two_row",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::ErrorDiffusion),
    );
    env.assert_ed("sierra_two_row", params);
}

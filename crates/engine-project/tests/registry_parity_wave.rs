//! Parity: Registry `wave` vs the legacy ordered-dither path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_wave() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::Wave);
    env.assert_gpu(
        "wave",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither),
    );
    env.assert_ordered("wave", params);
}

//! Parity: Registry `cmyk_halftone` vs the legacy ordered-dither path.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{DitherModeV2, FilterParams};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_parity_cmyk_halftone() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::CmykHalftone);
    env.assert_gpu(
        "cmyk_halftone",
        &FilterParams::DitherV2(params.clone()),
        GpuEligibility::Eligible,
    );
    let mut biased = params.clone();
    biased.threshold_bias = 0.1;
    env.assert_gpu(
        "cmyk_halftone",
        &FilterParams::DitherV2(biased),
        GpuEligibility::Cpu(CpuCheckpointKind::IneligibleDither),
    );
    env.assert_ordered("cmyk_halftone", params);
}

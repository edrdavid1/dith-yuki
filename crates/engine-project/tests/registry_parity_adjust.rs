//! Parity: Registry `adjust` vs `apply_adjust_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::FilterParams;
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::Env;

#[test]
fn registry_parity_adjust() {
    let env = Env::new();
    env.assert_gpu(
        "adjust",
        &FilterParams::Adjust {
            contrast: 0.2,
            brightness: 0.1,
            saturation: -0.1,
            blur: 0.0,
            sharpness: 0.5,
            noise: 0.0,
        },
        GpuEligibility::Eligible,
    );
    env.assert_gpu(
        "adjust",
        &FilterParams::Adjust {
            contrast: 0.0,
            brightness: 0.0,
            saturation: 0.0,
            blur: 1.0,
            sharpness: 0.0,
            noise: 0.0,
        },
        GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur),
    );
    env.assert_adjust(0.2, 0.1, -0.1, 0.0, 0.5, 0.0);
}

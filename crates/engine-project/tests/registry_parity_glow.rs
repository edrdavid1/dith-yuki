//! Parity: Registry `glow` vs `apply_glow_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::FilterParams;
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::Env;

#[test]
fn registry_parity_glow() {
    let env = Env::new();
    env.assert_gpu(
        "glow",
        &FilterParams::Glow {
            radius: 1.0,
            intensity: 1.0,
            threshold: 0.0,
        },
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
    );
    env.assert_glow(1.0, 1.0, 0.0);
}

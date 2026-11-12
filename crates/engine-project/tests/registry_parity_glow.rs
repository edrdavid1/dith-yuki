//! Parity: Registry `glow` vs `apply_glow_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::FilterParams;
use engine_registry::GpuEligibility;
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
            softness: 1,
        },
        GpuEligibility::Eligible,
    );
    env.assert_gpu(
        "glow",
        &FilterParams::Glow {
            radius: 8.0,
            intensity: 1.0,
            threshold: 0.35,
            softness: 2,
        },
        GpuEligibility::Cpu(engine_registry::CpuCheckpointKind::AdjustBlur),
    );
    env.assert_glow(1.0, 1.0, 0.0);
}

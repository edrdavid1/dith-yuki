//! Parity: Registry `glitch` vs `GlitchFilter::apply_to_tile_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::FilterParams;
use engine_project::filters::glitch::{GlitchParams, GlitchSubOp};
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::Env;

#[test]
fn registry_parity_glitch() {
    let env = Env::new();
    let params = GlitchParams {
        seed: 42,
        channel_swap: GlitchSubOp {
            enabled: true,
            amount: 50.0,
        },
        ..GlitchParams::default()
    };
    env.assert_gpu(
        "glitch",
        &FilterParams::Glitch(params.clone()),
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
    );
    env.assert_glitch(params);
}

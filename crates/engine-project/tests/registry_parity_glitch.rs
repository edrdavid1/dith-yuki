//! Parity: Registry `glitch` vs `GlitchFilter::apply_to_tile_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::FilterParams;
use engine_project::filters::GlitchType;
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::Env;

#[test]
fn registry_parity_glitch() {
    let env = Env::new();
    env.assert_gpu(
        "glitch",
        &FilterParams::Glitch {
            glitch_type: GlitchType::RGBShift,
            intensity: 0.5,
            seed: 42,
        },
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
    );
    env.assert_glitch(GlitchType::RGBShift, 0.5, 42);
}

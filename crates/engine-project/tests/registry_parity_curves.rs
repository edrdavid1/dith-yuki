//! Parity: Registry `curves` vs `CurvesFilter::apply_to_tile_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::FilterParams;
use engine_project::filters::CurveChannel;
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use parity_harness::Env;

#[test]
fn registry_parity_curves() {
    let env = Env::new();
    let curve = vec![(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)];
    env.assert_gpu(
        "curves",
        &FilterParams::Curves {
            curve: curve.clone(),
            channel: CurveChannel::All,
        },
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
    );
    env.assert_curves(curve, CurveChannel::All);
}

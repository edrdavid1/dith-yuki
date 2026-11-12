//! Parity: Registry `threshold` vs `apply_threshold_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{FilterKind, FilterParams};
use engine_project::filters::threshold::apply_threshold_into;
use engine_registry::GpuEligibility;
use engine_tiles::PixelTile;
use parity_harness::{gradient_tile, Env};

#[test]
fn registry_parity_threshold() {
    let env = Env::new();
    let params = FilterParams::Threshold { threshold: 0.5 };
    env.assert_gpu("threshold", &params, GpuEligibility::Eligible);

    let src = gradient_tile();
    let mut expected = PixelTile::new();
    apply_threshold_into(&src, 0.5, &mut expected);
    env.assert_registry("threshold", FilterKind::Threshold, params, &src, &expected);
}

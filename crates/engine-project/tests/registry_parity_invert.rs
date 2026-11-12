//! Parity: Registry `invert` vs `apply_invert_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{FilterKind, FilterParams};
use engine_project::filters::invert::apply_invert_into;
use engine_registry::GpuEligibility;
use engine_tiles::PixelTile;
use parity_harness::{gradient_tile, Env};

#[test]
fn registry_parity_invert() {
    let env = Env::new();
    let params = FilterParams::Invert {};
    env.assert_gpu("invert", &params, GpuEligibility::Eligible);

    let src = gradient_tile();
    let mut expected = PixelTile::new();
    apply_invert_into(&src, &mut expected);
    env.assert_registry("invert", FilterKind::Invert, params, &src, &expected);
}

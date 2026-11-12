//! Parity: Registry `levels` vs `LevelsFilter::apply_to_tile_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{FilterKind, FilterParams};
use engine_project::filters::levels::LevelsFilter;
use engine_registry::GpuEligibility;
use engine_tiles::PixelTile;
use parity_harness::{gradient_tile, Env};

#[test]
fn registry_parity_levels() {
    let env = Env::new();
    let params = FilterParams::Levels {
        input_black: 0.05,
        input_white: 0.95,
        gamma: 1.4,
        output_black: 0.0,
        output_white: 1.0,
        channel_r: true,
        channel_g: true,
        channel_b: true,
    };
    env.assert_gpu("levels", &params, GpuEligibility::Eligible);

    let src = gradient_tile();
    let mut filter = LevelsFilter::new();
    filter.input_black = 0.05;
    filter.input_white = 0.95;
    filter.gamma = 1.4;
    filter.rebuild_lut();
    let mut expected = PixelTile::new();
    filter.apply_to_tile_into(&src, &mut expected).unwrap();
    env.assert_registry("levels", FilterKind::Levels, params, &src, &expected);
}

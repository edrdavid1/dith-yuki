//! Parity: Registry `posterize` vs `apply_posterize_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_project::filter::{FilterKind, FilterParams};
use engine_project::filters::posterize::apply_posterize_into;
use engine_registry::{CpuCheckpointKind, GpuEligibility};
use engine_tiles::PixelTile;
use parity_harness::{gradient_tile, Env};

#[test]
fn registry_parity_posterize() {
    let env = Env::new();
    let params = FilterParams::Posterize { levels: 8 };
    env.assert_gpu(
        "posterize",
        &params,
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
    );

    let src = gradient_tile();
    let mut expected = PixelTile::new();
    apply_posterize_into(&src, 8, &mut expected);
    env.assert_registry("posterize", FilterKind::Posterize, params, &src, &expected);
}

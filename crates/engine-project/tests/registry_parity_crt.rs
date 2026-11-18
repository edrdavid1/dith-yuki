//! Parity: Registry `crt` vs `apply_crt_into`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use engine_registry::GpuEligibility;
use parity_harness::Env;

#[test]
fn registry_parity_crt() {
    let env = Env::new();
    env.assert_gpu(
        "crt",
        &engine_project::filter::crt_params(2, 0.5, 0.25),
        GpuEligibility::Eligible,
    );
    env.assert_crt(2, 0.5, 0.25);
}

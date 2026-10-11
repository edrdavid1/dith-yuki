//! Full GPU↔CPU parity matrix for `blocked_fs` and `blue_noise`.
//!
//! Spec: `.local-doc/Dither полный тест паритета GPU и CPU.md`  
//! Policy: [`docs/adr/gpu-dither-families.md`](../../../docs/adr/gpu-dither-families.md)
//!
//! | Mode | When |
//! |---|---|
//! | `quick` | Every CI commit (minutes; category subset) |
//! | `full` | Before release (deterministic; multi-hour) |
//! | `random` | Optional soak; separate from `full` |
//!
//! ```bash
//! cargo test -p engine-project --test gpu_parity_matrix --release --no-run
//!
//! # CI / every commit
//! PARITY_MODE=quick PARITY_REPEAT=1 PARITY_SEED=0xc1 \
//!   cargo test -p engine-project --test gpu_parity_matrix --release -- --ignored --nocapture --test-threads=1
//!
//! # Pre-release
//! PARITY_MODE=full PARITY_SEED=0xC0FFEE PARITY_REPEAT=3 \
//!   cargo test -p engine-project --test gpu_parity_matrix --release -- --ignored --nocapture --test-threads=1
//!
//! # Resume: PARITY_SKIP from parity_progress.txt
//! ```

#[path = "gpu_parity/mod.rs"]
mod gpu_parity;

#[test]
#[ignore = "requires GPU adapter; long-running parity matrix"]
fn gpu_cpu_parity_matrix() {
    gpu_parity::run_parity_matrix();
}

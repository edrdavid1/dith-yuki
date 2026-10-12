//! Dedicated rayon pool for full-document pixel loops.
//!
//! Tile workers are plain `std::thread`s (`main.rs`), not rayon workers. The
//! global rayon pool is therefore free when a worker runs a full-document
//! apply. This dedicated pool still isolates pixel work from any other rayon
//! use (e.g. multi-layer `compute_processed_tile` scopes) so helpers are always
//! available to the leader.

use std::sync::OnceLock;

use rayon::ThreadPool;
use rayon::ThreadPoolBuilder;

fn pool() -> &'static ThreadPool {
    static POOL: OnceLock<ThreadPool> = OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8);
        ThreadPoolBuilder::new()
            .num_threads(n)
            .thread_name(|i| format!("pixel-pool-{i}"))
            .build()
            .expect("pixel-pool rayon build")
    })
}

/// Run `f` on the dedicated pixel pool (installs as current rayon pool).
pub fn install<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    pool().install(f)
}

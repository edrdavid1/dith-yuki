//! # engine-tiles
//!
//! Tile cache, pyramid, and scheduler infrastructure.
//!
//! For architecture details, see `tile-engine-architecture.md`.

pub mod types;
pub use types::*;

pub mod tile;
pub use tile::*;

pub mod cache;
pub use cache::*;

pub mod pyramid;
pub use pyramid::*;

pub mod generation;
pub use generation::*;

pub mod scheduler;
pub use scheduler::*;

pub mod invalidation;
pub use invalidation::*;

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}

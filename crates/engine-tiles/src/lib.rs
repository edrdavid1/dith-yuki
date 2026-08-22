//! # engine-tiles
//!
//! Tile cache, pyramid, and scheduler infrastructure.
//!
//! For architecture details, see `tile-engine-architecture.md`.

pub mod types;
pub use types::*;

pub mod coords;
pub use coords::*;

pub mod block_cache;
pub use block_cache::*;

pub mod tile;
pub use tile::*;

pub mod park;
pub use park::*;

pub mod cache;
pub use cache::*;

pub mod pyramid;
pub use pyramid::*;

pub mod generation;
pub use generation::*;

pub mod scheduler;
pub use scheduler::*;

pub mod ed;
pub use ed::*;

pub mod invalidation;
pub use invalidation::*;

pub mod decompose;
pub use decompose::*;

#[cfg(test)]
mod tests {
    #[test]
    fn stub_compiles() {
        assert!(true);
    }
}

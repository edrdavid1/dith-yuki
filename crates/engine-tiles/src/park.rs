//! Amortised worker-local park of owned [`PixelTile`] buffers.
//!
//! Model (tile-memory-inplace Wave 2):
//! - Keep at most **2** free tiles in the park.
//! - `take` pops or allocates; `give` returns a spare (drops if already full).
//! - After a successful Processed compute, one buffer becomes `Arc` (leaves the
//!   park forever); the other returns via `give` → next task amortises ~1 alloc.

use std::cell::RefCell;

use crate::PixelTile;

/// Soft cap on free buffers held between tasks.
pub const TILE_PARK_CAPACITY: usize = 2;

/// Owned-tile park for ping-pong filter apply / empty-filter copy.
#[derive(Default)]
pub struct TileBufferPark {
    free: Vec<PixelTile>,
}

impl TileBufferPark {
    pub fn new() -> Self {
        Self { free: Vec::new() }
    }

    /// Number of buffers currently sitting in the park (not in use).
    pub fn len(&self) -> usize {
        self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.free.is_empty()
    }

    /// Ensure at least `n` free buffers (clamped to [`TILE_PARK_CAPACITY`]).
    pub fn ensure(&mut self, n: usize) {
        let target = n.min(TILE_PARK_CAPACITY);
        while self.free.len() < target {
            self.free.push(PixelTile::new());
        }
    }

    /// Take one owned buffer (allocate if the park is empty).
    pub fn take(&mut self) -> PixelTile {
        self.free.pop().unwrap_or_else(PixelTile::new)
    }

    /// Return a spare buffer to the park. Excess beyond capacity is dropped.
    pub fn give(&mut self, tile: PixelTile) {
        if self.free.len() < TILE_PARK_CAPACITY {
            self.free.push(tile);
        }
    }
}

thread_local! {
    static THREAD_PARK: RefCell<TileBufferPark> = RefCell::new(TileBufferPark::new());
}

/// Run `f` with this thread's amortised [`TileBufferPark`].
pub fn with_tile_buffer_park<R>(f: impl FnOnce(&mut TileBufferPark) -> R) -> R {
    THREAD_PARK.with(|cell| f(&mut cell.borrow_mut()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_caps_at_two() {
        let mut park = TileBufferPark::new();
        park.ensure(10);
        assert_eq!(park.len(), TILE_PARK_CAPACITY);
    }

    #[test]
    fn take_give_round_trip() {
        let mut park = TileBufferPark::new();
        park.ensure(2);
        let a = park.take();
        let b = park.take();
        assert!(park.is_empty());
        park.give(a);
        park.give(b);
        assert_eq!(park.len(), 2);
        // Third give is dropped (capacity).
        park.give(PixelTile::new());
        assert_eq!(park.len(), 2);
    }

    #[test]
    fn thread_park_survives_across_calls() {
        with_tile_buffer_park(|p| {
            p.ensure(2);
            let t = p.take();
            p.give(t);
        });
        with_tile_buffer_park(|p| {
            assert!(p.len() >= 1, "spare should remain after give");
        });
    }
}

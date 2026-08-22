//! Resident slot allocator (free-list).

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use crossbeam::queue::SegQueue;
use engine_tiles::CacheStage;

/// Index into the resident `Texture2DArray`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotHandle {
    pub index: u32,
}

/// Metadata for a resident GPU slot.
#[derive(Clone, Debug)]
pub struct GpuSlotMeta {
    pub slot: SlotHandle,
    pub generation: u64,
    pub stage: CacheStage,
    pub last_touched: Instant,
}

/// Free-list allocator over `[0, capacity)`.
pub struct SlotAllocator {
    free: SegQueue<u32>,
    capacity: u32,
    live: AtomicU32,
}

impl SlotAllocator {
    pub fn new(capacity: u32) -> Self {
        let alloc = Self {
            free: SegQueue::new(),
            capacity,
            live: AtomicU32::new(0),
        };
        for i in 0..capacity {
            alloc.free.push(i);
        }
        alloc
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    pub fn live_count(&self) -> u32 {
        self.live.load(Ordering::Relaxed)
    }

    pub fn free_count(&self) -> u32 {
        self.capacity.saturating_sub(self.live_count())
    }

    pub fn alloc(&self) -> Option<SlotHandle> {
        let index = self.free.pop()?;
        self.live.fetch_add(1, Ordering::Relaxed);
        Some(SlotHandle { index })
    }

    pub fn free(&self, slot: SlotHandle) {
        debug_assert!(slot.index < self.capacity);
        if slot.index >= self.capacity {
            return;
        }
        let prev = self.live.fetch_sub(1, Ordering::Relaxed);
        debug_assert!(prev > 0, "double-free slot {}", slot.index);
        self.free.push(slot.index);
    }

    /// Test helper: mark `index` allocated without going through `alloc()` order.
    #[cfg(test)]
    pub fn reserve(&self, index: u32) -> bool {
        if index >= self.capacity {
            return false;
        }
        let mut requeue = Vec::new();
        let mut found = false;
        while let Some(i) = self.free.pop() {
            if i == index {
                found = true;
            } else {
                requeue.push(i);
            }
        }
        for i in requeue {
            self.free.push(i);
        }
        if found {
            self.live.fetch_add(1, Ordering::Relaxed);
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alloc_until_exhausted_then_none() {
        let a = SlotAllocator::new(3);
        assert_eq!(a.alloc().unwrap().index, 0);
        assert_eq!(a.alloc().unwrap().index, 1);
        assert_eq!(a.alloc().unwrap().index, 2);
        assert!(a.alloc().is_none());
        assert_eq!(a.live_count(), 3);
    }

    #[test]
    fn free_and_realloc() {
        let a = SlotAllocator::new(2);
        let s0 = a.alloc().unwrap();
        let _s1 = a.alloc().unwrap();
        assert!(a.alloc().is_none());
        a.free(s0);
        assert_eq!(a.alloc().unwrap().index, 0);
    }
}

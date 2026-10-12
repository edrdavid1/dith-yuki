//! SplitMix64 — stable across crate versions (not `rand`).

#[derive(Clone, Debug)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E3779B97F4A7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^ (z >> 31)
    }

    pub fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    pub fn gen_range_u32(&mut self, lo: u32, hi_inclusive: u32) -> u32 {
        debug_assert!(lo <= hi_inclusive);
        let span = (hi_inclusive - lo) as u64 + 1;
        lo + (self.next_u64() % span) as u32
    }

    pub fn gen_u8(&mut self) -> u8 {
        self.next_u32() as u8
    }

    pub fn choose<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.next_u64() as usize % items.len()]
    }
}

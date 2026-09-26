//! Void-and-cluster dither array generation (Ulichney, SPIE 1993).
//!
//! Builds a rank matrix where every integer in `0..SIZE²` appears once. The
//! matrix is generated once ([`OnceLock`]) with a fixed PRNG seed so thresholds
//! are process-stable. σ = 1.5 matches Ulichney's recommended Gaussian for
//! isotropic blue-noise arrays.

use std::sync::OnceLock;

/// Side length of the built-in void-and-cluster threshold map.
pub const SIZE: usize = 64;
/// Ulichney-recommended Gaussian σ for blue-noise arrays.
pub const SIGMA: f32 = 1.5;
/// Initial minority-pixel fraction for the prototype binary pattern.
pub const INITIAL_SEED_FRACTION: f32 = 0.1;
/// Deterministic seed for the (only) randomised step of the algorithm.
pub const GENERATOR_SEED: u64 = 0x5641_434c_556c_6963; // "VACUlich"

/// Flat rank array, row-major, values in `0..SIZE*SIZE` (each exactly once).
pub fn ranks() -> &'static [u16] {
    static RANKS: OnceLock<Vec<u16>> = OnceLock::new();
    RANKS.get_or_init(|| generate_ranks(SIZE, SIGMA, INITIAL_SEED_FRACTION, GENERATOR_SEED))
}

/// Threshold in `[0, 1)` for matrix cell `(mx, my)`.
#[inline]
pub fn threshold(mx: usize, my: usize) -> f32 {
    let n = SIZE * SIZE;
    ranks()[my * SIZE + mx] as f32 / n as f32
}

type Kernel = Vec<(i32, i32, f32)>;

/// Generate a void-and-cluster rank array (tests / alternate sizes).
pub fn generate_ranks(size: usize, sigma: f32, seed_fraction: f32, seed: u64) -> Vec<u16> {
    assert!(size >= 2, "void-and-cluster size must be ≥ 2");
    let n = size * size;
    let n_initial = ((n as f32) * seed_fraction)
        .round()
        .clamp(1.0, ((n - 1) / 2) as f32) as usize;

    let kernel = build_kernel(sigma, size);
    let mut ones = vec![false; n];
    let mut energy = vec![0.0f32; n];

    // Deterministic white-noise seed of minority pixels.
    let mut order: Vec<usize> = (0..n).collect();
    shuffle_with_seed(&mut order, seed);
    for &idx in order.iter().take(n_initial) {
        set_one(&mut ones, &mut energy, &kernel, size, idx, true);
    }

    // Relax: move tightest cluster → largest void until stable.
    loop {
        let cluster = find_tightest_cluster(&ones, &energy);
        set_one(&mut ones, &mut energy, &kernel, size, cluster, false);
        let void = find_largest_void(&ones, &energy);
        if void == cluster {
            set_one(&mut ones, &mut energy, &kernel, size, cluster, true);
            break;
        }
        set_one(&mut ones, &mut energy, &kernel, size, void, true);
    }

    let prototype = ones.clone();
    let mut ranks_out = vec![0u16; n];

    // Phase 1: rank minority pixels (remove tightest cluster → assign descending ranks).
    {
        let mut pattern = prototype.clone();
        let mut e = energy_from_ones(&pattern, &kernel, size);
        for rank in (0..n_initial).rev() {
            let i = find_tightest_cluster(&pattern, &e);
            set_one(&mut pattern, &mut e, &kernel, size, i, false);
            ranks_out[i] = rank as u16;
        }
    }

    // Phase 2 + 3: insert into largest voids until the array is full.
    // When ones are the majority, Python's FindTightestCluster (flipped) and
    // FindLargestVoid (flipped) both select among the remaining zeros; continuing
    // with largest-void insertion matches MomentsInGraphics / demofox Phase 2–3.
    let half = n.div_ceil(2);
    {
        let mut pattern = prototype;
        let mut e = energy_from_ones(&pattern, &kernel, size);
        for rank in n_initial..n {
            let i = if rank < half {
                find_largest_void(&pattern, &e)
            } else {
                // Phase 3: densest cluster of zeros ≡ min ones-energy among zeros
                // (same finder as largest void while ones are still minority; once
                // majority, FindLargestVoid flips — use densest-zeros explicitly).
                find_densest_zeros(&pattern, &e)
            };
            set_one(&mut pattern, &mut e, &kernel, size, i, true);
            ranks_out[i] = rank as u16;
        }
    }

    ranks_out
}

fn build_kernel(sigma: f32, size: usize) -> Kernel {
    // Extent: Ulichney notes truncation below ~8σ harms wrap-around quality.
    let radius = ((sigma * 8.0).ceil() as i32).max(3).min(size as i32 / 2);
    let two_s2 = 2.0 * sigma * sigma;
    let mut k = Kernel::new();
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let d2 = (dx * dx + dy * dy) as f32;
            let w = (-d2 / two_s2).exp();
            if w > 1e-8 {
                k.push((dx, dy, w));
            }
        }
    }
    k
}

fn set_one(
    ones: &mut [bool],
    energy: &mut [f32],
    kernel: &Kernel,
    size: usize,
    idx: usize,
    value: bool,
) {
    if ones[idx] == value {
        return;
    }
    ones[idx] = value;
    let sign = if value { 1.0f32 } else { -1.0f32 };
    apply_kernel_delta(energy, kernel, size, idx, sign);
}

fn apply_kernel_delta(energy: &mut [f32], kernel: &Kernel, size: usize, idx: usize, sign: f32) {
    let y0 = (idx / size) as i32;
    let x0 = (idx % size) as i32;
    let s = size as i32;
    for &(dx, dy, w) in kernel {
        let y = (y0 + dy).rem_euclid(s) as usize;
        let x = (x0 + dx).rem_euclid(s) as usize;
        energy[y * size + x] += sign * w;
    }
}

fn energy_from_ones(ones: &[bool], kernel: &Kernel, size: usize) -> Vec<f32> {
    let mut energy = vec![0.0f32; ones.len()];
    for (i, &is_one) in ones.iter().enumerate() {
        if is_one {
            apply_kernel_delta(&mut energy, kernel, size, i, 1.0);
        }
    }
    energy
}

fn find_tightest_cluster(ones: &[bool], energy: &[f32]) -> usize {
    let ones_count = ones.iter().filter(|&&o| o).count();
    if ones_count * 2 >= ones.len() {
        // Majority ones → flip: densest zeros (min ones-energy among zeros).
        find_extreme(ones, energy, false, false)
    } else {
        find_extreme(ones, energy, true, true)
    }
}

fn find_largest_void(ones: &[bool], energy: &[f32]) -> usize {
    let ones_count = ones.iter().filter(|&&o| o).count();
    if ones_count * 2 >= ones.len() {
        // Majority ones → flip: argmin gaussian(zeros) among ones
        // = argmax ones-energy among ones.
        find_extreme(ones, energy, true, true)
    } else {
        find_extreme(ones, energy, false, false)
    }
}

fn find_densest_zeros(ones: &[bool], energy: &[f32]) -> usize {
    // Max gaussian(zeros) among zeros ≡ min ones-energy among zeros.
    find_extreme(ones, energy, false, false)
}

fn find_extreme(ones: &[bool], energy: &[f32], among_ones: bool, maximize: bool) -> usize {
    let mut best_i = usize::MAX;
    let mut best_v = if maximize {
        f32::NEG_INFINITY
    } else {
        f32::INFINITY
    };
    for (i, &is_one) in ones.iter().enumerate() {
        if is_one != among_ones {
            continue;
        }
        let v = energy[i];
        let better = if maximize { v > best_v } else { v < best_v };
        if best_i == usize::MAX || better {
            best_v = v;
            best_i = i;
        }
    }
    debug_assert!(best_i != usize::MAX, "void-and-cluster finder empty set");
    best_i
}

/// Deterministic Fisher–Yates (SplitMix64).
fn shuffle_with_seed(items: &mut [usize], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        state = splitmix64(state);
        let j = (state as usize) % (i + 1);
        items.swap(i, j);
    }
}

fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_are_permutation_of_0_to_n_minus_1() {
        let r = generate_ranks(16, 1.5, 0.1, GENERATOR_SEED);
        assert_eq!(r.len(), 256);
        let mut sorted = r.clone();
        sorted.sort_unstable();
        for (i, &v) in sorted.iter().enumerate() {
            assert_eq!(v as usize, i);
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let a = generate_ranks(16, 1.5, 0.1, GENERATOR_SEED);
        let b = generate_ranks(16, 1.5, 0.1, GENERATOR_SEED);
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_differ() {
        let a = generate_ranks(16, 1.5, 0.1, 1);
        let b = generate_ranks(16, 1.5, 0.1, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn builtin_64_ranks_unique() {
        let r = ranks();
        assert_eq!(r.len(), SIZE * SIZE);
        let mut seen = vec![false; SIZE * SIZE];
        for &v in r {
            assert!(!seen[v as usize], "duplicate rank {v}");
            seen[v as usize] = true;
        }
    }
}

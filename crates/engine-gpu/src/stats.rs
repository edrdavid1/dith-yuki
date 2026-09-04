//! A3 gate: auto-dispatch must not be significantly slower than CPU.

/// True if `candidate` is not significantly worse than `baseline` (same n).
///
/// Worse = higher median **and** the gap exceeds a rough 95% SE band
/// (`1.96 * sqrt(σ_c²/n + σ_b²/n)`), matching industrial-gate CI overlap.
pub fn not_worse_than(
    candidate_median: f64,
    candidate_sigma: f64,
    baseline_median: f64,
    baseline_sigma: f64,
    n: usize,
) -> bool {
    if n == 0 {
        return false;
    }
    if candidate_median <= baseline_median {
        return true;
    }
    let se = ((candidate_sigma.powi(2) / n as f64) + (baseline_sigma.powi(2) / n as f64)).sqrt();
    (candidate_median - baseline_median) < 1.96 * se
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faster_or_equal_passes() {
        assert!(not_worse_than(10.0, 1.0, 12.0, 1.0, 20));
        assert!(not_worse_than(10.0, 1.0, 10.0, 1.0, 20));
    }

    #[test]
    fn noisy_slower_passes() {
        assert!(not_worse_than(10.2, 3.0, 10.0, 3.0, 20));
    }

    #[test]
    fn clearly_slower_fails() {
        assert!(!not_worse_than(40.0, 1.0, 10.0, 1.0, 20));
    }
}

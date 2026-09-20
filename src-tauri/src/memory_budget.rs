//! Track C Phase 1: process RAM tile-cache budget from system memory.

/// Current hardcoded floor (do not regress weak machines).
pub const MIN_RAM_BUDGET_BYTES: usize = 512 * 1024 * 1024;
/// Cap so a 64 GiB machine still leaves RAM for the OS and other apps.
pub const MAX_RAM_BUDGET_BYTES: usize = 4 * 1024 * 1024 * 1024;

/// How the session RAM budget was chosen (diagnostics / T0.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RamBudgetSource {
    Adaptive,
    MinFallback,
    EnvOverride,
    TestOverride,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RamBudget {
    pub bytes: usize,
    pub source: RamBudgetSource,
    pub total_system_ram: u64,
}

/// Pure clamp: 25% of system RAM, bounded by MIN/MAX.
pub fn compute_ram_budget(total_system_ram: u64) -> usize {
    let share = total_system_ram / 4;
    (share as usize).clamp(MIN_RAM_BUDGET_BYTES, MAX_RAM_BUDGET_BYTES)
}

pub fn ram_budget_source(total_system_ram: u64, bytes: usize) -> RamBudgetSource {
    if (total_system_ram / 4) as usize <= MIN_RAM_BUDGET_BYTES {
        RamBudgetSource::MinFallback
    } else if bytes == compute_ram_budget(total_system_ram) {
        RamBudgetSource::Adaptive
    } else {
        RamBudgetSource::Adaptive
    }
}

/// `DITHER_RAM_BUDGET_MIB` wins when set to a positive integer.
pub fn resolve_ram_budget_from_env(total_system_ram: u64) -> Option<RamBudget> {
    let raw = std::env::var("DITHER_RAM_BUDGET_MIB").ok()?;
    let mib: u64 = raw.parse().ok()?;
    if mib == 0 {
        return None;
    }
    Some(RamBudget {
        bytes: (mib as usize).saturating_mul(1024 * 1024),
        source: RamBudgetSource::EnvOverride,
        total_system_ram,
    })
}

pub fn resolve_ram_budget() -> RamBudget {
    let total = query_total_system_ram();
    if let Some(over) = resolve_ram_budget_from_env(total) {
        return over;
    }
    let bytes = compute_ram_budget(total);
    let source = if (total / 4) as usize <= MIN_RAM_BUDGET_BYTES {
        RamBudgetSource::MinFallback
    } else {
        RamBudgetSource::Adaptive
    };
    RamBudget {
        bytes,
        source,
        total_system_ram: total,
    }
}

fn query_total_system_ram() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn two_gib_clamps_to_min() {
        // 25% of 2 GiB == 512 MiB == MIN
        assert_eq!(compute_ram_budget(2 * GIB), MIN_RAM_BUDGET_BYTES);
        assert_eq!(
            ram_budget_source(2 * GIB, MIN_RAM_BUDGET_BYTES),
            RamBudgetSource::MinFallback
        );
    }

    #[test]
    fn eight_gib_is_quarter() {
        assert_eq!(compute_ram_budget(8 * GIB), 2 * GIB as usize);
        assert_eq!(
            ram_budget_source(8 * GIB, 2 * GIB as usize),
            RamBudgetSource::Adaptive
        );
    }

    #[test]
    fn thirty_two_gib_clamps_to_max() {
        // 25% of 32 GiB == 8 GiB → MAX 4 GiB
        assert_eq!(compute_ram_budget(32 * GIB), MAX_RAM_BUDGET_BYTES);
    }

    #[test]
    fn sixty_four_gib_clamps_to_max() {
        assert_eq!(compute_ram_budget(64 * GIB), MAX_RAM_BUDGET_BYTES);
    }
}

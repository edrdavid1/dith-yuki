//! Tile geometry variants for `blocked_fs` (Part A).
//!
//! Default remains variant **A** (56×56 core + 8 pad, 64 threads). Changing the
//! default requires an owner decision (spec §3.7) and `algo_version` (§3.8).

use super::gpu_dither_cpu::{CORE, PAD, WG_SIZE};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryVariant {
    pub name: &'static str,
    pub core_w: u32,
    pub core_h: u32,
    pub pad: u32,
    pub threads: u32,
}

impl GeometryVariant {
    pub const fn wg_w(self) -> u32 {
        self.core_w + self.pad
    }
    pub const fn wg_h(self) -> u32 {
        self.core_h + self.pad
    }

    /// Declared workgroup storage bytes (memory C: err ring only, no tile cache).
    /// `3 channels × 4 slots × (wg_h + 2) rows × 4 bytes`.
    pub const fn workgroup_bytes(self) -> u64 {
        let rows = (self.wg_h() as u64) + 2;
        3 * 4 * rows * 4
    }

    /// Meets the blocked_fs device-create floor (32 KiB reported limit).
    pub const fn meets_device_create_floor(self) -> bool {
        self.threads <= 256 && self.workgroup_bytes() <= 32 * 1024
    }
}

pub const VARIANT_A: GeometryVariant = GeometryVariant {
    name: "A",
    core_w: 56,
    core_h: 56,
    pad: 8,
    threads: 64,
};

pub const VARIANT_B: GeometryVariant = GeometryVariant {
    name: "B",
    core_w: 120,
    core_h: 56,
    pad: 8,
    threads: 64,
};

pub const VARIANT_C: GeometryVariant = GeometryVariant {
    name: "C",
    core_w: 248,
    core_h: 56,
    pad: 8,
    threads: 64,
};

pub const VARIANT_D: GeometryVariant = GeometryVariant {
    name: "D",
    core_w: 248,
    core_h: 120,
    pad: 8,
    threads: 128,
};

pub const ALL_VARIANTS: &[GeometryVariant] = &[VARIANT_A, VARIANT_B, VARIANT_C, VARIANT_D];

/// Shipping default — must stay A until owner confirms otherwise.
pub fn default_geometry() -> GeometryVariant {
    debug_assert_eq!(VARIANT_A.core_w, CORE);
    debug_assert_eq!(VARIANT_A.pad, PAD);
    debug_assert_eq!(VARIANT_A.wg_w(), WG_SIZE);
    VARIANT_A
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_a_and_matches_cpu_constants() {
        let g = default_geometry();
        assert_eq!(g.name, "A");
        assert_eq!(g.core_w, CORE);
        assert_eq!(g.core_h, CORE);
        assert_eq!(g.pad, PAD);
    }

    #[test]
    fn a_declared_bytes_match_constant() {
        assert_eq!(VARIANT_A.workgroup_bytes(), 3168);
        assert_eq!(
            VARIANT_A.workgroup_bytes(),
            u64::from(engine_gpu::GEOMETRY_A_WORKGROUP_BYTES)
        );
        assert!(VARIANT_A.meets_device_create_floor()); // 3168 ≪ 32768
        assert!(VARIANT_B.meets_device_create_floor());
    }
}

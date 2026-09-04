//! Per-tile GPU vs CPU preview dispatch (auto-dispatch A1).
//!
//! No wgpu. Slot warmth is a boolean from `GpuTileCache::get_slot`.

/// Inputs for one tile. Stack eligibility is computed by the caller (filters / mask / groups).
#[derive(Clone, Copy, Debug)]
pub struct TileDispatchInput {
    pub force_cpu: bool,
    pub gpu_available: bool,
    pub preview_opt_in: bool,
    pub stack_eligible: bool,
    pub level: u32,
    pub composite_slot_warm: bool,
}

/// Where this tile's Composite should be authored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileDispatch {
    Cpu,
    /// Resident Composite slot matches `document_gen` — readback only.
    GpuDownload,
    /// Cold eligible tile; opt-in allows promote + graph.
    GpuCompute,
}

/// Decide GPU vs CPU for one tile. Warm download does not require opt-in.
pub fn decide_tile_dispatch(input: TileDispatchInput) -> TileDispatch {
    if input.force_cpu || !input.gpu_available || !input.stack_eligible || input.level != 0 {
        return TileDispatch::Cpu;
    }
    if input.composite_slot_warm {
        return TileDispatch::GpuDownload;
    }
    if input.preview_opt_in {
        return TileDispatch::GpuCompute;
    }
    TileDispatch::Cpu
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> TileDispatchInput {
        TileDispatchInput {
            force_cpu: false,
            gpu_available: true,
            preview_opt_in: false,
            stack_eligible: true,
            level: 0,
            composite_slot_warm: false,
        }
    }

    #[test]
    fn decision_table() {
        let cases: &[(&str, TileDispatchInput, TileDispatch)] = &[
            ("default cold", base(), TileDispatch::Cpu),
            (
                "warm no opt-in",
                TileDispatchInput {
                    composite_slot_warm: true,
                    ..base()
                },
                TileDispatch::GpuDownload,
            ),
            (
                "cold opt-in",
                TileDispatchInput {
                    preview_opt_in: true,
                    ..base()
                },
                TileDispatch::GpuCompute,
            ),
            (
                "warm wins over opt-in",
                TileDispatchInput {
                    preview_opt_in: true,
                    composite_slot_warm: true,
                    ..base()
                },
                TileDispatch::GpuDownload,
            ),
            (
                "force cpu",
                TileDispatchInput {
                    force_cpu: true,
                    composite_slot_warm: true,
                    preview_opt_in: true,
                    ..base()
                },
                TileDispatch::Cpu,
            ),
            (
                "no gpu",
                TileDispatchInput {
                    gpu_available: false,
                    composite_slot_warm: true,
                    preview_opt_in: true,
                    ..base()
                },
                TileDispatch::Cpu,
            ),
            (
                "ineligible stack",
                TileDispatchInput {
                    stack_eligible: false,
                    composite_slot_warm: true,
                    preview_opt_in: true,
                    ..base()
                },
                TileDispatch::Cpu,
            ),
            (
                "pyramid",
                TileDispatchInput {
                    level: 1,
                    composite_slot_warm: true,
                    preview_opt_in: true,
                    ..base()
                },
                TileDispatch::Cpu,
            ),
        ];
        for (name, input, expect) in cases {
            assert_eq!(decide_tile_dispatch(*input), *expect, "{name}");
        }
    }
}

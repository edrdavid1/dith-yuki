//! Riemersma (`riemersma`) — Hilbert-curve error diffusion (full-document).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, ExecutionScope, FilterAlgorithm, FilterCtx,
    FilterError, GpuEligibility, ParamField,
};
use engine_tiles::{PixelTile, HALO, TILE_SIZE};

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::riemersma::apply_riemersma_rgba;

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "levels",
        label: "Levels",
        min: 2.0,
        max: 256.0,
        default: 4.0,
        step: None,
    },
    ParamField::Slider {
        key: "threshold_scale",
        label: "Threshold Scale",
        min: 0.1,
        max: 4.0,
        default: 1.0,
        step: None,
    },
    ParamField::Dropdown {
        key: "color_mode",
        label: "Color Mode",
        options: &[("rgb", "RGB"), ("grayscale", "Grayscale")],
        default: "rgb",
    },
];

pub struct Riemersma;

impl FilterAlgorithm for Riemersma {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("riemersma")
    }

    fn display_name(&self) -> &'static str {
        "Riemersma"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        _ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        // Tile-local apply exists for isolated unit tests only. Production
        // preview/export must use the FullDocument path (§0.1) — Hilbert error
        // does not terminate at tile boundaries.
        let mut params: DitherParamsV2 = serde_json::from_value(params.clone())?;
        params.mode = DitherModeV2::Riemersma;

        let src = scratch_src(tile);
        let w = TILE_SIZE;
        let h = TILE_SIZE;
        let mut rgba = vec![0.0f32; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let sx = HALO + x;
                let sy = HALO + y;
                rgba[i] = src.at(sx, sy, 0);
                rgba[i + 1] = src.at(sx, sy, 1);
                rgba[i + 2] = src.at(sx, sy, 2);
                rgba[i + 3] = src.at(sx, sy, 3);
            }
        }

        let cancel = std::sync::atomic::AtomicBool::new(false);
        apply_riemersma_rgba(&mut rgba, w, h, &params, &cancel)
            .map_err(FilterError::from_engine)?;

        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let dx = HALO + x;
                let dy = HALO + y;
                tile.set(dx, dy, 0, rgba[i]);
                tile.set(dx, dy, 1, rgba[i + 1]);
                tile.set(dx, dy, 2, rgba[i + 2]);
                tile.set(dx, dy, 3, rgba[i + 3]);
            }
        }
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::SequentialGlobalDependency)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Dithering
    }

    fn requires_full_row(&self) -> bool {
        false
    }

    fn execution_scope(&self) -> ExecutionScope {
        ExecutionScope::FullDocument
    }
}

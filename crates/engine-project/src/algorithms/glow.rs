//! Glow / bloom (`glow`).

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::{PixelTile, HALO};
use serde::Deserialize;

use crate::filters::context::FilterContext;
use crate::filters::glow::{apply_glow_into_at, glow_spatial_radius_px};

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "radius",
        label: "Radius",
        min: 0.5,
        max: 16.0,
        default: 4.0,
        step: Some(0.5),
    },
    ParamField::Slider {
        key: "intensity",
        label: "Intensity",
        min: 0.0,
        max: 4.0,
        default: 1.0,
        step: Some(0.1),
    },
    ParamField::Slider {
        key: "threshold",
        label: "Threshold",
        min: 0.0,
        max: 1.0,
        default: 0.35,
        step: Some(0.05),
    },
    ParamField::Slider {
        key: "softness",
        label: "Softness",
        min: 1.0,
        max: 3.0,
        default: 2.0,
        step: Some(1.0),
    },
];

#[derive(Deserialize)]
struct GlowParams {
    radius: f32,
    intensity: f32,
    threshold: f32,
    #[serde(default = "default_softness")]
    softness: u8,
}

fn default_softness() -> u8 {
    1
}

pub struct Glow;

impl FilterAlgorithm for Glow {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("glow")
    }

    fn display_name(&self) -> &'static str {
        "Glow"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: GlowParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        apply_glow_into_at(
            &src,
            ctx.coord,
            params.radius,
            params.intensity,
            params.threshold,
            params.softness,
            tile,
        );
        Ok(())
    }

    fn gpu_eligibility(&self, params: &serde_json::Value) -> GpuEligibility {
        // PR-E interim (§6.1 B): GPU Glow clamps in-tile. When r×softness exceeds
        // HALO, CPU neighbor gather is required for preview=export — fall back.
        if !engine_gpu::spatial_gpu_enabled() {
            return GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur);
        }
        match serde_json::from_value::<GlowParams>(params.clone()) {
            Ok(p) if glow_spatial_radius_px(p.radius, p.softness) <= HALO as i32 => {
                GpuEligibility::Eligible
            }
            Ok(_) => GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur),
            Err(_) => GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter),
        }
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        2
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Stylize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn eligibility_within_halo_is_gpu() {
        let g = Glow;
        assert_eq!(
            g.gpu_eligibility(&json!({
                "radius": 2.0,
                "intensity": 1.0,
                "threshold": 0.0,
                "softness": 1
            })),
            GpuEligibility::Eligible
        );
    }

    #[test]
    fn eligibility_large_radius_is_cpu_adjust_blur() {
        let g = Glow;
        assert_eq!(
            g.gpu_eligibility(&json!({
                "radius": 8.0,
                "intensity": 1.0,
                "threshold": 0.35,
                "softness": 2
            })),
            GpuEligibility::Cpu(CpuCheckpointKind::AdjustBlur)
        );
    }
}

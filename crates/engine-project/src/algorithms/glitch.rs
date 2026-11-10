//! Glitch effects (`glitch`) — gather-model JPEG-style corruption.

use engine_registry::{
    AlgorithmId, CpuCheckpointKind, EffectCategory, FilterAlgorithm, FilterCtx, FilterError,
    GpuEligibility, ParamField,
};
use engine_tiles::PixelTile;

use crate::filters::context::FilterContext;
use crate::filters::glitch::{GlitchFilter, GlitchParams};

use super::scratch::scratch_src;

const SCHEMA: &[ParamField] = &[
    ParamField::Checkbox {
        key: "enabled",
        label: "Enabled",
        default: true,
    },
    ParamField::Slider {
        key: "seed",
        label: "Seed",
        min: 0.0,
        max: 99999.0,
        default: 42.0,
        step: Some(1.0),
    },
    ParamField::Dropdown {
        key: "block_size",
        label: "Block Size",
        options: &[("8", "8"), ("16", "16"), ("32", "32")],
        default: "8",
    },
    ParamField::Dropdown {
        key: "group_size",
        label: "Group Size",
        options: &[("2", "2"), ("4", "4"), ("8", "8")],
        default: "4",
    },
    ParamField::Checkbox {
        key: "block_shift_enabled",
        label: "Block Shift",
        default: false,
    },
    ParamField::Slider {
        key: "block_shift_amount",
        label: "Block Shift Amount",
        min: 0.0,
        max: 20.0,
        default: 0.0,
        step: Some(1.0),
    },
    ParamField::Checkbox {
        key: "block_scramble_enabled",
        label: "Block Scramble",
        default: false,
    },
    ParamField::Slider {
        key: "block_scramble_amount",
        label: "Block Scramble Amount",
        min: 0.0,
        max: 20.0,
        default: 0.0,
        step: Some(1.0),
    },
    ParamField::Checkbox {
        key: "channel_swap_enabled",
        label: "Channel Swap",
        default: false,
    },
    ParamField::Slider {
        key: "channel_swap_amount",
        label: "Channel Swap Amount",
        min: 0.0,
        max: 100.0,
        default: 0.0,
        step: Some(1.0),
    },
    ParamField::Dropdown {
        key: "scanline_segment",
        label: "Scanline Segment",
        options: &[("64", "64"), ("128", "128"), ("256", "256")],
        default: "64",
    },
    ParamField::Checkbox {
        key: "scanline_enabled",
        label: "Scanline",
        default: false,
    },
    ParamField::Slider {
        key: "scanline_amount",
        label: "Scanline Amount",
        min: 0.0,
        max: 100.0,
        default: 0.0,
        step: Some(1.0),
    },
    ParamField::Checkbox {
        key: "interlace_enabled",
        label: "Interlace",
        default: false,
    },
    ParamField::Slider {
        key: "interlace_amount",
        label: "Interlace Amount",
        min: 0.0,
        max: 100.0,
        default: 0.0,
        step: Some(1.0),
    },
];

pub struct Glitch;

impl FilterAlgorithm for Glitch {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("glitch")
    }

    fn display_name(&self) -> &'static str {
        "Glitch"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: GlitchParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let filter = GlitchFilter::new(params).map_err(FilterError::from)?;
        let src = scratch_src(tile);
        filter
            .apply_to_tile_into(
                &src,
                ctx.coord,
                ctx.document.width,
                ctx.document.height,
                tile,
            )
            .map_err(FilterError::from)
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Cpu(CpuCheckpointKind::UnsupportedFilter)
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        3
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Glitch
    }
}

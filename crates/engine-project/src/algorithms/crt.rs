//! CRT scanlines (`crt`).

use engine_registry::{
    AlgorithmId, EffectCategory, FilterAlgorithm, FilterCtx, FilterError, GpuEligibility,
    ParamField,
};
use engine_tiles::PixelTile;
use serde::Deserialize;

use crate::filters::context::FilterContext;
use crate::filters::crt::{
    apply_crt_into, CrtApplyParams, CrtMaskType, CrtOrientation,
};

use super::scratch::scratch_src;

const MASK_OPTIONS: &[(&str, &str)] = &[("aperture", "Aperture"), ("shadow", "Shadow")];
const ORIENT_OPTIONS: &[(&str, &str)] =
    &[("horizontal", "Horizontal"), ("vertical", "Vertical")];

const SCHEMA: &[ParamField] = &[
    ParamField::Slider {
        key: "period",
        label: "Period",
        min: 2.0,
        max: 8.0,
        default: 2.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "line_width",
        label: "Line Width",
        min: 1.0,
        max: 8.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "scale",
        label: "Scale",
        min: 1.0,
        max: 16.0,
        default: 1.0,
        step: Some(1.0),
    },
    ParamField::Slider {
        key: "strength",
        label: "Strength",
        min: 0.0,
        max: 1.0,
        default: 0.5,
        step: None,
    },
    ParamField::Slider {
        key: "soft_scanlines",
        label: "Soft Scanlines",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Dropdown {
        key: "orientation",
        label: "Orientation",
        options: ORIENT_OPTIONS,
        default: "horizontal",
    },
    ParamField::Dropdown {
        key: "mask_type",
        label: "Mask Type",
        options: MASK_OPTIONS,
        default: "aperture",
    },
    ParamField::Slider {
        key: "mask_strength",
        label: "Mask Strength",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "brightness_compensate",
        label: "Brightness Comp",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "curvature",
        label: "Curvature",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "vignette",
        label: "Vignette",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "phosphor_bleed",
        label: "Phosphor Bleed",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
    ParamField::Slider {
        key: "sharpness",
        label: "Sharpness",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        step: None,
    },
];

#[derive(Deserialize)]
struct CrtParams {
    period: u8,
    strength: f32,
    #[serde(default)]
    mask_strength: f32,
    #[serde(default)]
    soft_scanlines: f32,
    #[serde(default)]
    line_width: Option<u8>,
    /// Legacy fraction from the first CRT expansion.
    #[serde(default)]
    thickness: Option<f32>,
    #[serde(default = "default_scale")]
    scale: u8,
    #[serde(default = "default_orientation")]
    orientation: String,
    #[serde(default = "default_mask_type")]
    mask_type: String,
    #[serde(default)]
    brightness_compensate: f32,
    #[serde(default)]
    curvature: f32,
    #[serde(default)]
    vignette: f32,
    #[serde(default)]
    phosphor_bleed: f32,
    #[serde(default)]
    sharpness: f32,
}

fn default_scale() -> u8 {
    1
}
fn default_orientation() -> String {
    "horizontal".into()
}
fn default_mask_type() -> String {
    "aperture".into()
}

fn resolve_line_width(period: u8, line_width: Option<u8>, thickness: Option<f32>) -> u8 {
    let p = period.clamp(2, 8);
    if let Some(w) = line_width {
        return w.clamp(1, p);
    }
    if let Some(t) = thickness {
        return ((t.clamp(0.05, 0.95) * p as f32).round() as u8).clamp(1, p);
    }
    (p / 2).max(1)
}

pub struct Crt;

impl FilterAlgorithm for Crt {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::new("crt")
    }

    fn display_name(&self) -> &'static str {
        "CRT"
    }

    fn apply(
        &self,
        tile: &mut PixelTile,
        params: &serde_json::Value,
        ctx: &dyn FilterCtx,
    ) -> Result<(), FilterError> {
        let params: CrtParams = serde_json::from_value(params.clone())?;
        let ctx = FilterContext::from_ctx(ctx);
        let src = scratch_src(tile);
        let line_width = resolve_line_width(params.period, params.line_width, params.thickness);
        let apply = CrtApplyParams {
            period: params.period,
            strength: params.strength,
            mask_strength: params.mask_strength,
            soft_scanlines: params.soft_scanlines,
            line_width,
            scale: params.scale.clamp(1, 16),
            orientation: CrtOrientation::parse(&params.orientation),
            mask_type: CrtMaskType::parse(&params.mask_type),
            brightness_compensate: params.brightness_compensate,
            curvature: params.curvature,
            vignette: params.vignette,
            phosphor_bleed: params.phosphor_bleed,
            sharpness: params.sharpness,
            doc_w: ctx.document.width.max(1),
            doc_h: ctx.document.height.max(1),
        };
        apply_crt_into(&src, ctx.coord, apply, tile);
        Ok(())
    }

    fn gpu_eligibility(&self, _params: &serde_json::Value) -> GpuEligibility {
        GpuEligibility::Eligible
    }

    fn param_schema(&self) -> &'static [ParamField] {
        SCHEMA
    }

    fn schema_version(&self) -> u32 {
        1
    }

    fn category(&self) -> EffectCategory {
        EffectCategory::Stylize
    }

    fn migrate_params(&self, _old_version: u32, params: &mut serde_json::Value) {
        // Convert legacy fractional `thickness` → pixel `line_width`.
        let Some(obj) = params.as_object_mut() else {
            return;
        };
        if obj.contains_key("line_width") {
            obj.remove("thickness");
            return;
        }
        let period = obj
            .get("period")
            .and_then(|v| v.as_u64())
            .unwrap_or(2)
            .clamp(2, 8) as u8;
        if let Some(t) = obj.get("thickness").and_then(|v| v.as_f64()) {
            let w = ((t.clamp(0.05, 0.95) as f32 * period as f32).round() as u8).clamp(1, period);
            obj.insert("line_width".into(), serde_json::json!(w));
        } else {
            obj.insert("line_width".into(), serde_json::json!((period / 2).max(1)));
        }
        obj.remove("thickness");
    }
}

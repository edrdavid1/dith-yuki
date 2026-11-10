//! 1D ED style table (tickets 0.2a / 0.2b).
//!
//! Topology (`axis`, strip flags, deps) and style-param kind live here so
//! [`DitherModeV2`] match arms cannot drift from scheduling flags.

use crate::filter::{DitherModeV2, DitherParamsV2};
use crate::filters::ed1d::{
    contrast_gain_from_line_scale, ema_alpha_from_smoothness, sine_amp_from_ui, Axis, Ed1dCfg,
    SrcXf, WeightFn, CONTRAST_STD_HALF,
};

/// Cross-line dependency for strip scheduling (`Prev` = bleed chain, Phase 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ed1dDeps {
    None,
    /// Strip *k* waits on residual from strip *k−1* (uniform bleed).
    Prev,
}

/// Which style-param bundle the mode consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ed1dStyleKind {
    /// Neighbor-ΔY contrast (`modulated_diffuse_*`).
    Contrast,
    /// Local luminance-σ contrast (`contrast_aware_*`).
    ContrastStd,
    /// Neighbor contrast + EMA (`smooth_diffuse`).
    Smooth,
    /// Position sine weight (`wave_modulation_*`).
    Wave,
    /// Const weight + [`SrcXf::Glitch`].
    Glitch,
}

/// One row of [`ED1D_STYLES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ed1dStyleEntry {
    pub axis: Axis,
    pub requires_full_row: bool,
    pub requires_full_column: bool,
    pub deps: Ed1dDeps,
    pub kind: Ed1dStyleKind,
}

/// Typed style knobs (nested JSON form). Flat fields on [`DitherParamsV2`]
/// remain the **canonical serialize** shape for FE + old files.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StyleParams {
    Contrast {
        #[serde(default = "default_line_scale")]
        line_scale: u8,
    },
    /// Same knobs as Contrast; kind distinguishes σ-weight styles.
    ContrastStd {
        #[serde(default = "default_line_scale")]
        line_scale: u8,
    },
    Smooth {
        #[serde(default = "default_line_scale")]
        line_scale: u8,
        #[serde(default = "default_smoothness")]
        smoothness: u8,
    },
    Wave {
        #[serde(default = "default_frequency")]
        frequency: u8,
        #[serde(default = "default_amplitude")]
        amplitude: u8,
    },
    Glitch {
        #[serde(default = "default_glitch_intensity")]
        intensity: u8,
        #[serde(default = "default_glitch_seed")]
        seed: u32,
    },
}

fn default_line_scale() -> u8 {
    5
}

fn default_smoothness() -> u8 {
    5
}

fn default_frequency() -> u8 {
    8
}

fn default_amplitude() -> u8 {
    50
}

fn default_glitch_intensity() -> u8 {
    1
}

fn default_glitch_seed() -> u32 {
    42
}

const MODULATED_Y: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Horizontal,
    requires_full_row: true,
    requires_full_column: false,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::Contrast,
};

const MODULATED_X: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Vertical,
    requires_full_row: false,
    requires_full_column: true,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::Contrast,
};

const CONTRAST_AWARE_Y: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Horizontal,
    requires_full_row: true,
    requires_full_column: false,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::ContrastStd,
};

const CONTRAST_AWARE_X: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Vertical,
    requires_full_row: false,
    requires_full_column: true,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::ContrastStd,
};

const SMOOTH_DIFFUSE: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Horizontal,
    requires_full_row: true,
    requires_full_column: false,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::Smooth,
};

const WAVE_Y: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Horizontal,
    requires_full_row: true,
    requires_full_column: false,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::Wave,
};

const WAVE_X: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Vertical,
    requires_full_row: false,
    requires_full_column: true,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::Wave,
};

const GLITCH_DITHER: Ed1dStyleEntry = Ed1dStyleEntry {
    axis: Axis::Horizontal,
    requires_full_row: true,
    requires_full_column: false,
    deps: Ed1dDeps::None,
    kind: Ed1dStyleKind::Glitch,
};

/// Lookup table for shipped 1D ED styles (algorithm id → entry).
pub static ED1D_STYLES: &[(&str, Ed1dStyleEntry)] = &[
    ("modulated_diffuse_y", MODULATED_Y),
    ("modulated_diffuse_x", MODULATED_X),
    ("contrast_aware_y", CONTRAST_AWARE_Y),
    ("contrast_aware_x", CONTRAST_AWARE_X),
    ("smooth_diffuse", SMOOTH_DIFFUSE),
    ("wave_modulation_y", WAVE_Y),
    ("wave_modulation_x", WAVE_X),
    ("glitch_dither", GLITCH_DITHER),
];

/// Table entry for an ed1d mode, if any.
#[inline]
pub fn ed1d_style(mode: &DitherModeV2) -> Option<&'static Ed1dStyleEntry> {
    match mode {
        DitherModeV2::ModulatedDiffuseY => Some(&MODULATED_Y),
        DitherModeV2::ModulatedDiffuseX => Some(&MODULATED_X),
        DitherModeV2::ContrastAwareY => Some(&CONTRAST_AWARE_Y),
        DitherModeV2::ContrastAwareX => Some(&CONTRAST_AWARE_X),
        DitherModeV2::SmoothDiffuse => Some(&SMOOTH_DIFFUSE),
        DitherModeV2::WaveModulationY => Some(&WAVE_Y),
        DitherModeV2::WaveModulationX => Some(&WAVE_X),
        DitherModeV2::GlitchDither => Some(&GLITCH_DITHER),
        _ => None,
    }
}

/// Carry axis from the table (`None` if not ed1d).
#[inline]
pub fn ed1d_axis_for_mode(mode: &DitherModeV2) -> Option<Axis> {
    ed1d_style(mode).map(|e| e.axis)
}

/// Build [`Ed1dCfg`] from params + table.
pub fn ed1d_cfg_from_params(params: &DitherParamsV2) -> Ed1dCfg {
    let entry = ed1d_style(&params.mode).unwrap_or(&MODULATED_Y);
    let gain = contrast_gain_from_line_scale(params.line_scale);
    match entry.kind {
        Ed1dStyleKind::Glitch => Ed1dCfg {
            axis: entry.axis,
            carry: 1.0,
            weight: WeightFn::Const,
            src_transform: Some(SrcXf::Glitch {
                intensity: params.glitch_intensity,
                seed: params.glitch_seed,
            }),
            ..Ed1dCfg::default()
        },
        Ed1dStyleKind::Contrast => Ed1dCfg {
            axis: entry.axis,
            carry: 1.0,
            weight: WeightFn::Contrast { gain },
            ..Ed1dCfg::default()
        },
        Ed1dStyleKind::ContrastStd => Ed1dCfg {
            axis: entry.axis,
            carry: 1.0,
            weight: WeightFn::ContrastStd {
                gain,
                half: CONTRAST_STD_HALF,
            },
            ..Ed1dCfg::default()
        },
        Ed1dStyleKind::Smooth => Ed1dCfg {
            axis: entry.axis,
            carry: 1.0,
            // Same neighbor contrast as modulated; EMA from smoothness.
            // `smoothness = 0` ⇒ α = 1 ⇒ bit-identical to modulated_diffuse_y.
            weight: WeightFn::Contrast { gain },
            ema: Some(ema_alpha_from_smoothness(params.smoothness)),
            ..Ed1dCfg::default()
        },
        Ed1dStyleKind::Wave => Ed1dCfg {
            axis: entry.axis,
            carry: 1.0,
            weight: WeightFn::Sine {
                freq: params.frequency.clamp(1, 30) as f32,
                amp: sine_amp_from_ui(params.amplitude),
            },
            ..Ed1dCfg::default()
        },
    }
}

/// StyleParams view derived from mode + flat fields.
pub fn style_params_from_dither(params: &DitherParamsV2) -> Option<StyleParams> {
    let entry = ed1d_style(&params.mode)?;
    Some(match entry.kind {
        Ed1dStyleKind::Contrast => StyleParams::Contrast {
            line_scale: params.line_scale,
        },
        Ed1dStyleKind::ContrastStd => StyleParams::ContrastStd {
            line_scale: params.line_scale,
        },
        Ed1dStyleKind::Smooth => StyleParams::Smooth {
            line_scale: params.line_scale,
            smoothness: params.smoothness,
        },
        Ed1dStyleKind::Wave => StyleParams::Wave {
            frequency: params.frequency,
            amplitude: params.amplitude,
        },
        Ed1dStyleKind::Glitch => StyleParams::Glitch {
            intensity: params.glitch_intensity,
            seed: params.glitch_seed,
        },
    })
}

/// Fold nested [`StyleParams`] into flat dither fields.
pub fn apply_style_params(params: &mut DitherParamsV2, style: &StyleParams) {
    match style {
        StyleParams::Contrast { line_scale } | StyleParams::ContrastStd { line_scale } => {
            params.line_scale = *line_scale;
        }
        StyleParams::Smooth {
            line_scale,
            smoothness,
        } => {
            params.line_scale = *line_scale;
            params.smoothness = *smoothness;
        }
        StyleParams::Wave {
            frequency,
            amplitude,
        } => {
            params.frequency = *frequency;
            params.amplitude = *amplitude;
        }
        StyleParams::Glitch { intensity, seed } => {
            params.glitch_intensity = *intensity;
            params.glitch_seed = *seed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::DitherParamsV2;
    use serde_json::json;

    #[test]
    fn topology_matrix_matches_pre_0_2a() {
        let y = ed1d_style(&DitherModeV2::ModulatedDiffuseY).unwrap();
        assert!(y.requires_full_row && !y.requires_full_column);
        assert_eq!(y.axis, Axis::Horizontal);
        assert_eq!(y.deps, Ed1dDeps::None);
        assert_eq!(y.kind, Ed1dStyleKind::Contrast);

        let x = ed1d_style(&DitherModeV2::ModulatedDiffuseX).unwrap();
        assert!(!x.requires_full_row && x.requires_full_column);
        assert_eq!(x.axis, Axis::Vertical);

        let g = ed1d_style(&DitherModeV2::GlitchDither).unwrap();
        assert!(g.requires_full_row && !g.requires_full_column);
        assert_eq!(g.kind, Ed1dStyleKind::Glitch);

        let cy = ed1d_style(&DitherModeV2::ContrastAwareY).unwrap();
        assert_eq!(cy.kind, Ed1dStyleKind::ContrastStd);
        assert!(cy.requires_full_row);

        let cx = ed1d_style(&DitherModeV2::ContrastAwareX).unwrap();
        assert_eq!(cx.kind, Ed1dStyleKind::ContrastStd);
        assert!(cx.requires_full_column);

        let s = ed1d_style(&DitherModeV2::SmoothDiffuse).unwrap();
        assert_eq!(s.kind, Ed1dStyleKind::Smooth);
        assert!(s.requires_full_row && !s.requires_full_column);

        assert!(ed1d_style(&DitherModeV2::FloydSteinberg).is_none());
        assert_eq!(ED1D_STYLES.len(), 8);
    }

    #[test]
    fn table_ids_match_mode_algorithm_ids_and_axis() {
        for &(id, entry) in ED1D_STYLES {
            let mode = match id {
                "modulated_diffuse_y" => DitherModeV2::ModulatedDiffuseY,
                "modulated_diffuse_x" => DitherModeV2::ModulatedDiffuseX,
                "contrast_aware_y" => DitherModeV2::ContrastAwareY,
                "contrast_aware_x" => DitherModeV2::ContrastAwareX,
                "smooth_diffuse" => DitherModeV2::SmoothDiffuse,
                "wave_modulation_y" => DitherModeV2::WaveModulationY,
                "wave_modulation_x" => DitherModeV2::WaveModulationX,
                "glitch_dither" => DitherModeV2::GlitchDither,
                other => panic!("unexpected ed1d style id {other}"),
            };
            assert_eq!(mode.algorithm_id(), Some(id));
            assert_eq!(ed1d_axis_for_mode(&mode), Some(entry.axis));
            assert_eq!(mode.ed1d_axis(), Some(entry.axis));
            assert!(mode.is_ed1d());
            assert_eq!(ed1d_style(&mode).unwrap(), &entry);
        }
    }

    #[test]
    fn flat_json_round_trip_preserves_knobs() {
        let p = DitherParamsV2 {
            mode: DitherModeV2::ModulatedDiffuseY,
            line_scale: 12,
            ..DitherParamsV2::default()
        };
        let v = serde_json::to_value(&p).unwrap();
        assert!(v.get("style").is_none(), "canonical serialize stays flat");
        assert_eq!(v["line_scale"], 12);
        let back: DitherParamsV2 = serde_json::from_value(v).unwrap();
        assert_eq!(back.line_scale, 12);
        assert!(matches!(back.mode, DitherModeV2::ModulatedDiffuseY));
    }

    #[test]
    fn nested_style_json_folds_into_flat_fields() {
        let v = json!({
            "mode": "modulated_diffuse_y",
            "levels": 4,
            "style": { "kind": "contrast", "line_scale": 17 }
        });
        let mut p: DitherParamsV2 = serde_json::from_value(v).unwrap();
        assert!(p.style.is_some());
        p.fold_style();
        assert_eq!(p.line_scale, 17);
        assert!(p.style.is_none());
    }

    #[test]
    fn nested_smooth_style_folds() {
        let v = json!({
            "mode": "smooth_diffuse",
            "levels": 4,
            "style": { "kind": "smooth", "line_scale": 8, "smoothness": 12 }
        });
        let mut p: DitherParamsV2 = serde_json::from_value(v).unwrap();
        p.fold_style();
        assert_eq!(p.line_scale, 8);
        assert_eq!(p.smoothness, 12);
    }

    #[test]
    fn nested_glitch_style_folds() {
        let v = json!({
            "mode": "glitch_dither",
            "levels": 4,
            "style": { "kind": "glitch", "intensity": 9, "seed": 99 }
        });
        let mut p: DitherParamsV2 = serde_json::from_value(v).unwrap();
        p.fold_style();
        assert_eq!(p.glitch_intensity, 9);
        assert_eq!(p.glitch_seed, 99);
    }

    #[test]
    fn ed1d_cfg_kinds() {
        let g = DitherParamsV2 {
            mode: DitherModeV2::GlitchDither,
            glitch_intensity: 5,
            glitch_seed: 42,
            ..DitherParamsV2::default()
        };
        let cfg = ed1d_cfg_from_params(&g);
        assert_eq!(cfg.weight, WeightFn::Const);
        assert!(cfg.ema.is_none());

        let c = DitherParamsV2 {
            mode: DitherModeV2::ModulatedDiffuseX,
            line_scale: 10,
            ..DitherParamsV2::default()
        };
        let cfg = ed1d_cfg_from_params(&c);
        assert_eq!(cfg.axis, Axis::Vertical);
        assert_eq!(cfg.weight, WeightFn::Contrast { gain: 0.5 });

        let a = DitherParamsV2 {
            mode: DitherModeV2::ContrastAwareY,
            line_scale: 10,
            ..DitherParamsV2::default()
        };
        let cfg = ed1d_cfg_from_params(&a);
        assert_eq!(
            cfg.weight,
            WeightFn::ContrastStd {
                gain: 0.5,
                half: CONTRAST_STD_HALF
            }
        );

        let s0 = DitherParamsV2 {
            mode: DitherModeV2::SmoothDiffuse,
            line_scale: 10,
            smoothness: 0,
            ..DitherParamsV2::default()
        };
        let cfg = ed1d_cfg_from_params(&s0);
        assert_eq!(cfg.weight, WeightFn::Contrast { gain: 0.5 });
        assert_eq!(cfg.ema, Some(1.0));
    }

    #[test]
    fn smoothness_zero_matches_modulated_pixels() {
        use crate::filters::ed1d::{ed1d_image, UniformLevelsQuantizer};

        let w = 96usize;
        let h = 48usize;
        let mut src = vec![0.0f32; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) * 4;
                let t = x as f32 / w as f32;
                src[i] = t;
                src[i + 1] = 1.0 - t;
                src[i + 2] = (t * 0.5 + 0.25).clamp(0.0, 1.0);
                src[i + 3] = 1.0;
            }
        }
        let mod_p = DitherParamsV2 {
            mode: DitherModeV2::ModulatedDiffuseY,
            line_scale: 8,
            ..DitherParamsV2::default()
        };
        let smooth_p = DitherParamsV2 {
            mode: DitherModeV2::SmoothDiffuse,
            line_scale: 8,
            smoothness: 0,
            ..DitherParamsV2::default()
        };
        let cfg_m = ed1d_cfg_from_params(&mod_p);
        let cfg_s = ed1d_cfg_from_params(&smooth_p);
        let mut q1 = UniformLevelsQuantizer { levels: 4.0 };
        let mut q2 = UniformLevelsQuantizer { levels: 4.0 };
        assert_eq!(
            ed1d_image(&src, w, h, &cfg_m, &mut q1),
            ed1d_image(&src, w, h, &cfg_s, &mut q2),
            "smoothness=0 must match modulated_diffuse_y at same line_scale"
        );
    }
}

//! P3 pass fusion: consecutive opaque Normal pointwise filters in one pixel loop.
//!
//! Fusible when `opacity >= 1`, `blend == Normal`, and the filter is Curves,
//! Levels, or Adjust with blur/sharpness/noise all exactly 0. Result must be
//! bitwise equal to sequential `_into` applies. Kill-switch: `DITHER_POST_FUSION=0`.

use std::sync::atomic::{AtomicBool, Ordering};

use engine_tiles::{PixelTile, HALO, TILE_SIZE};

use crate::error::EngineError;
use crate::filter::{FilterInstance, FilterParams};
use crate::filters::adjust::{adjust_sat_m, apply_adjust_point_rgba};
use crate::filters::curves::{CurveChannel, CurvesFilter};
use crate::filters::exposure::apply_exposure_point_rgba;
use crate::filters::highlights_shadows::apply_highlights_shadows_point_rgba;
use crate::filters::levels::LevelsFilter;
use crate::filters::neutral::filter_is_neutral_for_apply;
use crate::filters::temperature_tint::apply_temperature_tint_point_rgba;
use crate::filters::vibrance::apply_vibrance_point_rgba;
use crate::types::BlendMode;

const TILE_FULL_SIZE: u32 = TILE_SIZE + 2 * HALO;

static FUSION_ENABLED: AtomicBool = AtomicBool::new(true);
static FUSION_INIT: AtomicBool = AtomicBool::new(false);

fn fusion_env_or_default() -> bool {
    match std::env::var("DITHER_POST_FUSION") {
        Ok(v) => match v.trim() {
            "0" | "false" | "FALSE" | "no" | "NO" | "off" | "OFF" => false,
            "1" | "true" | "TRUE" | "yes" | "YES" | "on" | "ON" => true,
            _ => true,
        },
        Err(_) => true,
    }
}

fn ensure_fusion_init() {
    if FUSION_INIT.swap(true, Ordering::Relaxed) {
        return;
    }
    let on = fusion_env_or_default();
    FUSION_ENABLED.store(on, Ordering::Relaxed);
    log::info!(
        target: "post_barrier",
        "DITHER_POST_FUSION={}",
        if on { "on" } else { "off" }
    );
}

/// Whether pass fusion is active (default on; `DITHER_POST_FUSION=0` disables).
pub fn post_fusion_enabled() -> bool {
    ensure_fusion_init();
    FUSION_ENABLED.load(Ordering::Relaxed)
}

/// Test / harness override.
pub fn set_post_fusion_enabled_for_test(on: bool) {
    FUSION_INIT.store(true, Ordering::Relaxed);
    FUSION_ENABLED.store(on, Ordering::Relaxed);
}

/// Opaque Normal pointwise filter eligible for a fused run.
pub fn is_fusible_pointwise(filter: &FilterInstance) -> bool {
    if filter_is_neutral_for_apply(filter) {
        return false;
    }
    if filter.effective_opacity() < 1.0 || filter.blend_mode != BlendMode::Normal {
        return false;
    }
    if filter.requires_full_row || filter.requires_full_column {
        return false;
    }
    match &filter.params {
        FilterParams::Adjust {
            blur,
            sharpness,
            noise,
            ..
        } => *blur == 0.0 && *sharpness == 0.0 && *noise == 0.0,
        FilterParams::Curves { .. }
        | FilterParams::Levels { .. }
        | FilterParams::Exposure { .. }
        | FilterParams::HighlightsShadows { .. }
        | FilterParams::TemperatureTint { .. }
        | FilterParams::Vibrance { .. } => true,
        _ => false,
    }
}

/// Length of a consecutive fusible run starting at `filters[0]`, or 0 if none / too short.
pub fn fusible_run_len(filters: &[&FilterInstance]) -> usize {
    if !post_fusion_enabled() || filters.is_empty() || !is_fusible_pointwise(filters[0]) {
        return 0;
    }
    let mut n = 1;
    while n < filters.len() && is_fusible_pointwise(filters[n]) {
        n += 1;
    }
    if n >= 2 {
        n
    } else {
        0
    }
}

enum FusibleOp {
    Adjust {
        contrast_m: f32,
        brightness: f32,
        sat_m: f32,
    },
    Curves(CurvesFilter),
    Levels(LevelsFilter),
    Exposure {
        gain: f32,
    },
    HighlightsShadows {
        shadows: f32,
        highlights: f32,
    },
    TemperatureTint {
        temperature: f32,
        tint: f32,
    },
    Vibrance {
        amount: f32,
    },
}

fn build_op(filter: &FilterInstance) -> Result<FusibleOp, EngineError> {
    match &filter.params {
        FilterParams::Adjust {
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
        } => {
            debug_assert_eq!(*blur, 0.0);
            debug_assert_eq!(*sharpness, 0.0);
            debug_assert_eq!(*noise, 0.0);
            Ok(FusibleOp::Adjust {
                contrast_m: 1.0 + *contrast,
                brightness: *brightness,
                sat_m: adjust_sat_m(*saturation),
            })
        }
        FilterParams::Curves { curve, channel } => {
            let mut curves = CurvesFilter::new(*channel);
            curves.curve = curve.clone();
            curves.rebuild_lut();
            Ok(FusibleOp::Curves(curves))
        }
        FilterParams::Levels {
            input_black,
            input_white,
            gamma,
            output_black,
            output_white,
            channel_r,
            channel_g,
            channel_b,
        } => {
            let mut levels = LevelsFilter::new();
            levels.input_black = *input_black;
            levels.input_white = *input_white;
            levels.gamma = *gamma;
            levels.output_black = *output_black;
            levels.output_white = *output_white;
            levels.channel_r = *channel_r;
            levels.channel_g = *channel_g;
            levels.channel_b = *channel_b;
            levels.rebuild_lut();
            Ok(FusibleOp::Levels(levels))
        }
        FilterParams::Exposure { stops } => Ok(FusibleOp::Exposure {
            gain: 2f32.powf(stops.clamp(-5.0, 5.0)),
        }),
        FilterParams::HighlightsShadows {
            shadows,
            highlights,
        } => Ok(FusibleOp::HighlightsShadows {
            shadows: shadows.clamp(-1.0, 1.0),
            highlights: highlights.clamp(-1.0, 1.0),
        }),
        FilterParams::TemperatureTint { temperature, tint } => Ok(FusibleOp::TemperatureTint {
            temperature: temperature.clamp(-1.0, 1.0),
            tint: tint.clamp(-1.0, 1.0),
        }),
        FilterParams::Vibrance { amount } => Ok(FusibleOp::Vibrance {
            amount: amount.clamp(-1.0, 1.0),
        }),
        _ => Err(EngineError::invalid_filter_params(
            "filter is not fusible pointwise",
        )),
    }
}

#[inline]
fn apply_curves_rgba(
    curves: &CurvesFilter,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> (f32, f32, f32, f32) {
    match curves.channel {
        CurveChannel::Red => (curves.lut_lookup(r), g, b, a),
        CurveChannel::Green => (r, curves.lut_lookup(g), b, a),
        CurveChannel::Blue => (r, g, curves.lut_lookup(b), a),
        CurveChannel::All => (
            curves.lut_lookup(r),
            curves.lut_lookup(g),
            curves.lut_lookup(b),
            a,
        ),
        // Matches `CurvesFilter::apply_to_tile_into` (luminance maps green).
        CurveChannel::Luminance => (r, curves.lut_lookup(g), b, a),
    }
}

#[inline]
fn apply_levels_rgba(
    levels: &LevelsFilter,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
) -> (f32, f32, f32, f32) {
    let r = if levels.channel_r {
        levels.lut_lookup(r)
    } else {
        0.0
    };
    let g = if levels.channel_g {
        levels.lut_lookup(g)
    } else {
        0.0
    };
    let b = if levels.channel_b {
        levels.lut_lookup(b)
    } else {
        0.0
    };
    (r, g, b, a)
}

#[inline]
fn apply_op_rgba(op: &FusibleOp, r: f32, g: f32, b: f32, a: f32) -> (f32, f32, f32, f32) {
    match op {
        FusibleOp::Adjust {
            contrast_m,
            brightness,
            sat_m,
        } => apply_adjust_point_rgba(r, g, b, a, *contrast_m, *brightness, *sat_m),
        FusibleOp::Curves(c) => apply_curves_rgba(c, r, g, b, a),
        FusibleOp::Levels(l) => apply_levels_rgba(l, r, g, b, a),
        FusibleOp::Exposure { gain } => apply_exposure_point_rgba(r, g, b, a, *gain),
        FusibleOp::HighlightsShadows {
            shadows,
            highlights,
        } => apply_highlights_shadows_point_rgba(r, g, b, a, *shadows, *highlights),
        FusibleOp::TemperatureTint { temperature, tint } => {
            apply_temperature_tint_point_rgba(r, g, b, a, *temperature, *tint)
        }
        FusibleOp::Vibrance { amount } => apply_vibrance_point_rgba(r, g, b, a, *amount),
    }
}

/// Apply a fusible run into `dst` in one 260² pass (chain of closures).
pub fn apply_fused_pointwise_into(
    src: &PixelTile,
    dst: &mut PixelTile,
    filters: &[&FilterInstance],
) -> Result<(), EngineError> {
    debug_assert!(filters.len() >= 2);
    debug_assert_ne!(src.data.as_ptr(), dst.data.as_ptr());

    let mut ops = Vec::with_capacity(filters.len());
    for f in filters {
        ops.push(build_op(f)?);
    }

    for y in 0..TILE_FULL_SIZE {
        for x in 0..TILE_FULL_SIZE {
            let mut r = src.at(x, y, 0);
            let mut g = src.at(x, y, 1);
            let mut b = src.at(x, y, 2);
            let mut a = src.at(x, y, 3);
            for op in &ops {
                let out = apply_op_rgba(op, r, g, b, a);
                r = out.0;
                g = out.1;
                b = out.2;
                a = out.3;
            }
            dst.set(x, y, 0, r);
            dst.set(x, y, 1, g);
            dst.set(x, y, 2, b);
            dst.set(x, y, 3, a);
        }
    }

    crate::post_barrier_metrics::record_post_fusion_run(filters.len() as u64);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{FilterKind, FilterParams};
    use crate::filters::curves::CurveChannel;
    use crate::types::BlendMode;

    fn adjust_pointwise(contrast: f32, brightness: f32, saturation: f32) -> FilterInstance {
        FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast,
                brightness,
                saturation,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        )
    }

    fn curves_s_curve() -> FilterInstance {
        FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (0.5, 0.6), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        )
    }

    fn levels_gamma(gamma: f32) -> FilterInstance {
        FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 0.0,
                input_white: 1.0,
                gamma,
                output_black: 0.0,
                output_white: 1.0,
                channel_r: true,
                channel_g: true,
                channel_b: true,
            },
        )
    }

    fn gradient_tile() -> PixelTile {
        let mut tile = PixelTile::new();
        for y in 0..TILE_FULL_SIZE {
            for x in 0..TILE_FULL_SIZE {
                let u = x as f32 / (TILE_FULL_SIZE - 1) as f32;
                let v = y as f32 / (TILE_FULL_SIZE - 1) as f32;
                tile.set(x, y, 0, u);
                tile.set(x, y, 1, v);
                tile.set(x, y, 2, (u + v) * 0.5);
                tile.set(x, y, 3, if (x + y) % 17 == 0 { 0.0 } else { 1.0 });
            }
        }
        tile
    }

    #[test]
    fn fusible_predicates() {
        assert!(is_fusible_pointwise(&adjust_pointwise(0.2, -0.1, 0.3)));
        assert!(is_fusible_pointwise(&curves_s_curve()));
        assert!(is_fusible_pointwise(&levels_gamma(1.4)));

        let mut with_blur = adjust_pointwise(0.2, 0.0, 0.0);
        if let FilterParams::Adjust { blur, .. } = &mut with_blur.params {
            *blur = 0.5;
        }
        assert!(!is_fusible_pointwise(&with_blur));

        let mut faded = curves_s_curve();
        faded.opacity = 0.5;
        assert!(!is_fusible_pointwise(&faded));

        let mut multiply = levels_gamma(1.2);
        multiply.blend_mode = BlendMode::Multiply;
        assert!(!is_fusible_pointwise(&multiply));
    }

    #[test]
    fn fused_matches_sequential_curves_levels_adjust() {
        set_post_fusion_enabled_for_test(true);
        let a = adjust_pointwise(0.15, -0.05, 0.25);
        let c = curves_s_curve();
        let l = levels_gamma(1.3);
        let stack = [&a, &c, &l];
        assert_eq!(fusible_run_len(&stack), 3);

        let src = gradient_tile();
        let mut fused = PixelTile::new();
        apply_fused_pointwise_into(&src, &mut fused, &stack).unwrap();

        let mut seq = PixelTile::new();
        seq.copy_from(&src);
        for f in &stack {
            let mut next = PixelTile::new();
            match &f.params {
                FilterParams::Adjust {
                    contrast,
                    brightness,
                    saturation,
                    ..
                } => {
                    crate::filters::adjust::apply_adjust_into(
                        &seq,
                        engine_tiles::TileCoord {
                            level: 0,
                            x: 0,
                            y: 0,
                        },
                        *contrast,
                        *brightness,
                        *saturation,
                        0.0,
                        0.0,
                        0.0,
                        &mut next,
                    );
                }
                FilterParams::Curves { curve, channel } => {
                    let mut curves = CurvesFilter::new(*channel);
                    curves.curve = curve.clone();
                    curves.rebuild_lut();
                    curves.apply_to_tile_into(&seq, &mut next).unwrap();
                }
                FilterParams::Levels {
                    input_black,
                    input_white,
                    gamma,
                    output_black,
                    output_white,
                    channel_r,
                    channel_g,
                    channel_b,
                } => {
                    let mut levels = LevelsFilter::new();
                    levels.input_black = *input_black;
                    levels.input_white = *input_white;
                    levels.gamma = *gamma;
                    levels.output_black = *output_black;
                    levels.output_white = *output_white;
                    levels.channel_r = *channel_r;
                    levels.channel_g = *channel_g;
                    levels.channel_b = *channel_b;
                    levels.rebuild_lut();
                    levels.apply_to_tile_into(&seq, &mut next).unwrap();
                }
                _ => unreachable!(),
            }
            seq = next;
        }

        assert_eq!(
            fused.data, seq.data,
            "fused vs sequential must be bitwise equal"
        );
    }

    #[test]
    fn single_filter_is_not_a_run() {
        set_post_fusion_enabled_for_test(true);
        let c = curves_s_curve();
        assert_eq!(fusible_run_len(&[&c]), 0);
    }
}

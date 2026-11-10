//! Skip apply / hash for filters that are exact identities at current params.

use crate::filter::{FilterInstance, FilterParams};
use crate::filters::curves::CurveChannel;
use crate::filters::glitch::glitch_params_neutral;
use crate::types::BlendMode;

/// Whether this filter can be omitted from a tile apply chain with no pixel change.
///
/// Rules use exact param equality (no epsilon).
/// - Disabled → always skip.
/// - `effective_opacity() == 0` → always skip (blend never runs).
/// - Otherwise params must be identity **and** blend must be [`BlendMode::Normal`]
///   (non-Normal blends of an identity op are not identity — e.g. Multiply → src·src).
pub fn filter_is_neutral_for_apply(filter: &FilterInstance) -> bool {
    if !filter.enabled {
        return true;
    }
    if filter.effective_opacity() == 0.0 {
        return true;
    }
    // Identity params under a non-Normal blend are not passthrough.
    if filter.blend_mode != BlendMode::Normal {
        return false;
    }
    params_are_identity(filter)
}

fn params_are_identity(filter: &FilterInstance) -> bool {
    match &filter.params {
        FilterParams::Adjust {
            contrast,
            brightness,
            saturation,
            blur,
            sharpness,
            noise,
        } => {
            *contrast == 0.0
                && *brightness == 0.0
                && *saturation == 0.0
                && *blur == 0.0
                && *sharpness == 0.0
                && *noise == 0.0
        }
        FilterParams::Glitch(p) => glitch_params_neutral(p),
        FilterParams::Curves { curve, channel } => {
            curve.len() == 2
                && curve[0].0 == 0.0
                && curve[0].1 == 0.0
                && curve[1].0 == 1.0
                && curve[1].1 == 1.0
                && matches!(
                    channel,
                    CurveChannel::All
                        | CurveChannel::Red
                        | CurveChannel::Green
                        | CurveChannel::Blue
                )
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
            *input_black == 0.0
                && *input_white == 1.0
                && *gamma == 1.0
                && *output_black == 0.0
                && *output_white == 1.0
                && *channel_r
                && *channel_g
                && *channel_b
        }
        FilterParams::Exposure { stops } => *stops == 0.0,
        FilterParams::HighlightsShadows {
            shadows,
            highlights,
        } => *shadows == 0.0 && *highlights == 0.0,
        _ => false,
    }
}

/// Same predicate as apply skip; used when omitting neutral bodies from prefix/stack hash.
/// Non-Normal zero-param filters stay in the hash (they still affect pixels).
pub fn filter_is_neutral_for_hash(filter: &FilterInstance) -> bool {
    filter.enabled && filter_is_neutral_for_apply(filter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::{FilterKind, FilterParams};
    use crate::filters::glitch::GlitchParams;

    #[test]
    fn adjust_all_zero_normal_is_neutral() {
        let f = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        assert!(filter_is_neutral_for_apply(&f));
        assert!(filter_is_neutral_for_hash(&f));
    }

    #[test]
    fn adjust_all_zero_multiply_is_not_neutral() {
        let mut f = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        f.blend_mode = BlendMode::Multiply;
        assert!(!filter_is_neutral_for_apply(&f));
        assert!(!filter_is_neutral_for_hash(&f));
    }

    #[test]
    fn opacity_zero_skips_regardless_of_blend() {
        let mut f = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
            },
        );
        f.blend_mode = BlendMode::Multiply;
        f.opacity = 0.0;
        assert!(filter_is_neutral_for_apply(&f));
    }

    #[test]
    fn glitch_defaults_inactive_is_neutral() {
        let f = FilterInstance::new(
            FilterKind::Glitch,
            FilterParams::Glitch(GlitchParams::default()),
        );
        assert!(filter_is_neutral_for_apply(&f));
    }

    #[test]
    fn curves_and_levels_params_neutral() {
        let curves = FilterInstance::new(
            FilterKind::Curves,
            FilterParams::Curves {
                curve: vec![(0.0, 0.0), (1.0, 1.0)],
                channel: CurveChannel::All,
            },
        );
        assert!(filter_is_neutral_for_apply(&curves));
        let levels = FilterInstance::new(
            FilterKind::Levels,
            FilterParams::Levels {
                input_black: 0.0,
                input_white: 1.0,
                gamma: 1.0,
                output_black: 0.0,
                output_white: 1.0,
                channel_r: true,
                channel_g: true,
                channel_b: true,
            },
        );
        assert!(filter_is_neutral_for_apply(&levels));
    }
}

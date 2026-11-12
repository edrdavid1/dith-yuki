//! Split legacy Adjust spatial knobs into BlurNoise / Detail (PR-G).

use crate::filter::{FilterInstance, FilterKind, FilterParams};

/// Expand Adjust instances that still carry blur/sharpness/noise/clarity into
/// sibling BlurNoise / Detail filters and zero those fields on Adjust.
///
/// Panel order is top-first (`filters[0]` = last applied). After expansion,
/// apply order is Adjust → BlurNoise → Detail (blur/noise then clarity/unsharp).
/// That is close to the historical in-Adjust order (blur → sharp → clarity →
/// noise); exact residual order is not bit-identical when both sharpness and
/// clarity are non-zero.
pub fn expand_adjust_spatial_filters(filters: &[FilterInstance]) -> Vec<FilterInstance> {
    let mut out = Vec::with_capacity(filters.len());
    for f in filters {
        match &f.params {
            FilterParams::Adjust {
                contrast,
                brightness,
                saturation,
                blur,
                sharpness,
                noise,
                exposure,
                shadows,
                highlights,
                temperature,
                tint,
                vibrance,
                hue,
                clarity,
            } if *blur > 1e-6
                || *sharpness > 1e-6
                || *noise > 1e-6
                || clarity.abs() > 1e-6 =>
            {
                let need_blur_noise = *blur > 1e-6 || *noise > 1e-6;
                let need_detail = *sharpness > 1e-6 || clarity.abs() > 1e-6;

                // Toward top of panel (earlier in vec = applied later): Detail, BlurNoise.
                if need_detail {
                    let mut d = FilterInstance::new(
                        FilterKind::Detail,
                        FilterParams::Detail {
                            clarity: *clarity,
                            sharpness: *sharpness,
                        },
                    );
                    d.enabled = f.enabled;
                    d.opacity = f.opacity;
                    d.blend_mode = f.blend_mode;
                    d.algorithm_id = Some("detail".into());
                    d.schema_version = Some(1);
                    out.push(d);
                }
                if need_blur_noise {
                    let mut bn = FilterInstance::new(
                        FilterKind::BlurNoise,
                        FilterParams::BlurNoise {
                            blur: *blur,
                            noise: *noise,
                        },
                    );
                    bn.enabled = f.enabled;
                    bn.opacity = f.opacity;
                    bn.blend_mode = f.blend_mode;
                    bn.algorithm_id = Some("blur_noise".into());
                    bn.schema_version = Some(1);
                    out.push(bn);
                }

                let mut adj = f.clone();
                adj.params = FilterParams::Adjust {
                    contrast: *contrast,
                    brightness: *brightness,
                    saturation: *saturation,
                    blur: 0.0,
                    sharpness: 0.0,
                    noise: 0.0,
                    exposure: *exposure,
                    shadows: *shadows,
                    highlights: *highlights,
                    temperature: *temperature,
                    tint: *tint,
                    vibrance: *vibrance,
                    hue: *hue,
                    clarity: 0.0,
                };
                adj.schema_version = Some(3);
                out.push(adj);
            }
            _ => out.push(f.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_adjust_unchanged() {
        let f = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.1,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.0,
                sharpness: 0.0,
                noise: 0.0,
                exposure: 0.0,
                shadows: 0.0,
                highlights: 0.0,
                temperature: 0.0,
                tint: 0.0,
                vibrance: 0.0,
                hue: 0.0,
                clarity: 0.0,
            },
        );
        let out = expand_adjust_spatial_filters(&[f.clone()]);
        assert_eq!(out.len(), 1);
        match (&out[0].params, &f.params) {
            (
                FilterParams::Adjust { contrast: a, .. },
                FilterParams::Adjust { contrast: b, .. },
            ) => assert!((a - b).abs() < 1e-6),
            _ => panic!("expected Adjust"),
        }
    }

    #[test]
    fn splits_blur_and_detail() {
        let f = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.2,
                brightness: 0.0,
                saturation: 0.0,
                blur: 1.0,
                sharpness: 0.5,
                noise: 0.1,
                exposure: 0.0,
                shadows: 0.0,
                highlights: 0.0,
                temperature: 0.0,
                tint: 0.0,
                vibrance: 0.0,
                hue: 0.0,
                clarity: 0.3,
            },
        );
        let out = expand_adjust_spatial_filters(&[f]);
        assert_eq!(out.len(), 3);
        assert!(matches!(
            out[0].params,
            FilterParams::Detail {
                clarity: c,
                sharpness: s
            } if (c - 0.3).abs() < 1e-6 && (s - 0.5).abs() < 1e-6
        ));
        assert!(matches!(
            out[1].params,
            FilterParams::BlurNoise { blur: b, noise: n }
                if (b - 1.0).abs() < 1e-6 && (n - 0.1).abs() < 1e-6
        ));
        match &out[2].params {
            FilterParams::Adjust {
                blur,
                sharpness,
                noise,
                clarity,
                contrast,
                ..
            } => {
                assert_eq!(*blur, 0.0);
                assert_eq!(*sharpness, 0.0);
                assert_eq!(*noise, 0.0);
                assert_eq!(*clarity, 0.0);
                assert!((*contrast - 0.2).abs() < 1e-6);
            }
            other => panic!("expected Adjust, got {other:?}"),
        }
    }

    #[test]
    fn expand_is_idempotent() {
        let f = FilterInstance::new(
            FilterKind::Adjust,
            FilterParams::Adjust {
                contrast: 0.0,
                brightness: 0.0,
                saturation: 0.0,
                blur: 0.5,
                sharpness: 0.0,
                noise: 0.0,
                exposure: 0.0,
                shadows: 0.0,
                highlights: 0.0,
                temperature: 0.0,
                tint: 0.0,
                vibrance: 0.0,
                hue: 0.0,
                clarity: 0.0,
            },
        );
        let once = expand_adjust_spatial_filters(&[f]);
        let twice = expand_adjust_spatial_filters(&once);
        assert_eq!(once.len(), twice.len());
        assert_eq!(once.len(), 2);
        assert!(matches!(once[0].params, FilterParams::BlurNoise { .. }));
        assert!(matches!(twice[0].params, FilterParams::BlurNoise { .. }));
        assert!(matches!(
            once[1].params,
            FilterParams::Adjust { blur: 0.0, .. }
        ));
    }
}

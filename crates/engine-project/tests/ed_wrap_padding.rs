//! Integration tests for ED approximate wrap-padding (gamedev.md §7.2).

use std::sync::atomic::{AtomicBool, Ordering};

use engine_project::filter::{
    DiffusionKernel, DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::filters::ed_wrap_padding::{
    apply_ed_buffer_no_wrap, apply_ed_with_wrap_padding, apply_ed_with_wrap_padding_simple,
    edge_margin, EdWrapError, WRAP_PADDING_HINT,
};
use engine_project::filters::{filter_uses_ed_wrap_padding, layer_has_full_document_filter_ex};
use engine_project::layer::Layer;
use engine_project::types::{LayerId, LayerKind};

fn ed_params(wrap: bool) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 4,
        wrap_padding: wrap,
        dither_alpha: false,
        serpentine: false,
        ..DitherParamsV2::default()
    }
}

fn ordered_params(wrap: bool) -> DitherParamsV2 {
    DitherParamsV2 {
        mode: DitherModeV2::Bayer8x8,
        levels: 4,
        wrap_padding: wrap,
        dither_alpha: false,
        ..DitherParamsV2::default()
    }
}

fn layer_with(params: DitherParamsV2, enabled: bool) -> Layer {
    let mut layer = Layer::new(LayerId(1), LayerKind::Raster, 64, 64);
    let mut filter = FilterInstance::new(FilterKind::Dither, FilterParams::DitherV2(params));
    filter.enabled = enabled;
    layer.filters.push(filter);
    layer
}

fn gradient(w: u32, h: u32) -> Vec<f32> {
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = (y * w as usize + x) * 4;
            let t = ((x + y) as f32 * 0.11).sin() * 0.5 + 0.5;
            rgba[i] = t;
            rgba[i + 1] = t * 0.85;
            rgba[i + 2] = 1.0 - t;
            rgba[i + 3] = 1.0;
        }
    }
    rgba
}

/// Mean abs discontinuity across opposite edges (tile-boundary proxy).
fn seam_metric(rgba: &[f32], w: u32, h: u32) -> f64 {
    let wu = w as usize;
    let hu = h as usize;
    let mut sum = 0.0f64;
    let mut n = 0u64;
    for y in 0..hu {
        let i0 = y * wu * 4;
        let i1 = (y * wu + (wu - 1)) * 4;
        for c in 0..3 {
            sum += (rgba[i0 + c] as f64 - rgba[i1 + c] as f64).abs();
            n += 1;
        }
    }
    for x in 0..wu {
        let i0 = x * 4;
        let i1 = ((hu - 1) * wu + x) * 4;
        for c in 0..3 {
            sum += (rgba[i0 + c] as f64 - rgba[i1 + c] as f64).abs();
            n += 1;
        }
    }
    sum / n as f64
}

#[test]
fn flag_true_deterministic() {
    let src = gradient(48, 40);
    let params = ed_params(true);
    let mut a = src.clone();
    let mut b = src;
    apply_ed_with_wrap_padding_simple(&mut a, 48, 40, &params).unwrap();
    apply_ed_with_wrap_padding_simple(&mut b, 48, 40, &params).unwrap();
    assert_eq!(a, b);
}

#[test]
fn ordered_ignores_flag() {
    let src = gradient(32, 32);
    let mut a = src.clone();
    let mut b = src;
    apply_ed_with_wrap_padding_simple(&mut a, 32, 32, &ordered_params(true)).unwrap();
    apply_ed_with_wrap_padding_simple(&mut b, 32, 32, &ordered_params(false)).unwrap();
    assert_eq!(a, b);
}

fn toroidal(w: u32, h: u32) -> Vec<f32> {
    // Smooth field that is continuous on the torus — padless ED still seams;
    // wrap-padding should reduce opposite-edge discontinuity.
    let mut rgba = vec![0.0f32; (w * h * 4) as usize];
    let tw = std::f32::consts::TAU / w as f32;
    let th = std::f32::consts::TAU / h as f32;
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = (y * w as usize + x) * 4;
            let v = 0.5
                + 0.25 * (x as f32 * tw).sin()
                + 0.25 * (y as f32 * th).cos();
            rgba[i] = v.clamp(0.0, 1.0);
            rgba[i + 1] = v.clamp(0.0, 1.0);
            rgba[i + 2] = v.clamp(0.0, 1.0);
            rgba[i + 3] = 1.0;
        }
    }
    rgba
}

#[test]
fn flag_true_nonperiodic_reduces_seam() {
    let w = 64u32;
    let h = 48u32;
    let src = toroidal(w, h);
    let mut off = src.clone();
    let mut on = src;
    let params_off = DitherParamsV2 {
        mode: DitherModeV2::FloydSteinberg,
        levels: 32,
        wrap_padding: false,
        dither_alpha: false,
        serpentine: false,
        ..DitherParamsV2::default()
    };
    let params_on = DitherParamsV2 {
        wrap_padding: true,
        ..params_off.clone()
    };
    apply_ed_buffer_no_wrap(&mut off, w, h, &params_off).unwrap();
    apply_ed_with_wrap_padding_simple(&mut on, w, h, &params_on).unwrap();
    let seam_off = seam_metric(&off, w, h);
    let seam_on = seam_metric(&on, w, h);
    assert_ne!(on, off);
    assert!(
        seam_on < seam_off,
        "wrap-padding should reduce wrap discontinuity ({seam_on} < {seam_off})"
    );
}

#[test]
fn routing_full_document_triggered() {
    let layer = layer_with(ed_params(true), true);
    assert!(filter_uses_ed_wrap_padding(&layer.filters[0]));
    assert!(layer_has_full_document_filter_ex(&layer, true));
}

#[test]
fn routing_not_triggered_when_disabled() {
    let layer = layer_with(ed_params(true), false);
    assert!(!layer_has_full_document_filter_ex(&layer, true));
}

#[test]
fn routing_not_triggered_for_ordered() {
    let layer = layer_with(ordered_params(true), true);
    assert!(!filter_uses_ed_wrap_padding(&layer.filters[0]));
    assert!(!layer_has_full_document_filter_ex(&layer, true));
}

#[test]
fn routing_flag_combinations() {
    for enabled in [true, false] {
        for wrap in [true, false] {
            let ed = layer_with(ed_params(wrap), enabled);
            let expect = enabled && wrap;
            assert_eq!(
                layer_has_full_document_filter_ex(&ed, true),
                expect,
                "ED enabled={enabled} wrap={wrap}"
            );
            let ord = layer_with(ordered_params(wrap), enabled);
            assert!(
                !layer_has_full_document_filter_ex(&ord, true),
                "ordered must never trigger wrap full-doc"
            );
        }
    }
}

#[test]
fn cancellation_leaves_clean_error() {
    let mut rgba = gradient(64, 96);
    let params = ed_params(true);
    let cancel = AtomicBool::new(true);
    let err = apply_ed_with_wrap_padding(&mut rgba, 64, 96, &params, &cancel, None).unwrap_err();
    assert_eq!(err, EdWrapError::Cancelled);
    cancel.store(false, Ordering::Relaxed);
    let mut rgba2 = gradient(64, 96);
    apply_ed_with_wrap_padding(&mut rgba2, 64, 96, &params, &cancel, None).unwrap();
}

#[test]
fn progress_reported_per_row() {
    let mut rgba = gradient(24, 24);
    let params = ed_params(true);
    let cancel = AtomicBool::new(false);
    let mut last = -1.0f32;
    let mut on_progress = |p: f32| {
        assert!(p + 1e-6 >= last);
        last = p;
    };
    apply_ed_with_wrap_padding(
        &mut rgba,
        24,
        24,
        &params,
        &cancel,
        Some(&mut on_progress),
    )
    .unwrap();
    assert!((last - 1.0).abs() < 1e-5);
}

#[test]
fn edge_margin_pair_api() {
    assert_eq!(edge_margin(DiffusionKernel::FloydSteinberg, 1), (1, 1));
}

#[test]
fn hint_constant_stable() {
    assert_eq!(
        WRAP_PADDING_HINT,
        "Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact."
    );
}

#[test]
fn output_size_invariant() {
    for &(w, h) in &[(1u32, 1), (3, 7), (16, 16), (33, 19)] {
        let mut rgba = gradient(w, h);
        apply_ed_with_wrap_padding_simple(&mut rgba, w, h, &ed_params(true)).unwrap();
        assert_eq!(rgba.len(), (w * h * 4) as usize);
    }
}

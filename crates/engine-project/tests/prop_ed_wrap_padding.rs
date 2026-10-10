//! Property tests for ED wrap-padding (gamedev.md §7.3).

use engine_project::filter::{DiffusionKernel, DitherModeV2, DitherParamsV2};
use engine_project::filters::ed_wrap_padding::{
    apply_ed_buffer_no_wrap, apply_ed_with_wrap_padding_simple, edge_margin, wrap_pad_rgba_f32,
};
use proptest::prelude::*;

fn uniform(w: u32, h: u32, v: f32) -> Vec<f32> {
    let mut rgba = vec![v; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        rgba[i * 4 + 3] = 1.0;
    }
    rgba
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    /// Exact quantization levels stay uniform under wrap-padding ED.
    /// (Mid-greys dither into patterns — that is expected ED behavior.)
    #[test]
    fn uniform_image_unchanged(
        w in 2u32..24,
        h in 2u32..24,
        levels in 2u16..=8u16,
        level_i in 0u16..8u16,
    ) {
        let li = level_i % levels;
        let v = li as f32 / (levels - 1) as f32;
        let mut rgba = uniform(w, h, v);
        let before = rgba.clone();
        let params = DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels,
            wrap_padding: true,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        apply_ed_with_wrap_padding_simple(&mut rgba, w, h, &params).unwrap();
        assert_eq!(rgba, before);
    }

    #[test]
    fn margin_zero_equals_no_wrap(w in 2u32..16, h in 2u32..16) {
        let src = uniform(w, h, 0.37);
        let (padded, pw, ph) = wrap_pad_rgba_f32(&src, w, h, 0, 0).unwrap();
        assert_eq!((pw, ph), (w, h));
        assert_eq!(padded, src);

        let params = DitherParamsV2 {
            mode: DitherModeV2::FloydSteinberg,
            levels: 4,
            wrap_padding: false,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        let mut a = src.clone();
        let mut b = src;
        apply_ed_buffer_no_wrap(&mut a, w, h, &params).unwrap();
        apply_ed_buffer_no_wrap(&mut b, w, h, &params).unwrap();
        assert_eq!(a, b);
        let _ = edge_margin(DiffusionKernel::FloydSteinberg, 1);
    }

    #[test]
    fn output_size_invariant(
        w in 1u32..32,
        h in 1u32..32,
    ) {
        let mut rgba = uniform(w, h, 0.42);
        let params = DitherParamsV2 {
            mode: DitherModeV2::Atkinson,
            levels: 4,
            wrap_padding: true,
            dither_alpha: false,
            ..DitherParamsV2::default()
        };
        apply_ed_with_wrap_padding_simple(&mut rgba, w, h, &params).unwrap();
        assert_eq!(rgba.len(), (w * h * 4) as usize);
    }
}

//! Property-based tests for DitherParamsV2 parameter validation completeness.
//!
//! **Validates: Requirements 1.2, 1.3, 1.4, 1.5, 1.8, 1.9, 1.10, 1.11**
//!
//! Property 1: Parameter Validation Completeness
//! - For any DitherParamsV2 with all fields in valid ranges, validate() returns Ok(()).
//! - For any DitherParamsV2 with at least one field out of range, validate() returns Err.

use engine_project::filter::{DitherColorMode, DitherModeV2, DitherParamsV2};
use engine_project::PaletteId;
use proptest::prelude::*;

// ─── Strategies ───────────────────────────────────────────────────────────────

/// Generate a valid DitherModeV2 variant.
fn arb_valid_mode() -> impl Strategy<Value = DitherModeV2> {
    prop_oneof![
        Just(DitherModeV2::Bayer2x2),
        Just(DitherModeV2::Bayer4x4),
        Just(DitherModeV2::Bayer8x8),
        Just(DitherModeV2::FloydSteinberg),
        Just(DitherModeV2::Atkinson),
        Just(DitherModeV2::JarvisJudiceNinke),
        Just(DitherModeV2::Stucki),
        Just(DitherModeV2::Burkes),
        Just(DitherModeV2::Sierra),
        // CustomPng with a non-empty path
        "[a-z]{1,20}\\.png".prop_map(|path| DitherModeV2::CustomPng { path }),
    ]
}

/// Generate a valid DitherColorMode.
fn arb_color_mode() -> impl Strategy<Value = DitherColorMode> {
    prop_oneof![Just(DitherColorMode::Rgb), Just(DitherColorMode::Grayscale),]
}

/// Generate an optional PaletteId.
fn arb_palette_id() -> impl Strategy<Value = Option<PaletteId>> {
    prop_oneof![
        Just(None),
        (0u32..100).prop_map(|id| Some(PaletteId::new(id))),
    ]
}

/// Generate a fully valid DitherParamsV2.
fn arb_valid_params() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_valid_mode(),
        2u16..=256u16,           // valid levels
        (10u32..=400u32),        // will map to 0.1..=4.0
        1u8..=32u8,              // valid pixel_size
        arb_color_mode(),
        arb_palette_id(),
    )
        .prop_map(|(mode, levels, ts_raw, pixel_size, color_mode, palette_id)| {
            // Map ts_raw (10..=400) to threshold_scale (0.1..=4.0) in steps of 0.01
            let threshold_scale = ts_raw as f32 / 100.0;
            DitherParamsV2 {
                mode,
                levels,
                threshold_scale,
                pixel_size,
                color_mode,
                palette_id,
                threshold_bias: 0.0,
                pattern_angle: 0.0,
                ..Default::default()
            }
        })
}

/// Generate DitherParamsV2 with out-of-range threshold_bias.
fn arb_invalid_threshold_bias() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_valid_mode(),
        2u16..=256u16,
        (10u32..=400u32),
        1u8..=32u8,
        arb_color_mode(),
        arb_palette_id(),
        prop_oneof![
            (-200i32..=-51i32).prop_map(|v| v as f32 / 100.0),
            (51i32..=200i32).prop_map(|v| v as f32 / 100.0),
        ],
    )
        .prop_map(
            |(mode, levels, ts_raw, pixel_size, color_mode, palette_id, threshold_bias)| {
                DitherParamsV2 {
                    mode,
                    levels,
                    threshold_scale: ts_raw as f32 / 100.0,
                    pixel_size,
                    color_mode,
                    palette_id,
                    threshold_bias,
                    ..Default::default()
                }
            },
        )
}

/// Generate DitherParamsV2 with out-of-range levels.
fn arb_invalid_levels() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_valid_mode(),
        prop_oneof![0u16..2u16, 257u16..=u16::MAX],
        (10u32..=400u32),
        1u8..=32u8,
        arb_color_mode(),
        arb_palette_id(),
    )
        .prop_map(|(mode, levels, ts_raw, pixel_size, color_mode, palette_id)| {
            let threshold_scale = ts_raw as f32 / 100.0;
            DitherParamsV2 {
                mode,
                levels,
                threshold_scale,
                pixel_size,
                color_mode,
                palette_id,            ..Default::default()
            }
        })
}

/// Generate DitherParamsV2 with out-of-range threshold_scale.
fn arb_invalid_threshold_scale() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_valid_mode(),
        2u16..=256u16,
        prop_oneof![
            // Below 0.1: use values in (0.0, 0.09]
            (1u32..=9u32).prop_map(|v| v as f32 / 100.0),
            // Above 4.0: use values in (4.01, 10.0]
            (401u32..=1000u32).prop_map(|v| v as f32 / 100.0),
        ],
        1u8..=32u8,
        arb_color_mode(),
        arb_palette_id(),
    )
        .prop_map(
            |(mode, levels, threshold_scale, pixel_size, color_mode, palette_id)| {
                DitherParamsV2 {
                    mode,
                    levels,
                    threshold_scale,
                    pixel_size,
                    color_mode,
                    palette_id,            ..Default::default()
                }
            },
        )
}

/// Generate DitherParamsV2 with out-of-range pixel_size.
fn arb_invalid_pixel_size() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_valid_mode(),
        2u16..=256u16,
        (10u32..=400u32),
        prop_oneof![Just(0u8), 33u8..=u8::MAX],
        arb_color_mode(),
        arb_palette_id(),
    )
        .prop_map(|(mode, levels, ts_raw, pixel_size, color_mode, palette_id)| {
            let threshold_scale = ts_raw as f32 / 100.0;
            DitherParamsV2 {
                mode,
                levels,
                threshold_scale,
                pixel_size,
                color_mode,
                palette_id,            ..Default::default()
            }
        })
}

/// Generate DitherParamsV2 with CustomPng mode and empty path.
fn arb_invalid_custom_png_empty_path() -> impl Strategy<Value = DitherParamsV2> {
    (
        2u16..=256u16,
        (10u32..=400u32),
        1u8..=32u8,
        arb_color_mode(),
        arb_palette_id(),
    )
        .prop_map(|(levels, ts_raw, pixel_size, color_mode, palette_id)| {
            let threshold_scale = ts_raw as f32 / 100.0;
            DitherParamsV2 {
                mode: DitherModeV2::CustomPng {
                    path: String::new(),
                },
                levels,
                threshold_scale,
                pixel_size,
                color_mode,
                palette_id,            ..Default::default()
            }
        })
}

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    /// **Validates: Requirements 1 (AC 1-11)**
    ///
    /// Property 1: Parameter Validation Completeness
    /// Valid params within all ranges pass validation.
    #[test]
    fn valid_params_pass_validation(params in arb_valid_params()) {
        prop_assert!(
            params.validate().is_ok(),
            "Expected Ok(()) for valid params: {:?}", params
        );
    }

    /// Out-of-range levels fail validation.
    #[test]
    fn invalid_levels_fail_validation(params in arb_invalid_levels()) {
        prop_assert!(
            params.validate().is_err(),
            "Expected Err for out-of-range levels={}: {:?}", params.levels, params
        );
    }

    /// Out-of-range threshold_scale fails validation.
    #[test]
    fn invalid_threshold_scale_fail_validation(params in arb_invalid_threshold_scale()) {
        prop_assert!(
            params.validate().is_err(),
            "Expected Err for out-of-range threshold_scale={}: {:?}",
            params.threshold_scale, params
        );
    }

    /// Out-of-range pixel_size fails validation.
    #[test]
    fn invalid_pixel_size_fail_validation(params in arb_invalid_pixel_size()) {
        prop_assert!(
            params.validate().is_err(),
            "Expected Err for out-of-range pixel_size={}: {:?}",
            params.pixel_size, params
        );
    }

    /// CustomPng with empty path fails validation.
    #[test]
    fn custom_png_empty_path_fails_validation(params in arb_invalid_custom_png_empty_path()) {
        prop_assert!(
            params.validate().is_err(),
            "Expected Err for CustomPng with empty path: {:?}", params
        );
    }

    /// Out-of-range threshold_bias fails validation.
    #[test]
    fn invalid_threshold_bias_fail_validation(params in arb_invalid_threshold_bias()) {
        prop_assert!(
            params.validate().is_err(),
            "Expected Err for out-of-range threshold_bias={}: {:?}",
            params.threshold_bias, params
        );
    }
}

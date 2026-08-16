//! Property-based tests for DitherParamsV2 serialization round-trip.
//!
//! **Validates: Requirements 11 (AC 1-2)**
//!
//! Property 11: Serialization Round-Trip
//! - For any valid DitherParamsV2 instance, serializing to JSON and deserializing
//!   back produces a value equal to the original.

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
        // CustomPng with a non-empty path (use printable ASCII to avoid JSON encoding edge cases)
        "[a-zA-Z0-9_/]{1,50}\\.png".prop_map(|path| DitherModeV2::CustomPng { path }),
    ]
}

/// Generate a valid DitherColorMode.
fn arb_color_mode() -> impl Strategy<Value = DitherColorMode> {
    prop_oneof![Just(DitherColorMode::Rgb), Just(DitherColorMode::Grayscale)]
}

/// Generate an optional PaletteId.
fn arb_palette_id() -> impl Strategy<Value = Option<PaletteId>> {
    prop_oneof![
        Just(None),
        (0u32..1000).prop_map(|id| Some(PaletteId::new(id))),
    ]
}

/// Generate a fully valid DitherParamsV2.
fn arb_valid_params() -> impl Strategy<Value = DitherParamsV2> {
    (
        arb_valid_mode(),
        2u16..=256u16,      // valid levels
        (10u32..=400u32),   // will map to 0.1..=4.0
        1u8..=32u8,         // valid pixel_size
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
                threshold_bias: ((levels as i32 % 11) - 5) as f32 / 10.0,
                pattern_angle: (pixel_size as f32) * 15.0,
                serpentine: levels % 2 == 0,
                dither_alpha: pixel_size % 2 == 0,
                ..Default::default()
            }
        })
}

// ─── Property Tests ───────────────────────────────────────────────────────────

proptest! {
    /// **Validates: Requirements 11 (AC 1-2)**
    ///
    /// Property 11: Serialization Round-Trip
    /// For any valid DitherParamsV2, serialize to JSON then deserialize back.
    /// The deserialized value, when re-serialized, must produce identical JSON.
    #[test]
    fn serde_round_trip_preserves_all_params(params in arb_valid_params()) {
        // Serialize to JSON Value
        let json_value = serde_json::to_value(&params)
            .expect("serialization should succeed for valid params");

        // Deserialize back from JSON Value
        let deserialized: DitherParamsV2 = serde_json::from_value(json_value.clone())
            .expect("deserialization should succeed for valid serialized params");

        // Re-serialize and compare JSON values (since PartialEq is not derived on all types)
        let json_value_2 = serde_json::to_value(&deserialized)
            .expect("re-serialization should succeed");

        prop_assert_eq!(
            json_value, json_value_2,
            "Round-trip failed: original JSON differs from re-serialized JSON"
        );
    }

    /// **Validates: Requirements 11 (AC 1-2)**
    ///
    /// Property 11: Serialization Round-Trip (string format)
    /// For any valid DitherParamsV2, serialize to a JSON string, deserialize,
    /// and verify the deserialized value passes validation.
    #[test]
    fn deserialized_params_remain_valid(params in arb_valid_params()) {
        // Confirm input is valid
        prop_assert!(params.validate().is_ok(), "Generated params should be valid");

        // Serialize to JSON string
        let json_str = serde_json::to_string(&params)
            .expect("serialization to string should succeed");

        // Deserialize back
        let deserialized: DitherParamsV2 = serde_json::from_str(&json_str)
            .expect("deserialization from string should succeed");

        // Deserialized params must still pass validation
        prop_assert!(
            deserialized.validate().is_ok(),
            "Deserialized params should still be valid: {:?}", deserialized
        );
    }
}

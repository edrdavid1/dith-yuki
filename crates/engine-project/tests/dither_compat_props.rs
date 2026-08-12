//! Property-based and unit tests for legacy dither migration correctness.
//!
//! **Validates: Requirements 12 (AC 1-4)**

use engine_project::filter::{
    DiffusionKernel, DitherColorMode, DitherMode, DitherModeV2, DitherParamsV2,
};
use proptest::prelude::*;

// ─── Generators ───────────────────────────────────────────────────────────────

/// Generate a valid legacy DitherMode covering all variants.
fn arb_legacy_dither_mode() -> impl Strategy<Value = DitherMode> {
    prop_oneof![
        // Bayer with valid matrix sizes
        Just(DitherMode::Bayer { matrix_size: 2 }),
        Just(DitherMode::Bayer { matrix_size: 4 }),
        Just(DitherMode::Bayer { matrix_size: 8 }),
        // ThresholdMap with random non-empty paths
        "[a-z/]{1,50}\\.png".prop_map(|path| DitherMode::ThresholdMap { path }),
        // ErrorDiffusion with each kernel variant
        Just(DitherMode::ErrorDiffusion {
            kernel: DiffusionKernel::FloydSteinberg,
        }),
        Just(DitherMode::ErrorDiffusion {
            kernel: DiffusionKernel::Atkinson,
        }),
        Just(DitherMode::ErrorDiffusion {
            kernel: DiffusionKernel::JarvisJudiceNinke,
        }),
        Just(DitherMode::ErrorDiffusion {
            kernel: DiffusionKernel::Stucki,
        }),
    ]
}

/// Generate a valid legacy color_depth in [1, 8].
fn arb_color_depth() -> impl Strategy<Value = u8> {
    1u8..=8u8
}

// ─── Property 12: Legacy Migration Correctness ────────────────────────────────

proptest! {
    /// **Validates: Requirements 12.1, 12.2, 12.3, 12.4**
    ///
    /// For all valid legacy (DitherMode, color_depth) combinations:
    /// 1. The resulting DitherParamsV2::validate() returns Ok(())
    /// 2. levels == 2^color_depth
    /// 3. Default params are correct (threshold_scale=1.0, pixel_size=1, color_mode=Rgb, palette_id=None)
    #[test]
    fn prop_legacy_migration_produces_valid_params(
        mode in arb_legacy_dither_mode(),
        color_depth in arb_color_depth(),
    ) {
        let params = DitherParamsV2::from((mode, color_depth));

        // 1. Validation must pass
        params.validate().expect("Converted params should be valid");

        // 2. levels == 2^color_depth
        let expected_levels = 1u16 << color_depth;
        prop_assert_eq!(
            params.levels, expected_levels,
            "levels should be 2^color_depth = 2^{} = {}, got {}",
            color_depth, expected_levels, params.levels
        );

        // 3. Default params
        prop_assert_eq!(params.threshold_scale, 1.0, "threshold_scale should default to 1.0");
        prop_assert_eq!(params.pixel_size, 1, "pixel_size should default to 1");
        prop_assert_eq!(params.color_mode, DitherColorMode::Rgb, "color_mode should default to Rgb");
        prop_assert!(params.palette_id.is_none(), "palette_id should default to None");
    }

    /// Verify Bayer modes map to the correct V2 variant.
    /// **Validates: Requirements 12.2**
    #[test]
    fn prop_bayer_mode_mapping(
        color_depth in arb_color_depth(),
    ) {
        let p2 = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 2 }, color_depth));
        prop_assert!(matches!(p2.mode, DitherModeV2::Bayer2x2), "Bayer{{2}} should map to Bayer2x2");

        let p4 = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 4 }, color_depth));
        prop_assert!(matches!(p4.mode, DitherModeV2::Bayer4x4), "Bayer{{4}} should map to Bayer4x4");

        let p8 = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 8 }, color_depth));
        prop_assert!(matches!(p8.mode, DitherModeV2::Bayer8x8), "Bayer{{8}} should map to Bayer8x8");
    }

    /// Verify ThresholdMap maps to CustomPng with the same path preserved.
    /// **Validates: Requirements 12.4**
    #[test]
    fn prop_threshold_map_preserves_path(
        path in "[a-z/]{1,50}\\.png",
        color_depth in arb_color_depth(),
    ) {
        let params = DitherParamsV2::from((
            DitherMode::ThresholdMap { path: path.clone() },
            color_depth,
        ));
        match &params.mode {
            DitherModeV2::CustomPng { path: p } => {
                prop_assert_eq!(p, &path, "Path should be preserved in CustomPng");
            }
            other => {
                prop_assert!(false, "ThresholdMap should map to CustomPng, got {:?}", other);
            }
        }
    }

    /// Verify ErrorDiffusion kernels map to correct V2 modes.
    /// **Validates: Requirements 12.3**
    #[test]
    fn prop_error_diffusion_kernel_mapping(
        color_depth in arb_color_depth(),
    ) {
        let fs = DitherParamsV2::from((
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::FloydSteinberg },
            color_depth,
        ));
        prop_assert!(matches!(fs.mode, DitherModeV2::FloydSteinberg),
            "FloydSteinberg kernel should map to FloydSteinberg mode");

        let at = DitherParamsV2::from((
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Atkinson },
            color_depth,
        ));
        prop_assert!(matches!(at.mode, DitherModeV2::Atkinson),
            "Atkinson kernel should map to Atkinson mode");

        // JJN and Stucki fall back to FloydSteinberg
        let jjn = DitherParamsV2::from((
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::JarvisJudiceNinke },
            color_depth,
        ));
        prop_assert!(matches!(jjn.mode, DitherModeV2::FloydSteinberg),
            "JarvisJudiceNinke should fall back to FloydSteinberg");

        let stucki = DitherParamsV2::from((
            DitherMode::ErrorDiffusion { kernel: DiffusionKernel::Stucki },
            color_depth,
        ));
        prop_assert!(matches!(stucki.mode, DitherModeV2::FloydSteinberg),
            "Stucki should fall back to FloydSteinberg");
    }
}

// ─── Task 7.4: Unit tests for specific legacy conversions ─────────────────────

#[cfg(test)]
mod unit_tests {
    use super::*;

    /// Verify: Bayer{2} with color_depth=2 → Bayer2x2 with levels=4 (2^2)
    /// **Validates: Requirement 12.2**
    #[test]
    fn bayer2_color_depth_2_maps_to_bayer2x2_levels4() {
        let params = DitherParamsV2::from((DitherMode::Bayer { matrix_size: 2 }, 2u8));

        assert!(matches!(params.mode, DitherModeV2::Bayer2x2));
        assert_eq!(params.levels, 4); // 2^2
        assert_eq!(params.threshold_scale, 1.0);
        assert_eq!(params.pixel_size, 1);
        assert_eq!(params.color_mode, DitherColorMode::Rgb);
        assert!(params.palette_id.is_none());
        params.validate().unwrap();
    }

    /// Verify: FloydSteinberg with color_depth=4 → FloydSteinberg with levels=16 (2^4)
    /// **Validates: Requirement 12.3**
    #[test]
    fn floyd_steinberg_color_depth_4_maps_to_floyd_steinberg_levels16() {
        let params = DitherParamsV2::from((
            DitherMode::ErrorDiffusion {
                kernel: DiffusionKernel::FloydSteinberg,
            },
            4u8,
        ));

        assert!(matches!(params.mode, DitherModeV2::FloydSteinberg));
        assert_eq!(params.levels, 16); // 2^4
        assert_eq!(params.threshold_scale, 1.0);
        assert_eq!(params.pixel_size, 1);
        assert_eq!(params.color_mode, DitherColorMode::Rgb);
        assert!(params.palette_id.is_none());
        params.validate().unwrap();
    }

    /// Verify: ThresholdMap{"/path/to/map.png"} → CustomPng{"/path/to/map.png"}
    /// **Validates: Requirement 12.4**
    #[test]
    fn threshold_map_maps_to_custom_png_with_same_path() {
        let params = DitherParamsV2::from((
            DitherMode::ThresholdMap {
                path: "/path/to/map.png".to_string(),
            },
            3u8,
        ));

        match &params.mode {
            DitherModeV2::CustomPng { path } => {
                assert_eq!(path, "/path/to/map.png");
            }
            other => panic!("Expected CustomPng, got {:?}", other),
        }
        assert_eq!(params.levels, 8); // 2^3
        assert_eq!(params.threshold_scale, 1.0);
        assert_eq!(params.pixel_size, 1);
        assert_eq!(params.color_mode, DitherColorMode::Rgb);
        assert!(params.palette_id.is_none());
        params.validate().unwrap();
    }
}

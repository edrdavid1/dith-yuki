//! F1 step 4: period snap chip + Relative/Exact chip wiring (engine-side).

use engine_project::filter::{
    DitherModeV2, DitherParamsV2, FilterInstance, FilterKind, FilterParams,
};
use engine_project::gamedev::{
    chip_action, evaluate_gamedev_chips, evaluate_seamless_chip, period_snap_crop_size,
    validate_seams_rgba8, ChipState, GamedevContext, SeamMode,
};
use engine_project::layer::Layer;
use engine_project::types::{LayerId, LayerKind};

fn bayer_layer(w: u32, h: u32) -> Layer {
    let mut layer = Layer::new(LayerId::new(1), LayerKind::Raster, w, h);
    layer.filters.push(FilterInstance::new(
        FilterKind::Dither,
        FilterParams::DitherV2(DitherParamsV2 {
            mode: DitherModeV2::Bayer8x8,
            pixel_size: 1,
            ..DitherParamsV2::default()
        }),
    ));
    layer
}

#[test]
fn snap_chip_action_id_and_target() {
    let layer = bayer_layer(40, 36);
    assert_eq!(period_snap_crop_size(&layer, 40, 36), Some((40, 32)));
    // 40 is multiple of 8; 36 → 32.
    let chip = evaluate_seamless_chip(&layer, true, 40, 36, None);
    assert_eq!(chip.state, ChipState::Warn);
    assert_eq!(chip.action_id.as_deref(), Some(chip_action::SNAP_TO_PERIOD));
    assert!(chip.label.contains("40×32") || chip.label.contains("snap"));
}

#[test]
fn after_snap_size_no_period_chip() {
    let layer = bayer_layer(32, 32);
    assert!(period_snap_crop_size(&layer, 32, 32).is_none());
    // Flat Exact-ok buffer.
    let buf = vec![200u8; (32 * 32 * 4) as usize];
    let report = validate_seams_rgba8(&buf, 32, 32, SeamMode::Exact).unwrap();
    assert!(report.ok);
    let mut ctx = GamedevContext::default();
    ctx.tile.enabled = true;
    let chips = evaluate_gamedev_chips(&layer, &ctx, 32, 32, Some(&report));
    assert_eq!(chips[0].state, ChipState::Ok);
    assert!(chips[0].label.contains('✓') || chips[0].label.contains("seamless"));
}

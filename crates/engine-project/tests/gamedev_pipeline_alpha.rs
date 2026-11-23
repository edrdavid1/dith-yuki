//! F4 acceptance: soft alpha → hard; transparent RGB cleared (no bleed).

use engine_project::{
    apply_alpha_policy_rgba8, clear_transparent_rgb_rgba8, evaluate_alpha_chip,
    has_semi_transparent_rgba8, integer_nearest_upscale_rgba8, threshold_alpha_rgba8,
    AlphaPolicy, ChipState, GamedevContext,
};

#[test]
fn soft_alpha_threshold_leaves_no_semi_transparent() {
    let mut rgba = Vec::new();
    for a in [0u8, 1, 64, 127, 128, 200, 255] {
        rgba.extend_from_slice(&[180, 40, 40, a]);
    }
    assert!(has_semi_transparent_rgba8(&rgba));
    threshold_alpha_rgba8(&mut rgba, &AlphaPolicy::default());
    assert!(!has_semi_transparent_rgba8(&rgba));
    // Below 0.5 → cleared RGB
    assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
    assert_eq!(&rgba[4..8], &[0, 0, 0, 0]);
    assert_eq!(&rgba[8..12], &[0, 0, 0, 0]);
    assert_eq!(&rgba[12..16], &[0, 0, 0, 0]);
    // ≥ 0.5 → opaque, RGB kept
    assert_eq!(&rgba[16..20], &[180, 40, 40, 255]);
    assert_eq!(&rgba[20..24], &[180, 40, 40, 255]);
    assert_eq!(&rgba[24..28], &[180, 40, 40, 255]);
}

#[test]
fn transparent_neighbor_does_not_keep_fringe_rgb() {
    // Soft red fringe beside opaque gray — after policy, fringe RGB is zeroed.
    let mut rgba = vec![
        220, 30, 30, 40, // soft fringe
        20, 20, 20, 255, // neighbor
    ];
    apply_alpha_policy_rgba8(&mut rgba, &AlphaPolicy::default());
    assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
    assert_eq!(&rgba[4..8], &[20, 20, 20, 255]);
    // Sampling the transparent pixel cannot pollute with red.
    assert_eq!(rgba[0], 0);
    assert_eq!(rgba[1], 0);
    assert_eq!(rgba[2], 0);
}

#[test]
fn clear_rgb_only_when_already_binary() {
    let mut rgba = vec![9u8, 9, 9, 0, 1, 2, 3, 255];
    clear_transparent_rgb_rgba8(&mut rgba);
    assert_eq!(&rgba[0..4], &[0, 0, 0, 0]);
    assert_eq!(&rgba[4..8], &[1, 2, 3, 255]);
}

#[test]
fn alpha_chip_warns_on_soft() {
    let soft = vec![10u8, 10, 10, 90, 0, 0, 0, 255];
    let ctx = GamedevContext::default();
    let chip = evaluate_alpha_chip(&ctx, Some(&soft));
    assert_eq!(chip.state, ChipState::Warn);
    assert_eq!(chip.action_id.as_deref(), Some("threshold_alpha"));

    let hard = vec![0u8, 0, 0, 0, 1, 2, 3, 255];
    let ok = evaluate_alpha_chip(&ctx, Some(&hard));
    assert_eq!(ok.state, ChipState::Ok);
}

#[test]
fn workspace_defaults_set_png8_and_nearest() {
    let mut ctx = GamedevContext::default();
    ctx.apply_workspace_defaults();
    assert!(ctx.export.png8);
    assert!(ctx.pixel_perfect.nearest_preview);
    assert!(ctx.alpha.no_dither_alpha);
}

#[test]
fn integer_upscale_preserves_blocks() {
    let src = vec![7u8, 8, 9, 255];
    let (out, w, h) = integer_nearest_upscale_rgba8(&src, 1, 1, 3).unwrap();
    assert_eq!((w, h), (3, 3));
    for px in out.chunks_exact(4) {
        assert_eq!(px, &[7, 8, 9, 255]);
    }
}

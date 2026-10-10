//! Fuzz: ED wrap-padding pad → ED → crop invariants (gamedev.md §7.4).

#![no_main]
use std::sync::atomic::AtomicBool;

use engine_project::filter::{DitherModeV2, DitherParamsV2};
use engine_project::filters::ed_wrap_padding::{
    apply_ed_with_wrap_padding, wrap_pad_rgba_f32, MAX_PADDED_PIXELS,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 6 {
        return;
    }
    let pixel_size = (data[0] % 8) + 1;
    let kernel_i = data[1] % 5;
    let w = (u16::from_le_bytes([data[2], data[3]]) % 257) as u32; // 0..=256
    let h = (u16::from_le_bytes([data[4], data[5]]) % 257) as u32;
    if w == 0 || h == 0 {
        return;
    }
    // Soft cap: avoid multi-GB allocations in the fuzzer.
    if (w as u64).saturating_mul(h as u64) > 64 * 64 {
        return;
    }
    let mode = match kernel_i {
        0 => DitherModeV2::FloydSteinberg,
        1 => DitherModeV2::JarvisJudiceNinke,
        2 => DitherModeV2::Stucki,
        3 => DitherModeV2::Burkes,
        _ => DitherModeV2::Atkinson,
    };
    let mut rgba = vec![0.5f32; (w * h * 4) as usize];
    for i in 0..(w * h) as usize {
        let b = data.get(6 + (i % data.len().saturating_sub(6).max(1))).copied().unwrap_or(128);
        let v = b as f32 / 255.0;
        rgba[i * 4] = v;
        rgba[i * 4 + 1] = v;
        rgba[i * 4 + 2] = v;
        rgba[i * 4 + 3] = 1.0;
    }
    let params = DitherParamsV2 {
        mode,
        levels: 4,
        pixel_size,
        wrap_padding: true,
        dither_alpha: false,
        ..DitherParamsV2::default()
    };

    // Pad alone must respect ceilings.
    if let Ok((padded, pw, ph)) = wrap_pad_rgba_f32(&rgba, w, h, 1.min(w), 1.min(h)) {
        assert_eq!(padded.len(), (pw * ph * 4) as usize);
        assert!((pw as u64).saturating_mul(ph as u64) as usize <= MAX_PADDED_PIXELS
            || pw * ph <= w * h + 4); // if over limit, wrap_pad errors — Ok branch is under limit
    }

    let mut a = rgba.clone();
    let mut b = rgba;
    let cancel = AtomicBool::new(false);
    let r1 = apply_ed_with_wrap_padding(&mut a, w, h, &params, &cancel, None);
    let r2 = apply_ed_with_wrap_padding(&mut b, w, h, &params, &cancel, None);
    match (r1, r2) {
        (Ok(()), Ok(())) => {
            assert_eq!(a.len(), (w * h * 4) as usize);
            assert_eq!(a, b);
        }
        (Err(e1), Err(e2)) => {
            assert_eq!(e1, e2);
        }
        _ => panic!("deterministic error/ok mismatch"),
    }
});

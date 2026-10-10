//! Bake-off: moxcms forward sRGB→CMYK vs lcms2 (print-export CMS gate).
//!
//! Decision recorded in `docs/print-export-cmyk.md`: stay on moxcms for soft
//! proof; use **lcms2 for print export** because app-owned forward BPC diverges.
use engine_color::soft_proof::BUILTIN_FOGRA51_ICC;
use lcms2::{Flags, Intent, PixelFormat, Profile, Transform};
use moxcms::{ColorProfile, Layout, RenderingIntent, TransformOptions};

#[test]
fn fogra51_forward_moxcms_vs_lcms2() {
    let proof = ColorProfile::new_from_slice(BUILTIN_FOGRA51_ICC).expect("FOGRA51");
    let srgb = ColorProfile::new_srgb();
    let opts = TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        ..Default::default()
    };
    let to_cmyk = srgb
        .create_transform_f32(Layout::Rgb, &proof, Layout::Rgba, opts)
        .expect("moxcms");

    let srgb_l = Profile::new_srgb();
    let proof_l = Profile::new_icc(BUILTIN_FOGRA51_ICC).expect("lcms");
    let x_no = Transform::new_flags(
        &srgb_l,
        PixelFormat::RGB_8,
        &proof_l,
        PixelFormat::CMYK_8,
        Intent::RelativeColorimetric,
        Flags::default(),
    )
    .unwrap();
    let x_bpc = Transform::new_flags(
        &srgb_l,
        PixelFormat::RGB_8,
        &proof_l,
        PixelFormat::CMYK_8,
        Intent::RelativeColorimetric,
        Flags::default() | Flags::BLACKPOINT_COMPENSATION,
    )
    .unwrap();

    let mut samples = Vec::new();
    for r in [0u8, 64, 128, 192, 255] {
        for g in [0u8, 64, 128, 192, 255] {
            for b in [0u8, 64, 128, 192, 255] {
                samples.push([r, g, b]);
            }
        }
    }
    for i in 0..32 {
        let t = ((i as f32 / 31.0) * 255.0) as u8;
        samples.push([t, t, t]);
    }

    let mut max_no = 0i32;
    let mut max_bpc = 0i32;
    let mut sum_no = 0i64;
    let mut sum_bpc = 0i64;
    let mut n = 0i64;
    for rgb in &samples {
        let src_f = [
            rgb[0] as f32 / 255.0,
            rgb[1] as f32 / 255.0,
            rgb[2] as f32 / 255.0,
        ];
        let mut mox = [0f32; 4];
        to_cmyk.transform(&src_f, &mut mox).unwrap();
        let mox_u8 = [
            (mox[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            (mox[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            (mox[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
            (mox[3].clamp(0.0, 1.0) * 255.0 + 0.5) as u8,
        ];
        let mut lcms_no = [0u8; 4];
        let mut lcms_bpc = [0u8; 4];
        x_no.transform_pixels(rgb, &mut lcms_no);
        x_bpc.transform_pixels(rgb, &mut lcms_bpc);
        for i in 0..4 {
            let d_no = (mox_u8[i] as i32 - lcms_no[i] as i32).abs();
            let d_bpc = (mox_u8[i] as i32 - lcms_bpc[i] as i32).abs();
            max_no = max_no.max(d_no);
            max_bpc = max_bpc.max(d_bpc);
            sum_no += d_no as i64;
            sum_bpc += d_bpc as i64;
        }
        n += 4;
    }
    eprintln!(
        "FORWARD Rel no-BPC: max|Δch|={max_no} mean={:.3}",
        sum_no as f64 / n as f64
    );
    eprintln!(
        "FORWARD Rel vs lcms+BPC: max|Δch|={max_bpc} mean={:.3}",
        sum_bpc as f64 / n as f64
    );

    let mut mox_k = [0f32; 4];
    to_cmyk.transform(&[0.0, 0.0, 0.0], &mut mox_k).unwrap();
    let mut ln = [0u8; 4];
    let mut lb = [0u8; 4];
    x_no.transform_pixels(&[0u8, 0, 0], &mut ln);
    x_bpc.transform_pixels(&[0u8, 0, 0], &mut lb);
    eprintln!(
        "black mox={:?} lcms_no={:?} lcms_bpc={:?}",
        [
            (mox_k[0] * 255.0) as u8,
            (mox_k[1] * 255.0) as u8,
            (mox_k[2] * 255.0) as u8,
            (mox_k[3] * 255.0) as u8
        ],
        ln,
        lb
    );

    assert!(
        max_no <= 4,
        "moxcms vs lcms2 forward no-BPC max channel delta {max_no} > 4"
    );
}

/// Probe: XYZ BPC then sRGB→CMYK (no BPC) vs lcms2 Rel+BPC.
#[test]
fn fogra51_app_forward_bpc_vs_lcms() {
    use engine_color::preview_encode::{linear_to_srgb_f32, srgb_f32_to_linear};

    let proof = ColorProfile::new_from_slice(BUILTIN_FOGRA51_ICC).expect("FOGRA51");
    let srgb = ColorProfile::new_srgb();
    let opts = TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        ..Default::default()
    };
    let to_cmyk = srgb
        .create_transform_f32(Layout::Rgb, &proof, Layout::Rgba, opts)
        .unwrap();
    let to_srgb = proof
        .create_transform_f32(Layout::Rgba, &srgb, Layout::Rgb, opts)
        .unwrap();

    // Measure dest BP: full black CMYK → display → XYZ
    let mut black_rgb = [0f32; 3];
    to_srgb
        .transform(&[1.0, 1.0, 1.0, 1.0], &mut black_rgb)
        .unwrap();
    let dst_bp = {
        let r = srgb_f32_to_linear(black_rgb[0].clamp(0., 1.));
        let g = srgb_f32_to_linear(black_rgb[1].clamp(0., 1.));
        let b = srgb_f32_to_linear(black_rgb[2].clamp(0., 1.));
        let x65 = r * 0.4124564 + g * 0.3575761 + b * 0.1804375;
        let y65 = r * 0.2126729 + g * 0.7151522 + b * 0.0721750;
        let z65 = r * 0.0193339 + g * 0.1191920 + b * 0.9503041;
        [
            x65 * 1.0478112 + y65 * 0.0228866 + z65 * -0.0501270,
            x65 * 0.0295424 + y65 * 0.9904844 + z65 * -0.0170491,
            x65 * -0.0092345 + y65 * 0.0150436 + z65 * 0.7521316,
        ]
    };
    let src_bp = [0.0f32; 3];
    let d50 = [0.9642f32, 1.0, 0.8249];

    let srgb_l = Profile::new_srgb();
    let proof_l = Profile::new_icc(BUILTIN_FOGRA51_ICC).unwrap();
    let x_bpc = Transform::new_flags(
        &srgb_l,
        PixelFormat::RGB_8,
        &proof_l,
        PixelFormat::CMYK_8,
        Intent::RelativeColorimetric,
        Flags::default() | Flags::BLACKPOINT_COMPENSATION,
    )
    .unwrap();

    let mut max_d = 0i32;
    let mut sum_d = 0i64;
    let mut n = 0i64;
    for t in 0..=32 {
        let v = ((t as f32 / 32.0) * 255.0) as u8;
        let rgb = [v, v, v];
        // app BPC
        let enc = [v as f32 / 255.0; 3];
        let r = srgb_f32_to_linear(enc[0]);
        let g = srgb_f32_to_linear(enc[1]);
        let b = srgb_f32_to_linear(enc[2]);
        let x65 = r * 0.4124564 + g * 0.3575761 + b * 0.1804375;
        let y65 = r * 0.2126729 + g * 0.7151522 + b * 0.0721750;
        let z65 = r * 0.0193339 + g * 0.1191920 + b * 0.9503041;
        let mut xyz = [
            x65 * 1.0478112 + y65 * 0.0228866 + z65 * -0.0501270,
            x65 * 0.0295424 + y65 * 0.9904844 + z65 * -0.0170491,
            x65 * -0.0092345 + y65 * 0.0150436 + z65 * 0.7521316,
        ];
        for i in 0..3 {
            let denom = d50[i] - src_bp[i];
            if denom.abs() < 1e-10 {
                continue;
            }
            let a = (d50[i] - dst_bp[i]) / denom;
            let bb = dst_bp[i] - a * src_bp[i];
            xyz[i] = a * xyz[i] + bb;
        }
        let x65 = xyz[0] * 0.9555766 + xyz[1] * -0.0230393 + xyz[2] * 0.0631636;
        let y65 = xyz[0] * -0.0282895 + xyz[1] * 1.0099416 + xyz[2] * 0.0210077;
        let z65 = xyz[0] * 0.0122982 + xyz[1] * -0.0204830 + xyz[2] * 1.3299098;
        let rl = x65 * 3.2404542 + y65 * -1.5371385 + z65 * -0.4985314;
        let gl = x65 * -0.9692660 + y65 * 1.8760108 + z65 * 0.0415560;
        let bl = x65 * 0.0556434 + y65 * -0.2040259 + z65 * 1.0572252;
        let src_f = [
            linear_to_srgb_f32(rl).clamp(0., 1.),
            linear_to_srgb_f32(gl).clamp(0., 1.),
            linear_to_srgb_f32(bl).clamp(0., 1.),
        ];
        let mut mox = [0f32; 4];
        to_cmyk.transform(&src_f, &mut mox).unwrap();
        let mox_u8 = [
            (mox[0].clamp(0., 1.) * 255. + 0.5) as u8,
            (mox[1].clamp(0., 1.) * 255. + 0.5) as u8,
            (mox[2].clamp(0., 1.) * 255. + 0.5) as u8,
            (mox[3].clamp(0., 1.) * 255. + 0.5) as u8,
        ];
        let mut lcms = [0u8; 4];
        x_bpc.transform_pixels(&rgb, &mut lcms);
        for i in 0..4 {
            let d = (mox_u8[i] as i32 - lcms[i] as i32).abs();
            max_d = max_d.max(d);
            sum_d += d as i64;
        }
        n += 4;
        if t == 0 || t == 16 || t == 32 {
            eprintln!("gray {v}: app_bpc={mox_u8:?} lcms_bpc={lcms:?}");
        }
    }
    eprintln!(
        "APP forward BPC vs lcms BPC gray ramp: max|Δch|={max_d} mean={:.3}",
        sum_d as f64 / n as f64
    );
}

//! Print-export goldens: lcms2 forward CMYK + pure-K rules + TIFF round identity.

use engine_color::print_export::{rgba8_to_cmyk8, scale_cmyk8_nearest, PrintExportTransform};
use engine_color::soft_proof::{SoftProofIntent, BUILTIN_FOGRA51_ICC};
use engine_io::{write_cmyk_tiff, CmykTiffCompression};
use std::io::Cursor;

#[test]
fn fogra51_black_white_pure_k() {
    let x = PrintExportTransform::new(BUILTIN_FOGRA51_ICC, SoftProofIntent::Relative, true)
        .expect("xform");
    let rgba = vec![0, 0, 0, 255, 255, 255, 255, 255];
    let (cmyk, _, _) = rgba8_to_cmyk8(&rgba, &x, true, None).unwrap();
    assert_eq!(&cmyk[0..4], &[0, 0, 0, 255]);
    assert_eq!(&cmyk[4..8], &[0, 0, 0, 0]);
}

#[test]
fn scale_preserves_blocks() {
    let cmyk = vec![1, 2, 3, 4];
    let out = scale_cmyk8_nearest(&cmyk, 1, 1, 3).unwrap();
    assert_eq!(out.len(), 9 * 4);
    for chunk in out.chunks_exact(4) {
        assert_eq!(chunk, &[1, 2, 3, 4]);
    }
}

#[test]
fn tiff_bytes_deterministic() {
    let x =
        PrintExportTransform::new(BUILTIN_FOGRA51_ICC, SoftProofIntent::Relative, false).unwrap();
    let mut rgba = Vec::new();
    for y in 0..8u8 {
        for xch in 0..8u8 {
            rgba.extend_from_slice(&[xch.wrapping_mul(17), y.wrapping_mul(31), 40, 255]);
        }
    }
    let (cmyk, _, _) = rgba8_to_cmyk8(&rgba, &x, true, None).unwrap();
    let mut a = Cursor::new(Vec::new());
    let mut b = Cursor::new(Vec::new());
    write_cmyk_tiff(
        &mut a,
        8,
        8,
        &cmyk,
        BUILTIN_FOGRA51_ICC,
        300.0,
        CmykTiffCompression::None,
    )
    .unwrap();
    write_cmyk_tiff(
        &mut b,
        8,
        8,
        &cmyk,
        BUILTIN_FOGRA51_ICC,
        300.0,
        CmykTiffCompression::None,
    )
    .unwrap();
    assert_eq!(a.into_inner(), b.into_inner());
}

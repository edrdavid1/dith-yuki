//! Calibration thumbnail: non-square, edge-to-edge, orientation marker.
//! Border is 80px so a 32px thumbnail still has a pure edge pixel (R2).

use dither_thumb::{extract_from_bytes, ThumbKind};
use png::Encoder;
use std::io::Write;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

const W: u32 = 640;
const H: u32 = 400;
const BORDER: u32 = 80;

fn put(px: &mut [u8], x: u32, y: u32, rgba: [u8; 4]) {
    let i = ((y * W + x) * 4) as usize;
    px[i..i + 4].copy_from_slice(&rgba);
}

fn calibration_rgba() -> Vec<u8> {
    let mut px = vec![0u8; (W * H * 4) as usize];
    for y in 0..H {
        for x in 0..W {
            put(&mut px, x, y, [255, 255, 255, 255]);
        }
    }
    for y in 0..BORDER {
        for x in BORDER..W - BORDER {
            put(&mut px, x, y, [255, 0, 0, 255]);
        }
    }
    for y in H - BORDER..H {
        for x in BORDER..W - BORDER {
            put(&mut px, x, y, [0, 0, 255, 255]);
        }
    }
    for y in 0..H {
        for x in 0..BORDER {
            put(&mut px, x, y, [255, 255, 0, 255]);
        }
        for x in W - BORDER..W {
            put(&mut px, x, y, [0, 255, 0, 255]);
        }
    }
    // "F": stem, top bar, middle bar. No bottom bar.
    for y in 100..220 {
        for x in 120..148 {
            put(&mut px, x, y, [0, 0, 0, 255]);
        }
    }
    for y in 100..128 {
        for x in 120..220 {
            put(&mut px, x, y, [0, 0, 0, 255]);
        }
    }
    for y in 150..174 {
        for x in 120..200 {
            put(&mut px, x, y, [0, 0, 0, 255]);
        }
    }
    for y in 120..280 {
        for x in 260..300 {
            put(&mut px, x, y, [0, 0, 0, 0]);
        }
    }
    for y in 150..250 {
        for x in 400..500 {
            put(&mut px, x, y, [0, 0, 255, 128]);
        }
    }
    px
}

fn png_bytes(rgba: &[u8]) -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut enc = Encoder::new(&mut buf, W, H);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(rgba).unwrap();
    }
    buf
}

fn calibration_archive() -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut cursor);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(ThumbKind::Project.mimes()[0]).unwrap();
        zip.start_file("thumbnail.png", stored).unwrap();
        zip.write_all(&png_bytes(&calibration_rgba())).unwrap();
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

fn pixel(bmp: &dither_thumb::ThumbBitmap, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * bmp.width + x) * 4) as usize;
    bmp.rgba[i..i + 4].try_into().unwrap()
}

fn opaque_full(bmp: &dither_thumb::ThumbBitmap) -> bool {
    let mut min_x = bmp.width;
    let mut min_y = bmp.height;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut any = false;
    for y in 0..bmp.height {
        for x in 0..bmp.width {
            if pixel(bmp, x, y)[3] == 255 {
                any = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    any && min_x == 0 && min_y == 0 && max_x + 1 == bmp.width && max_y + 1 == bmp.height
}

fn expected(max_side: u32) -> (u32, u32) {
    let long = W.max(H);
    if long <= max_side {
        return (W, H);
    }
    let scale = max_side as f64 / long as f64;
    let tw = ((W as f64) * scale).round().max(1.0) as u32;
    let th = ((H as f64) * scale).round().max(1.0) as u32;
    (tw, th)
}

#[test]
fn calibration_is_fitted_without_padding_or_flip() {
    let bytes = calibration_archive();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/preview/calibration.dyproj");
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &bytes).unwrap();
    }

    for size in [32u32, 96, 256, 1024] {
        let bmp = extract_from_bytes(&bytes, ThumbKind::Project, size, true).unwrap();
        assert_eq!((bmp.width, bmp.height), expected(size), "size {size}");
        assert!(bmp.width as f64 / bmp.height as f64 > 1.5);
        assert!(opaque_full(&bmp), "padding or corner pin at {size}");
        let top = pixel(&bmp, bmp.width / 2, 0);
        let bottom = pixel(&bmp, bmp.width / 2, bmp.height - 1);
        let left = pixel(&bmp, 0, bmp.height / 2);
        let right = pixel(&bmp, bmp.width - 1, bmp.height / 2);
        assert!(
            top[0] > bottom[0],
            "top should be redder than bottom at {size}"
        );
        assert!(
            bottom[2] > top[2],
            "bottom should be bluer than top at {size}"
        );
        assert!(
            left[0] > right[0],
            "left should be yellower than right at {size}"
        );
        assert!(right[1] >= left[1], "right should stay green at {size}");
    }

    let native = extract_from_bytes(&bytes, ThumbKind::Project, 1024, true).unwrap();
    assert_eq!((native.width, native.height), (W, H));
    assert_eq!(pixel(&native, 320, 4), [255, 0, 0, 255]);
    assert_eq!(pixel(&native, 320, 396), [0, 0, 255, 255]);
    assert_eq!(pixel(&native, 4, 200), [255, 255, 0, 255]);
    assert_eq!(pixel(&native, 636, 200), [0, 255, 0, 255]);
    assert_eq!(pixel(&native, 180, 110), [0, 0, 0, 255], "F top bar");
    assert_eq!(
        pixel(&native, 180, 200),
        [255, 255, 255, 255],
        "F has no bottom bar"
    );
    assert_eq!(pixel(&native, 130, 160), [0, 0, 0, 255], "F stem");
    assert_eq!(
        pixel(&native, 210, 160),
        [255, 255, 255, 255],
        "F is not mirrored"
    );
    assert_eq!(pixel(&native, 280, 200), [0, 0, 0, 0]);
    assert_eq!(pixel(&native, 450, 200), [0, 0, 128, 128], "premultiply");

    let huge = extract_from_bytes(&bytes, ThumbKind::Project, 4096, false).unwrap();
    assert_eq!((huge.width, huge.height), (W, H), "no upscale");
}

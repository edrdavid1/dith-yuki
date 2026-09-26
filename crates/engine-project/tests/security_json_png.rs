//! Security corpus — JSON / PNG / sanitize (SPEC §14.8 subset).

use engine_project::serialize::archive::create_zip;
use engine_project::serialize::{
    decode_png_to_f32_with_limits, open_project_from_bytes, parse_json_value,
    sanitize_display_string, ArchiveLimits, ExpectedKind, PngDecodeLimits, SecureJsonError,
    SecureZipArchive, MAX_JSON_DEPTH,
};
use engine_project::types::DocumentId;
use engine_tiles::TileCache;

#[test]
fn json_duplicate_keys_rejected() {
    let err = parse_json_value(br#"{"a":1,"a":2}"#, MAX_JSON_DEPTH).unwrap_err();
    assert!(matches!(err, SecureJsonError::DuplicateKey(_)));
}

#[test]
fn json_depth_bomb_rejected() {
    let mut s = String::from("1");
    for _ in 0..80 {
        s = format!("[{s}]");
    }
    let err = parse_json_value(s.as_bytes(), MAX_JSON_DEPTH).unwrap_err();
    assert!(matches!(err, SecureJsonError::DepthExceeded(_)));
}

#[test]
fn dyproj_with_duplicate_manifest_key_rejected() {
    // Build a minimal zip whose manifest.json has duplicate keys.
    let zip = create_zip(&[(
        "manifest.json",
        br#"{"format_version":1,"format_version":1,"kind":"dyproj","app_version":"0.2.0","created_at":"x","modified_at":"x"}"#,
    )])
    .unwrap();
    // Secure zip opens; project open fails on strict JSON.
    let mut ar =
        SecureZipArchive::open(&zip, ExpectedKind::Dyproj, ArchiveLimits::dyproj()).unwrap();
    let bytes = ar.read_entry("manifest.json").unwrap();
    assert!(parse_json_value(&bytes, MAX_JSON_DEPTH).is_err());
}

#[test]
fn png_oversized_dimensions_rejected_before_huge_alloc() {
    // Craft IHDR claiming 65535×65535 without a full pixel payload.
    // Signature + IHDR chunk with huge dimensions; decoder must fail via limits
    // or our pixel cap without allocating width*height*4 floats.
    let mut png = Vec::new();
    png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]); // signature
                                                               // IHDR length=13
    png.extend_from_slice(&13u32.to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(&65535u32.to_be_bytes()); // width
    png.extend_from_slice(&65535u32.to_be_bytes()); // height
    png.extend_from_slice(&[8, 2, 0, 0, 0]); // bit depth, RGB, …
                                             // Wrong CRC is fine — we only need failure without panic / huge alloc.
    png.extend_from_slice(&[0, 0, 0, 0]);

    let limits = PngDecodeLimits {
        max_pixels: 1_000_000,
        max_decoder_bytes: 4 * 1024 * 1024,
        ..PngDecodeLimits::default()
    };
    let err = decode_png_to_f32_with_limits(&png, limits).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("PNG") || msg.contains("corrupt") || msg.contains("pixel"),
        "{msg}"
    );
}

#[test]
fn sanitize_strips_bidi_override() {
    let s = sanitize_display_string("ok\u{202E}bad", 64, false);
    assert_eq!(s, "okbad");
}

#[test]
fn golden_still_opens_after_strict_json() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/dyproj/v1/minimal.dyproj");
    let bytes = std::fs::read(path).unwrap();
    let staging = TileCache::new(50_000_000);
    open_project_from_bytes(&bytes, &staging, DocumentId::new(1)).expect("golden open");
}

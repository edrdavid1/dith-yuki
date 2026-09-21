//! Fuzz: secure JSON parse + manifest normalize (SPEC §14.9).

#![no_main]
use engine_project::serialize::{normalize_manifest_value, parse_json_value, MAX_JSON_DEPTH};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    let Ok(value) = parse_json_value(data, MAX_JSON_DEPTH) else {
        return;
    };
    let _ = normalize_manifest_value(value);
});

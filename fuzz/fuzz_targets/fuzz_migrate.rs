//! Fuzz: migrate ladders on arbitrary JSON (SPEC §14.9).

#![no_main]
use engine_project::serialize::{migrate_dyproj, migrate_dyuki, parse_json_value, MAX_JSON_DEPTH};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 || data.len() > 256 * 1024 {
        return;
    }
    let version = data[0] as u32;
    let payload = &data[1..];
    let Ok(value) = parse_json_value(payload, MAX_JSON_DEPTH) else {
        return;
    };
    let _ = migrate_dyproj(version, value.clone());
    let _ = migrate_dyuki(version, value);
});

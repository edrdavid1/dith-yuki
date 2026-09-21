//! Fuzz: open arbitrary bytes as `.dyuki` (SPEC §14.9).

#![no_main]
use engine_project::serialize::unpack_pattern_from_bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 2 * 1024 * 1024 {
        return;
    }
    let _ = unpack_pattern_from_bytes(data, "0.3.0");
});

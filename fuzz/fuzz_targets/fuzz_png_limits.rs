//! Fuzz: PNG decode with resource limits (SPEC §14.9).

#![no_main]
use engine_project::serialize::{decode_png_to_f32_with_limits, threshold_map_png_limits};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 1024 * 1024 {
        return;
    }
    let _ = decode_png_to_f32_with_limits(data, threshold_map_png_limits());
});

//! Fuzz: journal `.state` decoder must not panic on corrupt / truncated input.

#![no_main]
use engine_project::serialize::decode_journal_state;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() > 256 * 1024 {
        return;
    }
    let _ = decode_journal_state(data);
});

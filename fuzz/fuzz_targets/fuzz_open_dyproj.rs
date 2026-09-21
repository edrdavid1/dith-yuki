//! Fuzz: open arbitrary bytes as `.dyproj` (SPEC §14.9).
//! Must not panic; Err is fine.

#![no_main]
use engine_project::serialize::open_project_from_bytes;
use engine_project::types::DocumentId;
use engine_tiles::TileCache;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Cap input so the fuzzer stays inside ArchiveLimits without spending
    // all time on ArchiveTooLarge early rejects.
    if data.len() > 4 * 1024 * 1024 {
        return;
    }
    let cache = TileCache::new(8 * 1024 * 1024);
    let _ = open_project_from_bytes(data, &cache, DocumentId::new(1));
});

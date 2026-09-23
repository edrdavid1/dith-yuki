//! Registry + full-document tests for `riemersma`.

#[path = "support/parity_harness.rs"]
mod parity_harness;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use engine_project::algorithms::builtin_registry;
use engine_project::filter::{DitherModeV2, DitherParamsV2, FilterParams};
use engine_project::filters::riemersma::{apply_riemersma_rgba, RiemersmaError};
use engine_registry::{CpuCheckpointKind, ExecutionScope, GpuEligibility};
use parity_harness::{dither_params, Env};

#[test]
fn registry_riemersma_scope_and_gpu() {
    let env = Env::new();
    let params = dither_params(DitherModeV2::Riemersma);
    env.assert_gpu(
        "riemersma",
        &FilterParams::DitherV2(params),
        GpuEligibility::Cpu(CpuCheckpointKind::SequentialGlobalDependency),
    );
    let algo = builtin_registry().get_by_str("riemersma").expect("registered");
    assert_eq!(algo.execution_scope(), ExecutionScope::FullDocument);
    assert!(!algo.requires_full_row());
}

#[test]
fn riemersma_cancel_does_not_finish_large_pass() {
    let w = 1024u32;
    let h = 1024u32;
    let mut rgba = vec![0.5f32; (w * h * 4) as usize];
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_flag = Arc::clone(&cancel);
    let handle = thread::spawn(move || {
        thread::sleep(Duration::from_millis(5));
        cancel_flag.store(true, Ordering::Relaxed);
    });
    let params = DitherParamsV2 {
        mode: DitherModeV2::Riemersma,
        levels: 2,
        ..DitherParamsV2::default()
    };
    let err = apply_riemersma_rgba(&mut rgba, w, h, &params, &cancel).unwrap_err();
    assert_eq!(err, RiemersmaError::Cancelled);
    handle.join().unwrap();
}

#[test]
fn riemersma_odd_sizes_produce_valid_levels() {
    for &(w, h) in &[(3u32, 5), (7, 2), (1, 1), (64, 33)] {
        let mut rgba = vec![0.42f32; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            rgba[i * 4 + 3] = 1.0;
        }
        let cancel = AtomicBool::new(false);
        let params = DitherParamsV2 {
            mode: DitherModeV2::Riemersma,
            levels: 4,
            ..DitherParamsV2::default()
        };
        apply_riemersma_rgba(&mut rgba, w, h, &params, &cancel).unwrap();
        let levels = 4.0f32;
        for i in 0..(w * h) as usize {
            let v = rgba[i * 4];
            let k = v * (levels - 1.0);
            assert!(
                (k - k.round()).abs() < 1e-4,
                "{w}x{h} pixel {i} not on grid: {v}"
            );
        }
    }
}

#[test]
fn riemersma_perf_smoke_1k() {
    let w = 1024u32;
    let h = 1024u32;
    let mut rgba = vec![0.5f32; (w * h * 4) as usize];
    let cancel = AtomicBool::new(false);
    let params = DitherParamsV2 {
        mode: DitherModeV2::Riemersma,
        levels: 2,
        ..DitherParamsV2::default()
    };
    let start = std::time::Instant::now();
    apply_riemersma_rgba(&mut rgba, w, h, &params, &cancel).unwrap();
    let ms = start.elapsed().as_millis();
    // Honest smoke bound — not a regression gate for CI flakiness; just ensure
    // the pass finishes in a human-scale window on debug builds.
    eprintln!("riemersma 1024×1024 took {ms}ms");
    assert!(ms < 30_000, "unexpectedly slow: {ms}ms");
}

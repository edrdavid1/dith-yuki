//! T8: Floyd–Steinberg GPU prototype vs CPU.

#[test]
#[ignore = "requires GPU adapter; cargo run -p engine-gpu --example ed_gpu_prototype --release"]
fn ed_serial_gpu_matches_cpu_fs() {
    let r = engine_gpu::ed_prototype::run_ed_serial_prototype().expect("GPU adapter");
    println!(
        "ED serial n={} cpu={:.3}ms gpu={:.3}ms max_diff={:.6} mismatches={}",
        r.n, r.cpu_ms, r.gpu_ms, r.max_abs_diff, r.mismatches
    );
    assert_eq!(r.mismatches, 0, "serial GPU FS must match CPU (max_diff={})", r.max_abs_diff);
}

#[test]
#[ignore = "requires GPU adapter"]
fn ed_naive_parallel_diverges_as_expected() {
    let r = engine_gpu::ed_prototype::run_ed_parallel_prototype().expect("GPU adapter");
    println!(
        "ED parallel-naive n={} mismatches={} max_diff={:.6}",
        r.n, r.mismatches, r.max_abs_diff
    );
    assert!(
        r.mismatches > 0,
        "naive anti-diagonal FS must diverge (same-diagonal race on (−1,+1))"
    );
}

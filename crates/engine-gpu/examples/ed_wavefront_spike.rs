//! §0.3 spike runner: exact wavefront FS (`t = x + 2y`), fixed-point, tiled.
//!
//! ```bash
//! cargo run -p engine-gpu --example ed_wavefront_spike --release
//! ```

fn main() {
    println!("=== ED wavefront spike (t = x + 2y, Q8.8, tiled) ===\n");
    println!("Acceptance:");
    println!("  - 4K: bit-identical to CPU reference");
    println!("  - 16K: ≥ 2× vs CPU including upload + readback\n");

    match engine_gpu::ed_wavefront_spike::run_spike() {
        None => {
            println!("No GPU adapter — spike inconclusive.");
            std::process::exit(2);
        }
        Some(results) => {
            let mut parity_4k_ok = false;
            let mut speedup_16k_ok = false;
            let mut any_mismatch = false;

            for r in &results {
                println!(
                    "[{label}] n={n}  CPU={cpu:.1}ms  GPU={gpu:.1}ms  speedup={sp:.2}x  mismatches={mis}  backend={be}",
                    label = r.label,
                    n = r.n,
                    cpu = r.cpu_ms,
                    gpu = r.gpu_ms,
                    sp = r.speedup,
                    mis = r.mismatches,
                    be = r.backend,
                );
                if r.mismatches > 0 {
                    any_mismatch = true;
                }
                if r.n == 4096 && r.mismatches == 0 {
                    parity_4k_ok = true;
                }
                if r.n >= 16384 && r.mismatches == 0 && r.speedup >= 2.0 {
                    speedup_16k_ok = true;
                }
            }

            println!();
            if any_mismatch {
                println!("FAIL: GPU output not bit-identical to CPU reference.");
            } else {
                println!("Parity: OK on all completed sizes.");
            }
            println!(
                "4K bit-identical: {}",
                if parity_4k_ok { "PASS" } else { "FAIL" }
            );
            println!(
                "16K ≥2× speedup: {}",
                if speedup_16k_ok {
                    "PASS"
                } else {
                    "FAIL (or 16K not run)"
                }
            );

            if parity_4k_ok && speedup_16k_ok {
                println!(
                    "\nSPIKE PASSED — exact GPU ED is viable; \
                     blocked_fs approximate family is SUPERSEDED. Stop and report."
                );
                std::process::exit(0);
            } else {
                println!(
                    "\nSPIKE FAILED — proceed with approximate blocked_fs + blue_noise \
                     per ED-on-GPU.md."
                );
                std::process::exit(1);
            }
        }
    }
}

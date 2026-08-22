//! Off-product Floyd–Steinberg GPU prototype (Path B T8).
//!
//! ```bash
//! cargo run -p engine-gpu --example ed_gpu_prototype --release
//! ```

fn main() {
    println!("=== ED GPU prototype (Floyd–Steinberg) ===\n");
    match engine_gpu::ed_prototype::run_ed_prototype() {
        Some((serial, parallel)) => {
            for r in [&serial, &parallel] {
                println!(
                    "[{mode}] n={n}  CPU={cpu:.3}ms  GPU={gpu:.3}ms  ratio={ratio:.2}x  max_diff={max:.6}  mismatches={mis}",
                    mode = r.mode,
                    n = r.n,
                    cpu = r.cpu_ms,
                    gpu = r.gpu_ms,
                    ratio = r.gpu_ms / r.cpu_ms.max(1e-9),
                    max = r.max_abs_diff,
                    mis = r.mismatches,
                );
            }
            println!();
            if serial.mismatches == 0 {
                println!("Serial GPU: parity OK (math correct).");
            } else {
                println!("Serial GPU: parity FAILED.");
            }
            if parallel.mismatches > 0 {
                println!(
                    "Naive parallel anti-diagonal: DIVERGES (FS weight (−1,+1) shares diagonal) — expected."
                );
            }
            println!(
                "\nPRODUCT DECISION: keep CpuCheckpoint(ErrorDiffusion) permanently.\n\
                 Reasons: parallel FS not viable without a different algorithm; serial GPU is slower\n\
                 than CPU on 128²; product needs tiles + cross-tile residuals + RGB + halo."
            );
        }
        None => println!("No GPU adapter — skip."),
    }
}

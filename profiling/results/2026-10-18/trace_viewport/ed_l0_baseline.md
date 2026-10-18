# ED L0 pass baseline (Step 1)

Mode: Floyd–Steinberg, pixel_size=1, levels=16.
Cold pass: Processed/Composite evicted before apply; full-document L0 prefix scheduled.
Metric: wall ms from `apply_threshold` until all L0 Processed tiles are fresh
(scheduler + ed_frontier idle).

Harness: `cargo test -p dither --release ed_l0_pass_baseline_three_sizes -- --ignored --nocapture --test-threads=1`
Date: 2026-10-18

| size | tiles_l0 | wall_ms | ok |
| --- | ---: | ---: | --- |
| 748x1024 | 12 | 90.3 | yes |
| 1920x1080 | 40 | 259.2 | yes |
| 4000x3000 | 192 | 1655.1 | yes |

`PARALLEL_ED_PASSES` here is the count of ED layers with unfinished L0 Processed (one layer → 0 or 1).

Events: `profiling/results/2026-10-18/trace_viewport/events_ed_l0_baseline.jsonl`

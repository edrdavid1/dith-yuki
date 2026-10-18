# ED L0 pass baseline (Step 1)

Mode: Floyd–Steinberg, pixel_size=1, levels=16.
Cold pass: Processed/Composite evicted before apply; full-document L0 prefix scheduled.
Metric: wall ms from `apply_threshold` until all L0 Processed tiles are fresh
(scheduler + ed_frontier idle).

| size | tiles_l0 | wall_ms_release | wall_ms_dev | ok |
| --- | ---: | ---: | ---: | --- |
| 748x1024 | 12 | 90.3 | 91.8 | yes |
| 1920x1080 | 40 | 259.2 | 394.9 | yes |
| 4000x3000 | 192 | 1655.1 | 3201.9 | yes |

`PARALLEL_ED_PASSES` here is the count of ED layers with unfinished L0 Processed (one layer → 0 or 1).

Last profile run: `dev`. Merge file: `/Users/dave/Prahramavannie/dither-yuki-2/src-tauri/../profiling/results/2026-10-18/trace_viewport/ed_l0_baseline.json`

Events: `/Users/dave/Prahramavannie/dither-yuki-2/src-tauri/../profiling/results/2026-10-18/trace_viewport/events_ed_l0_baseline.jsonl`

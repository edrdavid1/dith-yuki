# Check (no behavior change): schedule #1 vs #2 on real 6K CPU

Source: `profiling/results/2026-10-22/trace_viewport/events.jsonl`  
Session: fit view L3 zoom≈0.120, then one `update_filter` (ps=5, Floyd–Steinberg).  
Note: this capture has **open + one update**, not add→update pair. Zoom×2 consecutive updates (#1/#2) included for schedule symmetry.

## Verdict

`schedule_enter` / `schedule_result` **do not** show a larger visible enqueue on the slow interaction. Visible work is flat at **9**. The slow path is **ED L0 prefix** (~408 tiles) counted in `refresh_started.enqueued_delta` (411), which holds `fullDocumentBusy` ~3.1s while eyes already see L3 paint ~200ms.

## schedule_enter / schedule_result

| wave | vis | level | reason | schedule_result.enqueued | refresh enqueued_delta | busy hold |
|------|----:|------:|--------|-------------------------:|-----------------------:|----------:|
| zoom2 update #1 | 9 | 0 | param_change | 9 | 81 | 613 ms |
| zoom2 update #2 | 9 | 0 | param_change | 9 | 96 | 508 ms |
| 6K open (fit) | 9 | 3 | unspecified | 9 | — (no busy) | — |
| 6K update #1 | 9 | 3 | param_change | 9 | **411** | **3161 ms** |

- Visible `enqueued` is **not** higher on the slow wave (still 9).
- Extra queue ≈ `411 − 9 = 402` ≈ `ed_pass_start.total_l0_processed = 408`.

## Outside-viewport work (6K update)

During busy:

| signal | value |
|--------|------:|
| `ed_pass_*` L0 | 408 tiles, **3128 ms** |
| `worker_dequeue` by level | L0:7513 L1:808 L2:237 L3:72 |
| `composite_start` | 490, **all level 0** |
| `tile_enqueued` (visible set) | 9 × L3 |
| `tile_ready_emitted` | 17 × L3, all inside bbox (0,0)–(2,2) |

Tasks **are** running outside the viewport pyramid level (L0 ED / Processed+Composite). Visible L3 readiness is not what keeps busy up.

## Eye vs Rendering… (6K update)

| metric | ms after busy_on |
|--------|-----------------:|
| first `tile_drawn` (L3) | **201** |
| first `tile_ready_emitted` | 2316 |
| `ed_pass_complete` | 3128 |
| `busy_changed` false | **3161** |

Busy ≈ ED wall. FE `Rendering…` tracks busy until wave idle, not first visible paint.

## Harness: 6k CPU `fs_ps_alt` (#1+#2)

`DITHER_SECOND_IX_SIZE=6k DITHER_GPU_PATH=cpu DITHER_SECOND_IX_ONLY=fs_ps_alt DITHER_SECOND_IX_MAX_IX=2`  
Artifacts: `profiling/results/2026-10-22/h_6k_cpu_fs_ps_alt/`

| ix | kind | first_vis_ms | queued_after | vis_count | rendering? |
|---:|------|-------------:|-------------:|----------:|------------|
| 1 | add ps=1 | **219** | 118 | 24 | false |
| 2 | update ps=8 | **472** | 149 | 24 | **true** |

Ignore `wall_ms` (~30s): capped wait after first paint; not eye latency.  
`all_visible_ms` never hit in 30s (1/24 fresh at end) while `quiet_ok` — ED/background work ≠ first paint.

Earlier long run (600s caps) also saw `#2` first_vis **775** vs `#1` **214**, queued **408** vs **230** (closer to live `enqueued_delta=411`).

## Conclusion

- Not “more visible tiles on #2” in `schedule_result` (live: 9=9).
- Yes: large **non-viewport** queue (ED L0 / `enqueued_delta≈411`) and busy held for that wave.
- Eye latency ≈ first visible tile (**~0.2–0.5s**); `Rendering…` ≈ busy until ED idle (**~3s** live).

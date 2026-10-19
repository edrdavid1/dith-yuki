# Step B1 — ED strip owner (≤1 calc per `(layer, ty)`)

**Branch:** `perf/cpu-fs-ed-pass`  
**Build:** release  
**Invariant:** bitwise ED unchanged (`ed_wavefront_matches_row_major_reference` green); no file-format change  
**Diag:** `DITHER_FS_PROF` / `DITHER_TRACE` → `fs_ed_prof` counters  

## Design decisions

| Item | Choice |
| --- | --- |
| **Owner** | First thread to acquire `TileState::ed_strip_gates[(doc, layer, level, ty)]` after observing the row is not fully fresh. Causal schedule usually makes **`min_x`** the first claimant. |
| Non-owners | Block on the same gate; after release, return via **`tile_fresh` + cache get** (no strip recompute). |
| **doc_gen** | Checked after prev-row ensure / before strip apply, and again before insert (stale → no publish). |
| **Batch wake** | Insert all row tiles first, then `wake_ed_frontier_after_strip_batch` (deduped ready tasks). |

## Before / after — 5984×4172 harness (8 workers)

| | Step A (before) | B1 (after) | Δ |
| --- | ---: | ---: | ---: |
| wall | 3.24 s | **0.711 s** | **4.6×** |
| wall MPix/s | 7.7 | **35.1** | **4.6×** |
| strip_runs / line | 21.82 | **1.00** | target met |
| recalc_extra | 354 | **0** | |
| strip_pixels | 583.5 MPix | **26.7 MPix** | ~1× doc |
| bare cycle (criterion) | 38.1 MPix/s | (unchanged) | wall now ≈ bare |

Artifacts: `harness_5984x4172.log` (A), `harness_b1_5984x4172.log` (B1).

## ed_l0_baseline (three sizes, B1)

| Size | wall_ms | MPix/s | strip_runs/line |
| --- | ---: | ---: | ---: |
| 748×1024 | 19.5 | 39.2 | 1.00 |
| 1920×1080 | 66.2 | 31.3 | 1.00 |
| 4000×3000 | 312.6 | 38.4 | 1.00 |

Artifact: `ed_l0_baseline_b1.log`.

## Parity / golden

- `ed_wavefront_matches_row_major_reference` — **ok** (bitwise vs sequential row-major)
- `ed_strip_owner_one_calc_per_ty` — **ok** (`strip_runs == unique_ty`, `recalc_extra == 0`)

## GUI Z3 ≥12 MPix (expected; FE confirm)

| | Step A GUI (prior) | B1 expected (release) |
| --- | ---: | ---: |
| Build | `tauri:dev:release` | same |
| Doc | ~5984×4172 (408 L0) | same |
| T0→T1 (L0 ED) | ~13–17 s | **~0.7–1.5 s** (harness L0 wall 0.71 s + IPC/quiet) |
| strip_runs / line | ~22 | **1.0** (`DITHER_FS_PROF=1` or `DITHER_TRACE=1`) |

Please run Stress Z3 on release with `DITHER_FS_PROF=1` and record T0→T1 + `strip_runs/line` from the report / log.

## Reproduce

```bash
cargo test -p dither --release ed_wavefront_matches_row_major -- --test-threads=1
cargo test -p dither --release ed_strip_owner_one_calc_per_ty -- --test-threads=1
DITHER_FS_PROF=1 cargo test -p dither --release fs_ed_l0_5984x4172 -- --ignored --nocapture --test-threads=1
DITHER_FS_PROF=1 cargo test -p dither --release ed_l0_pass_baseline_three_sizes -- --ignored --nocapture --test-threads=1
```

**Stopped before B2** (DashMap / buffer pool) — next only by profile after this confirmation.
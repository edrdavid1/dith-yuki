# Step A — CPU Floyd–Steinberg pass (measurements only)

**Branch:** `perf/cpu-fs-ed-pass` (from `origin/main`, separate from `fix/h8`)  
**Date:** 2026-10-19  
**Build:** release / bench; **no** `DITHER_TRACE` for Instruments/`sample`  
**Counters:** `DITHER_FS_PROF=1` (also gated by `DITHER_TRACE`)

No optimizations applied. Step B (DashMap / buffer pool / wavefront) waits for confirmation.

---

## 1. Bare stripe cycle (criterion, no caches)

```text
cargo bench -p engine-project --bench fs_ed_stripe_bench
```

| Metric | Value |
| --- | ---: |
| Geometry | 24 tiles × 256 = **6144** px wide strip (covers **5984**-wide doc) |
| Pixels / strip | 1 572 864 (1.57 MPix) |
| Median wall | **41.3 ms** (criterion mean ~40.4 ms) |
| **ns/pixel** | **26.24** |
| **Bare cycle MPix/s** | **38.11** |

Fresh `ErrorResidualsStore` + `BlockRepresentativeCache` each iteration (no cache hits). Artifact: `criterion_stripe.log`.

---

## 2. Full-doc harness — 5984×4172 release

```text
DITHER_FS_PROF=1 cargo test -p dither --release fs_ed_l0_5984x4172 \
  -- --ignored --nocapture --test-threads=1
```

| Metric | Value |
| --- | ---: |
| L0 grid | **24×17 = 408** tiles (~24.97 MPix) |
| Workers | 8 |
| Wall (one cold pass) | **3.24 s** → **7.7 wall MPix/s** |
| `processed_calls` | ~370 |
| Fresh | 408/408 |

### Counters (`fs_ed_prof`) — % of **timed CPU** (sum across workers)

| Phase | ms | % of timed |
| --- | ---: | ---: |
| **ed_cycle** | 17722 | **73.2%** |
| **raw_gather** (DashMap get + pixel copy into strip) | 3453 | **14.3%** |
| **residual_store** (writeback + residual DashMap) | 2801 | **11.6%** |
| wake | 198 | 0.8% |
| residual_load | 17 | 0.1% |
| cache_write (`insert_fresh_gen`) | 12 | 0.0% |
| prev_row_ensure (freshness probe only) | 0.7 | 0.0% |
| **timed_total** | **24204** | 100% |

Timed total ≫ wall because scopes accumulate on **8 parallel workers**.

### Recalc / tasks per line

| Counter | Value |
| --- | ---: |
| `strip_runs` | 371 |
| `unique_ty` | 17 |
| `recalc_extra` | 354 |
| **strip_runs / line** | **21.82** (ideal = 1) |
| **tile_tasks / line** | **21.82** |
| strip_pixels accounted | **583.5 MPix** (~23× document pixels) |

Every scheduled tile in a row re-runs the **entire** horizontal strip → ~22× redundant ED work. That dominates the gap vs bare 38 MPix/s.

Artifact: `harness_5984x4172.log`.

---

## 3. `sample` 12 s during apply (release, no tracing)

```text
DITHER_FS_PROF_LOOPS=6 cargo test -p dither --release fs_ed_l0_5984x4172 -- --ignored ...
sample <pid> 12 -file sample_12s.txt
```

| Item | Observation |
| --- | --- |
| Process footprint | ~1.9 GB peak |
| Worker threads | 8 active drain threads (~2.3–2.5k samples each over 12 s @ 1 ms) |
| Core load | Busy workers stay hot while a strip runs; main/test threads mostly parked. Roughly **~0.6 of an 8-core budget** averaged over the 12 s window (includes inter-loop invalidate gaps). |

### Top of useful stacks (under `compute_processed_tile`)

From call graph (one worker, illustrative):

- `compute_processed_tile` → `apply_filter_stack_tile_row_strip` → **`apply_error_diffusion_tile_row_strip`** (~1620 / 2363 samples on that path)
- Inside strip: hot self + **`quantize_ed_rgb`**
- Also visible: `PixelTile::copy_halo_from`, large **`malloc`** / `memmove` for strip buffers, **`DashMap`/`hashbrown`** on tile-cache get / frontier

“Total number in stack” (collapsed ≥5) highlights: `tile_fresh`, strip entry, malloc, DashMap `__get`, hasher write, `quantize_ed_rgb`, `PixelTile::new`, frontier wake/block — consistent with counters (ED + gather/alloc + DashMap), not a separate mystery hot path.

Artifact: `sample_12s.txt`.

---

## 4. Where time goes (verdict)

1. **Bare FS pixel cycle ≈ 38 MPix/s** (26 ns/px) on one 5984-wide stripe with empty caches.  
2. **Pipeline waste ≈ 22× strip recalculation per ty** → ~580 MPix of strip work for a 25 MPix doc; timed CPU is **~73% ed_cycle**, **~14% raw gather**, **~12% residual/store writeback**.  
3. **DashMap / alloc / wake** show up in `sample` but are secondary in timed % (cache_write + wake < 1% combined; raw gather includes cache reads + copies).  
4. Wall ~3.2 s @ 8 workers still leaves large headroom once **one strip per line** (and buffer reuse) lands in Step B.

---

## How to reproduce

```bash
# Bare cycle
cargo bench -p engine-project --bench fs_ed_stripe_bench -- --sample-size 12

# Counters + harness
DITHER_FS_PROF=1 cargo test -p dither --release fs_ed_l0_5984x4172 \
  -- --ignored --nocapture --test-threads=1

# sample during apply (no trace)
DITHER_FS_PROF_LOOPS=6 cargo test -p dither --release fs_ed_l0_5984x4172 \
  -- --ignored --nocapture --test-threads=1 &
# after "scheduled_tasks=" appears:
sample <dither-test-pid> 12 -file profiling/results/2026-10-19/fs_ed/sample_12s.txt
```

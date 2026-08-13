# Implementation Plan: Track A — Correctness Debt

План закрывает A1 (error diffusion seams) и A2 (`pixel_size` / block cache) из [tech-debit.md](../tech-debit.md). Спека: [requirements.md](./requirements.md), [design.md](./design.md). Предшествующий черновик silent-skip: [TASK_global_coords.md](../../TASK_global_coords.md).

A1 и A2 можно вести параллельно после §0; §1 (diagnosis) блокирует только реализацию waiters (§2), не diagonal и не levels.

---

## 0. Baseline

- [x] 0.1 Inventory current green/red
  - Run `dither_seam_matrix`, diffusion/ordered prop tests, `coords` unit tests
  - Note which `pixel_size` × mode cells already fail — paste table into PR notes
  - Confirm `BlockRepresentativeCache` is already in AppState and apply path
  - _Requirements: 5, 6, 8_
  - **Baseline 2026-08-11:** seam matrix all `ps ∈ {1..32} × {Bayer, FS}` clean (`c0/u0`); `coords` 17 ok; BRC in AppState + apply path.

- [x] 0.2 Link docs
  - Point this folder from `tech-debit.md` Track A (one-line link)
  - _Requirements: 9_

---

## 1. A1.1 — Diagnose silent-skip (gate for §2)

- [x] 1.1 Add skip counter
  - In `src-tauri/src/tile_pipeline.rs`, next to left/top `get_entry(*_raw_key).is_some()` else-paths, increment `AtomicU64` (AppState or lazy static for diagnosis)
  - Optional: `tracing::debug` with tile keys
  - _Requirements: 1.1_

- [x] 1.2 Reproduce and record
  - Scenario A: full 1:1 load → then enable FS — seam? counter?
  - Scenario B: pan under active FS — counter? sticky seam after settle?
  - Write outcome in PR description / comment block in tasks (§1.2 result)
  - Decision: implement §2 **or** mark §2 N/A
  - _Requirements: 1.2, 1.3, 1.4, 1.5_

**§1.2 result (fill in):**

```
Date: 2026-08-11
Scenario A seam / counter: Code-path analysis + prior TASK_global_coords diagnosis —
  decompose inserts all level-0 raws; production never calls evict_* today;
  load_raw cannot resurrect missing raw → skip branch unreachable after full load.
  Remaining 1:1 seam root cause → Diagonal_Error_Loss (A1.4), not silent-skip.
Scenario B seam / counter: Same — without eviction, pan cannot leave missing level-0
  raw for in-document tiles. Counter wired for future if eviction is enabled.
Decision: N/A for waiters-as-sole-fix; helpers + register/wake wired lightly
  so contract stays live if skip ever fires. Primary A1 fix = IncomingErrorBuffer.
```

**Track N follow-up (2026-08-13):** `TileCache::evict_layer` is now called from Orphan_GC.
Whole-layer eviction of the *current* layer errors on missing current raw (skip counter
stays 0). The skip branch **is** reachable when a *neighbor* raw is absent while the
current raw remains (lab: `skip_branch_increments_when_neighbor_raw_missing`). That
pattern is LRU / partial-raw loss, not Orphan_GC of an unreferenced `LayerId`.
Waiters stay as previously wired; no user-visible seam from GC itself → no waiter
reimplementation (Track N Req 8).

---

## 2. A1.2 — Pending diffusion waiters (conditional)

Skip implementation of production wiring if §1.2 = N/A; still do 2.4 contract test if feasible.

- [x] 2.1 Add `pending_diffusion_waiters` to pipeline state
  - `DashMap<TileKey, Vec<TileKey>>` beside `error_residuals`
  - Helpers: `register_diffusion_waiter(missing_raw, waiter_processed)`, `wake_diffusion_waiters(loaded_raw) -> Vec<TileKey>`
  - _Requirements: 2.1_

- [x] 2.2 Register on silent skip
  - Else-branch of left/top raw missing: register current Processed key
  - Still allow zero-seed compute for this pass
  - _Requirements: 2.2_

- [x] 2.3 Wake on raw insert
  - After raw tile inserted into `tile_cache`, wake waiters → mark dirty + schedule via existing invalidation
  - Multi-neighbor: recompute on first wake OK
  - _Requirements: 2.3, 2.4_

- [x] 2.4 Tests
  - Unit: register → insert raw → waiter dirty
  - Integration: 2×2 with delayed raw matches eager-raw result (within seam tol)
  - If N/A: keep unit test on helpers; document production path omitted
  - _Requirements: 2.5, 2.6_
  - Unit tests in `src-tauri/src/diffusion_waiters.rs`; register+wake also wired in pipeline/worker.

---

## 3. A1.3 — Enforcement on all pyramid levels

- [x] 3.1 Remove `level == 0` gate
  - `tile_pipeline.rs`: `if has_error_diffusion {` only
  - Verify left/top keys preserve `level`
  - _Requirements: 3.1, 3.2, 3.3_

- [x] 3.2 Level > 0 seam test
  - Extend seam / gradient test for FS (and Atkinson) at `level >= 1`
  - _Requirements: 3.4, 5.2_

- [x] 3.3 Docs
  - Update `TILE_PIPELINE.md` §4.2 (drop “only level 0”)
  - _Requirements: 3.5, 9.1_

---

## 4. A1.4 — Diagonal error (IncomingErrorBuffer)

Largest A1 slice — schedule most time here.

- [x] 4.1 Extend `ErrorResiduals`
  - Add `corner: Vec<f32>` (sized for FS 1×1 / Atkinson up to 2×2 past both edges)
  - Update `new`, `store`, seed helpers, existing residual unit tests
  - _Requirements: 4.1, 4.2_

- [x] 4.2 Capture diagonal in distribute_*
  - In `distribute_fs` / `distribute_atkinson`, when `nx >= SIZE && ny >= SIZE` (and Atkinson out-of-both-edges), accumulate into `corner` instead of discard
  - Remove “negligible” discard comment
  - _Requirements: 4.1_

- [x] 4.3 Seed diagonal on apply
  - `get_diag(layer, coord)` from `(x-1,y-1)` → add into top-left of `error_buf`
  - Wire in `apply_error_diffusion_with_cache`
  - _Requirements: 4.2_

- [x] 4.4 Enforce diagonal neighbor in pipeline
  - When `requires_full_row` and `x>0 && y>0`, ensure Processed `(x-1,y-1)` before current (same raw-present / waiter pattern as left/top)
  - _Requirements: 4.2_

- [x] 4.5 Reference + seam tests
  - Small full-buffer FS vs tiled 2×2 — no systematic boundary darkening
  - Gradient seam matrix FS/Atkinson clean
  - _Requirements: 4.3, 4.4, 5.1, 5.3_

- [x] 4.6 Docs
  - Document corner channel + diag recurse in `TILE_PIPELINE.md` §4 / §6
  - _Requirements: 9.2_

---

## 5. A2 — Block representative + pixel_size matrix

Can start in parallel with §1–§3.

- [x] 5.1 Audit apply paths for halo-clamp reps
  - Grep diffusion/ordered for manual `HALO` / `tile_x +` block origin
  - Replace with `GlobalCoordSigned::from_local_with_halo(...).aligned(ps)`
  - _Requirements: 6.4, 7.1, 7.2_
  - Hot path uses `GlobalCoordSigned` + BRC; clamp remains legacy fallback only when cache empty.

- [x] 5.2 Complete dithered side-channel
  - Producer inserts dithered RGB at representative; consumers in other tiles `get_dithered` when rep outside core
  - Invalidation: `clear_dithered` with residual clear; `invalidate_all` on raw change
  - _Requirements: 6.1, 6.2, 6.3, 6.5_

- [x] 5.3 Populate guarantees
  - Confirm `ensure_populated_from_tiles` / `populate_from_buffer` before apply when `ps > 1`
  - Fix any path that still samples rep via clamped local coords
  - _Requirements: 6.1, 6.2_

- [x] 5.4 Full seam matrix gate
  - `dither_seam_matrix.rs`: all `PIXEL_SIZES × {Bayer, FS}` clean
  - Add/keep Atkinson coverage if not already in matrix
  - _Requirements: 6.6, 5.1, 7.3_

---

## 6. Preservation + Definition of Done

- [x] 6.1 Regression suite
  - `dither_determinism_props`, alpha/palette/validation, ordered `pixel_size==1` baseline
  - Non-`requires_full_row` layers never touch waiters/enforcement extras
  - _Requirements: 8.1–8.4_

- [ ] 6.2 Manual QA checklist
  - [ ] 1:1 FS after full load — no seam
  - [ ] Zoom out (pyramid > 0) — no seam
  - [ ] Pan sticky-seam scenario — clears after settle **or** N/A documented
  - [ ] `pixel_size` 3, 5, 7, 12 — Bayer + FS blocks continuous across tile edge
  - [ ] Bayer-only doc unchanged visually vs pre-change smoke

- [x] 6.3 Close Track A in roadmap
  - Mark A1/A2 criteria satisfied in `tech-debit.md` when §4.5 + §5.4 green
  - _Requirements: 5.4, 9.3_

---

## Definition of Done

- Diagnosis recorded; waiters shipped **or** N/A with contract test
- Dependency enforcement on **all** pyramid levels
- Diagonal FS/Atkinson error preserved via IncomingErrorBuffer (+ diag neighbor ensure)
- `dither_seam_matrix` clean for Bayer×FS× listed `pixel_size` values
- Block reps without halo-clamp; FS uses `GlobalCoordSigned` for block logic
- Preservation tests green; `TILE_PIPELINE.md` matches code
- Track C unblocked from correctness perspective

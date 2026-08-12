# Implementation Plan: Track D — GPU Pipeline

План закрывает GPU pipeline из [tech-debit.md](../tech-debit.md). Спека: [requirements.md](./requirements.md), [design.md](./design.md).

**Gate:** precondition checklist в design (реальные `cargo test`, не «файлы есть»); ED остаётся CPU-only.

**Locked (design):** RGBA32 float I/O; Bayer parity exact; Halftone/CRT ≤ 1/255; `map_timeout_counter`; D2/D3 только после пяти D1 exit criteria.

**Порядок:** precondition → D0 → D1 Bayer pilot (все 5 exit) → D2 Halftone ∥ D3 CRT → (optional Glow) → docs/DoD.

---

## 0. Baseline

- [x] 0.1 Run precondition checklist (design.md)
  - Actually execute Track A / C seam tests; do not rely on markdown status alone
  - Commands (adjust names if renamed): `dither_seam_matrix`, `phase1_pattern_seam` / `cmyk_halftone_2x2`, `crt_seamless`
  - Note date + pass/fail in §0.1 result; **proceed** only if all pass
  - _Requirements: 1_

- [x] 0.2 Inventory apply / AppState extension points
  - `compute_processed_tile` / `filters/apply.rs` / ordered + crt dispatch
  - `AppState` in `commands.rs` / `main.rs` / `worker.rs`
  - Confirm locked format RGBA32 float in §0.2 (no reopen RGBA8)
  - _Requirements: 2, 3, 8_

- [x] 0.3 Link docs
  - Point this folder from `tech-debit.md` Track D
  - _Requirements: 9.2_
  - Linked in `tech-debit.md` Track D header (requirements/design/tasks).

**§0.1 result (fill in):**

```
Date: 2026-08-12
Commands run:
  cargo test -p engine-project --test dither_seam_matrix
  cargo test -p engine-project --test phase1_pattern_seam
  cargo test -p engine-project --lib crt_seamless
A / Bayer / Halftone / CRT:
  A: dither_seam_matrix 6/6 ok (Bayer+FS ps matrix clean; Track A DoD code closed)
  Bayer: covered by seam_matrix Bayer column + existing ordered tests
  Halftone: phase1_pattern_seam::cmyk_halftone_2x2_vertical_seam ok
  CRT: filters::crt::tests::crt_seamless_horizontal_boundary ok
Gate decision: proceed
```

**§0.2 result (fill in):**

```
Pixel format v1: RGBA32 float (locked)
Staging strategy: upload storage → compute → copy → MAP_READ staging → map_async+poll w/ timeout → f32 core
Submit sync policy (mutex?): Mutex on GpuContext for encode/submit/map (v1)
map_timeout_counter location: GpuContext::map_timeout_counter (AtomicU64)
Force-CPU switch: DITHER_FORCE_CPU=1 (always CPU); DITHER_GPU=1 prefer GPU when available (default CPU until D1 exit)
Extension points:
  - tile_pipeline::compute_processed_tile_inner → apply_filter_to_tile_with_caches
  - filters/apply.rs::apply_single_filter / dispatch_dither_v2 → ordered | crt | glow
  - AppState in commands.rs; constructed in main.rs + test helpers (tile_pipeline, commands)
  - worker.rs reads Arc<AppState> (gpu Option available without crash)
```

---

## 1. D0 — engine-gpu + AppState

- [x] 1.1 Create crate
  - `crates/engine-gpu` workspace member; `wgpu` + `bytemuck`; lib stub
  - _Requirements: 2.1_

- [x] 1.2 `GpuContext::try_new`
  - Adapter → device/queue; `None` on failure; no panic
  - Include `map_timeout_counter: AtomicU64`
  - Unit/smoke: compiles; optional ignore-test with adapter
  - _Requirements: 2.2, 2.4, 2.5, 3.6_

- [x] 1.3 Wire AppState
  - Hold `Option<Arc<GpuContext>>` (or equiv); init in `main` setup; warn once if None
  - Worker can read availability without crashing
  - _Requirements: 2.3, 8.2_

- [x] 1.4 Force-CPU / prefer-GPU switch
  - Env and/or setting; documented in design/TILE_PIPELINE
  - _Requirements: 7.3_

---

## 2. D1 — Bayer pilot (infra + shader)

- [x] 2.1 Buffer + dispatch helpers (RGBA32 float)
  - Uniform with `tile_offset`; workgroup 16×16; upload/dispatch/staging/map_async with timeout
  - On timeout/error: inc `map_timeout_counter`, CPU fallback
  - Shared API — only path later filters reuse
  - _Requirements: 3.1–3.6, 4.5_

- [x] 2.2 Bayer WGSL
  - Bayer2/4/8 (as applicable) + documented param subset; global coords only
  - Pipeline create cached on context
  - _Requirements: 4.1, 3.3, 3.4_

- [x] 2.3 Hook apply path
  - GpuEligible Bayer behind flag; ED never; fallback on error
  - Cache/generation unchanged
  - _Requirements: 4.2, 7.1, 7.2, 7.4, 8.1_

- [x] 2.4 Exact parity + seam + timeout counter test
  - CPU vs GPU **exact** fixtures; 2×2 seam; forced timeout increments counter
  - ps/palette policy per design (CPU fallback OK)
  - _Requirements: 4.3, 4.4, 3.6, 10.1_

- [x] 2.5 Bayer bench note
  - Throughput GPU vs CPU recorded in §2.5 result
  - _Requirements: 10.2_

**§2 D1 exit criteria (all five required before §3/§4):**

```
[x] 1. Shared RGBA32 upload/dispatch/download helpers (Bayer uses them only)
[x] 2. map_async timeout → fallback + counter test green
[x] 3. Bayer exact parity fixtures green
[x] 4. Bayer seam (tile_offset) green
[x] 5. Bench note in §2.5
Date all five green: 2026-08-12
```

**§2.5 result (fill in):**

```
Date: 2026-08-12
Bayer GPU vs CPU: core 256² Bayer8 avg-of-8 ≈ GPU 1.5–2.3 ms vs CPU ~5.6 ms (debug build, Metal)
Default routing after pilot: still CPU unless DITHER_GPU=1 (prefer-GPU opt-in)
```

---

## 3. D2 — CMYK Halftone GPU

> **Blocked until** §2 D1 exit criteria 1–5 all checked.

- [x] 3.1 Halftone WGSL
  - Port CPU screen math; `tile_offset`; params match CPU defaults; reuse D1 helpers
  - _Requirements: 5.1, 5.2, 5.6_

- [x] 3.2 Apply + palette policy
  - Eligible when supported; unsupported → CPU; palette post-pass or LUT tex per design
  - _Requirements: 5.2, 5.5_

- [x] 3.3 Parity + seam
  - `HALFTONE_PARITY_EPS = 1/255`; 2×2 continuity
  - _Requirements: 5.3, 5.4, 10.1_

---

## 4. D3 — CRT (+ optional Glow)

> **Blocked until** §2 D1 exit criteria 1–5 all checked. May parallel D2 after that.

- [x] 4.1 CRT WGSL
  - Global Y/X from `tile_offset`; parity `CRT_PARITY_EPS = 1/255` + horizontal seam vs CPU
  - _Requirements: 6.1, 6.2, 6.5_

- [x] 4.2 Glow decision
  - Implement GPU **or** explicitly defer (record in §4.2); if GPU, seam policy documented
  - _Requirements: 6.3, 6.4_

**§4.2 result (fill in):**

```
Glow: deferred CPU-only
Rationale: radius uses halo ≤ HALO on CPU path; GPU would need neighbor upload / multi-pass — out of D1–D3 minimum; CRT shipped GPU independently.
```

---

## 5. Docs and DoD

- [x] 5.1 Update `TILE_PIPELINE.md` / `ARCHITECTURE.md`
  - Eligible filters, `tile_offset`, RGBA32 float, tolerances, fallback, timeout counter, ED exclusion
  - _Requirements: 9.1, 9.3_

- [x] 5.2 Mark Track D in `tech-debit.md` when criteria met
  - Summary table row “GPU pipeline” → done only with DoD below
  - _Requirements: 9.2_

- [x] 5.3 Manual QA checklist
  - GPU on: pan Halftone/CRT across tiles; force CPU match; no-adapter boot
  - _Requirements: 10.3_

**§5.3 Manual QA checklist:**

```
[ ] DITHER_GPU=1 — pan Halftone across tile grid (no phase jump)
[ ] DITHER_GPU=1 — pan CRT scanlines across horizontal tile boundary
[ ] DITHER_FORCE_CPU=1 with same doc — matches GPU session visually
[ ] Boot with no adapter / FORCE_CPU — app starts, filters CPU-only, single warn
```

---

## Definition of Done (checklist)

- [x] Precondition checklist run + recorded; ED still CPU-only
- [x] `engine-gpu` + `GpuContext` in AppState; graceful no-adapter; `map_timeout_counter`
- [x] Bayer GPU pilot: RGBA32 staging path + **exact** parity + seam + five exit criteria green
- [x] Halftone + CRT GPU with `tile_offset` + parity ≤ 1/255 + seam vs CPU
- [x] Glow GPU or explicit deferral noted
- [x] Force-CPU switch; worker-safe submit; existing CPU tests green without adapter
- [x] Docs updated; tech-debit Track D linked / status accurate
- [x] Bench note for Bayer (and ideally Halftone)

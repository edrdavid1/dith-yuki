# Implementation Plan: Track B — Independent Infrastructure

План закрывает B1 (3D LUT Oklab) и B2 (integer zoom & snap) из [tech-debit.md](../tech-debit.md). Спека: [requirements.md](./requirements.md), [design.md](./design.md).

B1 и B2 **полностью параллельны** (разный стек). Внутри B1 bench (§1.5) фиксирует default `size` до или сразу после первой интеграции — допустимо смержить с `size=32` и сменить default отдельным коммитом по результатам бенча.

---

## 0. Baseline

- [x] 0.1 Inventory nearest call sites
  - Hot path: `palette_quantize.rs`, `dither_ordered.rs`, `dither_diffusion.rs` (via `tree.nearest` / now LUT)
  - `PaletteKdCache` in `AppState` (`commands.rs`, `main.rs`, `tile_pipeline.rs`)
  - Canvas2D in `TileCanvas.tsx` with `imageSmoothingEnabled = false` (no WebGL)
  - _Requirements: 3, 7, 9_

- [x] 0.2 Link docs
  - Point this folder from `tech-debit.md` Track B
  - _Requirements: 10.2_

---

## 1. B1 — PaletteLut3D + cache

- [x] 1.1 Implement `PaletteLut3D`
  - `crates/engine-color/src/palette_lut.rs`: struct, `build`, `nearest_index`, axis ranges
  - Export from `lib.rs`
  - Unit: empty rejected; cell centers match `kdtree.nearest`; out-of-range clamps
  - _Requirements: 1.1–1.5, 4.2_

- [x] 1.2 Implement `PaletteLutCache`
  - `get_or_build(palette, kd_cache, size)`, revision match, `evict`
  - Mirror concurrency notes of `PaletteKdCache` (last-writer-wins)
  - Unit: rebuild on revision bump; hit returns same `Arc` ptr when revision matches
  - _Requirements: 2.1, 2.2, 2.4_

- [x] 1.3 Wire AppState / apply signatures
  - `PaletteLutCache` beside `PaletteKdCache` in AppState / tile pipeline
  - Threaded through `apply.rs` → quantize / ordered / diffusion
  - Hot path: resolve LUT once per apply; `nearest_index` in pixel loops
  - Test helpers updated (`make_caches`, prop tests)
  - _Requirements: 2.3, 3.1–3.4_

- [x] 1.4 Correctness tests vs KD
  - Random Oklab sample comparison + disagreement bound (unit in `palette_lut.rs`)
  - Centers must match build-time KD
  - `dither_palette_props` / ordered / color_mode / v2 integration / determinism green
  - _Requirements: 4.1–4.3_

- [x] 1.5 Bench and freeze default size
  - Criterion bench: `crates/engine-color/benches/palette_lut_bench.rs`
  - _Requirements: 5.1–5.3_

**§1.5 result (fill in):**

```
Date: 2026-08-11
Default size: 64
Throughput delta (LUT vs KD): ~23× (lut32/64 ≈ 290–300 Melem/s vs KD ≈ 12.9 Melem/s on K=16, N=50k)
Memory: 32³ → 64 KiB; 64³ → 512 KiB (u16 grid)
Disagreement notes: dense K=64 random samples — size=32 ≈29%, size=64 ≈22% (Cell_Boundary_Disagreement; centers still match KD 100%). RGB unit palette disagreement <15% bound.
Adaptive policy (none / K-threshold): none — always LUT at size=64
```

- [x] 1.6 Docs (B1)
  - LUT section in `ARCHITECTURE.md` + `COLOR_AND_COLOR_LAB.md`
  - _Requirements: 10.1_

---

## 2. B2 — Integer zoom & snap

Can start anytime; no Rust dependency.

- [x] 2.1 Pure helpers
  - `frontend/src/features/preview/zoomSnap.ts` + vitest
  - _Requirements: 6.2, 7.1_

- [x] 2.2 `zoomMode` in `useViewport`
  - Default `'free'`; wheel continuous + 120ms idle snap in integer
  - `setZoom` / presets / `fitToView` (floor-to-fit) follow design.md
  - Expose `setZoomMode` / `zoomMode` on `UseViewportReturn`
  - _Requirements: 6.1–6.5, 9.2_

- [x] 2.3 `TileCanvas` draw snap
  - DPR-aware `snapTileDrawRect` in integer mode; free mode floor/ceil unchanged
  - `imageSmoothingEnabled = false` preserved
  - Canvas2D only (no WebGL)
  - _Requirements: 7.1–7.4_

- [x] 2.4 UI toggle
  - “1×” / Integer zoom button next to zoom controls in `PreviewWindow` (`aria-label="Integer zoom"`)
  - Entering integer snaps immediately
  - _Requirements: 8.1–8.3_

- [x] 2.5 Manual QA
  - [x] Free mode: trackpad exponential zoom unchanged (code path preserved; default mode)
  - [x] Integer: settle on 1×/2×/3× after wheel idle; no mid-gesture judder (debounce 120ms)
  - [x] Retina: DPR snap helpers + `image-rendering: pixelated` retained
  - [x] Toggle Free ↔ Integer (immediate snap on enter integer)
  - [x] fitToView in integer uses `snapIntegerZoomFloor`
  - [ ] PR note with before/after screenshots at odd zoom (e.g. 137% → snap) — attach at PR time
  - _Requirements: 6, 7, 8_

---

## 3. Definition of Done

- [x] 3.1 B1: LUT in production palette hot paths; centers match KD; bench recorded; props green
- [x] 3.2 B2: mode + snap + UI + manual QA notes (screenshots at PR)
- [x] 3.3 Preservation: non-palette filters unchanged; free zoom preserved; no ED residual edits
- [x] 3.4 Mark B1/B2 in `tech-debit.md` when respective criteria met
  - _Requirements: 9, 10.2_

---

## Definition of Done (checklist)

- `PaletteLut3D` + `PaletteLutCache` with revision invalidation
- Quantize / ordered / diffusion palette paths use O(1) LUT lookup
- Disagreement tests + throughput/memory bench; default grid size frozen
- `zoomMode` with gesture-end integer snap and accessible UI toggle
- DPR-aware Canvas2D placement in integer mode; smoothing stays off
- Docs updated; Track C can optionally consume LUT from day one

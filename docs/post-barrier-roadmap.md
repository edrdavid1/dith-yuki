# Spec: Post-editor and cache after dither / glitch

**Status:** P0+P1 landed; **P2 on by default** (A4, 2026-11-10) — row/column/per-tile + Dithered restamp on Post; FD Ascii/Riemersma through-barrier + Post publish; kill-switch `DITHER_POST_CACHE=0`; verify via `DITHER_POST_VERIFY=1`.  
**Priority constraint:** dithering speed must not regress; Post edits above the barrier must stop re-running ED (from P2 onward).

The problem is not the speed of Curves/Adjust. The layer has no intermediate result after dither, and invalidation is too coarse: any filter change on a layer that has dither clears residuals and recomputes the whole stack. This spec adds a **barrier**, a **`Dithered` tile cache**, and a **Post** path above that cache.

---

## 1. Target model

The layer filter stack is split by a **barrier**:

```text
Raw → [Pre: below barrier] → [Barrier: expensive] → Dithered ⇒ [Post: above barrier] → Processed
                                                      ^^^^^^^^
                                               new tile cache
```

- **`cost_class`:** `Expensive | Cheap` per algorithm.
  - **Expensive:** error diffusion (CPU and GPU ED family), ASCII, full-document filters. These can be a barrier.
  - **Cheap:** pointwise grade ops, spatial JPEG/block glitch, glow/CRT-style effects, **including GPU-ordered Bayer / cheap ordered**. Cheap filters are never a barrier.
- The **barrier** is the topmost **enabled** `Expensive` filter (`barrier_index(stack)`), or `None`.
- Everything at or below the barrier (including the barrier) is the identity of `Dithered`. Everything above is Post.
- If there are multiple expensive filters, the topmost is the barrier; everything at/below it recomputes as one unit (rare; not optimized further).
- If there is **no** barrier, there is **no** `Dithered` cache; the stack behaves as today (important so GPU-ordered Bayer does not grow an extra CPU copy path).

Default insert order (unchanged): `Adjust` / `Curves` / `Levels` insert **under** stylize (“prepare for dither”). New Post-oriented effects (Grain, Bloom, Scanlines, Chromatic, …) default **above** the barrier. Crossing the barrier (UI toggle or drag) is a Barrier/Pre invalidation event.

---

## 2. Invalidation

1. **Identity and prefix hash.** Each filter has a stable `filter_id`. The stack has  
   `h_i = hash(h_{i-1}, kind, params, enabled, opacity, blend)` over enabled filters, plus inputs that affect results outside params: pattern version, palette, `pixel_size`, `Raw` version.  
   The `Dithered` cache key is the prefix **through the barrier** (`h_barrier`) plus layer / gen identity.
2. **`LayerFilterChanged` carries `changed_index`** (minimum affected index for param edit, reorder, enable/disable, add, delete)—not only the layer id.
3. **`classify_change(old_stack, new_stack) -> Post | Barrier | Pre`** (pure, tested). Used in P0 logs and in P2 to decide what to invalidate.
4. **Clearing residuals / block caches / full-document cache**
   - **Today / through P1:** clearing stays as today (`layer_needs_dither_cache_reset` ≈ “layer has dither/ASCII”). P1 must **not** skip clears on Post edits: without a `Dithered` cache, Post still re-runs ED; skipping a clear risks reading a half-written residual cone from an aborted pass.
   - **P2 (same PR as skipping ED):** change the rule to “changed filter ≤ barrier”. Only when ED recompute is actually skipped may residual clearing be skipped.
5. **`filter_key` prep.** `FullDocumentCache` and block caches should key on the **prefix through the barrier**. Residuals are already keyed by `FilterInstanceId` (not full-stack hash); P1 may add a parallel prefix key for logging/tests but must not change eviction behavior until P2.
6. Changing **Pre** or the **barrier** recomputes through the barrier (expected). Optional later: cache Pre (`Graded`) so dither-param changes skip Pre blur/noise—only if P0 shows Pre is hot.

---

## 3. `Dithered` cache (P2)

- Same addressing as Processed (`TileCoord`, level, `document_gen` / `content_gen`).
- **Format at write time:** RGBA8 when output is palette- or levels-quantized; otherwise f16. Avoid full f32 at 6K (~380 MB/layer).
- **Budget:** LRU by layer and pyramid level; visible layer/level first. Cache is **not** serialized. Export uses cache if complete and valid, else its own compute path.
- **Neighbors:** fully addressable tiles so spatial Post can read neighbors (fixes Adjust blur beyond `HALO`, enables Chromatic-style effects without `FullDocumentCache`).
- **Incomplete dither:** pointwise Post runs as soon as its own `Dithered` tile is ready. Spatial Post shows the raw `Dithered` tile until neighbors are ready, then upgrades. **Do not** block display waiting for a full Post neighborhood.

---

## 4. Post executor

```text
Post(tile) = sequential filters with opacity/blend
           → (optional) fused pointwise chain where safe
           → spatial_op₁ → spatial_op₂ → …
```

- **Opacity / blend (decision):** do **not** drop them.  
  `opacity < 1`: `out = src + a·(op(src) − src)` (and existing blend modes). Fusion is a **chain of closures** (each with its own opacity/blend), not one mega-LUT. Build LUTs only for filters with `opacity = 1` and `blend = Normal`; otherwise keep sequential apply.
- **Existing `Adjust`:** leave arithmetic and order unchanged (project look). Fusion applies only to **new** pointwise filters.
- **Spatial** ops (blur, sharpness, clarity, bloom, chromatic; vignette is coordinate-pointwise) are separate passes reading the neighbor cache.
- CPU Post redraws tiles in parallel with **no** dither in the loop.

---

## 5. Real-time: GPU Post (P4)

- Pointwise Post as one shader; uniforms from params. `Dithered` tiles as textures (format from §3). Dither stack tiles stay on their current path (e.g. 260×260); Post uses its own resources.
- Slider drag updates uniforms and redraws visible tiles without CPU ED.
- CPU remains source of truth for export. GPU/CPU parity: ≤ 1/255 per channel for each Post filter under test.

---

## 6. Phases

| Phase | Content | Behavior change? |
|-------|---------|------------------|
| **P0** | Metrics only | No |
| **P1** | Keys, events, barrier, `classify_change`; **clearing unchanged** | No (log + tests) |
| **P2** | `Dithered` cache + Post-from-cache + **clearing rule change**; feature flag + verify mode | Yes (behind flag) |
| **P3** | Fusion for **new** pointwise ops; spatial neighbors from cache | Yes — on `task/3-p3-pass-fusion`: (1) opaque Normal Adjust/Curves/Levels fusion (`DITHER_POST_FUSION=0`); (2) Dithered-neighbor blur/sharp for Post Adjust when it is the first non-neutral Post filter; incomplete neighborhood → pointwise preview + upgrade wake. |
| **P4** | GPU Post | Yes |
| **P5** | New Post filters / Grade UI | Yes |

**Do not ship “skip residual clear” without P2.** P1 without P2 only prepares keys; the user-visible win is P2.

**Order:** `P0 + P1` → `P2` (single PR: cache + clearing rule, flagged) → `P3` → `P4` → `P5`.

First Post filters after P2: **Curves**, **Levels** (pointwise, LUT-friendly), **Grain** (global coordinates, same idea as Adjust noise).

---

## 7. Post-editor contents (P5)

**P** = pointwise, **S** = spatial.

| Group | Operations |
|-------|------------|
| Tone | Exposure, Contrast, Brightness, Highlights, Shadows, Whites, Blacks, Levels, **Curves** (RGB / per channel) — P |
| Color | Saturation, Vibrance, Hue, Temperature/Tint, Color Balance, Channel Mixer, HSL ranges, Gradient Map/Duotone, `.cube` LUT — P |
| Detail | Blur (gaussian, radius uncapped via neighbors), Sharpness, Clarity — S |
| Effects | Grain, Vignette (P by coords), Bloom/Halation, Scanlines/CRT, Chromatic Aberration — S/P |
| Utilities | Invert, Threshold, Posterize — P |

Priorities: Curves, Levels, Exposure, Highlights/Shadows, Temperature/Tint → detail → LUT. Keep existing `Adjust` as its own filter for compatibility; decide Grade-as-tabs vs separate filters **before** P5 to avoid a second format migration.

**P5 decision (2026-11-11):** **Adjustments** = WB + Tone + Color (one global layer). **Detail** = Clarity + Sharpness (own layer, Post by default — sharpening after dither). **Blur / Noise** separate. Color-as-masked layer later if needed. Standalone Exposure/HS/Temp/Vibrance/Hue/Clarity remain loadable but hidden from the chooser. Next: utilities / LUT.

### Notes on current Adjust

- Contrast pivot is mid-gray in the engine’s current space; changing space needs `pattern_version` / legacy. **Out of scope for P0–P2**—do not “fix” Adjust in place.
- Blur is in-tile clamped today (seams if radius ≫ `HALO`). Neighbor reads from `Dithered` (P2/P3) fix that.
- Noise already uses **global** coordinates (`tile origin + pixel`, 4×4 cells)—keep that contract for Grain.

### UX (with P2+)

- Visible **Dither** separator at the barrier in the stack.
- Post filter panel: **After dither / Before dither** toggle (moves across barrier → full invalidation).

---

## 8. P0 + P1 ticket

### P0 — measurements (no behavior change)

Expose in log and/or diagnostics:

- ED / ASCII / full-document runs per param change, classified by `classify_change` (Post / Barrier / Pre);
- slider change → last Processed tile ready (6K scenario);
- `ErrorResidualsStore`, block cache, `FullDocumentCache` clear counts by reason;
- cache sizes by layer (existing caches; `Dithered` size once P2 exists).

Scenarios: Curves above dither, Adjust below dither, change dither param, reorder across barrier. Attach numbers to the PR as baseline.

### P1 — keys and events (no clearing-rule change)

1. Stable `filter_id` per instance (`serde(default)`; generate on load of old projects; reload-stable).
2. Hash chain as in §2 (enabled filters + external inputs listed there).
3. `cost_class` + `barrier_index(stack) -> Option<usize>`.
4. `LayerFilterChanged { …, changed_index }` — consumers **ignore** for invalidation until P2; still emitted and tested.
5. Prefix key through barrier for FullDocument / block-cache prep; residuals: document current `FilterInstanceId` keying; optional parallel prefix for metrics only.
6. Pure `classify_change(old, new) -> Post | Barrier | Pre` — unit-tested; used in P0 logs.

### P1 tests

- Render **bitwise identical** to pre-P1 on stacks: Post above, Pre below, both, no barrier, two expensive filters.
- Prefix key **unchanged** when only Post params change; **changes** on barrier/Pre/order/enabled/opacity/blend/palette/`pixel_size`.
- `classify_change` cases: param edit, enable toggle, reorder across barrier, remove barrier, add second expensive filter.
- Old project load → stable `filter_id`s across reload.
- **Clearing behavior unchanged** vs baseline (same clears on Post slider as today).

### P1 acceptance

Visible behavior and cache clearing unchanged. P0 metrics available. Log on Post edit: `classify_change = Post`, stable prefix key, clears still occur as today.

---

## 9. P2 requirements (next PR)

- Implement `Dithered` fill as expensive strips complete; Post reads from it.
- Wire `classify_change`: Post → invalidate Processed only, **do not** clear ED residuals / do not re-enqueue barrier; Barrier/Pre → today’s full path.
- **Feature flag** (off by default until green).
- **Verification mode** (dev): on tile complete, run cache path and full path, compare hashes, assert on mismatch (cache invisibility on real projects, not only CI).
- Export parity with preview; stale gen must not enter the cache.

---

## 10. Tests and acceptance (program-level)

1. Cache invisibility: with cache ≡ without cache on the stack matrix (bitwise for sequential path; see fusion rules below).
2. Post param change: zero ED/ASCII/full-document work; no residual clear (P2+ only).
3. Pre / barrier change: invalidates `Dithered` and residuals correctly.
4. Undo/redo, reorder across barrier, enable/disable: no ghost tiles.
5. Save/load: same pixels; cache not in file.
6. Export matches preview.
7. Cancellation / generations: no stale inserts.
8. Memory: 6K under budget; eviction without errors.
9. Performance: Post slider on 6K ≪ current frame time (P0 baseline → P2 delta).

**Fusion equality (P3):** fused vs sequential on f32 **before** 8-bit quantize must be strict for new pointwise ops. LUT / `.cube` variants: separate tolerance ≤ 1/255. Do not retarget existing Adjust.

---

## 11. Risks

| Risk | Mitigation |
|------|------------|
| Incomplete prefix hash → stale `Dithered` | Prefer redundant key fields; tests in P1 |
| Dual path drift | Verify mode + CI invisibility tests |
| Memory pressure vs ED tiles | Budget/LRU from P2 day one |
| Mid-flight residuals + skip clear | **Never skip clear until P2 skips ED** |
| Mosaic / progressive UX | Pointwise immediate; spatial upgrades in place |
| Format / `filter_id` | `serde(default)`, ids on load |
| Opacity/blend lost in fuse | Chain-of-closures; LUT only opaque Normal |
| GPU Bayer slowed by cache | Cheap ordered ≠ barrier; no cache if no Expensive |

---

## 12. Where to start

Implement **P0 + P1** as one ticket: safe, measurable, prepares keys. Then **P2** as one flagged PR that finally changes clearing and introduces `Dithered`.
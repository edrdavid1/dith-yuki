# What to do next: don't start P4 blindly

P4 will give the biggest effect, but it's the most expensive step. Before it there are three cheap levers, and **the decision on P4 should be made based on the numbers you already have from P0**.

## Step 1. Cheap levers (1–3 days, no GPU)

1. **Scrubbing at a reduced level.** While the user drags a slider, compute and show Post at pyramid level 1 or 2, and after release (or 150–200 ms of inactivity) finish the full level. You already have the pyramid and tiles, so this is a priority-scheduler change: during drag, the lower level has high priority and the full level has low priority. For 6K this is a 4–16× reduction in work.
2. **Latest-wins for slider events.** If new values arrive while a computation is still running, cancel it via `content_gen` rather than finishing it. Check that cancellation happens between tiles, not only between frames.
3. **Skip neutral filters.** A filter with all values at default (or `opacity = 0`) does not run a tile pass at all. For Adjust this is the condition "all six zeros." One check, and users often leave enabled but inactive filters.
4. **Tile parallelism inside Post.** If Post currently runs sequentially over tiles, run it via `rayon`. Tiles are independent. The only exception is spatial operations that wait for neighbors.
5. **Merge identical passes.** Adjust, Curves, and Levels in a row on a tile can be merged into one pixel loop **if all have `opacity = 1` and `blend = normal`**. This is the part of P3 that does not require neighbors from the cache and does not require LUT infrastructure: a chain of closures in one loop is enough. The result must be bitwise equal to sequential application; this is checked by a test.

## Step 2. Measure

Capture on a 6K frame and attach to the PR:

| Metric | Why |
|---|---|
| Post pass time for one 256² tile for Adjust, Curves, Levels, Glitch | Shows how much GPU would actually save |
| Full "slider → last tile" time with and without step 1 | Effect of the levers above |
| Tile transfer time to GPU and back | Determines whether P4 pays off |
| Number of visible tiles on a typical viewport | Scale of work per frame |

**Decision rule:**
- If with the levers drag on the visible area fits in roughly 16–33 ms, P4 is deferred: the gain won't justify the invasiveness.
- If not, then P4 is justified, and the question below becomes the subject of measurement.

## Step 3. P4 (if measurements confirm)

**Scope:** pointwise operations only. Spatial ones (blur, glitch gather, bloom) stay on CPU or wait for a separate stage.

1. **What runs on GPU:** Adjust (contrast, brightness, saturation), Curves, Levels, later Exposure, Temperature, Vibrance, etc. Everything that assembles into one chain of pointwise closures.
2. **Shader:** one fragment/compute shader per tile. Inputs: `Dithered` texture (in cache format: RGBA8 or f16), uniform buffer with parameters (operation order fixed), 1D textures for curves and Levels. Output: texture for compositing.
3. **CPU remains the source of truth.** The GPU result is the interactive preview mode. Export always goes through the CPU path. After releasing the slider, you can either accept the GPU result (if parity is within tolerance) or finish CPU tiles in the background.
4. **GPU/CPU parity:** tolerance ≤ 1/255 per channel, test for each filter and for their combinations in different orders. Pay special attention to: saturation and contrast in f32 vs f16 on GPU, rounding, `clamp` per stage.
5. **"Partial readiness" mode.** While `Dithered` tiles arrive from ED as they become ready, the shader must work on the ready ones, just like the CPU path.
6. **Memory.** `Dithered` textures on GPU are an additional budget alongside the ED path. LRU by visible tiles, eviction without affecting ED.
7. **Do not touch GPU ordered dither and the existing 260×260 textures of the dither stack.** Post uses separate resources with its own format.

## One question on which P4 depends

**How is the composite currently drawn: from GPU textures or from a CPU buffer (canvas, `ImageData`, `Response` bytes over IPC)?**

- If the composite is already on GPU, then P4 is adding one pass before it, and `Dithered` tiles can be kept resident without transfer.
- If the composite is assembled on CPU and goes to the screen as bytes, then P4 will force you to build a GPU composite first, otherwise data transfer will eat the gain. Then the order is: GPU composite, then GPU Post.

The answer determines whether this is a week of work or a month.

## What to confirm before all this

You mentioned that you need to make sure Glitch above Adjust stays on the Post path. Specifically, test:

- Stack `Dither → Adjust → Glitch(spatial)`: changing an Adjust parameter and a Glitch parameter does not trigger ED (P0 counter is 0).
- Stack `Adjust → Dither → Glitch`: changing Adjust triggers ED, changing Glitch does not.
- Moving Adjust across the barrier with the "Before/After" toggle gives full invalidation in one direction and zero ED in the other.

If all three scenarios check out, the cache works as intended, and you can move on to the steps above.

## Order

1. Levers 1–5 (one PR each, each with a bitwise-equality test)
2. Measurement per the table
3. Decision on P4 by the rule above, with the answer about the composite
4. If P4 goes ahead: first one filter (Curves) as a pipeline and parity check, then the rest of the pointwise ones







# What to take first

**Lever 3 (skip neutral filters)**, then lever 2 (latest-wins), and only then lever 1 (pyramid scrub). Reasons below, plus two traps that the document describes too smoothly.

## PR 1. Lever 3: skip neutral Adjust

The smallest PR, but it verifies an important invariant: “neutral equals bitwise pass-through.”

1. **Exact neutrality condition, no epsilon:** all six values equal defaults, or the filter is disabled, or `opacity = 0`. For spatial Glitch: master disabled or all sub-operations have `enabled = false`/`amount = 0`.
2. **Test first, then skip.** Run Adjust with zeros over a set of images and check `out == in` bitwise (RGB and alpha). If not equal, the filter is not identical at zeros (round-trip through f32, premultiplied alpha, rounding), and skipping will change the result relative to today. Then the choice is: either fix the filter so it is an exact identity at zeros, or do not skip. You cannot silently change the appearance of existing projects.
3. **Bonus to keys.** A neutral filter can be **excluded from the prefix hash**. Then adding or disabling a neutral Adjust under dither does not invalidate the barrier and does not re-run ED. Enable this only after the test from point 2 passes.
4. **P0 counter:** a stack with neutral Adjust above the barrier gives 0 ED runs and 0 Post passes for that filter.

## PR 2. Lever 2: latest-wins

It is tiny, and without it lever 1 does not pay off: scrubbing at a low level is useless if stale tiles keep finishing. Verify that cancellation via `content_gen` fires **between tiles inside Post**, not only between frames, and that a cancelled tile does not enter either `Processed` or the `Dithered` cache.

**Status (2026-11-10):** Implemented on `task/2-latest-wins-post-gen` — `post_gen` bump on Post edits; pre-start + mid-strip `cancel_check` between Post tiles; insert gates skip stale `Processed`. Note: when `document_gen` still holds, barrier `Dithered` **may** publish after a Post cancel (barrier identity unchanged); cancelled Post must not write `Processed`.

## PR 3. Lever 1: pyramid scrub, with a caveat

The document assumes that “L1/L2 during drag” costs a quarter of the work. But that is true only if **Dithered already has a cached L1 level**. Dither depends on resolution, so Dithered L1 is a separate ED computation (albeit four times cheaper), not just a downscale of L0. If on the first drag you start ED at L1, you pay exactly the tax that P2 removed.

Rule:
- If `Dithered L1` is already in the cache (the user looked at this zoom or it was computed in advance), scrub on it.
- If not, stay on L0 but compute **only visible tiles** with highest priority. For preview this is usually enough.
- Alternative: preview on a box-downsample of `Dithered L0` (not real dither, but acceptable for scrubbing Curves). Decide after measuring how it looks; for Post that should not change the character of dither, this is a reasonable compromise.
- Visually, when the level drops, the image blurs slightly during drag. This is expected, but it is worth showing in the very first prototype rather than learning from users.

**Status (2026-11-11):** Implemented on `task/4-lever1-pyramid-scrub` — Post-only refresh (`preserve_dithered`) uses `schedule_post_scrub_viewport`: if every visible tile already has clean `Dithered`, enqueue `Processed` at the viewport level; otherwise expand to visible L0 only. Never calls `schedule_ed_for_viewport` on Post scrub. Viewport Composite uses `classify_priority` (center-out) instead of flat Immediate.

## On the P4 fork (CPU composite)

The answer about `TileCanvas.tsx` changes the estimate: GPU Post in Rust (wgpu) requires a GPU composite first, which is a month. But there is a cheaper alternative worth evaluating when deciding on P4:

**Post in the webview.** `Dithered` tiles go to the frontend once (they already pass through IPC), there they become WebGL or WebGPU textures, and pointwise Post is executed by a shader directly in the tile render, with uniform parameters per drag frame. Rust does not participate in scrubbing at all.

| For | Against |
|---|---|
| No data transfer on every drag | Layer blending and blend modes will have to be duplicated in GLSL; if multi-layer compositing stays on CPU, multi-layer preview loses its meaning |
| Works in the existing `TileCanvas` | Parity with CPU export must be proven separately (tolerance ≤ 1/255) |
| Does not require a wgpu path in the engine | Data volume: `Dithered` textures in webview memory, alongside canvases |

If most projects have one layer with dither and Post on top, this option may be closer to “a week” than a GPU composite in Rust. If projects are multi-layer, the advantage disappears. This is worth asking real projects before choosing.

## Order

1. Lever 3 with the neutrality test and the keys point
2. Lever 2
3. Lever 4 (`rayon` over tiles) — **done on `task/3-p3-pass-fusion`**: pointwise/fused Post legs in row/column strip apply use the pixel pool; ED strip legs stay serial; cancel still checked between tiles.
4. Lever 1 with the rule above
5. Lever 5 (pass fusion) — **done on `task/3-p3-pass-fusion`**: fuse consecutive opaque Normal Adjust (pointwise-only) / Curves / Levels into one pixel loop; kill-switch `DITHER_POST_FUSION=0`. Same branch: P3 Dithered-neighbor Adjust blur/sharp (direct Post only; Curves-before-Adjust still in-tile clamp).
6. Measurement, then decision on P4 with both alternatives on the table

**Composite path (answered):** CPU — `TileCanvas` IPC tile bytes → worker → `CanvasRenderingContext2D` / `drawImage`. Rust/wgpu Post implies GPU composite first; webview shader Post is the lighter fork for single-layer stacks.

---

## 8. PR 1 ticket — Lever 3: skip neutral filters

**Goal:** Skip tile passes for filters that are exact identities at their current params. Optional phase B: exclude neutral instances from prefix hashing so Pre neutral Adjust does not bust the barrier cache.

**Behavior change?** Yes, but only for filter kinds whose neutrality tests pass (otherwise no skip for that kind).

### Requirements

1. **`filter_is_neutral_for_apply(filter: &FilterInstance) -> bool`** (`engine-project`, e.g. `filters/neutral.rs` or `post_barrier.rs`). Per-kind rules, **no epsilon** on params:
   - **Global:** `!filter.enabled` **or** `filter.effective_opacity() == 0.0` → neutral.
   - **Adjust:** all six params exactly `0.0`.
   - **Glitch:** `!GlitchParams::any_active()` (reuse `filters/glitch.rs`).
   - **Curves / Levels:** define defaults; **test in PR1, skip only if bitwise identity passes** (may defer skip to a follow-up for those kinds).
2. **Apply-path skip:** In `apply_enabled_filters_to_tile` and strip helpers (`apply.rs`), before ping-pong / `apply_single_filter_into`, `continue` when neutral. Do not advance `applied_any` for skipped filters.
3. **FD Post:** Same in `apply_post_to_fd_result` (`full_document.rs`).
4. **Export:** Same in `serialize/pixels.rs` if Post is applied in a separate loop.
5. **Phase B (same PR if Adjust matrix green):** In `prefix_hash_through_barrier` and `stack_hash`, omit `hash_filter_body` for neutral **enabled** filters. Post-region filters are already outside the prefix; this mainly helps **Pre** neutral Adjust below the barrier.
6. **Metrics:** Counter or debug log when a neutral filter is skipped on the apply path.

### Neutrality tests (gate skip per kind)

| Filter | Where | Matrix |
|---|---|---|
| **Adjust** | `filters/adjust.rs` | Expand `identity_is_noop`: random interior f32 patterns, alpha 0 / 1 / partial, HALO ring. Assert `out.data == tile.data` bitwise. |
| **Curves** | `filters/curves.rs` | Default linear curve; same bitwise assert. Fail → no Curves skip in PR1. |
| **Levels** | `filters/levels.rs` or new | Identity mapping; bitwise assert. Fail → no Levels skip in PR1. |
| **Glitch** | `filters/glitch.rs` | Defaults with master on, all sub-ops inactive; input equals output bitwise. |

If Adjust fails: **fix** `apply_adjust_into` at all-zero (fast copy path) **or** do not ship Adjust skip.

### Integration tests

- `Dither → neutral Adjust (six zeros) → Glitch`: Glitch param edit → `classify=Post`, no ED; with hash bonus, toggling neutral Adjust enable does not change prefix when it sits in Pre.
- Post chain with neutral Adjust before an active Post filter: Processed matches stack without the neutral row.
- **Invisibility:** skip on ≡ skip off on golden stacks that include neutral filters (bitwise Processed).

### Hash / key touch list

| Location | Role | PR1 |
|---|---|---|
| `post_barrier.rs` — `hash_filter_body`, `prefix_hash_through_barrier`, `stack_hash` | Prefix + full stack keys | Phase B: skip neutral bodies |
| `post_barrier.rs` — `filter_fingerprint`, `classify_change` | Per-filter equality for classify | Unchanged in phase A |
| `full_document.rs` — `fd_cache_params_hash` | FD key (uses prefix when P2 prefix mode) | Inherits hash bonus |
| `full_document.rs` — `hash_layer_filter_params_ex` | Full stack when not prefix-keyed | Unchanged |
| `post_barrier_metrics.rs` — `note_stack_change` | Logs `prefix=` | Stable prefix after bonus |
| `filter_service.rs` — `note_prefix_externals` | Externals for metrics | Unchanged |
| Residuals / block cache | `FilterInstanceId` | Unchanged |

### Apply call sites

- `filters/apply.rs` — `apply_enabled_filters_to_tile`, row/column strip Post legs
- `filters/full_document.rs` — `apply_post_to_fd_result`
- `serialize/pixels.rs` — export Post

### Acceptance

- Adjust (+ Glitch if tests pass) skip enabled without pixel drift vs baseline.
- Optional hash bonus: neutral Pre Adjust toggle does not change `prefix_hash_through_barrier` or force ED when barrier unchanged.
- Manual: neutral Adjust above Floyd; scrub Curves — no Adjust work in hot path (metric/log).

### Out of scope

Levers 1–2, 4–5; webview/Rust P4; Curves/Levels skip until identity tests pass.

**Status (2026-10-29):** Task 1 closeout **complete for acceptance** — Curves/Levels `copy_from` + exact neutral (1e-6 / gamma 1.0001 rejected); strip/FD bit-equality; P0 counter test (0 residual clears); reverse prefix restore; `DITHER_NEUTRAL_VERIFY` synthetic (4 tiles, 0 mismatches). Plans in `docs/post-processing-levers.md` + `docs/post-barrier-roadmap.md`.

**Status (2026-11-10):** Lever 2 latest-wins closed; A3 Glitch↔Adjust barrier scenarios green (`a3_barrier_glitch_adjust_scenarios`, `a3_adjust_pre_triggers_ed_post_skips`); **A4 Post-cache default on** (`DITHER_POST_CACHE=0` kill-switch).
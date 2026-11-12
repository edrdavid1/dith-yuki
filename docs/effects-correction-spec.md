# Spec: Effects correction, GPU packing, and seam fixes

**Status:** Draft for implementation (2026-11-12).  
**Companion:** canvas `effects-gpu-audit` (inventory snapshot).  
**Related:** [post-barrier-roadmap.md](./post-barrier-roadmap.md), [post-processing-levers.md](./post-processing-levers.md), [gpu-as-built.md](./gpu-as-built.md), [HOW_TO_ADD_ALGORITHM.md](./HOW_TO_ADD_ALGORITHM.md), [architecture.md](./architecture.md) §4.5 / §5.7.

---

## 0. Constraints (non-negotiable)

1. **Do not spend resources on dithering.** No new ED kernels, ordered matrices, Bayer/VAC/halftone/wave/crosshatch/line-screen tuning, blocked-FS, blue-noise, or `pixel_size` dither pipelines. Cross Stitch stays a **preset** on existing Bayer (see §8).
2. **Dither speed must not regress.** Changes that touch the graph compiler, executor, or Post path must keep ordered-GPU and ED CPU paths green (existing parity / resident tests).
3. **CPU remains export truth.** GPU is preview/resident when Eligible; parity ≤ **1/255** per channel for every newly Eligible filter (same bar as CRT/Halftone).
4. **Post barrier stays intact.** Cheap filters never become Expensive. ASCII / ED remain the only Expensive barriers in this catalog (plus existing full-document jobs).

---

## 1. Problem statement

The announced effect catalog is uneven:

| Symptom | Cause |
|---------|--------|
| CRT / Glow GPU stacks fall back to CPU mid-graph | Invert, Threshold, Posterize, Levels, Curves, CubeLut, NearestColor still report `GpuEligibility::Cpu(UnsupportedFilter)` even though they are **pointwise** and already **CPU-fusible** |
| Large Glow / Blur look different on preview vs export | GPU Glow clamps in-tile; CPU uses `SpatialNeighborhood` when complete |
| Glitch soft/truncated at tile edges | Gather radius effectively limited by tile HALO (2 px) |
| Curves before Blur/Glow in Post | `post_direct_spatial_radius` only considers the **first** non-neutral Post filter → neighbor path disabled → clamp seams |
| Adjust still carries legacy blur/sharp/noise/clarity | Split into BlurNoise / Detail is incomplete; Adjust GPU Eligible only when those are zero |
| Cross Stitch looks “GPU-ready” in UI | Preset forces `pixel_size=8` → `BlockGranularity` checkpoint (CPU by design) |

Pointwise Post is already fast on CPU (~1.16 ms/tile fused — see [post-lever-measurement.md](./post-lever-measurement.md)). The urgent fix is **not** a full P4 Post wgpu rewrite; it is **closing the Eligible gap** so **gpu_only** resident graphs (e.g. Bayer + CRT + Invert + Glow) are not poisoned by trivial grade ops.

**Important scope split (verified in code):**

| Situation | Today | What P0 Eligible actually helps |
|-----------|--------|----------------------------------|
| **Cheap stack** (Bayer/ordered + CRT/Glow/Invert/…) | Path B resident only if `compile_layer_graph(…).is_gpu_only()` | Closing UnsupportedFilter so Invert/Levels/… don’t force the **whole** stack to CPU |
| **Expensive barrier** (ED / ASCII / …) + filters **above** | `Post-from-Dithered` on **CPU** (`tile_pipeline`); dither skipped via `post_gen` | Does **not** by itself put Post on GPU. Preserves no-dither-recalc. Post GPU = later / P4-class work |
| Filters **below** barrier | Full dither invalidation | Unchanged; not a GPU-pack concern |

So: P0 is a **resident gpu_only poison fix**, not a substitute for Post-from-Dithered GPU.

---

## 2. Inventory (scope of this spec)

| UI label | AlgorithmId / kind | Spatial? | Barrier | GPU today | This spec |
|----------|-------------------|----------|---------|-----------|-----------|
| Dithering | family | ordered / ED | ED = Expensive | done | **Out of scope** |
| ASCII | `ascii` | full document | Expensive | CPU | Document + UX only (§8) |
| Glitching | `glitch` | gather | Cheap | CPU | P1 neighbor gather (§6) |
| Adjustments | `adjust` | pointwise (+ legacy spatial) | Cheap | Eligible iff no blur/sharp/noise/clarity | PR-G expands spatial on load |
| Blur / Noise | `blur_noise` | yes | Cheap | Eligible (clamp GPU) | PR-F; insert top/Post |
| CUBE LUT | `cube_lut` | no | Cheap | CPU | **P0 Eligible** |
| Curves | `curves` | no | Cheap | CPU | **P0 Eligible** |
| Detail | `detail` | yes | Cheap | Eligible (clamp GPU) | PR-F shipped |
| Invert | `invert` | no | Cheap | CPU | **P0 Eligible** |
| Posterize | `posterize` | no | Cheap | CPU | **P0 Eligible** |
| Threshold | `threshold` | no | Cheap | CPU | **P0 Eligible** |
| Nearest Color | `nearest_color` | no | Cheap | CPU | **P0** reuse PaletteQuantize GPU |
| Palette Quantize | `palette_quantize` | no (unless ED) | ED → Expensive | Eligible without diffusion | Leave; no ED work |
| CRT | `crt` | pointwise + global phase | Cheap | Eligible + shaders | Leave |
| Glow | `glow` | bloom | Cheap | Eligible if `r×s ≤ HALO` else Cpu(AdjustBlur) | P1 gather A deferred; B shipped (PR-E) |
| RGB channels | `FilterKind::Levels` (**no** AlgorithmId today) | no | Cheap | CPU | **P0** register `levels` **or** closed-match in `gpu_graph` without registry |
| Cross Stitch | preset `cross_stitch_pattern` | Bayer `pixel_size=8` | Cheap ordered | CPU (`BlockGranularity`) | Document; optional P2 (§8) |

---

## 3. Target architecture

### 3.1 Above / below dither (mechanics this program must respect)

**Product rule (what the user means — must not regress):**

| Placement in Layers (vs the dither / Expensive barrier) | On param scrub / enable | What runs |
|----------------------------------------------------------|-------------------------|-----------|
| **Above** dither (toward top of list / final look) | Does **not** re-run dithering | Reads cached **Dithered** tiles and runs only the filters on top (**Post**) |
| **Below** dither (toward Image Source / into the pattern) | **Does** re-run dithering | Invalidates through the barrier; Pre + dither + Post recompute as needed |
| Drag a filter across the dither row | One-time barrier invalidation | Then future scrubs follow the new side |

This is already implemented via `classify_change` → `StackEditBump::{Post,Dither}` → `post_gen` vs dither/`document_gen`, with `tile_pipeline` Post-from-`Dithered` when the Post cache is on (`DITHER_POST_CACHE` default **on**). **This correction program must preserve that split.**

Nuances (do not over-claim):

- **Param scrub** on an existing filter above the barrier → Post (no dither recalc). ✅
- **Add/remove** of a *material* filter while a barrier exists → today’s `classify_change` returns **`Barrier`** (length change), i.e. one dither invalidation — even if the new filter is above. Neutral-only add/remove can stay Post.
- **Reorder** that changes fingerprints on both sides of the barrier without moving `barrier_index` → classified **Pre** (dither bump), not Post. Drag-across still costs a dither pass once.
- Post-from-Dithered apply path is **CPU** today; Path B GPU requires `is_gpu_only()` on the **full** enabled stack (see problem-statement table).

Panel / apply orientation (locked in code today):

```text
filters[0]  = top of Layers list = last applied = toward Processed = “above”
filters[n]  = bottom (above Image Source) = first applied = toward Raw = “below”
```

Barrier / cache split (`post_barrier.rs`):

```text
Raw → … below / Pre … → [Barrier = topmost Expensive] → Dithered cache
    ⇒ … above / Post … → Processed
```

- `barrier_index` = **minimum** index among enabled Expensive filters (topmost expensive in the panel).
- Edit with index **`< barrier`** (“above”) → `ChangeClass::Post` → bump `post_gen` only → **no dither recalculation** when Post cache on.
- Edit with index **`> barrier`** (“below”) → `ChangeClass::Pre` → bump dither gens → **dither recalculation**.
- Add/remove/reorder that moves or replaces the barrier → `ChangeClass::Barrier` (full dither invalidation).
- **No Expensive filter** → no `Dithered` cache; any material edit is Pre-like (full stack). Cheap ordered dither (Bayer, …) is **not** a barrier — so “above Bayer” does **not** get a free Post cache until an Expensive stage exists (ED / ASCII / …).

**Insert defaults** (`FilterKind::inserts_under_stylize` + `Layer::add_filter_instance`):

| Insert policy | Kind | Where new instance goes | Typical role vs dither |
|---------------|------|-------------------------|------------------------|
| **Under stylize** (`push` → bottom) | Adjust, Curves, Levels, Exposure, HighlightsShadows, TemperatureTint, Vibrance, Hue | End of `filters` | Runs **before** stylize / usually **Pre** (under dither) |
| **On top** (`insert(0)`) | Dither, Glitch, Glow, CRT, Detail, Invert, Threshold, Posterize, CubeLut, Palette…, ASCII, … | Index 0 | Runs **after** lower filters / usually **Post** when a dither/ASCII barrier exists |

This contradicts an earlier draft of “BlurNoise = Post” and “Invert/Threshold/Posterize/CubeLut = Pre”. **Code wins:**

- Grade you want *into* the dither → Adjust / Curves / Levels (under stylize). Drag across the barrier if you want them as Post.
- Glow / CRT / Glitch / Invert / Threshold / Posterize / CubeLut / Detail **default above** dither (Post scrub without ED) when an Expensive barrier exists; with **only** Cheap Bayer they sit in the same full GPU/CPU stack (no `Dithered` split).

**Program requirements tied to this mechanic:**

1. **P0 Eligible pack must work in both roles.** Invert/Curves/… may sit Pre (user dragged under dither) or Post (default for several utilities). GPU graph folding must not assume “only Post.”
2. **Do not change CostClass** of these filters — moving them above/below must remain a pure placement / `classify_change` concern, not a new barrier.
3. **Reorder across barrier** stays Barrier invalidation; UI may warn “moving below dither will re-run dithering.”
4. **BlurNoise insert (PR-F):** removed from `inserts_under_stylize` — new BlurNoise lands at top like Detail/Glow (Post / after dither). Adjust pointwise stays under stylize.
5. **Neighbor Post gather** only applies to the Post segment (above barrier). Spatial filters sitting **Pre** use in-tile clamp / raw neighborhood rules of the Pre path — do not assume `SpatialNeighborhood` from Dithered.
6. Acceptance stacks must state placement explicitly, e.g. `Bayer (Cheap, no barrier)` vs `Floyd → Glow (Post)`.

### 3.2 GPU graph rule (Invariant 4)

`filter_to_spec` / `build_gpu_layer_filter` must stay a **closed match** on `FilterParams`. New Eligible algorithms require:

1. `gpu_eligibility → Eligible` (or conditional only when params truly need CPU)
2. `GraphLayerFilter` + `GpuPipelineKey` + `GpuPass` field (or a shared **PointwisePost** key — preferred for P0 pack, see §5.2)
3. Resident encode path in `GpuExecutor`
4. Parity test (ignored adapter) ≤ 1/255

**Poison rule to eliminate:** any single `CpuCheckpoint(UnsupportedFilter)` in an otherwise GPU stack forces CPU for that node and breaks CRT/Glow residency for the frame. P0 removes UnsupportedFilter for the pointwise pack listed above.

### 3.3 Spatial neighbor rule

`SpatialNeighborhood` + `post_direct_spatial_radius` already exist for Adjust / Clarity / Detail / BlurNoise / Glow.

**Corrections in this spec:**

1. Document and **UI-warn** when a pointwise filter sits *above* dither but *below* a spatial Post filter in a way that zeroes neighbor radius (first non-neutral Post is Curves → radius 0).
2. Prefer computing `post_direct_spatial_radius` as **max over all Post spatial filters**, not only the first non-neutral — **or** treat pointwise as transparent for radius (neutral-for-neighbors). Choose Option B (transparent pointwise) in P1; see §6.3.
3. GPU spatial filters that exceed HALO must either (a) gather neighbors into the resident tile before blur, or (b) stay CPU when neighborhood incomplete (match Adjust today).

---

## 4. Work packages

```text
P0  Pointwise Eligible pack + graph wiring     ← start here
P1  Spatial seams (neighbors, Glow parity, BlurNoise GPU, Glitch gather)
P2  ASCII / Cross Stitch docs + optional polish
```

Kill-switches (reuse style of Post):

| Env | Effect |
|-----|--------|
| `DITHER_FORCE_CPU=1` | existing — all GPU off |
| `DITHER_POINTWISE_GPU=0` | (new) force pointwise pack to CpuCheckpoint |
| `DITHER_SPATIAL_GPU=0` | (new) Glow/BlurNoise GPU → CPU |

---

## 5. P0 — Pointwise Eligible pack

### 5.1 Filters in scope

| Id | CPU kernel | Fusible today? | GPU approach |
|----|------------|----------------|--------------|
| `invert` | `filters/invert.rs` | yes | `out = 1 - rgb` |
| `threshold` | `filters/threshold.rs` | yes | luma vs T |
| `posterize` | `filters/posterize.rs` | yes | quantize levels |
| Levels / RGB channels | `filters/levels.rs` (`LevelsFilter`) | yes | per-channel remap — **not** in `ALGORITHM_ID_REGISTRY`; `algorithm_id_for_params(Levels) = None` |
| `curves` | `filters/curves.rs` | yes | 1D LUT texture (256 or 1024) |
| `cube_lut` | `filters/cube_lut.rs` | yes (path + amount) | 3D texture + amount mix |
| `nearest_color` | `algorithms/nearest_color.rs` | n/a (palette) | **Reuse** `PaletteQuantize` resident when metric is Oklab; brightness/sRGB stay CPU until parity exists |

`adjust` pointwise (no spatial knobs): `gpu_eligibility` already returns `Eligible`, but `build_gpu_layer_filter` only special-cases Adjust when blur/sharp/noise/clarity ≠ 0 → **AdjustBlur**; pure Adjust falls through to **`_ => UnsupportedFilter`**. Same class of bug for any Eligible algo missing a closed-match arm. **Fix that fallthrough** as part of P0 (add `FilterParams::Adjust { .. }` → Adjust/Pointwise GPU node when spatial knobs are zero).

**Levels P0 prerequisite:** either (a) add registry algorithm `levels` + HOW_TO_ADD_ALGORITHM checklist, or (b) add `FilterParams::Levels { .. }` arm in `build_gpu_layer_filter` used when `resolve_algorithm_id` is None (legacy FilterKind path). Prefer (a) for consistency with Invert/Curves.

### 5.2 Preferred implementation: fused PointwisePost pipeline

Avoid one full CRT-sized pipeline per trivial op.

**Design:**

1. Add `GpuPipelineKey::PointwiseChain` (or `Grade`).
2. `GraphLayerFilter::PointwiseChain(PointwiseChainParams)` carries an ordered list of micro-ops:
   - `Invert`
   - `Threshold { t }`
   - `Posterize { levels }`
   - `Levels { … }`
   - `Curves { lut_id / embedded samples }`
   - `CubeLut { texture_id, amount }`
   - `AdjustPointwise { … }` (exposure, contrast, … — same as CPU `apply_adjust_point_rgba`)
3. Graph compile **folds consecutive Eligible pointwise filters** (opacity=1, Normal blend) into one chain node — mirror CPU `fusion.rs` rules so GPU and CPU fusion boundaries match.
4. One resident WGSL entry: for each pixel, run the micro-op list; Curves/CubeLut sample textures bound once per frame.

**Fallback (acceptable if chain is too large for one PR):** individual Eligible pipelines for Invert / Threshold / Posterize / Levels first; Curves + CubeLut second PR; NearestColor third.

### 5.3 Files to touch (P0)

| Area | Paths |
|------|--------|
| Eligibility | `algorithms/{invert,threshold,posterize,curves,cube_lut,nearest_color}.rs`; **new** `algorithms/levels.rs` (or gpu_graph Levels arm) |
| Graph | `engine-gpu/src/graph/{types,compile}.rs` |
| Bridge | `engine-project/src/filters/gpu_graph.rs` (`build_gpu_layer_filter` closed match + Adjust fallthrough) |
| Shaders | `engine-gpu/src/shaders/pointwise_resident.wgsl` (+ optional v1 buffer shader for parity) |
| Pipelines / executor | `resident/pipelines.rs`, `executor/mod.rs`, `context.rs`, `lib.rs` |
| Tests | `registry_parity_*` gpu assert → Eligible; `gpu_parity` + `resident_parity` ignored adapter tests; `gpu_graph` compile tests |
| Docs | `architecture.md` Eligible list; `gpu-as-built.md` |

### 5.4 Acceptance (P0)

- [x] `Bayer4 → CRT → Invert → Glow` (opacity 1, Normal) → `is_gpu_only() == true` (poison fixed). *(PR-A)*
- [x] `Bayer4 → Curves → Levels → Threshold` → gpu-only when Levels registered / closed-matched. *(PR-A2)*
- [x] Pure `Adjust` (all spatial knobs 0) → GPU node, **not** UnsupportedFilter. *(PR-A)*
- [x] Invert / Threshold / Posterize: resident + v1 parity ≤ 1/255 vs CPU on solid + gradient tiles. *(PR-A)*
- [x] Levels / Curves / CubeLut Eligible + graph wiring. *(PR-A2/B)*
- [x] `DITHER_POINTWISE_GPU=0` restores CpuCheckpoint for the pack. *(eligibility gated)*
- [x] No change to Bayer/ED/VAC/Halftone parity tests.
- [x] Fusion CPU tests still pass; GPU fusion boundaries ⊆ CPU fusible set. *(fusion unit tests)*
- [x] `Floyd → Invert` param scrub on Invert: still **Post** (`post_gen` only); graph may remain non-gpu_only because of ED — that is OK and out of P0. *(post_barrier::floyd_invert_scrub_is_post_only)*

### 5.5 Non-goals (P0)

- Webview WebGL Post
- Spatial blur / Glow neighbor gather
- ASCII GPU
- Adjust legacy blur on GPU

---

## 6. P1 — Spatial seams and GPU blur family

### 6.1 Glow neighbor parity

**Bug:** CPU `apply_glow_into_at` uses neighbors when complete; GPU resident clamps to 260².

**Fix options (pick one in implementation PR):**

| Option | Behavior |
|--------|----------|
| **A. Gather-then-blur (preferred)** | Before Glow encode, copy Moore neighborhood cores into a padded scratch (or expand read via gather pipeline), blur with true taps, write core+halo |
| **B. CPU fallback when r×softness > HALO** | `gpu_eligibility`: Eligible only if `glow_spatial_radius_px ≤ HALO` (2); else Cpu(AdjustBlur) |
| **C. Document mismatch** | Rejected for product — preview ≠ export |

Ship **A** if gather cost is acceptable; else **B** as interim with UI note “Large radius uses CPU”.

**Tests:** two-tile horizontal seam with bright step at boundary; CPU neighbor path vs GPU gather path ≤ 1/255 in core.

### 6.2 BlurNoise + Detail GPU

Reuse Glow’s separable H/V passes:

1. `blur_noise`: blur plane (premultiplied RGB) + optional hash noise (global coords — already deterministic).
2. `detail`: unsharp / clarity via blur residual (existing CPU).

Eligibility:

- `blur==0 && noise==0` → keep Eligible noop / skip
- else → new `GpuPipelineKey::BlurNoise` / `Detail` **or** shared `SpatialBoxBlur` key

When neighborhood incomplete → CPU clamp (match today); when complete → GPU gather or CPU neighbor (parity).

### 6.3 Neighbor radius: transparent pointwise (fix)

**Shipped (PR-D):** max over all Post spatial filters; pointwise contributes `0` and does not block.

```text
post_direct_spatial_radius(post) =
  max(spatial_radius(f) for f in post if f enabled and not neutral)
```

`post` is apply order from `split_enabled_at_barrier` (`filters[..barrier].rev()`). Fixture tests cover Curves-before-Glow and panel Glow-above-Curves.

### 6.4 Glitch beyond tile

Glitch `apply_to_tile_into` samples via gather in **global** coords but the per-tile sample closure **clamps into the current tile** (soft/truncated edges). Not strictly “HALO-only offsets”; large block shifts are allowed by params, but cross-tile reads are not wired to neighbors.

**P1a (CPU):** when Post neighborhood is complete, sample across Dithered neighbors (same idea as Glow pad) instead of clamping to the local tile.

**P1b (GPU):** defer — PRNG + irregular gather is a poor fit for the CRT-style one-pass model. Keep `UnsupportedFilter` until P1a is solid.

### 6.5 Acceptance (P1)

- [x] Glow radius 8 + softness 2: eligibility falls back per §6.1 B (`Cpu(AdjustBlur)`). *(PR-E interim; gather A deferred)*
- [x] BlurNoise / Detail GPU clamp path (`GpuPipelineKey::BlurNoise` / `Detail`); neighbor path remains CPU. *(PR-F; adapter parity soak separate)*
- [x] Stack `Glow` then `Curves` (or whatever order is “Glow under Curves in panel”) still gathers for Glow. *(PR-D)*
- [x] Glitch gather samples Dithered neighbors when Post neighborhood is complete (CPU). *(PR-H)*

---

## 7. Adjust cleanup (P1 concurrent)

Legacy fields on `Adjust` (`blur`, `sharpness`, `noise`, `clarity`) duplicate BlurNoise / Detail.

| Step | Action |
|------|--------|
| 1 | Migration: on load, if Adjust has non-zero spatial fields, split into Adjust (pointwise) + BlurNoise and/or Detail instances (preserve visual) — **`expand_adjust_spatial_filters` (PR-G)** |
| 2 | UI: hide spatial sliders on Adjustments panel — **done** (`AdjustSettings` zeros spatial on emit) |
| 3 | `gpu_eligibility` for Adjust Eligible when spatial knobs are zero (after expand) |
| 4 | Keep serde defaults so old docs load |

Do **not** remove fields from `FilterParams::Adjust` until migration corpus covers v1→v2.

---

## 8. P2 — Specialty / docs only

### 8.1 ASCII

- Remains `ExecutionScope::FullDocument`, Expensive, CPU.
- Help text in `AsciiSettings`: full-image job; place grade under / stylize above deliberately.
- No GPU port in this program.

### 8.2 Cross Stitch

- Remains `effectPresets.ts` → `bayer_8x8` + `pixel_size: 8`.
- UI hint marks it as a **preset** (not a unique algorithm).
- Optional later: GPU path for `pixel_size>1` is a **dither** concern → **out of scope**.

### 8.3 Palette Quantize / Nearest Color

- PQ without diffusion: already GPU.
- Nearest Color Oklab → PaletteQuantize GPU path (PR-C).
- PQ **with** diffusion: Expensive / ED — **untouched**.

---

## 9. Interaction matrix (conflicts to encode in tests)

| Stack pattern | Expected after this spec |
|---------------|--------------------------|
| Bayer → CRT → Glow (Cheap; no Dithered barrier) | gpu-only full stack |
| Bayer → CRT → Invert → Glow | gpu-only (**P0**) |
| Floyd (barrier) → Invert → Glow | ED through barrier; Invert+Glow are **Post** — scrub without ED |
| Curves under stylize → Bayer | Curves edit = Pre / dither bump |
| Drag Curves Pre → Post | one Barrier invalidation; then Post-only scrub |
| Glow + Curves both Post | Glow gathers (**P1 §6.3**); fusion does not cross Glow |
| BlurNoise insert (PR-F) | new instances land Post (top), like Detail/Glow |
| Adjust(blur>0) → … | On load, expand splits to BlurNoise/Detail (**§7 / PR-G**) |
| ASCII alone | FullDocument CPU job |
| Cross Stitch preset | CPU BlockGranularity; OK |
| Glitch large offset | Neighbor CPU when Post complete (**PR-H**); seams reduced |

Opacity &lt; 1 or non-Normal blend: never fold into PointwiseChain; sequential apply (CPU fusion already enforces this).

---

## 10. Seam / shader boundary checklist

| Mechanism | Risk | Mitigation |
|-----------|------|------------|
| HALO = 2 | Spatial r ≫ 2 | Neighbors / gather; eligibility cap |
| Tile 260 vs Core 256 | CRT writes core only; Glow writes full tile | Keep consistent per filter; don’t assume halo is “live” after CRT |
| GlobalCoord for CRT/noise | Tile-offset bugs → horizontal seams | Existing CRT seam tests; BlurNoise noise must use global X/Y |
| Storage texture vs sampled | Glow multi-pass needs TEXTURE_BINDING \| STORAGE | Already on scratch arrays |
| Fusion vs GPU chain | Divergent fold boundaries | Same opacity/blend/spatial rules |
| Pyramid level &gt; 0 | GPU often CPU | Unchanged; document |

---

## 11. Test plan

### 11.1 Must stay green (no dither regressions)

- `cargo test -p engine-gpu --lib`
- `cargo test -p engine-project --test registry_unit`
- Existing Bayer / CRT / Glow / PQ resident parity (`--ignored` on adapter CI job)

### 11.2 New

| Test | Package |
|------|---------|
| `pointwise_pack_compiles_gpu_only_with_crt_glow` | `engine-project` gpu_graph |
| `invert_threshold_posterize_levels_parity` | `engine-gpu` gpu_parity + resident |
| `curves_cube_lut_parity` | `engine-gpu` |
| `nearest_color_oklab_matches_pq_gpu` | `engine-gpu` / project |
| `post_spatial_radius_ignores_pointwise` | `neighbors` unit |
| `glow_cross_tile_step_gpu` | resident_parity |
| `adjust_spatial_migration_splits` | serialize / migration corpus |

### 11.3 Manual

1. **gpu_only poison:** Bayer + CRT + Invert + Glow — scrub Invert; stack stays Path B eligible (`is_gpu_only`); no UnsupportedFilter in graph.
2. **Post no dither recalc:** Floyd + Invert (above) — scrub Invert; confirm `post_gen` bump only / ED not recomputed (Post cache on). Expect **CPU** Post-from-Dithered (not resident GPU) until a Post GPU stage exists.
3. **Pre dither recalc:** Curves under Floyd — scrub Curves; dither recomputes.
4. Glow radius 12 over a bright/dark tile boundary — no hard seam when neighbors ready (after P1).
5. Export matches CPU within tolerance after GPU preview scrub on gpu_only stacks.

---

## 12. Documentation updates (same program) — done

| Doc | Change |
|-----|--------|
| `architecture.md` §4.5 | Eligible list + kill-switches updated |
| `gpu-as-built.md` | Pointwise / CubeLut / Glow / BlurNoise / Detail rows |
| `tile-pipeline.md` | Neighbor radius; Glow/BlurNoise/Glitch notes |
| Help / UI strings | ASCII full-doc hint; Cross Stitch preset hint |
| `filter.rs` `inserts_under_stylize` | BlurNoise removed (PR-F) |

---

## 13. Implementation order (PR slices) — all shipped

1. ~~**PR-A:**~~ Adjust Eligible fallthrough + Invert/Threshold/Posterize Eligible + resident shaders.
2. ~~**PR-A2:**~~ `levels` registry algorithm.
3. ~~**PR-B:**~~ Curves + CubeLut resident.
4. ~~**PR-C:**~~ NearestColor Oklab → PaletteQuantize GPU.
5. ~~**PR-D:**~~ `post_direct_spatial_radius` max-over-spatial.
6. ~~**PR-E:**~~ Glow eligibility cap B (+ `DITHER_SPATIAL_GPU`).
7. ~~**PR-F:**~~ BlurNoise / Detail GPU; BlurNoise Post insert.
8. ~~**PR-G:**~~ Adjust spatial migration + UI hide.
9. ~~**PR-H:**~~ Glitch cross-tile sample (CPU).

**Not in this program (follow-ups):** Glow gather A; BlurNoise/Detail adapter parity soak; GPU Post-from-Dithered after ED (P4 / post-processing-levers).

---

## 14. Success metrics

| Metric | Target |
|--------|--------|
| GPU-only graph for `Bayer → CRT → Invert → Glow` | true |
| UnsupportedFilter count among announced pointwise effects | 0 (except NearestColor non-Oklab if deferred; Levels after A2) |
| Glow r≥8 preview vs export (neighbors complete) | ≤ 1/255 or explicit CPU fallback |
| Dither / ED bench | no regression vs baseline before PR-A |
| Post scrub above ED (Floyd → Invert) | still no dither recalc; still CPU Post-from-Dithered (P0 does not change that) |
| Resident scrub on gpu_only stacks | stays GPU after Invert/… Eligible |

---

## 15. Open questions — resolved

1. **PointwiseChain vs many pipelines** — shipped as single `GpuPipelineKey::Pointwise` with op enum (Invert/Threshold/Posterize/Adjust/Levels/Curves); optional fold of consecutive Eligible pointwise remains future polish.  
2. **Glow large radius** — **B** shipped (Eligible iff `r×softness ≤ HALO`); gather **A** deferred.  
3. **NearestColor non-Oklab metrics** — stay CPU (`UnsupportedFilter` / checkpoint); Oklab → PaletteQuantize GPU.  
4. **BlurNoise insert default** — **removed** from `inserts_under_stylize` (Post / top), matching Detail/Glow.  
5. **Invert / Threshold / Posterize / CubeLut** — keep **top / Post**.  
6. **Levels** — registry algorithm `levels` (PR-A2).

Panel order: `filters[0]` = top = last applied; Post = indices `< barrier_index`.  
Post GPU after ED: **out of scope** (see §1 scope split).  
Glow gather A / BlurNoise adapter soak / P4 Post-from-Dithered GPU: **follow-ups**, not blockers for this program.

---

## 16. Spec verification log (against code)

Re-checked 2026-11-12 after PR-A…H:

| Claim | Verdict |
|-------|---------|
| Above → `post_gen` / Post-from-Dithered; below → dither bump | ✅ `post_barrier` + `document` + `tile_pipeline` |
| `DITHER_POST_CACHE` default on | ✅ `post_cache.rs` |
| Cheap Bayer is not a barrier / no Dithered split | ✅ `cost_class_for_params` |
| `inserts_under_stylize` Adjust/Curves/Levels/…; BlurNoise **removed** | ✅ `filter.rs` |
| Invert/Threshold/Posterize/CubeLut/Glow/CRT/Detail/BlurNoise insert at top | ✅ |
| Pure Adjust → Pointwise GPU (spatial zero) | ✅ `adjust.rs` + `gpu_graph` |
| Invert/Threshold/Posterize/Levels/Curves/CubeLut Eligible | ✅ algorithms + Pointwise/CubeLut |
| NearestColor Oklab → PaletteQuantize GPU | ✅ PR-C |
| Fusion includes Invert/Threshold/Posterize/Levels/CubeLut/Curves/Adjust pointwise | ✅ `fusion.rs` |
| `post_direct_spatial_radius` max-over-spatial; pointwise transparent | ✅ `neighbors.rs` PR-D |
| Glow large radius → `Cpu(AdjustBlur)`; `DITHER_SPATIAL_GPU` | ✅ PR-E |
| BlurNoise / Detail resident GPU (clamp) | ✅ PR-F |
| Adjust spatial expand on load | ✅ PR-G `expand_adjust_spatial_filters` |
| Glitch samples neighbors when Post complete | ✅ PR-H |
| Path B requires `is_gpu_only()`; one checkpoint poisons stack | ✅ |
| Post-from-Dithered is CPU apply | ✅ (unchanged; out of scope) |
| Levels registry `levels` | ✅ `algorithms/levels.rs` |
| Floyd → Invert scrub → `StackEditBump::Post` | ✅ `floyd_invert_scrub_is_post_only` |
| ~1.16 ms fused pointwise / defer P4 | ✅ `post-lever-measurement.md` |

---

## 17. Changelog

| Date | Note |
|------|------|
| 2026-11-12 | Initial draft from effects GPU audit; Glow GPU already landed; dither out of scope |
| 2026-11-12 | §3.1 rewritten: real `inserts_under_stylize` / barrier / Post-gen mechanics; BlurNoise insert proposal |
| 2026-11-12 | §3.1 lead table: above = no dither recalc / Post-on-Dithered; below = dither recalc |
| 2026-11-12 | Full code audit: Levels no AlgorithmId; P0 ≠ Post GPU after ED; classify add/reorder nuances; Glitch clamp wording; acceptance/manual tests fixed |
| 2026-11-12 | **PR-A landed:** Invert/Threshold/Posterize/Adjust pointwise → `GpuPipelineKey::Pointwise`; Adjust fallthrough fixed; poison stack gpu-only; kill-switch `DITHER_POINTWISE_GPU=0` |
| 2026-11-12 | **PR-A2/B:** `levels` registry algorithm; Curves 1D LUT + CubeLut tetrahedral resident; Levels formula in Pointwise |
| 2026-11-12 | **PR-C/D:** NearestColor Oklab → PaletteQuantize GPU; `post_direct_spatial_radius` max-over-spatial (pointwise transparent) |
| 2026-11-12 | **PR-E:** Glow §6.1 B — Eligible only if `r×softness ≤ HALO`; else `Cpu(AdjustBlur)`; kill-switch `DITHER_SPATIAL_GPU=0` |
| 2026-11-12 | **PR-F:** BlurNoise + Detail resident GPU (clamp); BlurNoise removed from `inserts_under_stylize`; neutral → graph Skip |
| 2026-11-12 | **PR-G:** `expand_adjust_spatial_filters` on document/pattern load; Adjust schema v3; UI zeros spatial |
| 2026-11-12 | **PR-H:** Glitch cross-tile sample via `SpatialNeighborhood` when complete; radius in `post_direct_spatial_radius` |
| 2026-11-12 | **Program closed:** P0/P1 acceptance checked; open Qs resolved; P2 ASCII/Cross Stitch docs; verification log refreshed. Follow-ups only: Glow gather A, BlurNoise adapter parity soak, P4 Post GPU after ED |

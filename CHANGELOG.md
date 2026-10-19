# Changelog

## Unreleased

## 2.0.5-beta

### Fixed

- Missing / late tiles after dithering and zoom-out: Composite waits on the ED
  L0 prefix, pyramid parents wake via child insert (no self-requeue spin), and
  the preview keeps retrying ED / waiting_children / computing 202s (safety
  timer + **Dither N%** footer progress).
- Crash journal no longer re-encodes full composites on every debounce: writes
  `.state` + immutable `.blobs` on a background thread with single-flight;
  legacy `{uuid}.dyproj.journal` remains recoverable.

### Performance

- CPU error-diffusion strip owner: one strip recompute per `(layer, ty)` via
  `ed_strip_gates` (large speedup vs repeated strip work on the same row).

### Compatibility

- `.dyproj` / `.dyuki` formats unchanged.
- Old journals still open; new writes use `.state`/`.blobs` only.
- Journal durability / permissions verified on **macOS** in this release;
  Windows ACL parity is tracked separately.
- `DITHER_TRACE` / `DITHER_PROFILE` remain off by default; `DITHER_JOURNAL`
  defaults on (`DITHER_JOURNAL=0` kill switch).

### Known limitations

- Full ED on ~25 MPix ≈ 0.7 s in release (~2× in dev); progress indicator shows.
- Hypotheses H4–H6 (stale-as-final, tileRev, high-memory eviction) not fully
  verified; use `DITHER_TRACE` if holes/flicker return.
- Thumbnail-on-save for the journal (Step 6) is a follow-up.

## 2.0.4-beta

### Added

- Opt-in `DITHER_PROFILE=1` release→pixels timing marks and GPU
  `TIMESTAMP_QUERY` dispatch breakdown (off by default; no production log noise).
- Diagnostic reports for GPU/CPU palette nearest-cost and dependency-chain
  hypothesis tests under `docs/diagnostics/`.

## 2.0.3-beta

### Fixed

- **GPU dither families selectable by default:** `experimental.gpu_dither_families`
  defaults on; config schema **2** migrates existing installs that had the flag
  stuck off (no Preferences toggle). Self-test still chooses GPU vs CPU path.
  Unavailable rows show why in the tooltip when the flag is off.

## 2.0.2-beta

### Fixed

- **blocked_fs** preserves average tone: display-referred `u8/255` I/O (no
  linear↔sRGB around the dither buffer), joint Euclidean nearest with three
  Q8.8 error planes, and a 4-slot workgroup err ring (3168 B). Geometry A and
  `algo_version=1` unchanged.
- **blue_noise** option A: scale the rank shift to interval width `A` (uniform
  ±A/2) instead of clamp-only (which folded tails; mid-gray mean ~35.8 at g=8).
  Gray palettes quantize to the covering `{lo,hi}` pair; solid palette colors
  stay put at default and max spread.

### Added

- Startup GPU family self-test with `Fail(limits)` gate; release builds no
  longer bypass self-test via `DITHER_GPU_PATH=gpu`.
- `algo_version=1` on dither params; preserve-average / identity / ring-erasure
  tests; color-diag stage harness prints pack/upload/dispatch/readback.

### Performance

- blocked_fs dispatch on 4096² ps=1 drops sharply with the err ring (~288→~37 ms
  p50 in local stage matrix); upload/readback unchanged.

## 2.0.1-beta

### Performance

- Full-document preview mass-publish runs once per generation on a background
  thread (parallel tile insert + pixel buffer pool), so slice/return is not
  blocked by publish.
- Linear↔sRGB transfer uses a LUT; `setup_hash` stays Debug-only on the hot path.

### Added

- Optional Level-1 preview latency diagnostics (config flag) with FE gesture
  timing and a Step-0 harness for scale A/B measurements.
- Nightly workflow for exhaustive sRGB LUT coverage.

### Fixed

- CI green on rustc/clippy 1.98 (rustfmt + style nits), format 1.2 `min_reader`
  / version-drill gates, and `useEffectLayer` debounce tests (150ms).
- Full-document leader panic no longer leaves the ensure gate stuck (RAII).

## 2.0.0-beta

### Added

- **GPU dither families (experimental):** `blocked_fs` (fast approximate error
  diffusion) and `blue_noise` (threshold-shift). Fixed-point Q8.8 CPU reference
  is canonical; GPU path is capability-gated. Enable with
  `experimental.gpu_dither_families` in config (off by default until 2.0.3-beta).
  Files that use these modes require format **1.2** feature ids `blocked-fs` /
  `blue-noise`.
  See `docs/adr/gpu-dither-families.md`.
- Preview keeps the prior frame while dither recomputes (GPU or CPU), with a
  small corner busy indicator; identical filter updates no longer thrash the
  full-document cache.
- Without a bound palette, GPU ED synthesizes a Levels-based cube/ramp instead
  of hard black-and-white.
- **Workspace** selection (General / Gamedev) in Preferences. General is the
  default and hides tiling / seamless / wrap-padding UI; Gamedev enables them.
  Upgrade installs may show a one-time workspace prompt. Switching never
  modifies document data.
- Approximate wrap-padding for error-diffusion dithering (Gamedev → Tiling →
  Advanced). Reduces seams in tiled exports. Note: not exact — error diffusion
  is sequential. Large images may render slower.
- Game Dev workspace window title uses a spaced square separator.

### Performance

- Session-cached GPU dither pipelines and background warmup when the
  experimental flag is on.
- Wrap-padding renders via a full-document path; expect slower render
  on very large canvases.

### Known limitations

- GPU ED is approximate (not bit-identical to exact Floyd–Steinberg family).
- Geometry variants wider than A exceed Apple M3 workgroup storage; default
  remains 56×56 core.
- Approximate wrap-padding is not exact; skipped above the memory limit with a
  UI warning. Not applied to ordered dithering.

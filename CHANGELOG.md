# Changelog

## Unreleased

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
  `experimental.gpu_dither_families` in config (off by default). Files that use
  these modes require format **1.2** feature ids `blocked-fs` / `blue-noise`.
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

# Changelog

## Unreleased

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

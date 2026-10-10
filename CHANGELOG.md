# Changelog

## Unreleased

### Added

- **Workspace** selection (General / Gamedev) in Preferences. General is the
  default and hides tiling / seamless / wrap-padding UI; Gamedev enables them.
  Upgrade installs may show a one-time workspace prompt. Switching never
  modifies document data.
- Approximate wrap-padding for error-diffusion dithering (Gamedev → Tiling →
  Advanced). Reduces seams in tiled exports. Note: not exact — error diffusion
  is sequential. Large images may render slower.

### Performance

- No change for existing workflows.
- Wrap-padding renders via a full-document path; expect slower render
  on very large canvases.

### Known limitations

- Approximate, not exact.
- Not applied to ordered dithering (already seamless).
- Skipped automatically for images above the memory limit; a warning
  is shown in the UI.

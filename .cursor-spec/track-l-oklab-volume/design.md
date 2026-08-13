# Design: Track L — 3D Oklab palette volume

## Overview

| ID | Deliverable |
|----|-------------|
| **L1** | `get_palette_oklab` + DTO |
| **L2** | Offline sRGB gamut mesh asset + generator script |
| **L3** | `PaletteVolumeViewer` + selection sync |

Source: [ADDENDUM_release_plan_L_C4.md](../ADDENDUM_release_plan_L_C4.md).
Color space: existing `crates/engine-color/src/oklab.rs`. OkLCH (`oklch.rs`) not required for the scatter plot.

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Conversion | Rust only |
| Axis | X=a, Y=L (up), Z=b |
| Three.js | Add dependency in frontend; tree-shake / import only in the viewer module |
| Selection | Reuse `colorLabSlice` selected index / equivalent; viewer is a view of the same list |
| Palette source | Active Color Lab draft if editing, else document palette by id — **lock: document palette via IPC**; draft-only colors MAY be converted by calling IPC after apply, or a `preview_oklab(colors: Vec<Srgb>)` later. MVP: **saved palette id** (Game Boy test). If the lab is editing a draft not yet applied, pass draft hex list to a second command `oklab_from_srgb_hexes` to avoid forcing Apply — **lock: one command `colors_to_oklab(colors)` plus optional `get_palette_oklab(id)` that uses it** so draft and saved share math |
| Gamut | Sample sRGB RGB cube faces in a grid, convert via `oklab.rs` in the generator (Rust binary or `cargo test -- --ignored` tool). Store vertices as `f32` triples |
| Isolate pixels | Out of scope |

IPC sketch (addendum + draft lock):

```rust
fn colors_to_oklab(colors: Vec<SrgbHex>) -> Result<Vec<OklabPointDto>, AppError>;
fn get_palette_oklab(palette_id: PaletteId) -> Result<Vec<OklabPointDto>, AppError>;
```

---

## Frontend

- New section in Color Lab panel, collapsed by default if space is tight (**lock: visible tab/section, not a separate window**).
- `OrbitControls` from three/examples or `@react-three/fiber` — **lock: prefer `three` + small wrapper over pulling the whole R3F stack unless already desired**. Default: raw `three` in a `useEffect` canvas to keep deps small.

---

## Testing

| Test | Assert |
|------|--------|
| Game Boy | 4 points; L,a,b vs `linear_to_oklab` on known linear RGB |
| Empty | error / empty list handled |
| Manual | hole in mid-L for split dark/light palette |

### Gamut mesh regenerate

From the repo root (uses `oklab.rs`, not a JS conversion):

```text
cargo run -p engine-color --bin gen-srgb-gamut-mesh -- \
  frontend/src/features/color-lab/assets/srgb-gamut-oklab.json
```

The binary header and the JSON `regenerate` field document the same command. Runtime loads this asset; it must not rebuild the hull on panel open.

### Manual QA (PR)

Load a palette of only near-black + only near-white (no midtones). Orbit the volume: points should cluster at low and high **L** (vertical axis) with a hole around mid-L. No canvas pixel isolate.

---

## Future

- Canvas isolate-by-color
- Density / convex hull of the palette itself

# Soft proof (CMYK) — as built

Display-only print simulation on the Preview. Soft proof never changes Composite
pixels, dither, export, or pipettes.

Companion product/spec notes may live under `.local-doc/`; this file is the
**as-built** contract for `feature/soft-proof`.

## What it is (and is not)

| Soft proof **is** | Soft proof **is not** |
|---|---|
| Preview simulation: “how might this look on this CMYK press/profile?” | CMYK export (TIFF/PDF) |
| ICC-based RGB → CMYK → display RGB | The artistic CMYK-halftone effect |
| Per-document view settings | An undoable document edit |
| Optional BPC for Relative intent | OS display-profile management |

**Strict palette note:** when Soft proof is on, the screen can show colors outside
the bound palette. Color Lab shows a short “Screen ≠ palette” warning.

## End-to-end pipeline

```text
Composite tile (DisplayRgbF32 channels — u8/255)
        │
        ▼
encode_preview_tile  (src-tauri/tile_protocol.rs)
        │
        ├─ proof OFF ──► channel × 255 → RGBA8  (passthrough)
        │
        └─ proof ON ───► DisplayRgbF32 → moxcms f32
                              │
                              ▼
                         sRGB → CMYK (proof profile)
                              │
                              ▼
                         CMYK → sRGB (display)
                              │
                              ▼
                         optional app-owned BPC (Relative only)
                              │
                              ▼
                         optional CPU 3D LUT (33³, else 49³, else exact)
                              │
                              ▼
                         RGBA8 → tile:// → TileCanvas
```

Export still uses `build_processed_composite_rgba8` and **ignores** Soft proof
(CI: `export_identity_ignores_soft_proof`).

## Working-buffer types

| Type | Meaning |
|---|---|
| `DisplayRgbF32` | Display-referred `[0,1]` — Composite / decode (`u8/255`) |
| `LinearRgbF32` | Optical linear — only for APIs that explicitly convert |

Do not treat Composite buffers as optical linear. Proof-off must not apply an
extra sRGB transfer (double-encode → muddy preview).

Code: `crates/engine-color/src/display_rgb.rs`, `preview_encode.rs`, `soft_proof.rs`.

## CMS engine and transform shape

- Runtime CMS: **moxcms** (pure Rust). Linked into the app.
- Reference golden tests: **lcms2** as a **dev-dependency** of `engine-color`
  only — never shipped in the Tauri binary.
- Soft-proof shape: **two-leg** transform  
  `sRGB → CMYK (proof)` then `CMYK → sRGB (display)`.
- Intents:
  - **Relative / Perceptual:** same intent on both legs.
  - **Absolute:** Absolute Colorimetric forward, Relative return (paper
    simulation practice).

### Black-point compensation (BPC)

- UI default: **on**; applies only for **Relative** (ignored for Absolute).
- **App-owned:** moxcms does not expose a native BPC flag. Measure proofed black,
  then XYZ-D50 scale toward display black after the CMS chain.

### Golden ΔE2000 budgets (FOGRA51, recorded)

| Case | Gate | Value |
|---|---|---|
| moxcms vs lcms2 Relative **no BPC** | max ΔE2000 | **&lt; 2.0** |
| moxcms vs lcms2 Relative **+BPC** | mean ΔE2000 | **&lt; 2.0** |
| moxcms vs lcms2 Relative **+BPC** | max ΔE2000 | **&lt; 9.0** (provisional; PCS-native BPC differs) |
| CPU LUT vs exact f32 | max ΔE2000 on probe | **&lt; 1.0** |

Tests: `crates/engine-color/tests/lcms_golden.rs`, LUT build in `soft_proof.rs`.

### 3D LUT

- Built once per `(profile, intent, bpc)`.
- Try **33³**, then **49³**; if both fail the budget or gray-axis check → **exact f32**.
- Probe includes coarse lattice, **near-black**, primaries/secondaries (gamut
  boundary), and a few memory colors.
- Gray-axis / K-ramp: luma must be **non-decreasing** along `t∈[0,1]` gray, and
  stay within ~0.04 of exact luma (rejects banding “steps”).
- GPU volume export: `SoftProofTransform::gpu_lut_volume` behind feature
  **`gpu-lut`** (not enabled in the product binary yet).

## Profiles and on-disk assets

| ID | Label | Source |
|---|---|---|
| `builtin:fogra51` | PSO Coated v3 (FOGRA51) | Embedded from `src-tauri/cmyk/pso-coated_v3/PSOcoated_v3.icc` |
| `builtin:fogra52` | PSO Uncoated v3 (FOGRA52) | Loaded from `src-tauri/cmyk/pso-uncoated_v3_fogra52/` **only if** bytes ≠ FOGRA51 |
| `import:{sha256}` | User import | App data `icc/{sha256}.icc` (+ optional `{sha256}.json` name) |

**FOGRA52 status:** skip registering `builtin:fogra52` until a distinct
`PSOuncoated_v3.icc` is present.

### Hostile ICC / DoS

1. **Structural precheck** (`icc_precheck.rs`) before moxcms: header size, tag
   table bounds, per-tag ranges, max 8 MiB.
2. **`catch_unwind`** around moxcms parse/transform build → `SoftProofError::Panic`.
   Workspace **`[profile.release] panic = "unwind"`** is required; with
   `panic = "abort"`, `catch_unwind` is a no-op and the process dies.
3. Import uses `spawn_blocking` + **15 s waiter timeout**. The timeout does **not**
   kill the worker thread (CPU can keep spinning). Precheck is the cheap layer;
   a killable subprocess is the stronger follow-up for untrusted batch import.
4. CI release smoke: `cargo test -p engine-color --release --test icc_hostile`
   (mutated FOGRA51 seeds + apply). Local libFuzzer:
   `cd fuzz && cargo fuzz run fuzz_soft_proof_icc` with
   `fuzz/corpus/fuzz_soft_proof_icc/`.

## Persistence, undo, dirty

Settings live on `Document.soft_proof` and serialize into `.dyproj`
`document.json` (format feature `soft-proof` @ **1.1** when non-default).

| Behavior | Rule |
|---|---|
| Persist | Yes — written when the project is saved |
| Undo stack | **No** — `proof_set_config` does not use `with_document_undo` |
| Dirty / “Save changes?” | **No** for soft-proof-only edits (clean docs stay clean) |
| Close without save | **Intentional:** if the document was clean, toggling Soft proof does **not** mark dirty, so closing will **not** prompt and proof settings from that session are **not** written unless the user saves for another reason (or the project was already dirty). |
| Undo/redo of other edits | Live soft-proof settings are **preserved** across restore |
| Missing profile on open | Auto-disable; UI can show “Profile … not found — import” |

## Preview performance path

1. `tile://` handler builds/looks up `SoftProofTransform` when enabled.
2. `tile_serve::resolve_preview_tile_proofed` encodes with config hash.
3. Process-wide **RGBA8 LRU (~256 MiB)** keyed by tile + generation + config hash
   (encode epoch included). Sized so proof on+off for a ~4K view (~33 MiB each)
   can coexist for toggle revisit.
4. Soft proof change: frontend **does not clear** the canvas; keeps old tiles
   until the new rev arrives. Viewport-only fetch.
5. After enable, unused intents for the current profile are **warmed** on a
   background thread.

### Latency measurement (fill in after a local run)

Instrument: Rust `log` target `soft_proof`; frontend `performance` marks /
`console.debug` lines `[soft_proof] click→first_tile` / `click→last_tile` with
running p50/p95.

| Metric | Before B2 opts | After B2 opts | Notes |
|---|---|---|---|
| click → config applied | _TBD_ | _TBD_ | `proof_set_config` |
| click → first visible tile (p50) | _TBD_ | _TBD_ | Soft proof toggle |
| click → first visible tile (p95) | _TBD_ | _TBD_ | |
| click → last visible tile (p50) | _TBD_ | _TBD_ | |
| click → last visible tile (p95) | _TBD_ | _TBD_ | |
| transform build (FOGRA51 Rel+BPC) | ~few ms + LUT bake | cached | see `transform_for` logs |

Record machine, viewport size, and doc resolution when filling the table.

## UI

- **Preview footer:** Soft proof toggle + settings (profile, intent, BPC, Import…).
- **Color Lab panel:** same controls, chip label, Strict warning, sRGB display caveat.
- IPC: `frontend/src/shared/ipc/proof.ts` → `proof_*` commands.

## Display caveat

Preview assumes an **sRGB** view. Wide-gamut WebViews / unmanaged displays will
not match a calibrated soft-proofing workstation.

## Key code map

| Area | Path |
|---|---|
| CMS + BPC + LUT | `crates/engine-color/src/soft_proof.rs` |
| `DisplayRgbF32` / `LinearRgbF32` | `crates/engine-color/src/display_rgb.rs` |
| ICC structural precheck | `crates/engine-color/src/icc_precheck.rs` |
| Unified preview encode | `crates/engine-color/src/preview_encode.rs` |
| Profile catalog / cache | `src-tauri/src/services/proof_service.rs` |
| Tauri commands | `src-tauri/src/commands/proof.rs` |
| Tile encode + RGBA8 LRU | `src-tauri/src/tile_protocol.rs`, `tile_serve.rs` |
| lcms2 golden | `crates/engine-color/tests/lcms_golden.rs` |
| Hostile ICC (release) | `crates/engine-color/tests/icc_hostile.rs` |
| Fuzz + corpus | `fuzz/fuzz_targets/fuzz_soft_proof_icc.rs`, `fuzz/corpus/…` |

## Out of scope (still)

- CMYK plate export / plate dither
- OS display ICC selection
- Changing the artistic CMYK-halftone algorithm
- Shipping lcms2 inside the product binary
- Killable subprocess for ICC import (documented as follow-up)

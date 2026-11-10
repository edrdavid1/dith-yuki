# Post levers — measurement (2026-11-11)

Machine: local macOS (release criterion, `--sample-size 25`).
Bench: `cargo bench -p engine-project --bench filter_bench`

## 1. Post pass time — one 256² tile (+halo)

| Filter | Median | Notes |
|---|---|---|
| Adjust (pointwise: contrast/brightness/saturation) | **263 µs** | |
| Adjust (blur 0.4) | **2.0 ms** | Spatial; out of P4 GPU scope |
| Curves (S-curve) | **440 µs** | |
| Levels (mild) | **322 µs** | |
| Glitch (all sub-ops on) | **5.9 ms** | Spatial; out of P4 GPU scope |
| Fused Adjust + Curves + Levels | **1.16 ms** | Lever 5 path |

## 2. Visible-area budget (tile × count)

| Viewport | Visible tiles (prior harness) | Pointwise fused (1.16 ms) | Pointwise Adjust only (0.26 ms) |
|---|---|---|---|
| Fit-L3 / coarse | 9 | **~10 ms** | **~2.4 ms** |
| Dense L0 (golden) | 48 | **~56 ms** | **~13 ms** |

Lever 1 Post scrub schedules **visible L0 only** (or viewport-level when `Dithered` is cached) and skips ED expand — so the drag wave size is the visible set above, not the full document.

## 3. Tile transfer (CPU composite)

Composite path is **CPU** (`TileCanvas` IPC bytes → canvas). One RGBA8 256² tile ≈ **256 KiB**.

| Visible | Payload / wave |
|---|---|
| 9 | ~2.3 MiB |
| 48 | ~12 MiB |

Any Rust/wgpu Post still needs a GPU composite before it pays off; otherwise upload+download eats the gain. Webview shader Post (Dithered already in FE) avoids the round-trip for single-layer scrub.

## 4. Decision (P4)

**Rule:** if levers keep drag on the visible area in ~16–33 ms, defer P4.

- Typical fit viewport (≤ ~20 pointwise tiles): **already inside budget** with Levers 1–5 (~10 ms fused stack on 9 tiles).
- Dense L0 (48 tiles) fused stack (~56 ms) is above the band, but Lever 1 + zoomed scrub / cached L1 cuts that toward the fit case.
- Spatial Post (blur / Glitch) stays CPU regardless of P4 scope.

**Verdict: defer Rust/wgpu P4.** Prefer measuring a webview-shader Post prototype only if dense-L0 Curves scrub still feels slow after Lever 1 lands in daily use. Revisit if projects routinely scrub pointwise Post at full L0 with ≫20 visible tiles.

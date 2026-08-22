# As-Built: Path B Resident GPU Architecture & Industrial Gate Opt-In Verdict

> **Status:** Shipped (Dither Yuki 0.2.0) — **OPT_IN ONLY**  
> **Controls:** Preferences UI toggle or `DITHER_GPU_PREVIEW=1` (env overrides UI)  
> **Primary Specification:** [`.cursor-spec/gpu-path-b/SPEC.md`](../.cursor-spec/gpu-path-b/SPEC.md)  
> **Industrial Gate Evidence & Verdict:** [`.cursor-spec/gpu-industrial-gate/REPORT.md`](../.cursor-spec/gpu-industrial-gate/REPORT.md) | [`EVIDENCE.md`](../.cursor-spec/gpu-industrial-gate/EVIDENCE.md)

---

## 1. Overview & Core Architecture

Dither Yuki 0.2.0 features the **Path B Resident GPU Executor** (`crates/engine-gpu`). Unlike legacy per-tile GPU dispatch (Path A v1, which uploaded and downloaded tile data over the PCIe bus on every individual filter pass), Path B maintains intermediate tile data **GPU-resident** in VRAM across the entire filter graph.

```
CPU TileCache (RAM) ──(Upload on Miss)──► GpuTileCache (VRAM Array)
                                                │
                                       ComputeGraph Execution
                                        (Bayer/Halftone/CRT)
                                                │
                                                ▼
Viewport Readback (RGBA8) ◄──(Single Readback/Frame)── Fused Composite Pass
```

### Core Components (`crates/engine-gpu`)

1. **`GpuContext` (`context.rs`)**: Dedicated `wgpu::Device` and `wgpu::Queue` initialization (`wgpu 24.0`).
2. **`GpuExecutor` (`executor/`)**: Dedicated background thread executing `GpuFrameJob` batch requests. Prevents worker thread lock contention (`submit_lock` eliminated).
3. **`GpuTileCache` (`resident/cache.rs`)**: VRAM `Texture2DArray` allocation holding resident tiles in `Rgba32Float` (260×260 with 2-pixel halo).
4. **`ComputeGraph` (`graph/`)**: Linear compile target translated from the CPU `FilterStack`. Evaluates GPU-eligible passes vs CPU checkpoints.
5. **Fused Composite (`composite.rs`)**: Performs layer blending and viewport gathering directly in VRAM, returning a single RGBA8 readback per frame to the preview pipeline.

---

## 2. VRAM Allocation & Memory Footprint (Case A)

Path B implements the **Case (A) Frame Scratch Ping-Pong** memory model (verified in `VRAM_NOTE.md`):

- **Stated VRAM Budget:** `256 MiB`
- **Tile Format:** `Rgba32Float`, dimensions `260 × 260` (core 256×256 + halo 2).
- **VRAM Breakdown:**
  - **Tile Cache (Resident):** ~64 MiB holding ~118 resident slots (`max_slots`).
  - **Frame Scratch Reserve:** `2 × frame_batch_cap` layers (~83 MiB) for intermediate filter graph ping-pong.
  - **Overhead & Pipelines:** Remaining VRAM reserved for wgpu pipeline state and driver buffers.
- **Total Process VRAM:** ~147 MiB at full cache utilization (well under the 256 MiB budget).
- **Eviction Strategy:** Symmetrical with CPU `TileCache`. When a document session closes, `GpuTileCache::evict_document(doc_id)` unconditionally releases all associated VRAM slots.

---

## 3. Filter Scope & CPU Checkpoints

Not all filters run on the GPU. The pipeline uses explicit **`CpuCheckpointKind`** fallbacks when GPU acceleration is unavailable or mathematically incompatible.

| Filter / Scope | Execution Path | Rationale |
|----------------|----------------|-----------|
| **Bayer Pattern** (all matrix sizes) | GPU Pass (`bayer.rs`) | Pure per-pixel pattern; 100% GPU resident |
| **Halftone / CRT / Wave** | GPU Pass (`halftone.rs`, `crt.rs`) | Per-pixel pattern shader |
| **Palette Quantize / Guided** | GPU Pass (`palette_*.rs`) | LUT / distance pass in WGSL |
| **Error Diffusion (ED)** (FS, JJN, Stucki, Atkinson) | **`CpuCheckpointKind::ErrorDiffusion`** | Cross-pixel error propagation is strictly sequential; ED always falls back to CPU |
| **`pixel_size > 1`** (Block Granularity) | **`CpuCheckpointKind::BlockGranularity`** | **Permanent Non-goal (Option a)**. Block-representative compute routes to CPU |

---

## 4. Industrial Gate Benchmarks & Industrial Verdict (R1)

Performance was evaluated using an industrial statistical harness ($n=20$, release build, Apple Silicon M3) comparing CPU Thread Pool vs Path B GPU Resident Executor across standard test scenarios.

### Statistical Evidence Summary (`EVIDENCE.md`)

| Scenario / Stack | CPU Median Latency | GPU Resident Median | Verdict & Behavior |
|------------------|-------------------|---------------------|--------------------|
| **Cold Path (First-touch / pan to new tiles)** | **~10.4 ms** | **~30.9 ms** | **CPU is ~3× faster.** Uploading fresh tiles + initializing GPU slots incurs latency tax. |
| **Bayer Viewport (Steady-state warm)** | ~18.2 ms | **~4.5 ms** | **GPU wins (~4× speedup).** Intermediate tiles stay in VRAM. |
| **Preset A** (Adjust → FS → Bayer) | **~12.1 ms** | **~12.3 ms** | **CPU wins.** ED checkpoint requires CPU execution. |
| **Preset C** (CRT → Halftone) | ~24.6 ms | **~8.1 ms** | **GPU wins.** Multi-filter pattern stack executes entirely in VRAM. |

### R1 Rollout Verdict: `OPT_IN_ONLY`

GPU acceleration is **NOT default-on** in Dither Yuki 0.2.0 for the following architectural reasons:

1. **Cold Path Penalty (E3):** First-touch navigation and panning into uncached tiles is ~3× slower on GPU than CPU (~30.9 ms vs ~10.4 ms).
2. **ED Checkpoint Tax:** Any document pipeline containing Error Diffusion (a primary feature of Dither Yuki) requires falling back to CPU, eliminating GPU gains.
3. **Composite Median (E1):** On standard multi-layer composite frames, the CPU multi-threaded pool is equal to or faster than GPU readback overhead.

---

## 5. Retirement of Legacy v1 GPU Dispatch

Per **Task T9**, all legacy per-tile v1 GPU code (`dispatch_rgba32`, `gpu_bridge::try_*`, single-pass buffer uploads) has been **completely removed**.

- **Retired Semantics:** The environment variable `DITHER_GPU=1` no longer invokes legacy v1 dispatch. It is mapped to `gpu_preview_enabled()`.
- **Primary Control:** `DITHER_GPU_PREVIEW=1` or the Preferences UI toggle (`get_gpu_preview_status` / `set_gpu_preview_enabled`).
- **Safety Guarantee:** CPU path remains the bit-exact source of truth and default preview engine.
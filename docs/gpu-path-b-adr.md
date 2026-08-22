# ADR: GPU Path B — GPU-resident tiles & compute graph

Status: **Accepted (Phase 0, revised)** — implement per [`.cursor-spec/gpu-path-b/`](../.cursor-spec/gpu-path-b/SPEC.md)  
Supersedes: Path A (v1 buffer pool); does not remove v1 until preview gate  
Date: 2026-08-22 (rev. 2)

---

## Context

Track D v1 uploads/downloads each 256² tile per filter pass, serializes workers with
`submit_lock`, and loses to the CPU thread pool on real viewports. Patching v1 with
pools does not fix the model: data still crosses the bus every pass.

Path B keeps pixels **GPU-resident** between filters and readbacks **once per frame**.

CPU path remains source of truth until benchmark gate passes.

---

## Decision summary

| # | Topic | Decision |
|---|--------|----------|
| D0 | Pixel format | `Rgba32Float`, **260×260** (full halo). f16 later, separate parity ADR. |
| D1 | Tile storage | **One resident** `Texture2DArray` + slot free-list. **Frame scratch ping-pong:** two small arrays (`2 × frame_batch_cap` layers), **not** duplicate resident cache. VRAM formula: `max_slots = floor((budget − 2×cap×TILE − overhead) / TILE)`. Default 256 MiB, cap 64 → **~118 resident slots**. |
| D2 | Eviction | VRAM → CPU `TileCache` → cold. Same **`EvictContext`**. **`close_session` → `gpu_cache.evict_document(doc)`** (unconditional, symmetric with CPU). Promote = **GPU miss** → upload from CPU. |
| D3 | Graph | Linear **`ComputeGraph`** from CPU `FilterStack`. Nodes: `GpuPass` or **`CpuCheckpoint`** (ED, **`pixel_size>1` / BlockGranularity**, ineligible). |
| D4 | Threading | **`GpuExecutor`** dedicated thread; one submit/frame; no worker `submit_lock`. |
| D5 | Display (Ph.1) | **`viewport_gather`** → one RGBA8 readback/frame. CPU `tile://` until preview gate. |
| D6 | Ship gate | Bayer bit-exact; Bayer-only viewport p95 **>** CPU pool **and** v1 GPU; **realistic ED-stack benchmark before T10**; generation-safe eviction + close_session GPU evict. |
| — | Phase 2.5 | **Multi-layer GPU composite** required before preview gate (T7.5). |
| — | Non-goals | **`pixel_size > 1` on GPU** — **permanent Non-goal (option a)** until a separate block-granularity ADR. Runtime: `CpuCheckpoint(BlockGranularity)` / CPU apply. Same honesty class as ED. See industrial-gate H2. |

---

## Rejected alternatives

| Alternative | Why rejected |
|-------------|--------------|
| Path A — pool v1 `dispatch_rgba32` | Does not reuse in Path B; wasted work |
| Global `Mutex` on all workers | v1 anti-pattern; use executor thread |
| **Dual full resident arrays** (`2 × max_slots`) | Doubles VRAM vs stated budget; use frame scratch (D1b) |
| Sparse virtual texture (Phase 1) | High complexity; halo array sufficient |
| Parallel FilterStack model | Diverges from CPU semantics |
| GPU ED in Phase 1–2 | Research track; checkpoint if prototype fails |
| WebGPU preview in Phase 1 | Integration cost; readback/frame is valid architecture |
| GpuPreviewGate on Bayer-only bench | Typical stacks hit ED checkpoint; measure realistic stack (T8) |

---

## Consequences

**Positive**

- Filter stack runs in VRAM; bus traffic ∝ viewport updates, not tiles×filters.
- Industry-standard wgpu patterns (storage textures, batched encoder, readback ring).
- Explicit CPU checkpoints — ED scope bounded by design.
- VRAM math honest; tab close cannot leak GPU slots.

**Negative**

- Dedicated GPU thread + VRAM budget tuning (scratch reserve reduces resident slots).
- Dual code paths until preview gate.
- Phase 3 ED on GPU may never ship — checkpoint is permanent acceptable state.
- Typical `[Adjust] → [ED] → […]` stacks pay checkpoint tax — must benchmark before ship.

---

## Implementation sequence

```
T0 ADR → T1–T6 Phase 1 (Bayer slice) → gate
→ T7 filters → T7.5 multi-layer composite → T8 ED prototype + realistic bench
→ T7 filters → T7.5 multi-layer composite → T8 ED prototype + realistic bench
→ T9 display **no-go** ([DISPLAY_DECISION.md](../.cursor-spec/gpu-path-b/DISPLAY_DECISION.md))
→ T10 gate criteria ([PREVIEW_GATE.md](../.cursor-spec/gpu-path-b/PREVIEW_GATE.md)) — **OPT_IN ONLY**; `DITHER_GPU_PREVIEW=1` (industrial R1: no default-on)
```

Tasks: [`.cursor-spec/gpu-path-b/TASKS.md`](../.cursor-spec/gpu-path-b/TASKS.md)

---

## Industrial gate addendum (H2, 2026-08-22)

**`pixel_size > 1`:** Chosen **(a) Non-goal**, analogous to permanent ED checkpoint.  
Block-representative compute (`BlockRepresentativeCache`) is not a per-pixel shader; do not fold into resident filter PRs.  
Compiler path: `engine-project` `gpu_graph` → `CpuCheckpointKind::BlockGranularity` when `params.pixel_size > 1`.  
Option (b) (future block-granularity ADR) remains open only as a **new** ADR, not a drive-by.

VRAM H1: see [`.cursor-spec/gpu-industrial-gate/VRAM_NOTE.md`](../.cursor-spec/gpu-industrial-gate/VRAM_NOTE.md) — case **(A) frame scratch**.


# Requirements: Track D — GPU Pipeline (wgpu / WGSL)

## Introduction

Трек D из [tech-debit.md](../tech-debit.md) — **GPU pipeline** для per-tile pattern/filter ускорения. Это **строго последний** трек roadmap: CPU-референсы из A/C уже бесшовные; WGSL должен их воспроизводить, а не заново открывать швы.

**Предусловия (gate):**

| Gate | Статус / источник |
|------|-------------------|
| Трек A закрыт | ED + `pixel_size` seam matrix green — [track-a-correctness/tasks.md](../track-a-correctness/tasks.md) |
| Bayer CPU | уже есть + seam tests |
| CMYK Halftone CPU | Track C1 + `phase1_pattern_seam` |
| CRT CPU | Track C3 + seam unit |
| Glow CPU | опционально для старта D; в порядке портирования после CRT |

Error Diffusion (FS/Atkinson) **остаётся CPU-only** в этом треке (последовательная зависимость между пикселями/тайлами не мапится на независимый compute pass без отдельного дизайна).

## Glossary

- **engine-gpu**: новый workspace crate (`crates/engine-gpu`) — device lifecycle, buffers, pipelines, WGSL sources, CPU↔GPU bridge.
- **GpuContext**: обёртка над `wgpu::Device` + `wgpu::Queue` (+ adapter info); живёт в `AppState`.
- **tile_offset**: uniform `vec2<u32>` = `(tile.x * TILE_SIZE, tile.y * TILE_SIZE)` — глобальный origin тайла для шейдеров (аналог `GlobalCoord` на CPU).
- **Workgroup**: `16×16` threads; тайл core `256×256` → `16×16` workgroups на диспатч (без halo в v1 GPU path, если фильтр не требует соседей).
- **Staging / map_async**: download path GPU → CPU `PixelTile` / RGBA для кэша Processed (или documented zero-copy policy).
- **GpuEligible**: фильтр/mode, для которого есть WGSL + feature flag / capability check; иначе fallback CPU.
- **Pixel_Parity_Test**: CPU apply vs GPU apply — Bayer **exact**; Halftone/CRT **max abs ≤ 1/255** per channel (design locked).
- **map_timeout_counter**: `AtomicU64` incremented on `map_async` timeout/failure (Track A silent-path pattern).
- **D1_exit_criteria**: five checks in design.md; all green before D2/D3.
- **Pilot**: первый порт — Bayer; валидирует всю infra до Halftone/CRT/Glow.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Crate `engine-gpu` + `GpuContext` в `AppState` | Порт Error Diffusion на GPU |
| Bayer GPU pilot (staging/map_async end-to-end) | WebGL frontend rewrite / replace Canvas2D preview |
| WGSL Bayer, CMYK Halftone, CRT (+ Glow если стабилен) | Полный GPU compositor / layer blend stack |
| `tile_offset` uniform → бесшовность как CPU `GlobalCoord` | Compute для Curves/Levels (можно позже, вне DoD) |
| Feature flag / graceful CPU fallback | Обязательный GPU на всех машинах CI без adapter |
| Pixel parity + seam tests vs CPU refs | Visual regression CI infra |
| Bench: GPU vs CPU throughput на больших холстах | Multi-GPU, mobile-only backends as product goal |

---

## Requirements

### Requirement 1: Gate — Prerequisites Closed

**User Story:** As a maintainer, I want GPU work to start only when CPU Bayer/Halftone/CRT are seam-stable references, so shader bugs are measurable against known-good output.

#### Acceptance Criteria

1. BEFORE any D0 apply-path work (and before claiming gate green), THE implementer SHALL run the precondition checklist in [design.md](./design.md) (real `cargo test` for A/C seam refs, not “files exist”) and record pass/fail + date in [tasks.md](./tasks.md) §0.1.
2. BEFORE merging production GPU filter paths, Track A DoD SHALL be closed as recorded in track-a tasks.
3. BEFORE merging GPU Halftone/CRT, Track C GPU gate checklist (Bayer + Halftone + CRT seam-green) SHALL be satisfied — see [track-c-phase1-filters/tasks.md](../track-c-phase1-filters/tasks.md) §6.3 — and tasks §0.1 SHALL show **proceed**.
4. IF a prerequisite regresses, GPU PRs for that filter SHALL NOT land until CPU reference is green again (or an explicit waiver is recorded in `tech-debit.md`).
5. Infrastructure-only PRs (crate skeleton, device init, no filter dispatch) MAY land earlier but MUST NOT change default apply path away from CPU.
6. D2/D3 implementation SHALL NOT start until all five D1 exit criteria in design.md are green and recorded in tasks §2.

### Requirement 2: Crate and Device Lifecycle

**User Story:** As a developer, I want a dedicated `engine-gpu` crate and a shared device in AppState so workers can submit compute without each spawning an adapter.

#### Acceptance Criteria

1. THE workspace SHALL add `crates/engine-gpu` as a member with `wgpu` dependency (version pinned in crate/`Cargo.toml`; document MSRV/backend notes in design.md).
2. `engine-gpu` SHALL expose `GpuContext` (name MAY vary) that owns `wgpu::Device`, `wgpu::Queue`, and reports `is_available() -> bool`.
3. `AppState` SHALL hold `Option<GpuContext>` or equivalent (`Arc`/`Mutex` as needed for worker threads); init at app startup SHALL attempt adapter request and on failure leave GPU disabled without crashing the app.
4. WHEN GPU init fails (no adapter, headless CI without Vulkan/Metal/DX12), THE app SHALL continue with CPU-only filters and log a single clear warning.
5. Device loss / error paths SHALL fall back to CPU for that tile (or disable GPU for the session) — no panic in worker loop.

### Requirement 3: Compute Dispatch Contract (Tiles)

**User Story:** As a tile worker, I want a uniform GPU dispatch for a 256×256 tile so Bayer/Halftone/CRT share one buffer layout and sync path.

#### Acceptance Criteria

1. THE default workgroup size SHALL be `16×16`; dispatch for a full core tile SHALL cover `256×256` pixels (`16×16` workgroups) unless design documents a different packing.
2. Input/output buffer layout SHALL be **RGBA32 float** (tightly packed `f32×4`, core 256×256) matching `PixelTile` linear core — RGBA8 unorm SHALL NOT be used for v1 GPU I/O (see design locked decisions).
3. EVERY pattern shader SHALL receive `tile_offset: vec2<u32>` (or `vec2<i32>` if signed halo later) as a uniform equal to document origin of local `(0,0)` of the tile core — equivalent to `GlobalCoord::from_local(tile, 0, 0)`.
4. Pattern indexing in WGSL SHALL use global coords `tile_offset + local` with true modulo semantics matching CPU `rem_euclid` (document helper; do not use `%` on negative without care — v1 core-only avoids negatives).
5. THE sync path SHALL include: upload input → dispatch → copy to staging → `map_async` (or equivalent) → write into `PixelTile` / cache-compatible buffer; timeouts/errors → CPU fallback.
6. WHEN `map_async` times out or fails, THE engine SHALL increment an observable counter (e.g. `AtomicU64` on `GpuContext` / AppState, same pattern as Track A `diffusion_skip_counter`) and SHALL NOT leave the failure silent.

### Requirement 4: D1 — Bayer Pilot

**User Story:** As a developer, I want Bayer on GPU first so staging and parity infrastructure are proven before porting more complex screens.

#### Acceptance Criteria

1. THE engine SHALL provide a WGSL compute shader implementing Bayer ordered dither equivalent to CPU `dither_ordered` Bayer path for the documented param subset (levels / threshold_scale / color_mode / pixel_size policy in design.md).
2. WHEN GPU is available and Bayer is GpuEligible, tile apply MAY use GPU path behind a feature flag or runtime preference (default MAY remain CPU until parity green — document default in design.md).
3. Pixel_Parity_Test for Bayer SHALL be **exact** vs CPU on fixed fixtures (solid, gradient, multi-tile) — zero soft ε; any mismatch is a failure (design locked decision).
4. Pattern seam across tile boundary SHALL match CPU (no phase jump) — reuse 2×2 style assertion comparing GPU tiles or GPU vs CPU edge.
5. Pilot SHALL exercise the full staging/map_async path used by later ports (no special-case “test-only” download), including timeout → counter + CPU fallback coverage.
6. D1 SHALL be considered complete only when all five exit criteria in design.md are recorded green in tasks §2; D2/D3 SHALL NOT begin before that.

### Requirement 5: D2 — CMYK Halftone GPU

**User Story:** As a user with a large document and Halftone mode, I want GPU acceleration that looks identical to the CPU Halftone I already trust.

#### Acceptance Criteria

1. WGSL SHALL port CPU CMYK Halftone screen math (angles, cell size, √t radius policy) using `tile_offset` + local coords — no local-only lattice.
2. Params SHALL match CPU serde surface (same defaults); unsupported param combinations SHALL fall back to CPU rather than silent wrong output.
3. Pixel_Parity_Test vs CPU Halftone SHALL pass with **max abs per channel ≤ 1/255** (linear f32); the constant SHALL appear in the test (systematic bias not allowed).
4. 2×2 seam test SHALL pass for GPU Halftone (edge continuity).
5. Palette quantization AFTER reconstruct: either CPU post-pass, or GPU LUT texture — policy in design.md; if GPU LUT deferred, post-pass CPU LUT is acceptable for v1.
6. Work SHALL start only after D1 exit criteria 1–5 are green.

### Requirement 6: D3 — CRT (and Glow) GPU

**User Story:** As a user enabling CRT, I want scanlines continuous across tiles on the GPU path the same way they are on CPU.

#### Acceptance Criteria

1. CRT WGSL SHALL modulate using global `Y` (and `X` for mask) derived from `tile_offset`, never `local_y` alone.
2. Pixel_Parity_Test vs CPU CRT SHALL use **max abs per channel ≤ 1/255**; horizontal boundary seam test SHALL pass.
3. Glow GPU MAY be ported after CRT; IF ported, radius policy SHALL match CPU (≤ HALO or explicit multi-pass with neighbor tiles — document); IF neighbor tiles required and not implemented, Glow SHALL stay CPU-only.
4. Glow/CRT GpuEligible gating SHALL be independent (CRT can ship GPU while Glow remains CPU).
5. Work SHALL start only after D1 exit criteria 1–5 are green.

### Requirement 7: Eligibility, Fallback, and ED Exclusion

**User Story:** As a user on a machine without GPU or with FS dither, I want correct results without caring which backend ran.

#### Acceptance Criteria

1. WHEN mode is Error Diffusion (FS/Atkinson) or `requires_full_row == true`, THE apply path SHALL NOT use GPU compute for that filter instance (CPU only).
2. WHEN GPU is unavailable or shader pipeline failed to create, THE apply path SHALL use existing CPU filters with identical user-visible params.
3. A single documented switch (env, settings, or compile feature) SHALL allow forcing CPU for debugging parity.
4. Mixing GPU and CPU tiles in one document for the same filter revision SHALL still be generation-consistent (same params → same pixels within tolerance regardless of which tiles hit GPU — prefer deterministic choice per session: all-GPU or all-CPU for eligible filters).

### Requirement 8: Integration with Tile Pipeline

**User Story:** As a tile worker, I want GPU dispatch plugged into `compute_processed_tile` / filter apply without breaking scheduler semantics.

#### Acceptance Criteria

1. GPU path SHALL produce Processed tiles compatible with existing cache keys / generations (no separate “gpu cache” that desyncs invalidation).
2. Worker threads SHALL safely share `GpuContext` (wgpu queue submit rules documented; serialize submits via mutex if required).
3. Existing CPU prop/integration tests SHALL remain green with GPU default-off or in environments without adapter.
4. Optional: CI job with GPU MAY run parity tests; CPU-only CI SHALL still compile `engine-gpu` and run unit tests that do not require an adapter (shader compile-at-runtime tests gated).

### Requirement 9: Documentation and tech-debit

**User Story:** As a future maintainer, I want the GPU contract (uniforms, fallback, port order) written down next to the tile pipeline docs.

#### Acceptance Criteria

1. `TILE_PIPELINE.md` and/or `ARCHITECTURE.md` SHALL document GPU eligible filters, `tile_offset` contract, and CPU fallback.
2. `tech-debit.md` Track D SHALL link this folder; item “GPU pipeline” MAY be marked done only when DoD in tasks.md is checked.
3. Port order in docs SHALL remain: Bayer → Halftone → CRT → Glow (optional).

### Requirement 10: Testing and Performance

**User Story:** As a reviewer, I want parity/seam tests and a rough throughput note so “GPU works” is not only a screenshot.

#### Acceptance Criteria

1. Unit/integration: Bayer **exact** parity; Halftone/CRT parity at **1/255**; seam 2×2 for each GPU-ported pattern filter; map_async timeout counter coverage.
2. Bench or timed note: GPU vs CPU on a large buffer/document for Bayer (and ideally Halftone) recorded in tasks.md results.
3. Manual QA checklist in tasks.md: enable GPU, pan across tile boundaries on Halftone/CRT, toggle force-CPU and confirm match.
4. Full visual CI NOT required.

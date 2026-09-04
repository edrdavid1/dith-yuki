# Documentation

As-built notes for Dither Yuki **0.2.0**. Start with the [root README](../README.md).

**Канон:** `architecture.md` + `tile-pipeline.md` + `FLEXLAYOUT_DOCKING.md` + `gpu-as-built.md`.  
Спеки и evidence живут в `.cursor-spec/`, не дублируются здесь.

| File | What it covers |
|---|---|
| [architecture.md](./architecture.md) | Stack, crates, IPC, preview, cost model |
| [multi-doc-tabs.md](./multi-doc-tabs.md) | Tabs, multi-project sessions, shared cache, save/export |
| [tile-pipeline.md](./tile-pipeline.md) | 256×256 tiles, global coords, ED, GPU routing |
| [gpu-as-built.md](./gpu-as-built.md) | Path B resident GPU, auto-dispatch, industrial verdict |
| [gpu-path-b-adr.md](./gpu-path-b-adr.md) | ADR: Path B closed decisions |
| [KANBAN_gpu_and_docking.md](./KANBAN_gpu_and_docking.md) | Track A (GPU) + Track B (docking) |
| [FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md) | FlexLayout as-built (Layers / Effect / Color Lab, B4c) |
| [B2_ADR_flexlayout_docking.md](./B2_ADR_flexlayout_docking.md) | Why FlexLayout |
| [legal/flexlayout-license-snapshot.md](./legal/flexlayout-license-snapshot.md) | MIT LICENSE snapshot for 0.7.15 |
| [palette-dither.md](./palette-dither.md) | Bound palette vs dither filter |
| [color-lab.md](./color-lab.md) | Palette data, Oklab, generators, Color Lab UI |
| [dither_yuki_0.2.0_overview.md](./dither_yuki_0.2.0_overview.md) | What’s real vs still beta |
| [CONTRIBUTING.md](./CONTRIBUTING.md) | Local setup, tests, style |

`.cursor-spec/` entry points:

| Spec dir | Topic |
|---|---|
| [`gpu-path-b/`](../.cursor-spec/gpu-path-b/REPORT.md) | Path B report; cold compute **OPT_IN** |
| [`gpu-industrial-gate/`](../.cursor-spec/gpu-industrial-gate/REPORT.md) | Industrial evidence + R1 |
| [`gpu-auto-dispatch/`](../.cursor-spec/gpu-auto-dispatch/SPEC.md) | A1–A4 |
| [`gpu-export-a5/`](../.cursor-spec/gpu-export-a5/SPEC.md) | A5 GPU export — **NO-GO** |
| [`gpu-vram-a8/`](../.cursor-spec/gpu-vram-a8/SPEC.md) | A8 occupancy (f16 not started) |
| [`track-r-docking/`](../.cursor-spec/track-r-docking/B4c_js_popout_drag_spec.md) | B4c JS popout drag |

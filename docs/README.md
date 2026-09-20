# Documentation

As-built для Dither Yuki **0.2.0**. Старт: [root README](../README.md).

**Канон:** `architecture.md` · `tile-pipeline.md` · `FLEXLAYOUT_DOCKING.md` · `gpu-as-built.md`.

Рабочие спеки (не as-built) — [`.cursor-spec/README.md`](../.cursor-spec/README.md). Не дублировать TZ в `docs/`.

| File | What it covers |
|---|---|
| [architecture.md](./architecture.md) | Stack, crates, IPC, preview, cost model |
| [multi-doc-tabs.md](./multi-doc-tabs.md) | Tabs, sessions, shared cache, save/export |
| [tile-pipeline.md](./tile-pipeline.md) | 256×256 tiles, coords, ED, GPU routing |
| [gpu-as-built.md](./gpu-as-built.md) | Path B resident GPU, auto-dispatch |
| [gpu-path-b-adr.md](./gpu-path-b-adr.md) | ADR: Path B decisions |
| [FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md) | Layers / Effect / Color Lab docking |
| [B2_ADR_flexlayout_docking.md](./B2_ADR_flexlayout_docking.md) | Why FlexLayout |
| [legal/flexlayout-license-snapshot.md](./legal/flexlayout-license-snapshot.md) | MIT snapshot for 0.7.15 |
| [palette-dither.md](./palette-dither.md) | Bound palette vs dither filter |
| [color-lab.md](./color-lab.md) | Palettes, Oklab, Color Lab UI |
| [dither_yuki_0.2.0_overview.md](./dither_yuki_0.2.0_overview.md) | What’s real vs still beta |
| [KANBAN_gpu_and_docking.md](./KANBAN_gpu_and_docking.md) | Open work only |
| [CONTRIBUTING.md](./CONTRIBUTING.md) | Local setup, tests, style |
| [RELEASE.md](./RELEASE.md) | Tags, updater secrets, notarization, public alpha |
| [HOW_TO_ADD_ALGORITHM.md](./HOW_TO_ADD_ALGORITHM.md) | Built-in `FilterAlgorithm` checklist |
| [TRACK_E_extensibility_architecture.md](./TRACK_E_extensibility_architecture.md) | Track E design (registry + format versioning) |

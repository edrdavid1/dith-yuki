# Documentation (developers)

As-built notes for Dither Yuki. Product overview and install: [root README](../README.md).

**Canon (read these first):** [`architecture.md`](./architecture.md) · [`tile-pipeline.md`](./tile-pipeline.md) · [`FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md) · [`gpu-as-built.md`](./gpu-as-built.md)

Working specs (not as-built) live under [`.cursor-spec/`](../.cursor-spec/README.md) locally — do not duplicate TZ into `docs/`.

## Dev

| Doc | Contents |
|---|---|
| [dev-setup.md](./dev-setup.md) | Setup, tests, conventions |
| [RELEASE.md](./RELEASE.md) | Tags, updater secrets, notarization, alpha |
| [HOW_TO_ADD_ALGORITHM.md](./HOW_TO_ADD_ALGORITHM.md) | Built-in `FilterAlgorithm` checklist |

## As-built

| Doc | Contents |
|---|---|
| [architecture.md](./architecture.md) | Stack, crates, IPC, preview, cost model |
| [multi-doc-tabs.md](./multi-doc-tabs.md) | Tabs, sessions, shared cache, save/export |
| [tile-pipeline.md](./tile-pipeline.md) | 256×256 tiles, coords, ED, GPU routing |
| [gpu-as-built.md](./gpu-as-built.md) | Path B resident GPU, auto-dispatch |
| [FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md) | Layers / Effect / Color Lab docking |
| [palette-dither.md](./palette-dither.md) | Bound palette vs dither filter |
| [color-lab.md](./color-lab.md) | Palettes, Oklab, Color Lab UI |
| [dither_yuki_0.2.0_overview.md](./dither_yuki_0.2.0_overview.md) | What’s real vs still beta |

## Design decisions & backlog

| Doc | Contents |
|---|---|
| [gpu-path-b-adr.md](./gpu-path-b-adr.md) | ADR: Path B decisions |
| [B2_ADR_flexlayout_docking.md](./B2_ADR_flexlayout_docking.md) | Why FlexLayout |
| [TRACK_E_extensibility_architecture.md](./TRACK_E_extensibility_architecture.md) | Registry + format versioning |
| [KANBAN_gpu_and_docking.md](./KANBAN_gpu_and_docking.md) | Open work only |
| [legal/flexlayout-license-snapshot.md](./legal/flexlayout-license-snapshot.md) | MIT snapshot for flexlayout-react 0.7.15 |

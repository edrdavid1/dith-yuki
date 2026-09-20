# Documentation (developers)

As-built notes for Dither Yuki. Product overview and install: [root README](../README.md).

**Canon (read these first):** [`architecture.md`](./architecture.md) · [`tile-pipeline.md`](./tile-pipeline.md) · [`FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md) · [`gpu-as-built.md`](./gpu-as-built.md) · [`crash-recovery.md`](./crash-recovery.md)

Working agent specs stay local only (not in this repo on GitHub).

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
| [crash-recovery.md](./crash-recovery.md) | Atomic save, journals, clean-exit, soft discard, roster |
| [tile-pipeline.md](./tile-pipeline.md) | 256×256 tiles, coords, ED, GPU routing |
| [gpu-as-built.md](./gpu-as-built.md) | Path B resident GPU, auto-dispatch |
| [FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md) | Layers / Effect / Color Lab docking |
| [palette-dither.md](./palette-dither.md) | Bound palette vs dither filter |
| [color-lab.md](./color-lab.md) | Palettes, Oklab, Color Lab UI |
| [dither_yuki_0.2.0_overview.md](./dither_yuki_0.2.0_overview.md) | What’s real vs still beta |
| [legal/flexlayout-license-snapshot.md](./legal/flexlayout-license-snapshot.md) | MIT snapshot for flexlayout-react 0.7.15 |

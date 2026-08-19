# Dither Yuki

Desktop studio for **dithering**, palettes, and layered pixel-art images. Tile-based preview keeps large documents responsive.

Built with **Rust** (Tauri 2) and **React**. Current version: **0.2.0**.

[Features](#features) · [Install](#install) · [Develop](#develop) · [Docs](./docs/README.md) · [License](#license)

## Features

- Ordered dithering (Bayer, halftone, custom threshold maps) and error diffusion (Floyd–Steinberg, Atkinson, and others)
- Palette quantization with Color Lab (Oklab, ramps, harmony, import ASE/GPL/…)
- Palette dither modes: Strict, Guided, Mixed, Simple
- Non-destructive layers, blend modes, undo/redo
- Projects (`.dyproj`) and shareable patterns (`.dyuki`)
- Dockable / floating panels
- In-app updates from GitHub Releases (from 0.2.0)

## Install

macOS builds are published on [GitHub Releases](https://github.com/edrdavid1/dith-yuki/releases). After 0.2.0, Help → Check for Updates pulls `latest.json` (Minisign-verified).

**From source**

- Rust (stable) via [rustup](https://rustup.rs/)
- Node.js 18+
- macOS 10.15+, Windows 10+, or a recent Linux distro with WebKitGTK (Tauri)

```bash
git clone https://github.com/edrdavid1/dith-yuki.git
cd dith-yuki
npm run setup
npm run tauri:dev
```

Production bundle:

```bash
npm run tauri:build
```

Artifacts land under `src-tauri/target/release/bundle/` (DMG on macOS).

Optional GPU path for some ordered filters: `DITHER_GPU=1`.

## Develop

```bash
npm run tauri:dev          # Vite + Tauri
cargo test --all           # Rust tests
npm test --prefix frontend # Vitest
cargo fmt --all
cargo clippy --all -- -D warnings
```

See [Contributing](./docs/CONTRIBUTING.md) for style and workflow.

## Repository

```
src-tauri/          # Tauri app: IPC, workers, tile://, menus, panels
crates/
  engine-project/   # Document, layers, filters, compositor, .dyproj / .dyuki
  engine-tiles/     # Tile cache, coords, scheduler
  engine-color/     # Oklab, palettes, KD-tree, LUT
  engine-gpu/       # Optional wgpu compute (opt-in)
  engine-io/        # Image decode / encode
  engine-core/      # Legacy stub
frontend/           # React + Redux Toolkit UI
docs/               # Architecture and contributor guides
```

## Docs

| Document | Contents |
|---|---|
| [docs/architecture.md](./docs/architecture.md) | As-built system map |
| [docs/tile-pipeline.md](./docs/tile-pipeline.md) | Tiles, coordinates, error-diffusion wavefront, GPU |
| [docs/palette-dither.md](./docs/palette-dither.md) | Strict / Guided / Mixed / Simple |
| [docs/color-lab.md](./docs/color-lab.md) | Palettes and Color Lab |
| [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md) | Setup and conventions |

## License

Copyright © 2026 L'eco non di Bergamo.

Source is available under the **Fair Core License 1.0** (MIT future license). See [LICENSE](./LICENSE).

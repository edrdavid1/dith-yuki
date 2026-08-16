# Dither — Tile-Based Image Processing Engine

Dither is a cross-platform desktop application for artistic image processing with pixel-perfect rendering and instant UI feedback. Built with **Rust** (backend) and **React** (frontend) using Tauri 2.

## Overview

Dither processes images through a tile-based rendering architecture, enabling:
- **Dithering algorithms** (ordered, error diffusion, Bayer matrices)
- **Color manipulation** (quantization, hue shifts, ICC profiles)
- **Glitch effects** (pixel sorting, color channel shifts)
- **Real-time preview** (instant feedback as parameters change)
- **Project management** (save/load, undo/redo, layer stacks)
- **Video processing** (frame extraction, batch processing)

**Tech Stack**:
- **Frontend**: React 18 + TypeScript 5, Vite 4.4
- **Backend**: Rust (Tauri 2.11)
- **Desktop Runtime**: Tauri + WebView
- **Build System**: Cargo + npm

## Quick Start

### Prerequisites

- **Rust 1.70+** — [Install from rustup.rs](https://rustup.rs/)
- **Node.js 18+** — [Install from nodejs.org](https://nodejs.org/)
- **macOS 10.15+**, **Windows 10+**, or **Linux (Ubuntu 20.04+)**

### Installation and Build

```bash
# Clone the repository
git clone https://github.com/yourusername/dither-yuki-2.git
cd dither-yuki-2

# Install dependencies
npm install

# Build all Rust crates
cargo build --all

# (Optional) Run tests
cargo test --all
```

### Development Mode

```bash
# Integrated development: starts Vite + Tauri together
npm run tauri:dev
```

**Manual two-terminal setup**:

Terminal 1 (Frontend dev server with hot-reload):
```bash
npm run dev --workspace=frontend
```

Terminal 2 (Tauri application):
```bash
npm run tauri:dev
```

The application window will open with:
- Title: **"Dither"**
- Size: **1024×768** (resizable)
- Frontend: React UI rendering "Dither Editor"

### Production Build

```bash
# Build optimized production bundle
npm run tauri:build

# Find the binary in:
# macOS: target/release/bundle/dmg/Dither.dmg
# Linux: target/release/bundle/deb/dither_*.deb
# Windows: target/release/bundle/msi/Dither_*.msi
```

### Beta updates (0.2.0+)

The first updater-capable build is **0.2.0**. Copies of 0.1.0 must install a DMG once; after that, Help → Check for Updates (and a launch prompt in release builds) pulls `latest.json` from GitHub Releases.

Updates are Minisign-verified (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`). Tag `v*` runs `.github/workflows/release.yml`, which fails closed without `TAURI_SIGNING_PRIVATE_KEY`. Apple code signing / notarization is optional; Gatekeeper may warn on the first unsigned DMG open.

GPU filters stay opt-in: `DITHER_GPU=1`.

## Project Structure

```
dither-yuki-2/
├── crates/                          # Rust backend modules
│   ├── app/                         # Tauri wrapper + main entry point
│   │   ├── src/main.rs              # Tauri app initialization
│   │   ├── tauri.conf.json          # Tauri configuration
│   │   └── Cargo.toml
│   │
│   ├── engine-core/                 # Core data model (Phase 2)
│   │   ├── src/lib.rs               # Layer, Document, Filter, BlendMode
│   │   └── Cargo.toml
│   │
│   ├── engine-tiles/                # Tile cache & pyramid (Phase 1)
│   │   ├── src/lib.rs               # TileKey, TileCache, TileBounds
│   │   └── Cargo.toml
│   │
│   ├── engine-color/                # Color pipeline (Phase 5+)
│   │   ├── src/lib.rs               # ICC profiles, LUT, color space
│   │   └── Cargo.toml
│   │
│   ├── engine-io/                   # File I/O & codecs (Phase 4+)
│   │   ├── src/lib.rs               # PNG, JPEG, WebP, video decoding
│   │   └── Cargo.toml
│   │
│   └── engine-project/              # Project storage (Phase 6+)
│       ├── src/lib.rs               # SQLite, undo/redo, project format
│       └── Cargo.toml
│
├── frontend/                        # React + TypeScript UI
│   ├── src/
│   │   ├── main.tsx                 # React entry point
│   │   ├── App.tsx                  # Root component
│   │   └── index.css                # Global styles
│   ├── dist/                        # Production bundle (generated)
│   ├── index.html                   # HTML entry point
│   ├── package.json                 # npm dependencies
│   ├── tsconfig.json                # TypeScript configuration
│   ├── vite.config.ts               # Vite build configuration
│   └── node_modules/                # npm dependencies (gitignored)
│
├── docs/                            # Architecture and guides
│   ├── TAURI_INTEGRATION.md          # Frontend-backend integration
│   ├── CONTRIBUTING.md              # Development guidelines
│   ├── BUILD_VERIFICATION_SUMMARY.md # Build status and artifacts
│   ├── tile-engine-architecture.md  # Tile caching design (external)
│   ├── tauri-api-document-model.md  # Document model + API (external)
│   └── agent-kickoff-plan.md        # Phased roadmap (external)
│
├── target/                          # Build artifacts (gitignored)
│   ├── debug/                       # Debug binaries
│   ├── release/                     # Release binaries
│   ├── doc/                         # Generated documentation
│   └── ...
│
├── Cargo.toml                       # Rust workspace definition
├── Cargo.lock                       # Dependency lock file
├── package.json                     # Root npm scripts
├── .gitignore                       # Git ignore rules
├── README.md                        # This file
│
└── Architecture References (external files):
    ├── tile-engine-architecture.md
    ├── tauri-api-document-model.md
    └── agent-kickoff-plan.md
```

## Architecture

As-built (0.2.0): **[ARCHITECTURE.md](./ARCHITECTURE.md)**. Tile math and ED/GPU:
**[TILE_PIPELINE.md](./TILE_PIPELINE.md)**. Палитровый дизер (Strict / Guided / Mixed / Simple):
**[PALETTE_DITHER.md](./PALETTE_DITHER.md)**. Оптимизация — ARCHITECTURE §13.

Preview is Canvas2D + `tile://` push. GPU (`engine-gpu`) is optional wgpu compute
for Bayer/Halftone/CRT (`DITHER_GPU=1`), not the display path.

### High-Level Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Dither Application                   │
├─────────────────────────────────────────────────────────┤
│  React (RTK)  AppLayout / TileCanvas / Color Lab        │
│       ↕  Tauri IPC + tile:// + tile-ready events        │
│  src-tauri    AppState, workers, protocol, undo         │
│       ↕                                                 │
│  engine-project / tiles / color / gpu / io              │
└─────────────────────────────────────────────────────────┘
```

Старый phase-план (0–7) выполнен треками A–P. Не использовать таблицу
«Phase 1 Next» как статус.

## Development Workflow

### Building Individual Crates

```bash
# Build one crate
cargo build -p engine-tiles

# Build with release optimizations
cargo build -p dither --release

# Run tests for a crate
cargo test -p engine-core

# Generate docs for a crate
cargo doc -p engine-tiles --open
```

### Code Quality

```bash
# Format Rust code
cargo fmt --all

# Run linter with strict rules
cargo clippy --all -- -D warnings

# Type-check TypeScript
npm run build --workspace=frontend

# Run all tests
cargo test --all
```

### Debugging

**Rust Backend**:
```bash
# Enable debug logging
RUST_LOG=debug npm run tauri:dev

# Debug info in DevTools console
```

**Frontend**:
```bash
# Open browser DevTools (F12)
# Check console for JavaScript errors
# Use React DevTools extension for component inspection
```

## Architecture Documentation

1. **[ARCHITECTURE.md](./ARCHITECTURE.md)** — as-built система. **§13 = стоимость тайла / рычаги оптимизации.**
2. **[TILE_PIPELINE.md](./TILE_PIPELINE.md)** — координаты, ED wavefront, GPU contract, **§11 cost model.**
3. **[PALETTE_DITHER.md](./PALETTE_DITHER.md)** — Strict / Guided / Mixed / Simple и цвет на дизере.
4. **[COLOR_AND_COLOR_LAB.md](./COLOR_AND_COLOR_LAB.md)** — палитры и Color Lab.
5. **[.cursor-spec/](./.cursor-spec/)** — треки A–P (requirements / design / tasks).
6. **[docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md)** — setup / style / tests.

## System Requirements

### Minimum

- **CPU**: Dual-core 2.0 GHz (Intel/AMD/Apple Silicon)
- **RAM**: 4 GB
- **Storage**: 500 MB (including dependencies)
- **Display**: 1024×768 minimum

### Recommended

- **CPU**: Quad-core 2.5 GHz or better
- **RAM**: 8 GB+
- **Storage**: 2 GB+ (for video processing)
- **Display**: 1440×900+, 60 Hz refresh rate

### Supported Platforms

- **macOS**: 10.15+ (Intel/ARM64)
- **Windows**: 10+ (x86_64)
- **Linux**: Ubuntu 20.04+, Fedora 35+, other distros with glibc 2.29+

## Dependencies

### Rust Ecosystem

- **Tauri 2.11**: Desktop application framework
- **Tokio 1.53**: Async runtime
- **Rayon 1.7**: Parallel processing
- **DashMap 5.5**: Concurrent hash map (for tile cache)
- **Crossbeam 0.5**: Multi-threading utilities
- **Serde 1.0**: Serialization framework

### Frontend

- **React 18.2**: UI framework
- **TypeScript 5.0**: Type-safe JavaScript
- **Vite 4.4**: Build tool and dev server

### System

- **Rust 1.70+**: Backend language
- **Node.js 18+**: Frontend tooling
- **npm 8+**: Package manager

## Performance Notes

Карта bottleneck'ов (не Phase-0 цифры): **[ARCHITECTURE.md §13](./ARCHITECTURE.md)**.

Кратко:
- Тайл = **1.03 MB** f32; стек фильтров копирует его целиком.
- GPU opt-in и сериализован (`submit_lock`) — часто не быстрее CPU-пула на viewport.
- Error diffusion ломает параллелизм (wavefront left/top/diag).
- Zoom-out всё ещё level 0 (крупнейший будущий win).
- Уже сделано: PaletteLut3D, SIMD blend/levels/u8, WorkerWake Condvar.

## Troubleshooting

### Build Issues

**"cargo: command not found"**
- Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

**"npm: command not found"**
- Install Node.js from [nodejs.org](https://nodejs.org/)

**"Port 5173 already in use"**
- Kill the process: `lsof -ti:5173 | xargs kill -9`
- Or use a different port: `npm run dev -- --port 5174`

### Runtime Issues

**"Tauri window won't open"**
- Check frontend dist exists: `ls frontend/dist/`
- Rebuild: `npm run build --workspace=frontend && npm run tauri:dev`

**"Blank window/404 errors"**
- Check browser console (F12) for errors
- Verify `vite.config.ts` has `base: './'`
- Clear build cache: `cargo clean && npm run build`

**"Hot-reload not working"**
- Ensure Vite dev server is running: `npm run dev --workspace=frontend`
- Check terminal 1 shows "Local: http://localhost:5173"

See **[TAURI_INTEGRATION.md](./docs/TAURI_INTEGRATION.md#common-issues)** for more troubleshooting.

## Contributing

See **[CONTRIBUTING.md](./docs/CONTRIBUTING.md)** for:
- Development setup
- Code style guidelines
- Testing requirements
- Commit message format

## License

[To be determined — update as project matures]

## References

- **Tauri Docs**: https://tauri.app/docs
- **Rust Book**: https://doc.rust-lang.org/book
- **React Docs**: https://react.dev
- **Vite Docs**: https://vitejs.dev

## Status

As-built **0.2.0** (Beta 1 in tree). Треки A–N в коде; O/P — updates + dirty/Guard.
Пиксельный paint / pyramid level>0 / GPU v2 — открытый долг, см. ARCHITECTURE §14.

---

**Last Updated**: 14 August 2026
**Version**: 0.2.0

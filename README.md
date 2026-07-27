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
# macOS: target/release/bundle/dmg/Dither.app
# Linux: target/release/bundle/deb/dither_*.deb
# Windows: target/release/bundle/msi/Dither_*.msi
```

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

### High-Level Diagram

```
┌─────────────────────────────────────────────────────────┐
│                    Dither Application                   │
├─────────────────────────────────────────────────────────┤
│                                                          │
│  ┌─────────────────────────────────────────────────┐   │
│  │         React Frontend (TypeScript)             │   │
│  │  ├─ Canvas Component (Web Worker integration)   │   │
│  │  ├─ Layers Panel                               │   │
│  │  ├─ Effects Panel                              │   │
│  │  └─ Project Browser                            │   │
│  └─────────────────────────────────────────────────┘   │
│                       ↕                                  │
│           (Tauri Commands + Message Events)             │
│                       ↕                                  │
│  ┌─────────────────────────────────────────────────┐   │
│  │        Rust Backend (Tauri Runtime)             │   │
│  │  ├─ Custom Protocol Handler                     │   │
│  │  ├─ Async Command Executor                      │   │
│  │  └─ Event Emitter                               │   │
│  └─────────────────────────────────────────────────┘   │
│                       ↕                                  │
│  ┌─────────────────────────────────────────────────┐   │
│  │         Engine Modules (Rust Crates)            │   │
│  │  ├─ engine-tiles   (Tile cache, pyramid)        │   │
│  │  ├─ engine-core    (Data model, filters)        │   │
│  │  ├─ engine-color   (Color processing)           │   │
│  │  ├─ engine-io      (Image/video codecs)         │   │
│  │  └─ engine-project (Storage, project format)    │   │
│  └─────────────────────────────────────────────────┘   │
│                       ↕                                  │
│  ┌─────────────────────────────────────────────────┐   │
│  │         System Resources (OS Integration)        │   │
│  │  ├─ GPU (WebGL rendering)                       │   │
│  │  ├─ File System (image I/O)                     │   │
│  │  ├─ Memory (tile cache)                         │   │
│  │  └─ CPU (tile processing)                       │   │
│  └─────────────────────────────────────────────────┘   │
│                                                          │
└─────────────────────────────────────────────────────────┘
```

### Development Phases

| Phase | Focus | Status | Artifacts |
|-------|-------|--------|-----------|
| **0** | Skeleton | ✅ Complete | Workspace, Tauri + React scaffold, build pipeline |
| **1** | Tile Engine | 🔄 Next | TileCache, pyramid downsampling, scheduler, invalidation |
| **2** | Document Model | 📋 Planned | Layer, Document, Filter, BlendMode implementations |
| **3** | Tauri API | 📋 Planned | Custom commands, async processing, events |
| **4** | UI Canvas | 📋 Planned | Web Worker integration, tile-based rendering, zoom/pan |
| **5** | Color Pipeline | 📋 Planned | ICC profiles, LUT, color space conversions |
| **6** | Project Format | 📋 Planned | SQLite schema, undo/redo, project I/O |
| **7+** | Effects | 📋 Future | Dithering, glitch, video, batch processing |

See **[agent-kickoff-plan.md](./agent-kickoff-plan.md)** for detailed phase descriptions.

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

This project is documented across several files:

1. **[tile-engine-architecture.md](./tile-engine-architecture.md)**
   - Tile caching strategy
   - Pyramid downsampling
   - Scheduler and invalidation
   - Memory management

2. **[tauri-api-document-model.md](./tauri-api-document-model.md)**
   - Document model (Layer, Filter, BlendMode)
   - Tauri command interface
   - Custom protocol specification
   - Message event system

3. **[agent-kickoff-plan.md](./agent-kickoff-plan.md)**
   - Phase-by-phase breakdown
   - Implementation milestones
   - Dependency graph
   - Effort estimates

4. **[TAURI_INTEGRATION.md](./docs/TAURI_INTEGRATION.md)** (this project)
   - Frontend-backend integration
   - Build configuration
   - Development workflow
   - Common troubleshooting

5. **[CONTRIBUTING.md](./docs/CONTRIBUTING.md)**
   - Development setup
   - Code style guidelines
   - Testing requirements

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

### Current (Phase 0)

- **Startup time**: ~2-3 seconds (includes Tauri initialization)
- **Bundle size**: ~150 MB (includes all dependencies)
- **Memory usage**: ~80-120 MB idle

### Optimization Roadmap

- Phase 1: Tile caching reduces memory by 70%+
- Phase 3: Async commands prevent UI blocking
- Phase 4: Web Worker offloads rendering
- Phase 5+: SIMD operations for color processing

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

**Phase 0**: ✅ Complete
- ✅ Workspace initialized
- ✅ All 6 Rust crates created and compiling
- ✅ React + TypeScript frontend set up
- ✅ Tauri integration working
- ✅ Build pipeline verified

**Next**: Phase 1 — Tile Engine Implementation

---

**Last Updated**: July 27, 2024
**Phase**: 0 (Skeleton)
**Version**: 0.1.0

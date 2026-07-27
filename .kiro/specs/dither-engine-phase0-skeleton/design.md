# Design: Dither Engine Phase 0 — Skeleton

## Architectural Overview

**Goal**: Establish the build and project structure that will support the tile-based rendering engine without implementing any rendering logic.

**Key decisions**:
1. **Monorepo with Cargo workspace** — all Rust crates compiled together, single dependency lock
2. **Separation of concerns via crate boundaries** — compiler enforces that frontend code doesn't access raw pixels, color pipeline doesn't depend on Tauri
3. **Tauri 2 + React + TypeScript** — minimal framework overhead, native performance
4. **Stub-first approach** — create placeholder types now, fill implementations in subsequent phases

## System Structure

```
dither-yuki-2/
├── Cargo.workspace.toml         (root workspace config)
├── Cargo.lock                   (pinned dependencies)
├── README.md                    (quickstart + overview)
├── /crates                      (Rust workspace members)
│   ├── /app                     (Tauri wrapper)
│   │   ├── src/main.rs          (entry point, protocol handler stubs)
│   │   ├── Cargo.toml           (depends on engine-core, engine-tiles, tauri 2)
│   │   └── tauri.conf.json      (window config, protocol registration)
│   ├── /engine-core             (pure data model, no Tauri/async)
│   │   ├── src/lib.rs           (Layer, Document, Filter, BlendMode stubs)
│   │   └── Cargo.toml           (no engine-* dependencies)
│   ├── /engine-tiles            (TileCache, pyramid, scheduler)
│   │   ├── src/lib.rs           (TileKey, TileBounds, TileCache stubs)
│   │   ├── src/cache.rs         (stub: pub struct TileCache { })
│   │   └── Cargo.toml           (depends on engine-core; rayon, dashmap, crossbeam-channel)
│   ├── /engine-color            (ICC/LUT — placeholder)
│   │   ├── src/lib.rs           (module-level doc with TODO)
│   │   └── Cargo.toml           (no dependencies yet)
│   ├── /engine-io               (image codecs, video — placeholder)
│   │   ├── src/lib.rs           (module-level doc with TODO)
│   │   └── Cargo.toml           (no dependencies yet)
│   └── /engine-project          (SQLite storage — placeholder)
│       ├── src/lib.rs           (module-level doc with TODO)
│       └── Cargo.toml           (no dependencies yet)
├── /frontend                    (React + TypeScript)
│   ├── package.json             (React 18, TypeScript, Vite)
│   ├── tsconfig.json            (strict mode)
│   ├── vite.config.ts           (build config)
│   ├── index.html               (<div id="root"></div> + script)
│   ├── src/
│   │   ├── main.tsx             (React entry, renders App component)
│   │   ├── App.tsx              (<h1>Dither Editor</h1> + stubs for future panels)
│   │   └── styles/              (minimal CSS)
│   └── dist/                    (build output, copied by Tauri to static asset folder)
└── /docs
    ├── tile-engine-architecture.md
    ├── tauri-api-document-model.md
    └── agent-kickoff-plan.md
```

## Crate Dependency Graph

```
┌─────────────┐
│ app (Tauri) │  ← knows about all others, glues them together
└────┬────────┘
     │
     ├─→ engine-core (pure types)
     ├─→ engine-tiles (depends on engine-core)
     ├─→ engine-color (stub)
     ├─→ engine-io (stub)
     └─→ engine-project (stub)

engine-core, engine-color, engine-io, engine-project
   ↑
   └─→ NO cross-dependencies among themselves (or only downward)

engine-tiles
   └─→ engine-core only
```

**Invariant**: `engine-core` has zero engine-internal dependencies. This allows it to be used in e.g. a CLI tool or tests without pulling in Tauri/Tauri-async code.

## Dependency Versions (Pinned in Cargo.lock)

| Crate | Version | Rationale |
|-------|---------|-----------|
| tauri | 2.x | Latest stable; WebView, custom protocols |
| tokio | 1.x | Async runtime, required by Tauri |
| serde | 1.x | Serialization (DTOs to frontend) |
| serde_json | 1.x | JSON encoding for Tauri commands |
| rayon | 1.x | Work-stealing thread pool (Phase 1) |
| dashmap | 5.x | Concurrent HashMap for tile cache (Phase 1) |
| crossbeam-channel | 0.5.x | MPMC queue for scheduler (Phase 1) |
| parking_lot | 0.x | Efficient Mutex/RwLock (Phase 1+) |
| image | 0.24.x | Codec stubs (Phase 4+) |
| lcms2 | 2.x | ICC color management (Phase 5+) |
| sqlx or rusqlite | latest | ORM/bindings for SQLite (Phase 5+) |

**Phase 0 rule**: Only add crates that are needed for basic compilation and testing. Phase 1 will add parallel/caching crates; Phase 3 will add codecs.

## Tauri Configuration (tauri.conf.json)

Key sections:
```json
{
  "build": {
    "beforeBuildCommand": "npm run build",
    "devPath": "http://localhost:5173",
    "frontendDist": "../frontend/dist"
  },
  "app": {
    "windows": [
      {
        "label": "main",
        "title": "Dither",
        "width": 1024,
        "height": 768,
        "resizable": true,
        "fullscreen": false
      }
    ]
  }
}
```

**Custom protocol handler** (registered in `main.rs`, Phase 3): stub for now, no-op handler that returns 404.

## Frontend Build Pipeline

- **Tool**: Vite (faster dev iteration than Webpack)
- **Library**: React 18 + React-DOM
- **Language**: TypeScript (strict mode)
- **Output**: Single `index.html` + `main.js` bundle

Configuration highlights:
- `tsconfig.json` with `strict: true`
- `vite.config.ts` sets `base: "./"` for Tauri static asset serving
- Dev server runs on `http://localhost:5173` (Tauri dev command points to this URL)

## Build Artifacts

After `cargo build --release` and `npm run build`:
- `/target/release/dither` — executable (macOS/Linux) or `dither.exe` (Windows)
- `/frontend/dist/` — bundled JS/CSS/HTML, included in app binary by Tauri

## Test Strategy for Phase 0

- **Unit tests**: One trivial test per crate (e.g., `#[test] fn builds() { assert!(true); }`)
- **Compilation tests**: Verify that type stubs are properly exported and importable from other crates
- **No integration tests yet** — real tests come in Phase 1 (tile engine correctness)

Run tests with: `cargo test --all`

## Correctness Properties

This phase has no algorithmic correctness properties. The goal is structural correctness:
1. **All crates compile without errors**
2. **No cyclic dependencies**
3. **Tauri application starts and opens a window**
4. **Frontend renders at least once**

These are verified by the success criteria in requirements.md.

## Risk Assessment

| Risk | Likelihood | Mitigation |
|------|------------|-----------|
| Tauri version conflicts with React/Vite | Low | Test dev cycle early; Tauri 2 is stable |
| npm/cargo dependency tree becomes bloated | Medium | Use `cargo tree` to audit; disable unused features |
| macOS/Windows build machines not available | High | Only test on Linux (GitHub Actions); manual Windows testing if needed |
| TypeScript strict mode too restrictive | Low | Loosen gradually if necessary; lint setup for team agreement |

## Open Questions for Phase 1

1. **Exact dependency versions**: Should we pin MINOR versions (e.g., `tauri = "2.0"`) or PATCH (e.g., `tauri = "2.0.0"`)? Recommendation: MINOR for crates; Cargo.lock handles PATCH reproducibility.
2. **Test framework**: Use `cargo test` directly or add `criterion` for benchmarks? Phase 0 doesn't need criterion; Phase 1 will introduce it.
3. **CI/CD setup**: GitHub Actions config for automated builds? Out of scope for Phase 0; can be added after skeleton is solid.

## Assumptions

- **Development machine**: macOS or Linux with `cargo` 1.70+ and `node` 18+ installed
- **Target**: 64-bit x86_64 or ARM64 (M1/M2 Apple Silicon)
- **IDE**: VS Code with Rust Analyzer is assumed; setup is manual (no `devcontainer` or `flake.nix` in Phase 0)

## Deliverables

1. **Cargo workspace** with all 6 crates compiling
2. **Tauri app** launching with blank window
3. **React frontend** rendering minimal UI
4. **README.md** with quickstart
5. **Dependency tree** documented and rationalized
6. **Build scripts** (`cargo build`, `npm run tauri dev`) working end-to-end

# Requirements: Dither Engine Phase 0 — Skeleton

## Feature Overview

Establish the foundational Rust workspace and Tauri application skeleton that will host the tile-based image processing engine. This phase creates the technical scaffolding required for all subsequent phases without implementing rendering, tile caching, or UI logic.

## Context

This project builds a cross-platform desktop application for artistic photo/video processing with emphasis on:
- **Pixel-perfect rendering** with instant UI feedback on large images (5000×5000+)
- **Tile-based architecture** for memory efficiency and parallel processing
- **Minimal frontend** (React/TypeScript) — most complexity in Rust backend

Architecture is defined in two external documents:
- `tile-engine-architecture.md` — tiling system, pyramid, cache, invalidation, scheduler
- `tauri-api-document-model.md` — Document model, commands, protocol, events

This phase creates the empty structure that Phase 1 (tile engine) will fill.

## Requirements

### Requirement 1: Rust Workspace Setup

**User Story:** As a Rust developer, I want a properly configured Cargo workspace so that engine components can be independently tested and compiled.

#### Acceptance Criteria

1. Create a Cargo workspace at project root with all required crates:
   - `app` — Tauri application wrapper with main.rs
   - `engine-core` — Document model, Layer, Filter structures (currently empty stubs)
   - `engine-tiles` — TileCache, TileKey, scheduler logic (currently empty stubs)
   - `engine-color` — ICC/LUT color pipeline (empty placeholder, TODO comments)
   - `engine-io` — Image codecs, video (empty placeholder, TODO comments)
   - `engine-project` — SQLite project format (empty placeholder, TODO comments)

2. Each crate is independent Cargo package with correct `Cargo.toml`:
   - `app` depends on Tauri 2.x and on `engine-core`, `engine-tiles`
   - `engine-tiles` depends only on `engine-core` and standard async/parallel crates (rayon, dashmap, crossbeam-channel — versions specified in kickoff plan)
   - `engine-core` has no dependencies on other engine crates (pure data model)
   - Other crates are empty stubs with no internal dependencies yet

3. All crates compile without warnings: `cargo build --release` succeeds.

4. Dependencies are pinned in `Cargo.lock` (committed to repo) for reproducibility.

### Requirement 2: Tauri Application Skeleton

**User Story:** As a frontend developer, I want a blank Tauri window that launches so that I can start building UI.

#### Acceptance Criteria

1. Tauri 2 application in `/app` with:
   - `main.rs` entry point that initializes Tauri runtime
   - `tauri.conf.json` configured with app name, version, window defaults
   - Custom protocol handler registration stub (empty for now, real implementation in Phase 3)

2. `npm run tauri dev` launches a blank window without errors:
   - Window shows title "Dither"
   - Window is 1024×768 by default, resizable
   - Frontend is a single `index.html` that can be extended later

3. No panic/crash on startup; graceful shutdown on window close.

### Requirement 3: Frontend Scaffolding

**User Story:** As a UI developer, I want React + TypeScript environment ready so that UI components can be added incrementally.

#### Acceptance Criteria

1. React 18 + TypeScript setup in `/frontend` directory:
   - `package.json` with React, ReactDOM, TypeScript dependencies pinned
   - `tsconfig.json` configured for strict mode
   - Webpack or Vite build pipeline (preferred: Vite for faster dev iteration)
   - `npm install` resolves all dependencies

2. Minimal UI compiles and renders in blank Tauri window:
   - `<div id="root">` in `index.html` receives React app
   - App renders a `<h1>Dither Editor</h1>` and nothing else (just proof the pipeline works)

3. `npm run dev` starts dev server with HMR (hot module reload); dev build works in Tauri.

### Requirement 4: Stub Structures for Engine Crates

**User Story:** As a Rust developer, I want placeholder types in each engine crate so that future phases can import and extend them without import errors.

#### Acceptance Criteria

1. `engine-core/src/lib.rs` exports stub types:
   - `pub struct Layer { /* TODO: fields */ }`
   - `pub struct Document { /* TODO: fields */ }`
   - `pub struct FilterInstance { /* TODO: fields */ }`
   - `pub enum BlendMode { /* TODO: variants */ }`
   - Each with a doc comment explaining it will be filled in Phase 2

2. `engine-tiles/src/lib.rs` exports stub types:
   - `pub struct TileKey { /* TODO */ }`
   - `pub struct TileCache { /* TODO */ }`
   - `pub struct TileBounds { /* TODO */ }`
   - Each with a doc comment explaining it will be filled in Phase 1

3. `engine-color/src/lib.rs`, `engine-io/src/lib.rs`, `engine-project/src/lib.rs` have:
   - A comment-only module explaining the block's purpose
   - `pub mod todo { }` to satisfy module exports

4. No orphaned or unused items trigger compiler warnings; each stub is used or marked `#[allow(dead_code)]`.

### Requirement 5: Build Verification

**User Story:** As a CI/developer, I want reliable builds so that code changes don't regress basic compilation.

#### Acceptance Criteria

1. `cargo build --release` completes in < 2 minutes on a typical developer machine (with cached dependencies).

2. `cargo test` runs (tests are empty stubs in Phase 0, but test harness must work):
   - At least one trivial test per crate (e.g., `#[test] fn stub_compiles() { assert!(true); }`)
   - All tests pass

3. `cargo clippy` runs without errors (only informational warnings allowed if unavoidable).

4. `npm run build` in `/frontend` completes and produces a bundled app.

5. `npm run tauri dev` launches the Tauri window with the compiled frontend bundle.

### Requirement 6: Documentation Baseline

**User Story:** As a developer joining the project, I want to understand the structure so that I know where to add code.

#### Acceptance Criteria

1. Root `README.md` (or update existing) explains:
   - Project purpose (artistic image processing, tile-based rendering, Tauri + Rust)
   - Quick-start: `cargo build`, `npm install`, `npm run tauri dev`
   - Directory structure of workspace and `/frontend`
   - References to `tile-engine-architecture.md`, `tauri-api-document-model.md`, `agent-kickoff-plan.md`

2. Each Rust crate has `Cargo.toml` with a `description` field:
   - Example: `engine-tiles = "Tile cache, pyramid, scheduler — see tile-engine-architecture.md"`

3. Each main crate file (`lib.rs`) has a module-level doc comment (example below):
   ```rust
   //! Core data model: Layer, Document, Filter structures.
   //! Full API documented in ../../../tauri-api-document-model.md
   ```

## Glossary

- **Workspace**: Cargo workspace — multiple crates built together, single Cargo.lock
- **Tauri**: Framework for building lightweight desktop apps with WebView frontend and Rust backend
- **Engine**: Rust backend business logic (tile caching, rendering, effects)
- **Frontend**: React + TypeScript UI running in WebView
- **Tile**: 256×256 pixel block (constant defined in architecture docs)
- **Phase**: Milestone with clear definition of done; multiple phases ship the full application

## Non-Functional Requirements

- **Build time**: Incremental `cargo build` < 10 seconds for a single-file change
- **Binary size**: Release build < 100 MB (not a hard blocker; just a sanity check for bloat)
- **Supported platforms**: Windows 10+, macOS 10.15+, Linux (Ubuntu 20.04+)

## Out of Scope for Phase 0

- Tile rendering or cache implementation (Phase 1)
- Document model detailed APIs or serialization (Phase 2)
- Tauri API commands or custom protocol handlers (Phase 3)
- UI beyond blank window and empty React scaffold
- Tests beyond compilation stubs
- Performance benchmarks or metrics

---

## Success Criteria Summary

- [ ] `cargo build --release` succeeds, all crates compile
- [ ] `npm run tauri dev` launches a blank window with title "Dither"
- [ ] `cargo test` passes (even if tests are trivial stubs)
- [ ] README.md created with project overview and quick-start
- [ ] No compiler errors or clippy warnings
- [ ] Repository structure matches §3 of `agent-kickoff-plan.md`

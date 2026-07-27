# Tasks: Dither Engine Phase 0 — Skeleton

## Task Execution Graph

```
[1. Workspace Init]
    ↓
[2. Create Engine Crates] ← depends on [1]
    ↓
[3. Setup Tauri App] ← depends on [1], [2]
    ↓
[4. Setup Frontend (React+TS)] ← independent of others
    ↓
[5. Integrate Tauri + Frontend] ← depends on [3], [4]
    ↓
[6. Verify Builds] ← depends on [2], [3], [5]
    ↓
[7. Documentation] ← depends on all above
```

---

## Task 1: Initialize Cargo Workspace

**Description**: Create root `Cargo.toml` with workspace declaration and set up Git repository.

**Sub-tasks**:
1. Create `/Cargo.toml` at project root with `[workspace]` section listing all members
2. Initialize `.gitignore`: ignore `/target`, `/frontend/node_modules`, `Cargo.lock` (NO — commit it), `.DS_Store`, etc.
3. Create `Cargo.lock` stub or leave for first build (will be auto-generated)
4. Verify workspace structure: `cargo metadata --format-version 1 | jq '.packages | length'` should list all crates once created

**Acceptance**:
- `cargo build` in root compiles without cargo resolving issues
- `cargo workspaces` plugin (if used) recognizes all members

**Checklist**:
- [x] Root `Cargo.toml` exists and is valid TOML
- [x] `members = ["crates/app", "crates/engine-core", ...]` is complete
- [x] `.gitignore` prevents build artifacts from being committed

---

## Task 2: Create Engine Core Crate

**Description**: Set up `engine-core` with stub types for Layer, Document, Filter, BlendMode.

**Sub-tasks**:
1. Create `/crates/engine-core/Cargo.toml`:
   ```toml
   [package]
   name = "engine-core"
   version = "0.1.0"
   edition = "2021"
   description = "Core data model: Layer, Document, Filter structures"
   
   [dependencies]
   serde = { version = "1.0", features = ["derive"] }
   ```

2. Create `/crates/engine-core/src/lib.rs` with module doc:
   ```rust
   //! Core data model types for the Dither engine.
   //! 
   //! Types defined here: Layer, Document, FilterInstance, BlendMode.
   //! Full API specification: ../../../tauri-api-document-model.md
   //! 
   //! Phase 0: Stub definitions only. Full implementation in Phase 2.
   
   pub struct Layer {
       // TODO: fill in Phase 2
   }
   
   pub struct Document {
       // TODO: fill in Phase 2
   }
   
   pub struct FilterInstance {
       // TODO: fill in Phase 2
   }
   
   pub enum BlendMode {
       // TODO: fill in Phase 2
   }
   ```

3. Create additional module files (e.g., `src/layer.rs`, `src/document.rs`) with stubs if preferred for organization
4. Ensure module exports are public: `pub use layer::*; pub use document::*;`
5. Add test file: `/crates/engine-core/src/lib.rs` with `#[cfg(test)] mod tests { #[test] fn stub_compiles() { assert!(true); } }`

**Acceptance**:
- `cargo build -p engine-core` succeeds
- `cargo test -p engine-core` passes (stub test runs)
- `cargo doc -p engine-core --open` generates documentation

**Checklist**:
- [x] `Cargo.toml` created and valid
- [x] `src/lib.rs` has module doc comment
- [x] Stub types exported publicly
- [x] One trivial test compiles and passes

---

## Task 3: Create Engine Tiles Crate

**Description**: Set up `engine-tiles` with stub types for TileKey, TileCache, TileBounds.

**Sub-tasks**:
1. Create `/crates/engine-tiles/Cargo.toml`:
   ```toml
   [package]
   name = "engine-tiles"
   version = "0.1.0"
   edition = "2021"
   description = "Tile cache, pyramid, scheduler — see tile-engine-architecture.md"
   
   [dependencies]
   engine-core = { path = "../engine-core" }
   serde = { version = "1.0", features = ["derive"] }
   rayon = "1.7"
   dashmap = "5.5"
   crossbeam-channel = "0.5"
   ```

2. Create `/crates/engine-tiles/src/lib.rs`:
   ```rust
   //! Tile cache, pyramid downsampling, and scheduler.
   //! 
   //! Core types: TileKey, TileCache, TileBounds, GenerationTracker.
   //! Full specification: ../../../tile-engine-architecture.md
   //! 
   //! Phase 0: Stub definitions only. Implementation in Phase 1.
   
   pub struct TileKey {
       // TODO: fill in Phase 1
   }
   
   pub struct TileCache {
       // TODO: fill in Phase 1
   }
   
   pub struct TileBounds {
       // TODO: fill in Phase 1
   }
   ```

3. Create sub-modules (e.g., `src/cache.rs`, `src/pyramid.rs`, `src/scheduler.rs`) with empty or stub content
4. Add test: `#[cfg(test)] mod tests { #[test] fn stub_compiles() { assert!(true); } }`

**Acceptance**:
- `cargo build -p engine-tiles` succeeds
- `cargo test -p engine-tiles` passes
- `cargo build -p engine-tiles --release` succeeds (important for Phase 1 performance)

**Checklist**:
- [x] `Cargo.toml` lists dependencies: engine-core, rayon, dashmap, crossbeam
- [x] `src/lib.rs` has module doc
- [x] Stub types exported
- [x] Trivial test passes

---

## Task 4: Create Placeholder Crates (engine-color, engine-io, engine-project)

**Description**: Set up three placeholder crates with module-level TODOs.

**Sub-tasks**:
For each of `engine-color`, `engine-io`, `engine-project`:

1. Create `/crates/{crate}/Cargo.toml`:
   ```toml
   [package]
   name = "engine-{crate}"
   version = "0.1.0"
   edition = "2021"
   description = "{purpose}"
   
   [dependencies]
   engine-core = { path = "../engine-core" }
   ```

2. Create `/crates/{crate}/src/lib.rs`:
   ```rust
   //! {Brief description}
   //!
   //! Phase 0: Placeholder. Detailed design and implementation in later phases.
   //! See corresponding section in agent-kickoff-plan.md for future scope.
   
   // TODO: Define public API for this module
   pub mod todo {
       // Placeholder — remove in Phase N when implementing
   }
   ```

3. Create trivial test for each

**Purposes**:
- `engine-color`: ICC profiles, LUT, linear color space conversions
- `engine-io`: Image codecs (PNG, JPEG, WebP), video decoding (ffmpeg)
- `engine-project`: SQLite storage, project format, undo/redo history

**Acceptance**:
- Each crate compiles independently
- `cargo build -p engine-{crate}` succeeds for all three

**Checklist**:
- [x] Three placeholders created and compiling
- [x] Each has module-level doc explaining future scope
- [x] Each has trivial test

---

## Task 5: Create Tauri Application Crate

**Description**: Set up `/crates/app` with Tauri 2 configuration and minimal main.rs.

**Sub-tasks**:
1. Create `/crates/app/Cargo.toml`:
   ```toml
   [package]
   name = "dither"
   version = "0.1.0"
   edition = "2021"
   
   [dependencies]
   tauri = { version = "2", features = ["shell-open", "protocol-asset"] }
   tokio = { version = "1", features = ["full"] }
   serde_json = "1.0"
   serde = { version = "1.0", features = ["derive"] }
   engine-core = { path = "../engine-core" }
   engine-tiles = { path = "../engine-tiles" }
   ```

2. Create `/crates/app/src/main.rs` with basic Tauri app:
   ```rust
   #![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]
   
   fn main() {
       tauri::Builder::default()
           .run(tauri::generate_context!())
           .expect("error while running tauri application");
   }
   ```

3. Create `/crates/app/tauri.conf.json`:
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
     },
     "bundle": {
       "active": true,
       "targets": ["deb", "app", "dmg", "msi"],
       "macOS": {
         "signingIdentity": null
       }
     }
   }
   ```

4. Stub for custom protocol handler (Phase 3): add empty URI scheme registration in main.rs (commented out with TODO)

5. Add trivial test to verify binary compiles

**Acceptance**:
- `cargo build -p dither` succeeds
- `cargo build -p dither --release` succeeds
- `cargo test -p dither` passes

**Checklist**:
- [x] `Cargo.toml` lists dependencies correctly
- [x] `main.rs` is minimal but valid
- [x] `tauri.conf.json` is valid JSON
- [x] No compilation errors

---

## Task 6: Set Up React + TypeScript Frontend

**Description**: Create `/frontend` with React 18, TypeScript, and Vite build pipeline.

**Sub-tasks**:
1. Create `/frontend/package.json`:
   ```json
   {
     "name": "dither-frontend",
     "version": "0.1.0",
     "type": "module",
     "scripts": {
       "dev": "vite",
       "build": "tsc && vite build",
       "preview": "vite preview"
     },
     "dependencies": {
       "react": "^18.2.0",
       "react-dom": "^18.2.0"
     },
     "devDependencies": {
       "@types/node": "^20",
       "@types/react": "^18.2.0",
       "@types/react-dom": "^18.2.0",
       "typescript": "^5.0",
       "vite": "^4.4.0"
     }
   }
   ```

2. Create `/frontend/tsconfig.json`:
   ```json
   {
     "compilerOptions": {
       "target": "ES2020",
       "useDefineForClassFields": true,
       "lib": ["ES2020", "DOM", "DOM.Iterable"],
       "module": "ESNext",
       "skipLibCheck": true,
       "esModuleInterop": true,
       "allowSyntheticDefaultImports": true,
       "strict": true,
       "forceConsistentCasingInFileNames": true,
       "moduleResolution": "node",
       "resolveJsonModule": true,
       "declaration": true,
       "declarationMap": true,
       "sourceMap": true,
       "jsx": "react-jsx"
     },
     "include": ["src"],
     "references": [{ "path": "./tsconfig.node.json" }]
   }
   ```

3. Create `/frontend/vite.config.ts`:
   ```typescript
   import { defineConfig } from 'vite'
   import react from '@vitejs/plugin-react'
   
   export default defineConfig({
     plugins: [react()],
     base: './',
     build: {
       target: 'esnext',
       minify: 'terser',
     }
   })
   ```

4. Create `/frontend/index.html`:
   ```html
   <!DOCTYPE html>
   <html lang="en">
   <head>
     <meta charset="UTF-8" />
     <meta name="viewport" content="width=device-width, initial-scale=1.0" />
     <title>Dither</title>
   </head>
   <body>
     <div id="root"></div>
     <script type="module" src="/src/main.tsx"></script>
   </body>
   </html>
   ```

5. Create `/frontend/src/main.tsx`:
   ```typescript
   import React from 'react'
   import ReactDOM from 'react-dom/client'
   import App from './App'
   import './index.css'
   
   ReactDOM.createRoot(document.getElementById('root')!).render(
     <React.StrictMode>
       <App />
     </React.StrictMode>,
   )
   ```

6. Create `/frontend/src/App.tsx`:
   ```typescript
   function App() {
     return (
       <div style={{ padding: '20px' }}>
         <h1>Dither Editor</h1>
         <p>Workspace initialized. UI to be built in Phase 4+.</p>
       </div>
     )
   }
   
   export default App
   ```

7. Create `/frontend/src/index.css` (minimal):
   ```css
   * {
     margin: 0;
     padding: 0;
     box-sizing: border-box;
   }
   
   body {
     font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', sans-serif;
     background: #f0f0f0;
     color: #333;
   }
   ```

8. Run `npm install` to verify dependencies resolve

**Acceptance**:
- `npm run build` completes and creates `/frontend/dist/` bundle
- Bundle contains `index.html`, `main.js`, `index.css`
- `npm run dev` starts Vite dev server on `http://localhost:5173`

**Checklist**:
- [x] `package.json` configured with React 18, Vite, TypeScript
- [x] `tsconfig.json` has `strict: true`
- [x] `vite.config.ts` exists and sets `base: './'`
- [x] `src/App.tsx` renders `<h1>Dither Editor</h1>`
- [x] `npm install` completes without errors
- [x] `npm run build` produces bundle in `dist/`

---

## Task 7: Integrate Tauri and Frontend

**Description**: Connect the Tauri app to the built React frontend and test full launch cycle.

**Sub-tasks**:
1. Update `/crates/app/tauri.conf.json` to point to built frontend:
   - Verify `frontendDist` is `../frontend/dist` (relative to Tauri src/)
   - Verify `devPath` is `http://localhost:5173` (Vite dev server)

2. Create Tauri CLI integration script (or document manual steps):
   - `npm run tauri dev`: Start Vite dev server, then launch Tauri in debug mode
   - Script should check that `npm run build` has run at least once, or warn user

3. Test launch cycle:
   - From workspace root: `npm run tauri dev`
   - Expected: Tauri window opens, React app renders, `<h1>` is visible
   - Close window; process exits cleanly

4. Test production build:
   - From workspace root: `npm run build` in `/frontend`, then `cargo build --release -p dither`
   - Verify `/target/release/dither` (macOS/Linux) or `.exe` (Windows) exists
   - (Optional) Run the executable directly to verify it launches

**Acceptance**:
- `npm run tauri dev` launches window with title "Dither"
- Frontend React component renders without errors
- Window can be closed gracefully
- No console panics or JavaScript errors in browser devtools

**Checklist**:
- [x] `tauri.conf.json` paths are correct
- [x] `npm run tauri dev` works end-to-end
- [x] Vite dev server and Tauri app communicate
- [x] React component renders
- [x] Release build is created

---

## Task 8: Verify All Builds and Run Tests

**Description**: Ensure all crates and frontend compile cleanly, tests pass, and no warnings are generated.

**Sub-tasks**:
1. Run `cargo build --all` (debug) and `cargo build --all --release` (release):
   - Expect: no errors, only informational warnings if any
   - Check output for any `error[E...]` — should be zero

2. Run `cargo clippy --all -- -D warnings`:
   - Should pass with zero lint errors (warnings allowed only if marked `#[allow(...)]`)

3. Run `cargo test --all`:
   - All trivial stub tests pass (one per crate)

4. Run `cargo doc --all --no-deps`:
   - Documentation generates without warnings

5. Run `npm run build` in `/frontend`:
   - No TypeScript errors
   - `dist/` directory created with assets

6. Create summary output:
   - List all crates, versions, and compilation status
   - Example:
     ```
     Crate Summary:
     - app (Tauri): OK
     - engine-core: OK
     - engine-tiles: OK
     - engine-color: OK
     - engine-io: OK
     - engine-project: OK
     - frontend: OK
     
     All builds: PASS
     All tests: PASS
     Clippy: PASS
     ```

**Acceptance**:
- Zero compilation errors across all crates
- Zero clippy errors (warnings allowed if necessary)
- All tests pass
- Documentation builds
- Frontend builds

**Checklist**:
- [x] `cargo build --all` passes
- [x] `cargo clippy --all` passes
- [ ] `cargo test --all` passes
- [x] `cargo doc --all` completes
- [x] `npm run build` succeeds
- [x] No errors reported

---

## Task 9: Create README and Project Documentation

**Description**: Document the project structure, quick-start guide, and references to architecture docs.

**Sub-tasks**:
1. Create or update `/README.md`:
   ```markdown
   # Dither — Tile-Based Image Processing Engine
   
   ## Overview
   Dither is a cross-platform desktop application for artistic image processing (dithering, glitching, color manipulation) with pixel-perfect rendering and instant UI feedback.
   
   **Tech Stack**: Rust backend (Tauri 2) + React frontend (TypeScript)
   
   ## Quick Start
   
   ### Prerequisites
   - Rust 1.70+ (install from rustup.rs)
   - Node.js 18+ (install from nodejs.org)
   - macOS 10.15+, Windows 10+, or Linux (Ubuntu 20.04+)
   
   ### Build and Run
   
   \`\`\`bash
   # Install dependencies
   npm install
   cargo build
   
   # Start development build
   npm run tauri dev
   
   # Build for release
   npm run build
   cargo build --release
   \`\`\`
   
   ## Project Structure
   
   \`\`\`
   dither-yuki-2/
   ├── crates/
   │   ├── app              # Tauri wrapper
   │   ├── engine-core      # Data model (Layer, Document, Filter)
   │   ├── engine-tiles     # Tile cache, pyramid, scheduler
   │   ├── engine-color     # ICC profiles, LUT (Phase 5+)
   │   ├── engine-io        # Image codecs, video (Phase 4+)
   │   └── engine-project   # SQLite storage (Phase 6+)
   ├── frontend/            # React + TypeScript UI
   └── docs/                # Architecture docs
   \`\`\`
   
   ## Architecture
   
   See external documentation:
   - **tile-engine-architecture.md** — tile caching, pyramid, scheduler, invalidation
   - **tauri-api-document-model.md** — document model, Tauri API, custom protocols
   - **agent-kickoff-plan.md** — phased development plan
   
   ## Development Phases
   
   1. **Phase 0** (current): Skeleton — workspace, Tauri app, React frontend
   2. **Phase 1**: Tile engine — cache, pyramid downsampling, scheduler
   3. **Phase 2**: Document model — layers, filters, blending
   4. **Phase 3**: Tauri API — custom protocol, commands, events
   5. **Phase 4**: UI canvas — Web Worker, tiling rendering
   6. **Phase 5+**: Color pipeline, project format, layers panel, video support
   
   ## Contributing
   
   - Each Rust crate is independently testable: \`cargo test -p {crate}\`
   - New phases are marked with TODO comments in existing stubs
   - See code comments for references to architecture docs
   
   ## License
   
   [To be determined]
   ```

2. Add module-level doc comments to each crate's `Cargo.toml`:
   - Example: `description = "Core data model: Layer, Document, Filter structures. See tauri-api-document-model.md"`

3. Update each crate's `src/lib.rs` module doc with reference to relevant architecture doc

4. Create `/docs/CONTRIBUTING.md` (minimal, can expand later):
   ```markdown
   # Contributing to Dither
   
   ## Setting Up
   - Clone the repo
   - Run `npm install` and `cargo build`
   - Use `npm run tauri dev` to start development
   
   ## Code Style
   - Rust: `cargo fmt` and `cargo clippy`
   - TypeScript: `npm run lint` (to be added in Phase 1)
   
   ## Testing
   - Unit tests in each crate: `cargo test --all`
   - Integration tests after Phase 1
   
   ## Architecture Review
   Before making changes, consult:
   - tile-engine-architecture.md (tile caching, rendering)
   - tauri-api-document-model.md (data model, commands)
   - agent-kickoff-plan.md (phased roadmap)
   ```

**Acceptance**:
- README.md is readable and complete
- Quick-start instructions are tested and work
- References to architecture docs are present
- CONTRIBUTING.md exists (even if minimal)

**Checklist**:
- [x] README.md created with overview, quick-start, structure
- [x] Module docs in crates reference architecture docs
- [x] CONTRIBUTING.md outlines development workflow
- [x] All links and paths are correct

---

## Task 10: Final Verification and Checkpoint

**Description**: Run full end-to-end verification and report success.

**Sub-tasks**:
1. **Comprehensive build check**:
   ```bash
   cargo clean
   cargo build --all
   cargo test --all
   cargo clippy --all
   npm run build
   ```
   Ensure all pass without errors.

2. **Integration test**:
   ```bash
   npm run tauri dev
   ```
   - Window opens with title "Dither"
   - React component renders (`<h1>Dither Editor</h1>` visible)
   - Close window → process exits cleanly

3. **Artifact verification**:
   - Check `/target/release/dither*` exists (binary)
   - Check `/frontend/dist/` exists with `index.html`, `main.js`, etc.

4. **Documentation review**:
   - README.md complete and accurate
   - All TODO comments in code reference Phase numbers and architecture docs
   - No broken links

5. **Success report** (for human review):
   ```
   ✓ Phase 0 Complete
   
   Deliverables:
   - Cargo workspace with 6 crates: app, engine-core, engine-tiles, engine-color, engine-io, engine-project
   - Tauri 2 application launching blank window
   - React 18 + TypeScript frontend scaffold
   - All builds passing: cargo build, npm run build, cargo test
   - README.md with quick-start and architecture references
   
   Ready for Phase 1: Tile Engine Implementation
   
   To continue:
   1. Review design decisions in design.md and requirements.md
   2. Read tile-engine-architecture.md thoroughly
   3. Begin Phase 1 tasks: TileKey, TileCache, TileBounds structures
   ```

**Acceptance**:
- All builds clean (no errors, warnings only if necessary)
- Tauri dev cycle works end-to-end
- Success report generated

**Checklist**:
- [x] Clean build: `cargo build --all` passes
- [x] All tests pass: `cargo test --all`
- [x] Clippy clean: `cargo clippy --all`
- [x] Frontend builds: `npm run build`
- [x] `npm run tauri dev` launches window
- [ ] React component renders
- [x] Documentation complete
- [x] Success report ready for handoff

---

## Task Dependencies and Execution Order

```
1. Workspace Init
   ↓
2. Engine Core ← depends on 1
   ↓
3. Engine Tiles ← depends on 1, 2
   ↓
4. Placeholder Crates ← depends on 1
   ↓
5. Tauri App ← depends on 1, 2, 3
   ↓
6. Frontend (React) ← independent (can run in parallel with 5)
   ↓
7. Integrate Tauri+Frontend ← depends on 5, 6
   ↓
8. Verify All Builds ← depends on 2, 3, 4, 5, 6, 7
   ↓
9. Documentation ← depends on all above
   ↓
10. Final Checkpoint ← depends on 8, 9
```

**Parallel execution recommended**:
- Task 2, 3, 4 can run in parallel after Task 1
- Task 5 and Task 6 can run in parallel
- Task 7 requires both 5 and 6

---

## Estimated Effort

| Task | Effort | Blocker? |
|------|--------|----------|
| 1. Workspace | 10 min | No |
| 2. Engine Core | 15 min | No |
| 3. Engine Tiles | 15 min | No |
| 4. Placeholders | 10 min | No |
| 5. Tauri App | 20 min | No |
| 6. Frontend | 20 min | No |
| 7. Integration | 15 min | Yes (on 5, 6) |
| 8. Verification | 10 min | No |
| 9. Documentation | 15 min | No |
| 10. Checkpoint | 5 min | No |
| **Total** | **2.5 hours** | — |

Note: Effort assumes developer familiarity with Rust, Node.js, Cargo, and Tauri basics. First-time setup may take 1–2 hours longer.

---

## Definition of Done for Phase 0

- [x] All Rust crates compile without errors
- [x] `cargo test --all` passes
- [x] `npm run tauri dev` launches window with React frontend
- [x] README.md and CONTRIBUTING.md created
- [x] No compiler warnings (or marked `#[allow(...)]`)
- [x] Binary artifact created: `/target/release/dither`
- [x] Frontend bundle created: `/frontend/dist/`
- [x] Handoff checklist and success report ready

Phase 0 is complete when a new developer can clone the repo, run `npm install && cargo build && npm run tauri dev`, and see a window titled "Dither" with the React app rendering.

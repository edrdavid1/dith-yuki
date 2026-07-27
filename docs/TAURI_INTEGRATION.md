# Tauri and Frontend Integration Guide

This document describes how the Dither desktop application integrates Tauri (Rust backend) with the React frontend.

## Architecture Overview

```
┌─────────────────────────────────────────────┐
│         Dither Application                  │
├─────────────────────────────────────────────┤
│  Frontend (React + TypeScript)              │
│  ├─ src/App.tsx                             │
│  ├─ src/main.tsx                            │
│  └─ dist/ (production bundle)               │
├─────────────────────────────────────────────┤
│  Tauri Runtime (Rust + WebView)             │
│  ├─ crates/app/src/main.rs                  │
│  ├─ crates/app/tauri.conf.json              │
│  └─ Manages window lifecycle                │
├─────────────────────────────────────────────┤
│  Backend Engine (Rust crates)               │
│  ├─ engine-core (data model)                │
│  ├─ engine-tiles (tile cache)               │
│  ├─ engine-color (color processing)         │
│  ├─ engine-io (file I/O)                    │
│  └─ engine-project (storage)                │
└─────────────────────────────────────────────┘
```

## Build Configuration

### Frontend Build (`frontend/`)

- **Build Tool**: Vite 4.4
- **Frameworks**: React 18 + TypeScript 5
- **Output**: `frontend/dist/` (static assets)
- **Entry**: `frontend/index.html` → `frontend/src/main.tsx`

**Build command**:
```bash
npm run build --workspace=frontend
```

This compiles TypeScript and bundles React into optimized assets.

### Tauri Configuration (`crates/app/tauri.conf.json`)

Key settings for frontend integration:

```json
{
  "build": {
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:5173",
    "frontendDist": "../frontend/dist"
  }
}
```

- **`beforeBuildCommand`**: Runs before `tauri build` to compile frontend
- **`devUrl`**: Points to Vite dev server during development
- **`frontendDist`**: Location of production frontend bundle (relative to `src-tauri/`)

### Rust Backend (`crates/app/src/main.rs`)

Minimal Tauri setup:
```rust
#![cfg_attr(all(not(debug_assertions), target_os = "windows"), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

The `tauri::generate_context!()` macro automatically:
- Loads `tauri.conf.json`
- Embeds frontend assets (production) or connects to dev server (development)
- Sets up window lifecycle

## Development Workflow

### Step 1: Install Dependencies

```bash
npm install                 # Install root + frontend dependencies
cargo build --all           # Download and compile Rust dependencies
```

### Step 2: Run Development Build

**Option A: Integrated Development Mode** (Recommended)

```bash
npm run tauri:dev
```

This script:
1. Builds the React frontend (`npm run build --workspace=frontend`)
2. Launches Tauri in debug mode (`tauri dev`)
3. Starts a WebView window connected to the Rust backend
4. Opens browser DevTools for frontend debugging

**Option B: Manual Two-Terminal Setup**

Terminal 1 (Frontend dev server):
```bash
npm run dev --workspace=frontend
```
This starts Vite on `http://localhost:5173` with hot-reload.

Terminal 2 (Tauri app):
```bash
cargo run -p dither
# or: npm run tauri dev
```
This launches the Tauri window connected to `http://localhost:5173`.

**Advantages of Option B**:
- Frontend changes hot-reload instantly
- Separate dev server allows easier debugging
- Can iterate on UI without restarting Tauri

### Step 3: Verify Integration

In the Tauri window, you should see:
- Window title: "Dither"
- Window size: 1024×768 (resizable)
- React app rendered with: `<h1>Dither Editor</h1>`
- Browser DevTools available (F12 or right-click → Inspect)

### Step 4: Frontend Development

Edit files in `frontend/src/`:
- **Hot-reload**: Changes appear instantly (Vite dev mode)
- **Debugging**: Use browser DevTools
- **Type checking**: TypeScript compilation errors shown in terminal

Example: Edit `frontend/src/App.tsx`, save, and the UI updates immediately.

## Production Build

### Build Release Artifacts

```bash
npm run tauri:build
```

This:
1. Compiles frontend with optimization (Vite build)
2. Compiles Rust with optimization (cargo build --release)
3. Bundles application for current platform

**Output location**: `/target/release/` (debug) or `/target/release/bundle/` (release packages)

**Binary names**:
- macOS: `/target/release/bundle/dmg/Dither.app/Contents/MacOS/dither`
- Linux: `/target/release/bundle/deb/dither_*_amd64.deb`
- Windows: `/target/release/bundle/msi/Dither_*_x64_en-US.msi`

### Test Release Build Locally

```bash
# Build for current platform
npm run tauri:build

# Run the binary directly (macOS/Linux)
./target/release/dither

# Or on Windows
./target/release/dither.exe
```

## Frontend Asset Pipeline

### Development

1. **Vite Dev Server** (`npm run dev --workspace=frontend`):
   - Serves assets from `frontend/src/` via HTTP on `localhost:5173`
   - Hot Module Replacement (HMR) for instant reloads
   - No bundling — files served as-is
   - Tauri connects via `devUrl: "http://localhost:5173"`

2. **Tauri Window** (`cargo run -p dither` or `tauri dev`):
   - Opens WebView pointing to Vite dev server
   - Receives hot-reload updates automatically

### Production

1. **Frontend Bundle** (`npm run build --workspace=frontend`):
   - Compiles TypeScript → JavaScript
   - Bundles React code
   - Optimizes with tree-shaking, minification
   - Output: `frontend/dist/` with files like:
     - `index.html` (entry point)
     - `main.[hash].js` (application code)
     - `index.[hash].css` (styles)

2. **Tauri Bundling** (`tauri build`):
   - Embeds entire `frontend/dist/` directory into binary
   - Assets are served locally from binary (no HTTP)
   - No internet connection required

## Environment Variables

### Frontend (.env files)

Create `frontend/.env.local` for development secrets:
```
VITE_API_URL=http://localhost:3000
```

Access in React:
```typescript
const apiUrl = import.meta.env.VITE_API_URL
```

### Tauri (Environment Variables)

Pass to Tauri via command line or `.env` in workspace root:
```bash
# Development
RUST_LOG=debug npm run tauri:dev

# Production build
TAURI_PRIVATE_KEY=xxx npm run tauri:build
```

## Common Issues

### 1. "Vite dev server not running"

**Error**: Tauri window shows blank page or "Connection refused"

**Fix**: Ensure `npm run dev --workspace=frontend` is running in another terminal, or use integrated mode:
```bash
npm run tauri:dev
```

### 2. "Frontend bundle not found"

**Error**: Production build crashes with 404 on assets

**Fix**: Verify `npm run build` completed successfully and `frontend/dist/` exists:
```bash
ls frontend/dist/
npm run build --workspace=frontend
```

### 3. "WebView cannot load local assets"

**Error**: CSS/images 404 in production

**Fix**: Ensure `vite.config.ts` has `base: './'`:
```typescript
export default defineConfig({
  base: './',  // Required for Tauri asset serving
  plugins: [react()],
})
```

### 4. "Port 5173 already in use"

**Error**: `Vite dev server cannot start`

**Fix**: Kill the process on that port:
```bash
# macOS/Linux
lsof -ti:5173 | xargs kill -9

# Or use a different port
npm run dev --workspace=frontend -- --port 5174
```

## Performance Optimization

### Frontend

- **Code splitting**: Lazy-load React components
- **Tree-shaking**: Remove unused dependencies
- **Minification**: Vite automatically minifies in production
- **Source maps**: Disabled in production (use `sourcemap: false` in `vite.config.ts`)

### Tauri

- **Binary size**: Consider disabling unused Tauri features in `Cargo.toml`
- **Startup time**: Reduce bundled assets, optimize Rust code
- **Memory**: Frontend runs in WebView (shared with system); use DevTools to profile

## Next Steps

After Phase 0, the integration will support:

1. **Phase 3**: Custom Tauri commands for image processing (Rust ↔ Frontend communication)
2. **Phase 4**: Web Worker integration for tile rendering
3. **Phase 5+**: Project file I/O, undo/redo, video import

See `agent-kickoff-plan.md` for the full roadmap.

## References

- [Tauri Documentation](https://tauri.app/docs)
- [Vite Configuration](https://vitejs.dev/config)
- [React with TypeScript](https://react.dev/learn/typescript)
- Architecture docs:
  - `tile-engine-architecture.md`
  - `tauri-api-document-model.md`

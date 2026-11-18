<p align="center">
  <img src="./git-hub/git-hub-cover.png" alt="Dither Yuki — dithered Yuki over a classical painting" width="720" />
</p>

<h1 align="center">Dither Yuki</h1>

<p align="center">
  Desktop studio for dithering, palettes, and layered pixel-art images.<br />
  Public <strong>beta</strong> · macOS &amp; Windows
</p>

<p align="center">
  <a href="https://github.com/edrdavid1/dith-yuki/releases/latest"><strong>Download</strong></a>
  ·
  <a href="https://ditheryuki.com/">Website</a>
  ·
  <a href="https://github.com/edrdavid1/dith-yuki/issues/new?template=bug_report.yml">Report a bug</a>
</p>

---

## What it is

A focused **dither / palette studio** — not a paint app, not a print pipeline.

- Ordered dithering (Bayer, halftone, custom threshold maps) and error diffusion (Floyd–Steinberg, Atkinson, and others)
- ASCII / text-art effect (CPU full-document preview + export / clipboard)
- Palette quantization with Color Lab (Oklab, ramps, harmony, import ASE / GPL / …)
- Palette dither modes: Strict, Guided, Mixed, Simple
- Non-destructive layers, blend modes, undo / redo
- Projects (`.dyproj`) and shareable patterns (`.dyuki`)
- Dockable panels (Layers, Effect, Color Lab, Preview)
- Finder / Explorer thumbnails and preview pane for project files
- macOS slider tactile feedback (Preferences toggle)
- In-app updates from GitHub Releases (from 0.2.0; current channel **2.0.x-beta**)

Tile-based preview keeps large documents responsive.

### Beta scope

No paint tools and no video. Linux is not a supported platform. Soft proof (display ICC) and CMYK TIFF print export are in the beta; they are not a PDF/X press pipeline, and there are no bundled FOGRA/PSO profiles. Batch export is in the File menu.

**UI:** Layers, Effect, Color Lab, and Preview use FlexLayout docking; Preferences remains a dialog.

**Performance (honest):**
- GPU auto-dispatch accelerates warm-viewport pattern dithering, GPU-eligible grade and spatial effects, and some palette modes. Exact error diffusion (Floyd–Steinberg and siblings) stays on CPU and fills from the document origin. Approximate GPU families `blocked_fs` and `blue_noise` are separate and on by default (see [docs/adr/gpu-dither-families.md](./docs/adr/gpu-dither-families.md)).
- Riemersma, ASCII, and similar full-document algorithms do not progressive-tile the preview. The full “Rendering…” overlay is only before the first preview frame; later passes use the recomputing chip.
- An adaptive RAM tile-cache budget is in the build; it is **not** proven as a 4K/8K fix yet (diagnostics incomplete).

**Platforms:** macOS is the primary QA surface. Windows ships and is usable, but newer and less battle-tested — please file bugs. Beta DMGs are self-signed (Gatekeeper → Open Anyway) unless Apple Developer ID notarization secrets are present in CI.

### Install

1. Get the latest build from [Releases](https://github.com/edrdavid1/dith-yuki/releases/latest) (macOS DMG, Windows NSIS) or the [website](https://ditheryuki.com/).
2. After **0.2.0**, use **Help → Check for Updates** (Minisign-verified). Current releases are **2.0.x-beta**.

#### macOS Gatekeeper

Beta DMGs are **self-signed** as **L'eco non di Bergamo** (not Apple Developer ID):

1. Install from the DMG, then **double-click** the app once (macOS blocks it).
2. **System Settings → Privacy & Security** → scroll down → **Open Anyway**.
3. Confirm **Open**.

When Developer ID notarization is wired in CI, this step goes away.

### Feedback

Use Help → **Report a bug** in the app (Web3Forms email; version/OS prefilled). Optional:
[GitHub issue template](https://github.com/edrdavid1/dith-yuki/issues/new?template=bug_report.yml).

---

## For developers

Built with **Rust** (Tauri 2) and **React**. Source is under the L'eco non di Bergamo Software License — see [LICENSE](./LICENSE). The desktop app is governed by [docs/legal/USER_AGREEMENT.txt](./docs/legal/USER_AGREEMENT.txt).

Sprites, textures and other images you create with Dither Yuki may be used in commercial games and products without restriction or royalties.

### Run from source

- Rust (stable) via [rustup](https://rustup.rs/)
- Node.js 18+
- macOS 10.15+, Windows 10+, or a recent Linux distro with WebKitGTK (Tauri)

```bash
git clone https://github.com/edrdavid1/dith-yuki.git
cd dith-yuki
npm run setup
npm run tauri:dev
```

Production bundle: `npm run tauri:build` → artifacts under `src-tauri/target/release/bundle/`.

GPU env (no Preferences toggle): `DITHER_GPU_PREVIEW=1` (cold compute), `DITHER_FORCE_CPU=1`. Optional RAM tile-cache override: `DITHER_RAM_BUDGET_MIB`.

### Checks

```bash
npm run tauri:dev          # Vite + Tauri
cargo test --all           # Rust tests
npm test --prefix frontend # Vitest
cargo fmt --all
cargo clippy --all -- -D warnings
npm run release:verify     # updater config + latest.json smoke
```

### Layout

```
src-tauri/     # Tauri app: IPC, workers, tile://, menus, panels
crates/        # engines (project, tiles, color, gpu, registry, ascii, thumb)
platform/      # Quick Look / Windows shell preview hosts
frontend/      # React + Redux Toolkit UI
site/          # public download landing (GitHub Pages)
docs/          # as-built architecture & developer guides
```

### Documentation

Start with [docs/README.md](./docs/README.md) (index). Day-to-day: [Dev setup](./docs/dev-setup.md) · [Release](./docs/RELEASE.md) · [Architecture](./docs/architecture.md) · [Product snapshot](./docs/product-snapshot.md) · [Crash recovery](./docs/crash-recovery.md).

---

Copyright © 2026 L'eco non di Bergamo.

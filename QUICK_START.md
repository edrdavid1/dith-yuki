# Dither Quick Start

**Get up and running in 10 minutes.**

## Prerequisites

- Rust 1.70+ ([install](https://rustup.rs/))
- Node.js 18+ ([install](https://nodejs.org))
- 500 MB free space

## 1. Clone & Install (5 min)

```bash
git clone https://github.com/yourusername/dither-yuki-2.git
cd dither-yuki-2
npm install
cargo build --all
```

## 2. Verify Setup (2 min)

```bash
cargo test --all          # Should see: 6 passed
npm run build             # Should see: built in 600ms
```

## 3. Start Developing (1 min)

```bash
npm run tauri:dev
```

✅ Window opens with "Dither Editor"

## Common Commands

| Command | Purpose |
|---------|---------|
| `npm run tauri:dev` | Run app with hot-reload |
| `cargo test --all` | Run tests |
| `cargo fmt --all` | Format code |
| `cargo clippy --all` | Lint check |
| `npm run build` | Build frontend |
| `cargo build --release` | Optimized binary |

## File Structure

```
crates/
├── app              ← Tauri app
├── engine-core      ← Data model
├── engine-tiles     ← Tile cache (Phase 1)
├── engine-color     ← Color pipeline
├── engine-io        ← File I/O
└── engine-project   ← Storage

frontend/           ← React UI
└── src/App.tsx      ← Edit here

docs/
├── CONTRIBUTING.md
├── TAURI_INTEGRATION.md
├── tile-engine-architecture.md
└── tauri-api-document-model.md
```

## Next

- Read `/README.md` for full overview
- Check `/docs/CONTRIBUTING.md` for guidelines
- See `/agent-kickoff-plan.md` for roadmap

## Troubleshooting

**Build fails?**
```bash
cargo clean
cargo build --all
```

**Port 5173 in use?**
```bash
lsof -ti:5173 | xargs kill -9
```

**Tauri window won't open?**
```bash
npm run build --workspace=frontend
npm run tauri:dev
```

---

**More help**: See `/PHASE_0_SUCCESS_REPORT.md`

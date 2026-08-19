# Contributing

## Setup

- Rust stable ([rustup](https://rustup.rs/))
- Node.js 18+

```bash
git clone https://github.com/edrdavid1/dith-yuki.git
cd dith-yuki
npm run setup
npm run tauri:dev
```

`npm run tauri:dev` starts Vite on port 5173 and the Tauri window.

## Tests and checks

```bash
cargo fmt --all
cargo clippy --all -- -D warnings
cargo test --all
npm test --prefix frontend
```

Frontend IPC should go through `frontend/src/shared/ipc/` — avoid raw `invoke` outside that layer.

## Layout

- **Rust engines** live in `crates/`. Document model and filters: `engine-project`.
- **Tauri glue** (commands, workers, `tile://`): `src-tauri/src/`.
- **UI**: `frontend/src/` (React 18, Redux Toolkit, TypeScript).

Public APIs in Rust should have `///` comments. Match existing naming in the file you edit.

## Commits

Short imperative subject, focused diffs. Do not commit secrets, `target/`, or `node_modules/`.

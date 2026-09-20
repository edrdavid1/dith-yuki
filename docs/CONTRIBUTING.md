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

After `npm install`, `patch:flexlayout` must run (postinstall). Without it, FlexLayout popouts are unstable in Tauri.

## Layout

- **Rust engines** live in `crates/`. Document model and filters: `engine-project`. GPU: `engine-gpu` (Path B).
- **Tauri glue** (commands, workers, `tile://`): `src-tauri/src/`.
- **UI**: `frontend/src/` (React 18, Redux Toolkit, TypeScript, flexlayout-react 0.7.15).
- **Docs**: [docs/README.md](./README.md) is as-built. [`.cursor-spec/`](../.cursor-spec/README.md) is only open work + frozen decisions.

Public APIs in Rust should have `///` comments. Match existing naming in the file you edit.

## Releases / alpha

See [RELEASE.md](./RELEASE.md). Quick checks:

```bash
npm run release:verify
```

Cut an alpha with a pre-release tag (`v0.3.0-alpha.1`). Feedback goes through the [bug report template](../.github/ISSUE_TEMPLATE/bug_report.yml).

## Commits

Short imperative subject, focused diffs. Do not commit secrets, `target/`, or `node_modules/`.

# Security — Dither file formats

Threat model and engineering rules for `.dyproj` / `.dyuki`. Companion to
[`FORMAT.md`](./FORMAT.md) and SPEC §3 / §6–§9.

## Threat model (summary)

| ID | Threat | Mitigation |
|---|---|---|
| T1–T4 | Zip Slip / unsafe names / symlinks / encryption | `secure_zip` allowlist + syntax checks |
| T5–T7 | Zip bombs / ratio / entry count | `ArchiveLimits` + actual-byte budgets |
| T8–T9 | Hostile JSON | depth limit, reject duplicate keys |
| T10–T11 | Hostile PNG | IHDR peek before alloc; decoder byte caps |
| T12 | Kind / MIME confusion | `kind` + `mimetype` (aliases on read) |
| T13 | Hash substitution | BLAKE3 basenames + `manifest.files` SHA-256 |
| T14 | Executable content in params | data-only filters; audit in `FORMAT_DECISIONS` |
| T15–T16 | XSS / path injection in UI | text-only display; CSP; `sanitize_*`; no `innerHTML` |
| T17 | Panic → crash | `catch_loader_panic` + `spawn_blocking` JoinError |
| T18 | Privacy leak on share | Share Copy defaults; path scrub; privacy scan test |

## Hard rules for developers

**Strings from a file never become:**

- HTML (`innerHTML`, `dangerouslySetInnerHTML`, `v-html`)
- Shell / process arguments
- IPC command names or event names
- Filesystem paths (except after sandbox + extension whitelist from dialogs/recents)
- Format strings / regex patterns
- Network URLs that the app fetches automatically

**Always:**

- Open archives only via `SecureZipArchive`
- Display names through framework text nodes after `sanitize_display_string`
- Suggest Save As names through `sanitize_filename`
- Escape XML on SVG export (`escape_xml`); never emit `<script>`, `foreignObject`, or external `href` from file data
- Prefix CSV/text cells that start with `= + - @` when exporting user-controlled strings

## Limits

See the limits table in [`FORMAT.md`](./FORMAT.md) (must match `limits.rs`).

## CSP / frontend checks

- Production CSP in `src-tauri/tauri.conf.json`: no `unsafe-eval` / script
  `unsafe-inline`; `img-src` allows `tile:` and asset protocols.
- CI/local: `npm run lint:html` (forbids HTML sinks), `npm run lint:ipc`
  (IPC via `shared/ipc`; audited FlexLayout callers on a baseline allowlist —
  see `INVOKE_AUDIT.md`).

## Fuzzing

Targets live in `fuzz/` (excluded from the workspace; see `fuzz/README.md`):
`fuzz_open_dyproj`, `fuzz_open_dyuki`, `fuzz_manifest`, `fuzz_png_limits`,
`fuzz_migrate`. Seed corpus = goldens (+ mutated coverage from local runs).

- **Local DoD:** `cargo +nightly fuzz run <target> -- -max_total_time=1800`
  per target; any crash becomes a regression under
  `crates/engine-project/tests/`.
- **CI (every PR):** `cargo test -p engine-project --test fuzz_mutation`
  (byte mutations of goldens + proptest random bytes — typed error, no panic).
- **Nightly CI job** that runs the full 30‑minute schedule: still deferred.

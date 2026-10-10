# Documentation (developers)

As-built notes for Dither Yuki. Product overview and install: [root README](../README.md).

**Canon (read these first):** [`architecture.md`](./architecture.md) · [`tile-pipeline.md`](./tile-pipeline.md) · [`FLEXLAYOUT_DOCKING.md`](./FLEXLAYOUT_DOCKING.md) · [`gpu-as-built.md`](./gpu-as-built.md) · [`crash-recovery.md`](./crash-recovery.md)

Working agent specs stay local only (not in this repo on GitHub).

## Dev

| Doc | Contents |
|---|---|
| [dev-setup.md](./dev-setup.md) | Setup, tests, conventions |
| [RELEASE.md](./RELEASE.md) | Tags, updater secrets, notarization, alpha |
| [HOW_TO_ADD_ALGORITHM.md](./HOW_TO_ADD_ALGORITHM.md) | Built-in `FilterAlgorithm` checklist |
| [SIGNING_ALPHA.md](./SIGNING_ALPHA.md) | Alpha signing / Gatekeeper notes |

## As-built

| Doc | Contents |
|---|---|
| [architecture.md](./architecture.md) | Stack, crates, IPC, preview, cost model |
| [product-snapshot.md](./product-snapshot.md) | What’s real vs still beta (1.0.x) |
| [multi-doc-tabs.md](./multi-doc-tabs.md) | Tabs, sessions, shared cache, save/export |
| [crash-recovery.md](./crash-recovery.md) | Atomic save, journals, clean-exit, soft discard, roster |
| [tile-pipeline.md](./tile-pipeline.md) | 256×256 tiles, coords, ED, GPU routing |
| [gpu-as-built.md](./gpu-as-built.md) | Path B resident GPU, auto-dispatch |
| [ascii-as-built.md](./ascii-as-built.md) | ASCII effect (CPU path, export, preview toggle) |
| [haptics-as-built.md](./haptics-as-built.md) | macOS slider Taptic feedback |
| [FLEXLAYOUT_DOCKING.md](./FLEXLAYOUT_DOCKING.md) | Layers / Effect / Color Lab / Preview docking |
| [palette-dither.md](./palette-dither.md) | Bound palette vs dither filter |
| [color-lab.md](./color-lab.md) | Palettes, Oklab, Color Lab UI |
| [soft-proof-cmyk.md](./soft-proof-cmyk.md) | Soft proof: ICC CMYK preview, BPC, encode path |

## Formats, previews, security

| Doc | Contents |
|---|---|
| [FORMAT.md](./FORMAT.md) | `.dyproj` / `.dyuki` on-disk contract |
| [FORMAT_DECISIONS.md](./FORMAT_DECISIONS.md) | Format ADR trail |
| [FORMAT_PR_CHECKLIST.md](./FORMAT_PR_CHECKLIST.md) | PR checklist for format changes |
| [schema/](./schema/) | JSON Schema for manifest / document / filters / palettes |
| [PREVIEWS.md](./PREVIEWS.md) | Quick Look / Explorer thumbnail architecture |
| [ALPHA_PREVIEWS_MACOS.md](./ALPHA_PREVIEWS_MACOS.md) | macOS preview QA |
| [ALPHA_PREVIEWS_WINDOWS.md](./ALPHA_PREVIEWS_WINDOWS.md) | Windows shell preview QA |
| [SECURITY.md](./SECURITY.md) | Zip / PNG / preview threat model |

## Performance notes

| Doc | Contents |
|---|---|
| [PERF_DECISIONS.md](./PERF_DECISIONS.md) | Perf ADRs |
| [PERF_RESIZE_BASELINE.md](./PERF_RESIZE_BASELINE.md) | Live-resize baseline |

## Legal

| Doc | Contents |
|---|---|
| [legal/USER_AGREEMENT.txt](./legal/USER_AGREEMENT.txt) | End-user agreement |
| [legal/INSTALLER_ACCEPTANCE.txt](./legal/INSTALLER_ACCEPTANCE.txt) | Installer acceptance text |
| [legal/flexlayout-license-snapshot.md](./legal/flexlayout-license-snapshot.md) | MIT snapshot for flexlayout-react 0.7.15 |

# Implementation Plan: Track O — In-app updates

План: [requirements.md](./requirements.md), [design.md](./design.md).

**Gate:** none. **Locked:** official updater plugin; `dialog: false`; GitHub `latest.json`; Guard before download; plain semver; first updater build = `0.2.0`.

**Порядок:** O0 → O1 → O2 → O3 → O4 → O5. O5 can overlap O2 once pubkey exists. O3 before wiring Install in O2 (or land Install disabled until Guard exists — do not ship Install without Guard).

---

## 0. Baseline

- [ ] 0.1 Inventory
  - `tauri.conf.json` version / bundle targets / `signingIdentity`
  - `Cargo.toml` version; `env!("CARGO_PKG_VERSION")` call sites (save_project, pattern)
  - `capabilities/default.json`
  - Help dropdown (empty); Preferences About (no version)
  - Frontend IPC error surfacing for `open_project` / `import_pattern`
  - Confirm no existing updater/process plugin
  - _Requirements: 1, 5, 6_

- [ ] 0.2 Link docs
  - Point this folder from `RELEASE_TRACKS.md` and `tech-debit.md`
  - _Requirements: n/a_

**§0.1 result (fill in):**

```
Date:
tauri.conf version / Cargo version:
Help / About:
open_project / import_pattern error UI:
Gate: proceed O1
```

---

## 1. O1 — Plugin + config + capabilities

- [ ] 1.1 Add `tauri-plugin-updater` (desktop target cfg) and `tauri-plugin-process`
  - Register in `main.rs` setup
  - Frontend packages `@tauri-apps/plugin-updater` and `@tauri-apps/plugin-process`
  - _Requirements: 1.1_

- [ ] 1.2 `tauri.conf.json`
  - `bundle.createUpdaterArtifacts: true`
  - `plugins.updater.pubkey` (generated; **not** a placeholder that verifies nothing)
  - `plugins.updater.endpoints`: GitHub latest.json URL from design
  - `plugins.updater.dialog`: false
  - `dangerousInsecureTransportProtocol` absent/false
  - _Requirements: 1.2–1.3, 2.1–2.2_

- [ ] 1.3 Capabilities
  - updater check + download-and-install; process relaunch
  - Same window list as `default.json`
  - _Requirements: 1_

- [ ] 1.4 Debug skip
  - Launch auto-check compiled out or early-return on `debug_assertions`
  - _Requirements: 2.3_

**Operator (once, not in git):** `tauri signer generate`; store private key in GH secrets. Pubkey commit is expected.

---

## 2. O2 — Check UI

- [ ] 2.1 Shared frontend module (e.g. `frontend/src/shared/updates.ts`)
  - `checkForAppUpdate()` → none / available / error
  - `installAndRelaunch()` calls Guard then plugin
  - _Requirements: 3_

- [ ] 2.2 Help → Check for Updates…
  - Fill the empty Help dropdown
  - Disable while in-flight
  - _Requirements: 3.5_

- [ ] 2.3 Preferences About
  - Show running version (same string as tauri.conf)
  - Same check action
  - _Requirements: 3.5, 6.1_

- [ ] 2.4 Launch prompt (release only)
  - `AppLayout` after mount + 3s delay
  - Modal: notes + Later / Install and Restart
  - Later = dismiss, no download
  - _Requirements: 3.2–3.4_

- [ ] 2.5 Download progress + cancel
  - Determinate if content-length known; cancel leaves old binary
  - _Requirements: 3.6_

---

## 3. O3 — Restart_Guard

- [ ] 3.1 Dialog when `hasDocument`
  - Save and Restart / Restart without saving / Cancel
  - Cancel → no `downloadAndInstall`
  - _Requirements: 4.1–4.2_

- [ ] 3.2 Save path
  - `project_path` set → `save_project`; else Save As
  - Save error → abort
  - _Requirements: 4.4_

- [ ] 3.3 RTL
  - Cancel does not invoke relaunch
  - No document → no extra prompt (Install goes straight to download)
  - _Requirements: 4, 7_

Hook for future dirty-flag: **Track P P1** `runUnsavedGuard` + skip if
`!dirty`. Do not invent dirty state here.

---

## 4. O4 — Too_New_File

- [ ] 4.1 Classify IPC errors from open_project / import_pattern / open_image-adjacent project errors
  - Match UnsupportedVersion / AppVersionTooOld / unknown-enum families
  - _Requirements: 5.1–5.2_

- [ ] 4.2 Dialog action Check for Updates… → O2 check
  - Already-latest: keep original error + “this app is up to date”
  - _Requirements: 5.3_

---

## 5. O5 — Release pipeline + version bump

- [ ] 5.1 GitHub Actions macOS job on `v*` tags
  - Build with signing env; upload tar.gz / sig / dmg / latest.json
  - Fail if signing secrets missing
  - _Requirements: 7.1–7.3_

- [ ] 5.2 Version bump to `0.2.0` in **both** `tauri.conf.json` and `src-tauri/Cargo.toml` when this track ships
  - Note in README / beta notes: 0.1.0 → DMG once
  - _Requirements: 6.2–6.3_

- [ ] 5.3 Manual QA checklist
  - [ ] Release build: Help check against a newer `latest.json` (or skip if no tag yet — document)
  - [ ] Later does not download
  - [ ] Guard cancel
  - [ ] `tauri dev` does not prompt on launch
  - [ ] Future-format fixture → Check action
  - _Requirements: 2.4, 3, 4, 5_

- [ ] 5.4 Docs
  - Short ARCHITECTURE / README note: updater endpoint, two signatures, 0.1.0 first hop
  - _Requirements: 1.5, 6.3, 7.4_

---

## Definition of Done

- [ ] Release macOS build can `check` a newer GitHub `latest.json`, verify Minisign, install, relaunch
- [ ] Debug / `tauri dev` does not auto-prompt
- [ ] Help + About show version and check
- [ ] Open document cannot relaunch without Guard
- [ ] Too_New_File offers the same check
- [ ] Private key not in git; CI fails closed without secrets
- [ ] 0.1.0 → 0.2.0 documented as manual DMG

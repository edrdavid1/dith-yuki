# Design: Track O — In-app updates

> **Status:** spec only. Checklist: [tasks.md](./tasks.md).
> Requirements: [requirements.md](./requirements.md).

## Overview

| ID | Deliverable | Notes |
|----|-------------|-------|
| **O1** | Plugin + pubkey + capabilities | `tauri-plugin-updater` + `tauri-plugin-process`; debug skips launch check |
| **O2** | Check / prompt / download UI | Help + About; plugin `dialog: false` |
| **O3** | Restart_Guard | Before install+relaunch; reuse save_project |
| **O4** | Too_New_File action | Same check thunk from serialize errors |
| **O5** | Release workflow | Tag → macOS updater artifacts + `latest.json` |

**Gate:** none from A–N. Soft coupling: dirty-flag is **Track P**
([track-p-beta/](../track-p-beta/) P1 Saved_Mark). O3 skips Guard when
`!dirty`. If O3 lands first, keep treating `hasDocument` as dirty (do
not invent a second flag here).

---

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Signed macOS self-update from GitHub Releases | Custom server, Sparkle, Windows/Linux v1 |
| Prompted install + Restart_Guard | Silent install, skip-this-version |
| Wire “update the app” file errors | Change E/F migration policy |

---

## Locked decisions

| Topic | Decision |
|-------|----------|
| Mechanism | **`tauri-plugin-updater`** + **`tauri-plugin-process`** (`relaunch`). No custom HTTP replace |
| Native plugin dialog | **`dialog: false`**. Custom UI (Help is empty; MenuBar is custom) |
| Endpoint | `https://github.com/edrdavid1/dith-yuki/releases/latest/download/latest.json` |
| Channels | **One** (`latest.json`) until 1.0. No Preferences channel toggle |
| Arch | CI publishes whatever it builds. Prefer `macos-latest` → `darwin-aarch64`; Intel is a follow-up matrix row, not a v1 blocker |
| Minisign | `tauri signer generate` once; pubkey in `tauri.conf.json`; private key only in GitHub Actions secrets |
| Apple_Code_Sign | **Not** this track’s blocker. Document Gatekeeper. Do not set `signingIdentity` to a fake value |
| Semver | Plain `x.y.z`. No `-beta.N` in `CARGO_PKG_VERSION` / `tauri.conf.json` `version` |
| Version bump | First updater-capable release = **`0.2.0`**. 0.1.0 cannot self-update |
| Launch check | After main window shown + **3s** delay; **skipped** when `cfg!(debug_assertions)` |
| Download | Only after user clicks Install. Check ≠ download |
| Restart_Guard timing | Guard **before** `downloadAndInstall`. Cancel = still on old binary, no partial swap |
| Open document | Any `hasDocument` ⇒ confirm. **Track P** dirty-flag: prompt only if dirty. Reuse `runUnsavedGuard` from P1; do not a second three-button modal |
| Save and Restart | `project_path Some` → `save_project`; `None` → existing Save As dialog; failure aborts |
| Too_New_File | Frontend maps known error strings/codes to a dialog with **Check for Updates…**; backend messages stay as they are |
| `tauri.conf` / Cargo version | Single source: bump both in the same release commit (no codegen in v1) |
| Permissions | `updater:default` (or explicit check + download-and-install) + `process:allow-relaunch` on the existing `default` capability |
| JS vs Rust check | Frontend `@tauri-apps/plugin-updater` + `@tauri-apps/plugin-process` from Help/About. Launch check from `AppLayout` after mount. Do not duplicate a second Rust-only prompt |

---

## Two signatures (do not conflate)

```text
GitHub Release
  ├── Dither.app.tar.gz          ← updater payload
  ├── Dither.app.tar.gz.sig      ← Minisign (embedded pubkey verifies this)
  ├── Dither.dmg                 ← human install (0.1.0 → 0.2.0 first hop)
  └── latest.json                ← { version, notes, platforms[target].url/signature }

Apple Developer ID (optional, Gatekeeper)
  └── signs the .app / .dmg itself
```

Minisign answers “did we publish this bytes?”. Apple answers “does macOS trust this developer?”. An unsigned-but-minisigned beta still self-updates for testers who already launched the app; Gatekeeper only bites the **first** DMG open and some notarization-sensitive replacements. Call that out in the beta README, do not block O on an Apple account.

---

## Current → Target

```text
Today
  version 0.1.0, Help empty, About has no version
  no updater plugin, no CI, signingIdentity null
  Too_New_File = dead-end error string

Target
  0.2.0 ships plugin + pubkey
  Help → Check for Updates…
  About shows 0.2.0 + Check
  launch check (release builds only)
  tag v0.2.1 → latest.json → installed 0.2.0 offers Install
  Too_New_File → same Check action
```

---

## UX

### Launch (release)

1. Main window visible (Welcome or document).
2. Wait 3s. `check()`.
3. No update / error → nothing.
4. Update available → modal (app-styled): title “Update available”, `version`, `notes`, buttons Later / Install and Restart.
5. Later: close modal; no download. Next chance = next launch or Help.

### Manual

Help → Check for Updates… (and About button):
- Checking… (disable double-click)
- Up to date → short status
- Update available → same modal as launch
- Network/signature error → status text, binary untouched

### Install

1. Restart_Guard if `hasDocument`.
2. `downloadAndInstall` with progress callback.
3. `relaunch()`.

Do not persist “skip 0.2.1”. Later is per-session for the launch prompt.

---

## Too_New_File wiring

Existing Rust errors already tell the user to update:

- `ProjectError::UnsupportedVersion { kind, found, supported }`
- `ProjectError::AppVersionTooOld { .. }`
- unknown `FilterKind` / `DitherModeV2` serde (F safety net)

Frontend open/import thunks already surface IPC errors. O4: shared helper `offerUpdateFromFileError(message)` — if the text/code matches those three families, show the error **and** Check for Updates…. Do not parse `format_version` in the UI; do not soften the hard-fail.

If check returns “already latest”: keep the file error; add one line that this build cannot read the file (dev/newer private build).

---

## Version vs file formats

Three clocks, already in the product:

| Clock | What it is | Who bumps it |
|-------|------------|--------------|
| App semver `0.2.1` | Updater + About + `app_version` in `.dyproj` manifest | This track / release process |
| `format_version` u32 | Zip schema, per-kind ladders | Track E/F migrate modules |
| `.dyuki` `app_version_min` | Min app that can *run* the included kinds | `min_app_version_for_filters` |

Updater never bumps `format_version`. A 0.2.1 that only fixes CRT still writes `format_version: 1`. A 0.3.0 that changes the zip layout bumps the ladder **and** the app version; old apps fail Too_New_File and O4 points at the updater.

**First hop:** 0.1.0 has no plugin. Beta testers install 0.2.0 from a DMG **once**. After that, 0.2.1+ is in-app.

---

## CI sketch

```text
on: push tags v*
jobs:
  macos:
    runs-on: macos-latest
    steps:
      - tauri-action / cargo tauri build
        env:
          TAURI_SIGNING_PRIVATE_KEY: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
          TAURI_SIGNING_PRIVATE_KEY_PASSWORD: ${{ secrets.TAURI_SIGNING_PRIVATE_KEY_PASSWORD }}
      - upload Dither.app.tar.gz, .sig, dmg, latest.json to GH Release
```

If secrets missing: fail the job (do not attach a fake `latest.json`).
`includeUpdaterJson` on tauri-action is the default to prefer over hand-written JSON.

Operator one-time:
1. `npm run tauri signer generate -w ~/.tauri/dither.key`
2. Paste `.pub` into `plugins.updater.pubkey`
3. Put private key + password in repo secrets
4. Tag `v0.2.0`

---

## Capabilities

Existing `src-tauri/capabilities/default.json` needs updater + process relaunch. Scope to the same window labels already listed (`main` + panel-* ). Do not add `http` permissions for a custom fetch — the plugin uses its own client.

---

## Tests

| Case | How |
|------|-----|
| Debug launch | No outbound check (unit/flag), Help still callable |
| Manifest newer | Mock or documented manual QA against a test endpoint in debug with `dangerousInsecureTransportProtocol` **off the table for prod config** |
| Signature mismatch | Manual / plugin-level: install must not proceed (do not weaken pubkey in tests committed to prod conf) |
| Restart_Guard cancel | RTL: open doc + Install → Cancel → no relaunch invoke |
| Save and Restart | `project_path` set → save invoked; save reject → no relaunch |
| Too_New_File | Open fixture `format_version: 99` → dialog has Check action |
| Version display | About shows the same string as `tauri.conf.json` |

Live GitHub check is **manual QA**, not CI.

---

## Risks

1. **0.1.0 cannot update itself.** First beta communication must include a DMG. After 0.2.0, in-app works.
2. **Lost Minisign key** = ship a new app id / ask everyone to reinstall. Backup the key offline, not only in GitHub.
3. **Private repo + `latest.json`.** `edrdavid1/dith-yuki` must be public for an unauthenticated endpoint, **or** the updater needs an auth header (not v1). Lock: **public release assets**. If the repo stays private, host `latest.json` + tarballs on a public HTTPS bucket instead — same plugin config, different URL. Decide at O5 start; default GitHub public Releases.
4. **Apple replacement of .app** on newer macOS can fight unsigned updates. If testers report “update downloaded but app didn’t change”, that is the Gatekeeper follow-up, not a reason to write a custom updater.

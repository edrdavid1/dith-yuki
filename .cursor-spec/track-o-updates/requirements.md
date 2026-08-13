# Requirements: Track O — In-app updates

## Introduction

Бета без канала обновлений — это «скачай новый DMG с GitHub». Для
инструмента с `.dyproj` / `.dyuki` это ещё и продуктовая дыра: файл из
новой версии уже говорит «обновите приложение», а обновить нечем.

Этот трек — **не** свой HTTP-инсталлер. Tauri 2 уже даёт
`tauri-plugin-updater` (Ed25519-подпись артефакта, `latest.json`) и
`tauri-plugin-process` (relaunch). Задача — встроить их в существующий
шелл (кастомный `MenuBar`, Preferences About, ошибки `format_version` /
`app_version_min`) и в CI, которого в репо ещё нет.

Карта: [RELEASE_TRACKS.md](../RELEASE_TRACKS.md). Независим от H–N / C4.1.
Dirty-state документа — [track-p-beta/](../track-p-beta/) (P1). Этот трек
его **не** реализует; Restart_Guard обязан работать и без него (`hasDocument`
= potentially dirty). Когда P1 в дереве, Guard пропускает clean.

Текущий as-built: `tauri.conf.json` version `0.1.0`, `signingIdentity: null`,
bundle только `app` + `dmg`, Help-меню пустое, Preferences About без номера
версии, remote `edrdavid1/dith-yuki`.

## Glossary

- **Updater_Plugin**: `tauri-plugin-updater` — check / download / verify /
  install. Подпись **нельзя** отключить.
- **Minisign_Keypair**: ключи `tauri signer generate`. Public →
  `tauri.conf.json` `plugins.updater.pubkey`. Private → CI secret
  `TAURI_SIGNING_PRIVATE_KEY`, никогда в git.
- **Apple_Code_Sign**: Developer ID + notarization `.app` / `.dmg`. Это
  **другая** подпись, не Minisign. Нужна Gatekeeper'у, не updater'у.
- **Latest_Json**: статический манифест `{ version, notes, pub_date, platforms }`,
  который плагин качает по HTTPS.
- **Restart_Guard**: диалог перед relaunch, чтобы не убить несохранённый
  документ.
- **Too_New_File**: ошибка открытия `.dyproj` (`format_version` из будущего)
  или импорта `.dyuki` (`app_version_min` / unknown enum) — уже есть в
  serialize; этот трек даёт из неё действие «Check for Updates».
- **Update_Channel**: в v1 один канал (`latest.json`). Beta/stable split —
  follow-up после 1.0.

## Goals / Non-Goals

| Goals | Non-Goals |
|-------|-----------|
| Подписанные in-app updates на macOS (текущий bundle) | Свой updater / Sparkle / ручной «скачай DMG» как единственный путь |
| Check on launch + Help / About | Тихая установка без подтверждения |
| Restart_Guard перед relaunch | Реализовать dirty-flag документа (соседний бета-блокер) |
| Too_New_File → Check for Updates | Менять `format_version` / `app_version_min` политику Track E/F |
| Release workflow, который кладёт `.tar.gz` + `.sig` + `latest.json` | Windows / Linux бандлы (появятся — расширить endpoint) |
| Версия в About из того же semver, что `CARGO_PKG_VERSION` | Каналы beta/stable UI, staged rollout, delta patches |

---

## Requirements

### Requirement 1: Official updater, signed artifacts

**User Story:** As a user, I want the app to install only builds that you signed, so a hijacked download cannot replace the binary.

#### Acceptance Criteria

1. THE app SHALL use `tauri-plugin-updater` (desktop only) and SHALL NOT implement a custom download-and-replace path.
2. `tauri.conf.json` SHALL set `bundle.createUpdaterArtifacts` to `true` and `plugins.updater.pubkey` to the Minisign public key (inline string, not a file path).
3. THE updater SHALL reject an artifact whose signature does not match the embedded pubkey. Signature verification SHALL NOT be disable-able in production.
4. Private key material SHALL live only in operator secrets (`TAURI_SIGNING_PRIVATE_KEY`, optional `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`). THE repository SHALL NOT contain the private key, a `.env` with it, or a committed key file.
5. Apple_Code_Sign / notarization MAY be absent for the first internal beta (today `signingIdentity` is `null`). THE design SHALL treat it as a **distribution** prerequisite for Gatekeeper, not as a substitute for Minisign. Losing the Minisign private key SHALL be documented as “cannot push updates to already-installed copies.”

### Requirement 2: Single static endpoint (GitHub Releases)

**User Story:** As a maintainer, I want one HTTPS manifest URL, so beta does not need a custom update server.

#### Acceptance Criteria

1. THE production endpoint SHALL be the GitHub Releases static file
   `https://github.com/edrdavid1/dith-yuki/releases/latest/download/latest.json`
   (TLS required; `dangerousInsecureTransportProtocol` SHALL stay false).
2. v1 SHALL use **one** channel. No `beta.json` / channel picker in Preferences.
3. `tauri dev` / debug builds SHALL NOT prompt testers with a real update. Lock: skip the launch check when `cfg!(debug_assertions)` (manual Help check in debug MAY run and is allowed to fail softly).
4. IF the check request fails (offline, 404, timeout, malformed JSON), THE app SHALL stay running. Startup check: silent. Manual check: user-visible error, no panic.
5. Windows/Linux platform keys in `latest.json` are out of v1; macOS SHALL publish `darwin-aarch64` and `darwin-x86_64` if both are built, otherwise the arch that CI actually produces.

### Requirement 3: Prompted install, never silent

**User Story:** As a user mid-edit, I want to choose when to install, so an update does not restart the app under me.

#### Acceptance Criteria

1. Plugin built-in `dialog` SHALL be **off**. UI SHALL match the custom `MenuBar` (not a native Tauri menu rebuild).
2. Launch check SHALL run after the main window is shown, delayed (lock in design: **3s**), and SHALL NOT block Welcome / New Project.
3. WHEN a newer version exists, THE UI SHALL show version + notes (from `latest.json`) and actions **Install and Restart** / **Later**. Later dismisses until the next launch or a manual check.
4. THE app SHALL NOT download or install until the user chooses Install. A launch check that finds an update is information, not a download.
5. Help menu (today empty) SHALL gain **Check for Updates…**. Preferences → About SHALL show the running version (`CARGO_PKG_VERSION` / `tauri.conf.json` version — they MUST stay in sync) and the same check action.
6. Progress MAY be a simple determinate bar during download; cancel SHALL abort the download and leave the running binary untouched.

### Requirement 4: Restart_Guard

**User Story:** As a user with an open project, I want a chance to save before the updater relaunches the app.

#### Acceptance Criteria

1. Before `relaunch`, THE app SHALL run Restart_Guard. It SHALL NOT call `tauri-plugin-process` relaunch unconditionally.
2. WHILE a document is open (`hasDocument`), Restart_Guard SHALL offer **Save and Restart** / **Restart without saving** / **Cancel**. Cancel aborts install relaunch; the new binary SHALL NOT be left half-applied if the plugin has already swapped the bundle — lock in design: run Guard **before** `downloadAndInstall`, or document the plugin’s “install then relaunch” contract and Guard before that combined call.
3. IF a dirty-flag exists when that neighboring work lands, Restart_Guard SHALL skip the prompt when the document is clean. Until then, treat any open document as potentially dirty (do not invent a second dirty system in this track).
4. **Save and Restart** SHALL reuse existing `save_project` / `save_project_as` (if `project_path` is `None`, Save As then restart). IF save fails, do not relaunch.
5. Undo history (Track N) is in-memory; relaunch clearing it is expected. Do not persist undo across update.

### Requirement 5: Too_New_File offers an update

**User Story:** As a user opening a `.dyproj` or `.dyuki` from a newer app, I want to update from that error, not hunt for a DMG.

#### Acceptance Criteria

1. User-facing errors for `UnsupportedVersion` (future `format_version`) and `AppVersionTooOld` / unknown-enum import SHALL include an action **Check for Updates…** that runs the same check path as Help.
2. THE serialize policy SHALL NOT change: still hard-fail, no partial load, no silent drop of filters (Track E/F locked).
3. IF check finds no update (user is already on latest, file is from a private/dev build), THE message SHALL stay the original error plus “this app is already up to date.”

### Requirement 6: Version identity

**User Story:** As a maintainer, I want one semver that the updater, About, and `.dyuki` `app_version_min` all understand.

#### Acceptance Criteria

1. `src-tauri/tauri.conf.json` `version` and `src-tauri/Cargo.toml` `version` SHALL remain equal. Release process SHALL bump both (or generate one from the other — lock in design).
2. Beta SHALL use **plain** `major.minor.patch` triples (`0.2.0`, `0.2.1`, …), **not** `0.2.0-beta.N`. Rationale: `check_app_version_min` already strips pre-release suffixes; mixing pre-release in the updater and ignoring it in files is a footgun.
3. First beta that ships the updater is a version bump from `0.1.0` (the installed copies of 0.1.0 cannot self-update unless they already contained the plugin — they do not). Document: **updater starts working from the first build that includes this track**; 0.1.0 users still download a DMG once.
4. `min_app_version_for_filters` tables SHALL keep being updated when new filter kinds ship; this track does not rewrite that table.

### Requirement 7: Release pipeline

**User Story:** As a maintainer, I want tagging a release to publish updater artifacts, so I do not hand-assemble `latest.json`.

#### Acceptance Criteria

1. THE repo SHALL gain a GitHub Actions workflow that builds the macOS app, signs updater artifacts with the Minisign secret, and uploads `.app.tar.gz`, `.sig`, and `latest.json` to the GitHub Release for that tag.
2. `latest.json` `version` SHALL match the tag’s app version. Notes MAY come from the GitHub release body.
3. Workflow SHALL no-op or fail closed if signing secrets are missing (do not publish an unsigned updater payload as if it were signed).
4. Apple notarization is **optional** in this track (separate secrets / `APPLE_*`). If absent, the workflow still produces updater artifacts; Gatekeeper warnings stay a known beta limitation.

---

## Non-requirements (explicit)

- Sparkle / `sparkle:edSignature` parallel path
- Delta / bsdiff patches
- Update while `tauri dev` against production `latest.json`
- Auto-download on LAN without HTTPS
- Persisting “skip this version”
- In-app changelog browser beyond `notes` from the manifest

# Releasing Dither Yuki

How we cut macOS/Windows builds, sign updater artifacts, and ship a public alpha.

## Channels

| Tag example | Release title | Updater (`/releases/latest`) |
|---|---|---|
| `v0.3.0-alpha.1` | `Dither Yuki v0.3.0-alpha.1 (alpha)` | Becomes latest (see note) |
| `v0.3.0-beta.1` / `v0.3.0-rc.1` | `… (alpha)` suffix | Same |
| `v0.3.0` | `Dither Yuki v0.3.0` | Stable latest |

Workflow: [`.github/workflows/release.yml`](../.github/workflows/release.yml) on `push` of `v*`.

**Important:** GitHub’s `/releases/latest` **ignores** releases marked as GitHub “Pre-release”. The in-app updater endpoint uses that URL, so this workflow keeps `prerelease: false` and communicates alpha via the **tag** + title suffix `(alpha)`. When you later run stable + alpha in parallel, add a separate `alpha.json` endpoint instead of relying on GitHub prerelease flags.

App endpoint (hardcoded in `tauri.conf.json`):

`https://github.com/edrdavid1/dith-yuki/releases/latest/download/latest.json`

```bash
chmod +x scripts/verify-release-chain.sh
npm run release:verify
# or: ./scripts/verify-release-chain.sh
```

Checks:

1. Updater pubkey + `createUpdaterArtifacts` in `src-tauri/tauri.conf.json`
2. Local `TAURI_SIGNING_*` env (if present)
3. Optional Apple notarization env
4. Reachability of published `latest.json`
5. GitHub repo secrets via `gh` (when authenticated)

### Required GitHub secrets (updater)

| Secret | Purpose |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Minisign private key (CI refuses to publish without it) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | Key password, or empty |

Generate once (never commit the private key):

```bash
npm run tauri signer generate -w ~/.tauri/dither.key
# Put the public key into plugins.updater.pubkey (already done for this repo).
# Put the private key contents into the GitHub secret.
```

### Apple secrets (public alpha / no Gatekeeper warn)

| Secret | Purpose |
|---|---|
| `APPLE_CERTIFICATE` | Base64 of Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | `.p12` password |
| `APPLE_SIGNING_IDENTITY` | e.g. `Developer ID Application: Name (TEAMID)` (optional if CI can detect) |
| `KEYCHAIN_PASSWORD` | Temp keychain password on the runner |
| `APPLE_ID` | Apple ID email |
| `APPLE_PASSWORD` | App-specific password |
| `APPLE_TEAM_ID` | 10-character Team ID |

When `APPLE_CERTIFICATE` is unset, CI creates a **self-signed** identity `L'eco non di Bergamo` (`scripts/macos-self-sign-cert.sh`) so Gatekeeper can offer **Open Anyway**. This is not Apple Developer ID / notarization.

## Cut a release

1. Bump version in lockstep: root `package.json`, `frontend/package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
2. Update release notes intent (alpha scope: no paint / ICC / video).
3. Commit, then:

```bash
git tag -a v0.3.0-alpha.1 -m "Dither Yuki 0.3.0-alpha.1"
git push origin v0.3.0-alpha.1
```

4. Watch Actions → **Release**. macOS job creates the GitHub Release; Windows attaches NSIS.
5. Smoke: install DMG → Help → Check for Updates (should report up to date).
6. Optional: `npm run release:verify` after assets are public.

## Gatekeeper (self-signed macOS alpha)

1. Drag the app to Applications and **double-click** once (expect a block).
2. **System Settings → Privacy & Security** → **Open Anyway**.
3. Confirm **Open**.

Self-signed as **L'eco non di Bergamo**. With Apple Developer secrets + notarization, this prompt goes away.

## Feedback

- Issues: [bug report template](../.github/ISSUE_TEMPLATE/bug_report.yml)
- Download landing: [`site/`](../site/) (GitHub Pages via `.github/workflows/pages.yml`)

Enable Pages once: repo **Settings → Pages → Source: GitHub Actions**, then push to `main` (or run **Deploy download site** manually). Expected URL: `https://edrdavid1.github.io/dith-yuki/`.

## License note for download pages

Source is **Fair Core License 1.0** (MIT future license) — say “source available”, not “MIT open source today”.

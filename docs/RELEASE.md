# Releasing Dither Yuki

How we cut macOS/Windows builds, sign updater artifacts, and ship a public beta.

## Channels

| Tag example | Release title | Updater (`/releases/latest`) |
|---|---|---|
| `v0.4.5-alpha` | `Dither Yuki v0.4.5-alpha (alpha)` | Becomes latest (see note) |
| `v1.0.0-beta` | `Dither Yuki v1.0.0-beta (beta)` | Same |
| `v1.0.0-beta.2` / `v1.0.0-rc.1` | channel suffix from tag (`beta` / `alpha`) | Same |
| `v1.0.0` | `Dither Yuki v1.0.0` | Stable latest |

Workflow: [`.github/workflows/release.yml`](../.github/workflows/release.yml) on `push` of `v*`.

**Important:** GitHub’s `/releases/latest` **ignores** releases marked as GitHub “Pre-release”. The in-app updater endpoint uses that URL, so this workflow keeps `prerelease: false` and communicates the channel via the **tag** + title suffix (`(alpha)` / `(beta)`). When you later run stable + pre-release in parallel, add a separate channel endpoint instead of relying on GitHub prerelease flags.

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

### Apple secrets (public beta / no Gatekeeper warn)

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

Full signing guide (macOS + Windows self-sign): [`SIGNING_ALPHA.md`](./SIGNING_ALPHA.md).

## Cut a release

1. Bump version in lockstep: root `package.json`, `frontend/package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
2. Update release notes intent (beta scope: no paint / ICC / video).
3. Commit, then:

Beta builds use `1.0.0-beta` (then `1.0.0-beta.2`, …). Legacy alphas used `0.4.N-alpha`.

```bash
git tag -a v1.0.0-beta -m "Dither Yuki 1.0.0-beta"
git push origin v1.0.0-beta
```

4. Watch Actions → **Release**. macOS job builds Quick Look `.appex`, creates the
   GitHub Release; Windows builds `dither_shell.dll` and attaches NSIS.
5. Smoke: install DMG → Help → Check for Updates (should report up to date).
6. Preview smoke: save a `.dyproj`, check Finder Space / Explorer large icons
   ([ALPHA_PREVIEWS_MACOS.md](./ALPHA_PREVIEWS_MACOS.md) /
   [ALPHA_PREVIEWS_WINDOWS.md](./ALPHA_PREVIEWS_WINDOWS.md)).
7. Optional: `npm run release:verify` after assets are public.

## Gatekeeper (self-signed macOS beta)

1. Drag the app to Applications and **double-click** once (expect a block).
2. **System Settings → Privacy & Security** → **Open Anyway**.
3. Confirm **Open**.

Self-signed as **L'eco non di Bergamo**. With Apple Developer secrets + notarization, this prompt goes away.

## Feedback

- Issues: [bug report template](../.github/ISSUE_TEMPLATE/bug_report.yml)
- Download landing: [`site/`](../site/) (GitHub Pages via `.github/workflows/pages.yml`)

Enable Pages once: repo **Settings → Pages → Source: GitHub Actions**, then push to `main` (or run **Deploy download site** manually). Expected URL: `https://edrdavid1.github.io/dith-yuki/`.

## License note for download pages

Source is under the **L'eco non di Bergamo Software License** — see [LICENSE](../LICENSE) and [USER_AGREEMENT.txt](./legal/USER_AGREEMENT.txt) for the desktop app.

# Alpha signing (self-signed / T0–T1)

Until Apple Developer ID and Windows OV/EV (or cloud) certificates exist,
**use our own signatures**. This is intentional and already how CI ships
closed-alpha macOS builds.

| Tier | Who | macOS | Windows |
|---|---|---|---|
| **T0 Dev** | you locally | ad-hoc (`codesign -s -`) or self-signed | unsigned OK |
| **T1 Alpha** | testers | self-signed **L'eco non di Bergamo** | self-signed Authenticode (or unsigned + SmartScreen bypass) |
| **T2 Public** | everyone | Developer ID + notarization | OV/EV (or cloud) + timestamp |

Preview / Quick Look / shell DLL code is the same at every tier — only packaging
and signing change. Never put `if alpha` in the preview crates.

---

## macOS (T1)

### One-time: create identity

```bash
bash scripts/macos-self-sign-cert.sh
# prints APPLE_SIGNING_IDENTITY=…
export APPLE_SIGNING_IDENTITY="L'eco non di Bergamo"   # or the hash printed
```

CI does this automatically when `APPLE_CERTIFICATE` is unset
(`.github/workflows/release.yml`).

### Sign app + Quick Look

```bash
export APPLE_SIGNING_IDENTITY="L'eco non di Bergamo"
scripts/build-quicklook.sh --tier alpha --app "/Applications/Dither Yuki.app"
```

Order: sign `.appex` first, then the `.app`. Do **not** use `codesign --deep`.

### Tester flow

1. Move app to `/Applications`, launch once.
2. Gatekeeper → **Open Anyway** (Privacy & Security).
3. Enable Quick Look extensions if needed.
4. Details: [`ALPHA_PREVIEWS_MACOS.md`](./ALPHA_PREVIEWS_MACOS.md).

---

## Windows (T1)

### One-time: create Authenticode cert (PowerShell as Admin optional)

```powershell
pwsh scripts/windows-self-sign-cert.ps1
# Creates CurrentUser\My cert "L'eco non di Bergamo" and exports .pfx (local only)
```

Never commit the `.pfx`. Store the thumbprint for signing:

```powershell
$env:WINDOWS_CERT_THUMBPRINT = "<thumbprint from script>"
```

### Sign DLL / installer

```powershell
pwsh scripts/windows-self-sign-cert.ps1 -SignPath path\to\dither_shell.dll
pwsh scripts/windows-self-sign-cert.ps1 -SignPath path\to\DitherYuki_…-setup.exe
```

Without a cert, T0/T1 may still run: SmartScreen «More info → Run anyway», and
on some Win11 machines Smart App Control must be Off/Evaluation for the test VM.
See [`ALPHA_PREVIEWS_WINDOWS.md`](./ALPHA_PREVIEWS_WINDOWS.md).

CI Windows job currently ships **unsigned** PE (updater Minisign still required).
Self-signed PE is for local/alpha tester builds until an OV/EV cert is available.

---

## What is *not* OS code signing

| Artifact | Signing |
|---|---|
| In-app updater (`latest.json` + archives) | **Minisign** (`TAURI_SIGNING_PRIVATE_KEY`) — already required in CI |
| `.dyproj` / `.dyuki` contents | not signed; integrity via `manifest.files` hashes |

---

## Switching to T2 later

1. macOS: put Developer ID `.p12` in secrets `APPLE_CERTIFICATE*`, enable notarization env.
2. Windows: OV/EV or cloud signing; sign `dither_shell.dll` + NSIS with the **same** cert + RFC 3161 timestamp.
3. Keep using `--tier public` in build scripts; no code changes in preview components.

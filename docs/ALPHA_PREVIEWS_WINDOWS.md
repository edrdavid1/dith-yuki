# Alpha: Windows system thumbnails

Phase 1: Explorer / open-dialog thumbnails via `IThumbnailProvider`.
Phase 2 (preview pane / Peek): after phase 1 + security review.

Architecture: [`PREVIEWS.md`](./PREVIEWS.md).
Signing for alpha: [`SIGNING_ALPHA.md`](./SIGNING_ALPHA.md).

## SmartScreen / Smart App Control

Preferred for alpha: **self-sign** the DLL and installer:

```powershell
pwsh scripts/windows-self-sign-cert.ps1
pwsh scripts/windows-self-sign-cert.ps1 -SignPath .\dither_shell.dll
pwsh scripts/windows-self-sign-cert.ps1 -SignPath .\DitherYuki-setup.exe -Timestamp
```

Unsigned T0 builds may be blocked. For testing without a cert:

- SmartScreen: «More info» → Run anyway.
- Smart App Control (Win11): Off / Evaluation on the test VM.
- Corporate WDAC/AppLocker: ask IT for an exception on the test machine.

## Verify (once DLL is installed)

Release NSIS builds ship `dither_shell.dll` and register ShellEx in the
installer hook. After install:

1. Open a folder of `.dyproj` / `.dyuki` in Large icons view.
2. If icons only: clear thumbnail cache (Disk Cleanup → Thumbnails) and restart
   Explorer; check that «Always show icons, never thumbnails» is off.
3. Confirm ProgID / ShellEx keys under `HKCU` or `HKLM\Software\Classes` match
   the CLSID in PREVIEWS.md (`{BC7D0A00-220F-46DD-AAA8-C754864EE648}`).

## ARM64

Explorer is native ARM64 on ARM64 Windows — an x64-only DLL will not load.
Installer must place the matching `dither_shell.dll`.

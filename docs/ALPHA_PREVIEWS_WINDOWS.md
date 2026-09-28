# Alpha: Windows system thumbnails and preview pane

Explorer thumbnails and the preview pane (Alt+P) for `.dyproj` / `.dyuki`.
Peek (PowerToys) is not confirmed; see [`FORMAT_DECISIONS.md`](./FORMAT_DECISIONS.md).

Architecture and the CLSID table: [`PREVIEWS.md`](./PREVIEWS.md).
Signing: [`SIGNING_ALPHA.md`](./SIGNING_ALPHA.md).

## SmartScreen, Smart App Control, WDAC

Alpha builds may be unsigned. That does not change thumbnail behavior. It can
block the installer or the DLL.

- SmartScreen on the installer: More info → Run anyway.
- Smart App Control (Windows 11): turn it to Off or Evaluation on the test VM, or the unsigned DLL never loads. Check Event Viewer → Microsoft → Windows → Code Integrity.
- WDAC / AppLocker: an unsigned DLL is denied until IT allows the test machine. There is no in-app switch.

Preferred alpha signing, when a cert exists:

```powershell
pwsh scripts/windows-self-sign-cert.ps1
pwsh scripts/windows-self-sign-cert.ps1 -SignPath .\dither_shell.dll
pwsh scripts/windows-self-sign-cert.ps1 -SignPath .\DitherYuki-setup.exe -Timestamp
```

Public releases sign the DLL and the installer with the same certificate and an RFC 3161 timestamp.

## If a thumbnail or the preview pane is missing

Explorer's "Always show icons, never thumbnails" disables thumbnails. That is expected.

Otherwise, from the installed app directory (or a built `dither-shell-diag.exe`):

```text
dither-shell-diag clear-cache
dither-shell-diag check
dither-shell-diag render path\to\file.dyproj --size 256 --out out.png
dither-shell-diag last-errors
```

Attach the `check` and `last-errors` output to the bug report. Those lines are registry and reason codes, not file contents.

`render` writes the bitmap the provider would hand to Explorer, including the Windows `HBITMAP` step when the tool is run on Windows. If that PNG is right and Explorer is wrong, the problem is registration or isolation. If the PNG is wrong, the problem is the bitmap path.

Thumbnails can work while Alt+P still shows nothing. Those are separate registrations (`check` lists them separately).

## ARM64

On ARM64 Windows, Explorer is a native ARM64 process. An x64 `dither_shell.dll` does not load there. The installer uses `dither_shell_arm64.dll` when that file is in the install directory. Current CI ships one DLL matching the installer architecture.

## Caches

`clear-cache` stops Explorer, deletes `thumbcache_*.db` and `iconcache*` under `%LOCALAPPDATA%\Microsoft\Windows\Explorer`, starts Explorer, and notifies the shell. Run it before every manual check.

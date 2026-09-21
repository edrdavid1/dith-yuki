#!/usr/bin/env bash
# Build dither_shell.dll into src-tauri/previews/ for tauri.windows.conf.json resources.
# Invoked by CI and by tauri beforeBundleCommand on Windows (Git Bash).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST_DIR="$ROOT/src-tauri/previews"
DEST="$DEST_DIR/dither_shell.dll"

# Host triple for the runner (msvc on windows-latest).
case "$(uname -s 2>/dev/null || echo unknown)" in
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    ;;
  *)
    # Allow cross-prep only when explicitly requested.
    if [[ "${DITHER_FORCE_WINDOWS_PREVIEW:-}" != "1" ]]; then
      echo "ci-prepare-windows-previews: skip (not Windows)"
      exit 0
    fi
    ;;
esac

TARGET="${DITHER_SHELL_TARGET:-x86_64-pc-windows-msvc}"
echo "Preparing Windows shell thumbnail DLL (target=$TARGET)…"
bash "$ROOT/scripts/build-dither-shell.sh" --target "$TARGET"

SRC="$ROOT/target/$TARGET/release/dither_shell.dll"
if [[ ! -f "$SRC" ]]; then
  # Fallback if cargo placed it without --target dir layout.
  SRC="$ROOT/target/release/dither_shell.dll"
fi
if [[ ! -f "$SRC" ]]; then
  echo "dither_shell.dll not found after build" >&2
  exit 1
fi

mkdir -p "$DEST_DIR"
cp -f "$SRC" "$DEST"
echo "Copied → $DEST"

# Self-sign for alpha (T1) when PowerShell is available.
if command -v pwsh >/dev/null 2>&1; then
  pwsh -NoProfile -File "$ROOT/scripts/windows-self-sign-cert.ps1" -SignPath "$DEST" || \
    echo "Warning: Authenticode self-sign failed; shipping unsigned DLL" >&2
elif command -v powershell.exe >/dev/null 2>&1; then
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$ROOT/scripts/windows-self-sign-cert.ps1" -SignPath "$DEST" || \
    echo "Warning: Authenticode self-sign failed; shipping unsigned DLL" >&2
else
  echo "PowerShell not found — leaving dither_shell.dll unsigned"
fi

ls -la "$DEST"

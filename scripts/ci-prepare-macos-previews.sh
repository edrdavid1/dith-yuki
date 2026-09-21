#!/usr/bin/env bash
# Build Quick Look .appex into platform/macos/DitherQuickLook/dist/ for
# tauri.macos.conf.json → bundle.macOS.files (PlugIns/).
#
# Invoked by CI and by tauri beforeBundleCommand on macOS.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/platform/macos/DitherQuickLook/dist"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "ci-prepare-macos-previews: skip (not macOS)"
  exit 0
fi

# Prefer full Xcode.app on GitHub runners / developer Macs.
if [[ -d /Applications/Xcode.app/Contents/Developer ]]; then
  sudo xcode-select -s /Applications/Xcode.app/Contents/Developer 2>/dev/null || true
fi

if ! xcodebuild -version >/dev/null 2>&1; then
  echo "ci-prepare-macos-previews: full Xcode.app required to build Quick Look extensions." >&2
  echo "Install Xcode, then: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer" >&2
  exit 1
fi

if ! command -v xcodegen >/dev/null 2>&1; then
  if command -v brew >/dev/null 2>&1; then
    echo "Installing xcodegen via Homebrew…"
    brew install xcodegen
  else
    echo "xcodegen not found — brew install xcodegen" >&2
    exit 1
  fi
fi

TIER="${DITHER_PREVIEW_SIGN_TIER:-alpha}"
if [[ -n "${APPLE_SIGNING_IDENTITY:-}" && "${APPLE_SIGNING_IDENTITY}" != "-" ]]; then
  # Identity already provided (CI keychain / Developer ID).
  TIER="${DITHER_PREVIEW_SIGN_TIER:-alpha}"
fi

echo "Preparing macOS Quick Look plugins (tier=$TIER)…"
bash "$ROOT/scripts/build-quicklook.sh" --tier "$TIER"

test -d "$OUT/DitherQuickLookPreview.appex"
# Thumbnail appex is intentionally not shipped: Finder keeps type icons;
# Space uses Preview only.
echo "Quick Look Preview ready:"
ls -la "$OUT"

#!/usr/bin/env bash
# Wrap the universal Mac App Store .app into a pkg for Transporter.
# Does not upload. Requires APPLE_INSTALLER_IDENTITY (Mac Installer Distribution name).
set -euo pipefail

if [[ -z "${APPLE_INSTALLER_IDENTITY:-}" ]]; then
  echo "Set APPLE_INSTALLER_IDENTITY to the Mac Installer Distribution certificate name."
  echo "See docs/legal/STORE_ACCOUNT.md."
  exit 1
fi

root="$(cd "$(dirname "$0")/.." && pwd)"
app_dir="$root/src-tauri/target/universal-apple-darwin/release/bundle/macos"
app="$(find "$app_dir" -maxdepth 1 -name '*.app' -print -quit 2>/dev/null || true)"
if [[ -z "$app" ]]; then
  echo "No .app under $app_dir. Run npm run build:mas first."
  exit 1
fi

out="${1:-$root/dist/DitherYuki.pkg}"
mkdir -p "$(dirname "$out")"
xcrun productbuild \
  --component "$app" /Applications \
  --sign "$APPLE_INSTALLER_IDENTITY" \
  "$out"
echo "Wrote $out"

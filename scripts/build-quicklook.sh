#!/usr/bin/env bash
# Build dither-thumb staticlib + Quick Look Preview appex, optionally embed into an .app.
#
# Product choice (macOS): ship Preview only (Space / gallery). Finder icons stay
# the document-type .icns — do not embed Thumbnail unless --with-thumbnail.
#
# Usage:
#   scripts/build-quicklook.sh [--tier dev|alpha|public] [--app /path/to/Dither.app] [--spike] [--with-thumbnail]
#
# --spike keeps the fixed-image S0 path (no Rust link). Default links dt_extract.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
QL_DIR="$ROOT/platform/macos/DitherQuickLook"
TIER="dev"
APP_PATH=""
SPIKE=0
WITH_THUMBNAIL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --tier) TIER="$2"; shift 2 ;;
    --app) APP_PATH="$2"; shift 2 ;;
    --spike) SPIKE=1; shift ;;
    --with-thumbnail) WITH_THUMBNAIL=1; shift ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

IDENTITY="${APPLE_SIGNING_IDENTITY:--}"
case "$TIER" in
  dev) IDENTITY="${APPLE_SIGNING_IDENTITY:--}" ;;
  alpha)
    if [[ -z "${APPLE_SIGNING_IDENTITY:-}" || "${APPLE_SIGNING_IDENTITY}" == "-" ]]; then
      echo "TIER=alpha: bootstrapping self-signed identity (scripts/macos-self-sign-cert.sh)…"
      bash "$ROOT/scripts/macos-self-sign-cert.sh"
      if [[ -f "${HOME}/.cache/dither-yuki/apple-signing-identity" ]]; then
        IDENTITY="$(cat "${HOME}/.cache/dither-yuki/apple-signing-identity")"
      else
        IDENTITY="L'eco non di Bergamo"
      fi
      export APPLE_SIGNING_IDENTITY="$IDENTITY"
    else
      IDENTITY="$APPLE_SIGNING_IDENTITY"
    fi
    ;;
  public)
    if [[ -z "${APPLE_SIGNING_IDENTITY:-}" || "${APPLE_SIGNING_IDENTITY}" == "-" ]]; then
      echo "TIER=public requires APPLE_SIGNING_IDENTITY (Developer ID)" >&2
      exit 1
    fi
    IDENTITY="$APPLE_SIGNING_IDENTITY"
    ;;
  *) echo "unknown tier: $TIER" >&2; exit 2 ;;
esac

echo "Signing with identity: $IDENTITY (tier=$TIER)"

if ! command -v xcodegen >/dev/null 2>&1; then
  echo "xcodegen not found — brew install xcodegen" >&2
  exit 1
fi

QL_LIB="$ROOT/target/quicklook"
mkdir -p "$QL_LIB"

if [[ "$SPIKE" -eq 0 ]]; then
  echo "Building dither-thumb-ffi staticlibs (arm64 + x86_64)…"
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null
  cargo build -p dither-thumb-ffi --release --target aarch64-apple-darwin --locked 2>/dev/null \
    || cargo build -p dither-thumb-ffi --release --target aarch64-apple-darwin
  cargo build -p dither-thumb-ffi --release --target x86_64-apple-darwin --locked 2>/dev/null \
    || cargo build -p dither-thumb-ffi --release --target x86_64-apple-darwin
  lipo -create \
    "$ROOT/target/aarch64-apple-darwin/release/libdither_thumb_ffi.a" \
    "$ROOT/target/x86_64-apple-darwin/release/libdither_thumb_ffi.a" \
    -output "$QL_LIB/libdither_thumb_ffi.a"
  # Convenience alias matching older docs.
  cp "$QL_LIB/libdither_thumb_ffi.a" "$QL_LIB/libdither_thumb.a"
  ls -la "$QL_LIB"
fi

cd "$QL_DIR"
xcodegen generate --spec project.yml

if ! command -v xcodebuild >/dev/null 2>&1; then
  echo "xcodebuild missing" >&2
  exit 1
fi
if ! xcodebuild -version >/dev/null 2>&1; then
  echo "Full Xcode.app required (xcode-select currently points at CLT only)." >&2
  echo "Open Xcode once, then: sudo xcode-select -s /Applications/Xcode.app/Contents/Developer" >&2
  exit 1
fi

DERIVED="$QL_DIR/build"
rm -rf "$DERIVED"
SCHEMES=(DitherQuickLookPreview)
if [[ "$WITH_THUMBNAIL" -eq 1 ]]; then
  SCHEMES+=(DitherQuickLookThumbnail)
fi
for SCHEME in "${SCHEMES[@]}"; do
  xcodebuild \
    -project DitherQuickLook.xcodeproj \
    -scheme "$SCHEME" \
    -configuration Release \
    -derivedDataPath "$DERIVED" \
    CODE_SIGNING_ALLOWED=NO \
    build
done

PRODUCTS="$DERIVED/Build/Products/Release"
OUT="$QL_DIR/dist"
mkdir -p "$OUT"
rm -rf "$OUT/DitherQuickLookPreview.appex" "$OUT/DitherQuickLookThumbnail.appex"
cp -R "$PRODUCTS/DitherQuickLookPreview.appex" "$OUT/"
if [[ "$WITH_THUMBNAIL" -eq 1 ]]; then
  cp -R "$PRODUCTS/DitherQuickLookThumbnail.appex" "$OUT/"
fi

sign_one() {
  local path="$1"
  if [[ "$TIER" == "public" ]]; then
    codesign --force --sign "$IDENTITY" \
      --entitlements "$QL_DIR/Shared/DitherQuickLook.entitlements" \
      --options runtime \
      "$path"
  else
    codesign --force --sign "$IDENTITY" \
      --entitlements "$QL_DIR/Shared/DitherQuickLook.entitlements" \
      "$path"
  fi
}

sign_one "$OUT/DitherQuickLookPreview.appex"
if [[ "$WITH_THUMBNAIL" -eq 1 ]]; then
  sign_one "$OUT/DitherQuickLookThumbnail.appex"
fi

echo "Built:"
ls -la "$OUT"

if [[ -n "$APP_PATH" ]]; then
  PLUGINS="$APP_PATH/Contents/PlugIns"
  mkdir -p "$PLUGINS"
  rm -rf "$PLUGINS/DitherQuickLookPreview.appex" "$PLUGINS/DitherQuickLookThumbnail.appex"
  cp -R "$OUT/DitherQuickLookPreview.appex" "$PLUGINS/"
  sign_one "$PLUGINS/DitherQuickLookPreview.appex"
  if [[ "$WITH_THUMBNAIL" -eq 1 ]]; then
    cp -R "$OUT/DitherQuickLookThumbnail.appex" "$PLUGINS/"
    sign_one "$PLUGINS/DitherQuickLookThumbnail.appex"
  fi
  codesign --force --sign "$IDENTITY" "$APP_PATH"
  codesign --verify --deep --strict --verbose=2 "$APP_PATH" || true
  echo "Embedded into $APP_PATH"
  echo "Reset QL cache: qlmanage -r && qlmanage -r cache && killall Finder"
fi

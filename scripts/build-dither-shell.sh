#!/usr/bin/env bash
# Build dither_shell.dll for Windows (x64 and/or aarch64).
# Run on a Windows host or with an appropriate cross toolchain.
#
# Usage:
#   scripts/build-dither-shell.sh [--target x86_64-pc-windows-msvc|aarch64-pc-windows-msvc]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="x86_64-pc-windows-msvc"
if [[ "${1:-}" == "--target" ]]; then
  TARGET="${2:?}"
elif [[ -n "${1:-}" ]]; then
  TARGET="$1"
fi

cd "$ROOT"
rustup target add "$TARGET" >/dev/null
# Static CRT so the DLL does not need VC++ redistributable in dllhost.exe.
export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=+crt-static"
cargo build -p dither-shell --release --target "$TARGET"

OUT="$ROOT/target/$TARGET/release"
echo "Built: $OUT/dither_shell.dll"
ls -la "$OUT"/dither_shell.dll* 2>/dev/null || ls -la "$OUT"/dither_shell*

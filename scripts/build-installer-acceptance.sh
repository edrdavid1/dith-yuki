#!/usr/bin/env bash
# Build the NSIS license-page text: EULA + software license.
# Sources of truth:
#   docs/legal/USER_AGREEMENT.txt  — end-user agreement (edit this)
#   LICENSE                       — L'eco non di Bergamo Software License (edit that)
# Output (do not hand-edit):
#   docs/legal/INSTALLER_ACCEPTANCE.txt
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
UA="$ROOT/docs/legal/USER_AGREEMENT.txt"
LIC="$ROOT/LICENSE"
OUT="$ROOT/docs/legal/INSTALLER_ACCEPTANCE.txt"

[[ -f "$UA" ]] || { echo "missing $UA" >&2; exit 1; }
[[ -f "$LIC" ]] || { echo "missing $LIC" >&2; exit 1; }

{
  cat "$UA"
  printf '\n========================================================================\n\n'
  cat "$LIC"
} > "$OUT"

echo "build-installer-acceptance: wrote $OUT"

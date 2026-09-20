#!/usr/bin/env bash
# Phase 0 smoke: verify updater pubkey, signing env, and published latest.json.
# Does not print secret material. Exit non-zero on hard failures.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

REPO="${RELEASE_REPO:-edrdavid1/dith-yuki}"
CONF="src-tauri/tauri.conf.json"
PASS=0
FAIL=0
WARN=0

ok() { echo "  OK  $*"; PASS=$((PASS + 1)); }
warn() { echo "  WARN $*"; WARN=$((WARN + 1)); }
fail() { echo "  FAIL $*"; FAIL=$((FAIL + 1)); }

echo "== Dither Yuki release-chain check =="
echo

echo "1) Updater config in ${CONF}"
if [[ ! -f "$CONF" ]]; then
  fail "missing ${CONF}"
else
  if python3 - "$CONF" <<'PY'
import json, sys
conf = json.load(open(sys.argv[1]))
upd = conf.get("plugins", {}).get("updater", {})
pubkey = upd.get("pubkey") or ""
endpoints = upd.get("endpoints") or []
ok = True
if not pubkey.startswith("dW50cnVzdGVk"):
    print("pubkey missing or not minisign base64", file=sys.stderr)
    ok = False
if not any("latest.json" in e for e in endpoints):
    print("no latest.json endpoint", file=sys.stderr)
    ok = False
if conf.get("bundle", {}).get("createUpdaterArtifacts") is not True:
    print("createUpdaterArtifacts is not true", file=sys.stderr)
    ok = False
sys.exit(0 if ok else 1)
PY
  then
    ok "pubkey + latest.json endpoint + createUpdaterArtifacts"
  else
    fail "updater block incomplete in tauri.conf.json"
  fi
fi

echo
echo "2) Local updater signing env (optional for this script; required in CI)"
if [[ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
  ok "TAURI_SIGNING_PRIVATE_KEY is set (${#TAURI_SIGNING_PRIVATE_KEY} chars)"
  if [[ -n "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
    ok "TAURI_SIGNING_PRIVATE_KEY_PASSWORD is set"
  else
    warn "TAURI_SIGNING_PRIVATE_KEY_PASSWORD empty (ok if key has no password)"
  fi
else
  warn "TAURI_SIGNING_PRIVATE_KEY not in this shell — CI must have repo secret"
fi

echo
echo "3) Apple notarization env (optional until public alpha)"
APPLE_VARS=(
  APPLE_CERTIFICATE
  APPLE_CERTIFICATE_PASSWORD
  APPLE_SIGNING_IDENTITY
  APPLE_ID
  APPLE_PASSWORD
  APPLE_TEAM_ID
)
APPLE_SET=0
for v in "${APPLE_VARS[@]}"; do
  if [[ -n "${!v:-}" ]]; then
    APPLE_SET=$((APPLE_SET + 1))
  fi
done
if [[ "$APPLE_SET" -eq 0 ]]; then
  warn "no Apple signing env in this shell — Gatekeeper will warn until notarized"
elif [[ "$APPLE_SET" -eq ${#APPLE_VARS[@]} ]]; then
  ok "all Apple signing/notarization env vars present"
else
  warn "partial Apple env (${APPLE_SET}/${#APPLE_VARS[@]}) — complete before public alpha"
fi

echo
echo "4) Published latest.json from GitHub Releases"
URL="https://github.com/${REPO}/releases/latest/download/latest.json"
TMP="$(mktemp)"
HTTP_CODE="$(curl -sS -L -o "$TMP" -w '%{http_code}' "$URL" || true)"
if [[ "$HTTP_CODE" == "200" ]]; then
  EVAL="$(python3 - "$TMP" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
ver = data.get("version")
platforms = data.get("platforms") or {}
if not ver or not platforms:
    sys.exit(1)
print(ver)
print(",".join(sorted(platforms.keys())))
PY
)" && {
    VER="$(echo "$EVAL" | sed -n '1p')"
    PLATFORMS="$(echo "$EVAL" | sed -n '2p')"
    ok "latest.json reachable — version ${VER} platforms ${PLATFORMS}"
  } || fail "latest.json returned 200 but is not a valid updater manifest"
elif [[ "$HTTP_CODE" == "404" ]]; then
  warn "latest.json not published yet (HTTP 404) — push a signed v* tag to create it"
else
  warn "could not fetch latest.json (HTTP ${HTTP_CODE:-curl-failed})"
fi
rm -f "$TMP"

echo
echo "5) GitHub Actions secrets checklist (manual — gh auth required)"
echo "     Required:  TAURI_SIGNING_PRIVATE_KEY"
echo "     Optional:  TAURI_SIGNING_PRIVATE_KEY_PASSWORD"
echo "     Public α:  APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD,"
echo "                APPLE_SIGNING_IDENTITY, KEYCHAIN_PASSWORD,"
echo "                APPLE_ID, APPLE_PASSWORD, APPLE_TEAM_ID"
if command -v gh >/dev/null 2>&1; then
  if gh auth status >/dev/null 2>&1; then
    if gh secret list --repo "$REPO" 2>/dev/null | grep -q 'TAURI_SIGNING_PRIVATE_KEY'; then
      ok "repo secret TAURI_SIGNING_PRIVATE_KEY exists"
    else
      fail "repo secret TAURI_SIGNING_PRIVATE_KEY missing on ${REPO}"
    fi
    if gh secret list --repo "$REPO" 2>/dev/null | grep -q 'APPLE_CERTIFICATE'; then
      ok "repo secret APPLE_CERTIFICATE exists (notarization path)"
    else
      warn "APPLE_CERTIFICATE not set — closed alpha OK, public alpha needs notarization"
    fi
  else
    warn "gh not authenticated — run: gh auth login, then re-run this script"
  fi
else
  warn "gh CLI not installed — skip remote secret probe"
fi

echo
echo "Summary: ${PASS} ok, ${WARN} warn, ${FAIL} fail"
if [[ "$FAIL" -gt 0 ]]; then
  exit 1
fi
exit 0

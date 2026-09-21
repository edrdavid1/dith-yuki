#!/usr/bin/env bash
# Create a self-signed macOS code-signing identity (no Apple Developer Program).
# Publisher: L'eco non di Bergamo
#
# Env in:
#   RUNNER_TEMP / TMPDIR  — work directory
#   KEYCHAIN_PASSWORD     — optional (default: dither-yuki-ci-keychain)
# Env out (GITHUB_ENV when set):
#   APPLE_SIGNING_IDENTITY
#   SIGNING_MODE=self
set -euo pipefail

CERT_CN="L'eco non di Bergamo"
WORK="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/dither-selfsign-$$"
KEYCHAIN_PASSWORD="${KEYCHAIN_PASSWORD:-dither-yuki-ci-keychain}"
KEYCHAIN="${WORK}/build.keychain"
P12_PASS="dither-yuki-selfsign"

mkdir -p "$WORK"

# Double-quote CN so the apostrophe in L'eco is literal (openssl config).
cat > "$WORK/codesign.conf" <<'EOF'
[req]
distinguished_name = dn
x509_extensions = v3_req
prompt = no

[dn]
CN = "L'eco non di Bergamo"
O = "L'eco non di Bergamo"
OU = Software
C = IT

[v3_req]
basicConstraints = critical,CA:FALSE
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

openssl genrsa -out "$WORK/key.pem" 2048
openssl req -new -x509 -key "$WORK/key.pem" -out "$WORK/cert.pem" \
  -days 3650 -config "$WORK/codesign.conf" -extensions v3_req

# Show subject for debugging
openssl x509 -in "$WORK/cert.pem" -noout -subject

# macOS `security import` rejects OpenSSL 3 default PBES2 PKCS#12 — use legacy bag.
if openssl pkcs12 -help 2>&1 | grep -q -- '-legacy'; then
  OPENSSL_P12_EXTRA=(-legacy)
else
  OPENSSL_P12_EXTRA=(-certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg SHA1)
fi
openssl pkcs12 -export \
  -out "$WORK/cert.p12" \
  -inkey "$WORK/key.pem" \
  -in "$WORK/cert.pem" \
  -passout "pass:${P12_PASS}" \
  -name "$CERT_CN" \
  "${OPENSSL_P12_EXTRA[@]}"

if [[ "${CI:-}" == "true" || -n "${GITHUB_ACTIONS:-}" ]]; then
  security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
  security list-keychains -s "$KEYCHAIN"
  security default-keychain -s "$KEYCHAIN"
  security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
  security set-keychain-settings -t 3600 -u "$KEYCHAIN"
  security import "$WORK/cert.p12" -k "$KEYCHAIN" -P "$P12_PASS" \
    -T /usr/bin/codesign -T /usr/bin/security
  # Mark self-signed cert trusted for code signing on this runner.
  sudo security add-trusted-cert -d -r trustRoot -k "$KEYCHAIN" "$WORK/cert.pem" \
    || security add-trusted-cert -d -r trustRoot -k "$KEYCHAIN" "$WORK/cert.pem" || true
  security set-key-partition-list -S apple-tool:,apple:,codesign: \
    -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
  echo "All identities:"
  security find-identity -p codesigning "$KEYCHAIN" || true
  echo "Valid identities:"
  security find-identity -v -p codesigning "$KEYCHAIN" || true
else
  security import "$WORK/cert.p12" -k ~/Library/Keychains/login.keychain-db \
    -P "$P12_PASS" -T /usr/bin/codesign 2>/dev/null \
    || security import "$WORK/cert.p12" -P "$P12_PASS" -T /usr/bin/codesign
  sudo security add-trusted-cert -d -r trustRoot -k ~/Library/Keychains/login.keychain-db \
    "$WORK/cert.pem" 2>/dev/null || true
fi

# Resolve the identity string codesign actually sees (may differ slightly from CERT_CN).
IDENTITY="$(
  security find-identity -p codesigning "${KEYCHAIN:-}" 2>/dev/null \
    | sed -n 's/.*"\(.*\)".*/\1/p' \
    | head -1
)"
if [[ -z "$IDENTITY" ]]; then
  IDENTITY="$CERT_CN"
fi

# Smoke: codesign must accept the identity (empty file).
SMOKE="$WORK/smoke.bin"
echo smoke > "$SMOKE"
if ! codesign -s "$IDENTITY" -f "$SMOKE" 2>"$WORK/codesign.err"; then
  echo "codesign smoke failed for identity '$IDENTITY':" >&2
  cat "$WORK/codesign.err" >&2
  # Fallback: sign by SHA-1 hash from find-identity
  HASH="$(
    security find-identity -p codesigning "${KEYCHAIN:-}" 2>/dev/null \
      | sed -n 's/^[[:space:]]*[0-9]*)[[:space:]]*\([A-F0-9]*\) .*/\1/p' \
      | head -1
  )"
  if [[ -n "$HASH" ]]; then
    echo "Retrying codesign with hash $HASH"
    codesign -s "$HASH" -f "$SMOKE"
    IDENTITY="$HASH"
  else
    exit 1
  fi
fi
echo "codesign smoke OK with: $IDENTITY"

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    echo "APPLE_SIGNING_IDENTITY=${IDENTITY}"
    echo "SIGNING_MODE=self"
  } >> "$GITHUB_ENV"
fi

echo "Created self-signed identity: ${IDENTITY}"
# Machine-local hint for sibling scripts (not a secret).
mkdir -p "${HOME}/.cache/dither-yuki"
printf '%s\n' "$IDENTITY" > "${HOME}/.cache/dither-yuki/apple-signing-identity"
rm -f "$WORK/key.pem" "$WORK/cert.pem" "$WORK/cert.p12" "$SMOKE"

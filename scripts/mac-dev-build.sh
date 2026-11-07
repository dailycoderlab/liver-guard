#!/usr/bin/env bash
# Local macOS build signed with a stable self-signed identity.
#
# macOS keys Screen Recording / Accessibility grants to the app's code identity.
# An ad-hoc build is identified by its binary hash, so every rebuild looks like a
# new app and the grants are lost. Signing every build with the same certificate
# makes the identity "com.sky.liverguard + this certificate", which survives rebuilds.
#
# Local development only: the certificate is self-signed, so the result is not
# notarized and Gatekeeper still blocks it on other machines (see README "打包").
#
#   scripts/mac-dev-build.sh          create the identity if missing, then build
#   scripts/mac-dev-build.sh setup    only create the identity
#
# Load the OpenCV environment (AGENTS.md) before building.

set -euo pipefail

IDENTITY="${LIVERGUARD_SIGN_IDENTITY:-LiverGuard Dev}"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OPENSSL=/usr/bin/openssl # LibreSSL: its PKCS#12 output is what `security import` accepts

has_identity() {
  security find-identity -v -p codesigning "$KEYCHAIN" | grep -q "\"$IDENTITY\""
}

create_identity() {
  local tmp pass
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN

  cat >"$tmp/openssl.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $IDENTITY
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
EOF

  "$OPENSSL" req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$tmp/openssl.cnf" \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
  pass="$(uuidgen)"
  "$OPENSSL" pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -out "$tmp/id.p12" -passout "pass:$pass"
  security import "$tmp/id.p12" -k "$KEYCHAIN" -P "$pass" -T /usr/bin/codesign >/dev/null

  # A self-signed certificate is only usable for code signing once it is trusted for it;
  # macOS asks for your password / Touch ID here.
  if ! has_identity; then
    security add-trusted-cert -p codeSign -k "$KEYCHAIN" "$tmp/cert.pem"
  fi
}

if ! has_identity; then
  echo "Creating the local signing identity \"$IDENTITY\" in the login keychain..."
  create_identity
  has_identity || { echo "The identity \"$IDENTITY\" is still not valid for code signing." >&2; exit 1; }
fi
echo "Signing identity: $IDENTITY"

[ "${1:-}" = "setup" ] && exit 0

cd "$ROOT/apps/desktop"
APPLE_SIGNING_IDENTITY="$IDENTITY" ./web/node_modules/.bin/tauri build --bundles app

APP="$ROOT/target/release/bundle/macos/LiverGuard.app"
echo
echo "Designated requirement (must not contain a cdhash):"
codesign -d -r- "$APP" 2>&1 | grep designated

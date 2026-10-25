#!/bin/sh
# Make a local code-signing certificate for debug builds on a Mac, once per machine.
#
# macOS ties Accessibility, Camera and Microphone grants to an app's code signature. Tauri's
# debug bundle is ad-hoc signed with a per-build hash, so every rebuild is a new app to macOS
# and the grants silently stop working. Signed with this certificate, the grant follows the
# certificate and survives rebuilds. Dev only: Gatekeeper still blocks it on other Macs, and
# a release needs a Developer ID certificate from the Apple Developer Program.
#
# The certificate lives in its own keychain (supervisor-dev) with a known password, so
# codesign never asks for the login password: a frozen worker has hundreds of files to sign.
#
#   sh scripts/make_mac_dev_cert.sh          # creates "Supervisor Dev"
#   npm run bundle:mac-dev                   # debug Supervisor.app signed with it
set -eu

NAME="${1:-Supervisor Dev}"
KEYCHAIN="$HOME/Library/Keychains/supervisor-dev.keychain-db"
PASSWORD="supervisor-dev"

if [ ! -f "$KEYCHAIN" ]; then
  security create-keychain -p "$PASSWORD" "$KEYCHAIN"
  # Never lock on a timer; codesign runs unattended.
  security set-keychain-settings "$KEYCHAIN"
fi
security unlock-keychain -p "$PASSWORD" "$KEYCHAIN"
# Put it on the search list once, ahead of login, so codesign finds the identity by name.
if ! security list-keychains -d user | grep -q "supervisor-dev.keychain-db"; then
  # shellcheck disable=SC2046
  security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '" ')
fi

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "'$NAME' is already in $KEYCHAIN"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

openssl req -x509 -newkey rsa:2048 -nodes -days 3650 \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem" -subj "/CN=$NAME" \
  -addext "keyUsage=critical,digitalSignature" \
  -addext "extendedKeyUsage=critical,codeSigning" \
  -addext "basicConstraints=critical,CA:false" 2>/dev/null
# The keychain reads only the older PKCS#12 ciphers; OpenSSL 3 defaults to ones it rejects.
openssl pkcs12 -export -out "$tmp/cert.p12" -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -passout pass:dev \
  -macalg sha1 -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES

security import "$tmp/cert.p12" -k "$KEYCHAIN" -P dev -T /usr/bin/codesign -T /usr/bin/security
# Lets Apple's tools use the key without a dialog per file.
security set-key-partition-list -S apple-tool:,apple: -s -k "$PASSWORD" "$KEYCHAIN" >/dev/null
# Trust it for code signing in this user's trust domain (no admin needed).
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$tmp/cert.pem"

echo "created '$NAME' in $KEYCHAIN; codesign identities now:"
security find-identity -v -p codesigning

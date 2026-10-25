#!/bin/sh
# Make a local code-signing certificate for debug builds on a Mac, once per machine.
#
# macOS ties Accessibility, Camera and Microphone grants to an app's code signature. Tauri's
# debug bundle is ad-hoc signed with a per-build hash, so every rebuild is a new app to macOS
# and the grants silently stop working. Signed with this certificate, the grant follows the
# certificate and survives rebuilds. Dev only: Gatekeeper still blocks it on other Macs, and
# a release needs a Developer ID certificate from the Apple Developer Program.
#
#   sh scripts/make_mac_dev_cert.sh          # creates "Supervisor Dev" in the login keychain
#   npm run bundle:mac-dev                   # debug Supervisor.app signed with it
#
# macOS may show one keychain dialog the first time codesign uses the key: choose Always Allow.
set -eu

NAME="${1:-Supervisor Dev}"
KEYCHAIN="$HOME/Library/Keychains/login.keychain-db"

if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
  echo "'$NAME' is already in the login keychain"
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

# -T lets codesign use the key without a prompt per build.
security import "$tmp/cert.p12" -k "$KEYCHAIN" -P dev -T /usr/bin/codesign -T /usr/bin/security
# Trust it for code signing in this user's trust domain (no admin needed).
security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$tmp/cert.pem"

echo "created '$NAME'; codesign identities now:"
security find-identity -v -p codesigning

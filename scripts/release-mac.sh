#!/bin/sh
# Build a Mac release: freeze and sign the webcam worker, build and sign Supervisor.app, then
# zip it for scripts/install-mac.sh. Run on a Mac (or the macos CI runner) from the repo root:
#   sh scripts/release-mac.sh ["Signing identity"]
# Output: build/release/Supervisor-macos-<arch>.zip and .sha256. Upload both to a GitHub
# Release; the installer always fetches the latest one.
#
# The identity defaults to "Supervisor Dev" (scripts/make_mac_dev_cert.sh). Every release must
# use the SAME certificate, or macOS treats the update as a different app and silently drops
# the user's Accessibility, Camera and Microphone grants.
set -eu

IDENTITY="${1:-Supervisor Dev}"
REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"

case "$(uname -m)" in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "unsupported processor $(uname -m)" >&2; exit 1 ;;
esac
# The certificate every release must carry; anything else silently drops users' permissions.
REQUIREMENT='identifier "app.supervisor.desktop" and certificate leaf = H"f6705448be2a29c5de7579461d408f1e05ef3df1"'
out="build/release"
zip="$out/Supervisor-macos-$arch.zip"

security unlock-keychain -p supervisor-dev "$HOME/Library/Keychains/supervisor-dev.keychain-db" 2>/dev/null || true

.venv-webcam/bin/python scripts/build_webcam_runtime.py
sh scripts/sign_webcam_runtime.sh "$IDENTITY"
APPLE_SIGNING_IDENTITY="$IDENTITY" npx tauri build --bundles app --config src-tauri/tauri.bundle.conf.json

app="src-tauri/target/release/bundle/macos/Supervisor.app"
codesign --verify --deep --strict -R="$REQUIREMENT" "$app" \
  || { echo "the app is not signed with Supervisor's certificate (plans/distribution.md)" >&2; exit 1; }

mkdir -p "$out"
rm -f "$zip" "$zip.sha256"
ditto -c -k --keepParent "$app" "$zip"
(cd "$out" && shasum -a 256 "$(basename "$zip")" > "$(basename "$zip").sha256")
echo "release ready: $zip ($(du -h "$zip" | cut -f1))"
cat "$zip.sha256"

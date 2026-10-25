#!/bin/sh
# Install or update Supervisor on a Mac:
#   curl -fsSL https://raw.githubusercontent.com/pr4nav3/stealth-startup/main/scripts/install-mac.sh | sh
#
# Files fetched with curl carry no quarantine flag, so Gatekeeper never stops the app and the
# user sees no "Open Anyway" step. Integrity comes from the published SHA-256 instead.
# Re-running the same line updates the app. Nothing here depends on what the app does, so it
# never needs to change when the app does.
set -eu

REPO="pr4nav3/stealth-startup"
APP="Supervisor"
# SUPERVISOR_INSTALL_BASE overrides where the zip and its .sha256 come from (local testing).
BASE="${SUPERVISOR_INSTALL_BASE:-https://github.com/$REPO/releases/latest/download}"
DEST="${SUPERVISOR_INSTALL_DIR:-/Applications}"

fail() { echo "Supervisor install failed: $1" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "this installer is for macOS."
major=$(sw_vers -productVersion | cut -d. -f1)
[ "$major" -ge 14 ] || fail "Supervisor needs macOS 14 or newer (this Mac has $(sw_vers -productVersion))."

case "$(uname -m)" in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) fail "unsupported processor $(uname -m)." ;;
esac
zip="$APP-macos-$arch.zip"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

echo "Downloading $APP..."
curl -fL --progress-bar -o "$tmp/$zip" "$BASE/$zip" || fail "could not download $BASE/$zip"
curl -fsSL -o "$tmp/$zip.sha256" "$BASE/$zip.sha256" || fail "could not download the checksum"

expected=$(awk '{print $1}' "$tmp/$zip.sha256")
actual=$(shasum -a 256 "$tmp/$zip" | awk '{print $1}')
[ -n "$expected" ] && [ "$expected" = "$actual" ] || fail "checksum mismatch; the download is incomplete or altered."

# ditto keeps the code signature and the bundle's symlinks intact; unzip can break both.
ditto -x -k "$tmp/$zip" "$tmp/out" || fail "could not unpack the download."
[ -d "$tmp/out/$APP.app" ] || fail "the download does not contain $APP.app."
codesign --verify --deep --strict "$tmp/out/$APP.app" 2>/dev/null || fail "the app's signature does not verify."

# Replace a running copy cleanly: quit it, then swap the bundle.
if pgrep -f "$DEST/$APP.app/Contents/MacOS/" >/dev/null 2>&1; then
  echo "Closing the running $APP..."
  osascript -e "quit app \"$APP\"" >/dev/null 2>&1 || true
  sleep 2
  pkill -f "$DEST/$APP.app/Contents/MacOS/" 2>/dev/null || true
fi

if [ -w "$DEST" ]; then
  rm -rf "$DEST/$APP.app" && ditto "$tmp/out/$APP.app" "$DEST/$APP.app"
else
  echo "Installing to $DEST needs your password."
  sudo rm -rf "$DEST/$APP.app" && sudo ditto "$tmp/out/$APP.app" "$DEST/$APP.app"
fi

echo "Installed $APP to $DEST. Opening it..."
open "$DEST/$APP.app"

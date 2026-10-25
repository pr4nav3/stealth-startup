#!/bin/sh
# Install or update Supervisor on a Mac:
#   curl -fsSL https://raw.githubusercontent.com/pr4nav3/stealth-startup/main/scripts/install-mac.sh | sh
#
# Files fetched with curl carry no quarantine flag, so Gatekeeper never stops the app and the
# user sees no "Open Anyway" step. Integrity comes from the published SHA-256, authenticity from
# the code signature, which must be Supervisor's own certificate (a different one would also
# make macOS forget the user's Accessibility, Camera and Microphone grants).
# Re-running the same line updates the app.
set -eu

REPO="pr4nav3/stealth-startup"
APP="Supervisor"
# SUPERVISOR_INSTALL_BASE overrides where the zip and its .sha256 come from (local testing).
BASE="${SUPERVISOR_INSTALL_BASE:-https://github.com/$REPO/releases/latest/download}"
DEST="${SUPERVISOR_INSTALL_DIR:-/Applications}"
# The one certificate every release is signed with (plans/distribution.md).
REQUIREMENT='identifier "app.supervisor.desktop" and certificate leaf = H"f6705448be2a29c5de7579461d408f1e05ef3df1"'

fail() { echo "Supervisor install failed: $1" >&2; exit 1; }

# Everything runs from main(), called on the last line, so a download of this script cut off
# halfway runs nothing at all.
main() {
  [ "$(uname -s)" = "Darwin" ] || fail "this installer is for macOS."
  major=$(sw_vers -productVersion | cut -d. -f1)
  [ "$major" -ge 14 ] || fail "Supervisor needs macOS 14 or newer (this Mac has $(sw_vers -productVersion))."
  # hw.optional.arm64 is 1 on Apple Silicon even in a shell running under Rosetta.
  [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ] \
    || fail "Supervisor needs an Apple Silicon Mac (M1 or newer) for now; Intel Macs are not supported yet."
  zip="$APP-macos-aarch64.zip"

  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT

  # HTTPS only, except for a local test server given by SUPERVISOR_INSTALL_BASE.
  proto="=https"
  [ -n "${SUPERVISOR_INSTALL_BASE:-}" ] && proto="=http,https"

  echo "Downloading $APP..."
  curl -fL --proto "$proto" --retry 3 --progress-bar -o "$tmp/$zip" "$BASE/$zip" \
    || fail "could not download $BASE/$zip"
  curl -fsSL --proto "$proto" --retry 3 -o "$tmp/$zip.sha256" "$BASE/$zip.sha256" \
    || fail "could not download the checksum"

  expected=$(awk '{print $1}' "$tmp/$zip.sha256")
  actual=$(shasum -a 256 "$tmp/$zip" | awk '{print $1}')
  [ -n "$expected" ] && [ "$expected" = "$actual" ] || fail "checksum mismatch; the download is incomplete or altered."

  # ditto keeps the code signature and the bundle's symlinks intact; unzip can break both.
  ditto -x -k "$tmp/$zip" "$tmp/out" || fail "could not unpack the download."
  [ -d "$tmp/out/$APP.app" ] || fail "the download does not contain $APP.app."
  codesign --verify --deep --strict -R="$REQUIREMENT" "$tmp/out/$APP.app" 2>/dev/null \
    || fail "the app is not signed with Supervisor's certificate."

  # Writing to /Applications may need an admin password.
  run=""
  if [ ! -w "$DEST" ] || { [ -e "$DEST/$APP.app" ] && [ ! -w "$DEST/$APP.app" ]; }; then
    echo "Installing to $DEST needs your password."
    sudo -v || fail "an administrator password is needed to install into $DEST. To install just for you: curl ... | SUPERVISOR_INSTALL_DIR=~/Applications sh"
    run="sudo"
  fi

  # Copy next to the old app first, so a full disk or a closed terminal never leaves no app.
  staged="$DEST/.$APP.app.new"
  $run rm -rf "$staged" || fail "could not clear $staged."
  $run ditto "$tmp/out/$APP.app" "$staged" || { $run rm -rf "$staged"; fail "could not copy the app into $DEST."; }

  # Quit a running copy and wait for it to be gone; otherwise the old one keeps running and
  # the new one, finding it, hands over and exits.
  if pgrep -f "$DEST/$APP.app/Contents/MacOS/" >/dev/null 2>&1; then
    echo "Closing the running $APP..."
    pkill -TERM -f "$DEST/$APP.app/Contents/MacOS/" 2>/dev/null || true
    i=0
    while pgrep -f "$DEST/$APP.app/Contents/MacOS/" >/dev/null 2>&1 && [ $i -lt 20 ]; do
      sleep 0.5
      i=$((i + 1))
    done
    pkill -KILL -f "$DEST/$APP.app/Contents/MacOS/" 2>/dev/null || true
  fi

  old="$DEST/.$APP.app.old"
  $run rm -rf "$old" || fail "could not clear $old."
  if [ -e "$DEST/$APP.app" ]; then
    $run mv "$DEST/$APP.app" "$old" || fail "could not move the old app aside."
  fi
  if ! $run mv "$staged" "$DEST/$APP.app"; then
    [ -e "$old" ] && $run mv "$old" "$DEST/$APP.app"
    fail "could not put the new app in place; the old one is back."
  fi
  $run rm -rf "$old" || true

  echo "Installed $APP to $DEST. Opening it..."
  open "$DEST/$APP.app"
}

main "$@"

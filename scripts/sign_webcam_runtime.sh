#!/bin/sh
# Sign the staged webcam worker (build/webcam/worker) with one identity before Tauri bundles
# it. Tauri signs the app but copies resources as they are, and PyInstaller leaves every
# binary ad-hoc signed; notarization rejects that. Hardened runtime needs the entitlements
# below for a frozen Python (it loads its own libraries and JIT-free but unsigned-memory
# bytecode), plus the camera.
#
#   sh scripts/sign_webcam_runtime.sh "Supervisor Dev"          # dev
#   sh scripts/sign_webcam_runtime.sh "Developer ID Application: …"   # release, then notarize
#
# Run after scripts/build_webcam_runtime.py and before `tauri build`.
set -eu

IDENTITY="${1:?signing identity}"
REPO=$(cd "$(dirname "$0")/.." && pwd)
WORKER_DIR="$REPO/build/webcam/worker"
ENTITLEMENTS=$(mktemp -t webcam-entitlements).plist
trap 'rm -f "$ENTITLEMENTS"' EXIT

cat > "$ENTITLEMENTS" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
  <true/>
  <key>com.apple.security.cs.disable-library-validation</key>
  <true/>
  <key>com.apple.security.device.camera</key>
  <true/>
</dict>
</plist>
EOF

# Libraries first, the executable last, so every nested signature is final when the
# executable's is computed. A framework is signed as a bundle, not file by file.
find "$WORKER_DIR/_internal" -type f \( -name '*.so' -o -name '*.dylib' -o -perm -u+x \) \
  | grep -v '\.framework/' \
  | while read -r file; do
      if file "$file" | grep -q 'Mach-O'; then
        codesign --force --options runtime --timestamp --sign "$IDENTITY" "$file" 2>&1 | grep -v 'replacing existing signature' || true
      fi
    done
# PyInstaller copies Python.framework with Versions/Current, Python and Resources as real
# duplicates instead of the symlinks a framework has, which codesign calls "ambiguous".
# Restore the canonical layout, then sign the versioned bundle.
find "$WORKER_DIR/_internal" -type d -name '*.framework' | while read -r framework; do
  version=$(ls "$framework/Versions" | grep -v '^Current$' | head -1)
  if [ -d "$framework/Versions/Current" ] && [ ! -L "$framework/Versions/Current" ]; then
    rm -rf "$framework/Versions/Current"
    ln -s "$version" "$framework/Versions/Current"
  fi
  for item in Python Resources; do
    if [ -e "$framework/$item" ] && [ ! -L "$framework/$item" ]; then
      rm -rf "$framework/$item"
      ln -s "Versions/Current/$item" "$framework/$item"
    fi
  done
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$framework/Versions/$version" 2>&1 | grep -v 'replacing existing signature' || true
done
codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
  --sign "$IDENTITY" "$WORKER_DIR/supervisor-webcam-worker"

codesign --verify --deep --strict "$WORKER_DIR/supervisor-webcam-worker"
echo "signed $WORKER_DIR with '$IDENTITY'"

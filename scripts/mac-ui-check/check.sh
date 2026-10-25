#!/bin/sh
# Lays out every screen of the mock in WebKit (the macOS webview) and fails on anything a
# Windows-only pass cannot see: horizontal overflow, a modal taller than the window, or a
# status-window card taller than its window. Runs on a Mac; CI runs it on the macOS job.
#   sh scripts/mac-ui-check/check.sh          (starts its own Vite server)
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT=${TMPDIR:-/tmp}/mac-ui-check
mkdir -p "$OUT"
swiftc -O -target arm64-apple-macos14.0 -o "$OUT/webkit-eval" "$HERE/webkit-eval.swift" 2>/dev/null \
  || swiftc -O -o "$OUT/webkit-eval" "$HERE/webkit-eval.swift"
STARTED=0
if ! curl -sf -o /dev/null http://127.0.0.1:1420/mock.html; then
  (cd "$ROOT" && npx vite --host 127.0.0.1 --port 1420 --strictPort >"$OUT/vite.log" 2>&1 &)
  STARTED=1
  for i in $(seq 1 150); do curl -sf -o /dev/null http://127.0.0.1:1420/mock.html && break; sleep 0.2; done
fi
"$OUT/webkit-eval" "http://127.0.0.1:1420/mock.html" 920 760 "$HERE/states.js" > "$OUT/raw.txt" \
  || { cat "$OUT/raw.txt" >&2; exit 1; }
tail -1 "$OUT/raw.txt" > "$OUT/states.json"
[ "$STARTED" = 1 ] && pkill -f "vite --host 127.0.0.1 --port 1420" || true
python3 "$HERE/assert.py" "$OUT/states.json"

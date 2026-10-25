# Distribution (free, no paid Apple or Microsoft programs)

Decided 2026-10-25. Nothing here depends on what the app does, so app changes never require
redoing it; each release just reruns the same steps.

## macOS: one-line installer only

Users paste:

```
curl -fsSL https://raw.githubusercontent.com/pr4nav3/stealth-startup/main/scripts/install-mac.sh | sh
```

Why this works without an Apple Developer ID: Gatekeeper only checks files that carry the
quarantine flag, which browsers set and `curl` does not. The installer downloads
`Supervisor-macos-<arch>.zip` from the latest GitHub Release, checks it against the published
`.sha256`, verifies the code signature, quits a running copy, replaces `/Applications/Supervisor.app`
and opens it. Running the same line again is the update. There is no DMG and no "Open Anyway".

What the user still sees: the normal permission prompts (Accessibility, Camera, Microphone),
each once. They do not reappear after updates as long as every release is signed with the same
certificate (below).

Requirements the repo already meets: it is public, so release assets download without a token;
the installer refuses macOS < 14 (the bundle's minimum) and picks the zip by CPU.

### Tested on the Mac (2026-10-25)

Release built with `scripts/release-mac.sh` (160 MB zip), served locally, and installed with
`cat scripts/install-mac.sh | SUPERVISOR_INSTALL_BASE=http://127.0.0.1:8765 sh`, which is the
same code path as the real one-liner:

- Installed to `/Applications`, opened, and ran with **no Gatekeeper dialog**; zero quarantine
  flags on the installed bundle; signature verifies; the frozen webcam worker inside passed its
  CoreML self-test.
- The same app with a browser's quarantine flag added is **rejected** by Gatekeeper
  (`spctl`), which is what a DMG or zip download from a website would hit.
- Designated requirement: `identifier "app.supervisor.desktop" and certificate leaf =
  H"f6705448…"`, identical to the debug builds that already hold this Mac's Accessibility and
  Camera grants, so the permissions carried over without new prompts.
- Re-running the line while the app was open quit it, replaced it and reopened it (one
  process after). A wrong `.sha256` was refused before anything was installed.

Before the public line works: `scripts/install-mac.sh` must be on `main` (the raw URL reads
`main`, which is behind `app-v2-simplified-rules`), and at least one GitHub Release must exist
with the zip and `.sha256`.

## Voice runtime (downloaded by the app, separate from app releases)

Installed copies download the frozen voice worker on first use. The app reads a manifest at a
fixed address compiled in as the default (`voice_runtime.rs`, `DEFAULT_MANIFEST_URL`;
`SUPERVISOR_VOICE_PACK_MANIFEST_URL` still overrides it):

```
https://github.com/pr4nav3/stealth-startup/releases/download/voice-runtime/voice-runtime-manifest.json
```

That address is a GitHub Release with tag **`voice-runtime`**, marked **pre-release** so it never
becomes "latest" (the Mac installer reads "latest" for the app). It holds the manifest and one
zip per target. A new runtime means replacing those assets; app releases never change for it.

Build and publish, on a Mac (Windows: the same with the Windows venv, then rebuild the manifest
with both asset files and without `--partial`):

```
.venv-voice-runtime-macos/bin/python scripts/build_voice_runtime.py
# edit build/voice-runtime/*.asset.json: url = the release asset URL above, with the zip's name
python3 scripts/build_voice_runtime_manifest.py --version <name> --partial \
  --output build/voice-runtime/voice-runtime-manifest.json build/voice-runtime/*.asset.json
```

Then upload `voice-runtime-manifest.json` and the zip to the `voice-runtime` release.

Tested 2026-10-25: the macOS arm64 runtime (245 MB, sha256 `210878b6…`) installed through the
app's own install code (`cargo test manual_install_real_runtime_zip -- --ignored`) and the
installed worker made a 1.96 s clip from `derek.wav` over its `--serve` protocol. First use is
slow: 58 s of imports, 11 s model load, 16 s voice conditioning, then about 3 s a clip.

Bug fixed on the way: the build script's zipper followed symlinks, storing PyInstaller's 33
links as copies; the copied `_internal/Python` then looked for `libintl.8.dylib` outside the
runtime and the worker could not start at all. The zip now stores links, and the installer
recreates them, refusing any that point outside the runtime. Any runtime zip built before
2026-10-25 (including the old August one in `plans/STATUS.md`) has this bug on macOS.

Not yet: the release itself (needs uploading), a Windows runtime in the manifest (a Windows
copy gets "no voice runtime is published for windows-x64" until then), and the download step
over real HTTPS from GitHub, which can only be tried once the release exists.

### Making a release

On a Mac with the venvs from `plans/mac-handoff.md` and the signing certificate installed:

```
sh scripts/release-mac.sh
```

It freezes and signs the webcam worker, builds and signs `Supervisor.app` with
`tauri.bundle.conf.json`, and writes `build/release/Supervisor-macos-aarch64.zip` plus `.sha256`.
Upload both files to a new GitHub Release (mark it latest). The installer's URL never changes.

### The signing certificate (the one thing that must not be lost)

All releases are signed with the self-signed "Supervisor Dev" certificate
(SHA-1 `F6705448BE2A29C5DE7579461D408F1E05EF3DF1`). macOS keys a user's permission grants to
it. A release signed with any other certificate looks like a different app: every user silently
loses Accessibility (websites stop being recognized) and must re-grant.

Backup, made 2026-10-25, outside the repo: `~/supervisor-signing/` on the Mac it was made on:
`supervisor-signing.p12`, `p12-password.txt`, and `supervisor-signing.p12.base64` (for a
GitHub Actions secret). Copy these into a password manager. Never commit them.

To restore on another Mac, do **not** run `scripts/make_mac_dev_cert.sh` (it would mint a new,
different certificate). Import the backup instead:

```
KC=~/Library/Keychains/supervisor-dev.keychain-db
security create-keychain -p supervisor-dev $KC
security set-keychain-settings $KC
security unlock-keychain -p supervisor-dev $KC
security import supervisor-signing.p12 -k $KC -P "$(cat p12-password.txt)" -T /usr/bin/codesign
security set-key-partition-list -S apple-tool:,apple: -s -k supervisor-dev $KC
security find-certificate -c "Supervisor Dev" -p $KC > /tmp/supervisor-dev.pem
security add-trusted-cert -r trustRoot -p codeSign -k $KC /tmp/supervisor-dev.pem
security list-keychains -d user -s $KC ~/Library/Keychains/login.keychain-db
security find-identity -v -p codesigning   # must list "Supervisor Dev" with the SHA-1 above
```

### Without a Mac

Releases can be built on GitHub's free macOS runners (free for public repos). Not set up yet;
what it needs: the `.p12` and its password as secrets, imported as above; the two Python venvs
created from the pinned requirements; and `webcam_bakeoff/models/yolo26s.onnx`, which is not in
git, uploaded once to a separate GitHub Release so the job can download it (YuNet downloads by
itself). Intel Macs need the same job on an Intel runner (`macos-13`) producing
`Supervisor-macos-x86_64.zip`; until then the installer fails clearly on Intel.

## Windows

Not decided yet. Options and trade-offs are in the 2026-10-25 conversation: a direct download
from the website shows SmartScreen's "Windows protected your PC" (More info, Run anyway) on
every new version; the free Microsoft Store (MSIX, signed by Microsoft) shows no warning but
every update goes through Store certification.

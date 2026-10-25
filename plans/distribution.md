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

Known gap for real users: a release build finds the voice runtime only through
`SUPERVISOR_VOICE_PACK_MANIFEST_URL` at build time (`voice_runtime.rs`, `manifest_url`), which
no release sets yet (`plans/STATUS.md`, "Ship"). Until it does, voice cloning in an installed
copy cannot download its runtime. Webcam and website tracking are unaffected.

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

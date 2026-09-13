# Share an Apple-silicon client

On an Apple-silicon Mac with Python 3.11 or newer and the normal Rust toolchain:

```sh
python3 tools/package_macos_client.py
```

The packager builds the locked `release` client with thin LTO and stripped debug
symbols. It writes `dist/FistWorld-macOS-AppleSilicon-<revision>.zip`, an adjacent
SHA-256 file and an unpacked review directory. These are ignored build outputs.
Use `--output dist/<new-name>` to keep a second build of the same revision.

Send the ZIP. The recipient extracts it and opens `FistWorld.app`; Rust, Homebrew
and a source checkout are unnecessary. The included readme explains first launch
and joining. The package defaults to the existing Fly.io preset; use
`--server-preset Local` for a local-server package. This changes only the packaged
copy of `servers.ron`, preserving development preferences. The server must be
running a compatible protocol; this package neither starts nor deploys a server.

Only Git-tracked files under `client/assets` are included. The package excludes
developer accounts, local preferences, logs, generated review media and authoring
sources. `build-manifest.json` records the source revision, runtime file hashes,
ARM64 architecture, minimum OS encoded by the binary and dynamic dependencies.
The packager rejects non-system dynamic-library dependencies and verifies an
ad-hoc signature over the complete app. It does not perform Apple notarization.
First-launch approval follows [Apple's instructions](https://support.apple.com/guide/mac-help/mh40616/mac).

The app's small launcher resolves its bundled assets independently of the working
directory and stores preferences under
`~/Library/Application Support/FistWorld/client_data/`. The latest launch output
is `~/Library/Application Support/FistWorld/client.log`. For isolated launch
verification, set `FISTWORLD_USER_DATA_DIR` to a temporary absolute directory;
the player's normal files are then untouched.

Before sharing, unpack the actual ZIP somewhere outside the repository, verify
its manifest and signature, and launch that copy. Check the loaded launcher and
bundled server presets. The existing session capture harness can drive and capture
this real client with `FISTWORLD_SESSION_CAPTURE_DIR` and
`FISTFORCE_NO_SETTINGS_FILE=1`; see [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md).
The binary's minimum OS is not evidence of testing on that OS or an older Mac.

Verified 2026-09-13 for revision `aafae3ad`: the 55.6 MB release ZIP passed CRC,
all 225 manifest file hashes and strict app-signature verification. A fresh copy
extracted outside the repository launched through macOS `open`, loaded all startup
artwork and its bundled Fly.io preset, and connected to the hosted name-entry
screen. The app signature remained valid after exit, and its working directory
was the isolated user-data folder. The real renderer's launcher and connected
captures and their metadata were inspected under
`logs/client-build-2026-09-13/smoke-final/`. Workspace all-target checks also passed.

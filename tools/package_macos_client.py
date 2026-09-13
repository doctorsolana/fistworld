#!/usr/bin/env python3
"""Build an Apple-silicon playtest app and a ZIP containing only runtime files."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import shutil
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]


def output(*command):
    return subprocess.check_output(command, cwd=ROOT, text=True).strip()


def run(*command):
    subprocess.run(command, cwd=ROOT, check=True)


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


LAUNCHER = '''#!/bin/sh
set -eu
binary_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
data_dir="${FISTWORLD_USER_DATA_DIR:-$HOME/Library/Application Support/FistWorld}"
mkdir -p "$data_dir"
cd "$data_dir"
export BEVY_ASSET_ROOT="$binary_dir/assets"
export FISTFORCE_ASSET_PATH="$binary_dir/assets"
exec "$binary_dir/client" "$@" > "$data_dir/client.log" 2>&1
'''


def player_readme(server_preset):
    return f'''FistWorld — Mac (M1 or newer)

Unzip the download and open FistWorld.app.

If Apple says it cannot check the app for malicious software:
1. Dismiss the warning.
2. Open System Settings > Privacy & Security.
3. Scroll down to the FistWorld warning and click Open Anyway.
4. Click Open and enter your Mac password if asked.

You normally only need to approve this once.

To play:
Click Connect ({server_preset} is already selected), enter your player name,
then create your character.
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, help="New output directory (ZIP is written beside it)")
    parser.add_argument("--server-preset", default="Fly.io", help="Packaged default; source presets stay unchanged")
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("Run this packager on an Apple-silicon Mac.")

    revision = output("git", "rev-parse", "HEAD")
    destination = (args.output or ROOT / "dist" / f"FistWorld-macOS-AppleSilicon-{revision[:8]}").resolve()
    archive = destination.with_suffix(".zip")
    if destination.exists() or archive.exists():
        parser.error("Output already exists; choose a new --output directory.")
    destination.parent.mkdir(parents=True, exist_ok=True)
    run("cargo", "build", "--locked", "--release", "-p", "client", "--bin", "client")
    binary = ROOT / "target/release/client"
    if output("lipo", "-archs", str(binary)) != "arm64":
        raise RuntimeError("The built client must be a native arm64 executable.")
    dependencies = [line.strip().split(" (", 1)[0] for line in output("otool", "-L", str(binary)).splitlines()[1:]]
    external = [path for path in dependencies if not path.startswith(("/System/Library/", "/usr/lib/"))]
    if external:
        raise RuntimeError(f"Client depends on libraries absent from a normal Mac: {external}")
    minimum = re.search(r"\bminos\s+(\S+)", output("xcrun", "vtool", "-show-build", str(binary)))
    if minimum is None:
        raise RuntimeError("Cannot determine the binary's minimum macOS version.")
    package = tomllib.loads((ROOT / "client/Cargo.toml").read_text())["package"]
    identifier = package["metadata"]["bundle"]["identifier"]
    protocol = re.search(r"pub const PROTOCOL_ID: u64 = (0x[0-9A-Fa-f_]+);", (ROOT / "shared/src/protocol/config.rs").read_text())
    if protocol is None:
        raise RuntimeError("Cannot record the client's network protocol version.")
    asset_paths = output("git", "ls-files", "-z", "--", "client/assets").split("\0")
    asset_paths = [Path(path) for path in asset_paths if path]

    with tempfile.TemporaryDirectory(prefix=".fistworld-package-", dir=destination.parent) as temporary:
        staged = Path(temporary) / destination.name
        app = staged / "FistWorld.app"
        macos = app / "Contents/MacOS"
        macos.mkdir(parents=True)
        assets = app / "Contents/Resources/assets"
        assets.mkdir(parents=True)
        # macOS signs Resources as data; the existing client locates assets
        # beside its executable. Keep that portable layout through an internal link.
        (macos / "assets").symlink_to("../Resources/assets", target_is_directory=True)
        shutil.copy2(binary, macos / "client")
        (macos / "FistWorld").write_text(LAUNCHER)
        (macos / "FistWorld").chmod(0o755)
        run("sh", "-n", str(macos / "FistWorld"))
        for source in asset_paths:
            path = ROOT / source
            if not path.is_file() or path.is_symlink():
                raise RuntimeError(f"Runtime asset must be an ordinary file: {source}")
            target = assets / source.relative_to("client/assets")
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, target)

        # A remote tester should not accidentally connect to their own localhost.
        presets = assets / "servers.ron"
        config = presets.read_text()
        names = re.findall(r'\bname:\s*"([^"\\]+)"', config)
        if args.server_preset not in names:
            raise RuntimeError(f"Unknown server preset {args.server_preset!r}; available: {names}")
        config, replacements = re.subn(r"\bdefault_index:\s*\d+", f"default_index: {names.index(args.server_preset)}", config)
        if replacements != 1:
            raise RuntimeError("Expected exactly one default_index in servers.ron.")
        presets.write_text(config)

        info = {
            "CFBundleName": "FistWorld", "CFBundleDisplayName": "FistWorld",
            "CFBundleExecutable": "FistWorld", "CFBundleIdentifier": identifier,
            "CFBundlePackageType": "APPL", "CFBundleInfoDictionaryVersion": "6.0",
            "CFBundleShortVersionString": package["version"],
            "CFBundleVersion": output("git", "rev-list", "--count", "HEAD"),
            "LSMinimumSystemVersion": minimum.group(1),
            "LSArchitecturePriority": ["arm64"], "NSHighResolutionCapable": True,
        }
        with (app / "Contents/Info.plist").open("wb") as stream:
            plistlib.dump(info, stream)
        (app / "Contents/PkgInfo").write_bytes(b"APPL????")
        run("codesign", "--force", "--sign", "-", "--timestamp=none", str(macos / "client"))
        run("codesign", "--force", "--sign", "-", "--timestamp=none", str(app))
        run("codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app))

        (staged / "READ ME.txt").write_text(player_readme(args.server_preset))
        manifest = {
            "game": "FistWorld", "version": package["version"], "git_revision": revision,
            "profile": "release", "architecture": "arm64", "minimum_macos": minimum.group(1),
            "protocol_id": protocol.group(1),
            "signing": "ad-hoc; not notarized", "default_server_preset": args.server_preset,
            "runtime_asset_count": len(asset_paths), "system_dependencies": dependencies,
            "files": {
                str(path.relative_to(staged)): {"bytes": path.stat().st_size, "sha256": digest(path)}
                for path in sorted(staged.rglob("*")) if path.is_file()
            },
        }
        (staged / "build-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        run("ditto", "-c", "-k", "--sequesterRsrc", "--keepParent", str(staged), str(archive))
        staged.rename(destination)
    checksum = digest(archive)
    archive.with_suffix(".zip.sha256").write_text(f"{checksum}  {archive.name}\n")
    print(f"Ready: {archive}\nZIP size: {archive.stat().st_size / 1_000_000:.1f} MB\nSHA-256: {checksum}")


if __name__ == "__main__":
    main()

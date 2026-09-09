#!/usr/bin/env python3
"""Run real town-growth lab cases and assemble an offline snapshot viewer.

Example: python3 tools/town_growth.py --seeds 23,41 --profiles low,steady,burst
Rebuild: python3 tools/town_growth.py --skip-run --output logs/town-growth/<run>
The server owns every placement and metric; this tool never generates a town.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import signal
import shlex
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[1]
SMALL_PROFILES = ("low", "steady", "burst")
CITY_PROFILES = tuple(f"city-{population}-{pace}" for population in (100, 250, 500)
                      for pace in ("gradual", "surge"))
PROFILES = SMALL_PROFILES + CITY_PROFILES
DATA_MARKER = "__TOWN_GROWTH_DATA__"


def seed_list(value: str) -> list[int]:
    try:
        seeds = list(dict.fromkeys(int(part.strip()) for part in value.split(",")))
    except ValueError as error:
        raise argparse.ArgumentTypeError("seeds must be comma-separated integers") from error
    if not seeds or any(seed < 0 or seed >= 2**64 for seed in seeds):
        raise argparse.ArgumentTypeError("seeds must fit an unsigned 64-bit integer")
    return seeds


def profile_list(value: str) -> list[str]:
    profiles = list(dict.fromkeys(part.strip() for part in value.split(",")))
    if not profiles or any(profile not in PROFILES for profile in profiles):
        raise argparse.ArgumentTypeError("profiles must be " + ", ".join(PROFILES))
    return profiles


def positive_number(value: str) -> float:
    number = float(value)
    if not math.isfinite(number) or number <= 0:
        raise argparse.ArgumentTypeError("value must be finite and positive")
    return number


def case_environment(base: dict[str, str], seed: int, profile: str,
                     directory: Path, minutes: float, interval: float,
                     warp: float) -> dict[str, str]:
    # Ambient development fixtures must not silently change a comparison case.
    environment = {
        key: value for key, value in base.items()
        if not key.startswith(("FISTWORLD_LAB_", "FISTWORLD_TOWN_", "FISTWORLD_ARMY_"))
        and key not in {"CITYSIM_MAP_ID", "FISTWORLD_VILLAGE_LAB_RUNTIME",
                       "FISTWORLD_REALWORLD_LAB_RUNTIME"}
    }
    environment.update({
        "FISTWORLD_TOWN_SEED": str(seed),
        "FISTWORLD_TOWN_PROFILE": profile,
        "FISTWORLD_TOWN_OUTPUT": str(directory),
        "FISTWORLD_TOWN_MINUTES": str(minutes),
        "FISTWORLD_TOWN_SNAPSHOT_MINUTES": str(interval),
        "FISTWORLD_TOWN_WARP": str(warp),
        "CARGO_TERM_COLOR": "never",
    })
    return environment


def write_json(path: Path, value: object) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")
    temporary.replace(path)


def stop_process(process: subprocess.Popen) -> None:
    """Stop cargo and its test child together, including on a timeout or Ctrl-C."""
    def send(sig: int) -> None:
        try:
            if os.name == "posix":
                os.killpg(process.pid, sig)
            else:
                process.terminate() if sig == signal.SIGTERM else process.kill()
        except ProcessLookupError:
            pass
    send(signal.SIGTERM)
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        send(signal.SIGKILL)
        process.wait()


def run_case(directory: Path, seed: int, profile: str, args: argparse.Namespace) -> None:
    directory.mkdir(parents=True)
    environment = case_environment(dict(os.environ), seed, profile, directory,
                                   args.minutes, args.snapshot_minutes, args.warp)
    command = ([str(args.test_binary), "town_growth_lab", "--ignored", "--nocapture", "--test-threads=1"]
               if args.test_binary else ["cargo", "town-growth-lab"])
    metadata = {
        "seed": str(seed), "profile": profile, "status": "running", "command": command,
        "minutes": args.minutes, "snapshot_minutes": args.snapshot_minutes, "warp": args.warp,
        "started_at": datetime.now(timezone.utc).isoformat(),
    }
    if args.test_binary:
        metadata["test_binary_sha256"] = args.test_binary_sha256
    write_json(directory / "run.json", metadata)
    print(f"Running seed {seed}, {profile}; log: {directory / 'lab.log'}", flush=True)
    started = time.monotonic()
    interrupted = False
    try:
        with (directory / "lab.log").open("w") as log:
            process = subprocess.Popen(command, cwd=ROOT, env=environment, stdout=log,
                                       stderr=subprocess.STDOUT, start_new_session=os.name == "posix")
            try:
                code = process.wait(timeout=args.timeout)
                metadata.update(status="passed" if code == 0 else "failed", returncode=code)
            except subprocess.TimeoutExpired:
                stop_process(process)
                metadata.update(status="timeout", error=f"Exceeded {args.timeout:g} seconds")
            except KeyboardInterrupt:
                stop_process(process)
                metadata.update(status="interrupted", error="Stopped by user")
                interrupted = True
    except OSError as error:
        metadata.update(status="failed", error=str(error))
    metadata["wall_seconds"] = round(time.monotonic() - started, 3)
    write_json(directory / "run.json", metadata)
    print(f"  {metadata['status']} ({metadata['wall_seconds']:g}s)", flush=True)
    if interrupted:
        raise KeyboardInterrupt


def validate_snapshot(snapshot: object) -> dict:
    if not isinstance(snapshot, dict) or snapshot.get("version") != 1:
        raise ValueError("expected TownSnapshot schema version 1")
    for field in ("settlements", "buildings", "roads", "fields", "pastures", "piers"):
        if not isinstance(snapshot.get(field), list):
            raise ValueError(f"missing or invalid {field} array")
    for field in ("day", "elapsed_world_seconds"):
        value = snapshot.get(field)
        if not isinstance(value, (int, float)) or not math.isfinite(value) or value < 0:
            raise ValueError(f"invalid {field}")
    if not isinstance(snapshot.get("metrics"), dict):
        raise ValueError("missing metrics object")
    def vector(value: object, size: int) -> bool:
        return isinstance(value, list) and len(value) == size and all(
            isinstance(number, (int, float)) and math.isfinite(number) for number in value)
    for field in ("settlements", "buildings", "fields", "pastures", "piers"):
        for entry in snapshot[field]:
            if not isinstance(entry, dict) or not vector(entry.get("position"), 3):
                raise ValueError(f"invalid {field} position")
            if not isinstance(entry.get("rotation"), (int, float)) or not math.isfinite(entry["rotation"]):
                raise ValueError(f"invalid {field} rotation")
            if field != "piers" and not (vector(entry.get("footprint"), 2) and vector(entry.get("footprint_center"), 2)):
                raise ValueError(f"invalid {field} footprint")
    for town in snapshot["settlements"]:
        square = town.get("civic_square")
        if square is not None:
            if not isinstance(square, dict) or not vector(square.get("center"), 3) or not vector(square.get("market_position"), 3):
                raise ValueError("invalid civic square position")
            if not vector(square.get("half_extents"), 2) or any(value <= 0 for value in square["half_extents"]):
                raise ValueError("invalid civic square dimensions")
            if not all(isinstance(square.get(key), (int, float)) and math.isfinite(square[key])
                       for key in ("rotation", "market_rotation")):
                raise ValueError("invalid civic square rotation")
    for entry in snapshot["roads"]:
        road = entry.get("road", {})
        if not isinstance(road.get("points"), list) or not all(vector(point, 2) for point in road["points"]):
            raise ValueError("invalid road points")
        if not isinstance(road.get("built_through"), int) or road["built_through"] < 0:
            raise ValueError("invalid road construction progress")
        if not isinstance(road.get("width"), (int, float)) or not math.isfinite(road["width"]) or road["width"] <= 0:
            raise ValueError("invalid road width")
    for field in ("districts", "fortifications"):
        if not isinstance(snapshot.get(field, []), list):
            raise ValueError(f"invalid {field} array")
    for district in snapshot.get("districts", []):
        if not isinstance(district, dict) or not all(vector(district.get(field), 2)
                                                     for field in ("center", "axis", "half_extents")):
            raise ValueError("invalid residential district geometry")
        if any(value <= 0 for value in district["half_extents"]) or abs(sum(value**2 for value in district["axis"]) - 1.0) > 0.01:
            raise ValueError("invalid residential district dimensions")
    for section in snapshot.get("fortifications", []):
        if not isinstance(section, dict) or not vector(section.get("start"), 3) or not vector(section.get("end"), 3):
            raise ValueError("invalid fortification geometry")
    return snapshot


def summarize_growth(snapshots: list[dict], profile: str) -> dict:
    """Separate integrity-test success from actual city-growth achievements."""
    if not snapshots:
        return {}
    last = snapshots[-1]
    target = int(profile.split("-")[1]) if profile in CITY_PROFILES else {
        "low": 14, "steady": 29, "burst": 32,
    }.get(profile)
    residents = [frame["metrics"].get("residents", 0) for frame in snapshots]
    complete = [frame["metrics"].get("completed_buildings", 0) for frame in snapshots]
    last_building_change = snapshots[0]["day"]
    for index in range(1, len(snapshots)):
        if complete[index] != complete[index - 1]:
            last_building_change = snapshots[index]["day"]
    milestones = {}
    for population in (100, 250, 500):
        milestones[str(population)] = next((frame["day"] for frame in snapshots
                                           if frame["metrics"].get("residents", 0) >= population), None)
    transitions = []
    previous = {}
    for frame in snapshots:
        for town in frame["settlements"]:
            identity = str(town["id"])
            state = (town["tier"], town["development"]["next_gate"])
            if previous.get(identity) != state:
                transitions.append({"day": frame["day"], "settlement": town["name"],
                                    "tier": state[0], "gate": state[1]})
                previous[identity] = state
    return {
        "offered_population_target": target,
        "population_target_reached": target is not None and max(residents) >= target,
        "peak_residents": max(residents),
        "final_residents": residents[-1],
        "final_housed": last["metrics"].get("housed"),
        "town_tier_reached": any(town["tier"] in ("Town", "City")
                                 for frame in snapshots for town in frame["settlements"]),
        "city_tier_reached": any(town["tier"] == "City" for frame in snapshots for town in frame["settlements"]),
        "milestone_days": milestones,
        "days_since_last_completed_building": last["day"] - last_building_change,
        "tier_gate_changes": transitions,
    }


def read_case(directory: Path, output: Path) -> dict:
    metadata = json.loads((directory / "run.json").read_text())
    result = {**metadata, "snapshots": [], "errors": [],
              "log": str((directory / "lab.log").relative_to(output))}
    for path in sorted(directory.glob("snapshot-*.json")):
        try:
            snapshot = validate_snapshot(json.loads(path.read_text()))
            if str(snapshot.get("seed")) != str(metadata["seed"]) or snapshot.get("profile") != metadata["profile"]:
                raise ValueError("snapshot seed/profile disagrees with the recorded case")
            # Full terrain edits remain in the original export for Bevy. The plan
            # viewer does not use them, so do not duplicate large grids in HTML.
            snapshot.pop("terrain_deltas", None)
            snapshot["source_path"] = str(path.relative_to(output))
            # Runs may originate in an isolated checkout. The report's command
            # must still address this exact snapshot from the user's repo root.
            capture_path = str(path.resolve())
            snapshot["capture_command"] = shlex.join(["python3", "capture/town_growth.py", capture_path])
            result["snapshots"].append(snapshot)
        except (OSError, ValueError, TypeError) as error:
            result["errors"].append(f"{path.name}: {error}")
    result["snapshots"].sort(key=lambda snapshot: snapshot["elapsed_world_seconds"])
    if not result["snapshots"]:
        result["errors"].append("No valid simulation snapshots were written.")
    if result["errors"] and result["status"] == "passed":
        result["status"] = "invalid-output"
    result["growth"] = summarize_growth(result["snapshots"], metadata["profile"])
    return result


def embed_json(value: object) -> str:
    # The JSON sits inside a script element; escape HTML delimiters even in names/errors.
    def safe_integers(item: object) -> object:
        if isinstance(item, dict):
            return {key: safe_integers(child) for key, child in item.items()}
        if isinstance(item, list):
            return [safe_integers(child) for child in item]
        if isinstance(item, int) and abs(item) > 2**53 - 1:
            return str(item)
        return item
    return json.dumps(safe_integers(value), separators=(",", ":"), allow_nan=False).replace("&", "\\u0026").replace("<", "\\u003c").replace(">", "\\u003e")


def build_report(output: Path) -> tuple[Path, bool]:
    cases = []
    for metadata_path in sorted(output.glob("seed-*__*/run.json")):
        try:
            cases.append(read_case(metadata_path.parent, output))
        except (OSError, ValueError, KeyError, TypeError) as error:
            cases.append({"seed": "unknown", "profile": metadata_path.parent.name,
                          "status": "invalid-output", "snapshots": [],
                          "errors": [f"{metadata_path.name}: {error}"],
                          "log": str((metadata_path.parent / "lab.log").relative_to(output))})
    if not cases:
        raise ValueError(f"No recorded cases under {output}; --skip-run needs an existing run directory")
    manifest_path = output / "experiment.json"
    manifest = json.loads(manifest_path.read_text()) if manifest_path.exists() else {}
    for seed in manifest.get("seeds", []):
        for profile in manifest.get("profiles", []):
            if not any(str(case["seed"]) == str(seed) and case["profile"] == profile for case in cases):
                cases.append({"seed": str(seed), "profile": profile, "status": "not-run",
                              "snapshots": [], "errors": ["This requested case did not run."]})
    seed_order = {str(seed): index for index, seed in enumerate(manifest.get("seeds", []))}
    profile_order = {profile: index for index, profile in enumerate(manifest.get("profiles", PROFILES))}
    cases.sort(key=lambda case: (seed_order.get(str(case["seed"]), len(seed_order)),
                                str(case["seed"]), profile_order.get(case["profile"], len(profile_order))))
    data = {"generated_at": datetime.now(timezone.utc).isoformat(), "experiment": manifest, "cases": cases}
    template = Path(__file__).with_name("town_growth_viewer.html").read_text()
    if template.count(DATA_MARKER) != 1:
        raise ValueError("viewer template must contain exactly one data marker")
    report = output / "report.html"
    report.write_text(template.replace(DATA_MARKER, embed_json(data)))
    write_json(output / "results.json", data)
    return report, all(case["status"] == "passed" for case in cases)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--seeds", type=seed_list, default=[23, 41, 77])
    parser.add_argument("--profiles", type=profile_list, default=list(SMALL_PROFILES))
    parser.add_argument("--minutes", type=positive_number,
                        help="simulated minutes per case (default 240; 1440 when any city profile is selected)")
    parser.add_argument("--snapshot-minutes", type=positive_number, default=20.0)
    parser.add_argument("--warp", type=positive_number, default=25.0)
    parser.add_argument("--timeout", type=positive_number,
                        help="wall seconds per case, including build (default 1800; 3600 for large profiles)")
    parser.add_argument("--test-binary", type=Path,
                        help="reuse a previously verified server test binary instead of compiling; its hash is recorded")
    parser.add_argument("--output", type=Path, help="new output directory; defaults to logs/town-growth/<timestamp>")
    parser.add_argument("--skip-run", action="store_true", help="rebuild report from all cases in --output")
    args = parser.parse_args(argv)
    large_profiles = any(profile in CITY_PROFILES for profile in args.profiles)
    if args.minutes is None:
        args.minutes = 1440.0 if large_profiles else 240.0
    if args.timeout is None:
        args.timeout = 3600.0 if large_profiles else 1800.0
    if args.minutes > 2880:
        parser.error("--minutes must be at most 2880")
    if args.warp > 1000:
        parser.error("--warp must be at most 1000")
    if args.skip_run and args.output is None:
        parser.error("--skip-run requires --output")
    if args.test_binary:
        args.test_binary = args.test_binary.resolve()
        if not args.test_binary.is_file() or not os.access(args.test_binary, os.X_OK):
            parser.error("--test-binary must name an existing executable server test binary")
        digest = hashlib.sha256()
        with args.test_binary.open("rb") as executable:
            for chunk in iter(lambda: executable.read(1024 * 1024), b""):
                digest.update(chunk)
        args.test_binary_sha256 = digest.hexdigest()
    output = (args.output or ROOT / "logs" / "town-growth" / datetime.now().strftime("%Y%m%d-%H%M%S")).resolve()
    try:
        if not args.skip_run:
            if output.exists() and any(output.iterdir()):
                parser.error("output directory is not empty; choose a new directory or use --skip-run")
            output.mkdir(parents=True, exist_ok=True)
            revision = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
            dirty = bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True))
            write_json(output / "experiment.json", {"revision": revision, "working_tree_dirty": dirty,
                       "seeds": [str(seed) for seed in args.seeds], "profiles": args.profiles,
                       "minutes": args.minutes, "snapshot_minutes": args.snapshot_minutes, "warp": args.warp,
                       "test_binary": str(args.test_binary) if args.test_binary else None,
                       "test_binary_sha256": getattr(args, "test_binary_sha256", None)})
            try:
                for seed in args.seeds:
                    for profile in args.profiles:
                        run_case(output / f"seed-{seed}__{profile}", seed, profile, args)
            except KeyboardInterrupt:
                print("Interrupted; preserving completed and partial snapshots.", file=sys.stderr)
        report, passed = build_report(output)
        print(f"Report: {report}\nAll integrity checks passed: {passed}")
        return 0 if passed else 1
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f"town-growth: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())

#!/usr/bin/env python3
"""Audit founding spread and capture an ordinary connected world's real directory.

`run` owns its fresh server/client, uses normal creation, then camera-only inputs.
`compare` reuses a saved founding log and the Rust audit's surveyed viable sites;
it does not launch a game or treat ocean/mountain area as missing settlement land.
"""
import argparse
import csv
import json
import math
from pathlib import Path
import re
import subprocess

import first_session as harness

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_SEED = 4794248476676134349


def baseline_towns(path, seed):
    text = Path(path).read_text()
    match = re.search(r"World opening ready: seed=(\d+) settlements=(\d+)", text)
    if not match or int(match[1]) != seed:
        raise ValueError("Baseline log does not prove the requested world seed")
    pattern = (r"Founding (\w+): residents=(\d+) buildings=(\d+) "
               r"at=\(([-\d.]+),([-\d.]+)\) farmland=([\d.]+) timber=([\d.]+) stone=([\d.]+)")
    rows = [{"name": name, "population": int(population), "buildings": int(buildings),
             "x": float(x), "z": float(z), "farmland": float(farm),
             "timber": float(wood), "stone": float(stone)}
            for name, population, buildings, x, z, farm, wood, stone in re.findall(pattern, text)]
    if len(rows) != int(match[2]):
        raise ValueError("Baseline log does not contain every founding position exactly once")
    return rows


def spread(towns, sites):
    eligible = [s for s in sites if s["eligible"] == "true"]
    if not towns or not eligible:
        raise ValueError("Distribution metrics require towns and viable coastal land")
    nearest = sorted(min(math.hypot(t["x"] - s["x"], t["z"] - s["z"]) for t in towns)
                     for s in eligible)
    regions = {}
    for site in eligible:
        regions[site["region"]] = regions.get(site["region"], 0) + 1
    major = {r for r, count in regions.items() if count >= max(20, math.ceil(len(eligible) * .1))}
    occupied = set()
    for town in towns:
        site = min(sites, key=lambda s: math.hypot(s["x"] - town["x"], s["z"] - town["z"]))
        if math.hypot(site["x"] - town["x"], site["z"] - town["z"]) > .2:
            raise ValueError(f"Town {town['name']} is not on this audit's seeded survey grid")
        occupied.add(site["region"])
    return {"town_count": len(towns), "eligible_sites": len(eligible),
            "nearest_mean_m": sum(nearest) / len(nearest),
            "nearest_p90_m": nearest[math.ceil(len(nearest) * .9) - 1],
            "nearest_max_m": nearest[-1],
            "covered_1500_fraction": sum(d <= 1500 for d in nearest) / len(nearest),
            "major_regions": sorted(major), "occupied_major_regions": sorted(major & occupied),
            "occupied_regions": sorted(occupied),
            "span_m": [max(t[c] for t in towns) - min(t[c] for t in towns) for c in ("x", "z")]}


def output_directory(path, fresh=False):
    path = Path(path).resolve()
    if not path.is_relative_to(ROOT / "logs"):
        raise ValueError("Review artifacts belong under repository logs/")
    if fresh and path.exists() and any(path.iterdir()):
        raise ValueError("Use a fresh output directory")
    path.mkdir(parents=True, exist_ok=True)
    return path


def plot_comparison(out, report, sites, bounds):
    """Optional CPU-only audit figure; no invented terrain or route geometry."""
    import matplotlib
    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from matplotlib.lines import Line2D

    eligible = [site for site in sites if site["eligible"] == "true"]
    excluded = [site for site in sites if site["eligible"] != "true"]
    figure, axes = plt.subplots(1, 2, figsize=(14, 8.5), facecolor="#f8f6f0")
    colors = {True: "#4c9d7a", False: "#c76953"}
    for axis, key, title in zip(axes, ("baseline", "current"), ("Before: clustered opening", "After: distributed opening")):
        towns = report[f"{key}_towns"]
        metrics = report[key]
        axis.set_facecolor("#f0eee6")
        axis.scatter([s["x"] / 1000 for s in excluded], [s["z"] / 1000 for s in excluded],
                     s=8, c="#c9c9c4", linewidths=0, zorder=2)
        for covered in (False, True):
            points = [site for site in eligible if (
                min(math.hypot(t["x"] - site["x"], t["z"] - site["z"]) for t in towns) <= 1500
            ) == covered]
            axis.scatter([s["x"] / 1000 for s in points], [s["z"] / 1000 for s in points],
                         s=13, c=colors[covered], linewidths=0, zorder=3)
        axis.scatter([t["x"] / 1000 for t in towns], [t["z"] / 1000 for t in towns],
                     s=125, marker="P", c="#172e37", edgecolors="white", linewidths=.8, zorder=4)
        axis.set(xlim=(bounds[0] / 1000, bounds[2] / 1000),
                 ylim=(bounds[1] / 1000, bounds[3] / 1000), xlabel="World X (km)", ylabel="World Z (km)")
        axis.set_aspect("equal")
        axis.set_xticks(range(-4, 5, 2))
        axis.set_yticks(range(-4, 5, 2))
        axis.grid(color="#d8d6ce", linewidth=.6, zorder=1)
        axis.set_axisbelow(True)
        for spine in axis.spines.values():
            spine.set_color("#b6b6ad")
        axis.set_title(title, loc="left", fontsize=16, weight="bold", color="#172e37", pad=13)
        axis.text(.025, .965,
                  f"{metrics['town_count']} towns  |  {metrics['covered_1500_fraction']:.1%} within 1.5 km\n"
                  f"Mean {metrics['nearest_mean_m']:,.0f} m  |  90th percentile {metrics['nearest_p90_m']:,.0f} m",
                  transform=axis.transAxes, va="top", fontsize=11, color="#172e37",
                  bbox={"facecolor": "#fffef9", "edgecolor": "#d8d6ce", "pad": 8, "alpha": .96}, zorder=5)
    figure.suptitle("Same generated world, better settlement coverage", fontsize=21,
                    weight="bold", color="#172e37", x=.07, y=.96, ha="left")
    figure.text(.07, .915, f"Seed {report['seed']}  |  {len(eligible)} eligible survey sites  |  actual founding positions",
                fontsize=12, color="#57656a")
    figure.legend(handles=[
        Line2D([], [], marker="P", color="none", markerfacecolor="#172e37", markersize=10, label="Town hall"),
        Line2D([], [], marker="o", color="none", markerfacecolor=colors[True], label="Viable site within 1.5 km"),
        Line2D([], [], marker="o", color="none", markerfacecolor=colors[False], label="Viable site farther away"),
        Line2D([], [], marker="o", color="none", markerfacecolor="#c9c9c4", label="Survey site outside coastal shortlist"),
    ], loc="lower center", bbox_to_anchor=(.5, .095), ncol=2, frameon=False, fontsize=11)
    figure.text(.07, .049,
                "Both sides use the same current viable-site survey. Blank areas are unsurveyed or rejected, not a drawn coastline.\n"
                "Distances are straight-line spatial coverage, not certified travel routes or promised buildable plots.",
                fontsize=10, color="#57656a", linespacing=1.6)
    figure.subplots_adjust(left=.07, right=.98, bottom=.20, top=.85, wspace=.12)
    figure.savefig(out / "distribution-comparison.png", dpi=150, facecolor=figure.get_facecolor())
    figure.savefig(out / "distribution-comparison.svg", facecolor=figure.get_facecolor())
    plt.close(figure)


def compare(args):
    directory = Path(args.audit_dir)
    with (directory / f"{args.seed}-sites.csv").open() as source:
        sites = [{**row, "x": float(row["x"]), "z": float(row["z"]), "region": int(row["region"])}
                 for row in csv.DictReader(source)]
    with (directory / f"{args.seed}-towns.csv").open() as source:
        current = [{**row, "x": float(row["x"]), "z": float(row["z"])} for row in csv.DictReader(source)]
    old = baseline_towns(args.baseline_log, args.seed)
    report = {"seed": args.seed, "baseline_log": str(Path(args.baseline_log).resolve()),
              "audit_dir": str(directory.resolve()), "baseline": spread(old, sites),
              "current": spread(current, sites), "baseline_towns": old, "current_towns": current,
              "scope": "Both openings are evaluated against the same current equal-area viable-site survey on coastal-hint land; not a proof that every surveyed site can fit a town or that every distance is a traversable route."}
    out = output_directory(args.out)
    (out / "distribution-comparison.json").write_text(json.dumps(report, indent=2))
    if args.plot:
        with (directory / f"{args.seed}-coverage.csv").open() as source:
            bounds = next(csv.DictReader(source))
        plot_comparison(out, report, sites,
                        [float(bounds[key]) for key in ("min_x", "min_z", "max_x", "max_z")])
    print(json.dumps({"baseline": report["baseline"], "current": report["current"]}, indent=2))


def run(args):
    out = output_directory(args.out, fresh=True)
    occupied = subprocess.run(["lsof", "-nP", "-iUDP:5000"], capture_output=True, text=True)
    if occupied.returncode == 0:
        raise RuntimeError("UDP5000 is occupied; existing server left alone: " + occupied.stdout)
    server = client = None
    report = {"seed": args.seed, "passed": False, "visual_review_required": True,
              "scope": "Ordinary connected startup and camera views; no resource grants, actor placement, movement orders or time warp.",
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True)),
              "captures": []}
    try:
        env = harness.environment(ROOT) | {"FISTWORLD_WORLD_SEED": str(args.seed), "FISTWORLD_DEV": "0"}
        with (out / "server.log").open("w") as log:
            server = subprocess.Popen([ROOT / "target/playtest/server"], cwd=ROOT, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        print("Owned server", server.pid, flush=True)
        harness.wait_server(server, out / "server.log", lambda s: "Server UDP socket bound" in s,
                            "world founding and server readiness", timeout=600)
        report["founding"] = baseline_towns(out / "server.log", args.seed)
        session_dir = out / "views"
        session_dir.mkdir()
        env = harness.environment(ROOT) | {
            "FISTFORCE_NO_SETTINGS_FILE": "1", "FISTFORCE_AUTOCONNECT": "WorldOverviewReview",
            "FISTFORCE_DISPLAY_MODE": "windowed", "FISTFORCE_FULLSCREEN": "0",
            "FISTFORCE_RESOLUTION": args.resolution, "FISTFORCE_RENDER_SCALE": "1",
            "FISTFORCE_FRAME_CAP": "60", "FISTWORLD_SESSION_CAPTURE_DIR": str(session_dir),
        }
        with (out / "client.log").open("w") as log:
            client = subprocess.Popen([ROOT / "target/playtest/client"], cwd=ROOT, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        print("Owned client", client.pid, flush=True)
        (out / "processes.json").write_text(json.dumps({"server": server.pid, "client": client.pid}))
        session = harness.Session(session_dir)
        state = session.wait(lambda s: s.get("playing") and (s.get("creator") or s.get("hero")),
                             "ordinary startup", timeout=240)
        assert not state["god"]
        if state.get("creator"):
            session.command("button", name="creator-BEGIN JOURNEY")
        state = session.wait(lambda s: s.get("hero") and not s.get("cinematic")
                             and len(s.get("towns", [])) == len(report["founding"]),
                             "hero and complete replicated world directory", timeout=180)
        harness.close_book(session)
        towns = sorted(state["towns"], key=lambda t: (-t["residents"], t["id"]))
        first = towns[0]
        second = max(towns[1:], key=lambda t: harness.distance(t["position"], first["position"]))
        report["directory"] = towns
        report["hero_before"] = state["hero"]
        views = [("01-world", [0, 0, 0], 12000, None),
                 ("02-populated-region", first["position"], 3000, None),
                 ("03-largest-town", first["position"], 145, first["id"]),
                 ("04-distant-town", second["position"], 145, second["id"]),
                 ("05-world-return", [0, 0, 0], 12000, None)]
        for name, position, zoom, town_id in views:
            session.command("view", x=position[0], z=position[2], zoom=zoom)
            if town_id is not None:
                session.wait(lambda s: any(m["id"] == town_id for m in s.get("markets", [])),
                             f"replicated physical town {town_id}", timeout=180)
            session.command("capture", name=name, timeout=180)
            metadata = json.loads((session_dir / f"{name}.capture.json").read_text())
            snapshot = json.loads((session_dir / f"{name}.session.json").read_text())
            assert not snapshot["god"] and not snapshot["creator"]
            assert len(snapshot["towns"]) == len(towns)
            assert (session_dir / f"{name}.png").stat().st_size > 0
            assert all(a["passed"] for a in metadata["assertions"])
            assert metadata["world"]["ground_paint_pending_chunks"] == 0
            if town_id is not None:
                assert metadata["world"]["settlement_buildings"] > 0
                assert metadata["world"]["villagers"] > 0
                assert metadata["world"]["building_lod_pending"] == 0
            report["captures"].append({"name": name, "town_id": town_id,
                                       "camera": metadata["camera"], "world": metadata["world"]})
            (out / "report.json").write_text(json.dumps(report, indent=2))
        report["hero_after"] = session.status()["hero"]
        assert report["hero_before"]["person_id"] == report["hero_after"]["person_id"]
        report["passed"] = True
        session.command("quit")
        client.wait(timeout=20)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        (out / "report.json").write_text(json.dumps(report, indent=2))
        harness.stop(client)
        harness.stop(server)
    print("Five real connected captures complete; inspect PNGs with capture/session sidecars.", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    live = commands.add_parser("run")
    live.add_argument("--seed", type=int, default=DEFAULT_SEED)
    live.add_argument("--out", required=True)
    live.add_argument("--resolution", default="1600x1000")
    live.set_defaults(function=run)
    comparison = commands.add_parser("compare")
    comparison.add_argument("--seed", type=int, default=DEFAULT_SEED)
    comparison.add_argument("--audit-dir", required=True)
    comparison.add_argument("--baseline-log", required=True)
    comparison.add_argument("--out", required=True)
    comparison.add_argument("--plot", action="store_true", help="Also write a CPU-only PNG/SVG survey plot (requires matplotlib)")
    comparison.set_defaults(function=compare)
    arguments = parser.parse_args()
    arguments.function(arguments)

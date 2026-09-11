#!/usr/bin/env python3
"""Walk actual server-accepted garden gates using the connected production input driver."""
import argparse
import json
from pathlib import Path
import subprocess
import time

import first_session as harness


def walk(session, destination, label):
    state = session.status()
    start = state["hero"]["position"]
    session.command("view", x=destination[0], z=destination[2], zoom=32)
    session.command("right_click", x=destination[0], z=destination[2])
    deadline = time.monotonic() + max(45, harness.distance(start, destination) / 1.3 + 30)
    last = start
    progress = time.monotonic()
    observed = []
    while time.monotonic() < deadline:
        hero = session.status().get("hero")
        if hero:
            position = hero["position"]
            if not observed or position != observed[-1]["position"]:
                observed.append({"elapsed": time.monotonic(), "position": position})
            if harness.distance(position, destination) < 0.85 and not hero["aboard"]:
                return {"label": label, "target": destination, "arrival": position, "observed": observed}
            if harness.distance(position, last) > 0.25:
                last, progress = position, time.monotonic()
        if time.monotonic() - progress > 25:
            raise TimeoutError(f"Stopped outside {label}: {hero}; target={destination}")
        time.sleep(0.1)
    raise TimeoutError(f"Did not arrive at {label}: {session.status().get('hero')}")


def run(args):
    root = Path(__file__).resolve().parents[1]
    out = Path(args.out).resolve()
    if out.exists() and any(out.iterdir()):
        raise RuntimeError("Use a fresh output directory")
    out.mkdir(parents=True, exist_ok=True)
    probe = subprocess.run(["lsof", "-nP", "-iUDP:5000"], capture_output=True, text=True)
    if probe.returncode == 0:
        raise RuntimeError("UDP 5000 is occupied; existing server was left alone: " + probe.stdout)
    server = client = None
    report = {"seed": args.seed, "passed": False, "routes": [], "yards": [],
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
              "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True))}
    try:
        env = harness.environment(root) | {"FISTWORLD_WORLD_SEED": str(args.seed), "FISTWORLD_DEV": "0"}
        with (out / "server.log").open("w") as log:
            server = subprocess.Popen([root / "target/playtest/server"], env=env, stdout=log, stderr=subprocess.STDOUT)
        print("Owned server", server.pid, flush=True)
        harness.wait_server(server, out / "server.log", lambda text: "Server UDP socket bound" in text, "bootstrap")
        client, session = harness.start_client(root, out, "YardWalkingReview", args.resolution)
        print("Owned client", client.pid, flush=True)
        state = session.wait(lambda s: s.get("playing") and (s.get("creator") or s.get("hero")), "arrival", 180)
        if state.get("creator"):
            session.command("button", name="creator-BEGIN JOURNEY")
        state = session.wait(lambda s: s.get("hero") and not s.get("cinematic") and s.get("towns"), "hero and town directory", 180)
        harness.close_book(session)
        session.command("key", key="Home")
        town = min(state["towns"], key=lambda t: harness.distance(t["position"], state["hero"]["position"]))
        session.command("view", x=town["position"][0], z=town["position"][2], zoom=140)
        state = session.wait(lambda s: any(m["id"] == town["id"] for m in s["markets"]) and s.get("yards"), "town buildings and accepted gardens", 180)
        market = next(m for m in state["markets"] if m["id"] == town["id"])
        session.command("right_click", x=market["entrance"][0], z=market["entrance"][2])
        harness.arrive(session, market["entrance"], "ordinary voyage and landing", 300)
        state = session.wait(lambda s: s.get("hero") and not s["hero"]["aboard"], "hero disembarked", 60)
        report["arrival_town"] = town
        candidates = sorted(state["yards"], key=lambda y: harness.distance(y["approach"], state["hero"]["position"]))
        for number, yard in enumerate(candidates[:args.yards], 1):
            report["yards"].append(yard)
            for location in ["approach", "entry", "center", "entry", "approach"]:
                route = walk(session, yard[location], f"garden-{number}-{location}")
                report["routes"].append(route)
                if location == "center":
                    session.command("capture", name=f"{number:02}-inside-garden")
                (out / "report.json").write_text(json.dumps(report, indent=2))
            session.command("capture", name=f"{number:02}-street-exit")
        current = {yard["id"]: yard for yard in session.status().get("yards", [])}
        for yard in report["yards"]:
            assert yard["id"] in current and current[yard["id"]]["recipe"] == yard["recipe"], "Garden changed during the roundtrip; review and repeat against current accepted geometry"
        report["passed"] = len(report["yards"]) == args.yards
        session.command("quit")
        client.wait(timeout=20)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        (out / "report.json").write_text(json.dumps(report, indent=2))
        harness.stop(client)
        harness.stop(server)
    print(json.dumps({"passed": report["passed"], "yards":len(report["yards"]), "routes":len(report["routes"])}), flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True)
    parser.add_argument("--seed", type=int, default=7)
    parser.add_argument("--yards", type=int, default=2)
    parser.add_argument("--resolution", default="1600x1000")
    run(parser.parse_args())

#!/usr/bin/env python3
"""Connected two-client arrival, peaceful proximity and leave/rejoin regression.

Runs the maintained coastal Village Lab at 1x with production player input,
networking, boats and combat. No hero teleport, health reset, inventory grant,
combat override or direct gameplay message is used. A report is successful only
when every required behavior and real motion capture was observed.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor
import json
import math
from pathlib import Path
import shutil
import socket
import subprocess
import threading
import time

from first_session import Session, distance, enabled, environment, stop, wait_server
from startup_session import join, menu, replace


def account_seed(account):
    value = 0xCBF29CE484222325
    for byte in account.lower().encode():
        value = ((value ^ byte) * 0x100000001B3) & ((1 << 64) - 1)
    return value


def coastal_signature(account):
    value = account_seed(account)
    return value & 3, (value >> 8) % 48


def matching_account(first):
    # This is test data selection, not a server start override. On village_lab
    # these two ordinary account names prefer the same natural coastal route.
    for index in range(10000):
        candidate = f"Peaceful{index}"
        if candidate.lower() != first.lower() and coastal_signature(candidate) == coastal_signature(first):
            return candidate
    raise AssertionError("could not find matching coastal account")


def read_status(directory):
    try:
        return json.loads((directory / "status.json").read_text())
    except FileNotFoundError:
        return {}


def hero_for(state, account):
    return next((hero for hero in state.get("multiplayer", {}).get("heroes", [])
                 if (hero.get("account") or "").lower() == account.lower()), None)


def boat_for(state, account):
    return next((boat for boat in state.get("multiplayer", {}).get("boats", [])
                 if (boat.get("account") or "").lower() == account.lower() and not boat["wrecked"]), None)


class Audit:
    """Continuously check real samples even while commands/captures are pending."""
    def __init__(self, out, sessions, accounts):
        self.out, self.sessions, self.accounts = out, sessions, accounts
        self.expected = {}
        self.identities = {}
        self.failure = None
        self.samples = 0
        self.minimum_boat_distance = None
        self.npc_boats = set()
        self.stop_event = threading.Event()
        self.lock = threading.Lock()
        self.thread = threading.Thread(target=self.sample, daemon=True)
        self.thread.start()

    def arm(self, label, state):
        hero = hero_for(state, self.accounts[label])
        assert hero is not None and hero["health"] is not None, state
        with self.lock:
            prior = self.identities.get(label)
            identity = {"person_id": hero["person_id"], "health": hero["health"]["current"],
                        "account": hero["account"]}
            assert prior is None or prior == identity, (prior, identity)
            self.identities[label] = identity
            self.expected[label] = True
        return hero

    def allow_disconnect(self, label):
        with self.lock:
            self.expected[label] = False

    def check(self):
        if self.failure:
            raise AssertionError(self.failure)

    def sample(self):
        last = {}
        with (self.out / "samples.jsonl").open("a") as journal:
            while not self.stop_event.wait(0.1):
                try:
                    with self.lock:
                        expected, identities = dict(self.expected), dict(self.identities)
                    for label, session in self.sessions.items():
                        state = read_status(session.directory)
                        stamp = state.get("sampled_unix_ms")
                        if expected.get(label) and stamp and time.time() - stamp / 1000 > 20:
                            raise AssertionError(f"{label}: client stopped publishing fresh status")
                        if not stamp or stamp == last.get(label):
                            continue
                        last[label] = stamp
                        self.samples += 1
                        journal.write(json.dumps({"observer": label, "state": state}) + "\n")
                        journal.flush()
                        if expected.get(label):
                            multi = state.get("multiplayer", {})
                            hero = hero_for(state, self.accounts[label])
                            assert state.get("playing") and multi.get("connected_clients") == 1, (label, state)
                            assert not state.get("connection_error"), (label, state["connection_error"])
                            assert hero and hero["person_id"] == identities[label]["person_id"], (label, hero)
                            assert hero["owner_peer"] == multi["local_peer"], (label, hero, multi["local_peer"])
                        for hero in state.get("multiplayer", {}).get("heroes", []):
                            owner = next((key for key, account in self.accounts.items()
                                          if (hero.get("account") or "").lower() == account.lower()), None)
                            if owner not in identities:
                                continue
                            assert hero["person_id"] == identities[owner]["person_id"], (owner, hero)
                            assert hero["health"] and hero["health"]["current"] == identities[owner]["health"], (owner, hero)
                            assert hero["engaged_with"] is None and not hero["combat_ready"] and hero["swing"] is None, (owner, hero)
                        boats = state.get("multiplayer", {}).get("boats", [])
                        for boat in boats:
                            if boat["npc_arrival"]:
                                self.npc_boats.add((label, boat["entity"]))
                        ours = [boat for boat in boats if (boat.get("account") or "").lower()
                                in {account.lower() for account in self.accounts.values()} and not boat["wrecked"]]
                        if len(ours) == 2:
                            gap = distance(ours[0]["position"], ours[1]["position"])
                            self.minimum_boat_distance = gap if self.minimum_boat_distance is None else min(self.minimum_boat_distance, gap)
                            # Admission clearance is asserted at the initial still.
                            # This value observes subsequent motion; vessels do not
                            # yet implement general ship-to-ship collision avoidance.
                except Exception as error:
                    self.failure = repr(error)
                    (self.out / "audit-failure.json").write_text(json.dumps({"error": self.failure}, indent=2))
                    return

    def close(self):
        self.stop_event.set()
        self.thread.join(timeout=5)


def wait(session, audit, predicate, description, timeout=90):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        audit.check()
        state = session.status()
        if state and predicate(state):
            return state
        time.sleep(0.1)
    raise TimeoutError(f"{description}: {session.status()}")


def observe(session, audit, seconds, predicate, description):
    deadline = time.monotonic() + seconds
    samples, last = [], None
    while time.monotonic() < deadline:
        audit.check()
        state = session.status()
        stamp = state.get("sampled_unix_ms")
        if stamp and stamp != last:
            assert predicate(state), f"{description}: {state}"
            samples.append(state)
            last = stamp
        time.sleep(0.1)
    assert len(samples) >= max(2, seconds * 0.5), f"insufficient live samples: {description}"
    return samples


def start_client(root, out, resolution):
    out.mkdir(parents=True)
    env = environment(root)
    env.update(FISTFORCE_NO_SETTINGS_FILE="1", FISTFORCE_DISPLAY_MODE="windowed",
               FISTFORCE_FULLSCREEN="0", FISTFORCE_RESOLUTION=resolution,
               FISTFORCE_RENDER_SCALE="1", FISTWORLD_SESSION_CAPTURE_DIR=str(out),
               CITYSIM_MAP_ID="village_lab", FISTFORCE_CLIENT_PERF="1")
    with (out / "client.log").open("w") as log:
        process = subprocess.Popen([root / "target/playtest/client"], cwd=root,
                                   env=env, stdout=log, stderr=subprocess.STDOUT)
    return process, Session(out)


def check_capture(directory, prefix, require_motion=False):
    shots = sorted(directory.glob(f"{prefix}*.capture.json"))
    assert shots, f"missing real capture: {prefix}"
    positions = {}
    dimensions = set()
    for shot in shots:
        png = shot.with_name(shot.name.replace(".capture.json", ".png"))
        metadata = json.loads(shot.read_text())
        state = json.loads(shot.with_name(shot.name.replace(".capture.json", ".session.json")).read_text())
        assert png.exists() and png.stat().st_size > 1000, png
        assert metadata, shot
        dimensions.add((metadata["width"], metadata["height"]))
        for boat in state["multiplayer"]["boats"]:
            positions.setdefault(boat["entity"], []).append(boat["position"])
        for hero in state["multiplayer"]["heroes"]:
            positions.setdefault(f"hero-{hero['person_id']}", []).append(hero["position"])
    moved = max((max((distance(start, point) for point in samples), default=0)
                 for samples in positions.values() for start in samples[:1]), default=0)
    if require_motion:
        assert len(shots) >= 3 and moved >= 2, f"{prefix}: no demonstrated continuous movement ({moved:.3f}m)"
    return {"frames": len(shots), "maximum_displacement_m": moved,
            "actual_dimensions": sorted(dimensions),
            "artifacts": [str(shot.relative_to(directory)) for shot in shots]}


def claim_udp_port(port):
    # Reserve/test only our requested port; never inspect or stop another task.
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.bind(("0.0.0.0", port))
        return probe.getsockname()[1]
    finally:
        probe.close()


def run(args):
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError(f"Use a fresh output directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    port = claim_udp_port(args.port)
    accounts = {"a": "PeacefulA", "b": matching_account("PeacefulA")}
    report = {"passed": False, "fixture": "village_lab secure; seed 3; eight founders; 1x",
              "port": port, "requested_resolution": args.resolution, "accounts": accounts,
              "coastal_signatures": {key: coastal_signature(name) for key, name in accounts.items()},
              "driver": "production launcher, creator, Home, terrain right-click and pause disconnect",
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
              "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True)),
              "captures": {}, "reconnects": []}
    env = environment(root)
    env.update(CITYSIM_MAP_ID="village_lab", FISTWORLD_VILLAGE_LAB_RUNTIME="1",
               FISTWORLD_LAB_SCENARIO="secure", FISTWORLD_LAB_WARP="1",
               FISTWORLD_LAB_FOUNDERS="8", FISTWORLD_LAB_DAY_TWO_ARRIVALS="0",
               FISTWORLD_NATURAL_IMMIGRATION="1", FISTWORLD_DEV="0",
               FISTWORLD_SERVER_PORT=str(port), FISTFORCE_SERVER_PERF="1")
    server_log = out / "server.log"
    server = wake_guard = audit = None
    processes, sessions = {}, {}
    started = time.monotonic()
    try:
        with server_log.open("w") as log:
            server = subprocess.Popen([root / "target/playtest/server"], cwd=root, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        if caffeinate := shutil.which("caffeinate"):
            wake_guard = subprocess.Popen([caffeinate, "-s", "-w", str(server.pid)],
                                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_server(server, server_log, lambda text: f"Server UDP socket bound to 0.0.0.0:{port}" in text,
                    "owned coastal lab server readiness")
        for label, name in accounts.items():
            process, session = start_client(root, out / label, args.resolution)
            processes[label], sessions[label] = process, session
            (out / "processes.json").write_text(json.dumps({"server": server.pid, **{key: value.pid for key, value in processes.items()}}))
            menu(session)
            replace(session, "startup-server-field", f"127.0.0.1:{port}")
            join(session, name)
            creator = session.wait(lambda state: state.get("playing") and state.get("creator"), "ordinary creator")
            assert creator["hero"] is None and not creator["god"], creator
        audit = Audit(out, sessions, accounts)
        a, b = sessions["a"], sessions["b"]
        a.command("button", name="creator-BEGIN JOURNEY")
        initial_a = wait(a, audit, lambda state: hero_for(state, accounts["a"]) is not None,
                         "first actual starter boat")
        audit.arm("a", initial_a)
        b.command("button", name="creator-BEGIN JOURNEY")
        for label, session in sessions.items():
            ready = wait(session, audit, lambda state, label=label: state.get("hero") is not None
                         and state["hero"]["aboard"] and not state["cinematic"], "both opening cinematics complete")
            audit.arm(label, ready)
            session.command("key", key="Home")
        ready = wait(a, audit, lambda state: boat_for(state, accounts["a"]) is not None
                     and boat_for(state, accounts["b"]) is not None, "both boats replicated together")
        boat_a, boat_b = (boat_for(ready, name) for name in accounts.values())
        initial_gap = distance(boat_a["position"], boat_b["position"])
        assert args.boat_clearance <= initial_gap <= 35, f"arrival collision was not exercised: {initial_gap:.3f}m"
        report["opening"] = {"a": hero_for(ready, accounts["a"]), "b": hero_for(ready, accounts["b"]),
                             "boats": [boat_a, boat_b], "centre_distance_m": initial_gap}
        # The read-only terrain probe supplies an inland point along the
        # actual hull heading. The production server still certifies the route.
        origin, yaw = boat_a["position"], boat_a["yaw"]
        inward = [-math.sin(yaw), -math.cos(yaw)]
        shore = boat_a["forward_dry_point"]
        assert shore is not None, "no dry landing probe ahead of the actual arrival boat"
        for session in sessions.values():
            session.command("view", x=(origin[0] + shore[0]) / 2,
                            z=(origin[2] + shore[2]) / 2, zoom=110)
        a.command("capture", name="01-two-arrivals")
        b.command("capture", name="01-two-arrivals")
        a.command("view", x=(boat_a["position"][0] + boat_b["position"][0]) / 2,
                  z=(boat_a["position"][2] + boat_b["position"][2]) / 2, zoom=32)
        a.command("capture", name="01-close-arrivals")
        a.command("view", x=(origin[0] + shore[0]) / 2,
                  z=(origin[2] + shore[2]) / 2, zoom=110)
        # Pause/rejoin while a real starter route is active. The leaving
        # sailor and hull must remain paired and stop together, while A stays
        # connected. The coast's outward water corridor supplies a short sail.
        before_voyage = {key: session.status() for key, session in sessions.items()}
        departure = boat_for(before_voyage["b"], accounts["b"])["position"]
        b.command("right_click", x=departure[0] - inward[0] * 28,
                  z=departure[2] - inward[1] * 28)
        wait(b, audit, lambda state: (boat_for(state, accounts["b"]) is not None
             and distance(boat_for(state, accounts["b"])["position"], departure) >= 1
             and sum(value * value for value in (boat_for(state, accounts["b"])["velocity"] or [])) > 0.1),
             "boat visibly sailing before owner disconnect", timeout=20)
        b.command("key", key="Escape")
        wait(b, audit, lambda state: enabled(state, "pause-DISCONNECT"), "aboard disconnect control")
        offset = server_log.stat().st_size
        audit.allow_disconnect("b")
        b.command("button", name="pause-DISCONNECT")
        menu(b)
        wait_server(server, server_log,
                    lambda text: f"Freed up name '{accounts['b'].lower()}'" in text[offset:],
                    "aboard account released", timeout=30)
        stopped = wait(a, audit, lambda state: (boat_for(state, accounts["b"]) is not None
                       and sum(value * value for value in (boat_for(state, accounts["b"])["velocity"] or [])) < 0.01),
                       "offline boat motion stops", timeout=10)
        parked = boat_for(stopped, accounts["b"])["position"]
        offline = observe(a, audit, 4,
                          lambda state: (boat_for(state, accounts["b"]) is not None
                          and distance(boat_for(state, accounts["b"])["position"], parked) < 0.15),
                          "offline starter hull remains parked")
        a.command("capture", name="01-offline-voyage")
        join(b, accounts["b"].swapcase())
        resumed_aboard = wait(b, audit, lambda state: state.get("playing") and state.get("hero") is not None
                             and state["hero"]["aboard"] and boat_for(state, accounts["b"]) is not None,
                             "same aboard hero and hull readopted")
        audit.arm("b", resumed_aboard)
        assert not resumed_aboard["creator"] and not resumed_aboard["cinematic"]
        assert resumed_aboard["hero"]["cargo"] == before_voyage["b"]["hero"]["cargo"]
        assert resumed_aboard["hero"]["wallet"] == before_voyage["b"]["hero"]["wallet"]
        assert distance(resumed_aboard["hero"]["position"], boat_for(resumed_aboard, accounts["b"])["position"]) <= 1.5
        assert before_voyage["a"]["multiplayer"]["local_peer"] == a.status()["multiplayer"]["local_peer"]
        resumed_route = wait(b, audit, lambda state: (boat_for(state, accounts["b"]) is not None
                             and distance(boat_for(state, accounts["b"])["position"], parked) >= 1),
                             "original boat route resumes after rejoin without a new order", timeout=15)
        report["aboard_reconnect"] = {"before": before_voyage, "offline": offline[-1],
                                      "after": resumed_aboard, "route_resumed": resumed_route,
                                      "parked_boat_position": parked}
        b.command("key", key="Home")
        a.command("right_click", x=shore[0], z=shore[2])
        with ThreadPoolExecutor(max_workers=1) as executor:
            motion = executor.submit(a.command, "record", name="02-sailing", frames=20, interval_ms=500)
            b.command("right_click", x=shore[0] + inward[1] * 1.5, z=shore[2] - inward[0] * 1.5)
            motion.result()
        report["captures"]["sailing"] = check_capture(a.directory, "02-sailing", require_motion=True)
        for label, session in sessions.items():
            wait(session, audit, lambda state: state.get("hero") is not None and not state["hero"]["aboard"]
                 and distance(state["hero"]["position"], shore) < 8,
                 f"{label} lands through real vessel navigation", timeout=150)
            session.command("key", key="Home")
        landed_a = a.status()["hero"]["position"]
        # Both plain move orders converge at an already reached, collision-valid
        # ground point. The authority owns the actual final body separation.
        b.command("right_click", x=landed_a[0], z=landed_a[2])
        close = lambda state: (hero_for(state, accounts["a"]) is not None
                               and hero_for(state, accounts["b"]) is not None
                               and not hero_for(state, accounts["a"])["aboard"]
                               and not hero_for(state, accounts["b"])["aboard"]
                               and distance(hero_for(state, accounts["a"])["position"],
                                            hero_for(state, accounts["b"])["position"]) <= 2.5)
        wait(a, audit, close, "two peaceful heroes in melee proximity", timeout=60)
        report["peace_before"] = a.status()["multiplayer"]
        peaceful_position = hero_for(a.status(), accounts["a"])["position"]
        a.command("view", x=peaceful_position[0], z=peaceful_position[2], zoom=32)
        a.command("record", name="03-peaceful-neighbours", frames=12, interval_ms=1000)
        samples = observe(a, audit, args.peace_seconds, close, "peaceful proximity remains stable")
        report["peace_after"] = samples[-1]["multiplayer"]
        report["peace_samples"] = len(samples)
        report["captures"]["peace"] = check_capture(a.directory, "03-peaceful-neighbours")
        for cycle in range(1, args.reconnects + 1):
            audit.check()
            before = {key: session.status() for key, session in sessions.items()}
            # A visibly walks while B leaves; no synthetic idle connection probe.
            start = before["a"]["hero"]["position"]
            destination = [start[0] + inward[0] * (12 if cycle % 2 else -12), start[1],
                           start[2] + inward[1] * (12 if cycle % 2 else -12)]
            a.command("right_click", x=destination[0], z=destination[2])
            b.command("key", key="Escape")
            wait(b, audit, lambda state: enabled(state, "pause-DISCONNECT"), "production disconnect control")
            offset = server_log.stat().st_size
            audit.allow_disconnect("b")
            with ThreadPoolExecutor(max_workers=1) as executor:
                recording = executor.submit(a.command, "record", name=f"04-survivor-{cycle}", frames=14, interval_ms=500)
                b.command("button", name="pause-DISCONNECT")
                menu(b)
                wait_server(server, server_log,
                            lambda text: f"Freed up name '{accounts['b'].lower()}'" in text[offset:],
                            "only the leaving account released", timeout=30)
                audit.check()
                join(b, accounts["b"].swapcase())
                resumed = wait(b, audit, lambda state: state.get("playing") and state.get("hero") is not None,
                               "same hero readopted after rejoin")
                assert not resumed["creator"] and not resumed["cinematic"] and not resumed["god"], resumed
                audit.arm("b", resumed)
                recording.result()
            survivor = a.status()
            assert distance(start, survivor["hero"]["position"]) >= 2, "survivor did not move during leave/rejoin"
            assert before["a"]["multiplayer"]["local_peer"] == survivor["multiplayer"]["local_peer"], "survivor connection was replaced"
            for key, state in [("a", survivor), ("b", resumed)]:
                assert state["hero"]["person_id"] == before[key]["hero"]["person_id"]
                assert state["hero"]["cargo"] == before[key]["hero"]["cargo"]
                assert state["hero"]["wallet"] == before[key]["hero"]["wallet"]
            report["reconnects"].append({"cycle": cycle, "before": before, "after": {"a": survivor, "b": resumed},
                                         "survivor_displacement_m": distance(start, survivor["hero"]["position"])})
            report["captures"][f"survivor-{cycle}"] = check_capture(a.directory, f"04-survivor-{cycle}", require_motion=True)
            b.command("key", key="Home")
            b.command("capture", name=f"05-rejoined-{cycle}")
        # Confirm the survivor still accepts a new order after the last rejoin.
        a.command("right_click", x=landed_a[0], z=landed_a[2])
        wait(a, audit, lambda state: distance(state["hero"]["position"], landed_a) < 2.5,
             "survivor returns on a new authoritative move", timeout=60)
        observe(a, audit, 3, lambda state: state.get("playing"), "both sessions remain live")
        audit.check()
        report["passed"] = True
    except Exception as error:
        report["error"] = repr(error)
        raise
    finally:
        if audit:
            audit.close()
            report.update(audit_samples=audit.samples, audit_failure=audit.failure,
                          minimum_starter_boat_distance_m=audit.minimum_boat_distance,
                          observed_npc_boat_entities=len(audit.npc_boats))
            if audit.failure:
                report["passed"] = False
        report["elapsed_seconds"] = round(time.monotonic() - started, 2)
        report["final_states"] = {label: read_status(session.directory) for label, session in sessions.items()}
        report["actual_capture_dimensions"] = sorted({
            (metadata["width"], metadata["height"])
            for path in out.rglob("*.capture.json")
            for metadata in [json.loads(path.read_text())]
        })
        (out / "report.json").write_text(json.dumps(report, indent=2))
        for process in processes.values():
            stop(process)
        stop(server)
        stop(wake_guard)
    assert report["passed"], report
    print(json.dumps({key: report[key] for key in ["passed", "elapsed_seconds", "accounts", "audit_samples"]}, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--port", type=int, default=0, help="free local UDP port; zero allocates an unused port")
    parser.add_argument("--resolution", default="1024x576")
    parser.add_argument("--peace-seconds", type=int, default=20)
    parser.add_argument("--reconnects", type=int, default=2)
    parser.add_argument("--boat-clearance", type=float, default=6.0)
    args = parser.parse_args()
    try:
        width, height = [int(value) for value in args.resolution.lower().split("x")]
    except ValueError:
        parser.error("resolution must be WIDTHxHEIGHT")
    if width < 1024 or height < 576:
        parser.error("client settings require width >=1024 and height >=576")
    if args.peace_seconds < 10 or not 1 <= args.reconnects <= 5 or args.boat_clearance < 6:
        parser.error("require >=10 peaceful seconds, 1-5 reconnects and >=6m boat clearance")
    run(args)

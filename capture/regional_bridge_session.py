#!/usr/bin/env python3
"""Connected finite bridge construction, pedestrian and under-deck boat proof.

The server stages a paid initial contract only after the real client frames the
site. Afterwards this driver observes; it never moves actors or grants progress.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time

from first_session import distance, environment, stop, wait_server
from multiplayer_session import claim_udp_port, start_client
from startup_session import join, menu, replace
from worker_lifecycle_session import Journal, binary_identity, set_warp

ROOT = Path(__file__).resolve().parents[1]


def xz_distance(a, b):
    return ((a[0] - b[0]) ** 2 + (a[2] - b[2]) ** 2) ** .5


class Acceptance:
    def __init__(self, fixture):
        self.fixture = fixture
        self.previous = None
        self.money = None
        self.pickups = self.deliveries = self.work = 0
        self.walker_deck_samples = self.boat_under_samples = 0
        self.loaded_cart_samples = 0
        self.first_work_world = self.finished_world = None
        self.walked = self.sailed = self.finished = False
        self.events = []

    def observe(self, row):
        f, worker = self.fixture, row["worker"]
        assert row["stage"] == "after_bridge_before_movement", row
        assert worker and worker.get("simulation") == "canonical", "bridge worker left real body simulation"
        if worker['bridge_builder']:
            assert worker['capacity'] == f['cart_capacity'], ('active contract lacks its borrowed cart capacity', row)
            assert worker['cart'] is not None, ('active bridge haul has no visible cart', row)
            if worker['stock']['wood'] + worker['stock']['stone'] > 0 and worker['cart']['load_slots'] > 0:
                self.loaded_cart_samples += 1
        # Every reserved material stays physically at the Hall/source, on the
        # worker, at the bank, or is consumed by the completed deck exactly once.
        for good in ("wood", "stone"):
            actual = sum(row[key][good] for key in ("hall", "source", "site")) + worker["stock"][good]
            actual += f[good] if row["built"] else 0
            assert actual == f[good], ("material lost/created", good, row)
        money = row["treasury"] + row["escrow"] + worker["wallet"]
        if self.money is None:
            self.money = money
        assert money == self.money, ("fixture money including wage escrow changed", row)
        events = []
        if self.previous:
            for good in ("wood", "stone"):
                pickup = self.previous["source"][good] - row["source"][good]
                delivery = row["site"][good] - self.previous["site"][good]
                if pickup > 0:
                    assert xz_distance(worker["position"], f["pickup"]) <= .7, row
                    assert worker["stock"][good] - self.previous["worker"]["stock"][good] == pickup, row
                    self.pickups += 1
                    events.append("pickup")
                if delivery > 0:
                    assert xz_distance(worker["position"], f["deck"]["start"]) <= .7, row
                    assert self.previous["worker"]["stock"][good] - worker["stock"][good] == delivery, row
                    self.deliveries += 1
                    events.append("delivery")
        if worker["activity"] == "Building":
            assert xz_distance(worker["position"], f["deck"]["start"]) <= .7, row
            self.work += 1
            if self.first_work_world is None:
                self.first_work_world = [row['day'], row['world_seconds']]
            events.append("building")
        if row["built"] and not self.finished:
            assert self.pickups == f['expected_pickups'], ('cart failed to reduce full material trips', self.pickups, row)
            assert self.deliveries == self.pickups and self.work > 1, "completion lacks physical delivery/work evidence"
            assert self.loaded_cart_samples >= 2, 'no real loaded-cart presentation samples'
            self.finished_world = [row['day'], row['world_seconds']]
            self.finished = True
            events.append("finished")
        deck = f["deck"]
        dx, dz = deck["end"][0] - deck["start"][0], deck["end"][2] - deck["start"][2]
        length = (dx * dx + dz * dz) ** .5
        for key in ("walker", "boat"):
            actor = row[key]
            if not actor:
                continue
            assert row["built"], "crossing fixture must not authorize an unfinished deck"
            p = actor["position"]
            along = ((p[0] - deck["start"][0]) * dx + (p[2] - deck["start"][2]) * dz) / length
            side = abs((p[0] - deck["start"][0]) * dz - (p[2] - deck["start"][2]) * dx) / length
            if key == "walker":
                assert actor.get("simulation") == "canonical", "walker crossed using abstract strategic travel"
                if deck["ramp_length"] <= along <= length - deck["ramp_length"]:
                    assert side < deck["width"] * .5, row
                    assert abs(p[1] - deck["deck_height"]) < .35, ("walker is swimming or below deck", row)
                    self.walker_deck_samples += 1
                    events.append("walking-deck")
                self.walked |= along >= length + 2.3 and side < deck["width"] * .5
            else:
                if side < deck["width"] * .5 and 0 < along < length:
                    assert p[1] + 4.5 <= deck["deck_height"] - 1.05, ("mast and swell lack under-truss clearance", row)
                    self.boat_under_samples += 1
                    events.append("sailing-under")
                self.sailed |= xz_distance(p, f["boat_end"]) <= .3 and not actor["water_route"]
        self.previous = row
        for event in set(events):
            if event not in {item["event"] for item in self.events}:
                self.events.append({"event": event, "sample": row})
        return events

    def passed(self):
        return (self.finished and self.walked and self.sailed and self.walker_deck_samples >= 2
                and self.boat_under_samples >= 1 and self.previous["escrow"] == 0
                and self.previous["wages_paid"] == self.fixture["wages"]
                and self.previous["project_status"] == "Completed"
                and not self.previous['worker']['bridge_builder']
                and self.previous['worker']['cart'] is None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--server", type=Path, default=ROOT / "target/playtest/server")
    parser.add_argument("--client", type=Path, default=ROOT / "target/playtest/client")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--resolution", default="1600x1000")
    parser.add_argument("--warp", type=int, choices=(1, 25), default=1)
    parser.add_argument("--timeout", type=float, default=1800)
    args = parser.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(ROOT / "logs"):
        raise ValueError("Keep review artifacts under logs/")
    out.mkdir(parents=True, exist_ok=False)
    port = claim_udp_port(args.port)
    if args.client.resolve() != ROOT / "target/playtest/client":
        raise ValueError("The maintained startup helper uses target/playtest/client")
    report = {"passed": False, "scenario": "regional-bridge", "warp": args.warp,
              "binaries": {label: binary_identity(path) for label, path in (("server", args.server), ("client", args.client))},
              "captures": [], "scope": "Controlled paid contract; physical material/work/wage accounting, tactical walker deck crossing and actual boat underpass. Not natural investment selection, an economy soak or performance benchmark."}
    server = client = session = None
    tracker = None
    try:
        env = environment(ROOT) | {"CITYSIM_MAP_ID": "village_lab", "FISTWORLD_BRIDGE_TRACE_DIR": str(out),
            "FISTWORLD_NATURAL_IMMIGRATION": "0", "FISTWORLD_DEV": "1", "FISTWORLD_SERVER_PORT": str(port)}
        # The dedicated fixture supplies its own initial Hall; the ordinary
        # rendered economy-lab stage is intentionally not enabled alongside it.
        env.pop("FISTWORLD_VILLAGE_LAB_RUNTIME", None)
        env.pop("FISTWORLD_REALWORLD_LAB_RUNTIME", None)
        with (out / "server.log").open("w") as log:
            server = subprocess.Popen([args.server.resolve()], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, out / "server.log", lambda text: "Regional bridge fixture ready" in text, "shared river bridge fixture", timeout=180)
        fixture = json.loads((out / "fixture.json").read_text())
        report["fixture"] = fixture
        client, session = start_client(ROOT, out / "client", args.resolution)
        menu(session)
        replace(session, "startup-server-field", f"127.0.0.1:{port}")
        join(session, "BridgeQA")
        session.wait(lambda s: s.get("playing") and s.get("creator") and s.get("god_capability"), "creator and developer controls")
        session.command("button", name="creator-BEGIN JOURNEY")
        session.wait(lambda s: s.get("hero") and not s.get("cinematic") and not s.get("creator"), "ordinary opening finished", timeout=180)
        set_warp(session, args.warp)
        centre = [(a + b) * .5 for a, b in zip(fixture["deck"]["start"], fixture["deck"]["end"])]
        session.command("view", x=centre[0], z=centre[2], zoom=80, yaw=.65)
        session.wait(lambda s: distance(s["camera"]["focus"], centre) < 1, "bridge camera settled")
        report["captures"].append(session.command("capture", name="00-river-before-work"))
        (out / "start").touch()  # Initial fixture admission gate; no worker state is patched.
        journal, tracker = Journal(out / "bridge.jsonl"), Acceptance(fixture)
        deadline = time.monotonic() + args.timeout
        captured = set()
        while time.monotonic() < deadline:
            if any(p.poll() is not None for p in (server, client)):
                raise RuntimeError("Owned game process exited")
            events = []
            for row in journal.read():
                events.extend(tracker.observe(row))
            if tracker.previous:
                # Only current semantic evidence triggers capture. Older JSONL
                # events are retained in the report rather than mislabelled.
                latest = tracker.previous
                label = "building" if latest["worker"]["activity"] == "Building" else None
                if latest["worker"]["stock"]["wood"] + latest["worker"]["stock"]["stone"] > 0:
                    label = "carrying-materials"
                if latest["walker"] and "walking-deck" in events:
                    label = "finished-crossings"
                if (latest["boat"] and xz_distance(latest["boat"]["position"], centre) < 7
                        and "boat-under-bridge" not in captured):
                    label = "boat-under-bridge"
                if label and label not in captured:
                    captured.add(label)
                    report["captures"].append({"name": label, "trigger": latest,
                        "reply": session.command("record", name=label,
                            frames=16 if label == "boat-under-bridge" else 8, interval_ms=120, timeout=90)})
            if tracker.passed():
                report["captures"].append(session.command("capture", name="99-completed-bridge"))
                for name, yaw in (("100-bridge-side", 2.0), ("101-opposite-bank", 3.8)):
                    session.command("view", x=centre[0], z=centre[2], zoom=58, yaw=yaw)
                    session.wait(lambda s: abs(s["camera"]["zoom"] - 58) < .1
                                 and abs(s["camera"]["yaw"] - yaw) < .01,
                                 "finished bridge inspection angle")
                    report["captures"].append(session.command("capture", name=name))
                report["passed"] = True
                break
            time.sleep(.05)
        if not report["passed"]:
            raise TimeoutError(f"Bridge acceptance incomplete: {tracker.__dict__}")
        print("CONNECTED BRIDGE ACCEPTANCE PASSED; inspect PNGs and capture JSONs", flush=True)
    except BaseException as error:
        report["error"] = repr(error)
        raise
    finally:
        if tracker:
            report["evidence"] = {k: v for k, v in tracker.__dict__.items() if k != "fixture"}
        if session:
            report["last_client_state"] = session.status()
        (out / "report.json").write_text(json.dumps(report, indent=2))
        stop(client)
        stop(server)


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
"""Drive the opt-in real client session harness and retain semantic evidence.

The game still creates the character and applies ordinary UI/input handlers;
this tool never grants inventory, changes prices, or moves an actor directly.
"""

import argparse
import json
import math
import os
from pathlib import Path
import shutil
import subprocess
import time


class Session:
    def __init__(self, directory):
        self.directory = Path(directory).resolve()
        self.directory.mkdir(parents=True, exist_ok=True)
        replies = [int(p.stem.split("-")[1]) for p in self.directory.glob("reply-*.json")]
        self.next_id = max(replies, default=0) + 1

    def status(self):
        try:
            return json.loads((self.directory / "status.json").read_text())
        except FileNotFoundError:
            return {}

    def wait(self, predicate, description, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            value = self.status()
            if value and predicate(value):
                return value
            time.sleep(0.1)
        raise TimeoutError(f"{description}: last replicated state {self.status()}")

    def command(self, action, *, expect_ok=True, timeout=100, **parameters):
        request_id = self.next_id
        self.next_id += 1
        temporary = self.directory / "command.tmp"
        temporary.write_text(json.dumps({"id": request_id, "command": {"action": action, **parameters}}))
        temporary.replace(self.directory / "command.json")
        reply_path = self.directory / f"reply-{request_id}.json"
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if reply_path.exists():
                reply = json.loads(reply_path.read_text())
                if reply["ok"] != expect_ok:
                    raise AssertionError(reply)
                print(f"{request_id}: {action} {parameters}: {'ok' if reply['ok'] else reply['error']}", flush=True)
                return reply
            time.sleep(0.1)
        raise TimeoutError(f"No reply to {action} {parameters}")


def distance(a, b):
    return math.hypot(a[0] - b[0], a[2] - b[2])


def arrive(session, destination, description, timeout, capture_prefix=None):
    """Retain movement evidence and fail an accepted but motionless order early."""
    deadline = time.monotonic() + timeout
    last_progress = time.monotonic()
    last_position = session.status()["hero"]["position"]
    next_sample = time.monotonic()
    next_capture = time.monotonic() + 30
    capture_index = 0
    while time.monotonic() < deadline:
        state = session.status()
        hero = state.get("hero")
        if hero:
            if time.monotonic() >= next_sample:
                with (session.directory / "movement.jsonl").open("a") as journal:
                    journal.write(json.dumps({"time": time.time(), "journey": description,
                                              "destination": destination, "hero": hero}) + "\n")
                next_sample = time.monotonic() + 5
            if distance(hero["position"], destination) < 10 and not hero["aboard"]:
                return state
            if distance(hero["position"], last_position) >= 1:
                last_position = hero["position"]
                last_progress = time.monotonic()
            if capture_prefix and time.monotonic() >= next_capture:
                capture_index += 1
                session.command("key", key="Home")
                session.command("capture", name=f"{capture_prefix}-moving-{capture_index:02}")
                next_capture = time.monotonic() + 120
        if time.monotonic() - last_progress > 90:
            raise TimeoutError(f"No progress on {description}: hero={hero}, notice={state.get('notice')}, selection={state.get('selection')}")
        time.sleep(0.1)
    raise TimeoutError(f"{description}: exceeded travel budget, hero={session.status().get('hero')}")


def enabled(state, name):
    return any(button["name"] == name and button["enabled"] for button in state["buttons"])


def environment(root):
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("FISTWORLD_", "FISTFORCE_", "CITYSIM_"))}
    env.update(BEVY_ASSET_ROOT=str(root / "client/assets"), RUST_LOG="info")
    return env


def stop(process):
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def start_client(root, out, account, resolution):
    out.mkdir(parents=True, exist_ok=True)
    env = environment(root)
    env.update(FISTFORCE_NO_SETTINGS_FILE="1", FISTFORCE_AUTOCONNECT=account,
               FISTFORCE_DISPLAY_MODE="windowed", FISTFORCE_FULLSCREEN="0",
               FISTFORCE_RESOLUTION=resolution, FISTFORCE_RENDER_SCALE="1",
               FISTFORCE_CLIENT_PERF="1", FISTWORLD_SESSION_CAPTURE_DIR=str(out))
    with (out / "client.log").open("w") as log:
        process = subprocess.Popen([root / "target/playtest/client"], env=env,
                                   stdout=log, stderr=subprocess.STDOUT)
    return process, Session(out)


def wait_server(process, log, predicate, description, timeout=180):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        text = log.read_text()
        if process.poll() is not None:
            raise RuntimeError(f"Server exited: {text[-4000:]}")
        if predicate(text):
            return
        time.sleep(0.2)
    raise TimeoutError(description)


def close_book(session):
    for _ in range(4):
        if not session.status().get("ui_blocking"):
            return
        session.command("key", key="Escape")
        # Wait for the next published UI state, not a fixed animation delay.
        stamp = (session.directory / "status.json").stat().st_mtime_ns
        session.wait(lambda _: (session.directory / "status.json").stat().st_mtime_ns != stamp,
                     "updated UI state")
    assert not session.status()["ui_blocking"], "book did not close"


def trade(session, good, kind):
    before = session.status()["hero"]
    session.command("button", name=f"market-{kind}-{good}")
    expected = before["cargo"][good] + (1 if kind == "Buy" else -1)
    after = session.wait(lambda state: state["hero"]["cargo"][good] == expected,
                         f"authoritative {kind} of {good}")["hero"]
    return before, after


def town_market(state, town_id):
    return next((market for market in state["markets"] if market["id"] == town_id), None)


def assert_retained_claim(before, resumed, town_id):
    """The one listed unit either remains ours or has paid its exact net price."""
    original = town_market(before, town_id)
    current = town_market(resumed, town_id)
    assert original is not None and current is not None
    offers = original["my_offers"]
    remaining = current["my_offers"]
    assert sum(offer["units"] for offer in offers) <= 1
    expected_wallet = before["hero"]["wallet"]
    for offer in offers:
        kept = sum(other["units"] for other in remaining
                   if other["good"] == offer["good"] and other["unit_price"] == offer["unit_price"])
        assert 0 <= kept <= offer["units"]
        gross = (offer["units"] - kept) * offer["unit_price"]
        fee = min(gross, (gross * original["fee_bps"] + 9999) // 10000)
        expected_wallet += gross - fee
    assert all(any(offer["good"] == other["good"] and offer["unit_price"] == other["unit_price"]
                   for offer in offers) for other in remaining), "reconnect created a new claim"
    assert resumed["hero"]["wallet"] == expected_wallet, (expected_wallet, resumed["hero"]["wallet"])


def run(args):
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError(f"Use a fresh output directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    if subprocess.run(["lsof", "-nP", "-iUDP:5000"], stdout=subprocess.DEVNULL).returncode == 0:
        raise RuntimeError("UDP 5000 is occupied; this runner never stops another server")
    env = environment(root)
    env.update(FISTWORLD_WORLD_SEED=str(args.seed), FISTWORLD_DEV="0", FISTFORCE_SERVER_PERF="1")
    server_log = out / "server.log"
    with server_log.open("w") as log:
        server = subprocess.Popen([root / "target/playtest/server"], env=env,
                                  stdout=log, stderr=subprocess.STDOUT)
    client = None
    wake_guard = None
    report = {"seed": args.seed, "resolution": args.resolution, "passed": False,
              "working_tree_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=root, text=True)),
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()}
    try:
        # On macOS, keep an unattended test awake while connected to AC.
        # The assertion belongs to this server process and changes no saved
        # power preferences. Battery exhaustion can still suspend the host.
        if caffeinate := shutil.which("caffeinate"):
            wake_guard = subprocess.Popen([caffeinate, "-s", "-w", str(server.pid)],
                                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_server(server, server_log, lambda text: "Server UDP socket bound to 0.0.0.0:5000" in text,
                    "new world socket readiness")
        client, session = start_client(root, out / "arrival", "SessionQA", args.resolution)
        (out / "processes.json").write_text(json.dumps({"server": server.pid, "client": client.pid}))
        initial = session.wait(lambda state: state["playing"] and state["creator"], "normal character creator")
        assert not initial["god"] and initial["hero"] is None
        session.command("capture", name="01-creator")
        session.command("button", name="creator-BEGIN JOURNEY")
        opening = session.wait(lambda state: state["hero"] is not None and state["hero"]["aboard"] and not state["cinematic"], "opening voyage")
        assert opening["hero"]["wallet"] == 2000 and opening["hero"]["bulk"] == 0
        session.command("capture", name="02-aboard")
        town = min(opening["towns"], key=lambda town: distance(town["position"], opening["hero"]["position"]))
        report["arrival_town"] = town["name"]
        session.command("key", key="Home")
        session.command("button", name="journey-NOTICES")
        session.command("button", name="hud-VIEW TOWN")
        session.command("button", name="journey-NOTICES")
        focused = session.wait(lambda state: any(market["id"] == town["id"] for market in state["markets"]), "town replication")
        market = next(market for market in focused["markets"] if market["id"] == town["id"])
        entrance = market["entrance"]
        session.command("right_click", x=entrance[0], z=entrance[2])
        landed = arrive(session, entrance, "sail, land and walk to Hall", timeout=300)
        report["hero_id"] = landed["hero"]["person_id"]
        session.command("key", key="Home")
        session.command("capture", name="03-arrived")
        session.command("key", key="E")
        state = session.wait(lambda state: any(button["name"].startswith("market-Buy-") for button in state["buttons"]), "market UI")
        good = next((good for good in ["Wheat", "Flour", "Wood", "Stone"] if enabled(state, f"market-Buy-{good}")), None)
        assert good, "No eligible nonperishable stock; inspect the ordinary economy"
        report["trade_good"] = good
        trade(session, good, "Buy")
        trade(session, good, "Buy")
        trade(session, good, "Offer")
        session.command("capture", name="04-first-trade")
        before = session.status()
        assert before["hero"]["cargo"][good] == 1
        report["before_reconnect"] = before["hero"]
        log_offset = server_log.stat().st_size
        session.command("quit")
        client.wait(timeout=15)
        wait_server(server, server_log, lambda text: "Freed up name 'sessionqa'" in text[log_offset:], "account disconnect", timeout=30)
        client, session = start_client(root, out / "reconnect", "SessionQA", args.resolution)
        (out / "processes.json").write_text(json.dumps({"server": server.pid, "client": client.pid}))
        resumed = session.wait(lambda state: state["playing"] and state["hero"] is not None, "hero readoption")
        assert not resumed["creator"] and not resumed["cinematic"] and not resumed["god"]
        assert resumed["hero"]["person_id"] == before["hero"]["person_id"]
        assert resumed["hero"]["cargo"] == before["hero"]["cargo"]
        session.command("key", key="Home")
        resumed = session.wait(lambda state: town_market(state, town["id"]) is not None, "retained market claims")
        assert_retained_claim(before, resumed, town["id"])
        report["after_reconnect"] = resumed["hero"]
        session.command("capture", name="05-reconnected")
        session.command("key", key="E")
        session.wait(lambda state: enabled(state, f"market-Offer-{good}"), "reconnected market")
        session.command("capture", name="06-owned-offer")

        # Buy/post actual finite goods to exercise space and affordability. No
        # artificial money, refills, price changes or automatic profit.
        full_seen = False
        for _ in range(8):
            for _ in range(20):
                state = session.status()
                if not enabled(state, f"market-Buy-{good}"):
                    break
                trade(session, good, "Buy")
            state = session.status()
            if state["hero"]["bulk"] >= state["hero"]["capacity"]:
                if not full_seen:
                    session.command("button", name=f"market-Buy-{good}", expect_ok=False)
                    session.command("capture", name="07-cargo-full")
                    full_seen = True
                while session.status()["hero"]["cargo"][good] > 1:
                    trade(session, good, "Offer")
            else:
                session.command("button", name=f"market-Buy-{good}", expect_ok=False)
                session.command("capture", name="08-buy-unavailable")
                break
        report["full_cargo_checked"] = full_seen
        report["after_trade_limits"] = session.status()["hero"]

        close_book(session)
        session.command("right_click", x=entrance[0], z=entrance[2] - 40)
        session.wait(lambda state: distance(state["hero"]["position"], entrance) > 30, "walk away from counter")
        session.command("key", key="N")
        session.command("button", name="open-town-market")
        session.wait(lambda state: any(button["name"] == f"market-Buy-{good}" and not button["enabled"] for button in state["buttons"]), "remote market disabled")
        session.command("capture", name="09-out-of-range")
        close_book(session)

        # A sustained connected session traverses the existing town network.
        # Every destination is an ordinary RMB order; path failures remain
        # visible in the report instead of teleporting around them.
        start = time.monotonic()
        journeys = []
        report["town_visits"] = journeys
        current_town = town
        while time.monotonic() - start < args.soak_seconds:
            state = session.status()
            next_town = min((candidate for candidate in state["towns"] if candidate["id"] != current_town["id"]),
                            key=lambda candidate: distance(candidate["position"], state["hero"]["position"]))
            position = next_town["position"]
            session.command("view", x=position[0], z=position[2], zoom=180)
            state = session.wait(lambda state: any(market["id"] == next_town["id"] for market in state["markets"]), "next town details")
            next_market = next(market for market in state["markets"] if market["id"] == next_town["id"])
            destination = next_market["entrance"]
            session.command("right_click", x=destination[0], z=destination[2])
            # Seeded towns can be kilometres apart. Budget a conservative
            # walking route with detours, rather than treating a long valid
            # journey as a stuck actor after a fixed short timeout.
            travel_budget = max(420, distance(state["hero"]["position"], destination) / 1.5 + 120)
            arrive(session, destination, f"travel to {next_town['name']}", timeout=travel_budget,
                   capture_prefix=f"travel-{len(journeys)+1:02}")
            journeys.append(next_town["name"])
            session.command("key", key="Home")
            session.command("capture", name=f"travel-{len(journeys):02}")
            current_town = next_town
            report["sustained_connected_seconds"] = time.monotonic() - start
            (out / "report.json").write_text(json.dumps(report, indent=2))
            print(f"Sustained play {time.monotonic()-start:.0f}s: arrived at {current_town['name']}", flush=True)
        report["sustained_connected_seconds"] = time.monotonic() - start
        report["town_visits"] = journeys
        report["passed"] = True
        session.command("quit")
        client.wait(timeout=15)
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        (out / "report.json").write_text(json.dumps(report, indent=2))
        stop(client)
        stop(server)
        stop(wake_guard)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    tasks = parser.add_subparsers(dest="task", required=True)
    single = tasks.add_parser("command")
    single.add_argument("directory", type=Path)
    single.add_argument("command", help='JSON, e.g. {"action":"key","key":"Home"}')
    play = tasks.add_parser("run")
    play.add_argument("--seed", type=int, required=True)
    play.add_argument("--out", type=Path, required=True)
    play.add_argument("--resolution", default="1600x1000")
    play.add_argument("--soak-seconds", type=int, default=0)
    args = parser.parse_args()
    if args.task == "run":
        run(args)
        return
    command = json.loads(args.command)
    result = Session(args.directory).command(**command)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

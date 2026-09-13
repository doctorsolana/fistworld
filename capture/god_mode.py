#!/usr/bin/env python3
"""Verify God-mode exits against a real accelerated local server and Bevy UI.

Developer capability is granted by the isolated server. Production keyboard and
button handlers change mode and send time-warp intent; snapshots and composed
captures observe the resulting client/server state without mutating either.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import time

from first_session import enabled, environment, stop, wait_server
from multiplayer_session import claim_udp_port
from startup_session import join, menu, replace, start_client


def mode_ready(state, mode, console=False):
    return (state.get("playing") and state.get("hud_mode") == mode
            and state.get("debug_menu_open") is console
            and state.get("ui_blocking") is console
            and enabled(state, "god-return-to-play" if console else "hud-mode-toggle"))


def exit_mode(session, action, name):
    started = time.monotonic()
    if action == "key":
        session.command("key", key="G")
    else:
        session.command("button", name="god-return-to-play")
    state = session.wait(lambda value: mode_ready(value, "Play"),
                         "Play mode and restored HUD without reconnect", timeout=20)
    assert state["god_capability"], "exiting tools must not revoke the server grant"
    assert state["time_warp"] == 100, "local UI exit must not silently alter shared server time"
    elapsed = time.monotonic() - started
    session.command("capture", name=name)
    return {"latency_seconds": elapsed, "state": state}


def run(args):
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError(f"Use a fresh output directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    port = claim_udp_port(args.port)
    report = {"passed": False, "seed": args.seed, "resolution": args.resolution, "port": port,
              "driver": "real server grant, keyboard events, production UI buttons and replicated time",
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()}
    env = environment(root)
    env.update(FISTWORLD_WORLD_SEED=str(args.seed), FISTWORLD_DEV="1", FISTWORLD_SERVER_PORT=str(port))
    server = client = wake_guard = None
    started = time.monotonic()
    try:
        server_log = out / "server.log"
        with server_log.open("w") as log:
            server = subprocess.Popen([root / "target/playtest/server"], cwd=root, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        if caffeinate := shutil.which("caffeinate"):
            wake_guard = subprocess.Popen([caffeinate, "-s", "-w", str(server.pid)],
                                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_server(server, server_log,
                    lambda text: f"Server UDP socket bound to 0.0.0.0:{port}" in text,
                    "ordinary seeded world readiness")
        client, session = start_client(root, out / "client", args.resolution)
        (out / "processes.json").write_text(json.dumps({"server": server.pid, "client": client.pid}))
        menu(session)
        replace(session, "startup-server-field", f"127.0.0.1:{port}")
        join(session, "GodExitQA")
        session.wait(lambda state: state.get("playing") and state.get("creator")
                     and state.get("god_capability"), "creator and real server developer grant")
        session.command("button", name="creator-BEGIN JOURNEY")
        initial = session.wait(lambda state: state.get("hero") and not state.get("creator")
                               and not state.get("cinematic"), "normal created hero and completed opening")
        report["hero_id"] = initial["hero"]["person_id"]
        session.command("key", key="G")
        session.wait(lambda state: mode_ready(state, "God"), "God HUD")
        session.command("button", name="god-warp-100")
        accelerated = session.wait(lambda state: state.get("time_warp") == 100,
                                   "authoritative 100x time warp")
        session.command("capture", name="01-god-accelerated")
        session.command("key", key="J")
        session.wait(lambda state: mode_ready(state, "God", True), "ready time console")
        session.command("capture", name="02-time-console")
        report["keyboard_exit"] = exit_mode(session, "key", "03-play-after-key")
        # Re-enter with the mouse, and leave the console with its visible button.
        session.command("button", name="hud-mode-toggle")
        session.wait(lambda state: mode_ready(state, "God"), "God mode from HUD button")
        session.command("key", key="J")
        session.wait(lambda state: mode_ready(state, "God", True), "reopened time console")
        session.command("record", name="04-console-accelerated", frames=8, interval_ms=200)
        report["button_exit"] = exit_mode(session, "button", "05-play-after-button")
        final = report["button_exit"]["state"]
        assert final["account"] == "GodExitQA"
        assert final["hero"] and final["hero"]["person_id"] == report["hero_id"]
        assert final["clock"] != accelerated["clock"], "server clock must continue during the exits"
        session.command("key", key="G")
        session.wait(lambda state: mode_ready(state, "God"), "God HUD for deliberate clock restore")
        session.command("button", name="god-warp-1")
        session.wait(lambda state: state.get("time_warp") == 1, "authoritative normal time restored")
        session.command("key", key="G")
        session.wait(lambda state: mode_ready(state, "Play"), "final Play mode")
        report["passed"] = True
    except Exception as error:
        report["error"] = str(error)
        raise
    finally:
        report["elapsed_seconds"] = time.monotonic() - started
        (out / "report.json").write_text(json.dumps(report, indent=2))
        stop(client)
        stop(server)
        stop(wake_guard)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--seed", type=int, default=4800834198907808058)
    parser.add_argument("--resolution", default="1600x1000")
    parser.add_argument("--port", type=int, default=0, help="Unused local UDP port; 0 chooses one")
    run(parser.parse_args())

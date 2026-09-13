#!/usr/bin/env python3
"""Exercise real FistWorld startup controls, validation, creation and reconnect.

Uses first_session's isolated process/session helpers. Logical keyboard events
and named native buttons drive production handlers; there are no fabricated
connections, account drafts, world recipes or heroes. Offline art/animation
captures remain a separate presentation suite.
"""

import argparse
import json
from pathlib import Path
import subprocess
import time

from first_session import Session, environment, stop, wait_server, enabled


def start_client(root, out, resolution):
    out.mkdir(parents=True, exist_ok=True)
    env = environment(root)
    env.update(FISTFORCE_NO_SETTINGS_FILE="1", FISTFORCE_DISPLAY_MODE="windowed",
               FISTFORCE_FULLSCREEN="0", FISTFORCE_RESOLUTION=resolution,
               FISTFORCE_RENDER_SCALE="1", FISTWORLD_SESSION_CAPTURE_DIR=str(out))
    with (out / "client.log").open("w") as log:
        process = subprocess.Popen([root / "target/playtest/client"], cwd=root,
                                   env=env, stdout=log, stderr=subprocess.STDOUT)
    return process, Session(out)


def replace(session, field, text):
    session.command("button", name=field)
    session.command("text", text=text, replace=True)


def menu(session):
    return session.wait(lambda state: state.get("game_state") == "MainMenu"
                        and state.get("startup_art_ready") and enabled(state, "startup-primary"),
                        "ready real launcher", timeout=60)


def join(session, name):
    session.command("button", name="startup-primary")
    session.wait(lambda state: state.get("game_state") == "Connected", "connected name form", timeout=35)
    replace(session, "startup-name-field", name)
    session.command("key", key="Enter")


def key_state(session, key):
    return session.command("key", key=key)["state"]


def choose_local_with_keyboard(session):
    session.command("button", name="startup-server-field")
    assert key_state(session, "Tab")["focused_control"] == "startup-presets"
    assert key_state(session, "Enter")["presets_expanded"]
    assert key_state(session, "Tab")["focused_control"] == "startup-preset-0"
    chosen = key_state(session, "Enter")
    assert chosen["game_state"] == "MainMenu" and not chosen["presets_expanded"]
    assert chosen["focused_control"] == "startup-server-field"
    assert chosen["server_address"] == {"host": "127.0.0.1", "port": 5000}
    return chosen


def run(args):
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError(f"Use a fresh output directory: {out}")
    out.mkdir(parents=True, exist_ok=True)
    if subprocess.run(["lsof", "-nP", "-iUDP:5000"], stdout=subprocess.DEVNULL).returncode == 0:
        raise RuntimeError("UDP 5000 is occupied; this runner never stops another server")
    report = {"passed": False, "seed": args.seed, "account": "StartupQA",
              "driver": "real keyboard events and production button handlers; no autoconnect or state overrides",
              "revision": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()}
    env = environment(root)
    env.update(FISTWORLD_WORLD_SEED=str(args.seed), FISTWORLD_DEV="0")
    server_log = out / "server.log"
    server = client = None
    started = time.monotonic()
    try:
        with server_log.open("w") as log:
            server = subprocess.Popen([root / "target/playtest/server"], cwd=root,
                                      env=env, stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, server_log,
                    lambda text: "Server UDP socket bound to 0.0.0.0:5000" in text,
                    "ordinary seeded world readiness")
        client, session = start_client(root, out / "client", args.resolution)
        (out / "processes.json").write_text(json.dumps({"server": server.pid, "client": client.pid}))
        launcher = menu(session)
        report["window_title"] = launcher["window_title"]
        assert launcher["window_title"] == "FistWorld"
        session.command("capture", name="01-launcher")
        session.command("button", name="startup-server-field")
        assert key_state(session, "Tab")["focused_control"] == "startup-presets"
        assert key_state(session, "Tab")["focused_control"] == "startup-primary"
        assert key_state(session, "Tab")["focused_control"] == "startup-secondary"
        assert key_state(session, "Tab")["focused_control"] == "startup-server-field"
        choose_local_with_keyboard(session)
        report["keyboard_closed_presets_skipped"] = True
        report["keyboard_preset_selection"] = True
        replace(session, "startup-server-field", "127.0.0.1:6000")
        key_state(session, "Enter")
        connecting = session.wait(lambda state: state.get("game_state") == "Connecting", "real pending connection")
        assert connecting["server_address"]["port"] == 6000
        assert key_state(session, "Tab")["focused_control"] == "startup-secondary"
        key_state(session, "Enter")
        cancelled = menu(session)
        assert cancelled["connection_error"] is None
        choose_local_with_keyboard(session)
        report["keyboard_connecting_cancel"] = True
        report["preset_resets_custom_port"] = True
        replace(session, "startup-server-field", "127.0.0.1:0")
        session.command("key", key="Enter")
        invalid_port = session.wait(lambda state: state.get("game_state") == "MainMenu"
                                    and "port" in (state.get("connection_error") or ""), "invalid port feedback")
        report["invalid_port"] = invalid_port["connection_error"]
        replace(session, "startup-server-field", "firstworld-startup-test.invalid:5000")
        session.command("key", key="Enter")
        failed_dns = session.wait(lambda state: state.get("game_state") == "MainMenu"
                                  and bool(state.get("connection_error"))
                                  and "port" not in state["connection_error"], "failed DNS recovery", timeout=35)
        assert "port" not in failed_dns["connection_error"]
        report["failed_dns"] = failed_dns["connection_error"]
        session.command("capture", name="02-connection-error")
        replace(session, "startup-server-field", "127.0.0.1:5000")
        session.command("key", key="Enter")
        session.wait(lambda state: state.get("game_state") == "Connected", "first name form")
        session.command("key", key="Escape")
        returned = menu(session)
        assert not returned["submitted"] and not returned["connection_error"]
        report["back_from_name"] = True
        session.command("button", name="startup-primary")
        session.wait(lambda state: state.get("game_state") == "Connected", "name form for busy cancellation")
        replace(session, "startup-name-field", "CancelQA")
        submitting = key_state(session, "Enter")
        assert submitting["game_state"] == "Connected" and submitting["name_phase"] in ("Submitting", "Preparing")
        busy = key_state(session, "Tab")
        assert busy["game_state"] == "Connected" and busy["name_phase"] in ("Submitting", "Preparing")
        assert busy["focused_control"] == "startup-secondary"
        key_state(session, "Enter")
        cancelled = menu(session)
        assert not cancelled["submitted"] and cancelled["connection_error"] is None
        report["keyboard_name_busy_cancel"] = {"phase": busy["name_phase"], "focus": busy["focused_control"]}
        session.command("button", name="startup-primary")
        session.wait(lambda state: state.get("game_state") == "Connected", "name form after Back")
        replace(session, "startup-name-field", "WWWWWWWWWWWWWWWW")
        wide = session.wait(lambda state: state.get("account") == "WWWWWWWWWWWWWWWW", "wide sixteen-letter draft")
        assert wide["name_phase"] == "Editing" and not wide["submitted"]
        session.command("capture", name="03-wide-name")
        replace(session, "startup-name-field", "Al")
        session.command("key", key="Enter")
        short = session.wait(lambda state: "short" in (state.get("name_error") or ""), "short-name feedback")
        assert short["name_phase"] == "Editing" and not short["submitted"]
        session.command("capture", name="03-short-name")
        replace(session, "startup-name-field", "admin")
        session.command("key", key="Enter")
        rejected = session.wait(lambda state: "reserved" in (state.get("name_error") or ""), "authoritative reserved-name rejection")
        assert rejected["name_phase"] == "Editing" and not rejected["submitted"] and rejected["account"] == "admin"
        report["server_rejection"] = rejected["name_error"]
        session.command("capture", name="04-reserved-name")
        replace(session, "startup-name-field", report["account"])
        session.command("key", key="Enter")
        creator = session.wait(lambda state: state.get("playing") and state.get("creator"), "normal new-account creator", timeout=90)
        assert creator["hero"] is None and not creator["god"] and creator["account"] == report["account"]
        session.command("capture", name="05-creator")
        session.command("button", name="creator-BEGIN JOURNEY")
        opening = session.wait(lambda state: state.get("hero") is not None and not state.get("cinematic"), "real created hero after cinematic", timeout=70)
        assert not opening["creator"] and not opening["god"]
        report["before_reconnect"] = opening["hero"]
        session.command("capture", name="06-hero")
        session.command("key", key="Escape")
        session.wait(lambda state: enabled(state, "pause-DISCONNECT"), "pause disconnect control")
        offset = server_log.stat().st_size
        session.command("button", name="pause-DISCONNECT")
        menu(session)
        wait_server(server, server_log, lambda text: "Freed up name 'startupqa'" in text[offset:],
                    "server account disconnect", timeout=30)
        join(session, "sTaRtUpQA")
        resumed = session.wait(lambda state: state.get("playing") and state.get("hero") is not None,
                               "case-insensitive existing hero readoption", timeout=70)
        assert not resumed["creator"] and not resumed["cinematic"] and not resumed["god"]
        assert resumed["hero"]["person_id"] == opening["hero"]["person_id"]
        assert resumed["hero"]["wallet"] == opening["hero"]["wallet"]
        assert resumed["hero"]["cargo"] == opening["hero"]["cargo"]
        report["after_reconnect"] = resumed["hero"]
        session.command("capture", name="07-reconnected")
        phases = [json.loads(line)["state"] for line in (session.directory / "startup.transitions.jsonl").read_text().splitlines()]
        assert any(state["game_state"] == "Connected" and state["name_phase"] == "Submitting" for state in phases)
        assert any(state["game_state"] == "Connected" and state["name_phase"] == "Preparing" for state in phases)
        report["observed_phases"] = [{"game_state": state["game_state"], "name_phase": state["name_phase"]} for state in phases]
        report["passed"] = True
        session.command("quit")
        client.wait(timeout=15)
    finally:
        stop(client)
        stop(server)
        report["elapsed_seconds"] = round(time.monotonic() - started, 2)
        (out / "report.json").write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--seed", type=int, default=12345)
    parser.add_argument("--resolution", default="1600x1000")
    run(parser.parse_args())

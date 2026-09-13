#!/usr/bin/env python3
"""Observe ordinary generated-town NPCs through the real client/server.

No actor placement, resource grants, clock changes or NPC orders. Optional read-only
traces correlate authoritative targets with rendered positions and leg animation.
All review artifacts remain under logs/. Build the workspace before running.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time
import first_session as harness
from npc_movement_analysis import analyze

ROOT = Path(__file__).resolve().parents[1]


def last_sample(path):
    if not path.exists():
        return {}
    with path.open('rb') as stream:
        stream.seek(max(0, path.stat().st_size - 2_000_000))
        lines = stream.read().splitlines()
    for line in reversed(lines):
        try:
            return json.loads(line)
        except json.JSONDecodeError:
            continue
    return {}


def run(args):
    out = Path(args.out).resolve()
    if not out.is_relative_to(ROOT / 'logs') or (out.exists() and any(out.iterdir())):
        raise ValueError('Use a fresh output directory under logs/')
    out.mkdir(parents=True)
    if subprocess.run(['lsof', '-nP', '-iUDP:5000'], capture_output=True).returncode == 0:
        raise RuntimeError('Existing server left alone; UDP 5000 must be free')
    server = client = None
    report = {'seed': args.seed, 'passed': False, 'scope': 'Normal generated world at 1x; read-only camera observation', 'captures': []}
    try:
        env = harness.environment(ROOT) | {'FISTWORLD_WORLD_SEED': str(args.seed), 'FISTWORLD_DEV': '0',
            'FISTWORLD_STUCK_WATCH': '1', 'FISTWORLD_LAB_ROUTE_DIAGNOSTICS': '1',
            'FISTFORCE_SERVER_PERF': '1', 'FISTWORLD_MOVEMENT_TRACE_DIR': str(out)}
        with (out / 'server.log').open('w') as log:
            server = subprocess.Popen([ROOT / 'target/playtest/server'], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        harness.wait_server(server, out / 'server.log', lambda s: 'Server UDP socket bound' in s, 'world ready', 600)
        session_dir = out / 'client'
        session_dir.mkdir()
        env = harness.environment(ROOT) | {'FISTFORCE_NO_SETTINGS_FILE': '1', 'FISTFORCE_AUTOCONNECT': '1',
            'FISTFORCE_DISPLAY_MODE': 'windowed', 'FISTFORCE_FULLSCREEN': '0', 'FISTFORCE_RESOLUTION': args.resolution,
            'FISTFORCE_RENDER_SCALE': '1', 'FISTFORCE_FRAME_CAP': '60', 'FISTWORLD_SESSION_CAPTURE_DIR': str(session_dir),
            'FISTWORLD_MOVEMENT_TRACE_DIR': str(out)}
        with (out / 'client.log').open('w') as log:
            client = subprocess.Popen([ROOT / 'target/playtest/client'], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        (out / 'processes.json').write_text(json.dumps({'server': server.pid, 'client': client.pid}))
        session = harness.Session(session_dir)
        state = session.wait(lambda s: s.get('playing') and (s.get('creator') or s.get('hero')), 'ordinary startup', 240)
        if state.get('creator'):
            session.command('button', name='creator-BEGIN JOURNEY')
        state = session.wait(lambda s: s.get('hero') and not s.get('cinematic') and len(s.get('towns', [])) >= 10, 'hero and world directory', 180)
        harness.close_book(session)
        towns = sorted(state['towns'], key=lambda t: (-t['residents'], t['id']))
        town = towns[args.town_index]
        report['town'] = town
        x, _, z = town['position']
        session.command('view', x=x, z=z, zoom=105)
        session.wait(lambda s: any(m['id'] == town['id'] for m in s.get('markets', [])), 'replicated town', 180)
        session.command('capture', name='01-town', timeout=180)
        report['captures'].append('01-town')
        print('OBSERVING', town, flush=True)
        start = time.monotonic()
        recorded_porters = set()
        while time.monotonic() - start < args.seconds:
            if (out / 'stop').exists():
                break
            if client.poll() is not None or server.poll() is not None:
                raise RuntimeError('Owned game process exited')
            if len(recorded_porters) < args.porter_clips:
                sample = last_sample(out / 'client-movement.jsonl')
                candidates = [a for a in sample.get('actors', []) if a['cart'] and not a['culled']
                    and a['id'] not in recorded_porters and a['visual_speed'] > .6]
                if candidates:
                    actor = candidates[0]
                    recorded_porters.add(actor['id'])
                    px, _, pz = actor['position']
                    session.command('view', x=px, z=pz, zoom=18)
                    name = f'porter-{actor["id"]}'
                    session.command('record', name=name, frames=36, interval_ms=100)
                    report['captures'].append(name)
                    session.command('view', x=x, z=z, zoom=105)
            time.sleep(.25)
        session.command('record', name='02-town-motion', frames=40, interval_ms=100)
        report['captures'].append('02-town-motion')
        report['observed_seconds'] = time.monotonic() - start
        report['recorded_porters'] = sorted(recorded_porters)
        report['passed'] = True
    finally:
        (out / 'report.json').write_text(json.dumps(report, indent=2))
        harness.stop(client)
        harness.stop(server)
    analysis = analyze(out)
    report['passed'] = analysis['passed']
    report['analysis'] = str(out / 'movement-analysis.json')
    (out / 'report.json').write_text(json.dumps(report, indent=2))
    if not args.observe_only and not analysis['passed']:
        raise AssertionError(f'Movement/animation checks need attention: {out / "movement-analysis.json"}')
    print('Observation complete. Inspect real PNGs and their .capture.json plus both movement traces.', flush=True)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', required=True)
    parser.add_argument('--seed', type=int, default=4800834198907808058)
    parser.add_argument('--seconds', type=float, default=360)
    parser.add_argument('--town-index', type=int, default=0)
    parser.add_argument('--porter-clips', type=int, default=3)
    parser.add_argument('--resolution', default='1280x800')
    parser.add_argument('--observe-only', action='store_true', help='Retain failing baseline evidence without failing the runner')
    run(parser.parse_args())

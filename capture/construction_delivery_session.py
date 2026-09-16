"""Observe ordinary self-supplied builders, without timber or ownership grants."""
import argparse
import json
import subprocess
import time
from pathlib import Path

from first_session import Session, environment, wait_server, stop
from multiplayer_session import claim_udp_port
from startup_session import join, menu, replace
from construction_meals import (
    ConstructionMeals, ContinuousSnapshots, FOOD_OBJECTIVES, completed_captures, distance,
)
from worker_lifecycle_session import Journal, binary_identity, set_warp


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path,
                        default=Path('logs/construction-delivery') / time.strftime('%Y%m%d-%H%M%S'))
    parser.add_argument('--observe-meals', action='store_true',
                        help='also require the SAME self-builder to finish a food trip and resume at its site')
    parser.add_argument('--timeout', type=float, default=180,
                        help='wall seconds after connected readiness; meal observation may need 600')
    parser.add_argument('--port', type=int, default=0,
                        help='owned server UDP port; zero selects a free port without touching other runs')
    parser.add_argument('--warp', type=int, choices=(1, 10, 25), default=10)
    parser.add_argument('--server', type=Path, default=Path('target/playtest/server'))
    parser.add_argument('--client', type=Path, default=Path('target/playtest/client'))
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error('--timeout must be positive')
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)

    def save(name, data):
        (out / name).write_text(json.dumps(data, indent=2))

    port = claim_udp_port(args.port)
    binaries = {label: binary_identity(path)
                for label, path in (('server', args.server), ('client', args.client))}
    server = client = session = snapshots = None
    samples = []
    meals = ConstructionMeals() if args.observe_meals else None
    meal_captures = []
    movement_captures = []
    captured = set()
    passed = False
    error = None
    chopping = carrying_recorded = complete = False
    deliveries = 0

    def record(name, frames, interval_ms):
        session.command('record', name=name, frames=frames, interval_ms=interval_ms)
        artifacts = completed_captures(out / 'client', name)
        if len(artifacts) != frames:
            raise AssertionError(f'Expected {frames} completed frames for {name}, got {len(artifacts)}')
        movement_captures.append({'name': name, 'artifacts': artifacts})
        return artifacts

    def capture_meal_event(event):
        label = (event['actor']['id'], event['event'])
        if label in captured or event['event'] == 'construction':
            return
        state = session.status()
        actor = next((p for p in state.get('households', {}).get('people', [])
                      if p['id'] == label[0]), None)
        if actor is None:
            return
        if event['event'] == 'food' and actor.get('objective') not in FOOD_OBJECTIVES:
            return  # A sampled event may have ended while another PNG was encoding.
        if event['event'] == 'resumed' and (actor.get('activity') != 'Building'
                or distance(actor['position'], event['site']['stand']) > 4.0):
            return
        at = actor['position']
        name = f'meal-{label[0]}-{label[1]}'
        session.command('view', x=at[0], z=at[2], zoom=30)
        # Record itself waits for semantic camera/chunk/yard readiness, then
        # every screenshot completion. Sidecars expose any phase change while
        # the camera settles; a command reply alone is not visual proof.
        artifacts = record(name, frames=8, interval_ms=100)
        captured.add(label)
        meal_captures.append({'event': event, 'captures': artifacts})
    try:
        env = environment(root)
        env.update(CITYSIM_MAP_ID='village_lab', FISTWORLD_DEV='1',
                   FISTWORLD_VILLAGE_LAB_RUNTIME='1', FISTWORLD_LAB_SCENARIO='town-growth',
                   FISTWORLD_TOWN_PROFILE='inland-boats', FISTWORLD_TOWN_SEED='23',
                   FISTWORLD_LAB_WARP='1', FISTWORLD_VILLAGE_TRACE='1',
                   FISTWORLD_SERVER_PORT=str(port),
                   FISTWORLD_MOVEMENT_TRACE_DIR=str(out),
                   RUST_LOG='info,server::world::village::construction=debug')
        with (out / 'server.log').open('w') as log:
            server = subprocess.Popen([args.server.resolve()], cwd=root, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, out / 'server.log',
                    lambda text: f'Server UDP socket bound to 0.0.0.0:{port}' in text, 'owned server ready')
        ce = environment(root)
        ce.update(CITYSIM_MAP_ID='village_lab', FISTFORCE_NO_SETTINGS_FILE='1',
                  FISTFORCE_DISPLAY_MODE='windowed',
                  FISTFORCE_FULLSCREEN='0', FISTFORCE_RESOLUTION='1440x900',
                  FISTFORCE_RENDER_SCALE='1', FISTWORLD_SESSION_CAPTURE_DIR=str(out / 'client'),
                  FISTFORCE_START_FOCUS='-112,112', FISTFORCE_START_ZOOM='55')
        (out / 'client').mkdir()
        with (out / 'client/client.log').open('w') as log:
            client = subprocess.Popen([args.client.resolve()], cwd=root, env=ce,
                                      stdout=log, stderr=subprocess.STDOUT)
        save('processes.json', {'server': server.pid, 'client': client.pid, 'port': port,
                                'binaries': binaries})
        session = Session(out / 'client')
        snapshots = ContinuousSnapshots(session, out / 'client-states.jsonl')
        menu(session)
        replace(session, 'startup-server-field', f'127.0.0.1:{port}')
        join(session, 'BuilderObserver')
        state = session.wait(lambda s: s.get('playing'), 'connected world', timeout=180)
        if state['creator']:
            session.command('button', name='creator-BEGIN JOURNEY')
        session.wait(lambda s: s.get('hero') is not None and s.get('god_capability')
                     and not s.get('cinematic') and not s.get('creator'),
                     'ordinary opening finished', timeout=180)
        set_warp(session, args.warp)
        session.command('key', key='G')
        session.wait(lambda s: any(b['name'] == 'god-warp-1' and b['enabled']
                                  for b in s.get('buttons', [])), 'God view')
        session.command('view', x=-115, z=117, zoom=58)
        deadline = time.monotonic() + args.timeout
        prior_wood = {}
        journal = Journal(out / 'server-movement.jsonl') if meals else None
        if journal:
            journal.read()  # Acceptance begins after client/observer readiness.
            save('meal-baseline.json', {'client': session.status(), 'server': journal.latest})
        while time.monotonic() < deadline:
            if server.poll() is not None or client.poll() is not None:
                raise RuntimeError('Owned client/server exited during observation')
            state = session.status()
            samples.append({'sampled_unix_ms': state['sampled_unix_ms'], 'clock': state['clock'],
                            'people': state['households']['people'],
                            'construction': state['construction'], 'markets': state['markets']})
            if meals:
                sites = snapshots.known_sites()
                events = [event for row in journal.read() for event in meals.observe(row, sites)]
                for event in events:
                    capture_meal_event(event)
            if not chopping:
                actor = next((p for p in state['households']['people']
                              if p['activity'] == 'Chopping'), None)
                if actor:
                    chopping = True
                    session.command('view', x=actor['position'][0], z=actor['position'][2], zoom=22)
                    session.command('capture', name='00-real-tree-cutting')
                    record('00-tree-work', frames=8, interval_ms=100)
            if not carrying_recorded:
                carrier = next((p for p in state['households']['people']
                                if p['objective'] == 'CarryingConstructionWood'
                                and (p.get('load') or {}).get('good') == 'Wood'), None)
                if carrier:
                    carrying_recorded = True
                    session.command('view', x=(carrier['position'][0] - 115) / 2,
                                    z=(carrier['position'][2] + 117) / 2, zoom=82)
                    record('01-timber-journey', frames=20, interval_ms=125)
            for site in state['construction']['sites']:
                if site['wood'] > prior_wood.get(site['entity'], 0):
                    deliveries += 1
                    session.command('view', x=site['position'][0], z=site['position'][2], zoom=36)
                    session.command('capture', name=f'01-delivery-{deliveries:02}')
                prior_wood[site['entity']] = site['wood']
            if not complete:
                house = next((b for b in state['construction']['buildings']
                              if b['kind'] == 'House'), None)
                if house:
                    complete = True
                    session.command('view', x=house['position'][0], z=house['position'][2], zoom=36)
                    session.command('capture', name='02-first-real-home')
            if (chopping and carrying_recorded and deliveries >= 4 and complete
                    and (meals is None or meals.report()['passed'])):
                record('03-construction-follow-through', frames=16, interval_ms=200)
                break
            time.sleep(.12)
        else:
            if meals and not meals.report()['passed']:
                raise TimeoutError('Evidence gap: no same-builder construction/food displacement/'
                                   'physical return sequence observed before deadline; see meal-observation.json')
            raise TimeoutError('Ordinary construction did not satisfy acceptance milestones')
        passed = True
        save('result.json', {
            'passed': True, 'chopping_seen': chopping, 'carrying_seen': carrying_recorded,
            'delivery_events': deliveries, 'completed_house_seen': complete,
            'samples': len(samples), 'warp': state['time_warp'], 'port': port,
            'binaries': binaries, 'movement_captures': movement_captures,
            'meal_observation_required': args.observe_meals,
            'meal_observation': meals.report() if meals else None,
            'scope': 'Ordinary village_lab founders, no timber or ownership grants. '
                     'Inspect movement captures and server delivery traces for route geometry; '
                     'these milestones do not guarantee a tree-blocked delivery goal or queue detour was exercised, '
                     'and are not a shortest-path or performance assertion.',
        })
        print('ORDINARY CONSTRUCTION ACCEPTANCE RECORDED', flush=True)
    except BaseException as caught:
        error = repr(caught)
        raise
    finally:
        try:
            if snapshots:
                snapshots.close()
            if meals:
                report = meals.report()
                report.update(captures=meal_captures, error=error,
                              visual_scope='Completed real PNG/capture/session sidecars retained. '
                                           'Inspect the recorded phase in every sidecar; a short event '
                                           'may finish before camera/readback, so semantic events are '
                                           'not a claim that all three phases were captured visually.')
                save('meal-observation.json', report)
            if not passed:
                save('result.json', {'passed': False, 'error': error, 'port': port, 'binaries': binaries,
                                    'movement_captures': movement_captures, 'chopping_seen': chopping,
                                    'carrying_seen': carrying_recorded, 'delivery_events': deliveries,
                                    'completed_house_seen': complete})
            save('samples.json', samples)
            if session:
                save('last-state.json', session.status())
        finally:
            stop(client)
            stop(server)


if __name__ == '__main__':
    main()

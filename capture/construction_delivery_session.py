"""Observe ordinary self-supplied builders, without timber or ownership grants."""
import argparse
import json
import subprocess
import time
from pathlib import Path

from first_session import Session, environment, wait_server, stop


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path,
                        default=Path('logs/construction-delivery') / time.strftime('%Y%m%d-%H%M%S'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)

    def save(name, data):
        (out / name).write_text(json.dumps(data, indent=2))

    if subprocess.run(['lsof', '-nP', '-iUDP:5000'], stdout=subprocess.DEVNULL).returncode == 0:
        raise RuntimeError('UDP5000 already owned; leaving that server alone')
    server = client = session = None
    samples = []
    try:
        env = environment(root)
        env.update(CITYSIM_MAP_ID='village_lab', FISTWORLD_DEV='1',
                   FISTWORLD_VILLAGE_LAB_RUNTIME='1', FISTWORLD_LAB_SCENARIO='town-growth',
                   FISTWORLD_TOWN_PROFILE='inland-boats', FISTWORLD_TOWN_SEED='23',
                   FISTWORLD_LAB_WARP='10', FISTWORLD_VILLAGE_TRACE='1',
                   RUST_LOG='info,server::world::village::construction=debug')
        with (out / 'server.log').open('w') as log:
            server = subprocess.Popen([root / 'target/debug/server'], cwd=root, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, out / 'server.log',
                    lambda text: 'Server UDP socket bound to 0.0.0.0:5000' in text, 'server ready')
        ce = environment(root)
        ce.update(CITYSIM_MAP_ID='village_lab', FISTFORCE_NO_SETTINGS_FILE='1',
                  FISTFORCE_AUTOCONNECT='BuilderObserver', FISTFORCE_DISPLAY_MODE='windowed',
                  FISTFORCE_FULLSCREEN='0', FISTFORCE_RESOLUTION='1440x900',
                  FISTFORCE_RENDER_SCALE='1', FISTWORLD_SESSION_CAPTURE_DIR=str(out / 'client'),
                  FISTFORCE_START_FOCUS='-112,112', FISTFORCE_START_ZOOM='55')
        (out / 'client').mkdir()
        with (out / 'client/client.log').open('w') as log:
            client = subprocess.Popen([root / 'target/playtest/client'], cwd=root, env=ce,
                                      stdout=log, stderr=subprocess.STDOUT)
        save('processes.json', {'server': server.pid, 'client': client.pid})
        session = Session(out / 'client')
        state = session.wait(lambda s: s['playing'], 'connected world', timeout=180)
        if state['creator']:
            session.command('button', name='creator-BEGIN JOURNEY')
        session.wait(lambda s: s.get('hero') is not None and s['god_capability'], 'observer ready')
        session.command('key', key='G')
        session.wait(lambda s: any(b['name'] == 'god-warp-1' and b['enabled']
                                  for b in s.get('buttons', [])), 'God view')
        session.command('view', x=-115, z=117, zoom=58)
        deadline = time.monotonic() + 180
        prior_wood = {}
        deliveries = 0
        chopping = carrying_recorded = complete = False
        while time.monotonic() < deadline:
            state = session.status()
            samples.append({'sampled_unix_ms': state['sampled_unix_ms'], 'clock': state['clock'],
                            'people': state['households']['people'],
                            'construction': state['construction'], 'markets': state['markets']})
            if not chopping:
                actor = next((p for p in state['households']['people']
                              if p['activity'] == 'Chopping'), None)
                if actor:
                    chopping = True
                    session.command('view', x=actor['position'][0], z=actor['position'][2], zoom=22)
                    session.command('capture', name='00-real-tree-cutting')
                    session.command('record', name='00-tree-work', frames=8, interval_ms=100)
            if not carrying_recorded:
                carrier = next((p for p in state['households']['people']
                                if p['objective'] == 'CarryingConstructionWood'
                                and (p.get('load') or {}).get('good') == 'Wood'), None)
                if carrier:
                    carrying_recorded = True
                    session.command('view', x=(carrier['position'][0] - 115) / 2,
                                    z=(carrier['position'][2] + 117) / 2, zoom=82)
                    session.command('record', name='01-timber-journey', frames=20, interval_ms=125)
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
            if chopping and carrying_recorded and deliveries >= 4 and complete:
                session.command('record', name='03-construction-follow-through',
                                frames=16, interval_ms=200)
                break
            time.sleep(.12)
        else:
            raise TimeoutError('Ordinary construction did not satisfy acceptance milestones')
        save('result.json', {
            'passed': True, 'chopping_seen': chopping, 'carrying_seen': carrying_recorded,
            'delivery_events': deliveries, 'completed_house_seen': complete,
            'samples': len(samples), 'warp': state['time_warp'],
            'scope': 'Ordinary village_lab founders, no timber or ownership grants. '
                     'Inspect movement captures and server delivery traces for route geometry; '
                     'these construction milestones are not a shortest-path or performance assertion.',
        })
        print('ORDINARY CONSTRUCTION ACCEPTANCE RECORDED', flush=True)
    finally:
        try:
            save('samples.json', samples)
            if session:
                save('last-state.json', session.status())
        finally:
            stop(client)
            stop(server)


if __name__ == '__main__':
    main()

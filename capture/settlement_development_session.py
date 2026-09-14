"""Observe ordinary housing qualification and paid Village Hall completion online."""
import argparse
import json
import subprocess
import time
from pathlib import Path

from first_session import Session, distance, environment, wait_server, stop


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path,
                        default=Path('logs/settlement-development-session') / time.strftime('%Y%m%d-%H%M%S'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    if subprocess.run(['lsof', '-nP', '-iUDP:5000'], stdout=subprocess.DEVNULL).returncode == 0:
        raise RuntimeError('UDP5000 is occupied; leaving that server alone')

    def save(name, data):
        (out / name).write_text(json.dumps(data, indent=2))

    def meadow(state):
        return next((town for town in state.get('markets', [])
                     if town['name'] == 'Lab Meadow'), None)

    def button(state, name):
        return any(item['name'] == name and item['enabled']
                   for item in state.get('buttons', []))

    server = client = session = None
    samples = []
    try:
        env = environment(root)
        env.update(CITYSIM_MAP_ID='village_lab', FISTWORLD_DEV='1',
                   FISTWORLD_VILLAGE_LAB_RUNTIME='1', FISTWORLD_LAB_SCENARIO='secure',
                   FISTWORLD_LAB_DAY_TWO_ARRIVALS='8', FISTWORLD_LAB_WARP='25',
                   FISTWORLD_VILLAGE_TRACE='1')
        with (out / 'server.log').open('w') as log:
            server = subprocess.Popen([root / 'target/debug/server'], cwd=root, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, out / 'server.log',
                    lambda text: 'Server UDP socket bound to 0.0.0.0:5000' in text, 'server ready')
        ce = environment(root)
        ce.update(CITYSIM_MAP_ID='village_lab', FISTFORCE_NO_SETTINGS_FILE='1',
                  FISTFORCE_AUTOCONNECT='DevelopmentObserver', FISTFORCE_DISPLAY_MODE='windowed',
                  FISTFORCE_FULLSCREEN='0', FISTFORCE_RESOLUTION='1600x1000',
                  FISTFORCE_RENDER_SCALE='1', FISTWORLD_SESSION_CAPTURE_DIR=str(out / 'client'),
                  FISTFORCE_START_FOCUS='112,-158', FISTFORCE_START_ZOOM='65')
        (out / 'client').mkdir()
        with (out / 'client/client.log').open('w') as log:
            client = subprocess.Popen([root / 'target/playtest/client'], cwd=root, env=ce,
                                      stdout=log, stderr=subprocess.STDOUT)
        save('processes.json', {'server': server.pid, 'client': client.pid})
        session = Session(out / 'client')
        state = session.wait(lambda s: s['playing'], 'connected world', timeout=180)
        if state['creator']:
            session.command('button', name='creator-BEGIN JOURNEY')
        state = session.wait(lambda s: s.get('hero') is not None and meadow(s) is not None
                             and not s.get('cinematic'),
                             'ordinary observer and replicated Hall')
        initial = meadow(state)
        assert initial['tier'] == 'Hamlet', initial
        identity = initial['id']
        at = initial['position']
        session.command('view', x=at[0], z=at[2], zoom=48)
        session.wait(lambda s: distance(s['camera']['focus'], at) < .5
                     and abs(s['camera']['zoom'] - 48) < .5, 'Hall camera settled')
        session.command('left_click', x=at[0], z=at[2])
        session.wait(lambda s: button(s, 'text:EXPAND'), 'real settlement inspector')
        session.command('capture', name='00-founding-hamlet')
        save('before.json', session.status())

        seen_progress = seen_project = False
        deadline = time.monotonic() + 720
        while time.monotonic() < deadline:
            state = session.status()
            town = meadow(state)
            if town is None:
                raise AssertionError('Observed settlement left replication')
            assert town['id'] == identity, 'promotion replaced the settlement identity'
            development = town['development']
            samples.append({'clock': state['clock'], 'town': town})
            if not seen_progress and development['progress_days'] > 0:
                seen_progress = True
                save('qualification.json', state)
                session.command('capture', name='01-daily-qualification')
            if not seen_project and development['material_required'] > 0:
                seen_project = True
                assert development['progress_days'] == 2
                assert development['material_required'] == 12
                save('hall-project.json', state)
                session.command('capture', name='02-paid-hall-project')
                session.command('record', name='02-hall-work', frames=12, interval_ms=200)
            if town['tier'] == 'Village':
                assert seen_progress and seen_project, 'missed the actual qualification/project sequence'
                assert development['material_required'] == 0
                save('completed.json', state)
                session.command('capture', name='03-completed-village')
                session.command('button', name='text:EXPAND')
                session.wait(lambda s: button(s, 'text:Overview'), 'Places overview')
                session.command('capture', name='04-village-overview')
                save('result.json', {
                    'passed': True, 'settlement_id_retained': identity,
                    'daily_qualification_seen': seen_progress, 'paid_hall_project_seen': seen_project,
                    'completed_tier': town['tier'], 'completed_clock': state['clock'],
                    'scope': 'Connected coastal Village Lab at 25x with 8 founders and 8 day-two arrivals. '
                             'No additional grants of housing, work, timber, money or tier; normal simulation '
                             'earns qualification and constructs the Hall. Not a frame-rate benchmark.',
                })
                print('CONNECTED SETTLEMENT DEVELOPMENT PASSED', flush=True)
                break
            time.sleep(.25)
        else:
            raise TimeoutError('Ordinary settlement did not complete its Village Hall')
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

"""Real client request, physical timber delivery and occupied-home completion."""
import argparse
import json
import subprocess
import time
from pathlib import Path

from first_session import Session, environment, wait_server, stop


def building(state, identity):
    return next((b for b in state.get('construction', {}).get('buildings', []) if b.get('id') == identity), None)

def worksite(state, identity):
    return next((s for s in state.get('construction', {}).get('sites', []) if (s.get('house_upgrade') or {}).get('house') == identity), None)

def enabled(state, name):
    return any(b['name'] == name and b['enabled'] for b in state.get('buttons', []))

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=Path('logs/house-upgrade-session') / time.strftime('%Y%m%d-%H%M%S'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    # A fresh directory prevents an earlier process's status being mistaken for
    # semantic readiness in this run. Preserve each run's evidence.
    out.mkdir(parents=True, exist_ok=False)

    def persist(name, state):
        (out / name).write_text(json.dumps(state, indent=2))

    if subprocess.run(['lsof', '-nP', '-iUDP:5000'], stdout=subprocess.DEVNULL).returncode == 0:
        raise RuntimeError('UDP5000 already owned; leaving that server alone')
    env = environment(root)
    env.update(CITYSIM_MAP_ID='village_lab', FISTWORLD_DEV='1',
               FISTWORLD_VILLAGE_LAB_RUNTIME='1', FISTWORLD_LAB_SCENARIO='town-growth',
               FISTWORLD_TOWN_PROFILE='inland-boats', FISTWORLD_TOWN_SEED='23',
               FISTWORLD_LAB_WARP='10', FISTWORLD_VILLAGE_TRACE='1', FISTWORLD_HOUSE_UPGRADE_LAB='1')
    server = client = None
    try:
        with (out / 'server.log').open('w') as log:
            server = subprocess.Popen([root / 'target/debug/server'], env=env, cwd=root, stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, out / 'server.log', lambda text: 'Server UDP socket bound to 0.0.0.0:5000' in text, 'server bound')
        ce = environment(root)
        ce.update(CITYSIM_MAP_ID='village_lab', FISTFORCE_NO_SETTINGS_FILE='1',
                  FISTFORCE_AUTOCONNECT='UpgradeObserver', FISTFORCE_DISPLAY_MODE='windowed',
                  FISTFORCE_FULLSCREEN='0', FISTFORCE_RESOLUTION='1440x900', FISTFORCE_RENDER_SCALE='1',
                  FISTWORLD_SESSION_CAPTURE_DIR=str(out / 'client'),
                  FISTFORCE_START_FOCUS='-112,112', FISTFORCE_START_ZOOM='45', FISTFORCE_START_YAW='1.05')
        (out / 'client').mkdir(exist_ok=True)
        with (out / 'client/client.log').open('w') as log:
            client = subprocess.Popen([root / 'target/playtest/client'], env=ce, cwd=root, stdout=log, stderr=subprocess.STDOUT)
        persist('processes.json', {'server': server.pid, 'client': client.pid})
        session = Session(out / 'client')
        state = session.wait(lambda s: s['playing'], 'connected Playing', timeout=180)
        if state['creator']:
            session.command('button', name='creator-BEGIN JOURNEY')
        session.wait(lambda s: s['hero'] is not None and s['god_capability'], 'hero and God controls')
        session.command('key', key='G')
        session.wait(lambda s: enabled(s, 'god-warp-1'), 'warp controls')
        session.command('view', x=-112, z=112, zoom=50)
        def owns_house(s):
            hero = s.get('hero')
            return hero and any(b['kind']=='House' and b.get('owner') == hero['person_id']
                               for b in s.get('construction',{}).get('buildings',[]))
        state = session.wait(owns_house, 'ordinary occupied house staged for owner test', timeout=300)
        house = next(b for b in state['construction']['buildings'] if b['kind']=='House' and b.get('owner') == state['hero']['person_id'])
        identity, at = house['id'], house['position']
        entrance = house['entrance']
        # Completing the house starts its ordinary road-building job. Wait for
        # that real connector before commissioning work which needs Hall access.
        session.wait(lambda s: any(
            road['built_through'] >= len(road['points']) and road['points'] and
            any((point[0]-entrance[0])**2 + (point[1]-entrance[2])**2 < 0.25
                for point in (road['points'][0], road['points'][-1]))
            for road in s['construction']['roads']),
            'completed doorway connector', timeout=120)
        session.command('button', name='god-warp-1')
        session.wait(lambda s: s['time_warp'] == 1.0, 'normal speed')
        session.command('key', key='G')
        session.command('view', x=at[0], z=at[2], zoom=35)
        session.wait(lambda s: abs(s['camera']['focus'][0]-at[0]) < .2 and abs(s['camera']['zoom']-35) < .2, 'house framing')
        session.command('left_click', x=at[0], z=at[2])
        before = session.wait(lambda s: enabled(s, 'house-upgrade-upper-storey'), 'owner upgrade button')
        persist('before.json', before)
        session.command('capture', name='00-owned-four-bed-home')
        session.command('button', name='house-upgrade-upper-storey')
        session.command('capture', name='00-upgrade-request-feedback')
        try:
            accepted = session.wait(
                lambda s: worksite(s, identity) is not None
                or (s['construction'].get('house_upgrade_request') or {}).get('response') is not None,
                'authoritative project response', timeout=30)
            response = (accepted['construction'].get('house_upgrade_request') or {}).get('response')
            if response and not response['accepted']:
                raise RuntimeError(f"Extension rejected: {response['message']}")
        except (TimeoutError, RuntimeError):
            persist('request-failure.json', session.status())
            session.command('capture', name='00-upgrade-request-failure')
            raise
        persist('accepted.json', accepted)
        assert building(accepted, identity)['housing_capacity'] == 4
        session.command('capture', name='01-commissioned-home')
        samples = []
        deadline = time.monotonic()+300
        delivered_seen = working_seen = False
        while time.monotonic() < deadline:
            state = session.status()
            site = worksite(state, identity)
            home = building(state, identity)
            samples.append({'clock':state.get('clock'), 'hero':state.get('hero'), 'site':site,'house':home,
                            'people':state.get('households',{}).get('people',[]),
                            'groups':state.get('households',{}).get('groups',[])})
            if site and site['wood'] > 0 and not delivered_seen:
                delivered_seen = True
                session.command('capture', name='02-delivered-wood')
            if site and site['raising'] and not working_seen:
                working_seen = True
                assert home['housing_capacity'] == 4
                session.command('capture', name='03-occupied-home-during-work')
                session.command('record', name='04-worker-building', frames=16, interval_ms=300)
            if home and home.get('house_appearance',{}).get('level') == 'UpperStorey' and not site:
                break
            time.sleep(.15)
        else:
            raise TimeoutError('House did not finish its physical extension')
        after = session.wait(lambda s: (building(s,identity) or {}).get('rendered_type') in ('CabinL2','LongCabinL2'), 'actual L2 asset ready')
        session.command('capture', name='05-completed-eight-bed-home')
        persist('after.json', after)
        initial, final = building(before, identity), building(after, identity)
        assert final['id'] == initial['id'] and final['owner'] == initial['owner']
        assert final['housing_capacity'] == 8 and delivered_seen and working_seen
        assert set(initial['household']['resident_ids']).issubset(set(final['household']['resident_ids']))
        persist('samples.json', samples)
        persist('result.json', {'passed':True,'house':identity,'before':initial,'after':final,
                               'saw_delivered_wood':delivered_seen,'saw_occupied_construction':working_seen,
                               'normal_speed':after['time_warp'] == 1.0,'sample_count':len(samples)})
        print('CONNECTED HOUSE UPGRADE PASSED', flush=True)
    except BaseException:
        if 'session' in locals():
            persist('failure.json', session.status())
        if 'samples' in locals():
            persist('samples.json', samples)
        raise
    finally:
        stop(client)
        stop(server)


if __name__ == '__main__':
    main()

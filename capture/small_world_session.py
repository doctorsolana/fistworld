#!/usr/bin/env python3
"""Observe an ordinary small Frontier world through the real launcher/client.

The server generates its actual seeded opening. The only later controls are
normal character creation, God time speed and camera/capture commands. Nothing
grants resources, forces investment, places buildings or chooses immigrant towns.
"""
import argparse
import json
import math
from pathlib import Path
import subprocess
import time

from first_session import distance, environment, stop, wait_server
from multiplayer_session import claim_udp_port, check_capture
from startup_session import start_client, join, menu, replace
from worker_lifecycle_session import Journal, binary_identity, set_warp

ROOT = Path(__file__).resolve().parents[1]


class Acceptance:
    """Require actual registration after a separately observed voyage."""
    def __init__(self, opening, minimum_arrivals):
        assert opening['settlement_count'] == 4 and opening['founder_count'] == 24, opening
        self.towns = {town['id']: town for town in opening['settlements']}
        assert len(self.towns) == 4
        self.founders = {person['id'] for person in opening['people']}
        assert len(self.founders) == 24
        assert opening['coastal_gateways'] > 0
        extent = (opening['bounds']['max'][0] - opening['bounds']['min'][0]) / 2
        assert abs((extent / 4096) ** 2 - .2) < .000001
        for town in self.towns.values():
            assert town['resident_count_reported'] == 6 and len(town['resident_ids']) == 6
            assert town['tier'] == 'Hamlet' and not town['buildings']
            assert town['stock']['Bread'] == 20 and town['stock']['Wheat'] == 6 and town['stock']['Wood'] == 12
            assert town['treasury'] == 2300
            for other in self.towns.values():
                if other['id'] != town['id']:
                    assert distance(other['position'], town['position']) >= 480
        assert all(person['home'] is None and person['wallet'] is not None for person in opening['people'])
        self.minimum_arrivals = minimum_arrivals
        self.boat_entries = set()
        self.undecided_boats = set()
        self.destinations = {}
        self.decision_timeline = {}
        self.arrivals = {}
        self.events = []
        self.latest = None
        self.maximum_boats = 0
        self.maximum_route_points = 0
        self.samples = 0

    def observe(self, row):
        if self.latest:
            assert row['absolute_world_seconds'] >= self.latest['absolute_world_seconds'], 'calendar moved backwards'
        assert {town['id'] for town in row['settlements']} == set(self.towns), 'founding Hall identity changed'
        boats = row['incoming_boats']
        self.maximum_boats = max(self.maximum_boats, len(boats))
        assert len(boats) <= 8, 'ordinary immigration exceeded its live boat cap'
        for boat in boats:
            assert boat['position_in_bounds'] and boat['route_in_bounds'] and boat['cursor_valid'], boat
            self.maximum_route_points = max(self.maximum_route_points, boat['route_points'])
        for person in row['people']:
            if person['id'] in self.founders:
                continue
            registration_pending = (not person['counts_as_resident'] or person['aboard']
                                    or person['voyage'] or person['immigration_queue']
                                    or person['immigration_departure'])
            if registration_pending:
                assert not person['counts_as_resident'], ('census admission before Hall registration', person)
                assert person['resident_of'] is None, ('citizenship before Hall registration', person)
                assert person.get('home') is None, ('housing before Hall registration', person)
                assert person.get('workplace') is None, ('employment before Hall registration', person)
            if person['aboard']:
                self.boat_entries.add(person['id'])
                if person['chosen_town'] is None:
                    self.undecided_boats.add(person['id'])
            if person['chosen_town'] is not None:
                assert person['chosen_town'] in self.towns, person
                self.destinations[person['id']] = person['chosen_town']
                assert person['chosen_at'] is not None and person['entered_at'] is not None
                assert person['chosen_at'] > person['entered_at'], ('town chosen before physical world entry', person)
                assert person['chosen_score'] is not None and math.isfinite(person['chosen_score'])
                self.decision_timeline[person['id']] = {
                    'entry': person['entry'], 'entered_at': person['entered_at'],
                    'chosen_at': person['chosen_at'], 'settlement': person['chosen_town'],
                    'score': person['chosen_score']}

            if (person['resident_of'] is not None and not person['aboard']
                    and not person['voyage'] and not person['immigration_queue']
                    and person['counts_as_resident'] and not person['immigration_departure']
                    and person['id'] not in self.arrivals):
                assert person['id'] in self.boat_entries, ('registration without a separately observed voyage', person)
                assert person['id'] in self.decision_timeline, ('registration without entry-before-choice evidence', person)
                assert person['resident_of'] in self.towns
                self.arrivals[person['id']] = person['resident_of']
        self.events.extend(dict(event, absolute_world_seconds=row['absolute_world_seconds'])
                           for event in row['events'])
        self.latest = row
        self.samples += 1

    def passed(self):
        return len(self.arrivals) >= self.minimum_arrivals and len(self.decision_timeline) >= self.minimum_arrivals

    def evidence(self):
        by_town = {town: sum(target == town for target in self.arrivals.values()) for town in self.towns}
        return {'samples': self.samples, 'boat_entries': sorted(self.boat_entries),
                'boats_observed_before_destination': sorted(self.undecided_boats),
                'chosen_destinations': self.destinations, 'decision_timeline': self.decision_timeline,
                'registered_arrivals': self.arrivals,
                'arrivals_by_town': by_town, 'events': self.events,
                'maximum_live_immigrant_boats': self.maximum_boats,
                'maximum_observed_route_points': self.maximum_route_points, 'latest': self.latest}


def frame(session, position, zoom=100):
    session.command('view', x=position[0], z=position[2], zoom=zoom, yaw=.6)
    session.wait(lambda state: state.get('camera') and distance(state['camera']['focus'], position) < 1
                 and abs(state['camera']['zoom'] - zoom) < .2, 'actual Hall camera settled')


def check_voyage(directory):
    result = check_capture(directory, 'immigrant-sea-voyage')
    positions = {}
    for path in sorted(directory.glob('immigrant-sea-voyage*.session.json')):
        state = json.loads(path.read_text())
        focus = state['camera']['focus']
        for boat in state.get('multiplayer', {}).get('boats', []):
            if boat['npc_arrival'] and distance(boat['position'], focus) <= 70:
                positions.setdefault(boat['entity'], []).append(boat['position'])
    moved = max((max(distance(samples[0], point) for point in samples)
                 for samples in positions.values() if len(samples) >= 3), default=0)
    assert moved >= 2, 'the framed immigrant boat did not visibly travel across three captured samples'
    result['framed_immigrant_displacement_m'] = moved
    return result


def check_unvisited_towns(row, viewed_town):
    """Require completed server buildings before the camera's first visit."""
    towns = [town for town in row['settlements'] if town['id'] != viewed_town]
    assert len(towns) == 3
    assert all(any(building['kind'] == 'House' for building in town['buildings'])
               for town in towns), ('unvisited towns did not complete housing', towns)
    never_observed = [town['id'] for town in towns if not town['ever_observed']]
    assert len(never_observed) >= 2, ('not enough truly unobserved towns to prove background growth', towns)
    return {'absolute_world_seconds': row['absolute_world_seconds'],
            'camera_town': viewed_town, 'never_observed_towns': never_observed,
            'unvisited_towns': towns}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--server', type=Path, default=ROOT/'target/playtest/server')
    parser.add_argument('--seed', type=int, default=91)
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--resolution', default='1600x1000')
    parser.add_argument('--days', type=float, default=3)
    parser.add_argument('--warp', type=int, choices=(1, 25), default=25)
    parser.add_argument('--minimum-arrivals', type=int, default=3)
    parser.add_argument('--timeout', type=float, default=1500)
    parser.add_argument('--route-diagnostics', action='store_true',
                        help='Record exact rejected land-route segments and obstacle diagnostics')
    parser.add_argument('--hold-first-town', action='store_true',
                        help='Keep the camera at one town until all simulated days pass; require housing in the three unvisited towns')
    args = parser.parse_args()
    if not 0 < args.days <= 30 or not 1 <= args.minimum_arrivals <= 90:
        raise ValueError('Use 0–30 days and 1–90 minimum arrivals')
    out = args.out.resolve()
    if not out.is_relative_to(ROOT/'logs'):
        raise ValueError('Review artifacts belong under logs/')
    out.mkdir(parents=True, exist_ok=False)
    report = {'passed': False, 'scenario': 'ordinary-small-frontier', 'seed': args.seed,
              'days': args.days, 'warp': args.warp, 'captures': [], 'minimum_arrivals': args.minimum_arrivals,
              'route_diagnostics': args.route_diagnostics,
              'hold_first_town': args.hold_first_town,
              'binaries': {name: binary_identity(path) for name, path in
                           [('server', args.server), ('client', ROOT/'target/playtest/client')]},
              'scope': 'Ordinary random-recipe world with fixed verification seed and finite bare-Hall opening. '
                       'Real world-entry boats choose destinations, travel and register normally. Town distribution is observed, '
                       'never quota-driven. No actor movement, building placement, money or goods granted by the driver. '
                       'God mode is used only for time speed. Not a performance or long-term economic-balance benchmark.'}
    server = client = session = tracker = None
    try:
        port = claim_udp_port(args.port)
        env = environment(ROOT) | {'FISTWORLD_WORLD_CONFIG': str(ROOT/'config/worlds/small-frontier.ron'),
            'FISTWORLD_WORLD_SEED': str(args.seed), 'FISTWORLD_SMALL_WORLD_TRACE_DIR': str(out),
            'FISTWORLD_DEV': '1', 'FISTWORLD_SERVER_PORT': str(port)}
        if args.route_diagnostics:
            env['FISTWORLD_LAB_ROUTE_DIAGNOSTICS'] = '1'
        with (out/'server.log').open('w') as log:
            server = subprocess.Popen([args.server.resolve()], cwd=ROOT, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, out/'server.log', lambda text: 'Small-world acceptance opening recorded' in text
                    and f'Server UDP socket bound to 0.0.0.0:{port}' in text,
                    'ordinary small world ready', timeout=360)
        opening = json.loads((out/'opening.json').read_text())
        report['opening'] = opening
        tracker = Acceptance(opening, args.minimum_arrivals)
        journal = Journal(out/'small-world.jsonl')
        client, session = start_client(ROOT, out/'client', args.resolution)
        (out/'processes.json').write_text(json.dumps({'server': server.pid, 'client': client.pid}))
        menu(session)
        replace(session, 'startup-server-field', f'127.0.0.1:{port}')
        join(session, 'FrontierQA')
        session.wait(lambda state: state.get('playing') and state.get('creator') and state.get('god_capability'),
                     'ordinary character creator and speed access', timeout=240)
        session.command('button', name='creator-BEGIN JOURNEY')
        session.wait(lambda state: state.get('hero') and not state.get('creator') and not state.get('cinematic'),
                     'ordinary player arrival', timeout=300)
        first_views = opening['settlements'][:1] if args.hold_first_town else opening['settlements']
        for town in first_views:
            frame(session, town['position'])
            report['captures'].append(session.command('capture', name=f'first-visit-town-{town["id"]}'))
        for row in journal.read():
            tracker.observe(row)
        assert tracker.latest, 'server did not publish samples'
        begin = tracker.latest['absolute_world_seconds']
        end = begin + args.days * tracker.latest['cycle_duration']
        report['observation_start'] = tracker.latest
        set_warp(session, args.warp)
        deadline, next_view = time.monotonic() + args.timeout, begin
        viewed = 0
        voyage_recorded = False
        offscreen_checked = not args.hold_first_town
        while time.monotonic() < deadline:
            if any(process.poll() is not None for process in (server, client)):
                raise RuntimeError('An owned game process exited')
            for row in journal.read():
                tracker.observe(row)
            latest = tracker.latest
            if latest and not args.hold_first_town and latest['absolute_world_seconds'] >= next_view:
                town = opening['settlements'][viewed % 4]
                frame(session, town['position'])
                viewed += 1
                next_view = latest['absolute_world_seconds'] + latest['cycle_duration'] / 4
            if latest and not offscreen_checked and latest['absolute_world_seconds'] >= end:
                report['before_first_visits'] = check_unvisited_towns(latest, opening['settlements'][0]['id'])
                (out/'unvisited-towns.json').write_text(json.dumps(report['before_first_visits'], indent=2))
                offscreen_checked = True
            moving = [boat for boat in (latest or {}).get('incoming_boats', []) if boat['moving']]
            if moving and not voyage_recorded and offscreen_checked:
                # Inspect the real boat at readable speed; no position or voyage edits.
                set_warp(session, 1)
                for row in journal.read():
                    tracker.observe(row)
                boat = next((current for current in tracker.latest['incoming_boats']
                             if current['entity'] == moving[0]['entity']), moving[0])
                frame(session, boat['position'], 45)
                report['captures'].append(session.command('record', name='immigrant-sea-voyage',
                    frames=12, interval_ms=120, timeout=90))
                voyage_recorded = True
                set_warp(session, args.warp)
            if latest and latest['absolute_world_seconds'] >= end:
                if not tracker.passed():
                    raise AssertionError(('calendar elapsed without real entry/choice/registration evidence', tracker.evidence()))
                if voyage_recorded:
                    break
            time.sleep(.05)
        else:
            raise TimeoutError(f'Ordinary {args.days}-day run did not complete')
        # Speed is returned through the real UI for readable ending views.
        set_warp(session, 1)
        for town in opening['settlements']:
            frame(session, town['position'])
            report['captures'].append(session.command('capture', name=f'final-town-{town["id"]}'))
        for row in journal.read():
            tracker.observe(row)
        for prefix in ('first-visit-town-', 'final-town-'):
            report[prefix+'artifacts'] = check_capture(out/'client', prefix)
        assert voyage_recorded, 'no real moving-voyage capture'
        report['voyage_artifacts'] = check_voyage(out/'client')
        report['passed'] = True
        print('CONNECTED SMALL-WORLD ACCEPTANCE PASSED; inspect PNG and capture metadata', flush=True)
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        if tracker:
            report['evidence'] = tracker.evidence()
        if session:
            report['last_client_state'] = session.status()
        (out/'report.json').write_text(json.dumps(report, indent=2))
        stop(client)
        stop(server)


if __name__ == '__main__':
    main()

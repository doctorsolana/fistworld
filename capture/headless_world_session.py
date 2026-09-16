#!/usr/bin/env python3
"""Run the ordinary server with no clients and audit autonomous world progress.

The opt-in trace changes only the ordinary TimeWarp multiplier. It neither stages
actors nor supplies resources, chooses investments, moves bodies or completes work.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time

from first_session import environment, stop, wait_server
from multiplayer_session import claim_udp_port
from small_world_session import Acceptance
from worker_lifecycle_session import Journal, binary_identity

ROOT = Path(__file__).resolve().parents[1]


def reject_remote_success(log):
    """Reject known retired success shortcuts even when aggregate totals pass."""
    forbidden = ('handoff abstractly', 'completing at the counter fallback',
                 'completed at the counter fallback')
    hits = [line for line in log.splitlines() if any(phrase in line for phrase in forbidden)]
    assert not hits, {'error': 'remote-success shortcut in simulated actions', 'examples': hits[:5]}


class HeadlessAcceptance:
    def __init__(self, opening, minimum_arrivals, observation="none"):
        self.arrivals = Acceptance(opening, minimum_arrivals)
        self.observation = observation
        self.observation_states = set()
        self.opening_cash = opening['accounting']['money_total']
        self.endowment = opening['accounting']['new_villager_endowment']
        self.founders = {person['id'] for person in opening['people']}
        self.daily = {}
        self.latest = None
        self.samples = 0
        self.production = {}
        self.meals = {}
        self.first_house = {}
        self.first_business = {}

    def observe(self, row):
        self.arrivals.observe(row)
        assert row.get('connected_clients', 0) == 0, 'a real client invalidated the matched server run'
        observation = row.get('observation') or {'mode': 'none', 'active': False, 'view_slots': 0}
        assert observation['mode'] == self.observation, observation
        active = observation['active']
        self.observation_states.add(active)
        assert row['observer_clients'] == (observation['view_slots'] if active else 0), observation
        if not active:
            assert row['observed_regions'] == 0, 'inactive view inputs still marked a region observed'
        for town in row['settlements']:
            if self.observation == 'none':
                assert not town['ever_observed'] and town['observer_count'] == 0, town
            elif active:
                assert town['observer_count'] > 0, ('authored Hall view did not reach actual region interest', town)
            if town['houses']:
                self.first_house.setdefault(town['id'], row['absolute_world_seconds'])
            if town['businesses']:
                self.first_business.setdefault(town['id'], row['absolute_world_seconds'])
        expected = self.opening_cash + row.get('immigrant_endowments', row['boat_entries']) * self.endowment
        assert row['accounting']['money_total'] == expected, {
            'error': 'money conservation failed', 'expected': expected,
            'actual': row['accounting'], 'time': row['absolute_world_seconds']}
        for site in row['accounting']['businesses']:
            for ledger in (site['account']['current_day'], site['account']['previous_day']):
                if ledger['day'] < 2**32 - 1:
                    key = (site['id'], ledger['day'])
                    self.production[key] = max(self.production.get(key, 0), ledger['produced_units'])
        for person in row['people']:
            self.meals[person['id']] = max(self.meals.get(person['id'], 0),
                                          (person.get('nutrition') or {}).get('total_meals', 0))
        self.daily[row['day']] = {
            'absolute_world_seconds': row['absolute_world_seconds'],
            'population': len(row['people']), 'accounting': row['accounting'],
            'settlements': row['settlements']}
        self.latest = row
        self.samples += 1

    def finish(self):
        assert self.latest is not None, 'no ordinary server samples'
        required_states = {'none': {False}, 'all': {True}, 'alternating': {False, True}}[self.observation]
        assert self.observation_states == required_states, ('observation mode not exercised', self.observation_states)
        town_ids = set(self.arrivals.towns)
        assert set(self.first_house) == town_ids, {
            'error': 'unobserved town did not finish any housing',
            'missing': sorted(town_ids - set(self.first_house)),
            'towns': self.latest['settlements']}
        assert self.first_business, 'no unobserved town completed any productive business'
        assert sum(self.production.values()) > 0, 'no actual production was recorded'
        assert sum(self.meals.values()) > 0, 'no successful daily meals were recorded'
        assert self.arrivals.passed(), 'required physical immigration did not complete'

    def evidence(self):
        alive = {person['id'] for person in (self.latest or {}).get('people', [])}
        return {'samples': self.samples, 'observation': self.observation,
                'observation_states': sorted(self.observation_states), 'first_house_at': self.first_house,
                'first_business_at': self.first_business,
                'observed_site_day_production_units': sum(self.production.values()),
                'observed_lifetime_meals': sum(self.meals.values()),
                'founders_absent_at_end': sorted(self.founders - alive),
                'daily': self.daily, 'immigration': self.arrivals.evidence(),
                'latest': self.latest}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--server', type=Path, default=ROOT/'target/playtest/server')
    parser.add_argument('--seed', type=int, default=91)
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--days', type=float, default=3)
    parser.add_argument('--warp', type=int, choices=(1, 25), default=25)
    parser.add_argument('--minimum-arrivals', type=int, default=3)
    parser.add_argument('--timeout', type=float, default=1800)
    parser.add_argument('--route-diagnostics', action='store_true')
    parser.add_argument('--observation', choices=('none', 'all', 'alternating'), default='none')
    args = parser.parse_args()
    if not 0 < args.days <= 30 or not 0 <= args.minimum_arrivals <= 90:
        raise ValueError('Use 0–30 simulated days and 0–90 required arrivals')
    out = args.out.resolve()
    if not out.is_relative_to(ROOT/'logs'):
        raise ValueError('Evidence belongs under logs/')
    out.mkdir(parents=True, exist_ok=False)
    report = {'passed': False, 'scenario': 'ordinary-frontier-observation-check', 'observation': args.observation,
              'seed': args.seed, 'days': args.days, 'warp': args.warp,
              'server_binary': binary_identity(args.server),
              'scope': 'Normal seeded server startup; no network client, Hero or staged worker. '
                       'Optional authored commander views use the real interest pipeline; no '
                       'inventory grant or forced economic outcome. Diagnostic clock speed only. '
                       'Production and meals are observed outcomes, not complete goods-conservation proofs. '
                       'This is not a performance benchmark or observed/unobserved equivalence claim.'}
    server = tracker = None
    try:
        port = claim_udp_port(args.port)
        env = environment(ROOT) | {
            'FISTWORLD_WORLD_CONFIG': str(ROOT/'config/worlds/small-frontier.ron'),
            'FISTWORLD_WORLD_SEED': str(args.seed),
            'FISTWORLD_SMALL_WORLD_TRACE_DIR': str(out),
            'FISTWORLD_SMALL_WORLD_TRACE_WARP': str(args.warp),
            'FISTWORLD_SMALL_WORLD_TRACE_SAMPLE_SECONDS': '10',
            'FISTWORLD_SMALL_WORLD_TRACE_OBSERVATION': args.observation,
            'FISTWORLD_SERVER_PORT': str(port)}
        if args.route_diagnostics:
            env['FISTWORLD_LAB_ROUTE_DIAGNOSTICS'] = '1'
        with (out/'server.log').open('w') as log:
            server = subprocess.Popen([args.server.resolve()], cwd=ROOT, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        (out/'processes.json').write_text(json.dumps({'server': server.pid, 'clients': []}))
        wait_server(server, out/'server.log', lambda text:
                    'Small-world acceptance opening recorded' in text and
                    f'Server UDP socket bound to 0.0.0.0:{port}' in text,
                    'ordinary headless world ready', timeout=360)
        opening = json.loads((out/'opening.json').read_text())
        report['opening'] = opening
        tracker = HeadlessAcceptance(opening, args.minimum_arrivals, args.observation)
        journal = Journal(out/'small-world.jsonl')
        deadline = time.monotonic() + args.timeout
        begin = None
        reported_day = None
        while time.monotonic() < deadline:
            if server.poll() is not None:
                raise RuntimeError(f'Owned server exited: {server.returncode}')
            for row in journal.read():
                tracker.observe(row)
                if begin is None:
                    begin = row['absolute_world_seconds']
                if row['day'] != reported_day:
                    reported_day = row['day']
                    print(json.dumps({'day': row['day'], 'population': len(row['people']),
                        'money': row['accounting']['money_total'],
                        'houses': {t['id']: t['houses'] for t in row['settlements']},
                        'registered': row['registered_arrivals_by_town']}), flush=True)
            if tracker.latest and tracker.latest['absolute_world_seconds'] >= begin + args.days * tracker.latest['cycle_duration']:
                tracker.finish()
                reject_remote_success((out/'server.log').read_text())
                report['passed'] = True
                print(f'WORLD ACCEPTANCE PASSED: observation={args.observation}', flush=True)
                break
            time.sleep(.1)
        else:
            raise TimeoutError('Ordinary world did not reach the requested simulated duration')
    except BaseException as error:
        report['error'] = repr(error)
        raise
    finally:
        if tracker:
            report['evidence'] = tracker.evidence()
        (out/'report.json').write_text(json.dumps(report, indent=2))
        stop(server)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Compare ordinary worlds with no, constant, and changing camera interest.

Runs the same server binary, recipe, finite opening, warp and simulated duration.
Different wall-clock navigation budgets can change later outcomes; this reports
those differences rather than disguising them as an exact deterministic proof.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys

from worker_lifecycle_session import binary_identity

ROOT = Path(__file__).resolve().parents[1]


def compare(reports):
    reference = reports['none']
    for mode, report in reports.items():
        assert report['passed'], (mode, report.get('error'))
        assert report['server_binary'] == reference['server_binary'], 'different executables'
        for key in ('seed', 'days', 'warp'):
            assert report[key] == reference[key], (mode, key)
        for key in ('config', 'map_hash', 'bounds', 'settlements', 'people', 'accounting'):
            assert report['opening'][key] == reference['opening'][key], ('opening mismatch', mode, key)
    def outcome(report):
        evidence = report['evidence']
        final = evidence['latest']
        return {'observation': evidence['observation'],
                'final_world_seconds': final['absolute_world_seconds'],
                'population': len(final['people']),
                'founders_absent': evidence['founders_absent_at_end'],
                'first_house_at': evidence['first_house_at'],
                'produced_units': evidence['observed_site_day_production_units'],
                'meals': evidence['observed_lifetime_meals'],
                'registered_arrivals': evidence['immigration']['arrivals_by_town'],
                'towns': final['settlements'], 'accounts': final['accounting']}
    return {'passed': True, 'shared_inputs_verified': True,
            'scope': 'Each mode passed the same physical-world acceptance with identical finite opening and binary. '
                     'Outcome differences are explicit; runtime-budget ordering means this is not bit-exact equivalence.',
            'outcomes': {mode: outcome(report) for mode, report in reports.items()}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--server', type=Path, default=ROOT/'target/playtest/server')
    parser.add_argument('--seed', type=int, default=91)
    parser.add_argument('--days', type=float, default=3)
    parser.add_argument('--warp', type=int, choices=(1, 25), default=25)
    parser.add_argument('--minimum-arrivals', type=int, default=3)
    parser.add_argument('--timeout', type=float, default=1800)
    args = parser.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(ROOT/'logs'):
        raise ValueError('Evidence belongs below logs/')
    out.mkdir(parents=True, exist_ok=False)
    reports = {}
    result = {'passed': False, 'server_binary': binary_identity(args.server)}
    try:
        for mode in ('none', 'all', 'alternating'):
            subprocess.run([sys.executable, str(ROOT/'capture/headless_world_session.py'),
                '--out', str(out/mode), '--server', str(args.server.resolve()),
                '--seed', str(args.seed), '--days', str(args.days), '--warp', str(args.warp),
                '--minimum-arrivals', str(args.minimum_arrivals), '--timeout', str(args.timeout),
                '--observation', mode], cwd=ROOT, check=True)
            reports[mode] = json.loads((out/mode/'report.json').read_text())
        result = compare(reports)
    except BaseException as error:
        result['error'] = repr(error)
        raise
    finally:
        result['completed_modes'] = list(reports)
        (out/'comparison.json').write_text(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()

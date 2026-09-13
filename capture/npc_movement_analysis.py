"""Summarize opt-in NPC traces without treating queues or indoor work as stalls."""
from collections import defaultdict
import json
import math
from pathlib import Path


def samples(path):
    with path.open() as source:
        for line in source:
            if not line.endswith('\n'):
                return  # The owned process may have stopped during its final sample write.
            yield json.loads(line)


def analyze(directory):
    directory = Path(directory)
    tracks = defaultdict(list)
    for sample in samples(directory / 'server-movement.jsonl'):
        for actor in sample['actors']:
            tracks[actor['id']].append((sample['elapsed'], actor))
    stalls = []
    for person, rows in tracks.items():
        previous_end = float('-inf')
        for begin in range(0, len(rows), 5):
            start = rows[begin][0]
            if start < previous_end:
                continue
            end = begin
            while end + 1 < len(rows) and rows[end + 1][0] <= start + 8:
                end += 1
            window = rows[begin:end + 1]
            if len(window) < 20 or window[-1][0] - start < 7:
                continue
            if any(b[0] - a[0] > 1 for a, b in zip(window, window[1:])):
                continue  # A demoted/unobserved actor has no continuous tactical evidence.
            active = [a for _, a in window if a['target'] and math.dist(a['position'], a['target']) > .6]
            if len(active) < len(window) * .9:
                continue  # A person waiting without a movement order is allowed to stand still.
            span = math.hypot(*(max(a['position'][axis] for _, a in window)
                                - min(a['position'][axis] for _, a in window) for axis in (0, 2)))
            if span >= .5:
                continue
            stalls.append({'id': person, 'start': start, 'end': window[-1][0], 'span_m': span,
                           'position': active[0]['position'], 'target': active[0]['target'],
                           'objective': active[0]['objective'], 'queue': active[0]['queue']})
            previous_end = window[-1][0]
    porters = {}
    streaks = {}
    previous_legs = {}
    for sample in samples(directory / 'client-movement.jsonl'):
        now = sample['elapsed']
        for actor in sample['actors']:
            person = actor['id']
            if not actor['cart'] or actor['culled'] or actor['visual_speed'] < .5:
                streaks.pop(person, None)
                previous_legs.pop(person, None)
                continue
            record = porters.setdefault(person, {'id': person, 'samples': 0, 'players': set(),
                'animated_leg_samples': 0, 'bad_samples': 0, 'longest_bad_seconds': 0., 'reasons': set()})
            record['samples'] += 1
            record['players'].add(actor.get('player'))
            animation = actor.get('animation')
            reason = ('wrong animation player' if len(actor['legs']) != 2 else
                      'wrong body clip' if not actor['pull'] else
                      'frozen body clip' if not animation or animation['paused'] or animation['speed'] < .1 else None)
            if reason:
                record['bad_samples'] += 1
                record['reasons'].add(reason)
                old_reason, start, previous = streaks.get(person, (reason, now, now))
                if old_reason != reason or now - previous > .5:
                    start = now
                streaks[person] = (reason, start, now)
                record['longest_bad_seconds'] = max(record['longest_bad_seconds'], now - start)
            else:
                streaks.pop(person, None)
                rotation = actor['legs'][0]['rotation']
                if person in previous_legs:
                    previous_time, old = previous_legs[person]
                    dot = abs(sum(x * y for x, y in zip(rotation, old)))
                    if now - previous_time <= .5 and dot < .999:
                        record['animated_leg_samples'] += 1
                previous_legs[person] = (now, rotation)
    for porter in porters.values():
        porter['players'] = sorted(porter['players'], key=str)
        porter['reasons'] = sorted(porter['reasons'])
    observed_porters = [p for p in porters.values() if p['samples'] >= 10]
    report = {'server_actors_observed': len(tracks), 'stalls': stalls,
              'porters': sorted(porters.values(), key=lambda p: p['id']),
              'queue_fallbacks': (directory / 'server.log').read_text().count('made no progress at the front'),
              'scope': 'Eight-second tactical stalls and visible moving-cart animation; not a proof of every NPC routine or a frame-rate benchmark.'}
    report['passed'] = bool(observed_porters) and not stalls and report['queue_fallbacks'] == 0 and all(
        p['longest_bad_seconds'] < .6 and p['animated_leg_samples'] > 0 for p in observed_porters)
    (directory / 'movement-analysis.json').write_text(json.dumps(report, indent=2))
    return report


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory')
    args = parser.parse_args()
    print(json.dumps(analyze(args.directory), indent=2))

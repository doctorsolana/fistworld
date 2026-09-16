#!/usr/bin/env python3
"""Connected finite shipbuilding, real captain boarding and town-market freight.

Only fixture admission and a normal route order are authored. This driver never
sets actor positions, inventory, construction progress or navigation outcomes.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time

from first_session import distance, environment, stop, wait_server
from multiplayer_session import claim_udp_port, start_client
from startup_session import join, menu, replace
from worker_lifecycle_session import Journal, binary_identity, set_warp

ROOT = Path(__file__).resolve().parents[1]


def xz(a, b):
    return ((a[0]-b[0])**2+(a[2]-b[2])**2)**.5


class Acceptance:
    def __init__(self, fixture):
        self.fixture = fixture
        self.previous = None
        self.carried = self.worked = self.boarded = self.sailed = self.consigned = False
        self.purchase = False
        self.provisions_collected = False
        self.first_sailing = None
        self.events = []

    def observe(self, row):
        assert row['money'] == row['initial_money'], ('money including wages/fees/escrow changed', row)
        assert row['goods'] == row['initial_goods'], ('wood/iron/wool lost or created, including completed hull', row)
        events = []
        for person in row['people']:
            crew = person.get('crew') or ''
            if any(phase in crew for phase in ('CollectingProvisions', 'Approaching')):
                if not person.get('workplace_interior') and not person.get('workplace_door_transit'):
                    assert person['activity'] == 'Idle', ('captain retained an indoor/work animation on land', person)
                if 'CollectingProvisions' in crew:
                    events.append('provisioning')
            if person['haul'] and person['wood'] + person['iron'] + person['wool'] > 0:
                assert person.get('simulation') == 'canonical', 'construction freight bypassed physical movement'
                self.carried = True
                events.append('hauling')
            if person['builder'] and person['activity'] == 'Building':
                assert xz(person['position'], self.fixture['home']['shore']) <= .8, ('remote hull construction', row)
                self.worked = True
                events.append('building')
            if person['aboard']:
                assert person.get('simulation') == 'canonical', 'captain is not physically aboard'
                self.boarded = True
                events.append('aboard')
        if row['ships']:
            ship = row['ships'][0]
            assert self.carried and self.worked, 'hull appeared without physical material/work evidence'
            if ship['wood']:
                self.purchase = True
            if ship['status'] == 'Sailing':
                assert self.boarded, 'ship departed without observed real captain boarding'
                if self.first_sailing is None:
                    self.first_sailing = ship['position']
                self.sailed |= xz(ship['position'], self.first_sailing) > 30
                events.append('sailing')
            if row['away_wood'] >= self.fixture['cargo']:
                assert self.sailed and self.purchase, 'destination stock lacks purchased water-freight evidence'
                self.consigned = True
                events.append('consigned')
        if self.previous:
            previous = {person['id']: person for person in self.previous['people']}
            for person in row['people']:
                before = previous.get(person['id'])
                if before and person.get('food', 0) > before.get('food', 0) and before.get('crew'):
                    counter = before.get('provision_counter') or person.get('provision_counter')
                    assert counter is not None, ('crew food appeared without a counter trip', person)
                    assert xz(person['position'], counter) <= .7, ('remote captain provisions', person, counter)
                    assert abs(person['position'][1] - counter[1]) <= 1.5, ('captain provisioned from another floor', person, counter)
                    self.provisions_collected = True
                if not before or not person['haul']:
                    continue
                amount = sum(person[good] for good in ('wood','iron','wool'))
                prior = sum(before[good] for good in ('wood','iron','wool'))
                if amount > prior:
                    assert xz(person['position'], self.fixture['home_pickup']) <= .8, ('freight pickup away from Hall', row)
                if amount < prior:
                    assert xz(person['position'], self.fixture['home']['shore']) <= .8, ('freight deposit away from shore', row)
        self.previous = row
        for event in set(events):
            if event not in {entry['event'] for entry in self.events}:
                self.events.append({'event': event, 'sample': row})
        return events

    def passed(self):
        return (self.carried and self.worked and self.provisions_collected and self.boarded and self.purchase and self.sailed and self.consigned
                and self.previous['routes'] and self.previous['routes'][0]['completed_trips'] >= 1
                and self.previous['routes'][0]['status'] == 'Idle'
                and self.previous['ships'][0]['wood'] == 0)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--server', type=Path, default=ROOT/'target/playtest/server')
    parser.add_argument('--client', type=Path, default=ROOT/'target/playtest/client')
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--resolution', default='1600x1000')
    parser.add_argument('--warp', type=int, choices=(1,25), default=1)
    parser.add_argument('--timeout', type=float, default=1800)
    args = parser.parse_args()
    out = args.out.resolve()
    if not out.is_relative_to(ROOT/'logs'):
        raise ValueError('Keep review artifacts under logs/')
    out.mkdir(parents=True, exist_ok=False)
    if args.client.resolve() != ROOT/'target/playtest/client':
        raise ValueError('Maintained startup helper uses target/playtest/client')
    report = {'passed': False, 'scenario': 'port-trade', 'warp': args.warp,
              'binaries': {label: binary_identity(path) for label,path in (('server',args.server),('client',args.client))},
              'captures': [], 'scope': 'Finite staged coastal Towns; real hull material/labour purchases, local hauling, paid captain boarding, water navigation and common town-market cargo. Not autonomous investment or performance acceptance.'}
    server = client = session = tracker = None
    try:
        port = claim_udp_port(args.port)
        env = environment(ROOT) | {'CITYSIM_MAP_ID':'village_lab','FISTWORLD_PORT_TRACE_DIR':str(out),'FISTWORLD_NATURAL_IMMIGRATION':'0','FISTWORLD_DEV':'1','FISTWORLD_SERVER_PORT':str(port)}
        for key in ('FISTWORLD_VILLAGE_LAB_RUNTIME','FISTWORLD_REALWORLD_LAB_RUNTIME','FISTWORLD_BRIDGE_TRACE_DIR'):
            env.pop(key,None)
        with (out/'server.log').open('w') as log:
            server = subprocess.Popen([args.server.resolve()],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT)
        wait_server(server,out/'server.log',lambda text:'Port trade fixture ready' in text,'real coastal port fixture',timeout=240)
        fixture = json.loads((out/'fixture.json').read_text())
        report['fixture'] = fixture
        client,session = start_client(ROOT,out/'client',args.resolution)
        menu(session); replace(session,'startup-server-field',f'127.0.0.1:{port}'); join(session,'PortQA')
        session.wait(lambda s:s.get('playing') and s.get('creator') and s.get('god_capability'),'creator and developer controls')
        session.command('button',name='creator-BEGIN JOURNEY')
        session.wait(lambda s:s.get('hero') and not s.get('cinematic') and not s.get('creator'),'ordinary opening finished',timeout=180)
        set_warp(session,args.warp)
        centre = fixture['home']['shore']
        session.command('view',x=centre[0],z=centre[2],zoom=75,yaw=.65)
        session.wait(lambda s:distance(s['camera']['focus'],centre)<1,'port camera settled')
        report['captures'].append(session.command('capture',name='00-port-construction'))
        (out/'start').touch()
        journal,tracker = Journal(out/'port.jsonl'),Acceptance(fixture)
        deadline = time.monotonic()+args.timeout
        captured = set()
        while time.monotonic()<deadline:
            if any(process.poll() is not None for process in (server,client)):
                raise RuntimeError('Owned game process exited')
            events = []
            for row in journal.read(): events.extend(tracker.observe(row))
            latest = tracker.previous
            if latest:
                label = next((label for label in ('hauling','building','provisioning','aboard','sailing','consigned') if label in events and label not in captured),None)
                if label:
                    captured.add(label)
                    if label == 'provisioning':
                        person = next((person for person in latest['people'] if 'CollectingProvisions' in (person.get('crew') or '')), None)
                        if person:
                            p = person['position']; session.command('view',x=p[0],z=p[2],zoom=32,yaw=.65)
                    elif latest['ships']:
                        p=latest['ships'][0]['position']; session.command('view',x=p[0],z=p[2],zoom=52,yaw=.65)
                    report['captures'].append({'name':label,'trigger':latest,'reply':session.command('record',name=label,frames=12,interval_ms=120,timeout=90)})
            if tracker.passed():
                p=fixture['home']['berth']; session.command('view',x=p[0],z=p[2],zoom=48,yaw=2.3)
                session.wait(lambda s:abs(s['camera']['zoom']-48)<.1,'finished ship inspection')
                report['captures'].append(session.command('capture',name='99-completed-returned-ship'))
                report['passed']=True
                break
            time.sleep(.05)
        if not report['passed']:
            raise TimeoutError(f'Port acceptance incomplete: {tracker.__dict__}')
        print('CONNECTED PORT ACCEPTANCE PASSED; inspect PNGs and capture JSONs',flush=True)
    except BaseException as error:
        report['error']=repr(error)
        raise
    finally:
        if tracker: report['evidence']={key:value for key,value in tracker.__dict__.items() if key!='fixture'}
        if session: report['last_client_state']=session.status()
        (out/'report.json').write_text(json.dumps(report,indent=2))
        stop(client); stop(server)


if __name__=='__main__':
    main()

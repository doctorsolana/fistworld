#!/usr/bin/env python3
"""Exercise global chat through two real clients, native typing and server delivery.

The local Village Lab supplies the world. This driver never fabricates chat
messages, network connections, heroes, inventory, or a server-assigned name.
"""

import argparse
import json
from pathlib import Path
import subprocess
import time

from first_session import Session, distance, environment, stop, wait_server
from multiplayer_session import claim_udp_port, start_client
from startup_session import join, menu, replace


class FocusedSession(Session):
    """Focus the actual native window before sending ordinary player input."""
    def command(self, action, **parameters):
        if action not in ('focus_window', 'quit') and not self.status().get('window_focused'):
            super().command('focus_window')
            self.wait(lambda s: s.get('window_focused'), 'native window focus')
        return super().command(action, **parameters)


def chat(state):
    return state.get('chat') or {}


def messages(state):
    return chat(state).get('messages', [])


def open_chat(session):
    if not chat(session.status()).get('open'):
        session.command('key', key='T')
    return session.wait(lambda s: chat(s).get('open'), 'focused chat composer')


def send(session, text):
    open_chat(session)
    session.wait(lambda s: not chat(s).get('pending'), 'previous send acknowledged')
    session.command('text', text=text, replace=True)
    session.wait(lambda s: chat(s).get('draft') == text, 'native draft text')
    session.command('key', key='Enter')
    session.wait(lambda s: not chat(s).get('open'), 'Enter sends and closes chat')


def received(session, sender, text):
    state = session.wait(lambda s: any(line['sender'] == sender and line['text'] == text
                                      for line in messages(s)), 'authoritative chat delivery')
    matching = [line for line in messages(state)
                if line['sender'] == sender and line['text'] == text]
    assert len(matching) == 1, 'duplicate chat line'
    return matching[0]


def run(args):
    root = Path(__file__).resolve().parents[1]
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=False)
    port = claim_udp_port(args.port)
    accounts = {'a': 'ChatAlice', 'b': 'ChatBram'}
    report = {'passed': False, 'port': port, 'accounts': accounts,
              'scope': 'Real local client/server Village Lab at 1x; native chat input and reliable delivery',
              'revision': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
              'captures': [], 'deliveries': []}
    server = None
    processes, sessions = {}, {}

    def save(name, value):
        (out / name).write_text(json.dumps(value, indent=2) + '\n')

    def capture(label, name):
        session = sessions[label]
        session.command('capture', name=name)
        metadata = json.loads((session.directory / f'{name}.capture.json').read_text())
        assert metadata['target'] == 'window'
        assert metadata['world']['building_lod_pending'] == 0
        assert metadata['world']['ground_paint_pending_chunks'] == 0
        report['captures'].append({'client': label, 'name': name,
                                   'dimensions': [metadata['width'], metadata['height']]})

    def exchange(label, text):
        send(sessions[label], text)
        echoes = [received(session, accounts[label], text) for session in sessions.values()]
        assert echoes[0] == echoes[1], 'clients disagree on the delivered message'
        report['deliveries'].append(echoes[0])

    started = time.monotonic()
    try:
        env = environment(root)
        env.update(CITYSIM_MAP_ID='village_lab', FISTWORLD_VILLAGE_LAB_RUNTIME='1',
                   FISTWORLD_LAB_SCENARIO='secure', FISTWORLD_LAB_WARP='1',
                   FISTWORLD_LAB_DAY_TWO_ARRIVALS='0', FISTWORLD_DEV='0',
                   FISTWORLD_SERVER_PORT=str(port))
        server_log = out / 'server.log'
        with server_log.open('w') as log:
            server = subprocess.Popen([root / 'target/debug/server'], cwd=root, env=env,
                                      stdout=log, stderr=subprocess.STDOUT)
        wait_server(server, server_log,
                    lambda text: f'Server UDP socket bound to 0.0.0.0:{port}' in text,
                    'owned chat lab server')
        for label, resolution in [('a', '1600x1000'), ('b', '1280x720')]:
            process, session = start_client(root, out / label, resolution)
            session = FocusedSession(session.directory)
            processes[label], sessions[label] = process, session
            save('processes.json', {'server': server.pid,
                                    **{key: process.pid for key, process in processes.items()}})
            menu(session)
            replace(session, 'startup-server-field', f'127.0.0.1:{port}')
            join(session, accounts[label])
            session.wait(lambda s: s.get('playing') and s.get('creator'), 'normal character creator')
            session.command('button', name='creator-BEGIN JOURNEY')
            state = session.wait(lambda s: s.get('hero') and not s.get('cinematic'), 'opening finished')
            assert not state['god']
            session.command('key', key='Home')
            hall = state['markets'][0]['position']
            session.command('view', x=hall[0], z=hall[2], zoom=65)
            session.wait(lambda s: distance(s['camera']['focus'], s['camera']['target']) < .3
                         and abs(s['camera']['zoom'] - 65) < .3, 'camera settled')

        a, b = sessions['a'], sessions['b']
        assert messages(a.status()) == messages(b.status()) == []
        a.wait(lambda s: not chat(s).get('root_visible'), 'idle chat completely hidden')
        capture('a', '00-idle-hidden')
        first = open_chat(a)
        assert first['chat']['draft'] == '', 'opening T leaked into the draft'
        capture('a', '00-open-empty')
        exchange('a', 'Meet me by the village hall — café!')
        capture('b', '01-received-collapsed-720p')
        exchange('b', 'On my way. I will bring the wood.')
        open_chat(a)
        capture('a', '02-open-conversation')

        # Exercise physical shortcut keys, not merely a pasted string.
        a.command('key', key='Escape')
        a.wait(lambda s: not s.get('gameplay_blocking'), 'chat released input')
        a.command('key', key='C')
        a.wait(lambda s: s.get('combat'), 'combat mode for shortcut regression')
        before = open_chat(a)
        target = before['camera']['target']
        selection = before['selection']
        for key in ('C', 'G', 'J', 'N', 'M', 'R', 'X', '1', '2'):
            a.command('key', key=key)
        a.command('hold_key', key='W', frames=45)
        typed = a.status()
        assert typed['combat'] and typed['hud_mode'] == 'Play'
        assert not typed['ui_blocking'] and typed['gameplay_blocking']
        assert typed['selection'] == selection
        assert distance(typed['camera']['target'], target) < .05, 'typing moved the camera'
        assert chat(typed)['draft'] == 'cgjnmrx12w'
        save('typing-guards.json', {'before': before, 'after': typed})
        capture('a', '03-combat-typing')
        a.command('key', key='Escape')
        closed = a.wait(lambda s: not chat(s).get('open') and not s.get('gameplay_blocking'),
                        'Escape closes only chat')
        assert closed['combat'] and chat(closed)['draft'] == 'cgjnmrx12w'
        a.command('key', key='C')
        a.wait(lambda s: not s.get('combat'), 'ordinary controls resumed')
        open_chat(a)
        a.command('text', text='café', replace=True)
        a.command('key', key='Backspace')
        a.wait(lambda s: chat(s).get('draft') == 'caf', 'UTF-8 backspace')
        a.command('text', text='é by the Hall')
        a.command('key', key='Enter')
        for session in sessions.values():
            received(session, accounts['a'], 'café by the Hall')

        exchange('b', 'The northern road reaches the meadow. We can meet there after the market closes, '
                 'then head back together before nightfall. There is room for another house beside the old mill.')
        open_chat(b)
        capture('b', '04-wrapped-history-720p')
        # Long native drafts and unbroken words must stay inside the field/panel.
        long_draft = ('Please meet me near the northern road after the market closes. '
                      'Bring bread and wood for the new houses, and leave room for the cart. '
                      'We can return together before nightfall and collect the rest tomorrow. '
                      'I will wait beside the old mill.')
        b.command('text', text=long_draft, replace=True)
        b.wait(lambda s: chat(s).get('draft') == long_draft, 'long draft entered')
        capture('b', '04b-long-draft-tail-720p')
        b.command('key', key='Home')
        capture('b', '04c-long-draft-start-720p')
        word = 'W' * 240
        exchange('b', word)
        capture('a', '04d-long-word-received-preview')
        open_chat(b)
        capture('b', '04e-long-word-history-720p')
        b.command('key', key='Escape')
        b.wait(lambda s: not chat(s).get('preview_visible'), 'closed messages faded', timeout=35)
        assert len(messages(b.status())) >= 4, 'fading erased scrollback'
        b.wait(lambda s: not chat(s).get('root_visible'), 'faded chat completely hidden')
        capture('b', '05-faded-history-retained')

        # Disconnect clears local history; broadcasts are not a persistent log.
        a.command('key', key='Escape')
        a.command('button', name='pause-DISCONNECT')
        menu(a)
        assert not messages(a.status()) and not chat(a.status()).get('open')
        send(b, 'I will wait here while you reconnect.')
        received(b, accounts['b'], 'I will wait here while you reconnect.')
        join(a, 'cHaTaLiCe')
        resumed = a.wait(lambda s: s.get('playing') and s.get('hero') is not None,
                         'ordinary reconnect')
        assert not resumed['creator'] and not resumed['cinematic']
        assert not messages(resumed), 'old connection history leaked into reconnect'
        exchange('a', 'Back in the village. Chat still works.')
        a.wait(lambda s: not chat(s).get('pending') and not chat(s).get('draft'),
               'recased account echo acknowledged')
        open_chat(a)
        capture('a', '06-reconnected-chat')
        a.command('key', key='Escape')
        a.wait(lambda s: not s.get('gameplay_blocking'), 'composer input released')
        a.command('key', key='Escape')
        a.command('button', name='pause-CONTROLS')
        capture('a', '07-controls-chat-shortcut')
        report['typing_guards'] = True
        report['reconnect_isolated'] = True
        report['passed'] = True
        print('CONNECTED CHAT PASSED', flush=True)
    finally:
        for label, session in sessions.items():
            save(f'last-{label}.json', session.status())
        for process in processes.values():
            stop(process)
        stop(server)
        report['elapsed_seconds'] = round(time.monotonic() - started, 2)
        save('report.json', report)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--port', type=int, default=0)
    run(parser.parse_args())

#!/usr/bin/env python3
"""Isolated real-network acceptance check. Builds are performed separately.
Starts a server on 3107, uses real Python AND TypeScript starters, restarts it,
then runs a full Swiss -> final tournament via authenticated WebSockets.
No credentials or fixtures survive the temporary directory.
"""
import hashlib
import json
import os
from pathlib import Path
import secrets
import signal
import subprocess
import sys
import tempfile
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'sdk' / 'python'))
from pokerdraft import Client, http_command, ProtocolError

URL = 'http://127.0.0.1:3107'
ADMIN = secrets.token_hex(32)


def get(path):
    with urllib.request.urlopen(URL + path, timeout=5) as response:
        result = json.load(response)
    assert result['ok'], result
    return result['result']


def wait_ready(process):
    for _ in range(100):
        if process.poll() is not None:
            raise AssertionError('Server exited unexpectedly')
        try:
            return get('/healthz')
        except OSError:
            time.sleep(.05)
    raise AssertionError('Server did not start')


def server(directory):
    environment = dict(os.environ, POKERDRAFT_DB=str(directory / 'smoke.db'), POKERDRAFT_BIND='127.0.0.1:3107', POKERDRAFT_AUCTION_MS='250', POKERDRAFT_SEATS='2', POKERDRAFT_DEMO='false', POKERDRAFT_ADMIN_TOKEN=ADMIN, RUST_LOG='error')
    process = subprocess.Popen([str(ROOT / 'target' / 'release' / 'pokerdraft-server')], cwd=ROOT, env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    wait_ready(process)
    return process


def stop(process):
    if process.poll() is None:
        process.send_signal(signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    assert process.returncode == 0, process.stderr.read().decode()


def main():
    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix='pokerdraft-smoke-') as temp:
        directory = Path(temp)
        service = server(directory)
        processes = []
        clients = []
        try:
            users = []
            for name in ('Python Guild', 'TypeScript Guild'):
                team = http_command(URL, 'create_team', name=name)
                bot = http_command(URL, 'create_bot', team['owner_token'], name='baseline-v1')
                users.append(dict(owner=team['owner_token'], token=bot['bot_token'], bot_id=bot['bot']['id']))
            # Check auth scopes on the real transport.
            try:
                http_command(URL, 'create_bot', users[0]['token'], name='forbidden')
                raise AssertionError('Bot credential used as owner')
            except ProtocolError as error:
                assert str(error) == 'owner_auth_required'
            for index, user in enumerate(users):
                environment = dict(os.environ, POKERDRAFT_URL=URL, POKERDRAFT_TOKEN=user['token'])
                argv = [sys.executable, 'sdk/python/bot.py', '--once'] if index == 0 else ['node', '--experimental-strip-types', 'sdk/typescript/bot.ts', '--once']
                processes.append(subprocess.Popen(argv, cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE))
            # Observe the complete first match. Starters must play, not just connect.
            deadline = time.monotonic() + 35
            completed = None
            while time.monotonic() < deadline:
                matches = get('/api/matches')['matches']
                completed = next((m for m in matches if m['status'] == 'complete'), None)
                if completed:
                    break
                time.sleep(.1)
            assert completed, 'Both SDK starters must finish a complete match'
            replay = get(f"/api/matches/{completed['id']}/replay")
            assert replay['snapshot']['finished']
            assert len([e for e in replay['events'] if e['kind'] in ('award', 'burn')]) == 52
            assert sum(p['score'] for p in replay['snapshot']['players']) > 0, 'Strategies must earn score'
            assert {e['data']['seat'] for e in replay['events'] if e['kind'] == 'award'} == {0, 1}, 'Both strategies must acquire cards'
            assert hashlib.sha256(bytes.fromhex(replay['snapshot']['seed'])).hexdigest() == replay['snapshot']['commitment']
            assert 'private_seed' not in json.dumps(replay) and 'bids' not in json.dumps(replay)
            verifier = ROOT / 'target' / 'release' / 'examples' / 'verify_replay'
            verified = subprocess.run([str(verifier)], input=json.dumps(replay).encode(), stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            assert verified.returncode == 0, verified.stderr.decode()
            forged = json.loads(json.dumps(replay))
            forged['snapshot']['players'][0]['score'] += 1
            assert subprocess.run([str(verifier)], input=json.dumps(forged).encode(), capture_output=True).returncode != 0
            for process in processes:
                out, err = process.communicate(timeout=5)
                assert process.returncode == 0, err.decode()
                assert b'score ' in out, out.decode()
            print('PASS: real Python and TypeScript agents, 52 auctions, rewards, sealed snapshots, seed commitment')
            processes.clear()
            # Initiate another match; a graceful restart must mark it aborted/unrated.
            for user in users:
                c = Client(URL)
                c.command('auth', token=user['token'])
                c.command('join')
                clients.append(c)
            for _ in range(50):
                active = get('/api/directory')['matches']
                if active:
                    break
                time.sleep(.05)
            assert active
            active_id = active[0]['id']
            ratings = get('/api/leaderboard')
            stop(service)
            for c in clients:
                c.close()
            clients.clear()
            service = server(directory)
            after = get(f'/api/matches/{active_id}/replay')
            assert after['status'] == 'aborted' and after['snapshot']['seed']
            assert ratings == get('/api/leaderboard')
            print('PASS: graceful restart, aborted seed disclosure, persistent credentials and unchanged ratings')
            # Run a tournament using same live agents. Waiting agents know they are reserved.
            cup = http_command(URL, 'admin_create_tournament', ADMIN, name='Smoke Cup', swiss_rounds=1, cut=2)['tournament']
            for user in users:
                http_command(URL, 'register_tournament', user['owner'], tournament_id=cup['id'], bot_id=user['bot_id'])
            for index, user in enumerate(users):
                environment = dict(os.environ, POKERDRAFT_URL=URL, POKERDRAFT_TOKEN=user['token'])
                argv = [sys.executable, 'sdk/python/bot.py'] if index == 0 else ['node', '--experimental-strip-types', 'sdk/typescript/bot.ts']
                processes.append(subprocess.Popen(argv, cwd=ROOT, env=environment, stdout=subprocess.PIPE, stderr=subprocess.PIPE))
            http_command(URL, 'admin_start_tournament', ADMIN, tournament_id=cup['id'])
            deadline = time.monotonic() + 90
            final = None
            while time.monotonic() < deadline:
                final = next(t for t in get('/api/tournaments')['tournaments'] if t['id'] == cup['id'])
                if final['state'] == 'complete':
                    break
                time.sleep(.1)
            assert final and final['state'] == 'complete', final
            assert final['champion'] in [u['bot_id'] for u in users]
            assert len(final['history']) == 2 and max(final['history'][-1]['wins']) == 2
            print('PASS: tournament registration locks, Swiss round, best-of-three final, champion and durable standings')
            # Hard crash recovery is distinct from graceful shutdown.
            for user in users:
                c = Client(URL)
                c.command('auth', token=user['token'])
                try:
                    c.command('join')
                except ProtocolError as error:
                    assert str(error) == 'already_active'
                clients.append(c)
            hard_live = []
            for _ in range(100):
                hard_live = get('/api/directory')['matches']
                if hard_live:
                    break
                time.sleep(.02)
            assert hard_live, 'Create an in-flight match before a hard kill'
            http_command(URL, 'admin_pause', ADMIN, paused=True)
            hard_ratings = get('/api/leaderboard')
            service.kill(); service.wait(timeout=5)
            service = server(directory)
            assert next(t for t in get('/api/tournaments')['tournaments'] if t['id'] == cup['id'])['champion'] == final['champion']
            restored_ratings=get('/api/leaderboard')
            assert restored_ratings == hard_ratings, {'before':hard_ratings,'after':restored_ratings}
            crashed = get(f"/api/matches/{hard_live[0]['id']}/replay")
            assert crashed['status'] == 'aborted' and crashed['history'][-1]['finished'] and crashed['snapshot']['seed']
            print('PASS: hard restart preserves completed tournament and ratings')
            print(json.dumps({'elapsed_seconds': round(time.monotonic() - started, 2), 'status': 'passed'}))
        finally:
            for process in processes:
                if process.poll() is None:
                    process.terminate()
                process.communicate(timeout=5)
            for c in clients:
                c.close()
            stop(service)


if __name__ == '__main__':
    main()

#!/usr/bin/env python3
"""Run: POKERDRAFT_TOKEN=... python3 sdk/python/bot.py

Edit decide_bid/best_hand in pokerdraft.py to build your own strategy.
Public snapshots are the only strategy input; secrets are never logged.
"""
import os
import sys
import time
from collections import deque
from pokerdraft import Client, ProtocolError, decide_bid, best_hand


def run(url, token, once=False):
    backoff = 1
    completed = set()
    completed_order = deque()
    while True:
        client = None
        try:
            client = Client(url)
            auth = client.command('auth', token=token)
            bot = auth['bot_id']
            if auth.get('active_match'):
                snapshot = client.command('watch', match_id=auth['active_match'])['match']
                client.pending.append({'type': 'match', 'match': snapshot})
            else:
                try:
                    client.command('join')
                except ProtocolError as error:
                    if str(error) not in ('already_active', 'queue_paused'):
                        raise
            print('Connected to arena.', flush=True)
            backoff = 1
            acted = set()
            last_join = 0.0
            while True:
                message = client.receive()
                if message.get('type') == 'heartbeat':
                    client.command('ping')
                if message.get('type') == 'directory' and not message.get('paused') and time.monotonic() - last_join > 3:
                    last_join = time.monotonic()
                    queued = any(p['id'] == bot for p in message['lobby']['bots'])
                    active = any(any(p['id'] == bot for p in m['players']) for m in message['matches'])
                    if not queued and not active:
                        try:
                            client.command('join')
                        except ProtocolError as error:
                            if str(error) not in ('already_active', 'queue_paused'):
                                raise
                if message.get('type') != 'match':
                    continue
                match = message['match']
                snapshot = match['snapshot']
                mid = match['id']
                seat = next((i for i, p in enumerate(snapshot['players']) if p['id'] == bot), None)
                if seat is None:
                    continue
                if snapshot['finished']:
                    if mid in completed:
                        continue
                    completed.add(mid)
                    completed_order.append(mid)
                    if len(completed_order)>128:
                        completed.discard(completed_order.popleft())
                    print(f"Match {mid[:8]} {match['status']}: score {snapshot['players'][seat]['score']}", flush=True)
                    if once:
                        return
                    try:
                        client.command('join')
                    except ProtocolError as error:
                        if str(error) not in ('already_active', 'queue_paused'):
                            raise
                    continue
                key = (mid, snapshot['round'])
                if key in acted:
                    continue
                acted.add(key)
                player = snapshot['players'][seat]
                hand = best_hand(player['cards'])
                if hand and (hand[1] >= 35 or len(player['cards']) >= 9):
                    try:
                        client.command('cash', match_id=mid, round=snapshot['round'], cards=hand[0])
                        player['cards'] = [c for c in player['cards'] if c not in hand[0]]
                        player['balance'] += hand[1]
                    except ProtocolError as error:
                        if str(error) not in ('stale_round', 'deadline_passed', 'invalid_cards', 'match_not_live'):
                            raise
                try:
                    client.command('bid', match_id=mid, round=snapshot['round'], amount=decide_bid(snapshot, seat))
                except ProtocolError as error:
                    if str(error) not in ('already_bid', 'stale_round', 'deadline_passed', 'match_not_live'):
                        raise
                if len(acted) > 1024:
                    acted = {key}
        except (OSError, ConnectionError, ProtocolError, ValueError) as error:
            if str(error) == 'bot_auth_required':
                raise SystemExit('Bot credential rejected. Create or rotate it in the workshop.')
            print(f'Reconnecting after {type(error).__name__} in {backoff}s.', file=sys.stderr, flush=True)
            time.sleep(backoff)
            backoff = min(15, backoff * 2)
        finally:
            if client:
                client.close()


if __name__ == '__main__':
    token = os.environ.get('POKERDRAFT_TOKEN')
    if not token:
        raise SystemExit('Set POKERDRAFT_TOKEN to a bot credential from your workshop.')
    run(os.environ.get('POKERDRAFT_URL', 'http://localhost:3000'), token, '--once' in sys.argv)

"""Poker Draft protocol v1 client. Python 3.10+, standard library only.

External bot code runs on your hardware. Never pass an owner token to a bot.
"""
from __future__ import annotations
import base64
import hashlib
import itertools
import json
import os
import socket
import ssl
import struct
import urllib.parse
import urllib.request
import uuid
from collections import deque
from collections import Counter


class ProtocolError(Exception):
    pass


class Client:
    """Blocking RFC6455 text client; bounded frames, TLS verification, ping/pong.

    Pending public updates are preserved while waiting for command receipts.
    Reconnect by constructing a fresh Client and authenticating again.
    """
    def __init__(self, url: str, timeout: float = 30):
        parsed = urllib.parse.urlsplit(url)
        if parsed.scheme not in ('http', 'https', 'ws', 'wss') or not parsed.hostname:
            raise ValueError('Use an http(s) or ws(s) arena URL')
        secure = parsed.scheme in ('https', 'wss')
        port = parsed.port or (443 if secure else 80)
        raw = socket.create_connection((parsed.hostname, port), timeout=timeout)
        self.socket = ssl.create_default_context().wrap_socket(raw, server_hostname=parsed.hostname) if secure else raw
        self.pending: deque[dict] = deque()
        self.buffer = bytearray()
        nonce = base64.b64encode(os.urandom(16)).decode()
        host = parsed.hostname + (f':{port}' if port not in (80, 443) else '')
        path = (parsed.path.rstrip('/') + '/ws') if parsed.path not in ('', '/', '/ws') else '/ws'
        request = f'GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {nonce}\r\nSec-WebSocket-Version: 13\r\n\r\n'
        self.socket.sendall(request.encode())
        while b'\r\n\r\n' not in self.buffer:
            data = self.socket.recv(4096)
            if not data:
                raise ProtocolError('WebSocket handshake closed')
            self.buffer.extend(data)
            if len(self.buffer) > 16384:
                raise ProtocolError('Handshake too large')
        headers, extra = self.buffer.split(b'\r\n\r\n', 1)
        self.buffer = bytearray(extra)
        lines = headers.decode('latin1').split('\r\n')
        if ' 101 ' not in lines[0]:
            self.close()
            raise ProtocolError(f'WebSocket refused: {lines[0]}')
        fields = {k.lower().strip(): v.strip() for k, v in (line.split(':', 1) for line in lines[1:] if ':' in line)}
        expected = base64.b64encode(hashlib.sha1((nonce + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest()).decode()
        if fields.get('sec-websocket-accept') != expected or fields.get('upgrade', '').lower() != 'websocket':
            self.close()
            raise ProtocolError('Invalid WebSocket handshake')

    def _read(self, size: int) -> bytes:
        while len(self.buffer) < size:
            part = self.socket.recv(max(4096, size - len(self.buffer)))
            if not part:
                raise ConnectionError('Arena disconnected')
            self.buffer.extend(part)
        result = bytes(self.buffer[:size])
        del self.buffer[:size]
        return result

    def _send_frame(self, opcode: int, data: bytes) -> None:
        if len(data) > 8192:
            raise ProtocolError('Command too large')
        mask = os.urandom(4)
        head = bytes([0x80 | opcode])
        head += bytes([0x80 | len(data)]) if len(data) < 126 else bytes([0xFE]) + struct.pack('!H', len(data))
        self.socket.sendall(head + mask + bytes(v ^ mask[i % 4] for i, v in enumerate(data)))

    def send(self, message: dict) -> None:
        self._send_frame(1, json.dumps(message, separators=(',', ':')).encode())

    def _receive(self) -> dict:
        fragments = bytearray()
        fragmented = False
        while True:
            first, second = self._read(2)
            final, opcode = bool(first & 128), first & 15
            if first & 112 or second & 128:
                raise ProtocolError('Unsupported WebSocket frame')
            size = second & 127
            if size == 126:
                size = struct.unpack('!H', self._read(2))[0]
            elif size == 127:
                size = struct.unpack('!Q', self._read(8))[0]
            if size > 2_000_000 or len(fragments) + size > 2_000_000:
                raise ProtocolError('Arena message too large')
            payload = self._read(size)
            if opcode == 8:
                raise ConnectionError('Arena closed the connection')
            if opcode == 9:
                self._send_frame(10, payload)
                continue
            if opcode == 10:
                continue
            if opcode not in (0, 1) or (opcode == 0 and not fragmented) or (opcode == 1 and fragmented):
                raise ProtocolError('Unexpected WebSocket opcode')
            fragments.extend(payload)
            fragmented = not final
            if final:
                return json.loads(fragments.decode('utf-8'))

    def receive(self) -> dict:
        return self.pending.popleft() if self.pending else self._receive()

    def command(self, op: str, *, request_id: str | None = None, **fields) -> dict:
        rid = request_id or str(uuid.uuid4())
        self.send(dict(op=op, id=rid, **fields))
        while True:
            message = self._receive()
            if message.get('type') == 'ack' and message.get('id') == rid:
                if not message.get('ok'):
                    raise ProtocolError(message.get('error', 'command_rejected'))
                return message['result']
            self.pending.append(message)
            if len(self.pending) > 512:
                raise ProtocolError('Consumer too slow; reconnect for a fresh snapshot')

    def close(self):
        self.socket.close()


def http_command(url: str, op: str, token: str = '', **fields) -> dict:
    data = json.dumps(dict(op=op, id=str(uuid.uuid4()), **fields)).encode()
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    request = urllib.request.Request(url.rstrip('/') + '/api/command', data=data, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=10) as response:
            result = json.load(response)
    except urllib.error.HTTPError as error:
        result = json.load(error)
    if not result.get('ok'):
        raise ProtocolError(result.get('error', 'http_failed'))
    return result['result']


def evaluate(cards: tuple[int, ...] | list[int]) -> tuple[tuple, int]:
    """Independent readable evaluator for reference strategies, not the server."""
    counts = Counter(c % 13 + 2 for c in cards)
    groups = sorted(((count, rank) for rank, count in counts.items()), reverse=True)
    ranks = sorted(counts, reverse=True)
    flush = len({c // 13 for c in cards}) == 1
    straight = ranks[0] if len(ranks) == 5 and ranks[0] - ranks[-1] == 4 else 5 if ranks == [14, 5, 4, 3, 2] else 0
    if flush and straight:
        return (8, straight), 600 if straight == 14 else 400
    if groups[0][0] == 4:
        return (7, *(r for _, r in groups)), 200
    if [g[0] for g in groups] == [3, 2]:
        return (6, *(r for _, r in groups)), 110
    if flush:
        return (5, *ranks), 75
    if straight:
        return (4, straight), 55
    category = 3 if groups[0][0] == 3 else 2 if [g[0] for g in groups[:2]] == [2, 2] else 1 if groups[0][0] == 2 else 0
    return (category, *(r for _, r in groups)), [2, 8, 20, 35][category]


def best_hand(cards: list[int]) -> tuple[list[int], int] | None:
    if len(cards) < 5:
        return None
    combination = max(itertools.combinations(cards, 5), key=lambda hand: evaluate(hand)[0])
    return list(combination), evaluate(combination)[1]


def decide_bid(snapshot: dict, seat: int) -> int:
    player, card = snapshot['players'][seat], snapshot['card']
    cards = player['cards']
    if card is None or len(cards) >= 10:
        return 0
    pairs = sum(c % 13 == card % 13 for c in cards)
    suited = sum(c // 13 == card // 13 for c in cards)
    neighbors = sum(abs(c % 13 - card % 13) <= 2 for c in cards)
    value = 2 + pairs * 5 + suited + neighbors + (snapshot['round'] + seat * 7) % 4
    hand = best_hand(cards + [card])
    if hand:
        value += hand[1] // 5
    return min(value, player['balance'] // 3 + 1, player['balance'])

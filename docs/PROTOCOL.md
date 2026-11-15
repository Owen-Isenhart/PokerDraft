# Bot/API protocol v1

Server root: `http://localhost:3000`; use HTTPS/WSS at an external reverse proxy. All credentials are random 256-bit bearer secrets. Owner and bot roles are separate. No tokens in URLs. Public routes require no authentication.

## WebSocket

Connect `/ws`. One JSON object per text message, <=8192 bytes. Server initially sends a `directory` with protocol=1, clock time, rolling lobby and live table summaries. Send an auth message before participant commands:

```json
{"op":"auth","id":"your-unique-uuid","token":"BOT_CREDENTIAL"}
```

Receipt:

```json
{"type":"ack","id":"your-unique-uuid","ok":true,"result":{"bot_id":"uuid","active_match":null}}
```

Commands:

| op | Additional fields | Role |
| --- | --- | --- |
| `auth` | `token` | bot |
| `join` | none | bot |
| `leave` | none; removes queue reservation only | bot |
| `bid` | `match_id`, zero-based `round`, integer `amount` | seated bot |
| `cash` | `match_id`, zero-based `round`, `cards`: exactly five distinct owned IDs | seated bot |
| `watch` | `match_id` | public |
| `ping` | none | public |

Every command needs a nonempty unique string `id`, max 100 bytes. `watch` returns a current complete public match object in its receipt; later `match` broadcasts update the watched table. Authenticated bots automatically receive their own matches even without `watch`. On reconnect, authenticate again and watch the auth response's `active_match`, if present. Queue reservations survive a disconnect but not a server restart. Tournament registration survives both.

`directory`: `{type,protocol,server_time_ms,rules,paused,lobby:{bots,deadline_ms},matches,teams,registered_bots}`. Match summaries contain IDs, round and public seat identity (not future cards). Directory updates happen on lobby/match lifecycle changes, not on each bid. Its round is advisory; use full match snapshots for bidding.

`match`: `{type:"match",match:{id,status,deadline_ms,server_time_ms,snapshot,events}}`. Status is `live`, `complete` or `aborted`. The events array is the latest 12 public events, newest first. Use `seq` to de-duplicate and HTTP replay to recover the complete history. `snapshot` includes:

```text
rules_version: "1"
rules: {seats, auction_ms, pricing: "second_price"|"first_price"}
round: 0..52
card: 0..51 while live, null on completion or abort
players: [{id,name,house,balance,score,cards:[cardIDs]}]
pot: integer
finished: boolean
commitment: hex SHA256(seed)
seed: null during live play; hex 32-byte seed afterwards
seq: monotonically increasing public event sequence
```

Card ID = `13 * suit + rankIndex`. Suits: clubs=0, diamonds=1, hearts=2, spades=3. Rank index: 2=0 … T=8, J=9, Q=10, K=11, A=12. A five-card hand rank is an integer with category followed by five four-bit kickers; larger wins. Rewards and showdown semantics are specified in RULES.md.

`heartbeat` includes server clock time every 20s. Send `ping` at least every 30s; sockets idle from the client for 60s close. Browser/native WebSocket control ping/pong is also supported. Max 30 inbound application frames/s per socket; 2048 concurrent sockets total. Slow consumers are disconnected on a 3s send timeout or broadcast lag and must reconnect for a snapshot.

Accepted bids are private and volatile until settlement. Receipt discloses only acceptance/round to its sender. Exactly one immutable bid, including an explicit zero pass, is accepted per seat/round. Deadline is checked on admission to the serialized competition service using a monotonic clock. Late or stale messages are rejected even if your local clock differs. `server_time_ms`/`deadline_ms` are display/estimation aids; never wait for the last millisecond. A new full bidding window starts after durable settlement. Timing windows are not RTT compensation.

Successful authenticated commands have a bounded 8192-entry in-memory retry receipt cache, keyed by credential hash and request ID. Repeating an ID with identical JSON payload returns its prior receipt; changing payload yields `request_id_reused`. Rotate/revoked credentials are revalidated before cache access. Cross-restart or cache-evicted retries do not guarantee receipt replay: snapshots, one-bid validation and consumed-card ownership protect gameplay. Cash-ins and settlement are durable before public notification. Interrupted live games are aborted, never resumed with potentially missing bids.

Typical rejection strings: `bot_auth_required`, `owner_auth_required`, `admin_auth_required`, `already_active`, `team_already_active`, `queue_paused`, `match_not_live`, `not_seated`, `stale_round`, `deadline_passed`, `already_bid`, `insufficient_balance`, `inventory_full`, `invalid_cards`, `invalid_amount`, `request_id_reused`, `server_busy`. No errors disclose another bidder's action. Send a new request ID after a validation failure. A bot reserved in a tournament gets `already_active` on arena joins and should wait for its assigned match.

## HTTP

`GET /healthz`, `/metrics` (Prometheus text), `/api/directory`, `/api/bots`, `/api/leaderboard`, `/api/tournaments`, `/api/matches?offset=0`, `/api/matches/{id}`, `/api/matches/{id}/replay`.

`GET /api/bots/{id}/matches?offset=0` and `/api/teams/{id}/matches?offset=0` return public version/team match history, 50 records per page.

JSON responses: `{ok:true,result:...}` or `{ok:false,error:"..."}`. HTTP parser/body-limit errors may use framework text responses. `/api/matches` returns 50 newest records, offset pagination. Replay includes full public `events`, `history` snapshots and timestamps, never the private DB seed column. Live replay has no seed disclosure. Archived match results include seeds. Leaderboard excludes house agents, disabled teams and versions with no rated games.

POST `/api/command`, `Content-Type: application/json`, body `{op,id,...fields}`. Privileged operations use `Authorization: Bearer CREDENTIAL` (the `token` body field is only meaningful to WS auth). The bot command subset also works over this HTTP endpoint, but persistent WS is preferred.

| op | Fields | Role |
| --- | --- | --- |
| `create_team` | `name` (2–40 characters, unique case-insensitively) | public |
| `account` | none; returns team identity and versions | owner |
| `create_bot` | `name` (unique within team) | owner |
| `rotate_bot` | `bot_id`; returns `bot_token`, immediately revokes previous token | owner |
| `rotate_owner` | none; returns `owner_token`, immediately revokes previous token | owner |
| `register_tournament` | `tournament_id`, `bot_id` | owner |
| `unregister_tournament` | same, registration phase only | owner |
| `admin_create_tournament` | `name`, `swiss_rounds`:1–12, `cut`:2,4,8,16,32 | operator |
| `admin_start_tournament` | `tournament_id`, at least cut entrants required | operator |
| `admin_cancel_tournament` | `tournament_id`; aborts live pairings | operator |
| `admin_pause` | `paused`:boolean; active matches still complete | operator |
| `admin_abort_match` | `match_id`; no rating update, tournament pairing rescheduled | operator |
| `admin_disable_team` | `team_id`, `disabled`:boolean; disabling aborts team matches, removes queue entries, cancels its unfinished tournaments | operator |

Create-team result contains `team` and one-time `owner_token`; create-bot result contains public `bot` and one-time `bot_token`. Tokens are stored as SHA-256 digests in SQLite, never plaintext. There is no email/password provider. Losing an owner credential loses self-service access: save it offline. A bot credential can always be rotated by its owner. Do not publish credentials or put them in bot source.

HTTP mutations are limited to 600/min and 5 team creations/min per socket peer IP. WS handshakes limited to 30/min per peer IP. Forwarded IP headers are deliberately untrusted. For external hosting, enforce public-client limits at your reverse proxy as well; otherwise all clients behind it share these limits. Cross-origin browser mutation/WS requests require same Host origin or an exact `POKERDRAFT_ALLOWED_ORIGINS` entry. There is no wildcard CORS credential access.

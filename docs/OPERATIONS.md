# Local operation and future homelab constraints

## Prerequisites and launch

Rust 1.90+ with cargo/rustfmt/clippy, Node 22.12+ and npm, Python 3.10+ for the starter/tools. No paid accounts or external database required. Install from the official language/package sources.

```sh
npm ci --prefix web
npm run build --prefix web
cargo build --release --workspace
cargo build --release --workspace --examples
# Run from repository root so default web/dist and data/ paths resolve.
./target/release/pokerdraft-server
```

Open `http://localhost:3000`. Default: one-second auctions, six seats, thirty-second rolling lobby and a clearly labeled house demonstration whenever no other match is running. A queued human bot starts on lobby timeout with house padding or immediately when full. House-only and one-real-bot games are not rated. Connect at least two real bot versions from different teams for a rated game.

For development, `make dev` runs the Rust server and Vite on port 5173, proxying API/WS to 3000. Edit web assets with hot reload; restart the server after Rust changes. Production uses a single port and no Vite process.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `POKERDRAFT_BIND` | `127.0.0.1:3000` | listener; bind loopback for private local use |
| `POKERDRAFT_DB` | `data/pokerdraft.db` | SQLite database path |
| `POKERDRAFT_WEB` | `web/dist` | compiled assets path |
| `POKERDRAFT_SEATS` | `6` | arena seats 2–8; tournaments always heads-up |
| `POKERDRAFT_AUCTION_MS` | `1000` | 250–10000 ms; tournament and arena windows |
| `POKERDRAFT_PRICING` | `second_price` | `first_price` selects first-price experiment |
| `POKERDRAFT_DEMO` | `true` | `false` disables automatic all-house demos |
| `POKERDRAFT_ADMIN_TOKEN` | unset | operator control disabled unless configured; at least 32 characters |
| `POKERDRAFT_METRICS_TOKEN` | unset | metrics disabled unless separately configured; bearer secret at least 32 characters |
| `POKERDRAFT_ALLOWED_HOSTS` | `localhost,127.0.0.1,[::1]` | accepted Host names; configure public domain explicitly |
| `POKERDRAFT_ALLOWED_ORIGINS` | empty | comma-separated exact additional browser origins |
| `RUST_LOG` | service info | tracing filter |

Use a random operator token, for example `python3 -c 'import secrets; print(secrets.token_hex(32))'`. Set it in your local process environment or future k3s Secret; do not commit it. Operator console is at `/#operator`. No configured admin means public play/team management works but operator mutations are unavailable. Tokens passed to external bot starters use `POKERDRAFT_TOKEN` and `POKERDRAFT_URL`, never the operator credential.

Serve external users through free TLS tooling such as your existing homelab ingress. Set the public origin explicitly and preserve WebSocket upgrades. Do not disable certificate validation. Set proxy request/body/socket/rate limits based on real client IP; the application ignores spoofable X-Forwarded-For. Multiple users through one proxy otherwise share application peer limits. Public APIs expose public competition data. `/metrics` requires its own configured bearer token; proxy restrictions can further limit access.

## Persistence and restart

Run one authoritative instance. Use a local POSIX volume supporting SQLite WAL/file locking and fsync; never NFS/SMB or multiple independent writers. Match results, ratings and tournament progression are one transaction. The private DB has hashed credentials and undisclosed live seeds; protect file/volume permissions. The process needs write access to the database directory for WAL/SHM files. Application traces omit credentials and request bodies.

SIGINT/SIGTERM aborts current matches, discloses their seeds, leaves ratings unchanged and preserves tournament reservations for replay on restart. A hard kill is recovered the same way at next startup. Queue reservations and receipt cache are ephemeral. Agents reconnect/authenticate and rejoin automatically. Pausing new matches is durable and does not pause active rounds. Tournament completion and cancellation release reservations.

`GET /healthz` checks actor responsiveness and exposes only status/protocol, active matches and scheduler lag. `/metrics` is disabled (404) without `POKERDRAFT_METRICS_TOKEN`; when configured, send `Authorization: Bearer <metrics token>`. It exposes command/rejection totals, active matches and maximum observed scheduler lag in milliseconds. Compare lag and command acknowledgement latency with the configured bidding window. Storage errors fail the actor closed; inspect logs and disk capacity, then restart after fixing the underlying error. No automatic silent fallback to memory.

Schema v2 is migrated automatically in a transaction. Take a verified backup before upgrading, stop every older binary first, and do not downgrade against the migrated file; restore the pre-upgrade backup with the matching older binary if rolling back. Unknown future schemas fail closed. Archived reads use a separate read-only connection; only one process owns competition writes.

## Backup and restore

```sh
python3 scripts/backup.py data/pokerdraft.db /your/backups/pokerdraft-2026-11-15.db
```

This uses SQLite's online backup API and integrity check, creates a new mode-0600 file and refuses to overwrite. Never back up only a live main DB file while ignoring WAL. Retain periodic backups on your homelab storage; there is no paid backup dependency.

To restore: stop the server, preserve the existing database plus WAL/SHM as a rollback copy, put the verified backup at the configured database path with its correct owner, remove *stale* WAL/SHM associated with the replaced DB, then start one server. Verify `/healthz`, team authentication, archive and tournament standings. Any live games in the restored backup are aborted at startup. Never restore while a server has the DB open.

## Validation and performance

```sh
make profile                   # isolated storage/auth/fan-out fixture; see review/OPTIMIZATION.md
make security                  # isolated real-network security checks
make check                     # engine/service tests (including exhaustive evaluation), clippy, web build
make browser                   # requires running local server and Chromium
python3 scripts/smoke.py        # isolated real bots, complete games, tournament and restart checks
cargo run --release -p pokerdraft-engine --example simulate -- 2000
node scripts/load.mjs 20 5     # running server; WS receipt latency + selected-table fan-out
```

The load script respects the default 30-handshakes/minute peer limit when used with <=20 clients on a fresh minute. Increasing client counts requires a proxy with appropriate per-client routing or adjusting service limits in code; refused handshakes count as errors and invalidate capacity claims. A loopback ping measures network/WS handling, not durable bid latency. Use the smoke runner and real bots to validate game progress. See VALIDATION.md for measured results and limits.

## CI cost control

The repository is public. CI uses only GitHub's standard `ubuntu-latest` runner, which is free for public repositories ([GitHub billing documentation](https://docs.github.com/en/billing/concepts/product-billing/github-actions)). The job is explicitly skipped if the repository becomes private, and does not upload artifacts or configure billable caches. All checks are also runnable locally with `make check` and the smoke script; no CI subscription is required.

## Replay audit

```sh
curl -fsS http://localhost:3000/api/matches/MATCH_ID/replay | cargo run --release -p pokerdraft-engine --example verify_replay
```

After a completed or aborted match, the verifier checks the committed shuffle, every dealt card, owned-card consumption, best showdown/liquidation choices, pot splits, balances and final scores. It deliberately cannot prove private bid allocation or a dishonest operator's initial seed selection. Save the pre-match commitment independently if operator tampering is in your threat model.

## Security review changes

Read [the complete review](review/SECURITY.md). The listener defaults to loopback; set the bind address and accepted public host explicitly for ingress. Owner credentials are no longer persisted in browser storage. Public registration/owner/operator operations use HTTP only; WebSocket accepts bot gameplay, auth, watch and ping. Use HTTPS/WSS outside loopback; SDK plaintext development requires `POKERDRAFT_ALLOW_INSECURE=true`. API responses are not cacheable. A second authoritative process cannot lease the same database. Three consecutive drawn tournament games suspend it for operator attention/cancellation, without declaring a winner.

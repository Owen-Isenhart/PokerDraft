# Release validation — 2026-11-15

Validation ran on the managed Linux x86-64 workspace (three available logical CPUs), Rust 1.100 stable, Node 24.22.1, Python 3.12.14 and system Chromium. Application minimum Rust is 1.90; tests used the installed stable toolchain. These are local results, not a production capacity guarantee.

## Correctness and end-to-end behavior

| Check | Result |
| --- | --- |
| Rust formatting and `cargo clippy --workspace --all-targets -- -D warnings` | Pass |
| 11 engine integration tests | Pass |
| Exhaustive five-card evaluation | All **2,598,960** combinations; exact nine-category frequency counts and **7,462** unique ranks |
| Seeded full-match invariants | 300 complete games, no duplicate cards or inventory overflow; exactly 52 auctions |
| Auction and economy cases | First/second price, sealed snapshots, immutable bids, invalid-input atomicity, wheel/kickers, inventory cap, all-pass termination, seed disclosure |
| Tournament engine | Odd Swiss field/byes, draw replay, top cut, elimination and best-of-three final |
| 10 service tests | Scoped credentials and rotation, anonymous owner-secret replay rejection, private receipt handling, deadline enforcement, queue/tournament exclusivity, atomic durable results/ratings, restart aborts, tournament pairing recovery, public version/team history, disabling a team aborts every affected live pairing |
| 3 independent Python strategy tests | Pass |
| Production TypeScript/React build | Pass; ~81.1 kB gzip JS and ~6.8 kB gzip CSS |
| 2 Chromium browser flows | Pass at 1440px desktop and 390px mobile; no horizontal overflow or uncaught page errors; live table, workshop/team/version credentials, locking, rankings/history, tournaments, docs anchors, completed replay playback/scrubbing and revealed seed |
| Actual Python + TypeScript external bots | Both connect/authenticate, buy cards, cash hands, finish a 52-card game and auto-reconnect/rejoin |
| Actual tournament over WS | Registered agents reserved from arena queue, Swiss played, best-of-three final completed, champion/standings persisted |
| Graceful restart | In-flight match visibly aborted, seed disclosed, credentials/results/ratings preserved |
| Hard restart | Active match recovered as aborted, terminal replay snapshot present, no duplicate rating, completed tournament preserved |
| Replay audit | Committed shuffle, dealt cards, hand ownership/consumption, best showdown hands, payout splits and final accounting verified; deliberate score corruption rejected |
| Online backup | SQLite backup API copied a live database containing 17 matches; integrity check `ok` |
| npm dependency audit | 0 known vulnerabilities for installed production **and development** dependency tree at validation time |

The isolated real-network smoke suite completed in 55.49 seconds on one run (68.23 seconds on another, because a final needed its third decisive game). Match seeds are random; the suite permits replayed draws and checks invariants rather than assuming a specific champion. Test credentials exist only in a temporary database and process environment.

## Measured performance

Command:

```sh
cargo run --release -p pokerdraft-engine --example simulate -- 10000
```

10,000 six-agent matches, 520,000 auctions and 60,831 cash-ins ran in **2,498 ms**. Best-of-ten evaluation averaged **5,720 ns** over 10,000 iterations in that run. This includes the transparent reference strategy, not an advanced search agent. Earlier 2,000-match run: 476 ms, best-of-ten 6,293 ns. No hand-evaluator or game throughput claim is made for different CPUs/build profiles.

Command against the actual running service, default one-second demo tables:

```sh
node scripts/load.mjs 20 5
```

| Measurement | p50 | p95 | p99 |
| --- | ---: | ---: | ---: |
| WS ping receipt, 800 samples | 0.657 ms | 1.919 ms | 6.161 ms |
| Serialized service snapshot read over WS, 180 samples | 2.175 ms | 5.914 ms | 7.143 ms |

20 clients, five seconds, 1,126 received messages, 691,575 bytes, **zero errors**. These connections watched a real live table while receiving command receipts. Ping is handled by networking; watch receipts include the competition actor. Neither figure represents durable mutation/fsync latency or an intercontinental RTT. A prior comparable run had service-read p95 4.712 ms and zero errors. House/timer work and end-to-end gameplay were verified separately by the real-agent smoke suite.

## Remaining operational limits

Deployment, TLS/ingress and Proxmox/k3s manifests are intentionally deferred. The application is one authoritative process with one local SQLite WAL database, 16 simultaneous match slots and a 512-bot rolling queue. It does not claim geographically equal latency or thousands of measured active bots. Account metadata and public replay reads share the writer service; large archives or identity counts should be profiled before increasing defaults. Database retention is operator-managed; turn demos off if continuous demonstration archives are unwanted.

The operator is trusted with seeds and sealed bids. Public replay can verify economic transitions and the committed shuffle, not private bid allocation or operator seed selection. Open registration does not establish unique human identities or prove absence of collusion. Tournament draws replay; persistently inactive/tied entrants require cancellation by the operator. The economy is an explicit configurable-by-code game design, not a mathematically proven balanced equilibrium; the simulator supports future strategy/economy studies.

No paid runtime, API, hosted identity or model is required. The public repository's CI uses standard free Linux runners, skips private repositories and uploads no artifacts/caches. Local validation remains available independently of GitHub Actions.

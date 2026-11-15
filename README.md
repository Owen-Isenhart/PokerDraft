# Poker Draft

A free, self-hosted arena where external bots bid for playing cards, cash in poker hands and compete in periodic showdowns. Inspired by [Scrabblebot](https://github.com/clockworklabs/scrabblebot). Original implementation, MIT licensed. Fictional points only.

- **Real-time arena:** one-second sealed auctions, first- or second-price payment, a ten-card inventory, cash-ins and four automatic showdowns across a 52-card deck.
- **Complete competition:** rolling lobbies with house padding, team/version credentials, multiplayer Elo, Swiss → top cut → best-of-three final tournaments, operator controls and public replays.
- **Participant tools:** zero-dependency Python and TypeScript bot starters, deterministic simulator, exact poker evaluator, documented HTTP/WebSocket protocol and shuffle verification.
- **Self-hosted:** Rust/Axum service, SQLite WAL, React/TypeScript UI. No paid API, model, identity provider, managed database, analytics service or GPU required. No participant code executes on the server.

## Run locally

Requires Rust 1.90+, Node 22.12+/npm and optionally Python 3.10+.

```sh
npm ci --prefix web
make run
```

Open **http://localhost:3000**. The house demonstration plays immediately. Open **My workshop** to create a team and bot version, then connect your bot:

```sh
export POKERDRAFT_URL=http://localhost:3000
export POKERDRAFT_TOKEN='<bot credential from your workshop>'
python3 sdk/python/bot.py
# Or, with Node 22+:
node --experimental-strip-types sdk/typescript/bot.ts
```

Both starters reconnect and rejoin automatically. Two real teams are needed for rated matches; a single bot can practice against the house. Edit `decide_bid` / `decideBid` and cash-in selection to experiment. `make dev` runs the Rust server and Vite for development.

Set a random 32+ character `POKERDRAFT_ADMIN_TOKEN` before starting the server to enable `/#operator`. The operator can create/start/cancel tournaments, pause the queue, abort matches and disable teams. Participant team management never requires operator access. Save your owner credential offline; tokens are stored only as hashes and cannot be recovered from the database.

## Read next

[Exact rules](docs/RULES.md) · [Protocol/API](docs/PROTOCOL.md) · [Architecture](docs/ARCHITECTURE.md) · [Operation/backup/configuration](docs/OPERATIONS.md) · [Research review](docs/RESEARCH.md) · [Full implementation plan](docs/PLAN.md) · [Validation](docs/VALIDATION.md) · [Delivery status](docs/STATUS.md)

Review follow-up: [Ideas for review](docs/review/IDEAS.md) · [Security findings and fixes](docs/review/SECURITY.md) · [Optimization measurements](docs/review/OPTIMIZATION.md).

```sh
make check
make browser                 # local server + Chromium required
python3 scripts/smoke.py      # isolated real agents, games, tournament, restart
make simulate
node scripts/load.mjs 20 5   # running local server
```

Deployment is intentionally deferred. One process serves assets/API/WebSockets with one local durable database, suitable for later single-replica homelab packaging on Proxmox/k3s. Do not share a SQLite WAL file over NFS or run independent authoritative replicas against it. See operations and architecture for the scaling boundary.

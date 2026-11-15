# Implementation plan and acceptance contract

Deliver a complete self-hosted bot competition application, rather than a mock UI or prototype. Infrastructure deployment is deferred; a single binary plus static assets and durable local volume fits a future single-replica Proxmox/k3s workload.

1. **Research and specification** — retain the supplied report, correct unsupported claims, specify economy, timing, visibility, fairness and tournament rules. Commit/push this checkpoint.
2. **Deterministic engine** — exact evaluator, seeded shuffle and ties, bounded inventories, sealed first/second-price auctions, transactional cash-in, automatic showdowns, scores, final liquidation, reproducible simulation. Verify all 2,598,960 five-card category counts and randomized match invariants. Commit/push engine checkpoint.
3. **Durable competition service** — Rust async WebSocket/API, owner/bot credentials with hashed storage and rotation/revocation, named team versions, rolling lobby, built-in agents, bounded live matches and queues, idempotent commands, deadlines, public replay, ratings, restart abort policy, metrics and health. SQLite migrations and atomic result/rating writes. Commit/push service checkpoint.
4. **Tournament system** — registration, Swiss standings/pairings/byes/Buchholz, top cut, elimination, best-of-three finals, persistent recovery and operator management. Test tie/restart/abort cases. Commit/push checkpoint.
5. **Usable spectator and participant app** — live auction and seats, selectable matches and replay, leaderboard and team history, account/team/version/token controls, tournaments and operator controls, bot documentation, reconnect states, keyboard and mobile accessibility. Real service integration; no fabricated dashboards. Commit/push checkpoint.
6. **Participant tools and operations** — Python and TypeScript bot starters, reference strategy, local training/simulation, protocol docs, free local development commands, load/benchmark scripts, CI, backup/restore and future k3s constraints. No deployment to external infrastructure. Commit/push checkpoint.
7. **Release validation** — engine and service tests, exhaustive evaluator, seeded simulation study, protocol tests including privacy/auth/deadlines/retry, production web build, browser flow, actual bot smoke and persistence/restart test, benchmark baseline. Record results and limitations rather than claiming unmeasured scale. Push final state and describe how to run.

## Boundaries

No real money; no remote bot code execution or arbitrary webhook URLs. External agents run on participant hardware. No paid dependencies. Advanced machine-learning agents, geographically distributed consensus and production cluster deployment are not required platform features. The platform must support arbitrary external strategies through a documented protocol. Operator authorization is a configured secret, not a UI-only gate. User data and tokens must never be committed.

## Delivery tracking

Each checkpoint updates `docs/STATUS.md` with actual work, validation and remaining gaps. `work` is the implementation branch. Push after meaningful checkpoints and before stopping; do not rewrite remote history.

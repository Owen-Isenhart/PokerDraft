# Architecture

```text
External Python / TypeScript / arbitrary-language bots
                         │ private authenticated commands
React spectator/workshop ├──────── HTTP + persistent WebSocket
                         ▼
                    Axum / Tokio
           bounded ingress, scoped tokens, origin checks
                         │ bounded mpsc (2048)
                         ▼
            single competition writer thread
      rolling queue · match timers · tournament scheduler
                         │
           deterministic Rust game/rating engine
                         │
                 SQLite WAL, FULL sync
          public snapshots/replays + private credentials/seeds
                         │ only after commit
                         ▼
               bounded public broadcast feed
         subscribed table / own matches / directory
```

No browser state decides winners. No network call or participant code executes inside the game engine. Bot strategies run externally. A server-owned reference strategy fills house seats using only public snapshots. TLS is terminated by the future homelab reverse proxy.

The engine does not implement `Serialize` for private game state. Explicit public snapshots exclude bids and the future deck. Even persistence replay queries deliberately omit the private seed column. SHA-256 seed commitments precede play; ChaCha20 (`rand_chacha` 0.3) and Fisher–Yates (`rand` 0.8, locked in Cargo.lock) produce reproducible decks. SHA-256 domain `pokerdraft:tie:v1` plus seed, little-endian u64 round and seat gives arrival-independent tie order. The evaluator uses fixed-size counts, bit masks and a five-element insertion sort, with no heap allocation for five-card evaluation. Best-of-ten enumerates 252 subsets.

All state mutation is serialized on a dedicated OS thread, keeping synchronous SQLite/fsync off async network executor threads. Request queues, socket frames, retry cache, connections, active matches (16) and rolling queue (512) are bounded. House/timer work runs on the writer. Multiple matches share that writer; this is deliberately a single-node authoritative architecture, not an unmeasured horizontal-scaling claim.

Account data is durable JSON in the versioned `world` row. Schema v2 stores match headers/snapshots and private seeds separately from append-only event/history rows, with indexed bot/team membership for archives. Per-match cursors write only the new event/history suffix. The transaction migrates legacy arrays without losing public replay data; back up before upgrading. Match-only persistence avoids rewriting account data every auction. Final result, Elo and tournament advancement commit together. Accepted in-round bids live only in RAM; a crash aborts the whole live match instead of attempting an unfair partial recovery. Writes fail closed: a persistence error stops the competition actor; readiness fails and restart recovery handles live records. Completed results are never re-rated on restart.

Wall-clock timestamps are protocol/display values. `Instant` governs deadlines. New windows start after the previous round's durable commit, and timers skip missed scheduler ticks rather than collapse bidding windows. Geographic RTT, OS scheduling and writer backlog still affect admission; these are measured, not claimed away by cryptography.

Swiss pairings are deterministic score-adjacent with greedy repeat avoidance, not a claim of formal FIDE pairing compliance. Entries get one bye each before repeating. Buchholz uses realized opponent points. Top cut is highest versus lowest seed; elimination rounds and best-of-three decisive finals use heads-up games. Exact drawn games are replayed up to three consecutive draws, then the cup is suspended for operator intervention/cancellation; there is no fabricated winner. Tournament registration is persistent, a single team cannot occupy multiple seats, and aborted games reschedule without points.

## Scaling and limits

The initial release favors correctness and auditability on modest homelab hardware. Measure your storage sync latency and sustained fan-out before increasing defaults. SQLite WAL requires a local filesystem; one replica owns the database under a lifetime inode lease. A future partitioned deployment should assign whole matches to workers with their own databases or move durable metadata to a free PostgreSQL instance. Do not run several independent authoritative pods over the same file. Archive, roster, ranking and tournament reads use a separate read-only SQLite worker with a 64-request queue, coherent WAL snapshot transactions, 250 ms SQL/busy budgets and a three-second reply deadline. A revision number committed with world changes invalidates its public-only metadata cache; responses cap rosters/ranks at 100 and tournaments at 10. Live directory/match reads stay on the writer. Credential/identity/name indices avoid scanning accounts for each bid; world serialization and index rebuilding on metadata changes still scale with world size. Broadcast frames are serialized once and shared across receivers. These are measured single-process optimizations, not a horizontal scaling claim.

No deployment manifests are supplied in this phase. A single process serves compiled web assets, HTTP and WS on one port. Persistent `data/` holds all identity/results. Health, metrics, consistent backups, graceful SIGINT/SIGTERM aborts, configurable binding/window and origin allowlist support later Proxmox/k3s packaging without tying the app to a paid platform.

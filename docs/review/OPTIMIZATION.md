# Round 3 — optimization and profiling

Completed after the security checkpoint `068d671`. These are local release-build measurements on the managed Linux workspace, not homelab capacity guarantees. No paid profiler, service or runtime was introduced.

## Findings and implemented changes

1. **Archive scans were the dominant measured read cost.** Finding one bot's history scanned JSON in every match. Schema v2 indexes bot/team membership and orders history by indexed creation time. Events and timeline checkpoints move from rewritten whole JSON arrays to append-only keyed rows. Per-match durable cursors advance only after successful commits. Migration, rollback, complete replay verification and coherent concurrent-read tests cover this redesign.
2. **Public reads competed with auction deadlines.** Archive/roster/ranking/tournament queries now have a separate read-only SQLite worker and a bounded 64-request queue. Multi-query replay reads hold one WAL snapshot. SQL/busy budgets are 250 ms and reply waits three seconds. The writer checks due timers before processing queued work; tick production cannot build an unbounded backlog. Live directory/match reads still use the writer.
3. **Every command scanned credentials and identities.** Hash maps now resolve credentials, bot/team IDs, team membership and names. Indices rebuild only after committed metadata changes. Disabled-team checks, token rotation and authorization semantics remain enforced.
4. **Every listener cloned/serialized a full JSON frame.** Broadcasts serialize once into shared UTF-8 bytes, with shared topic/member data. Receivers clone the shared frame. Lagged/slow receivers still disconnect and reconnect for a snapshot; memory is bounded.
5. **Public metadata could grow into large responses.** A durable revision invalidates a reader cache that deserializes only public fields. Ranks sort once per revision; bot/rank pages cap at 100 and tournaments at 10. Tournament Buchholz is computed once per standings traversal rather than repeatedly inside comparisons. UI and operator views support paging; team history returns explicit bot membership. Read-cache tests use 105 bots to cover later pages and invalidation. Browser tests exercise independent roster/ranking pages against the API contract.
6. **Reference strategy work allocated excessive snapshots.** House decisions now read the same public player/card/round inputs directly; trial cash hands use a ten-card stack array. Deterministic simulation outputs are exactly equal before/after.
7. **Browser/background work was unnecessarily broad.** Small countdown components own ticking state, rather than re-rendering the whole application at 10 Hz. Name lookup is memoized, metadata refreshes deduplicate and queue page changes, hidden tabs stop polling, and historical pairings render when expanded. SDK completed-match bookkeeping is bounded to 128 IDs. The load tool now treats disconnects/refusals as failures instead of reporting misleading zero errors.

## Measured bottlenecks

The isolated fixture has 1,000 teams/bots and 1,000 completed matches, SQLite 3.54.1, WAL with FULL sync. Samples: 10,000 auth/fan-out, 100 history and 50 internal leaderboard calls; writes cover 52 auctions. Archive fixtures use fixed seed 77. Results include execution/serialization/storage work and exclude network RTT.

| Operation | Before p50 / p95 | After p50 / p95 | p50 change |
| --- | ---: | ---: | ---: |
| Credential lookup | 5.909 / 7.561 µs | 0.230 / 0.241 µs | 25.7× faster |
| Per-receiver frame preparation | 10.015 / 13.950 µs | 0.060 / 0.070 µs | 166.9× faster |
| Bot history with results | 1,958.023 / 2,877.427 µs | 23.325 / 44.225 µs | 83.9× faster |
| Bot history without results | 1,999.445 / 2,946.059 µs | 11.277 / 13.400 µs | 177.3× faster |
| Internal full 1,000-entry leaderboard construction | 3,947.384 / 5,352.228 µs | 1,085.158 / 1,279.625 µs | 3.6× faster |
| Round decision + durable record write* | 418.220 / 913.074 µs | 214.127 / 330.930 µs | indicative 2.0× |

*The original writer sample used a random match seed; the final harness fixes seed 88. This row is illustrative, not a paired workload experiment. Encoded event/history payload across its match fell from 785,762 to 29,076 bytes (96.3% less), also workload-dependent. FULL sync remains enabled. Snapshot/header writes, SQLite page/journal overhead and final metadata commits are additional work; payload bytes are not disk bytes.

The actual **100-entry cached public leaderboard page** takes 100.679 µs p50 / 161.669 µs p95 internally, with a first cold page of 1.140 ms. This new bounded endpoint differs from the full-1,000-entry benchmark above; do not interpret their ratio as an equal-payload speedup.

The same 10,000 seeded six-seat games take **2,385 → 1,117 ms (2.14×)** with identical 60,831 cash-ins, zero burns and identical per-seat score totals. Best-of-ten evaluation was 6,198 → 5,825 ns, a small difference consistent with noise; the key engine win is reference-strategy allocation removal, not a claimed evaluator breakthrough. Aggregate match speed does not predict network capacity.

Raw evidence: [profile before](profile-before.json), [profile after](profile-after.json), [engine before](engine-before.txt), [engine after](engine-after.txt). The baseline was captured at the security checkpoint with the same fixture/timing operations instrumented before optimization. Current harness adds separate cached-page measurements. Timing varies with CPU scheduling and storage; repeat locally when comparing changes.

## Reproduce and validate

```sh
make profile  # temporary isolated DB, removed afterward; never opens live DB
cargo run --release -p pokerdraft-engine --example simulate -- 10000
make check
make security
python3 scripts/smoke.py
make browser  # running local service and Chromium required
POKERDRAFT_URL=http://127.0.0.1:3000 node scripts/load.mjs 20 5
```

The profiler is feature-gated (`profiling`) and excluded from the normal server build. It exposes no HTTP seed control. The simulation is deterministic; the real-bot smoke uses random seeds and checks invariants instead of assuming a champion.

Final checks: 37 Rust tests, exhaustive 2,598,960-hand evaluation, seven Python tests, three Chromium flows, production web build, formatting and all-feature clippy. Isolated real-network security checks pass. Python and native TypeScript agents complete games, Swiss/final progression, graceful/hard recovery and exact preserved ratings; the final smoke took 55.16 seconds. The suite retains equality assertions for ratings rather than tolerating observed float drift. Dependency scans remain point-in-time checks.

The final real-network sample used 20 loopback clients for five seconds: 800 ping receipts (p50 0.784 ms, p95 3.051 ms) and 180 live writer-backed watch reads (p50 1.807 ms, p95 5.666 ms), 1,420 messages and zero errors. See [raw load output](load-after.json). This is a short smoke sample, not a sustained capacity test or a durable mutation latency measurement; no end-to-end speedup is claimed from it.

## Remaining limits and next profiling targets

One writer still owns all competitions. World metadata serialization/index rebuilds are O(world size), and a cold reader cache parses the world; SQL progress hooks do not interrupt Rust JSON parsing. Tournament payloads are finite but larger than simple roster pages. A separate reader prevents these from blocking the writer queue but shares CPU and storage. Offset pagination is bounded at 100,000; deeper retention/navigation will need keyset pagination/export. Disk retention is manual, especially for continuous demos.

Durable cash receipts include fsync; archive/network timings are not a substitute for measuring bid admission and scheduler lag on the actual Proxmox host. Test ingress peer limits, storage latency, WAN RTT, real bot think time and larger sustained concurrency before raising 16 live matches/512 queued bots. A separate metrics credential permits private collection without exposing command timing publicly. No architectural claim of cross-host SQLite sharing or automatically scalable replicas is made. Deployment remains deferred.

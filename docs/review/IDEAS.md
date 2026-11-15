# Round 1 — ideas for review

These are proposals, not approved changes to game rules. The security/performance work in later rounds preserves the v1 economy, ranking, card visibility and auction format. Each idea needs an experiment before becoming a ranked rule.

## Recommended first experiments

1. **Duplicate-deal strategy laboratory.** Compare policies over paired seeds and rotated seats, report confidence intervals on *paired* differences, and keep train/test seeds disjoint. This is inspired by variance-reduction work in computer poker, not a claim that AIVAT transfers unchanged to this game. Never sequentially publish a shared seed while another ranked game in the same batch still uses it: the first replay would reveal the second game's deck. Start with offline, unrated experiments.
2. **Economic accounting study.** Compare earned-reward score with final net wealth and net value created. Current payments feed the pot and winning it creates score: cooperating agents can recycle balances into score. This is bounded by 52 cards but worth measuring before a season. Log Gini/concentration, zero-balance duration, cash-in categories, qualified seats per showdown, pot rollover and comeback rates.
3. **Strategy benchmark league.** Add independently designed conservative, liquidity-aware, showdown-aware, denial and Monte Carlo policies. The current reference varies bids with seat index; do not mistake its different seat totals for a proof of engine seat bias. Compare agents under all seat rotations and both pricing formats.
4. **Tournament resilience.** Retain tied-game replay rather than seat-order advancement, but suspend persistently drawn/inactive pairings for operator attention. Define explicit forfeiture rules and appeals before allowing automated advancement.
5. **Bot developer feedback.** Add an offline decision inspector showing marginal hand value, future outs, liquidity constraints and why an action was rejected, without exposing live opponents' bids.

## Candidate backlog

| ID | Proposal | Benefit / question | Tradeoff or guardrail | Effort / priority | Evidence to collect |
| --- | --- | --- | --- | --- | --- |
| R01 | Configurable, versioned reward schedules in *unrated* rooms | Tune rare-hand premiums and liquidity | Do not mix incompatible rules on one rating ladder | M / high | Strategy diversity, concentration, win-rate intervals |
| R02 | Net-wealth or net-created-value scoring experiment | Test whether pot recycling distorts incentives | Changes the central objective; preserve a separate v1 ladder | M / high | Colluding-policy advantage and dominant spend patterns |
| R03 | Longer regional-neutral tournament windows | More usable over high RTT/jitter | Still cannot create equal physical latency; slower matches | S / high | RTT, admission margin, missed decisions by route |
| R04 | Configurable showdown frequency / sponsor contribution | Make six-seat showdowns consistently contested | Changes liquidity/hoarding pressure | S / medium | Qualifying entrants, rollover, cash timing |
| R05 | Voluntary showdown entry or nominated hands | Give bots explicit cash/hold decisions | More action surface and deadline failure modes | M / medium | Strategic diversity versus timeout advantage |
| R06 | Low-value partial-inventory salvage at finish | Reduce cliff effects below five cards | Can reward indiscriminate purchases; retune rewards | S / medium | Last-quarter participation and dead balances |
| R07 | Deterministic flat / category-dependent reserve experiments | Compare market dynamics beyond reserve 1 | Avoid hidden/random prices that obscure reproducibility | S / low | Burn rate, welfare, liquidity and bid shading |
| R08 | Human practice seats | Let people learn the mechanism interactively | One-second bot timing is unsuitable; use unrated slow rooms | L / medium | Completion rates and understanding of scoring |
| E01 | Paired-seed benchmark and confidence intervals | Distinguish improvement from favorable draws | Batch secrecy and disjoint holdout sets | M / high | Paired mean, standard error, effective sample size |
| E02 | Reproducible policy league / strategy ablations | Establish a credible baseline | External code stays sandboxed on participant machines | M / high | Head-to-head/rotation matrices, policy complexity |
| E03 | Tournament manifests with immutable rules hashes | Make seasons comparable and auditable | Requires migration/version policy | M / high | Reproduction from exported manifests |
| E04 | Multiple rating pools per rules/window/seat count | Avoid mixing different competitive tasks | Sparse ladders have higher uncertainty | M / high | Cross-format ranking stability |
| E05 | Uncertainty-aware ratings and provisional entrants | Show how much evidence a rating has | Choose an open implementation with tested tie semantics | M / medium | Calibration and predictive accuracy |
| E06 | Repeat-opponent throttles and season identity review | Discourage easy rating farming | Anonymous registration cannot prove human uniqueness | M / high | Opponent diversity and suspicious result clusters |
| E07 | Swiss pairing optimization and pairing audit | Improve repeat avoidance versus current greedy method | Define tie-breaks, byes and published standard explicitly | M / medium | Avoidable rematches across randomized fields |
| D01 | Local decision inspector and offline replay annotations | Explain bids and liquidity without affecting matches | Keep annotations out of authoritative state | M / high | Time from first bot to first valid game |
| D02 | SDK conformance suite in additional languages | Make arbitrary-language participation practical | Maintain one protocol contract | M / medium | Reconnect, rotation, stale-round and retry tests |
| D03 | Batched local training API / policy callback runner | Fast learning without WS wall-clock waits | Keep it isolated from rated state and credentials | M / high | Matches/sec, deterministic equivalence |
| D04 | Match comparison, downloadable CSV and per-round charts | Make policy improvements interpretable | Bound exports; no private-bid disclosure | M / medium | Investigation time and archive query cost |
| D05 | Saved watchlists and stable links to rounds | Better spectator navigation and sharing | Prefer non-sensitive local settings | S / medium | Successful replay deep links |
| D06 | Accessible slow replay and event narration | Improve mobile/screen-reader experience | Avoid narrating every live tick automatically | S / high | Keyboard/mobile/accessibility review |
| A01 | Owner passkeys with offline recovery key | Reduce bearer-secret handling in browsers | WebAuthn enrollment/recovery and CLI access need deliberate design | L / medium | Recovery success and account-takeover threat model |
| A02 | Client-contributed randomness / delayed batch reveal | Reduce operator discretion in deck selection | Abort/grinding resistance requires a clear protocol; no blockchain needed | L / low | Threat model, abort bias and recovery behavior |
| A03 | Signed, hash-linked replay manifests | Make later transcript edits detectable to independent witnesses | Signing cannot prove honest private bid allocation; key rotation matters | M / medium | Independent verification and retained commitments |
| A04 | Read replicas or per-match workers after measured thresholds | Grow beyond one writer safely | Never share a live SQLite WAL file across hosts | L / later | Queue delay, sync latency, CPU and outbound bytes |
| A05 | Archive retention, compaction and cold export | Bound disk growth in continuous operation | Preserve completed tournament/rating provenance | M / high | Daily growth, restore time and retention guarantees |

## Suggested acceptance gates

- No ranked rule change without a rules version, compatible replay verifier and a separate comparison against v1.
- Use multiple nontrivial policies and adversarial/cooperating policies. Self-play of one baseline does not establish balance.
- Publish seeds only after every game using them has ended. Never let training/evaluation reuse reveal the future deck in another live game.
- Report confidence intervals and cost per decision alongside win rates. Reproducible runs matter more than one favorable leaderboard result.
- Keep the platform fully free and self-hostable. GPU/model strategies remain optional participant choices.

## Primary sources consulted

- University of Alberta Computer Poker Research Group, [AIVAT research](https://poker.cs.ualberta.ca/publications/2017-aaaiw-aivat.pdf): variance-reduced agent evaluation motivates paired experiments; application here needs validation.
- [OWASP WebSocket Security Cheat Sheet](https://cheatsheetseries.owasp.org/cheatsheets/WebSocket_Security_Cheat_Sheet.html): origins, per-message authorization, bounded messages and cross-connection abuse controls inform round 2.
- [SQLite WAL](https://www.sqlite.org/wal.html): local shared-memory and single-writer constraints inform the proposed scaling boundary.
- [Tokio broadcast](https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html): bounded fan-out and lagged-consumer semantics inform round 3.

Reviewed 2026-11-15. This backlog intentionally records alternatives and tradeoffs; it does not claim novelty, equilibrium optimality or that every proposal should ship.

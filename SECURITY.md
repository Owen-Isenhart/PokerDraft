# Security policy and boundaries

Poker Draft handles fictional points, public competition data and bearer credentials. Run only your own bots externally; the server never executes uploaded strategies. See `docs/review/SECURITY.md` for the full review, fixes, tests and remaining limits.

Use TLS at your ingress, set `POKERDRAFT_ALLOWED_HOSTS` explicitly for your public hostname and keep the private SQLite volume accessible only to the service account. The default listener is loopback. Do not expose Vite development servers publicly. Never share live tokens, database files or undisclosed seeds in issues or logs.

Use GitHub private vulnerability reporting when available. Include the affected revision, a minimal reproduction against an isolated local instance and expected/actual behavior. Do not exploit third-party installations or publish credentials in a public report. Dependency updates are tracked through the free advisory tools and Dependabot configuration; a clean scan is not proof that every vulnerability has been found.

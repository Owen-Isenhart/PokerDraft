#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [ ! -d web/node_modules ]; then npm ci --prefix web; fi
cargo build -p pokerdraft-server
./target/debug/pokerdraft-server &
server_pid=$!
cleanup() { kill -TERM "$server_pid" 2>/dev/null || true; wait "$server_pid" 2>/dev/null || true; }
trap cleanup EXIT INT TERM
npm run dev --prefix web

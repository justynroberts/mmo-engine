#!/usr/bin/env bash
# Usage: scripts/bench.sh [bots] [world-size] [seconds]
# Pins the server to core 0 and the bots to core 1 (needs >= 2 cores).
set -euo pipefail
BOTS=${1:-2000}; WORLD=${2:-2048}; SECS=${3:-30}
cd "$(dirname "$0")/.."
cargo build --release -q
B=target/release
LOG=$(mktemp)
taskset -c 0 $B/server --duration $((SECS + 6)) --world-size "$WORLD" --quiet >"$LOG" 2>&1 &
SRV=$!
sleep 1
taskset -c 1 $B/bots --bots "$BOTS" --duration "$SECS" --ramp 1000 --quiet
wait $SRV || true
cat "$LOG"
rm -f "$LOG"

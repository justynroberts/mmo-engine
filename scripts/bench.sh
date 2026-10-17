#!/usr/bin/env bash
# Usage: scripts/bench.sh [bots] [world-size] [seconds] [workers]
# Server on core 0 (workers on core 1..), bots on the last core. Needs >= 2 cores.
# With only 2 cores, workers=2 shares core 1 with the bots, which understates the gain.
set -euo pipefail
BOTS=${1:-2000}; WORLD=${2:-2048}; SECS=${3:-30}; WORKERS=${4:-1}
cd "$(dirname "$0")/.."
cargo build --release -q
B=$PWD/target/release
DIR=$(mktemp -d); LOG=$DIR/server.log
CORES=$(nproc); BOTCORE=$((CORES - 1))
cd "$DIR"
taskset -c "0-$((WORKERS > 1 ? WORKERS - 1 : 0))" $B/server --pin 0 --workers "$WORKERS" --duration $((SECS + 6)) \
  --world-size "$WORLD" --quiet --no-admin --port 7790 >"$LOG" 2>&1 &
SRV=$!
sleep 1
PUB=$(grep -o 'server-key): [0-9a-f]*' "$LOG" | awk '{print $NF}')
taskset -c "$BOTCORE" $B/bots --server 127.0.0.1:7790 --bots "$BOTS" --duration "$SECS" --ramp 1000 --quiet --server-key "$PUB"
wait $SRV || true
cat "$LOG"
rm -rf "$DIR"

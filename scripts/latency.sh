#!/usr/bin/env bash
# Does one slow player cost anyone else anything? Runs the same bots twice against one server, with the same number
# of clients both times: first the extra group has the healthy link too, then a bad one (delay, jitter, loss, each
# way). Only the link changes, so any difference in the healthy bots' numbers (or the server's tick) is the cost of
# the slow ones. Linux only (the server is). The server gets its own cores and the bots the rest (taskset), and
# bots run as several processes so the load generator is never the bottleneck.
#
#   scripts/latency.sh [normal bots] [slow bots] [slow delay ms] [slow jitter ms] [slow loss %] [seconds] [workers]
#   e.g. scripts/latency.sh 3600 400 400 150 10 40 3
set -euo pipefail
NORMAL=${1:-3600}; SLOW=${2:-400}; DELAY=${3:-400}; JITTER=${4:-150}; LOSS=${5:-10}; SECS=${6:-40}; WORKERS=${7:-3}
PROCS=${PROCS:-4}; WORLD=${WORLD:-4096}; LINK=${LINK:-"--latency 20 --jitter 10"}
cd "$(dirname "$0")/.."
B=${BIN:-$PWD/target/release}
ulimit -n 65536 2>/dev/null || true

CORES=$(nproc)
SRV_CPUS="0-$((WORKERS - 1))"; BOT_CPUS="$WORKERS-$((CORES - 1))"
pin() { if command -v taskset >/dev/null && [ "$CORES" -gt "$WORKERS" ]; then taskset -c "$1" "${@:2}"; else "${@:2}"; fi; }

run() { # $1 = label, $2 = the extra group's link (args for bots)
  local dir; dir=$(mktemp -d)
  (cd "$dir" && pin "$SRV_CPUS" "$B/server" --port 7801 --workers "$WORKERS" --world-size "$WORLD" --duration $((SECS + 4)) ${SRVQUIET:---quiet} --no-admin --no-bots-dir) > "$dir/server.log" 2>&1 &
  sleep 1.5
  local key; key=$(grep -o 'server-key): [0-9a-f]*' "$dir/server.log" | awk '{print $NF}')
  local per=$((NORMAL / PROCS))
  for p in $(seq 1 "$PROCS"); do
    pin "$BOT_CPUS" "$B/bots" --server 127.0.0.1:7801 --server-key "$key" --bots "$per" --seed "$p" --duration "$SECS" --ramp 400 --quiet $LINK > "$dir/normal$p.log" 2>&1 &
  done
  pin "$BOT_CPUS" "$B/bots" --server 127.0.0.1:7801 --server-key "$key" --bots "$SLOW" --slow "$SLOW" --seed 99 --duration "$SECS" --ramp 400 --quiet $2 > "$dir/slow.log" 2>&1 &
  wait
  sleep 1
  echo "== $1"
  # The server's own view over the run: how many it carried and how its tick held up.
  grep -h -E 'peak clients|tick time ms|ticks over budget|^cpu:' "$dir/server.log" | sed 's|^|server: |'
  for f in "$dir"/normal*.log; do grep -h -E 'input->ack latency|snapshots applied|bots playing|late ticks' "$f" | sed "s|^|normal $(basename "$f" .log): |"; done
  grep -h -E 'group +slow|late ticks' "$dir/slow.log" | sed 's|^|extra group: |'
  [ -n "${KEEP:-}" ] && cp -r "$dir" "$KEEP/$(echo "$1" | cut -c1-4)" || true
  rm -rf "$dir"
}

run "control: $NORMAL normal bots + $SLOW more on the same healthy link" "--slow-latency 20 --slow-jitter 10 --slow-loss 0"
run "test: $NORMAL normal bots + $SLOW slow bots (${DELAY}+${JITTER} ms, ${LOSS}% loss each way)" "--slow-latency $DELAY --slow-jitter $JITTER --slow-loss $LOSS"

#!/usr/bin/env bash
# Headless smoke test of the Godot client against a real server. Needs Godot 4.6 on PATH (or GODOT=/path/to/godot)
# and a release build of server and bots. Exits 0 when the scripted session passes.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GODOT="${GODOT:-godot}"
UDP="${UDP:-47900}"; ADMIN="${ADMIN:-48900}"; WEB="${WEB:-48901}"
WORK="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$WORK"' EXIT
"$ROOT/scripts/build_godot.sh" >/dev/null
cd "$WORK"
"$ROOT/target/release/server" --port "$UDP" --admin "127.0.0.1:$ADMIN" --web "127.0.0.1:$WEB" --quiet --world-size "${WORLD:-1024}" >/dev/null 2>&1 &
for _ in $(seq 50); do curl -sf "http://127.0.0.1:$WEB/play/info" >/dev/null && break; sleep 0.2; done
KEY=$(curl -s "http://127.0.0.1:$WEB/play/info" | python3 -c 'import sys,json;print(json.load(sys.stdin)["server_key"])')
"$ROOT/target/release/bots" --server "127.0.0.1:$UDP" --server-key "$KEY" --bots 20 --duration 60 --quiet >/dev/null 2>&1 &
sleep 2
"$GODOT" --headless --path "$ROOT/godot" --import >/dev/null 2>&1 || true
"$GODOT" --headless --path "$ROOT/godot" -- --smoke --host 127.0.0.1 --udp "$UDP" --web "127.0.0.1:$WEB" --admin "127.0.0.1:$ADMIN"

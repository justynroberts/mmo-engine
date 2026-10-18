#!/usr/bin/env bash
# Headless smoke test of the Godot client against a real server. Needs Godot 4.6 on PATH (or GODOT=/path/to/godot)
# and a release build of server, bots and worldgen. Exits 0 when the scripted session passes.
#   WORLD=8000       world size (1024)
#   THEME=plain      client theme (fantasy)
#   NO_WORLD=1       run the server without a world file (just its own walls)
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
GODOT="${GODOT:-godot}"
UDP="${UDP:-47900}"; ADMIN="${ADMIN:-48900}"; WEB="${WEB:-48901}"
WORK="$(mktemp -d)"
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$WORK"' EXIT
"$ROOT/scripts/build_godot.sh" >/dev/null
cd "$WORK"
if [ -n "${NO_WORLD:-}" ]; then
  WORLD_ARGS=(--world-size "${WORLD:-1024}")
else
  "$ROOT/target/release/worldgen" --size "${WORLD:-1024}" --seed 5 --out "$WORK/world.json" >/dev/null
  WORLD_ARGS=(--world "$WORK/world.json")
fi
"$ROOT/target/release/server" --port "$UDP" --admin "127.0.0.1:$ADMIN" --web "127.0.0.1:$WEB" --quiet "${WORLD_ARGS[@]}" >/dev/null 2>&1 &
for _ in $(seq 50); do curl -sf "http://127.0.0.1:$WEB/play/info" >/dev/null && break; sleep 0.2; done
KEY=$(curl -s "http://127.0.0.1:$WEB/play/info" | python3 -c 'import sys,json;print(json.load(sys.stdin)["server_key"])')
"$ROOT/target/release/bots" --server "127.0.0.1:$UDP" --server-key "$KEY" --bots 20 --duration 60 --quiet >/dev/null 2>&1 &
sleep 2
"$GODOT" --headless --path "$ROOT/godot" --import >/dev/null 2>&1 || true
timeout 300 "$GODOT" --headless --path "$ROOT/godot" -- --smoke --theme "${THEME:-fantasy}" --host 127.0.0.1 --udp "$UDP" --web "127.0.0.1:$WEB" --admin "127.0.0.1:$ADMIN"

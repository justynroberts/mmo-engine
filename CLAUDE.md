# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Genre-neutral authoritative world server (Rust), a bot load tester that speaks the real encrypted UDP protocol, a browser admin console and a browser game client. `README.md` is the long-form design doc (security model, measured numbers, zones, items); `PROTOCOL.md` is the wire spec.

## Platform gotcha

The `server` crate is **Linux-only** (`recvmmsg`, `sendmmsg`, `sched_setaffinity` in `crates/server/src/io.rs`). On macOS `cargo build`/`cargo test` for the workspace fail to compile `server`; `proto`, `bots` and `mmo-client` build fine. To run or test the server on a Mac use Docker (`docker compose up --build`) or a Linux box. `scripts/bench.sh` also needs Linux (`taskset`, `nproc`).

## Commands

```sh
cargo build --release                       # server, bots, mmo-client, mmo-godot
cargo test --release                        # proto + server unit tests + crates/server/tests/e2e.rs (real UDP/HTTP)
cargo test -p proto                         # works on macOS
cargo test -p server --release --test e2e <name>        # one e2e test
cargo test -p server --release lane_scaling -- --ignored --nocapture   # lane benchmark

cd ui && npm install
npm run build        # tsc --noEmit && vite build (two entries: index.html = console, play.html = game client)
npm test             # vitest run (jsdom)
npx vitest run src/play/sim.test.ts         # one test file
npm run dev          # Vite on :5183, proxies /api to a server on 127.0.0.1:7780

scripts/ui_e2e.py [--shots DIR]   # Playwright against release binaries + built UI; starts its own server
scripts/bench.sh [bots] [world] [secs] [workers]

target/release/server --workers 2 [--web 0.0.0.0:7790]   # prints server key; console at http://127.0.0.1:7780/
target/release/bots --bots 200 --server-key <key>
```

Run `npm run build` after editing TypeScript in `ui/src/` (it is the type check). First server run writes `game.json`, `server.key`, `data/world.db` and `bots.d/` in the working directory (all gitignored).

## Architecture

**Workspace crates**
- `proto` — everything both sides must agree on: wire format (`wire.rs`, `PROTO_VERSION`), handshake + ChaCha20-Poly1305 sessions (`secure.rs`), delta snapshots (`snapshot.rs`), reliable chat channel (`chat.rs`), game file schema and validation (`game.rs`), the `Defs` derived from it (`defs.rs`, `content_hash`), static world + the movement/collision step (`world.rs`), client prediction (`predict.rs`). Compiled into server and every Rust client so they cannot drift.
- `server` — single binary. `main.rs` parses flags and runs the fixed tick. `sim.rs` owns the world (SoA entities, spatial grid, bot rules). `net.rs` handles handshakes and DATA packets and builds per-client snapshots, split across `--workers` lanes with `std::thread::scope` (each lane owns a disjoint slice of clients; sim is read-only in that phase, so no locks). `ctl.rs` is the loopback-only admin HTTP API. `web.rs` is the public `--web` gateway: serves `play.html` + `/play/info` + `/play/walls`, and relays WebSocket messages to the UDP port (it never decrypts). `store.rs` is a SQLite writer thread for players and the bot population. `zone.rs` is multi-process scale-out (strip ownership, TCP handoff, border ghosts). `director.rs` = weather and events (God tab), `items.rs` = bags and ground items, `botdir.rs` = hot-reloads `bots.d/*.json`.
- `bots` — N real encrypted clients for load testing.
- `client-ffi` (`mmo-client`) — the client protocol behind a C ABI; header in `sdk/c/mmo_client.h`. `godot-ext` wraps it as a GDExtension (compiles, never loaded in Godot).

**Single owner of the world.** Only the sim thread mutates the world. The admin API, web gateway, bot-dir watcher, store and zone links run on their own threads and talk to the sim through queues serviced once per tick. Keep it that way; the tick must never block on disk or network.

**Data, not code.** Attributes (health is just attribute 0), actions/effects, bot types ("archetypes") with rule stacks, places, routes, items, weather, events and hand-placed walls all live in one `GameFile` JSON. The distinction that matters when editing it:
- Attributes, actions and settings feed `Defs`, whose `content_hash` is checked in the handshake. Changing them restarts the world and drops clients.
- Bot types, places, routes, items, weather/events and walls are not in the hash and apply live without disconnecting anyone.

When renaming/deleting an attribute, place, route, bot type or weather, the code rewrites or drops every rule/event step that referred to it (both server-side and in the UI editors, e.g. `ui/src/gameEdit.ts`, `directorEdit.ts`). Preserve that when adding new references.

**Items ride the input action byte**: `200 + slot` uses, `216 + slot` drops, `250` picks up. Game action ids must stay below 200.

**Saved state is keyed by name**, not index (attributes, archetypes, items), so editing the game file never scrambles saved characters or bags.

## Duplicated implementations that must stay in sync

The protocol is implemented twice and movement three times. A change to any of these needs the matching change and its cross-check test:
- `crates/proto` ↔ `ui/src/play/protocol.ts` (handshake, keys, Argon2id identity and recovery code match Rust byte for byte; `protocol.test.ts`). Bump `PROTO_VERSION` in both and update `PROTOCOL.md`.
- `crates/proto/src/world.rs` + `predict.rs` ↔ `ui/src/play/sim.ts` (f32 rounding via `Math.fround`; `sim.test.ts`).
- Snapshot interpolation (same approach, not a byte-exact port): `crates/client-ffi/src/interp.rs` ↔ `ui/src/play/interp.ts`.
- Wall streaming chunks (256 units, `/play/walls`) in `web.rs` ↔ `ui/src/play/walls.ts`.
- Built block columns and 3D movement (`World::set_column`/`enter`/`settle`/`step`, `integrate` with the feet layer) in `crates/proto/src/world.rs` + `predict.rs` ↔ `ui/src/play/sim.ts` (same cases tested on both sides).

## UI

`ui/` is React 18 + TypeScript + Tailwind + Radix + Framer Motion + three.js, served by the server from `--ui-dir` (default `ui/dist`). Two apps: the console (`src/App.tsx`, tabs in `src/components/`, talks to `/api/*` with an `X-Admin: 1` header on writes) and the play client (`src/play/`). All play-page colours live in the `.play-skin` block in `ui/src/index.css`, which the 3D scene reads.

## Docker

`Dockerfile` builds the UI, then `server` and `bots`, into a slim non-root image. `docker-compose.yml` uses host networking because the admin interface refuses to bind anywhere but loopback; state persists in `./data`.

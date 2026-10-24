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
scripts/latency.sh [normal] [slow] [delay ms] [jitter ms] [loss %] [secs] [workers]   # does a slow player cost the others anything? (Linux)

target/release/server --workers 2 [--web 0.0.0.0:7790]   # prints server key; console at http://127.0.0.1:7780/
target/release/bots --bots 200 --server-key <key>
```

Run `npm run build` after editing TypeScript in `ui/src/` (it is the type check). First server run writes `game.json`, `server.key`, `data/world.db` and `bots.d/` in the working directory (all gitignored).

## Architecture

**Workspace crates**
- `proto` — everything both sides must agree on: wire format (`wire.rs`, `PROTO_VERSION`), handshake + ChaCha20-Poly1305 sessions (`secure.rs`), delta snapshots (`snapshot.rs`), reliable chat channel (`chat.rs`), game file schema and validation (`game.rs`), the `Defs` derived from it (`defs.rs`, `content_hash`), static world + the movement/collision step (`world.rs`), client prediction (`predict.rs`). Compiled into server and every Rust client so they cannot drift.
- `server` — single binary. `main.rs` parses flags and runs the fixed tick. `sim.rs` owns the world (SoA entities, spatial grid, bot rules). `net.rs` handles handshakes and DATA packets and builds per-client snapshots, split across `--workers` lanes with `std::thread::scope` (each lane owns a disjoint slice of clients; sim is read-only in that phase, so no locks). `ctl.rs` is the loopback-only admin HTTP API. `web.rs` is the public `--web` gateway: serves `play.html` + `/play/info` + `/play/tiles` (walls by tile) + `/play/world`, and relays WebSocket messages to the UDP port (it never decrypts). `store.rs` is a SQLite writer thread for players and the bot population. `blocks.rs` keeps built blocks by region: `<world>.blocks.d/<rx>_<ry>.json`, only changed regions written, by `RegionWriter`'s thread (a lone `<world>.blocks.json` from before is read once and moved into regions); `Feed` serves `/play/blocks/r`, caching each region's encoding per version. `GameFile::building` caps blocks per player and per region (not in the hash). The Godot themes mesh block chunks on worker threads (`theme.gd` `_build_pending_blocks`), rounded near and plain beyond `BLOCK_ROUND_NEAR`; `godot/tools/bench_blocks.gd` times it. `zone.rs` is multi-process scale-out: strips of one world (ownership by x, TCP handoff, border ghosts) or, with `--zone-lines`, separate maps joined by exits on places (`net::exits`; same handoff, and the zone left keeps a `Record::zone` marker that sends a returning player on). `statics.rs` holds the walls by tile and the world file for the admin and web threads, and prepares a reloaded world off the sim thread. `worldgen` (`crates/worldgen`, `roads.rs` for the road network) writes world files with roads that the server reads into road cells. `nav.rs` = walkability grid and flow fields that steer bots round walls. `director.rs` = weather and events (God tab), `items.rs` = bags and ground items, `botdir.rs` = hot-reloads `bots.d/*.json`.
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
- Walls by tile (256 units, `/play/tiles`, binary): `crates/proto/src/tiles.rs` (server and Rust clients) ↔ `ui/src/play/walls.ts` (a shared byte fixture in both tests).
- Blocks by region (256 units, `/play/blocks/r`, binary): `crates/proto/src/blockfeed.rs` ↔ `ui/src/play/blocks.ts` `decodeRegions` (shared fixture) ↔ `godot/scripts/main.gd` `_poll_block_regions`.
- Roads: road cells (`World::on_road`, `ROAD_CELL`, `road_key`) and the `road_speed` multiplier in `integrate`, `crates/proto/src/world.rs` ↔ `ui/src/play/sim.ts`; road cells in the tile format, `crates/proto/src/tiles.rs` ↔ `ui/src/play/walls.ts` (the shared fixture includes a road). Road lines are turned into cells only on the server (`tiles::road_cells`).
- Built block columns and 3D movement (`World::set_column`/`enter`/`settle`/`step`, `integrate` with the feet layer) in `crates/proto/src/world.rs` + `predict.rs` ↔ `ui/src/play/sim.ts` (same cases tested on both sides).

## UI

`ui/` is React 18 + TypeScript + Tailwind + Radix + Framer Motion + three.js, served by the server from `--ui-dir` (default `ui/dist`). Two apps: the console (`src/App.tsx`, tabs in `src/components/`, talks to `/api/*` with an `X-Admin: 1` header on writes) and the play client (`src/play/`). All play-page colours live in the `.play-skin` block in `ui/src/index.css`, which the 3D scene reads.

## Sound

`godot/sounds` is generated by `scripts/make_audio.py` (rerun it after changing it; the files are committed). `godot/scripts/sound.gd` plays them; `main.gd` (`_listen`, `_footsteps`) decides what is heard. A place's `sound`/`music` must be one of `proto::game::SOUNDS`/`MUSIC`, which must match the files and `sound.gd`'s lists (and `SOUNDS`/`MUSIC` in `ui/src/api.ts`).

## Spells

Spells are actions with optional fields (`aim`, `area`, `hits`, `hold`, `fx`, `bolt`, `school`, effect `secs`) in `proto::game::ActionFile`; none is in the content hash, so editing them applies live (`ctl.rs` replaces `sim.defs` when the hash is unchanged, and `ui/src/gameEdit.ts` `wireOf` must hash the same fields as `Defs::content_hash`). The server resolves them in `Sim::use_action`/`system_spells` and tells nearby clients each use in a CASTS packet (`proto::wire`), which `client-ffi` dedupes (`mmo_take_casts`). The reserved attribute `held` shows a hold. Who a harmful action may land on (`pvp`, places marked `safe`) is decided in `use_action` too. `proto::game::FX` must match `godot/scripts/spellfx.gd` COLORS, the `cast_<fx>`/`impact_<fx>` sounds from `scripts/make_audio.py`, and `FX` in `ui/src/api.ts`.

## Experience and quests

`GameFile::levels` (curve, cap, might/toughness per level) and bot types' `xp`/`level`; kill credit (`hurt_by`), awards and discovery are in `sim.rs` (`credit`, `award`, `system_discover`), quests in `crates/server/src/quests.rs` (talk, accept, abandon ride the input action byte as `ACT_TALK`/`ACT_ACCEPT`/`ACT_ABANDON`). A player's own progress and quests go out privately (PROGRESS, QUESTS packets in `proto::wire`); `ctl::quest_info` publishes the quests in `/play/info`. Both are kept by name in `store::Record`. In the Godot client, `scripts/quests.gd` draws offers, the log and the tracker.

## Social and trade

`crates/server/src/social.rs` handles "/" chat commands (whispers, /who, parties: `Sim::party`, `party_of`); `crates/server/src/trade.rs` shops (`Archetype::shop`, `GameFile::currency`), gear (`ItemDef::equip`, `Sim::gear`, added in `Sim::scaled`) and mounts (`ItemDef::mount`, added to the `speed` attribute; `Sim::mounted`). Their private packets are PARTY and GEAR (`proto::wire`). The Godot client's `scripts/shop.gd` is the shop; party frames are in `hud.gd`, the horse in `main.gd` (`_make_horse`).

## Classes

`GameFile::classes` (`ClassFile`, built into `ClassRt`); a player's class is `Sim::class` (index + 1), chosen once with `ACT_CLASS`, saved by name in `store::Record`, added in `Sim::scaled` and enforced in `use_action`; the reserved attribute `class` shows it. In the Godot client `scripts/classpick.gd` is the picker, and `main.gd` `_look_of` draws players by their class's `look`.

## A living world

Bot rules can depend on the time of day (`during`, `proto::game::DayPart::during`), the weather where the bot stands (`weather`) and seeing a particular bot type (`sees_kind`); a bot can go home and indoors (`indoors`: `Entities::hidden`, left out of snapshots and bot scans) and pass remarks (`chat`, chat channel 7, drawn as a bubble by `main.gd` `_bubble`). A bot type's `lasts_secs` makes event bots (a raid's fires, a fair's stalls) go of themselves (`Sim::system_expire`). Rule references to bot types and weather are rewritten in `directorEdit.ts` `rewriteRefs`. A player who falls stays down (`Entities::fallen`) until they send `ACT_RISE`, then rises in the nearest `safe` place (`Sim::rise`); `scripts/fallen.gd` is the screen. Patrols skip a waypoint they are pinned short of for three seconds.

## MCP server

`ui/mcp/` exposes the console's functions as MCP tools (`tools.ts`; stdio entry `server.ts`; `npm run build:mcp` → `ui/dist-mcp/mmo-mcp.mjs`). It only calls the admin API, and game file edits go through `gameEdit.ts` / `directorEdit.ts`, so reference rewriting stays in one place. A new console function or admin endpoint should get a tool here too; `mcp/tools.test.ts` and `node mcp/smoke.mjs <admin url>` (against a throwaway server) test them.

## Client updates

`scripts/package_godot.sh` stamps the app with a version (`godot/version.txt`, the UTC packaging time) and publishes the signed zip as `data/client/mac.zip` plus `mac.json` (version, size, sha256). The web gateway serves them at `/play/client` and `/play/client/mac.zip` (streamed; `web.rs` `stream_file`). A packaged app's join screen (`join.gd`) offers a newer version, downloads it, checks the checksum and hands over to a small shell script that swaps the .app and reopens it (to `~/Applications` when macOS runs the app translocated).

## Docker

`Dockerfile` builds the UI, then `server` and `bots`, into a slim non-root image. `docker-compose.yml` uses host networking because the admin interface refuses to bind anywhere but loopback; state persists in `./data`.

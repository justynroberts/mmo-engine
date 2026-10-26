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

**Tuning numbers are data too.** `GameFile::rules` (`crates/proto/src/rules.rs`) holds the reaches, timers and sizes the server once kept as constants (talk and pickup reach, cooldowns, chat reach, party and guild size, trade and auction limits, chase and credit times); each defaults to its old value, none is in the hash, and `/play/info` publishes them so clients read `rules.talk_reach` and the like instead of copying numbers. A new gameplay number belongs there, not in a `const`; edit it in the console's settings (`RULE_FIELDS` in `ui/src/components/Editors.tsx`, `Rules` in `ui/src/api.ts`).

**Building can cost materials.** An item with `block: "<material>"` is that material: building one takes one from the bag, breaking your own gives it back (a block nobody owns gives nothing). A material no item names is free, and a god builds anything free (`Sim::build_action`, `block_item`). `GameFile::starter` is a new character's bag (`Sim::give_starter`, from `net::prove`). A bot type's `respawn_secs`/`respawn_at` keep a fallen bot out of sight (`Entities::fallen` + `back_at`) and bring it back anywhere, at home or where it fell (`Sim::bring_back`), which is how quarries and trees are made from bots.

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

`GameFile::levels` (curve, cap, might/toughness per level, `rewards` given on reaching a level in `Sim::level_rewards`) and bot types' `xp`/`level`; life experience (`life.rs`, `LifeXp`) also pays for planting, harvest, and each awake minute in the world, more in company, but only new ground (`trail` cells) and deeds unlike the last few (`repeat`, `Sim::lived_once` with a `life::key`) pay or keep a player awake, so macros earn nothing (`life_stale`, shown as `looping` in `/api/state`); kill credit (`hurt_by`), awards and discovery are in `sim.rs` (`credit`, `award`, `system_discover`), quests in `crates/server/src/quests.rs` (talk, accept, abandon ride the input action byte as `ACT_TALK`/`ACT_ACCEPT`/`ACT_ABANDON`). A player's own progress and quests go out privately (PROGRESS, QUESTS packets in `proto::wire`); `ctl::quest_info` publishes the quests in `/play/info`. Both are kept by name in `store::Record`. In the Godot client, `scripts/quests.gd` draws offers, the log and the tracker.

## Social and trade

`crates/server/src/social.rs` handles "/" chat commands (whispers, /who, parties: `Sim::party`, `party_of`); `crates/server/src/trade.rs` shops (`Archetype::shop`, `GameFile::currency`), gear (`ItemDef::equip`, `Sim::gear`, added in `Sim::scaled`) and mounts (`ItemDef::mount`, added to the `speed` attribute; `Sim::mounted`). Their private packets are PARTY and GEAR (`proto::wire`). The Godot client's `scripts/shop.gd` is the shop; party frames are in `hud.gd`, the horse in `main.gd` (`_make_horse`). `crates/server/src/market.rs` is the rest of the bag (24 slots, `ACT_BAG`), the bank (24 more, sent after the bag in INVENTORY, used only at a market: a `safe` place or near a merchant, saved in `store::Record::bank`), trading between players (`ACT_TRADE`, swapped all or nothing, state on chat channel 9) and the auction house (`ACT_AUCTION`, listings on channel 10, kept with the realm's save with what is owed to sellers, `Post`). Clients: `scripts/bag.gd` (F4), `trade.gd`, `auction.gd`. Emotes are `GameFile::emotes` rows (`/wave`, `social::emote`), sent on chat channel 11 as "anim|line"; the Godot character plays `anim` as a clip (`character.gd` `emote`). An emote with `stays` (sitting) is held in `Sim::pose` until the player moves and shown in the reserved attribute `pose` (`POSE_ATTR`), which `main.gd` `_pose_clip` turns back into a looping clip (`hold_pose`).

## Classes

`GameFile::classes` (`ClassFile`, built into `ClassRt`); a player's class is `Sim::class` (index + 1), chosen once with `ACT_CLASS` or `/class Name`, saved by name in `store::Record`, added in `Sim::scaled` and enforced in `use_action` (its `spells`, plus every action of its `schools` and those of no school); the reserved attribute `class` shows it. In the Godot client `scripts/classpick.gd` is the picker, and `main.gd` `_look_of` draws players by their class's `look`.

## Settlements and dressing (Godot fantasy theme)

`godot/themes/fantasy/towns.gd` gives every settlement (each world-file `square`; the city's squares are one) a palette (tints on the Medieval Village kit's `MI_*` materials, roof textures washed out first for slate and the like), a building style and a trade (fishing, mining, market, farming, from what is round it); houses outside any are farmsteads styled by their stretch of country. `theme.gd` `_build_house` takes its odds from the style, so a village is of a piece while its houses differ; manors, forts and village church towers (`keep`, `tower` outside the city) are built from the same kit in stone (`grand`). Yards, squares, docks and crags are dressed from the trade (`TRADES`) with KayKit Resource Bits (`assets/resources`), RPG Tools Bits (`assets/tools`) and Dungeon pieces, through `_dress` (names: `d:` dungeon, `v:` village, `t:` tools, else resources). Palette copies of a piece are batch keys `path#palette`, warmed a few a frame (`_warm`); every piece is loaded on the loader's threads from setup (`_prefetch`). `themes/fantasy/land.gd` is the land between: the cold north (`cold_band`, the `world_cold_band` global uniform that the ground, grass, tree tint and light all read), a per-cell RGBA ground map (road, paved or field, wet, woodland) for the ground shader, farm fields with fences, and lanterns and signposts where roads end; `theme.gd` `_land_dress` places verge stones, reeds, shore drift, ford stepping stones and rubble from it. None of it is solid or known to the server. `godot --path godot --script res://tools/preview.gd -- --world FILE --at X,Y;...` renders the theme at points of a world file with no server. The source packs are in the gitignored `kaykit/`; copy in only what is used.

## A living world

Bot rules can depend on the time of day (`during`, `proto::game::DayPart::during`), the weather where the bot stands (`weather`) and seeing a particular bot type (`sees_kind`); a bot can go home and indoors (`indoors`: `Entities::hidden`, left out of snapshots and bot scans) and pass remarks (`chat`, chat channel 7, drawn as a bubble by `main.gd` `_bubble`). A bot type's `lasts_secs` makes event bots (a raid's fires, a fair's stalls) go of themselves (`Sim::system_expire`). Rule references to bot types and weather are rewritten in `directorEdit.ts` `rewriteRefs`. A player who falls stays down (`Entities::fallen`) until they send `ACT_RISE`, then rises in the nearest `safe` place (`Sim::rise`); `scripts/fallen.gd` is the screen. What falling costs (experience, coin, bag stacks left on the ground, a wait before rising in `back_at`) is `rules.fall_*` and `rise_delay_secs`, applied in `Sim::fall_cost`. Patrols skip a waypoint they are pinned short of for three seconds.

## Realm

Item rarity and loot tables, crafting, crops, land plots, guilds and dungeons: the game file schema and its checks are `crates/proto/src/realm.rs` (`GameFile::realm` resolves names into a `Realm`, which `Game` carries; none of it is in the content hash), the server side `crates/server/src/realm.rs` (per-slot state in `Sim::rs`, the faction a player joined among it, "/" commands via `realm::command` from `social.rs`, `system_realm` once a second). Factions are one list, `GameFile::factions` (`FactionFile`, `factions.rs` for standing): bot types belong to one, and players may join one as their side with `/faction` (refused below `hostile`); members never harm each other (`Sim::allies`) and its fighters never take a member for an enemy (`hostile_to`). Classes are main's `ClassFile`, with realm's `schools` added. Hooks elsewhere: `Sim::use_action` (no harm between allies, nothing across phases), `build_action` (plots, crops come from seeds, harvest on break), `items.rs` (`roll_loot` after drops, seeds plant on use), the snapshot builder and bot targeting (phases). Guild chat is channel 8 (7 is a bot's passing remark). Plots cost `plots.cost` and grow through `plots.sizes` (`/plot grow`); a plot's `name` (`/plot name`) greets those who walk in (`system_plot_names`) and is found with `/plots` and `/visit`. Guilds, plots and crops persist in the store's world table under "realm" (`Store::save_doc`), saved every 5 s when dirty and on exit. A dungeon run is a phase: its bots carry it, and only players in it see or meet them. Renaming or deleting an item, bot type or place rewrites the realm sections in `ui/src/realmEdit.ts` (called from `rewriteRefs`); the MCP's realm sections are in `ui/mcp/tools.ts` (`realmOps`).

**Crowd.** Past the `max_visible` snapshot, `net.rs` sends a CROWD packet (`proto::wire` `0x90`, 4 Hz, hidden entities left out, a bot's side its type's faction, up to 480 entities out to `CROWD_REACH` interest radii). Decoded in three places that must agree: `crates/client-ffi` (`mmo_crowd`, Godot `crowd()`), `ui/src/play/protocol.ts` (`decodeCrowd`, `crowd.test.ts` shares bytes with the Rust test) and the bots. `bots --teams A,B --rally X,Y,R` is the battle load test.

Block materials are listed in six places that must stay the same length: `proto::game::BLOCK_KINDS` (17, with `SOIL`, `SPROUT`, `RIPE`) and `BLOCK_NAMES`, `godot/scripts/theme.gd` `BLOCK_NAMES`/`BLOCK_COLOURS`, `godot/themes/fantasy/theme.gd` `BLOCK_TEXTURES`, `ui/src/play/blocks.ts` `BLOCK_KINDS`, `ui/src/components/mapLook.ts` `BLOCK_COLOURS` (and `MATERIALS` in `ui/src/api.ts`, which the MCP and the item editor use, for the buildable ones).

## MCP server

`ui/mcp/` exposes the console's functions as MCP tools (`tools.ts`; stdio entry `server.ts`; `npm run build:mcp` → `ui/dist-mcp/mmo-mcp.mjs`). It only calls the admin API, and game file edits go through `gameEdit.ts` / `directorEdit.ts`, so reference rewriting stays in one place. A new console function or admin endpoint should get a tool here too; `mcp/tools.test.ts` and `node mcp/smoke.mjs <admin url>` (against a throwaway server) test them.

## Client updates

`scripts/package_godot.sh` stamps the app with a version (`godot/version.txt`, the UTC packaging time) and publishes the signed zip as `data/client/mac.zip` plus `mac.json` (version, size, sha256). The web gateway serves them at `/play/client` and `/play/client/mac.zip` (streamed; `web.rs` `stream_file`). A packaged app's join screen (`join.gd`) offers a newer version, downloads it, checks the checksum and hands over to a small shell script that swaps the .app and reopens it (to `~/Applications` when macOS runs the app translocated).

## Docker

`Dockerfile` builds the UI, then `server` and `bots`, into a slim non-root image. `docker-compose.yml` uses host networking because the admin interface refuses to bind anywhere but loopback; state persists in `./data`.

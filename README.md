# mmo-engine

A genre-neutral authoritative world server in Rust, a bot load tester that speaks
the real (encrypted) protocol, and a browser console to define bots and
attributes and poke at a live world.

Health is not special here. It is attribute 0 in a data table, and "combat" is
one set of data-driven effects on an action. The same engine can host an MMO,
a cyberpunk city or a user-built world.

## Layout

| Path | What it is |
| --- | --- |
| `crates/proto` | Wire format, game file (attributes, actions, bot types), delta snapshots, handshake and encryption, static world, the movement step. Compiled into server and clients so they cannot drift |
| `crates/server` | The world server: fixed tick, SoA entities, spatial grid, interest management, `recvmmsg`/`sendmmsg` UDP, snapshot lanes on spare cores, admin API |
| `crates/bots` | N real encrypted UDP clients: handshake, random walk, zaps, delta decode, ack, prediction, chat (`--chat N`), latency measurement, reconnect |
| `crates/worldgen` | A standalone world generator: writes a world file (heights, water, typed objects; [WORLD.md](WORLD.md)) for `server --world`. Depends on nothing in the engine |
| `crates/client-ffi`, `crates/godot-ext`, `sdk/` | The client protocol as a C library, and a Godot extension on top of it |
| `godot/` | The Godot client (`scripts/`) and its themes (`themes/plain`, `themes/fantasy`) |
| `ui/` | The console: React, TypeScript, Tailwind, Radix, Framer Motion. Served by the server |
| `scripts/` | `bench.sh` (pinned load test), `ui_e2e.py` (real-browser UI tests), `godot_e2e.sh` and `build_godot.sh` |

## Run it

```sh
cargo build --release
(cd ui && npm install && npm run build)

target/release/server --workers 2          # prints the server public key, serves the console
# console: http://127.0.0.1:7780/
target/release/bots --bots 200 --server-key <key printed by the server>
```

First run writes `game.json` (the definitions you edit in the console) and
`server.key` (the server's private identity, mode 0600; keep it private).

Docker: `docker compose up --build` (host networking, because the admin
interface only binds to loopback). The runtime stage was verified by hand: runs
as non-root (uid 10001), serves the UI, accepts 30 persisting bots and writes
the database to the volume. The builder stages' commands were replayed by hand on a clean `git archive` of
HEAD (`npm ci && npm run build`, then `cargo build --release --locked -p server
-p bots`) and both succeed. Only the base image pull itself could not be run,
because the registry was unreachable where this was built.

Accounts and recovery: a player identity can be derived from an account name
and passphrase (Argon2id, 19 MiB, t=2), so logging in from another device
gives the same character. Bots: `--account NAME --passphrase PW`,
`--recovery-code CODE`, or `--print-recovery` to show the code. A recovery code
is the raw identity secret as 8 hex groups plus a checksum; keep it like a
password. Trade-off: the server never sees the passphrase, but anyone who can
see a handshake can brute-force a weak one offline, so use long passphrases.

Identity minting: each address may create `--new-identities-per-hour` new
identities (default 30, 0 disables, loopback exempt unless `--limit-loopback`).
Over the limit the session is dropped silently; returning identities and
guests are unaffected. Counted as `identity_limited` in stats.

### Server flags

| Flag | Meaning |
| --- | --- |
| `--workers N` | Snapshot lanes. 1 = everything on the sim thread, N > 1 builds, encrypts and sends snapshots on N-1 more cores (pinned to `--pin`+1, +2, ...) |
| `--hello-rate N` | Handshake packets per second allowed from one address (default 100, burst twice that, 0 = off). Loopback is exempt unless `--limit-loopback`, so local load tests work |
| `--db-expire-days N` | At startup forget players not saved for N days (default 90, 0 = never) |
| `--db FILE` / `--no-db` | Player database for returning players (default `data/world.db`, SQLite in WAL mode) |
| `--game FILE` | Game definitions (default `game.json`, created if missing) |
| `--key-file FILE` | Server identity key (default `server.key`, created if missing) |
| `--admin ADDR` / `--no-admin` | Admin and console HTTP, loopback only (default `127.0.0.1:7780`) |
| `--ui-dir DIR` | Built UI to serve (default `ui/dist` if present) |
| `--port --hz --world-size --seed --max-clients --npcs --pin --timeout --duration --quiet` | As before |

## World tab: make the world

The console's World tab makes the world from a recipe: seed, size, a walled city or not, how many villages, forts,
crags and lakes, how many rivers, how wooded and how hilly, and hand edits on the map (drag out a house, tower, castle,
wall, crag, lake or square; click something to remove it). Every change runs the generator and shows a preview on the
map; Apply puts it live without disconnecting anyone, and saves it as `world.json`, which the server loads on start.
A different size needs a restart. The recipe travels inside the world file, so the tab always opens on the world as it
is. See [WORLD.md](WORLD.md).

## Building

Players build and break blocks: 2 unit cubes on one grid for the whole world, up and down as well as across, in twelve
materials (stone, timber, red brick, plaster, brick, rough brick, roof tiles, iron, glass, slate, mossy stone and
sandstone). Every column starts in the layer its ground is in, so blocks line up level however the ground slopes, and
any layer can hold a block or not: build a block against the side of another and you have a bridge, leave one out of
a wall and you have a window or a doorway. In the Godot client press B for build mode: a ghost block shows where the
next one goes (the empty cell in front of whatever the mouse is over, green in reach, red out of it), a faint box
marks the block a right click would take out, click to place, right-click to break, 1-9, 0, - and = or the bar along the
bottom to pick the material (the fantasy theme draws them with the Medieval Village textures).

Trees, rocks, boulders and spires are solid too: worldgen gives each one a footprint the size of its trunk or stone,
which the engine treats like any wall. Bushes are walked through.

Blocks are solid ground. You walk up onto anything one block higher than where you stand and drop off any height,
walk across bridges and under them (a body is one block tall), and a wall two blocks high really keeps people out,
bots included. The server sends each character's layer in its snapshot (protocol version 8), and every client's
prediction runs the same rules. Blocks belong to whoever built them: only you can break yours, any of them, not just
the top one (guests' blocks, and blocks built before ownership existed, belong to nobody and anyone can break them).

Blocks are kept beside the world file (`world.blocks.json`), survive a restart, and stay where they are when the world
is regenerated, except where a new wall now stands or new ground buries them. The console shows them on both maps; the
World tab counts them and clears them, and `POST /api/blocks` places whole columns counted from the ground (`{"cols":
[[cx, cy, "01.3"], ...]}`, `.` for an empty layer). The browser play page draws them and moves on them the same way,
but has no build controls yet. Wire details are in [PROTOCOL.md](PROTOCOL.md#building-version-8).

## God tab: spawn, weather, events

The console's God tab is the game master's panel. Everything on it is data in the game file, so it is built in the console, applied live and saved, and none of it changes the wire format or drops a player.

| Part | What it does |
|---|---|
| Spawn | Pick a running bot type and a count, then drop them anywhere, spread over a place, or by clicking the map (the map arms until you press Stop) |
| Weather | Named conditions with a colour, thickness, a bot-sight multiplier (fog, clear night) and attribute drains on everything alive (acid rain: -1 health a second). Start one for N seconds or until cleared; a drain that reaches zero is a death like any other. Players see it: the play page draws haze and falling streaks in its colour |
| Events | A list of steps that run together: announce (a message from "server" to everyone), set or clear the weather, spawn bots in a place, change an attribute for everyone in a place, remove all bots of a type. Fire by hand, or on a timer with a chance each time it comes due |
| Log | What fired and what each step did |

Presets give a start (fog, acid rain, bot wave, strike, storm front, cleanup). Renaming or deleting an attribute, bot type, place or weather rewrites or drops the steps that used it, and a deleted place never turns a local strike into a global one. API: `POST /api/weather {"name": "fog", "duration_secs": 60}` (`name: null` clears), `POST /api/events/fire {"name": "wave"}`, and `/api/state` carries `director` (current weather, event counters, log). `POST /api/spawn` takes an optional `radius` to scatter around `x, y`. With several zones each zone runs its own timers.

## Items and loot

Items are data, like bots. The game file has an `items` list and each bot type can have `drops`:

```json
"items": [{ "name": "medkit", "color": "#ff4d6d", "stack_max": 5, "uses": [{ "attr": "health", "delta": 40 }], "consume": true }],
"archetypes": [{ "name": "hound", "drops": [{ "item": "medkit", "chance_pct": 40, "min": 1, "max": 1 }] }]
```

Every player has an 8 slot bag held on the server. Using, dropping and picking up ride the ordinary input action byte (`200 + slot` uses, `216 + slot` drops, `250` picks up what is within 3 units; game actions must stay below 200), so there is no new client message. The server tells you about your bag (`INVENTORY`) and what lies near you (`GROUND`); see PROTOCOL.md. A bot's drops are rolled where it falls. Things on the ground fade after five minutes, and at most 1,000 exist at once. Bags are saved with the character, by item name, so editing the item list never scrambles one (an item that no longer exists is dropped from bags and the ground).

Items are not part of the wire hash: add, edit or remove them in the **Items** tab and press Apply, and nobody is disconnected. In the God tab you can scatter items anywhere or at a place, and an event can drop them as a step. In the Entity tab you can see what a player carries and give them something. The default game ships eight, from medkits to a data chip, and every bot type leaves something behind.

In the browser: **E** picks up, **Shift + 1 to 8** uses a slot, **G then a number** drops one, and the bag buttons do the same by click or tap. The C SDK and Godot extension have no item getters yet (protocol 6 clients that ignore the two new messages still play fine).

## Browser client

The play page (`--web ADDR`, then open the address) is a full client, not a demo. It is the only client that needs no install.

| What | How it works |
|---|---|
| Responsive movement | Predicts your own character with the same movement step the server runs (`ui/src/play/sim.ts`, a port of `proto::world`), then corrects against each snapshot and replays unacknowledged input. Analogue input from the touch stick is sent as fractions |
| Smooth everyone else | Draws other characters two ticks in the past, blended between snapshots (`interp.ts`); holds the newest on a stall, never extrapolates, does not glide across a teleport |
| Targeting | Click or tap a character, `T` to cycle, `Esc` to release; otherwise the nearest is the target. Range shown live; digits pick among several actions |
| Feedback | Floating damage numbers, hit flash, low-health colouring, "signal lost" overlay while down, reconnect banner, minimap, optional synthesised sound (off by default) |
| Live walls | The page re-reads the world every 15 s, so wall edits made in the map editor reach players without a reload |
| Look | One CSS block (`.play-skin` in `ui/src/index.css`) holds every colour of the play page, in a dark and a light version, and the 3D scene reads it. The current skin is cyberpunk; changing the look is changing those values |

## The game file

One JSON document, validated before it is applied. Attributes, actions and
effects become the `Defs` that clients share and that the handshake hashes.
Bot types ("archetypes") are server-side only.

### Adding bots without a rebuild

Bot types are data, never code. Three ways in, all live, none drop players:

| Way | How |
|---|---|
| Drop a file | Put a `.json` file in `bots.d/` (next to `game.json`; in Docker, `./data/bots.d/`). It is picked up within two seconds. One bot type (an object) or several (an array), in the same shape the Builder writes. Editing the file updates the bot; a typo is reported in the console state (`bot_dir.error`) and changes nothing |
| Console | Bots tab: start from a template (wanderer, guard, hunter, coward, patroller, greeter), or paste JSON, or press Copy JSON on a bot to get a file you can reuse |
| API | `PUT /api/game` as before |

`bots.d/_example.json` is created on first start as a template; files starting with `_` or `.` are ignored. A bot from a file is merged into the game file by name (the file wins) and saved to `game.json`, so deleting the file does not remove the bot; remove it in the console. `--bots-dir DIR` moves the folder, `--no-bots-dir` turns it off.

Smallest useful file:

```json
{ "name": "warden", "color": "#ff3df0", "count": 4, "behaviour": "wander",
  "attack": "zap",
  "rules": [ { "when": { "kind": "sees", "who": "player", "within": 40 }, "then": { "kind": "attack" } } ] }
```

- **Attributes**: range, start value, bit width, replicated or not, regen per second, rule at minimum (nothing or respawn). At most 16, at most 4 replicated.
- **Actions**: id, cooldown, range, requirement on an own attribute, effects on self or target.
- **Bot types**: colour, spawn count, fallback behaviour (idle, wander, chase, flee), speed multiplier, sight, attack action, starting attribute overrides, an optional home place and an ordered rule stack.
- **Places**: named circles (x, y, radius). Spawn points, hangouts, areas a rule can test for.
- **Routes**: named paths of 2 to 64 points, looped or out-and-back.
- **Rules**: first matching movement rule wins, `say` is a side effect and the rules after it still run. Conditions: always, sees a player/bot/anyone within N, attribute below/above, inside a place, chance per decision. Actions: stand still, wander (drifts back to home), chase, flee, attack (chase and use the attack action in range), patrol a route, go to a place, say or shout. Bots decide twice a second, and speak at most once every six seconds. A bot with no matching rule falls back to its plain behaviour, so files from before rules existed load unchanged.
- Editing only bot types applies live and leaves players connected. Editing attributes, actions or settings changes the wire format, so the world restarts and clients are dropped; they get a content-mismatch rejection until they load the same file.

## The visual builder

The console's Builder tab edits all of the above on the map. Click "Add place" and click the map, "Draw route" and click points then finish, drag a place or a route point to move it, drag a rule by its handle (or use its up and down buttons) to reorder, move sliders for speed, sight and each starting attribute (a tick marks the default; returning to the default stores no override). "Drop test bot" spawns the selected bot type where you click so you can watch the rules run. Renaming or deleting a place, route or attribute rewrites every rule that referred to it. Edits are a draft until you press Apply, and bot-only edits apply live without dropping players.

## Players who come back

A client can hold a long-term identity (an X25519 key it keeps). It travels inside the encrypted CONNECT body, so passive observers cannot link sessions to a player, and it is *proven* by mixing a Diffie-Hellman with that key into the session keys: claiming someone else's identity without their secret yields a session that cannot send a valid packet. A client with no identity is a guest and nothing is saved for it.

- **Saving**: when a proven session ends (timeout, replaced, shutdown, world restart) and every 30 seconds for everyone online, the server queues the character (position and attributes by name) to a writer thread that batches into SQLite transactions. The tick never waits on disk. SIGINT and SIGTERM save everyone before exit.
- **Loading**: when a session proves itself with its first authenticated packet, the character is restored: position if it is still a legal spot in the current world, attributes matched by name and clamped to their current range. Adding, removing or renaming attributes in the game file never scrambles a saved character. The read is a primary-key lookup on the sim thread, measured at a few microseconds, and a save is visible to the next load immediately even before it has reached disk.
- **Takeover**: the same identity connecting again does not kick the first session until the newcomer proves itself, so nobody can log you out by claiming your key. Then the old session is saved and ended and the character carries on in the new one.
- Bots: `bots --persist` gives each bot a stable identity from `--seed` and its number, so rerunning with the same seed brings the same characters back.

Not done: no accounts, passwords or recovery (lose the key, lose the character), no limit on how many identities one address can mint (only the handshake rate limit), and old players are only expired at startup.

## Chat

An encrypted reliable channel on top of UDP (`proto::chat`). Client to server keeps one message in flight and resends it every tick until the server's batch acknowledges it; server to client queues every line per recipient and resends the unacknowledged ones each tick until the client's inputs acknowledge them. Delivery is in order and exactly once over a link that loses and reorders packets (tested at 30 percent loss both ways, 8 seeds). `say` reaches players within 64 units (and the speaker), `shout` reaches everyone, once per 10 seconds per player. Text is cleaned on the server (control characters and bidi overrides become spaces, collapsed, 120 bytes), and each player gets a burst of three lines with one more per second. Bots and the console can speak through the same path (`POST /api/say`, `GET /api/chat`).

## Client prediction

`proto::predict::Predictor` applies each input locally with the same `integrate` function the server runs, then on every snapshot snaps to the server position and replays the inputs the server has not seen. The bots use it: live correction is 0.000 at p50 and about 0.06 units at p99, and a lockstep test proves exact agreement through walls over 3,000 random inputs.

## Security model

- **Stateless handshake.** HELLO (padded to 96 bytes) is answered with a 53 byte CHALLENGE carrying a cookie: a keyed hash of the client's address, public key and a timestamp. The server stores nothing and does no key agreement until a CONNECT returns a valid, fresh cookie from the claimed address. Spoofed sources never see the challenge, and replies are smaller than requests, so there is no amplification.
- **Encryption.** X25519 key agreement, BLAKE3 key derivation, ChaCha20-Poly1305 per packet, one key per direction, 32-bit counter nonce, 64-packet replay window. The server's long-term key authenticates it (clients pin it); an ephemeral server key per session gives forward secrecy. Header bytes are authenticated. Cost: 25 bytes per packet.
- **Handshake rate limit.** HELLO and CONNECT, the only packets a stranger can send, pass a per-address token bucket in a fixed-size table (a flood from many addresses cannot grow memory; colliding addresses only share a stricter limit). Established players' game traffic is not limited, and chat has its own limits.
- **Admin interface.** Loopback only, no login. Checks the Host header (DNS rebinding) and requires `X-Admin: 1` on writes (cross-site requests). It refuses to start on a non-loopback address.
- Not done: rekeying (a session ends before its 32-bit counter wraps, years at 20 packets/s), accounts (identity is a key the client keeps; anyone with the server public key can connect as a guest), authenticated REJECT packets (a forged reject can only make a handshake fail).

## The second core

Building each player's personalised snapshot is the largest cost per tick in a crowd, and adding encryption made it bigger. A tick builds one snapshot per player, and nothing about player A's snapshot depends on player B's, so the clients are split across lanes and each lane builds, encrypts and sends its share at the same time. The sim is read-only during that phase, so there are no locks: each lane owns a disjoint slice of clients, its own scratch buffers and its own send queue. Output is byte-for-byte what one lane would produce.

Measured in-process (no network, no load generator competing, 2,000 clients, every snapshot full, one batch send per lane):

| World | 1 lane | 2 lanes |
| --- | --- | --- |
| 2048, sparse | 8.9 ms | 4.9 ms |
| 512, dense (each sees 64) | 19.4 ms | 10.8 ms |

Receiving inputs is split the same way: the sim thread reads each batch off the socket and handles handshakes in order, then with `--workers` above 1 the lanes authenticate and decrypt the DATA packets in parallel (each lane owns a slice of clients, so no locks) and the sim thread applies the results. At 2,000 bots with 2 lanes essentially every input is decrypted on a lane, with zero failed decryptions, and tick p50 went from 17.7 to 15.8 ms (the bots share the second core on this VM, so that understates it).

## Map editor

Walls normally come from the world seed. In the Builder tab, "Edit walls by hand" takes the current walls over,
and the "Draw wall" tool adds a wall by dragging a rectangle on the map. Each wall also has X0/Y0/X1/Y1 fields, a
Show button and a delete button; "Back to generated" returns to the seed's walls and "Clear all" gives an open field.
Hand-placed walls live in the game file as `"walls": [[x0, y0, x1, y1], ...]` (at most 2000, inside the world,
absent = generated). Applying them is live: nobody is disconnected, anything left inside a new wall is moved to a free
spot, the web play page's `/play/info` and the C client library get the new walls, and the file is saved so they
survive a restart. Connected players may see a small correction near a changed wall until they reconnect, because
their prediction used the old walls. Every zone of a scaled-out world must run the same game file.

## How big can the world be

`--world-size` takes 64 to 65,536 units on a side (default 2,048). At the default move speed of 20 units a second that is about 55 minutes edge to edge at the top end. Positions are 32 bit on the wire (protocol 7), which could describe far more; the server stops at 65,536 so that its own 32 bit floats keep a finer resolution than the wire does. Generated walls are capped at 4,000 however large the world, and the neighbour grid and wall index coarsen themselves so memory stays small: an idle 65,536 unit world with 300 bots measured 35 MB and a 4 ms average tick.

The browser does not download the whole map. Worlds with up to 600 walls send them with the join info as before. Bigger ones send only a version, and the page fetches `GET /play/walls?c=cx,cy;cx,cy` (256 unit chunks, at most 64 per request) for the 5x5 window around you, drops chunks more than three away, and builds and removes buildings to match. Editing walls in the console changes the version and open pages start over on their next 5 second poll. Native clients that cannot stream yet get a clear error from `mmo_connect`; fetch `/play/walls?all=1` once and put its `all` array into `boxes` in the info JSON.

Size is not the same as players. A client only receives what is within its interest radius, and a snapshot has to fit in one 1,200 byte datagram, which caps `max_visible` at 255 and in practice at about 70 with a couple of replicated attributes. Zones (below) add CPU and players, not area: every zone runs the same geometry and owns a vertical strip of it.

## Scale out (zones)

One process is one core's worth of sim. To go past that, cut the world into
vertical strips and run one server per strip:

```
export MMO_ZONE_SECRET='at least sixteen random bytes'
cp server.key zone1/server.key   # every zone must share one identity key and game file
server --zone 0/2 --port 7777 --zone-listen 10.0.0.1:9000 \
       --zone-peers 10.0.0.1:9000,10.0.0.2:9000 \
       --zone-public 203.0.113.1:7777,203.0.113.2:7777 --db z0/world.db
server --zone 1/2 --port 7777 --zone-listen 10.0.0.2:9000 (same peers/public) --db z1/world.db
```

Each zone runs the whole world's geometry but owns only its strip: its bots are
kept inside it (`count` in the game file is per zone), and a player who walks
6 units past a border is handed over. The old zone sends the character
(position and attributes) to the new one over TCP, MAC'd with the shared secret
and carrying an increasing sequence number so a captured frame cannot be replayed.
The new zone parks it and acknowledges, and only then does the old zone send
`REDIRECT` (protocol v5 and later, inside the encrypted session, repeated for 3 s in case a
datagram is lost). The client reconnects with the same identity key and the new
zone restores the parked character instead of reading its own database. The old
zone never saves a player it handed over. Network I/O is on background threads,
so a dead neighbour means a failed handoff that retries after 2 s, not a stalled tick.

Browsers follow the redirect too: set `--zone-web http://a:8080,http://b:8080`
so the play page knows each zone's gateway.

Seeing across a border: every tick each zone sends its neighbours the entities within interest range of the shared
border (a second TCP connection, same MAC and replay protection, latest frame wins so a slow link drops frames instead
of queueing them). The neighbour mirrors them as inert ghosts: they take part in interest management, so a player
beside a border sees the other side, but they are not simulated, cannot be targeted, are never saved or counted, and
vanish when the next frame omits them or the neighbour goes quiet for a second. The admin map draws them hollow.

Limits, stated plainly:

- Guests have no identity to hand over, so they are stopped at their zone's border.
- Bots do not cross borders, and nothing crosses a border except players (handoff) and what you see (ghosts):
  you cannot hit, or be hit by, something in the next strip until you are in it.
- Only adjacent zones see each other, so every strip must be wider than the interest radius.
- Zones are a static even split along x; nothing rebalances them.
- The handoff link is plain TCP with a MAC: keep it on a private network. A leaked
  secret lets someone inject characters.
- The admin console is per process; there is no combined view.
- Not measured at scale: the tests run two processes on one machine. The feed costs one pass over the entities per neighbour per tick.

## Measured results

Environment: a 2 vCPU VM, over loopback. Server pinned to core 0, bots on core 1, all traffic encrypted. Loopback charges the receiver's kernel work to the sender, so server numbers are pessimistic, and the load generator shares the machine.

| Scenario | Clients | Lanes | Tick p50 | Tick p99 | Over 50 ms |
| --- | --- | --- | --- | --- | --- |
| Normal density (2048 world) | 2,000 | 1 | 17.1 ms | 32.7 ms | 0 of 400 |
| Dense crowd (512 world, all see 64) | 2,000 | 1 | 30.7 ms | 44.1 ms | 0 of 400 |
| Headroom (2048 world) | 4,000 | 1 | 40.3 ms | 60.9 ms | 80 of 400 |
| Headroom (2048 world), bots share core 1 | 4,000 | 2 | 36.8 ms | 60.1 ms | 14 of 400 |

Re-checked after chat, rules, identity and persistence were added (same VM, same day, A/B against the first commit): the in-process snapshot phase is the same within noise (9.0 to 10.6 ms against 9.2 to 10.0 ms for 2,000 clients, 1 lane), and the live 2,000-client run gave p50 19 to 20 ms, p99 37 to 44 ms against 18.7 and 34.4 for the old build. The live tail is a little worse and the bots now also run prediction on the shared core, so I would not call that difference proven either way. With 2,000 live bots the prediction correction is p50 0.000 and p99 1.0 unit: the server uses the newest input per tick, so a tick that receives two inputs or none shifts a bot by one step until the next snapshot corrects it.

All runs: zero decode errors, zero missing baselines, zero failed decryptions.
For comparison, before encryption the same machine did 2,000 normal-density clients at p50 11.2 ms and 4,000 at 6 of 600 ticks over budget, so encryption costs about 50 percent more CPU per client here. One core now holds roughly 2,000 to 3,000 clients with the gate (2,000 bots at 20 Hz on one core) still passing. The two-lane live run is muddied because the bots occupy the second core on this machine; the in-process table above is the clean measurement.

## Tests

```sh
cargo test --release                 # 59 proto + 27 server unit tests, 26 end-to-end tests (real server, real UDP and HTTP)
(cd ui && npm test)                  # 40 unit and accessibility tests
scripts/ui_e2e.py [--shots DIR]      # 106 real-browser checks incl. axe scans of every tab in both themes, and building a place, a route and rules with the mouse
cargo test -p server --release lane_scaling -- --ignored --nocapture
```

The end-to-end tests include forged, bit-flipped and replayed packets, wrong-address CONNECTs, HELLO floods, wrong content hashes, a full server, idle timeouts, cross-site and DNS-rebinding requests to the admin API, path traversal, hot reload and the bot behaviours.

## Known limits and next steps

- Share candidate lists per grid cell
- Fields are byte aligned. The schema carries bit widths, so bit packing is a protocol-only change
- No accounts or login recovery, no rekeying (see Security model)
- The console possesses bots through the admin API, not through the UDP protocol, so it bypasses the encrypted path; bots-as-clients exercise that
- The game file is JSON on disk. The DB and baker from the plan would produce the same `GameFile`
- World is generated boxes. A real baker would emit heightfields, navmesh and chunks
- Single zone. Zones and cells come next
- Linux and IPv4 only (`recvmmsg`, `sendmmsg`, `sched_setaffinity`)
- Line of sight is not checked for actions
- Moodboard phase of the UI design was skipped; the look is an instrument-panel direction chosen from the brief

3D view: the 2D/3D switch over the map shows the world as a green wireframe
(three.js, loaded on demand). Drag to orbit, scroll to zoom, click an entity to
select it (same selection as the 2D map), and Follow tracks the selected one.
Each bot type has its own silhouette (diamond, square, triangle, ball, hex) and
brightness, players are white cones, hurt entities get a health bar, places are
named on the ground, the selected entity wears a tag, and a legend lists live
counts per type. Walls and places are wireframes, and digits fall in the background (static if you prefer reduced motion). It
reads the same `/api/state` feed as the 2D map and falls back to a message
when WebGL is unavailable.

Default population (40 bots): 12 drifters and 8 civilians (wander and flee),
4 drones and 3 hounds (chase and zap), 3 guards patrolling the ring road,
6 merchants working the market loop who call out to players, and 4 sentries
holding the watchtower who attack anyone who comes close. The last four are
built entirely from the rule builder, so they are good examples to edit.
Existing `game.json` files are not touched; delete yours to get the new defaults.

World persistence: the bot population is saved with the players (every 30 s, on
SIGTERM, and before a world restart) in the same database, and restored at
startup. Each saved bot comes back where it was, with its attributes, matched
by archetype name, so edits to the game file never scramble a saved world.
Archetypes that no longer exist are dropped, and every archetype is topped up
to its `count`. So `count` is the starting population of a new world and the
minimum of a returning one, and bots you added by hand survive a restart.
`--fresh-world` ignores the saved bots, and a world restart caused by an
attribute edit forgets them (the definitions changed). Saving the world is a
JSON dump of the bots on the sim thread, a millisecond or two per few thousand
bots. Rule state such as which waypoint a patrolling bot was heading for is not
saved; it picks up again from the nearest start.

Fixed on the way: a world restart used to save players after replacing the
world, writing blank characters over the good ones. It saves first now, with a
regression test.

Playing in a browser: start the server with `--web 0.0.0.0:7790` (off by default)
and open `http://host:7790/`. Sign in with an account name and passphrase, a
recovery code, or as a guest. Move with W A S D or the arrows, act with space (it
hits the nearest entity in range), Enter to chat, start a line with `!` to
shout. The page runs the same encrypted protocol as every other client, in
TypeScript (`ui/src/play/protocol.ts`, checked against the Rust implementation:
the Argon2id identity and recovery code match byte for byte), so:

- The gateway (`crates/server/src/web.rs`) only relays WebSocket messages to the
  UDP port, one message per datagram. It cannot read or forge anything, and the
  passphrase never leaves the browser.
- The public port serves the play page and `/play/info` (server key, definitions,
  world geometry) and nothing from the admin API, which stays on loopback.
- Every relayed connection arrives from 127.0.0.1, so the gateway tells the
  server each connection's real address and the per-address limits (handshake
  rate, new identities per hour) apply to that, not to the gateway. Behind a
  TLS-terminating proxy add `--web-trust-proxy` so the address comes from
  `X-Forwarded-For`; without a proxy leave it off, because then the header is
  attacker-controlled. TLS itself is the proxy's job: the traffic is already
  encrypted end to end, but browsers want `wss://` on an `https://` page.
- Cross-site pages cannot open a connection (Origin must match Host), at most
  `--web-per-ip` (8) connections per address and `--web-max-conns` (2000) in all,
  and each connection is rate limited and closed after 30 seconds of silence.

Limits of the browser client today: no client-side prediction (you see the
server's position, one network round trip behind, smoothed), every other entity
looks the same because snapshots carry position and replicated attributes but
not which archetype an entity is, and the touch controls are basic arrows.

## Godot client (vertical slice)

`godot/` is a Godot 4.6 project that plays the world through the `mmo_godot` extension: sign in as a guest, camera-relative WASD, orbit and zoom, tab or click targeting, attack, an 8 slot bag, loot on the ground, chat.

### Three separate parts: the engine, the world, the look

The world is data, and what it looks like is a swappable theme; the engine knows nothing about either.

| Part | Where | Changes when |
|---|---|---|
| Engine | `crates/server`, protocol, C SDK | the simulation does: it loads a world file (`--world`), makes its solid objects walls, and serves the file to clients untouched |
| World | `crates/worldgen` writes `world.json` ([WORLD.md](WORLD.md)): heights, rivers and lakes, typed objects | you want a different world: run the generator again (another seed, size or preset), or write another generator. `POST /api/world/reload` applies it live |
| Look | `godot/themes/<name>/` (the contract is `godot/scripts/theme.gd`) | you want a different look: write another theme. `--theme NAME` picks one |

```
cargo run --release -p worldgen -- --size 2048 --seed 7 --out data/world.json
server --world data/world.json --web 127.0.0.1:8081
godot --path godot -- --host 127.0.0.1 --udp 7777 --web 127.0.0.1:8081 [--theme plain]
```

The client (`godot/scripts/main.gd`) only connects, takes input, tracks entities and runs the HUD; it fetches the world file and hands it to the theme, which draws everything. Without a world file the server's own walls are used, as plain blocks on flat ground, so any theme still works. The console's maps draw the same file.

The generator lays out a walled city (curtain wall with towers and four gates, avenues to a plaza and castle, terraced streets, a market square), villages of houses round a green, forts, crags, lakes, rivers that never run uphill and cut gorges through rises, forests and meadows, and mountains past the playable edge. Every footprint is a typed object (`house`, `keep`, `tower`, `castle`, `wall`, `crag`, `lake`), and all of them are solid on the server, so you cannot walk into a house, up a mountain or into a lake; rivers are wadeable.

Themes:

- `plain`: grey ground from the heights, solid footprints as blocks, capsules for people. The base class; it proves the client needs nothing from a theme.
- `fantasy`: KayKit characters (you are a Knight, Barbarian, Mage or Rogue with `--class`; everyone else is one of the skeletons, picked by entity id), dungeon keeps, towers and curtain walls, Medieval Village houses (stone or plaster ground floors, half-timbered and sometimes jettied upper floors, tiled roofs with gables, chimneys and dormers, porches, outside stairs, yards), crags of KayKit rocks, docks on lakes, every tree, bush and rock of the Forest pack picked by each object's variant, water with depth and foam, rain, a daylight sky. Forward+ (bloom, SSAO, volumetric fog); set `renderer/rendering_method` to `gl_compatibility` in `project.godot` for weaker GPUs and those effects are skipped. Its assets and licences are in `godot/themes/fantasy/assets/`. The Bestiary monsters (Imp, Puglin) join the mobs when their models are present there; they are not in git, since their licence (Quaternius Asset License) forbids redistributing them as assets and this repository is public. They borrow the KayKit animations through Godot's humanoid retargeting.

`character.gd` (in the fantasy theme) drives each character from the snapshots: idle, walk or run by ground speed, facing the way it moves, an attack when you act, a flinch and a floating number when health drops, a health bar once hurt. The protocol has no death event and the server respawns in the same tick, so a death is inferred: a recently hit, nearly dead character that jumps across the map or vanishes leaves a body that plays the death animation. When something hits you, the nearest character in reach swings.

Spawned bots carry over every edit of the game file, bot-only or not (matched by archetype and attribute name, as a save and restore does); an archetype whose `count` the edit changes starts again at its new count. A client is sent the nearest `max_visible` entities within `interest_radius` (64 and 128 by default; one snapshot must fit a datagram, so with two replicated 8 bit attributes the most `max_visible` can be is 72), and the Godot HUD says when it is showing only the nearest ones.

Bots can never fill the slots kept for players: archetype spawns stop at the entity table's size minus `--max-clients`, `POST /api/spawn` says how many fitted, and a saved world bigger than that is restored only as far as it fits (the server logs how many it left out).

Space attacks your target when it is in reach, otherwise the nearest thing within reach, and keeps swinging while held; out of reach it says so instead of failing silently. Weather set in the console's God tab (or by an event) shows in the Godot client within a few seconds, as on the play page: the air thickens towards its colour, the sun dims, rain falls around you and the HUD names it; it reads `/play/info` every 5 s. **J** jumps: you are in the air for 0.6 s and attacks miss you meanwhile (a dodge; see PROTOCOL.md), in the Godot client, the play page and the console's 3D view alike. **G** toggles god mode and **F11** full screen (`--fullscreen` starts that way). God mode is an admin call, `POST /api/god {"id": 12, "on": true}` (loopback only, like the rest of the admin API): the character's attributes are held at their maximum every tick, so it cannot die or run out of energy. It is not saved and ends when the character is respawned into a new slot. The client reaches the admin API at `--admin` (default `127.0.0.1:7780`, the server's default).

```
scripts/build_godot.sh                     # builds the extension into godot/bin
godot --path godot -- --host 127.0.0.1 --udp 7777 --web 127.0.0.1:8081
scripts/godot_e2e.sh                       # generate a world, boot the server on it, run a scripted session headless (GODOT=, WORLD=, THEME=, NO_WORLD=1)
godot --path godot -- ... --walk near --zoom 40 --shot out.png --shot_after 10   # screenshot beside the nearest building
```

The extension is built against the Godot 4.6 API and refuses to load in older editors. It now exposes `bag()`, `ground()`, `use_slot()`, `drop_slot()` and `pickup()` (the C SDK has `mmo_bag_*`, `mmo_ground*` and `mmo_item_*` to match). Worlds that stream their walls are loaded whole by the slice (`/play/walls?all=1`), which is fine to a few thousand boxes; streaming by area in GDScript is the next step. Weather and the item `usable` flag are not exposed yet.

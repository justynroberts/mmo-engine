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
| `ui/mcp/` | The console's functions as an MCP server, so an AI can build and run worlds |
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

### Roads, towns and the city

Villages are built along a high street: houses shoulder to shoulder facing it, a green in the middle, a church tower
and a back lane of cottages. The city's streets and avenues are paved, a street runs round inside its walls, and houses
line the roads for a little way out of each gate. A road network joins the city, every village and every fort: it
grows outward from the city, finds the cheapest way over the land (round buildings, up gentle slopes, across as little
river as it can, merging with roads already going the same way) and wades rivers at fords.

Roads are faster underfoot: everyone, players and bots, moves `road_speed` times as fast on them (1.5 unless the game
file's World settings say otherwise, 1 to 4). The server turns the roads into road cells, 4 units square, and sends
them to clients with the walls by tile, so prediction speeds up on exactly the ground the server does. The Godot
client paints them into the ground (packed earth in the country, cobbles in the city); the console draws them on its
maps. Turn them off with the World tab's Roads switch (`"roads": false` in a recipe).

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
| Events | World and local events, raids, bosses: a script that runs over time (see "Events" below). Started by hand, on a timer, when players gather at a place, at a time of day, or after another event |
| Log | What fired and what each step did |
| Weather zones | A weather that stays over one place (`zones: [{name, place, weather, active, height}]`): red mist round a ruin, green mist thick enough to hide a castle, a small spring of healing light (a weather whose drain is positive). Inside it the weather's effects apply (drains, how far bots see) and players see that weather; from outside it is a wall of drifting mist in its colour standing over the place. On/Off live (`POST /api/zone {"name": "red mist", "on": false}`), and an event step switches one (`{"kind": "zone", "zone": "red mist", "on": true}`): raise the mist at dusk, lift it when the boss falls |
| Day and night | The schedule (game file `daylight`): a **cycle** (`day_minutes`, `night_share`), the **real clock** (`sunrise`, `sunset` as HH:MM, `utc_offset_minutes`), or **held** at one moment (`phase`). Dawn / Noon / Dusk / Midnight now buttons move a cycle at once (`POST /api/daylight {"phase": 0.8}`, saved). The server sends the schedule, not the time: every client works out the phase from its own clock, so they agree and nothing goes stale |

### Events

An event is a script. Its steps run in order, straight on until one waits:

| Step | What it does |
|---|---|
| `announce` | A message from the server, to everyone or (`nearby`) only to players at the event's place; a `banner` also goes across the middle of their screens |
| `weather`, `zone` | Set or clear the weather; raise or lift a weather zone |
| `spawn` | Bots of a type at a place (the event's, by default), optionally `name`d: a boss. They belong to this run: killed, they are gone instead of respawning, and they are never saved |
| `hit`, `drop`, `clear` | Change an attribute for everything at a place (hurt or heal), scatter items, remove every bot of a type |
| `wait` | Pause for N seconds |
| `defeat` | Wait until every bot this run spawned is beaten; with `within_secs`, the event fails if they are not |
| `reward` | Items into the bags of the players who took part (were at the event's place while it ran; everyone online, for an event with no place), or `everyone` online. What does not fit lands at their feet |

What makes it a world or a local event is `place`: with one, steps that name no place happen there, `nearby` messages
reach the players within 60 units of it, and those players are the ones taking part. What starts it: the console
(Start now), a timer (`every_secs`, `chance_pct`), or `when`: `{"kind": "players", "count": 3}` (that many players at
its place, or online), `{"kind": "time", "at": "dusk"}` (dawn, day, dusk, night, from the day's schedule), or
`{"kind": "after", "event": "raid", "outcome": "failure"}` (success, failure or either), each at most once per
`cooldown_secs` (600 by default). An event never runs twice at once.

It succeeds by reaching its last step. It fails if a `defeat` or its `limit_secs` runs out; then its `fail_steps` run
instead, and whatever is left of its bots leaves afterwards (they have had their way). On success, any of its bots still
standing stay until beaten. The console shows each run as it goes (which step, what it is waiting for, how many bots
are left and players taking part) with Start now and Stop (`POST /api/events/stop`: it ends there, its bots leave and
nothing follows it).

```json
{"name": "raid", "place": "market", "limit_secs": 600,
 "steps": [
   {"kind": "announce", "text": "Raiders are marching on the market!", "banner": true},
   {"kind": "wait", "secs": 30},
   {"kind": "spawn", "archetype": "raider", "count": 8},
   {"kind": "wait", "secs": 45},
   {"kind": "spawn", "archetype": "raider", "count": 12},
   {"kind": "spawn", "archetype": "brute", "count": 1, "name": "Gorrak the Red"},
   {"kind": "defeat", "within_secs": 240},
   {"kind": "announce", "text": "The market holds.", "banner": true},
   {"kind": "reward", "item": "medkit", "count": 2}],
 "fail_steps": [{"kind": "announce", "text": "The market has fallen.", "banner": true}]}
```

Banners travel on chat channel 3 (from the server, id 0), so clients that do not know it show them as a chat line;
the Godot client and the play page put them across the screen.

Presets give a start (fog, acid rain, bot wave, strike, storm front, raid, world boss, night terror, defend, cleanup). Renaming or deleting an attribute, bot type, place or weather rewrites or drops the steps that used it, and a deleted place never turns a local strike into a global one. API: `POST /api/weather {"name": "fog", "duration_secs": 60}` (`name: null` clears), `POST /api/events/fire {"name": "wave"}` (409 if it is already running), `POST /api/events/stop {"name": "wave"}`, and `/api/state` carries `director` (current weather, event counters and how each last ended, `runs` in progress, log). `POST /api/spawn` takes an optional `radius` to scatter around `x, y`. With several zones each zone runs its own timers.

In the dark the Godot client lights the world: you carry a torch that lights itself when it gets dark (L: on when
dark, always, never), and so does everyone near you; squares have campfires, keeps and towers braziers, and some
houses a lantern by the door (all placed by the theme from the world file, so every client draws the same); and two
building materials give light, the lamp (`[`) and the fire (`]`). A pool of 24 real lights goes to whichever sources
are nearest, so a town, or a player who builds a thousand lamps, costs the same.

**Sneaking.** Give the game file an attribute called `lit` (replicated, min 0, max 1, default 1; the default game has
one) and the server knows whose light is on: a client sends it (`ACT_LIGHT`, action 254, or `mmo_set_light` in the C
SDK, which rides an input with no other action so it never displaces one). In the dark, other players' clients hide the
name and health bar of anyone whose light is out, and at night bots see an unlit player only at a third of their range
(`SNEAK_SIGHT`). A replicated attribute costs a byte per visible character in each snapshot, so a game already at the
budget lowers `max_visible` a little (72 to 67 with three others).

## Clear down

The console's World tab clears what players leave behind: built blocks (all, one account's, or nobody's, the guests'),
or items on the ground, everywhere or inside one place. `POST /api/blocks/clear {"owner": "27e5f44f38eb50e5",
"place": "plaza"}` and `POST /api/ground/clear {"x": 300, "y": 400, "r": 50}`; every filter given must match, and an
empty body clears everything.

## Sound and music

Everything the Godot client plays is made by `scripts/make_audio.py` (numpy, scipy and ffmpeg; no samples, nothing to
license) into `godot/sounds`: twelve seamless ambience loops, the effects, and six pieces of music in the old Irish
modes (a slow air for the open land, a jig for towns, a night piece, a battle piece, the eerie and the holy), played on
a plucked harp, a low whistle, a fiddle, drones and a bodhrán.

Each mood also has more music beside its synthesised piece (`godot/sounds/music/<mood>_N.ogg`): pieces made for the
game by the project owner (the "Mist over the Isles" airs, "Cavernous Dissonance", "Modal March" and the "Ar Aghaidh"
title theme) and Celtic and medieval pieces by Kevin MacLeod (incompetech.com, CC BY 4.0). Every track, its source and
licence are listed in `godot/sounds/music/CREDITS.md`; keep that file complete when adding music. The client plays a
random piece of the mood through (not the one just heard), leaves a little silence between pieces in the quieter moods,
and crosses over when the mood changes.

The client listens to the land as you move (`scripts/sound.gd`, fed by `main.gd`): how much sea is round you, how near
a river is, the trees, the houses, the nearest fire, how high you stand, the weather and the hour, and blends the sea,
river, birdsong or crickets and owls, wind, rain, fire and town murmur to match, easing between them. Music follows the
moment: a fight (struck or striking) turns it to battle, a town to the jig, nightfall to the night piece. A place can
have its own `sound` (sea, river, forest, night, wind, rain, fire, town, market, crypt, holy, battle) and `music`
(explore, town, night, battle, eerie, holy, city, none): inside it, its sound comes up over the land's and its music plays.
Set them in the console's place editor or through the MCP. Effects: footsteps by what is underfoot (grass, road,
cobbles, water), a splash wading in, swings and hits, pickups and drops, placing and breaking blocks, a horn under
event banners and a chime walking into a named place. N cycles sound and music, music off, and silence.

## Building worlds with an AI (MCP)

Everything the console does is also an MCP server, so an AI assistant (Claude Code, Claude Desktop, any MCP client)
can build a world by talking to it: mark places and name them, make bot types and home them, write raids, lay out
buildings in blocks, change the land's recipe, set the weather and the time, and start events.

```sh
cd ui && npm run build:mcp                      # writes ui/dist-mcp/mmo-mcp.mjs (one file, no other dependencies)
claude mcp add mmo -e MMO_ADMIN=http://127.0.0.1:7780 -- node "$PWD/dist-mcp/mmo-mcp.mjs"
```

The repo's `.mcp.json` registers it for Claude Code sessions opened here. It speaks to the server's admin API, so
it needs the same access the console has: the admin port only listens on loopback, so for a remote server tunnel
it first (`ssh -L 7780:127.0.0.1:7780 host`). With zones, each zone has its own admin port; `use_server` switches
between them.

| Tools | What they do |
| --- | --- |
| `overview`, `game_schema`, `get_game` | Where things stand; the game file's types with comments; the file or one section |
| `upsert`, `rename`, `remove`, `add_preset` | Edit places, routes, bot types, items, weather, weather zones, events, attributes and actions by name, applied live. They use the console's own editing code, so a rename or removal rewrites every rule, home and event step that named it |
| `set_settings`, `walls`, `put_game` | Movement settings and the daylight schedule; hand-placed walls; the whole file |
| `list_entities`, `get_entity`, `spawn`, `remove_entities`, `drive`, `release`, `say`, `read_chat` | Who is where, and moving, making and removing bots |
| `give_item`, `drop_items`, `clear_ground_items`, `god_mode` | Items and players |
| `set_weather`, `switch_weather_zone`, `set_time`, `event`, `director_status` | The sky and events, now |
| `build`, `get_blocks`, `clear_blocks` | Blocks: `fill`, `walls` (a building's outline) or `line` over world coordinates, from a layer and so many high, in any of the 14 materials; `air` cuts doors and windows |
| `get_world`, `world_preview`, `world_apply` | The land: its recipe and what stands on it, a new recipe tried, then made real |

Two guards: a change to attributes, actions or movement settings restarts the world and disconnects everyone, so
those tools refuse unless called with `allow_restart: true`; and `clear_blocks` / `clear_ground_items` never clear
the whole world unless asked with `everything: true`. Tools that take things away are marked destructive, so an MCP
client asks before running them. `node ui/mcp/smoke.mjs http://127.0.0.1:PORT` builds and removes a small scene
through the MCP protocol against a throwaway server.

## Spells

Every action after the first (the plain attack) is a spell on the Godot client's spell bar (keys R T Y U I O P Z C H K and comma). A spell is an ordinary action in the game file with a few optional fields, none of them in the content hash, so they can be tuned live from the console's Actions editor, the MCP `upsert` tool or the game file:

| Field | Meaning |
|---|---|
| `aim` | `"target"` (default) or `"self"`: a self heal, a nova round you |
| `area` | radius (0..64) round the target, or round you when aimed at self: effects "to target" land on everyone in it |
| `hits` | `"any"` (default), `"foes"` or `"friends"`: players are one side and bots the other. A spell for friends with no friend picked lands on you |
| effect `secs` | spread an effect's `delta` over that many seconds (0..60): burns, curses, heals over time |
| `hold` | `{"kind": "mez" \| "stun" \| "root", "secs": 0..30}`: mez stops moving and acting until it runs out or the target is hurt; stun does the same and is not broken; root stops only moving |
| `fx` | how it looks and sounds: `fire`, `frost`, `shock`, `holy`, `shadow`, `nature`, `arcane`, `poison`, `mind`, `blood` (empty: a plain blow) |
| `bolt` | drawn flying from the caster to the target |
| `school` | free text to group spells by, e.g. `"dubh"` and `"geal"` for black and white magic |
| `desc` | what it does in plain words, shown when a player hovers over the spell (write it in the players' language, whatever the spell is called) |
| `lift` | levitate everyone it lands on for that many seconds (0..300): blows and areas on the ground miss them |
| effect `lasts` | take the effect back after that many seconds (0..300): buffs and curses that wear off (not with `secs`) |

A game that defines the reserved attribute `speed` (replicated, default 1) has it multiply movement, and clients predict their own steps at that speed, so `{"to": "self", "attr": "speed", "delta": 0.6, "lasts": 10}` is a ten second haste and a negative one a slow. Each action has its own cooldown (`cooldown_ticks`), and any action also pauses the others for at most half a second. Costs are the usual `requires` plus an effect on yourself (`{"to": "self", "attr": "mana", "delta": -15}`), with a `mana` attribute that regenerates. A game that defines the reserved attribute `held` (replicated, 0..3) lets clients see who is held, and stop moving while they are. Bots cast too: give a bot type a spell as its `attack`. Everyone within your interest radius sees each use (the CASTS packet, PROTOCOL.md): a glow at the caster's hand, a bolt, a burst or a sweeping ring for an area, rising sparks for a heal, a swirl over anyone held, with a cast and an impact sound for each look.

**Player against player.** `pvp` in the game file (on by default; the console's World settings, or the MCP `set_settings` tool) lets players harm each other with blows and spells for foes. A place with `"safe": true` stays peaceful: no player can harm another who stands in it, or harm anyone while standing in it. Heals and other spells for friends always land on players. Both apply live, and clients show PVP or SAFE under the player's name.

Example, a fireball that also burns for four seconds:

```json
{"id": 2, "name": "liathroid tine", "cooldown_ticks": 40, "range": 24, "requires": {"attr": "mana", "min": 15},
 "effects": [{"to": "target", "attr": "health", "delta": -22}, {"to": "target", "attr": "health", "delta": -10, "secs": 4},
             {"to": "self", "attr": "mana", "delta": -15}],
 "hits": "foes", "fx": "fire", "bolt": true, "school": "dubh"}
```

## Experience, levels and quests

`levels` in the game file turns on experience: `{"cap": 20, "base": 100, "exponent": 1.6, "might": 0.06, "toughness": 0.05, "discover_xp": 10}`. Level L needs `base * (L - 1) ^ exponent` experience in all. A bot type's `xp` goes to every player who hurt one in the last half minute when it falls; finding a named place (one with a title) gives `discover_xp` once; quests give their `xp`. Each level above the first makes a character's blows and heals land `might` harder and divides harm taken by `1 + toughness` per level, so health bars stay bars; a bot type's `level` puts it on the same footing. An action's `level` is the level a player learns it at. A replicated attribute named `level` lets everyone see levels on nameplates; your own experience comes privately (the PROGRESS packet). All of it applies live and is saved with the character.

A quest (`quests` in the game file) is given by a bot type, done step by step and handed in for its rewards:

```json
{"name": "brigands on the road", "title": "Brigands on the Road", "giver": "laoch", "level": 3,
 "text": "Brigands rob anyone on the road south...", "done_text": "The road is safer for it.",
 "steps": [{"kind": "visit", "place": "dibheirgigh"}, {"kind": "kill", "archetype": "dibheirgeach", "count": 4}],
 "xp": 260, "reward": [{"item": "pingin airgid", "count": 5}]}
```

Steps are `kill` (beat N of a type: it counts for everyone who hurt the one that fell and for players within 40 units), `visit` (walk into a place), `collect` (carry N of an item, taken on hand-in) and `talk` (speak to one of a type). `turn_in` hands it to another type, `after` chains quests, `level` holds one back, `repeatable` lets it be taken again. Players talk with F (to whoever has a gold ! or ? over them), see the offer and take it or not, follow it in the tracker under the minimap, and read the quest log with F3. Quests are edited in the console's Quests page or through the MCP (`upsert` with section `quests`), apply live, and are kept by name with each character. A game holds up to 512 quests and a player up to 25 in hand at once; the QUESTS packet carries the ones done as a bitmap, so a character who has done them all still gets one packet.

**Encounters.** The first blow between a bot and a player, either way, locks the bot onto that player (`crates/server/src/encounter.rs`). From then on its rules aim at them however far they get and whoever else is nearer, and a bot with an attack fights back whatever its rules say, unless a rule says to flee. The lock breaks when either falls, when the player gets further away than the bot type's `leash` (three times its sight when absent, at least 60 units), reaches a safe place, or when no blow passes for 10 seconds; the player is told the bot gave up. Each player hears which bots are locked on them (the ENCOUNTER packet, `mmo_encounter` in the C SDK), and the Godot client plays the battle music for as long as any are.

## Parties, whispers, shops, gear and mounts

**Whispers and who.** A chat line starting with `/` is a command: `/w Name words` whispers (a name with spaces goes in quotes), `/r words` answers the last whisper, `/who` lists who is on with their levels, `/name New Name` renames you.

**Parties.** `/invite Name`, then `/accept` or `/decline`; `/leave`; `/p words` talks to the party; `/party` lists it. Up to five. A bot that falls gives its experience, and "beat N" quest steps, to every member within 80 units as well as to whoever hurt it. Members see each other's names and health under their own frame (the PARTY packet).

**Shops and money.** `currency` names the item that is money. A bot type's `shop` lists what it sells (`[{"item": "claiomh", "price": 25}]`); talk to one (F) to open the shop, buy what it sells and sell anything with a `value`. Traders are marked "trader" on their nameplate.

**Gear.** An item with `"equip": {"slot": "weapon" | "armour" | "trinket", "might": 0.15, "toughness": 0.1}` is worn by using it (what was in that slot goes back to the bag), and adds its might and toughness to the wearer's level's. The journal (F2) shows what is worn, with Take off.

**Mounts.** An item with `"mount": 0.8` is ridden by using it: it adds that to the `speed` attribute until you use it again, use an action, or are hurt. Clients draw a horse under anyone riding.

All of it is game file data, edited live in the console (Items, Bot types, World settings) or through the MCP; what is worn and the party are private packets (GEAR, PARTY), and gear is saved with the character.

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

## Names

Everyone has a name (`proto::names`, PROTOCOL.md "Names"). A player claims one over the chat channel (the browser sends
the account name on sign in; type `/name Ana Lee` in either client's chat, or start Godot with `--name "Ana Lee"`), and the
session is the proof of whose it is. The server checks it (3 to 16 letters, readable, nothing that passes for staff),
keeps it unique ignoring case and marks, saves it with the character and keeps it taken while its player is away. Until
they claim one, a player goes by a name made from their account tag; bots go by their type. Clients look names up at
`/play/names?ids=...` on the web gateway, for nameplates, the target frame and chat; the console shows them in the
entity list and the chat log.

## Locations

A place with a `title` ("Market Square") is a location. Set it under Places in the console's builder ("Shown to players
as"); it applies live. Walking in, the title fades in at the top of the screen, holds a few seconds and fades away;
walking out, "Leaving Market Square" does the same, smaller. Where locations overlap the smallest wins, so a village
inside a forest is the village, and back out in the forest the forest's title shows again. Godot also puts it in the
line under the minimap. Places without a title stay what they were: spawn points and targets for bots and events.

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

The browser does not download the whole map. Worlds with up to 600 walls send them with the join info as before. Bigger ones send only a version, and the page fetches `GET /play/tiles?v=<ver>&c=cx,cy;cx,cy` (256 unit tiles, at most 64 per request, a compact binary reply, gzipped and cacheable for good because the version is in the URL) for the 5x5 window around you, drops tiles more than three away, and draws what it holds with three instanced draw calls: buildings, their outlines, and solid scenery. Editing walls in the console changes the version and open pages start over on their next 5 second poll. Native clients stream the same way through the C SDK (`mmo_tiles_wanted`, `mmo_put_tiles`, `mmo_set_walls`); the host only does the HTTP.

How far it goes, measured in Docker on a laptop (8 cores): a generated 16,384 unit world has 230,613 walls (trees and rocks are solid) and loads in 1.6 s at 111 MB resident; a 32,768 unit world has 917,479 walls, 169 MB of JSON, and loads in 5.5 s at 298 MB. Either way no tile holds more than about 200 walls, a 5x5 window is 34 KB (24 KB gzipped) and is answered in under a millisecond, and a live reload of the 32,768 world runs on the admin thread while the tick's p99 stays near 2 ms. The world file is parsed as a stream, keeping only solid footprints, the size and the heights. Clients take the file in parts too (WORLD.md): a head of every building, the heights and the water, then trees and rocks by tile, so the Godot client joins a 32,768 world in 2.7 s at 660 MB (it was 13.5 s and 2.6 GB taking the whole file), and a 65,536 world (6.2 million objects, 3.66 million walls) in 4.5 s at 820 MB, where the server starts in 7.5 s at 955 MB. worldgen writes its file as a stream, so that world takes it 1.4 GB and 33 s. The limits are 4,096 walls touching any one tile and 4,000,000 in all.

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

### Zone lines: maps of their own

Strips cut one world. Zone lines join separate ones, EverQuest style: a valley, a city, a barrow under the hill, each
its own server process with its own world file, game file (places, bots, weather, daylight) and database. Players cross
by walking into a place that leads somewhere:

```json
{"name": "barrow door", "x": 990, "y": 860, "r": 4, "title": "Barrow of Morvane",
 "exit": {"zone": "barrow", "x": 384, "y": 372}}
```

Walk in and the character is handed to the barrow exactly as at a strip border, arriving at 384, 372. Leave `zone`
out and the exit leads somewhere on the same map (a drop shaft, a portal), which needs no zones at all and works for
guests too. In the console it is "Leads somewhere" on a place. Arriving inside an exit does not take you through it:
an exit fires when you walk in.

```
server --zone 0/2 --zone-lines --zone-names valley,barrow --world valley.json --game valley-game.json \
       --zone-listen 10.0.0.1:9000 --zone-peers 10.0.0.1:9000,10.0.0.2:9000 \
       --zone-public 203.0.113.1:7777,203.0.113.2:7777 --zone-web https://valley.example,https://barrow.example
server --zone 1/2 --zone-lines --zone-names valley,barrow --world barrow.json --game barrow-game.json (same peers, public, web)
```

What must match across zones: the server key and the attributes, actions and settings (the content hash), since the
client's handshake is checked against them. Everything else may differ. The Godot client darkens the screen
("Travelling to Barrow"), loads the new zone's world and connects there; the browser does the same, reading the next
zone's info from its `--zone-web` address.

Characters: the zone a player leaves keeps a note of where they went, instead of a stale copy, so signing in there
later sends them straight on to the zone that has them. Guests have no character to hand over, so they cannot leave
their map. Names are unique per zone, not across them. A zone that is down closes the ways into it ("The way to the
barrow is closed for now"); a player whose character is in a zone that is down cannot sign in elsewhere until it is up.

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

### Slow and lossy players (`scripts/latency.sh`)

The question: does one player on a bad connection cost anyone else anything? `bots` emulates a network per bot (a
one-way delay, jitter that reorders packets, and loss, each way; `--latency`, `--jitter`, `--loss`, and a worse link
for the last `--slow N` bots). `scripts/latency.sh` runs the same server twice with the same number of clients: once
with the extra group on a healthy link (control), once on a bad one (test), so only the link differs. Healthy bots on a
perfect link, server on three cores (3 lanes), bots on the other five, all in Docker on an 8-vCPU laptop VM, 4096 world,
40 s:

| Clients | The extra group's link | Healthy bots, input to ack p50 / p99 | Server tick p50 / p99 | Reconnects |
| --- | --- | --- | --- | --- |
| 3,600 + 400 | healthy (control) | 50.1–50.4 / 142–158 ms | 14.5 / 91.5 ms | 0 |
| 3,600 + 400 | 400 ms + 150 jitter, 10% loss | 50.6–51.1 / 143–178 ms | 17.4 / 105 ms | 0 |
| 3,000 + 1,000 | healthy (control) | 50.1–51.8 / 114–122 ms | 16.8 / 83.0 ms | 0 |
| 3,000 + 1,000 | 1,000 ms + 500 jitter, 30% loss | 50.1–50.4 / 97–117 ms | 14.2 / 56.2 ms | 0 |

A slow player costs the others nothing measurable: their latency does not move, and every slow bot stays connected
(30% loss with 1 to 1.5 s delay still plays, at about 6 snapshots a second). What a slow player does cost is a little
server CPU: one whose acks take longer than `MAX_BASELINE_AGE` (12 ticks) never has a delta baseline, so it gets full
snapshots (bounded by `max_visible`). The tick's p99 in all these runs is the laptop VM, not the load: the same stalls
show in the control runs.

The first run of this test found a real problem: after one long tick (here a VM stall), the next tick read and handled
the whole backlog in the socket's 14 MB buffer, took longer still, and in a few ticks every client had timed out (a
4.3 s tick, all 4,000 bots reconnecting). The receive phase now gathers a tick's packets and handles them in one go, and
handles at most four per connected client per tick (or a third of a tick spent reading); the rest of a backlog is read
and dropped ("shed" in the server's per-second line and summary). A stall now costs everyone a little loss, which the
protocol shrugs off, and the server is back in step a tick or two later.

## Tests

```sh
cargo test --release                 # 59 proto + 27 server unit tests, 26 end-to-end tests (real server, real UDP and HTTP)
(cd ui && npm test)                  # 40 unit and accessibility tests
node ui/mcp/smoke.mjs URL            # the MCP server against a throwaway server's admin URL
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

Lettering: titles (names, location and zone titles, banners, headings) are Uncial Antiqua, and everything read at
length (chat, bars, help, nameplates) is Alegreya with level figures for numbers. Both are SIL Open Font License, in
`godot/fonts` with their licences; `fonts/title.tres` and `fonts/body.tres` say which file and settings each uses, so
changing the look is a one-line edit there.

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

### Packaging it for other players

`scripts/package_godot.sh [server]` builds the extension in release and exports `dist/mmo-client-mac.zip`, a macOS app (Apple silicon) that opens on a join screen asking for the server's address and a name; the address starts as `server` (default: this Mac's LAN address), and both are remembered for next time. A bare host means the web gateway on 7790, and the UDP port comes from `/play/info`. Players need UDP 7777 and TCP 7790 to reach the server. The admin API stays on loopback, so god mode works only on the server's own machine. It needs Godot's macOS export template for 4.6.2. With a Developer ID Application certificate in the keychain, the script signs the app (hardened runtime), notarises it through the `notarytool` keychain profile (or `NOTARY_KEYCHAIN_PROFILE`) and staples the ticket, so it opens on any Mac without warnings. Without the certificate the app is ad-hoc signed and testers must run `xattr -cr` on it first. If the first handshake lapses because a cold first launch is compiling shaders, the client tries again (up to three times) before showing an error.

The extension is built against the Godot 4.6 API and refuses to load in older editors. It now exposes `bag()`, `ground()`, `use_slot()`, `drop_slot()` and `pickup()` (the C SDK has `mmo_bag_*`, `mmo_ground*` and `mmo_item_*` to match). Worlds that stream their walls are fetched by tile as you move (`tiles_wanted`, `put_tiles`, `set_walls`); with no world file the streamed walls are also what the plain theme draws. Weather and the item `usable` flag are not exposed yet.

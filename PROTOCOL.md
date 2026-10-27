# Wire protocol, version 11

Everything is UDP, little-endian, byte aligned. The reference implementations are
`crates/proto` (Rust) and `ui/src/play/protocol.ts` (TypeScript, cross-checked against Rust).
If you are integrating an engine, use the C library in `sdk/` instead of porting this.

Primitives: X25519 (RFC 7748), ChaCha20-Poly1305 (RFC 8439, 12 byte nonce, 16 byte tag),
BLAKE3 (`derive_key(context, input)` is BLAKE3's key-derivation mode; `keyed_hash` is its MAC mode),
Argon2id for passphrase accounts.

## 1. Handshake

```
C -> S  HELLO      0x01 | version u16 | C ephemeral pub [32] | zero padding to 96 bytes
S -> C  CHALLENGE  0x84 | S static pub [32] | timestamp u32 | cookie [16]            (53 bytes)
C -> S  CONNECT    0x03 | version u16 | C pub [32] | timestamp u32 | cookie [16]
                   | AEAD(body = content_hash u64 | identity pub [32]) | tag [16]     (111 bytes)
S -> C  ACCEPT     0x85 | AEAD(body = S ephemeral pub [32] | session id u32 | welcome) | tag [16]
```

- The server keeps no state until ACCEPT. The cookie is `keyed_hash(cookie_key, ip4 | port(BE u16) | C pub | timestamp)`
  truncated to 16 bytes; it must be under 30 s old. HELLO must be the full 96 bytes (so the reply is smaller than the request).
  Resend HELLO, then CONNECT, every ~250 ms until the next message arrives; a retransmitted CONNECT gets the same ACCEPT.
- The client **pins** the server's static public key (from `/play/info`, `server_key`) and aborts if the CHALLENGE differs.
- `content_hash` must equal the server's (`/play/info`, hex u64); otherwise the server sends `REJECT 0x83 | 2`.
  Other rejects: `1` bad version, `3` server full. REJECT is unauthenticated.
- Identity: 32 bytes, the X25519 public key of a long-term secret the client keeps; all zero means guest (nothing is saved).

Key schedule (`derive(ctx, parts...)` = `blake3::derive_key(ctx, concat(parts))`):

```
dh1    = X25519(C eph secret, S static pub)
k_c2s0 = derive("mmo-engine 1 hs c2s", dh1 | C pub | S pub)       nonce 0, AAD = CONNECT bytes [0..55]
k_s2c0 = derive("mmo-engine 1 hs s2c", dh1 | C pub | S pub)       nonce 0, AAD = 0x85
dh2    = X25519(C eph secret, S eph pub)                           (S eph pub is in ACCEPT)
dh3    = X25519(C identity secret, S eph pub)   or 32 zero bytes for a guest
parts  = dh1 | dh2 | dh3 | C pub | S pub | S eph pub | identity pub
k_c2s  = derive("mmo-engine 1 session c2s", parts)
k_s2c  = derive("mmo-engine 1 session s2c", parts)
```

Mixing `dh3` is what proves the identity: a client that only claims someone else's identity cannot derive the keys.
Reject any all-zero X25519 output.

Welcome (inside ACCEPT): `0x81 | entity id u32 | tick u32 | tick_hz u8 | world_size u32 | world_seed u64`.

## 2. DATA packets

```
0x10 | session id u32 | counter u32 | AEAD(inner message) | tag [16]
```

Nonce = counter u32 followed by 8 zero bytes; AAD = the 9 header bytes. One key per direction, a counter per direction
starting at 0, and a 64-packet replay window on receive. Only authenticated packets move the window.
Keep datagrams under 1200 bytes.

Inner messages:

| Kind | Dir | Layout |
|---|---|---|
| `0x02` INPUT | C->S | `seq u32 | ack_tick u32 | dir_x i8 | dir_y i8 | action u8 | target u32 | arg u16 | chat_ack u32` (22 bytes) |
| `0x04` CHAT_SEND | C->S | `cseq u32 | channel u8 | len u8 | utf8 text` (len <= 120) |
| `0x82` SNAPSHOT | S->C | see section 3 |
| `0x86` CHAT_BATCH | S->C | `ack_cseq u32 | n u8 | n x (seq u32 | from u32 | channel u8 | len u8 | text)` |
| `0x87` REDIRECT | S->C | `zone u8 | ip [4] | port u16`: reconnect there (section 5) |
| `0x88` INVENTORY | S->C | `n u8 | n x (item+1 u8, count u8)`: your bag (24 slots), then your bank (24 more), sent when either changes and every second (item+1 of 0 is an empty slot) |
| `0x89` GROUND | S->C | `n u8 | n x (id u32, x u32, y u32, item u8, count u8)`: up to 40 items lying nearest you, every half second; positions quantised like entities |
| `0x8B` PROGRESS | S->C | `level u8 | xp u32 | at u32 | next u32`: your level and experience (`at` what this level needed, `next` the next one's, 0 at the cap), sent when they change and every few seconds |
| `0x8C` QUESTS | S->C | `offer u16 | n u8 | n x (quest u16, step u8, count u16, state u8) | m u8 | m bytes`: your quests by index in `/play/info` quests and a quest a giver has just put to you (0xFFFF for none), sent when they change. The n are the quests in hand (state 0) or ready to hand in (1), at most 25; the m bytes are a bitmap of the quests done (state 2): bit `k % 8` of byte `k / 8` for quest k. A game has at most 512 quests, so a player who has done them all still fits one packet |
| `0x8D` PARTY | S->C | `n u8 | n x id u32`: your party, you among them (n 0: in none), sent when it changes |
| `0x8E` GEAR | S->C | `weapon u8 | armour u8 | trinket u8 | riding u8`: what you wear (item index + 1, 0 for nothing) and whether you ride, sent when it changes (only in games with gear or mounts) |
| `0x8F` ENCOUNTER | S->C | `n u8 | n x id u32`: who is in a fight with you (n 0 when nobody is), the bots locked on you first, then players. A bot locks on when either of you hurts the other and lets go when one of you falls, you get further than its leash, reach a safe place, or no blow lands for 10 s. A player (pvp) is in it from a blow either way until one of you falls or neither lands a blow on the other for 8 s. Sent when it changes and every 50 ticks while there are any; clients play battle music while it is not empty |
| `0x90` CROWD | S->C | `part u8 | parts u8 | n u8 | n x (id u32, x u16, y u16, side u8)`: everyone past your snapshot out to twice the interest radius, nearest first, up to 480 in up to 4 parts of 120, four times a second. Whole world units; `side` is the faction (index + 1, 0 for none) with the top bit set for a player (a bot's is its type's faction). Hidden entities (indoors, fallen) are not in it. Replace a part's entries when that part comes again and drop parts at or past `parts`; an emptied crowd comes once as one empty part. Anyone your snapshot carries is never in it. Draw them simply; clients that do not know this kind ignore it |
| `0x8A` CASTS | S->C | `n u8 | n x (seq u32, caster u32, target u32, action u8)`: actions (blows and spells) used by anyone within your interest radius in the last 3 ticks, so each is sent about three times; `seq` counts up per use, so a repeat is known by it. `target` is 0 when it landed on the caster (aimed at self, or a friendly spell with no friend picked). Clients that do not know this kind ignore it; it carries nothing the simulation needs |

Send INPUT at the server's tick rate (20 Hz). `dir_x`/`dir_y` are -127..127 intent, never speed. `seq` increases;
the server applies the newest. `ack_tick` is the newest snapshot tick you hold. `action` is an action id (0 = none) with a
`target` entity id; send it once. Chat is reliable on top of UDP: resend CHAT_SEND until `ack_cseq` in a batch covers it;
`chat_ack` in INPUT tells the server the highest batch `seq` you have. Channels: 0 say (heard within 64 units), 1 shout
(heard by all, one per 10 s), 2 name (client to server only: claim a display name, see Names). A batch line `from` 0 is
the server speaking; entity slot 0 is never used, so no entity has id 0.

## 3. Snapshots

```
0x82 | tick u32 | baseline_tick u32 | last_input_seq u32 | n_changed u16 | n_removed u16
     | n_changed x entry | n_removed x entity id u32
entry = id u32 | x u32 | y u32 | z i8 | one value per replicated attribute (u8 if bits <= 8 else u16)
```

- Entries are sorted by id. `baseline_tick = 0` means a full snapshot. Otherwise it is a delta against the snapshot of that
  tick, which you must still hold (keep the last 16 by `tick % 16`); if you do not, drop the packet. The server only uses a
  baseline you acknowledged with `ack_tick`.
- State = baseline + changed (replaces by id) - removed. Ignore a snapshot whose tick is not newer than the last applied.
- `x`, `y` are `value * 16` rounded, so world units = `q / 16`. Attributes: `min + q / (2^bits - 1) * (max - min)`.
- `z` is the block layer the entity's feet are in (see Building): -128 on the bare ground, otherwise it stands at
  `z * 2` world units up, on top of the block in layer `z - 1`.
- The replicated attributes and their order come from `/play/info` (`replicated` indices into `attrs`, at most 4).
- `last_input_seq` is the newest INPUT the server applied: use it for round trip time and for client-side prediction
  (re-apply unacknowledged inputs on top of the authoritative position).
- You only receive entities within the interest radius of your own entity (which is itself in the list).

## 4. Accounts

A passphrase account is `identity = Argon2id(passphrase, salt, m = 19456 KiB, t = 2, p = 1, 32 bytes)`, salt =
the first 16 bytes of `blake3::derive_key("mmo-engine 1 account salt", lowercase(trim(account)))`.
Recovery code: the 32 bytes as 8 groups of 8 hex digits plus a 4 digit checksum
(`keyed_hash(derive_key("mmo-engine 1 recovery checksum", ""), secret)`, first 2 bytes), joined with `-`.

Test vector: account `Justyn`, passphrase `x y z` gives recovery code
`299756db-cbb23a83-fe82fa08-88fc5bac-04dcb88e-4716273c-47751076-12676686-00f9`
(account `"  justyn "` gives the same).

## 5. Zones

When the server is one zone of several it may send REDIRECT after it has handed your character to a neighbour. Do an
ordinary handshake with the given address using the **same identity**; the new zone restores the character. Your entity id changes.
Guests are never redirected (they are stopped at the border). Browsers use the zone's web gateway instead of the UDP address
(`/play/info` lists each zone's web base URL under `zones`).

Zone lines. When `/play/info` says `"zone_lines": true`, every zone is a map of its own (`zone_names` gives each one's
name, by index), and a REDIRECT means you walked through an exit. The new zone's walls, world file, blocks and names are
its own: read its `/play/info` from `zones[zone]` and load its world before you connect, then handshake with the same
identity. You arrive where the exit says. (The C library stops with `MMO_MOVED` for this when `mmo_follow_redirects(c, 0)`
is set; `mmo_moved` gives the zone and address.) Signing in to a zone you have left also gets a REDIRECT at once, to the
zone that has your character now. The gateway allows any page to read its public `/play` paths, and accepts a WebSocket
from its own pages or another zone's (`--zone-web`), so a page loaded from one zone can carry on in the next.

## 6. Browsers

The web gateway relays one WebSocket binary message as one datagram, both ways, at `/ws`. The protocol above is unchanged.

## Items (version 6)

Item kinds come from the game file (`items`), by index in file order, and are not part of the content hash; the play page
fetches them from `/play/info`. There is no new client message: item actions ride the INPUT action byte. Game actions use ids
below 200. `200 + n` uses bag slot n (0 to 15), `216 + n` drops slot n onto the ground, and `250` picks up the nearest item
within 3 units (or, with `target` set to a pile's GROUND id, that pile if it is within 3 units). All of it is checked server side, with a short shared cooldown. Version 6 added INVENTORY and GROUND and
nothing else; a client that ignores them still plays.

## Jump

`251` in the INPUT action byte jumps (target ignored): the character is in the air for 0.6 s, a jump in mid-air is
ignored, and while airborne no action can target it, so attacks miss (counted as dodges, and the attacker's cooldown is
not spent). Nothing new goes on the wire. If the game file has an attribute named `airborne`, the server keeps it at the
share of the current jump still to come, from its max at take-off to its min on landing; make it replicated and clients
draw the arc from it as a parabola, `4h * p * (1 - p)` with `p = 1 - share`. The default game has one (0 to 100, 8 bits).

## Banners

Chat channel 3 is a banner: a message from the server (sender 0) for the middle of the screen, sent by events. It is
sent only by the server and only to clients; show it as a chat line too. A client that does not know channel 3 can
treat it as an ordinary line. (No version change: the chat batch format already carries any channel.)

## Names (version 9)

Every entity has a display name, served by the web gateway: `GET /play/names?ids=1,2,3` (at most 256 ids, else 400)
answers `{"<id>": "<name>", ...}` for the ids that are alive (an id from an older generation of its slot is left out).
A player goes by the name they claimed, or until they claim one, a name made from their account tag (the same every
time, not unique); a guest is `Guest`; a bot goes by its type, each word capitalised (`cave troll` is `Cave Troll`).
Ask again now and then for the players in view: a name can change.

To claim a name, send it as CHAT_SEND on channel 2. The server answers with one line from 0: `You are now known as
<name>.`, or why not. A name is 3 to 16 characters: letters (any script), digits, single spaces, `'` and `-`, starting
with a letter, ending with a letter or digit, never two of space, `'`, `-` together, and not a reserved word (admin, gm,
god, moderator, server, system, guest...). Names are unique ignoring case and marks (`Ana-Lee` and `ana lee` clash), stay
taken while their player is away, and are saved with the character. Guests cannot claim one. `proto::names` has the
rules; the C SDK sends a claim with `mmo_set_name`.

## Building (version 8)

Blocks sit on one grid for the whole world, in all three directions: a cell is a `BLOCK` (2 unit) square, `cx =
floor(x / 2)`, `cy = floor(y / 2)`, and layer `z` fills heights `2z` to `2z + 2`, the same everywhere, so blocks line
up however the ground runs. A cell's column starts in its *base* layer, the one the lowest corner of its ground is in
(`floor(lowest / 2)`; the server reads the ground from the world file's `heights`, flat at 0 without them), and spans
16 layers up from there. Any of those layers may hold a block or not, so blocks bridge gaps and leave windows. Every
block has a material (0..11) and an owner: the account that built it (guests' blocks belong to nobody).

`252` builds and `253` breaks, in the INPUT action byte. The target names the cell, `cx | cy << 15`, and INPUT's
`arg` the layer (an i8, in the low byte) and, for a build, the material (the high byte). A build needs the layer empty and inside the column, the block resting on
the column's base layer or against another block above, below or on one of its four sides, nobody's body in that
layer of the cell, and the cell within 8 units of the builder sideways and its layer within 8 units of their feet.
A break takes out that one block, anywhere in the column, and only its owner can (anyone, for a block nobody owns).
The server refuses anything else (`rejected_actions`).

Movement is in three dimensions. Each entity's feet are in a layer (`z` in its snapshot entry): -128 on the bare ground
away from any column, otherwise the layer it stands in. Its bounding square overlaps some columns; a move is allowed if
the layer it would stand in there is empty in all of them, and when it is not, the move steps up `CLIMB` (1) layer if
that one is empty. After any move, and every tick while standing on blocks, it drops to the highest layer with a
block under it in one of those columns (or their highest base). A body is one layer tall: you walk under a block one
layer above the ground. Coming off the bare ground into a column you start at its base. `proto::world::World`
(`set_column`, `enter`, `settle`, `step`) is the reference, mirrored in `ui/src/play/sim.ts`; a client that predicts
mirrors the columns it is told about (`mmo_set_column`) and carries its own layer through prediction. `/play/info`
carries `build` (block size, reach, layers per column as `max_stack`, materials, climb, the ground layer, the two
action ids) when the server offers building.

The blocks travel over HTTP, not UDP: `GET /play/blocks?since=VER` returns `{"ver", "full", "owners": [tags],
"cols": [[cx, cy, base, "layers", [owners]], ...]}`. `layers` has a character per layer from the base up to the
highest block, `.` for an empty one and a material's character (`0`-`9`, then `a`-`z`: `0`-`b` for 12 materials); `owners` has a number per layer, 0 for nobody,
otherwise an index into the response's `owners` plus one. An owner tag is 16 hex digits of the builder's identity
public key, so a client knows its own (`mmo_account_tag`). With `full: false` the list holds only the columns changed
after `VER` (an empty `layers` is a cleared column); with `full: true` (no `since`, a version from before a restart,
or one too far behind) it is every column and the client drops what it had. Poll it every second or so, and soon
after building. This whole-world feed is kept for older clients; a world with many blocks should be streamed by
region instead.

Blocks by region: `GET /play/blocks/r?c=rx,ry,ver_...` (at most 64 regions). A region is 128 x 128 block cells,
256 world units, the same squares as the wall tiles; `ver` is the version of that region the client holds (0 for
none). Only regions whose version differs come back, so polling the 5 x 5 regions round the player every second
costs a few bytes when nothing changed. The reply is binary, little endian, gzipped if the request takes it:

```text
u8 format (1) | u16 n | n x region
region: u16 rx | u16 ry | u32 ver | u16 n_owners | n_owners x 8 bytes (an owner's tag) | u16 n_cols | n_cols x col
col:    u16 cell (local y * 128 + local x) | i8 base | u16 mask | per set bit, lowest first: u8 material | u8 owner
```

`mask` bit k is a block in layer base + k; `owner` 0 is nobody, otherwise the region's owner table plus one. A
region's record is everything in it now, so the client replaces what it held there; a region with no columns (or
version 0) clears it. A version a server never gave (one from before a restart) simply differs, so the client gets
the region afresh. Let go of regions more than three away. `proto::blockfeed` is the reference, with a byte fixture
shared by `ui/src/play/blocks.test.ts`; Godot decodes the same in `main.gd` `_poll_block_regions`. A server
without it answers 404, and the client falls back to `/play/blocks?since=`.

Version 8 added `z` to snapshot entries (13 bytes plus attributes; with two 8 bit attributes `max_visible` can be at
most 72) and a two byte `arg` to INPUT.

## World size (version 7)

Positions are 32 bit (`world units * 16`), so the wire can describe a world of 268 million units on a side. Version 7 widened
`x` and `y` in snapshot entries and GROUND, and `world_size` in WELCOME, from 16 to 32 bits (version 6 topped out at 4,095
units). The server itself runs worlds from 64 to 65,536 units (`--world-size`), a limit chosen so that 32 bit floats keep a finer
resolution than the wire does. A snapshot entry is now 12 bytes plus attributes, so fewer entries fit in one datagram: the
server refuses a game file whose `max_visible` could overflow it.

## Walls over HTTP

`/play/info` carries `weather_zones`: each weather zone that is on, `{name, x, y, r, height, weather, color, intensity}`. Inside
one (the smallest, where they overlap) a client shows that weather instead of `weather`; from outside it draws mist in
its colour standing `height` tall over the circle. (`zones` is something else: the web URLs of a multi-process world.)

`/play/info` carries `locations`: each place with a `title`, `{title, x, y, r}`. A client tells the player the title of
the smallest one round them as they walk in, and "Leaving" it as they walk out; it counts them as still inside until
3 units past the edge so the title does not flicker. Titles are live (a console edit is in the next `/play/info`).

`/play/info` carries `daylight`: the schedule (`mode` `cycle`, `clock` or `fixed` and its fields, as in the game file),
`night_share`, and `sample: [unix_ms, phase]`, one moment the server worked out. Clients compute the phase themselves
(`proto::daylight::Daylight::phase`, ported to `godot/scripts/theme.gd` and `ui/src/daylight.ts`): 0 is sunrise,
`1 - night_share` sunset. Block materials 12 (`c`) and 13 (`d`) are a lamp and a fire.

`/play/info` carries `walls: {ver, tile, count, inline}`. When `inline` is true (600 walls or fewer, and no roads) the boxes are in `boxes` and what each is in `box_class` (below). When false `boxes` is empty and walls come by tile: `GET /play/tiles?v=<ver>&c=cx,cy_cx,cy` (`_` between tiles: a TLS proxy may refuse a query with `;`, which is still read; up to 64 tiles of `tile` = 256 units, counted from (0, 0)). The reply is binary, little-endian, and gzipped for a client that sends `Accept-Encoding: gzip`:

```
u8 format (2) | u64 ver | u16 tiles | per tile: u16 cx | u16 cy | u32 n | n x (u32 id | f32 x0 | f32 y0 | f32 x1 | f32 y1 | u8 class) | u16 m | m x u16 road
```

Each tile ends with its road cells: the tile is 64 x 64 cells of 4 units, and each road cell is `row * 64 + column`. Movement is `road_speed` times faster (from `/play/info`, 1.5 unless the game file says otherwise) for a tick that starts on a road cell: `proto::world::integrate` checks `World::on_road(x, y)`, the cell `(floor(x / 4), floor(y / 4))`, so prediction must hold the same cells. A box touching several tiles appears in each with the same id, and its corners are the server's f32s bit for bit (collide with exactly those). Class 0 is a wall or building; class 1 is solid scenery the world file draws (a trunk, a rock), which a client should not draw as a block. A reply names its version, so it is cached for good (`Cache-Control: immutable`); asked for another version the server answers 409, which means the walls changed: read `/play/info` again and start over. More than 64 tiles is a 400. Keep the 5x5 tiles around the player and let go of tiles more than three away. No tile holds more than 4,096 boxes. `proto::tiles` implements both ends (`TileSet`, `TileStream`) and the C SDK exposes it as `mmo_tiles_wanted` / `mmo_put_tiles` / `mmo_set_walls`.

Chat channels 4 (a whisper to you, `from` the speaker), 5 (your own whisper echoed, `from` whom it went to) 6 (party) and 8 (guild) are server to client only. Input actions 232 talk, 233 accept a quest (`arg`), 234 give one up (`arg` 0xFFFF turns down the one offered), 235 buy (`target` the merchant, `arg` the shop line), 236 sell (`arg` the bag slot), 237 take off gear (`arg` the slot), 238 choose a class (`arg` its index), 239 rise again after falling.

The bag, the bank, trading and the auction house (`proto::game` `bag_op`, `trade_op`, `auction_op`; the server's `market.rs`) are input actions with the operation in the high byte of `arg`: 240 the bag and bank (a slot in `arg`'s low byte, a second slot in `target`), 241 trading (`target` the player asked, a bag slot, or an amount), 242 the auction house (a page or bag slot in `arg`'s low byte, an item tag, listing id or price in `target`). The answers come as chat: channel 9 is a trade's state from the other player ("?" they ask, "-" it is over, else `mine|coin|ready|theirs|coin|ready` with stacks as `tag:count,...`), channel 10 auction listings from 0 (`H page pages total mine`, then `R id tag count price secs mine seller` a line each, then `E`). Both are server to client only, and the wire is otherwise unchanged.

Chat channel 7 is a bot's passing remark, heard within 24 units (sender the bot): show it over the speaker's head rather than in the chat log; a client that does not know it can show it as a line said nearby. A player whose health runs out is not respawned: they stay where they fell with health held at its minimum, cannot move or act, and are left out of everyone else's snapshots, until they send action 239; then they come back whole in the nearest place marked `safe` (anywhere free, if there is none). A client tells it is down from its own health being at the minimum. Bots that have gone indoors (`indoors` rule) are also left out of snapshots. (No version change: none of this alters a packet.)

## Encounters, quests and the crowd (versions 10 and 11)

Version 10 widened QUESTS to u16 quest indices with a bitmap of the quests done, and added ENCOUNTER (`0x8F`).
Version 11 is the same wire with CROWD at `0x90` and guild chat on channel 8; a version 10 client would take a
CROWD packet for an ENCOUNTER, so the version moved.

## The realm (no packet change but CROWD)

Factions, classes, loot tables, crafting, farming, land, guilds and dungeons (`proto::realm`) need nothing new on the
wire but the CROWD message and chat channel 8 (guild): players reach them with "/" chat commands (`/faction`, `/class`,
`/claim`, `/unclaim`, `/plot`, `/guild ...`, `/g`, `/craft`, `/recipes`) and the server answers on channel 0 from 0.
`/play/info` lists them under `realm` (factions with colours and standing rules, recipes, crops, plots, dungeons; classes, with their spells, stay under `classes`)
and items carry `rarity` when it is not common. Block materials 14 (soil), 15 (a crop growing) and 16 (a ripe one) go
in block text as `e`, `f` and `g`; `build.kinds` is 15, the materials a player can build (crops come from seeds), and
`build.soil`, `build.sprout`, `build.ripe` name the farming ones. A game with replicated attributes named `faction`
and `class` gets each player's (index + 1, 0 for none) in every snapshot, kept by the server. Private progress and
quests are not sent until a session is proven (its save is in).

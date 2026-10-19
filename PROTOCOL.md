# Wire protocol, version 8

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
| `0x88` INVENTORY | S->C | `n u8 | n x (item+1 u8, count u8)`: your bag, sent when it changes and every second (item+1 of 0 is an empty slot) |
| `0x89` GROUND | S->C | `n u8 | n x (id u32, x u32, y u32, item u8, count u8)`: up to 40 items lying nearest you, every half second; positions quantised like entities |

Send INPUT at the server's tick rate (20 Hz). `dir_x`/`dir_y` are -127..127 intent, never speed. `seq` increases;
the server applies the newest. `ack_tick` is the newest snapshot tick you hold. `action` is an action id (0 = none) with a
`target` entity id; send it once. Chat is reliable on top of UDP: resend CHAT_SEND until `ack_cseq` in a batch covers it;
`chat_ack` in INPUT tells the server the highest batch `seq` you have.

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

## 6. Browsers

The web gateway relays one WebSocket binary message as one datagram, both ways, at `/ws`. The protocol above is unchanged.

## Items (version 6)

Item kinds come from the game file (`items`), by index in file order, and are not part of the content hash; the play page
fetches them from `/play/info`. There is no new client message: item actions ride the INPUT action byte. Game actions use ids
below 200. `200 + n` uses bag slot n (0 to 7), `216 + n` drops slot n onto the ground, and `250` picks up the nearest item
within 3 units. All of it is checked server side, with a short shared cooldown. Version 6 added INVENTORY and GROUND and
nothing else; a client that ignores them still plays.

## Jump

`251` in the INPUT action byte jumps (target ignored): the character is in the air for 0.6 s, a jump in mid-air is
ignored, and while airborne no action can target it, so attacks miss (counted as dodges, and the attacker's cooldown is
not spent). Nothing new goes on the wire. If the game file has an attribute named `airborne`, the server keeps it at the
share of the current jump still to come, from its max at take-off to its min on landing; make it replicated and clients
draw the arc from it as a parabola, `4h * p * (1 - p)` with `p = 1 - share`. The default game has one (0 to 100, 8 bits).

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
after building.

Version 8 added `z` to snapshot entries (13 bytes plus attributes; with two 8 bit attributes `max_visible` can be at
most 72) and a two byte `arg` to INPUT.

## World size (version 7)

Positions are 32 bit (`world units * 16`), so the wire can describe a world of 268 million units on a side. Version 7 widened
`x` and `y` in snapshot entries and GROUND, and `world_size` in WELCOME, from 16 to 32 bits (version 6 topped out at 4,095
units). The server itself runs worlds from 64 to 65,536 units (`--world-size`), a limit chosen so that 32 bit floats keep a finer
resolution than the wire does. A snapshot entry is now 12 bytes plus attributes, so fewer entries fit in one datagram: the
server refuses a game file whose `max_visible` could overflow it.

## Walls over HTTP

`/play/info` carries `walls: {ver, chunk, count, inline}`. When `inline` is true the boxes are in `boxes`. When false (more than 600 walls) `boxes` is empty and walls come from `GET /play/walls?c=cx,cy;...` (up to 64 chunks, chunk edge `chunk` units), answered with `{ver, chunk, chunks: {"cx,cy": [[id,x0,y0,x1,y1],...]}}`; a box touching several chunks appears in each with the same id. `?all=1` returns `{chunks: {all: [...]}}`. `ver` changes whenever any wall changes; a client holding another version discards what it has.

# Wire protocol, version 7

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
| `0x02` INPUT | C->S | `seq u32 | ack_tick u32 | dir_x i8 | dir_y i8 | action u8 | target u32 | chat_ack u32` (20 bytes) |
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
entry = id u32 | x u32 | y u32 | one value per replicated attribute (u8 if bits <= 8 else u16)
```

- Entries are sorted by id. `baseline_tick = 0` means a full snapshot. Otherwise it is a delta against the snapshot of that
  tick, which you must still hold (keep the last 16 by `tick % 16`); if you do not, drop the packet. The server only uses a
  baseline you acknowledged with `ack_tick`.
- State = baseline + changed (replaces by id) - removed. Ignore a snapshot whose tick is not newer than the last applied.
- `x`, `y` are `value * 16` rounded, so world units = `q / 16`. Attributes: `min + q / (2^bits - 1) * (max - min)`.
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

## Building

`252` builds and `253` breaks, in the INPUT action byte; nothing new goes on the wire, so the protocol version is
unchanged. The target names a block column: `cx | cy << 15 | material << 30`, where a column is a `BLOCK` (2 unit)
square cell, `cx = floor(x / 2)`, `cy = floor(y / 2)`, and the material is 0..3. A build puts a block of that material
on top of the column (at most 8); a break takes the top one off. The server refuses a column more than 8 units from
the builder's position, one inside a wall or outside the world, and a new column where anyone stands. A column with
any block in it blocks movement exactly as a wall does (`proto::world::World::set_block`), so a client that predicts
must mirror the columns it is told about. `/play/info` carries `build` (block size, reach, stack limit, materials,
the two action ids) when the server offers building.

The blocks travel over HTTP, not UDP: `GET /play/blocks?since=VER` returns `{"ver", "full", "cols": [[cx, cy,
"materials"], ...]}`, with materials as digits from the bottom up. With `full: false` the list holds only the columns
changed after `VER` (an empty string is a cleared column); with `full: true` (no `since`, a version from before a
restart, or one too far behind) it is every column and the client drops what it had. Poll it every second or so, and
soon after building.

## World size (version 7)

Positions are 32 bit (`world units * 16`), so the wire can describe a world of 268 million units on a side. Version 7 widened
`x` and `y` in snapshot entries and GROUND, and `world_size` in WELCOME, from 16 to 32 bits (version 6 topped out at 4,095
units). The server itself runs worlds from 64 to 65,536 units (`--world-size`), a limit chosen so that 32 bit floats keep a finer
resolution than the wire does. A snapshot entry is now 12 bytes plus attributes, so fewer entries fit in one datagram: the
server refuses a game file whose `max_visible` could overflow it.

## Walls over HTTP

`/play/info` carries `walls: {ver, chunk, count, inline}`. When `inline` is true the boxes are in `boxes`. When false (more than 600 walls) `boxes` is empty and walls come from `GET /play/walls?c=cx,cy;...` (up to 64 chunks, chunk edge `chunk` units), answered with `{ver, chunk, chunks: {"cx,cy": [[id,x0,y0,x1,y1],...]}}`; a box touching several chunks appears in each with the same id. `?all=1` returns `{chunks: {all: [...]}}`. `ver` changes whenever any wall changes; a client holding another version discards what it has.

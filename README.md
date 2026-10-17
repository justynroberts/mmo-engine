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
| `ui/` | The console: React, TypeScript, Tailwind, Radix, Framer Motion. Served by the server |
| `scripts/` | `bench.sh` (pinned load test), `ui_e2e.py` (real-browser UI tests) |

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

## The game file

One JSON document, validated before it is applied. Attributes, actions and
effects become the `Defs` that clients share and that the handshake hashes.
Bot types ("archetypes") are server-side only.

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
cargo test --release                 # 57 proto + 24 server unit tests, 23 end-to-end tests (real server, real UDP and HTTP)
(cd ui && npm test)                  # 40 unit and accessibility tests
scripts/ui_e2e.py [--shots DIR]      # 54 real-browser checks incl. axe scans of every tab in both themes, and building a place, a route and rules with the mouse
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

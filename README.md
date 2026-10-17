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
| `crates/bots` | N real encrypted UDP clients: handshake, random walk, zaps, delta decode, ack, latency measurement, reconnect |
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
interface only binds to loopback). The Dockerfile is written but I could not run
a Docker daemon where this was built, so it is untested.

### Server flags

| Flag | Meaning |
| --- | --- |
| `--workers N` | Snapshot lanes. 1 = everything on the sim thread, N > 1 builds, encrypts and sends snapshots on N-1 more cores (pinned to `--pin`+1, +2, ...) |
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
- **Bot types**: colour, spawn count, behaviour (idle, wander, chase, flee), speed multiplier, sight, attack action, starting attribute overrides.
- Editing only bot types applies live and leaves players connected. Editing attributes, actions or settings changes the wire format, so the world restarts and clients are dropped; they get a content-mismatch rejection until they load the same file.

## Security model

- **Stateless handshake.** HELLO (padded to 96 bytes) is answered with a 53 byte CHALLENGE carrying a cookie: a keyed hash of the client's address, public key and a timestamp. The server stores nothing and does no key agreement until a CONNECT returns a valid, fresh cookie from the claimed address. Spoofed sources never see the challenge, and replies are smaller than requests, so there is no amplification.
- **Encryption.** X25519 key agreement, BLAKE3 key derivation, ChaCha20-Poly1305 per packet, one key per direction, 32-bit counter nonce, 64-packet replay window. The server's long-term key authenticates it (clients pin it); an ephemeral server key per session gives forward secrecy. Header bytes are authenticated. Cost: 25 bytes per packet.
- **Admin interface.** Loopback only, no login. Checks the Host header (DNS rebinding) and requires `X-Admin: 1` on writes (cross-site requests). It refuses to start on a non-loopback address.
- Not done: rekeying (a session ends before its 32-bit counter wraps, years at 20 packets/s), rate limiting per source, authentication of *players* (anyone with the public key can connect), authenticated REJECT packets (a forged reject can only make a handshake fail).

## The second core

Building each player's personalised snapshot is the largest cost per tick in a crowd, and adding encryption made it bigger. A tick builds one snapshot per player, and nothing about player A's snapshot depends on player B's, so the clients are split across lanes and each lane builds, encrypts and sends its share at the same time. The sim is read-only during that phase, so there are no locks: each lane owns a disjoint slice of clients, its own scratch buffers and its own send queue. Output is byte-for-byte what one lane would produce.

Measured in-process (no network, no load generator competing, 2,000 clients, every snapshot full, one batch send per lane):

| World | 1 lane | 2 lanes |
| --- | --- | --- |
| 2048, sparse | 8.9 ms | 4.9 ms |
| 512, dense (each sees 64) | 19.4 ms | 10.8 ms |

Receiving and decrypting inputs still runs on the sim thread (about 5 ms at 2,000 clients); moving that onto lanes is the next obvious step.

## Measured results

Environment: a 2 vCPU VM, over loopback. Server pinned to core 0, bots on core 1, all traffic encrypted. Loopback charges the receiver's kernel work to the sender, so server numbers are pessimistic, and the load generator shares the machine.

| Scenario | Clients | Lanes | Tick p50 | Tick p99 | Over 50 ms |
| --- | --- | --- | --- | --- | --- |
| Normal density (2048 world) | 2,000 | 1 | 17.1 ms | 32.7 ms | 0 of 400 |
| Dense crowd (512 world, all see 64) | 2,000 | 1 | 30.7 ms | 44.1 ms | 0 of 400 |
| Headroom (2048 world) | 4,000 | 1 | 40.3 ms | 60.9 ms | 80 of 400 |
| Headroom (2048 world), bots share core 1 | 4,000 | 2 | 36.8 ms | 60.1 ms | 14 of 400 |

All runs: zero decode errors, zero missing baselines, zero failed decryptions.
For comparison, before encryption the same machine did 2,000 normal-density clients at p50 11.2 ms and 4,000 at 6 of 600 ticks over budget, so encryption costs about 50 percent more CPU per client here. One core now holds roughly 2,000 to 3,000 clients with the gate (2,000 bots at 20 Hz on one core) still passing. The two-lane live run is muddied because the bots occupy the second core on this machine; the in-process table above is the clean measurement.

## Tests

```sh
cargo test --release                 # 44 unit tests + 15 end-to-end tests (real server, real UDP and HTTP)
(cd ui && npm test)                  # 18 unit and accessibility tests
scripts/ui_e2e.py [--shots DIR]      # 37 real-browser checks incl. axe scans of every tab, both themes
cargo test -p server --release lane_scaling -- --ignored --nocapture
```

The end-to-end tests include forged, bit-flipped and replayed packets, wrong-address CONNECTs, HELLO floods, wrong content hashes, a full server, idle timeouts, cross-site and DNS-rebinding requests to the admin API, path traversal, hot reload and the bot behaviours.

## Known limits and next steps

- Move input receive and decrypt onto the lanes; share candidate lists per grid cell
- Fields are byte aligned. The schema carries bit widths, so bit packing is a protocol-only change
- No player accounts or login, no rate limiting, no rekeying (see Security model)
- The console possesses bots through the admin API, not through the UDP protocol, so it bypasses the encrypted path; bots-as-clients exercise that
- The game file is JSON on disk. The DB and baker from the plan would produce the same `GameFile`
- World is generated boxes. A real baker would emit heightfields, navmesh and chunks
- Single zone. Zones and cells come next
- Bots do not predict. Add client-side prediction using `proto::world::integrate`
- Linux and IPv4 only (`recvmmsg`, `sendmmsg`, `sched_setaffinity`)
- Line of sight is not checked for actions
- Moodboard phase of the UI design was skipped; the look is an instrument-panel direction chosen from the brief

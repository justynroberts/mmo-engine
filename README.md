# mmo-engine (phase 1 and 2 spike)

A genre-neutral authoritative world server in Rust, plus a bot load tester that
speaks the real protocol. Zero third-party dependencies apart from `libc`.

Health is not special here. It is attribute 0 in a data table, and "combat" is
one set of data-driven effects on an action. The same engine can host an MMO,
a cyberpunk city or a user-built world.

## Layout

| Crate | What it is |
| --- | --- |
| `crates/proto` | Wire format, attribute and action definitions, delta snapshots, static world and the movement step. Compiled into server and clients so they cannot drift |
| `crates/server` | The world server: fixed tick, SoA entities, spatial grid, interest management, `recvmmsg`/`sendmmsg` UDP |
| `crates/bots` | N real UDP clients: handshake, random walk, zaps, delta decode, ack, latency measurement |

## Run it

```sh
cargo build --release
cargo test --release            # 24 tests

target/release/server --pin 0 --world-size 2048 --npcs 200
target/release/bots --bots 2000 --duration 30

scripts/bench.sh 2000 2048 30   # pinned server + bots, prints both summaries
```

## What is implemented

- Fixed 20 Hz tick, deadline pacing (sleep, then spin the last 250 us)
- Entities as structure-of-arrays, 24-bit slot plus 8-bit generation ids
- Attributes declared in `Defs` (range, bit width, replicated, regen, on-zero rule), stored by attribute id
- Actions as data: cooldown, range, requirement, effects. Validated server side
- Movement from direction intent only. Speed, collision and bounds belong to the server, via one function shared with clients
- Uniform spatial grid rebuilt per tick with a counting sort, 3x3 neighbour queries
- Interest management: radius filter, nearest 64 only, sent per client
- Delta snapshots against the last client-acked snapshot, with full fallback when the ack is stale
- Handshake carrying a content hash, so mismatched definitions are rejected
- Idle client reaping, NPCs as ordinary entities with a low-rate AI writing the same direction fields as player input

## Measured results

Environment: a 2 vCPU VM. Server pinned to core 0, bots on core 1, over loopback.
Loopback charges the receiver's kernel work to the sender, so server numbers are
pessimistic. The load generator shares the machine.

| Scenario | Clients | Tick p50 | Tick p99 | Over 50 ms | Out |
| --- | --- | --- | --- | --- | --- |
| Normal density (2048 world) | 2,000 | 11.2 ms | 21.3 ms | 0 | 50 Mbit/s |
| Dense crowd (512 world, all see 64) | 2,000 | 22.0 ms | 36.1 ms | 0 | 190 Mbit/s |
| Headroom (2048 world) | 4,000 | 24.0 ms | 49.3 ms | 6 of 600 | 75 Mbit/s |

All runs: zero decode errors, zero missing baselines, zero lost own-entity.
The phase 1 gate (2,000 bots hold 20 Hz on one core) passes with room to spare.
On this machine one core tops out around 3,000 to 4,000 clients.

## Known limits and next steps

- Snapshot building is the biggest cost in a crowd (17 ms of 22 ms in the dense run). Next: build snapshots on spare cores, share candidate lists per cell
- Fields are byte aligned. The schema already carries bit widths, so bit packing is a protocol-only change
- No encryption, no auth, no rate limiting, no stateless handshake cookie. Required before anything is public
- Definitions are built in code. The DB and baker from the plan would produce the same `Defs`
- World is generated boxes. A real baker would emit heightfields, navmesh and chunks
- Single zone, single sim thread. Zones and cells come next (see the plan's zoning model)
- Bots do not predict. Add client-side prediction using `proto::world::integrate`
- Linux and IPv4 only (`recvmmsg`, `sendmmsg`, `sched_setaffinity`)
- Line of sight is not checked for actions

# Engine SDK

`mmo-client` is the whole client protocol behind a C interface, so any engine that can call C can play:
handshake, encryption, snapshots, chat, input timing, reconnects and zone handoffs. See `../PROTOCOL.md` for the wire
format and `c/mmo_client.h` for the API.

```
cargo build --release -p mmo-client       # target/release/libmmo_client.so (.dylib, .dll) and libmmo_client.a
cc sdk/c/example.c -Isdk/c -Ltarget/release -lmmo_client -Wl,-rpath,target/release -o example
```

The loop in any engine is the same: `mmo_connect` once, `mmo_poll` every frame, `mmo_set_move` from input,
read entities with `mmo_entity_count` / `mmo_entity`, free with `mmo_free`. Fetch `/play/info` over HTTP from the
server's web gateway (`--web`) and hand the body to `mmo_connect`; it carries the pinned server key and schema.
Store the player's identity (`mmo_new_identity`, or `mmo_identity_from_passphrase`) so their character comes back.

Tested here: the C example against a live server, and the library through the same C functions in the Rust end-to-end
suite (play, move, chat, a wrong pinned key refused, zone handoff followed automatically).

## Godot 4

`crates/godot-ext` is a GDExtension (Rust, `godot` crate) that adds an `MmoClient` node. It compiles
(`cargo build --release -p mmo-godot` gives `libmmo_godot.so`), but it has **not been loaded in Godot** here, because
there is no Godot in this environment. Expect to fix small API mismatches on first load.

1. Copy `target/release/libmmo_godot.so` to `res://bin/` and `sdk/godot/mmo.gdextension` next to it.
2. Attach `sdk/godot/Game.gd` to a Node2D. It fetches `/play/info`, creates and stores an identity, connects, sends
   WASD/arrow intent and draws every entity as a square.

Node API: `connect_to(host, port, info_json, identity) -> String` (empty on success), `set_move(Vector2)`,
`act(action, target)`, `say(text, shout)`, `entities() -> Array[Dictionary]` (`id`, `position`, one key per replicated
attribute), `get_self_id()`, `get_status()`, `get_status_text()`, `get_rtt_ms()`, signals `status_changed` and
`chat_heard`, and static `new_identity()` / `identity_from_passphrase(account, passphrase)`.

## Unreal 5

Also untested here. Put `mmo_client.h` and the library in a plugin's `ThirdParty` folder, add them in the module's
`Build.cs` (`PublicAdditionalLibraries`, `PublicIncludePaths`), and wrap the calls in a `UGameInstanceSubsystem` that
ticks `mmo_poll` from a `FTickableGameObject`. Entities map to replicated-looking actors you spawn and move yourself
(this server is authoritative, so do not use Unreal replication). Fetch `/play/info` with `FHttpModule`.

## Interpolation and prediction

Built in. Other entities are drawn two ticks in the past, blended between the snapshots either side, so they move
smoothly at your frame rate (teleports such as respawns are not slid across the map). Your own entity is predicted with the
same movement code the server runs (walls come from `/play/info` `boxes`) and reconciled on each snapshot, so it answers
input immediately. `mmo_entity` already returns these smoothed positions; just draw them.

IPv4 only.

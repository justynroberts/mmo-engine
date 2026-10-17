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

Not built or run here (no Godot in this environment), so treat this as the intended shape rather than a tested plugin.
Godot loads native code through GDExtension. Write a small extension (godot-cpp or the `godot` Rust crate) that links
`libmmo_client` and exposes a node, say `MmoClient`, with `connect_to(host, port, info_json, identity)`, a `_process`
that calls `mmo_poll`, `set_move(Vector2)`, `say(text)`, and a `entities()` method returning an `Array` of
`Dictionary` (`id`, `position`, attribute names as keys). Map world units to your scene with one scale factor; the world is
a flat plane, x east and y south. Use `HTTPRequest` for `/play/info`. Ship the library next to the `.gdextension` file.

## Unreal 5

Also untested here. Put `mmo_client.h` and the library in a plugin's `ThirdParty` folder, add them in the module's
`Build.cs` (`PublicAdditionalLibraries`, `PublicIncludePaths`), and wrap the calls in a `UGameInstanceSubsystem` that
ticks `mmo_poll` from a `FTickableGameObject`. Entities map to replicated-looking actors you spawn and move yourself
(this server is authoritative, so do not use Unreal replication). Fetch `/play/info` with `FHttpModule`.

## What the library does not do

No prediction or interpolation: snapshots arrive about 20 times a second and the engine should interpolate between
them. `crates/proto/src/predict.rs` is the reference predictor if you want it. IPv4 only.

# World files, format 1

A world file is the world as data: the shape of the land, the water, and every object in it. Three parts of
the system touch it, and none of them depends on the others:

| Part | What it does with the file |
|---|---|
| A generator (`crates/worldgen`, or anything else that writes this format) | Writes it. A different world is another run, with another seed or another generator |
| The engine (`server --world FILE`) | Reads the size and the footprints of solid objects, which become its walls. Serves the file to clients byte for byte. Knows nothing else about it |
| A client theme (`godot/themes/<name>/`) | Draws it: decides what each object type, the terrain and the water look like. A different look is another theme |

```sh
cargo run --release -p worldgen -- --size 2048 --seed 7 --out data/world.json
server --world data/world.json            # or, with the server running: POST /api/world/reload
```

`/play/info` carries `"world": {"ver": "<hex>", "bytes": N}` when there is a world file, and clients fetch it
from `GET /play/world` (the admin side has the same at `GET /api/worldfile`). `ver` changes whenever the file
does. `POST /api/world/reload` rereads the file and applies it live, as a wall edit is: nobody is
disconnected, anything left inside a new wall is moved. Changing the size needs a restart. While a world
file is in use, the game file's `walls` are not.

## Layout

One JSON object.

| Field | Meaning |
|---|---|
| `format` | `1` |
| `size` | Edge of the square world, 64 to 65536. The playable area is `[0, size]` on both axes |
| `seed` | Informational |
| `generator` | Informational |
| `heights` | The ground, below |
| `water` | Rivers and lakes, below |
| `objects` | Everything else, below |

Unknown fields are allowed everywhere and ignored by the engine, so a generator can add data for its own
theme.

### heights

A grid of ground heights, which may extend past the playable area (for mountains on the horizon).

| Field | Meaning |
|---|---|
| `x0`, `y0` | World position of sample (0, 0) |
| `cell` | Spacing between samples, in world units |
| `w`, `h` | Samples across and down |
| `lo`, `hi` | Height range |
| `u16le` | Base64 of `w * h` little-endian u16, row by row (`y` outer): height = `lo + (hi - lo) * v / 65535` |

Between samples, interpolate bilinearly. Rivers, lake beds, mountains and the levelling round buildings are
already in the heights; a theme never has to work them out.

### water

| Field | Meaning |
|---|---|
| `rivers` | `[{ "half_width": 5, "pts": [[x, y, water_level], ...] }]`, a polyline with the water surface height at each point |
| `lakes` | `[{ "b": [x0, y0, x1, y1], "level": h }]`, a surface at `level` over the box; the shoreline is where the heights rise above it |

Rivers are wadeable (the engine knows nothing of them). A lake is usually also a solid object of type `lake`,
so it cannot be entered.

### objects

`[{ "t": type, ... }]`. Two shapes:

| Shape | Fields |
|---|---|
| Footprint | `b: [x0, y0, x1, y1]`, `y`: ground height it stands on, optional `solid: true` |
| Point | `p: [x, y]`, `s`: scale, `r`: rotation in radians |

Every object has `v`, a random u32, so every client picks the same variant (which tree model, which wall
pieces). Only `solid` footprints matter to the engine; `t` is for themes. The types worldgen writes:

| Type | Shape | Meaning |
|---|---|---|
| `house` | footprint, solid | A dwelling (terraced rows are separate houses that share walls) |
| `keep` | footprint, solid | A fortified building |
| `tower` | footprint, solid | A tower (may straddle a wall) |
| `castle` | footprint, solid | The great keep of a city |
| `wall` | footprint, solid | A curtain wall run (runs meet at corners) |
| `crag` | footprint, solid | A rocky mountain; its height is in `heights` |
| `lake` | footprint, solid | Deep water |
| `city` | footprint | A city's extent (open ground inside is street) |
| `square` | footprint | An open square: a plaza, market or village green |
| `tree`, `tree_bare`, `bush`, `rock` | point | Scenery |
| `boulder`, `spire` | point | Big rocks on the mountains past the edge |

A theme should draw an unknown type as something neutral (a plain block for a solid footprint), so new
generators work with old themes.

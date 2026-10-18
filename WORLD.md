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

Or use the console's **World tab**: it edits a recipe (below), shows each change as a preview on the map, and
applies it live. The server runs the generator for it (`POST /api/world/generate` with a recipe returns the
preview; `POST /api/world/apply` makes the preview the world file and the world). The world file is `--world`,
or `world.json` in the server's working directory, which the server loads on start when it exists.

`/play/info` carries `"world": {"ver": "<hex>", "bytes": N}` when there is a world file, and clients fetch it
from `GET /play/world` (the admin side has the same at `GET /api/worldfile`). `ver` changes whenever the file
does. `POST /api/world/reload` rereads the file and applies it live, as a wall edit is: nobody is
disconnected, anything left inside a new wall is moved. Changing the size needs a restart. While a world
file is in use, the game file's `walls` are not.

## Recipes

worldgen's input, and written into each file it makes as `recipe`, so a world can be opened, changed and made
again (`worldgen --recipe world.json` accepts a whole world file). Every field is optional.

| Field | Meaning |
|---|---|
| `size`, `seed` | As the flags |
| `city` | A walled city (worlds of 1024 and up) |
| `villages`, `forts`, `crags`, `lakes` | How many, as a multiple of the usual number for the size (0 = none, 0..5) |
| `rivers` | River count (0..12), or null for the usual number |
| `forest`, `hills` | How wooded, and how tall the hills, as a multiple of usual (0..5) |
| `edits` | Hand edits, applied in order: `{"remove": [x, y]}` takes away the smallest footprint containing the point (a whole city with everything in it, on its open ground); `{"add": {"t": type, "b": [x0, y0, x1, y1]}}` puts a footprint there and clears what it covers. Types: house, keep, tower, castle, wall, crag, lake, square, city |

Edits go in before the land is shaped, so what they add stands on level ground with rivers bending round it.
Each river and each patch of scenery has its own random stream, so an edit changes only what is near it.

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

# World files, format 1

A world file is the world as data: the shape of the land, the water, and every object in it. Three parts of
the system touch it, and none of them depends on the others:

| Part | What it does with the file |
|---|---|
| A generator (`crates/worldgen`, or anything else that writes this format) | Writes it. A different world is another run, with another seed or another generator |
| The engine (`server --world FILE`) | Reads the size and the footprints of solid objects, which become its walls, and the `heights`, which say which layer each built block column starts in. Serves the file to clients byte for byte. Knows nothing else about it |
| A client theme (`godot/themes/<name>/`) | Draws it: decides what each object type, the terrain and the water look like. A different look is another theme |

```sh
cargo run --release -p worldgen -- --size 2048 --seed 7 --out data/world.json
server --world data/world.json            # or, with the server running: POST /api/world/reload
```

Or use the console's **World tab**: it edits a recipe (below), shows each change as a preview on the map, and
applies it live. The server runs the generator for it (`POST /api/world/generate` with a recipe returns the
preview; `POST /api/world/apply` makes the preview the world file and the world). The world file is `--world`,
or `world.json` in the server's working directory, which the server loads on start when it exists.

`/play/info` carries `"world": {"ver": "<hex>", "bytes": N, "parts": true, "head_bytes": N, "points": N, "tile": 256}`
when there is a world file. A client can fetch it whole from `GET /play/world` (the admin side has the same at
`GET /api/worldfile`), but a big world is better taken in parts, which is what the Godot client does:

- `GET /play/world/head`: the file with only its footprint objects (everything without a point `p`: buildings,
  walls, cities, crags, lakes), its heights and its water. Enough for a map of the whole world.
- `GET /play/world/tiles?v=<ver>&c=cx,cy;cx,cy` (up to 64): the point objects (trees, rocks, bushes, spires) whose
  `p` is in each 256 unit tile, as `{"ver", "tile", "tiles": {"cx,cy": [objects...]}}`, each object exactly as the
  file has it. Tiles past the edge are negative or beyond the last (scenery runs a little outside the world). Keep the
  5x5 around the player. A reply names its version, so it is cached for good; another version is a 409.

Both are slices of the file's own bytes, gzipped for a client that asks. `ver` changes whenever the file does.
A 65,536 unit world (6.2 million objects, 650 MB) costs a Godot client its head (about 145,000 footprints) and a
couple of thousand trees around it: it joins in 4.5 s at about 820 MB. `POST /api/world/reload` rereads the file and applies it live, as a wall edit is: nobody is
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
| `roads` | `true` (the default) or `false`: the city's streets, each village's high street, and a road network joining them and the forts |
| `coast` | Outlines of the land, `[[[x, y], ...], ...]` (three points or more each): land inside, open sea outside, a beach and wadeable shallows between. Without one the land runs to the edge and mountains rise past it |
| `river_courses` | Rivers along these lines, source to mouth, `[[[x, y], ...], ...]`, each followed with a meander of its own; replaces the rivers the seed lays out. Nothing is laid out across them |
| `settlements` | Places put where the recipe says before the seed lays out the rest: `[{"t": "city" \| "town" \| "village" \| "fort", "at": [x, y], "name": "...", "side": 240}]` (`side`: a city's width). A town is a long high street with a manor; each is moved a little (and made smaller) if its spot is taken |
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
| `roads` | Roads, below (optional) |
| `settled` | Where each of the recipe's `settlements` went: `[{"t", "name", "asked": [x, y], "at": [x, y] or null}]` |
| `farms` | `true` when the objects include `field`s: a theme draws those, and makes up no fields of its own |

Past 16,384 units worldgen also varies the land by region (a slow noise from flat lowland to rugged upland), raises
mountains where crags stand thick (a named range laid as a cluster of crags becomes a ridged mountain mass, a lone crag
a knoll), thins the woods above 40 and stops them above 52, and keeps its heights twice as fine. Lakes laid as strips
that overlap at their ends (at most 30 units, each at least 55 across) are one water: one level, the deepest bed
where they overlap, and water boxes cut back to the middle of each overlap. A theme scales its snow and heath lines by
the highest ground over 40 (`world_relief` in the fantasy theme).
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
| `sea` | `{ "level": h }` for land with a coast: open water at `level` everywhere the heights are below it |

Rivers are wadeable (the engine knows nothing of them). A lake is usually also a solid object of type `lake`,
so it cannot be entered.

### roads

`[{ "half_width": 3, "pts": [[x, y], ...] }]`: each a centre line and how far either side of it the road runs (0.5 to
32). The engine makes them faster underfoot: it turns them into road cells, 4 units square, whose centres a road
covers, and everything moves the game file's `road_speed` times as fast (1.5 by default) on them. A theme draws them.

worldgen lays the city's avenues (half width 4) and streets (3.5, paved), each village's high street, roads between
places (3) and tracks to forts (2.2). Past 16,384 units it lays the network in tiers instead: highways (3.5) joining
the city and the towns, roads (3) to the named villages and forts, every other village joined to the nearest road (2.2)
with lanes (1.8) to some of its neighbours, and tracks to forts (1.7) and farmsteads (1.3). Each tier is a
neighbourhood web over its places (no third place nearer both ends than they are to each other), and each link is
found on a grid coarse enough for its length inside a window round it, so the size of the world does not matter. The network grows outward from the city: each village and fort in turn is joined
to the nearest road already laid by the cheapest path over the land (gentle slopes, round anything solid, across as
little river as it can, along an existing road where one goes the same way), straightened and rounded off. Where a
road crosses a river the bed is raised to just under the water, a ford (an object of type `ford` marks each), and
houses line the roads for a little way out of the city's gates.

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
| `square` | footprint | An open square: a plaza, market or village green (a village is a green on a high street lined with houses, with a church tower and a back lane) |
| `field` | point | A field (worlds past 16,384 units): `q: [x0, y0, ... x3, y3]` its corners in order, `k` what grows (`pasture`, `hay`, `wheat`, `barley`, `oats`, `potato`), `r` the angle its furrows run at. Not solid. Its hedgerow is ordinary `bush` and `tree` objects along its edges |
| `ford` | point | Where a road wades a river: `r` is the road's direction there |
| `sea` | footprint, solid, scenery | Open sea past the shallows, as boxes: solid so nobody walks the sea floor, drawn as water (not as a wall) |
| `tree`, `tree_bare`, `rock` | point, solid | Scenery; the footprint is the trunk or the stone, so it is solid where it stands |
| `bush` | point | Scenery you walk through |
| `boulder`, `spire` | point, solid | Big rocks on the mountains past the edge (solid where they reach into the world) |

A point object may also carry `solid` and a footprint `b`: the engine treats it like any other solid footprint, and a
theme still draws it by its type at its point. Worldgen also marks those `scenery: true` (trees, rocks, boulders, spires),
and the engine treats any solid object with a point as scenery, marked or not: it still collides with them, but tells clients (`/play/tiles` class 1, `/play/info` `box_class`) that the file
draws them, so a client that draws walls as blocks leaves them out and nothing treats them as buildings.

No more than 4,096 solid footprints may touch any one 256 unit tile (counted from (0, 0)), and no more than 4,000,000
in all; the engine refuses a file past either.

A theme should draw an unknown type as something neutral (a plain block for a solid footprint), so new
generators work with old themes.

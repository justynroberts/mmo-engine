extends RefCounted
## Settlements and their character. Every square in the world file is the heart of a village or town (the squares
## inside the city are all the city), and each gets a look and a trade from the square itself, so every client
## agrees and no two neighbouring villages look alike:
## - a palette: what colour its roofs, plaster, timber and stone are (tints on the Medieval Village materials),
## - a way of building: stone or plaster ground floors, round or flat doors, how many storeys, jettied upper
##   floors, how steep the roofs are, fences, paving,
## - a trade, from what is round it, that decides what lies about the yards and the square: fishing by the sea or
##   a lake, mining under the crags, a market town when it is big, farming otherwise.
## A house in no settlement is a farmstead, styled by the stretch of country it stands in.

const REACH := 150.0   # how far from its square a village's houses go
const COUNTRY := 320.0 # farmsteads share a look across a stretch of country this wide

## Tints on the kit's materials, by material name: a colour to multiply by, or [colour, saturation] to wash the
## texture's own colour out first (so red tiles can become slate). The first is the kit as it comes.
const PALETTES := [
	{"name": "terracotta"},
	{"name": "slate", "MI_RoundTiles": [Color(0.66, 0.74, 0.88), 0.0], "MI_Plaster": Color(0.97, 0.97, 0.95), "MI_WoodTrim": Color(0.62, 0.57, 0.54)},
	{"name": "straw", "MI_RoundTiles": [Color(1.15, 0.95, 0.55), 0.25], "MI_Plaster": Color(1.0, 0.93, 0.8), "MI_WoodTrim": Color(0.9, 0.78, 0.62)},
	{"name": "moss", "MI_RoundTiles": [Color(0.62, 0.82, 0.48), 0.15], "MI_Plaster": Color(0.93, 0.92, 0.84), "MI_WoodTrim": Color(0.56, 0.52, 0.46)},
	{"name": "ochre", "MI_RoundTiles": Color(0.7, 0.48, 0.38), "MI_Plaster": Color(1.0, 0.78, 0.5), "MI_WoodTrim": Color(0.76, 0.63, 0.5)},
	{"name": "plum", "MI_RoundTiles": [Color(0.66, 0.46, 0.6), 0.0], "MI_Plaster": Color(1.0, 0.88, 0.86), "MI_WoodTrim": Color(0.6, 0.5, 0.5)},
	{"name": "charcoal", "MI_RoundTiles": [Color(0.46, 0.46, 0.5), 0.0], "MI_Plaster": Color(0.82, 0.88, 1.0), "MI_WoodTrim": Color(0.44, 0.41, 0.41), "MI_UnevenBrick": Color(0.78, 0.8, 0.88)},
	{"name": "sandstone", "MI_RoundTiles": [Color(1.05, 0.82, 0.62), 0.5], "MI_Plaster": Color(0.98, 0.9, 0.74), "MI_UnevenBrick": Color(1.05, 0.9, 0.7), "MI_Brick": Color(1.0, 0.86, 0.7)},
]

## What lies about, by trade: [name, scale], a name being a Resource Bits file, or "d:" a Dungeon piece, "v:" a
## village one, "t:" a tool (theme.gd _dress_path). Yards draw a few from `yard`; one house in `work_every` is the
## trade's workshop and has `work` laid out by its door; the square has `stall` goods on its tables and `square`
## piles round its edge.
const TRADES := {
	"farming": {
		"yard": [["Wood_Log_Stack", 1.3], ["Textiles_Stack_Small", 1.2], ["Pallet_Wood_Covered_A", 1.2], ["Wood_Log_A", 1.2], ["v:Prop_Wagon", 1.4], ["Textiles_B", 1.2], ["t:bucket_metal", 1.2]],
		"work": [["Wood_Log_A", 1.3], ["t:axe", 1.4], ["t:saw", 1.3], ["t:mallet", 1.3], ["Wood_Planks_Stack_Medium", 1.2], ["Wood_Log_Stack", 1.3]],
		"work_every": 4,
		"stall": ["Textiles_Stack_Small", "Textiles_B", "d:plate_food_A", "Textiles_A"],
		"square": [["Wood_Log_Stack", 1.3], ["Textiles_Stack_Small", 1.3], ["v:Prop_Wagon", 1.4], ["Pallet_Wood_Covered_B", 1.2]],
		"stalls": 2,
	},
	"fishing": {
		"yard": [["Pallet_Wood_Covered_A", 1.2], ["t:rope_bundle_A", 1.4], ["d:barrel_small_stack", 1.0], ["t:rope_bundle_B", 1.4], ["Wood_Planks_Stack_Small", 1.2], ["t:bucket_metal", 1.2], ["d:crates_stacked", 1.0]],
		"work": [["Pallet_Wood_Covered_B", 1.2], ["t:rope_bundle_A", 1.4], ["t:rope_bundle_B", 1.4], ["t:bucket_metal", 1.2], ["t:lantern", 1.3], ["d:barrel_small", 1.0]],
		"work_every": 3,
		"stall": ["d:plate_food_A", "d:barrel_small", "t:bucket_metal", "d:box_small"],
		"square": [["d:barrel_large", 1.0], ["t:rope_bundle_B", 1.5], ["Pallet_Wood_Covered_A", 1.2], ["Wood_Planks_Stack_Medium", 1.2]],
		"stalls": 3,
	},
	"mining": {
		"yard": [["Stone_Bricks_Stack_Small", 1.2], ["t:pickaxe", 1.3], ["Iron_Nuggets", 1.4], ["t:shovel", 1.3], ["Copper_Nuggets", 1.4], ["t:bucket_metal", 1.2], ["t:torch_burnt", 1.3], ["v:Prop_Wagon", 1.4]],
		"work": [["t:anvil", 1.4], ["t:grindstone", 1.3], ["t:tongs", 1.3], ["t:hammer", 1.3], ["Iron_Bars_Stack_Small", 1.2], ["Stone_Chunks_Small", 1.3]],
		"work_every": 3,
		"stall": ["Iron_Bars_Stack_Small", "Copper_Bars_Stack_Small", "Iron_Nuggets", "t:pickaxe"],
		"square": [["Stone_Bricks_Stack_Large", 1.2], ["Stone_Chunks_Large", 1.3], ["Iron_Bars_Stack_Medium", 1.2], ["t:anvil", 1.4]],
		"stalls": 2,
	},
	"market": {
		"yard": [["d:crates_stacked", 1.0], ["d:barrel_small", 1.0], ["Textiles_Stack_Large_Colored", 1.1], ["d:keg", 1.0], ["Pallet_Wood_Covered_B", 1.2], ["v:Prop_Crate", 1.4]],
		"work": [["t:anvil", 1.4], ["t:grindstone", 1.3], ["t:hammer", 1.3], ["t:tongs", 1.3], ["Iron_Bars_Stack_Small", 1.2], ["d:barrel_small", 1.0]],
		"work_every": 6,
		"stall": ["Textiles_Stack_Large_Colored", "Gold_Bars_Stack_Small", "d:coin_stack_small", "t:map", "t:journal_open", "Silver_Bars_Stack_Small"],
		"square": [["d:barrel_large_decorated", 1.0], ["d:crates_stacked", 1.0], ["Textiles_Stack_Large", 1.1], ["d:keg", 1.0]],
		"stalls": 6,
	},
}

var list: Array = []   # [{c, r, v, trade, pal, style}], the city's squares as one
var _city := Rect2()
## land.gd cold_at, set by the theme before survey: how cold it is at a latitude, for the north's palettes.
var cold_at := Callable()
const COLD_PALETTES := [1, 6, 3] # slate, charcoal, moss
var _country := {}     # stretch of country key -> farmstead character

## Look the world over once: its squares, and what is round each.
func survey(world: RefCounted) -> void:
	list.clear()
	_country.clear()
	var squares: Array = []
	var crags: Array = []
	var houses: Array = []
	for o in world.objects:
		var t := String(o.get("t", ""))
		if not o.has("b"):
			continue
		var b: Array = o["b"]
		var c := Vector2((float(b[0]) + float(b[2])) / 2.0, (float(b[1]) + float(b[3])) / 2.0)
		match t:
			"city": _city = Rect2(float(b[0]), float(b[1]), float(b[2]) - float(b[0]), float(b[3]) - float(b[1]))
			"square": squares.append([c, absi(int(o.get("v", 0)))])
			"crag": crags.append(c)
			"house": houses.append(c)
	var city_done := false
	for s in squares:
		var c: Vector2 = s[0]
		var v: int = s[1]
		if _city.has_area() and _city.has_point(c):
			if city_done:
				continue
			city_done = true
			list.append(_make(_city.get_center(), _city.size.length() / 2.0, v, "market", world))
			continue
		var count := 0
		for h in houses:
			if (h as Vector2).distance_to(c) < 110.0:
				count += 1
		var trade := "farming"
		if _near_water(world, c, 110.0):
			trade = "fishing"
		elif _near(crags, c, 95.0):
			trade = "mining"
		elif count >= 20:
			trade = "market"
		list.append(_make(c, REACH, v, trade, world))

func in_city(p: Vector2) -> bool:
	return _city.has_area() and _city.has_point(p)

## The settlement `p` is in, or the character of the farmsteads round it.
func at(p: Vector2) -> Dictionary:
	var best: Dictionary = {}
	var bd := INF
	for s in list:
		var d: float = (s["c"] as Vector2).distance_to(p)
		if d < s["r"] and d < bd:
			bd = d
			best = s
	if not best.is_empty():
		return best
	var k := Vector2i(floori(p.x / COUNTRY), floori(p.y / COUNTRY))
	if not _country.has(k):
		_country[k] = _make(Vector2(k) * COUNTRY, 0.0, absi(k.x * 73856093 ^ k.y * 19349663), "farming", null)
		_country[k]["farmstead"] = true
	return _country[k]

func _make(c: Vector2, r: float, v: int, trade: String, _world) -> Dictionary:
	var rng := RandomNumberGenerator.new()
	rng.seed = v * 2654435761 + 17
	var big := trade == "market"
	var style := {
		"stone": [0.0, 0.35, 0.7, 1.0][rng.randi() % 4],        # share of houses with a stone ground floor
		"round_door": [0.15, 0.5, 0.85][rng.randi() % 3],
		"timber_frame": rng.randf() < 0.5,                        # door frames of timber, or brick
		"floors": [1, 2] if not big else [2, 3],                   # storeys, least and most
		"jetty": [0.0, 0.2, 0.45][rng.randi() % 3],
		"pitch": [0.8, 0.95, 1.1, 1.3][rng.randi() % 4],          # how steep the roofs are
		"woodgrid": [0.1, 0.4, 0.8][rng.randi() % 3],             # share of upper walls with timber framing
		"shutters_open": rng.randf() < 0.5,
		"fence": ["wood", "wood", "metal", "none"][rng.randi() % 4],
		"paving": ["Floor_UnevenBrick", "Floor_Brick", "Floor_RedBrick"][rng.randi() % 3],
		"vines": [0.0, 0.15, 0.35][rng.randi() % 3],
		"balconies": rng.randf() < 0.5,
		"chimney": rng.randi() % 2,
	}
	if trade == "fishing":
		style["floors"] = [1, 2]
	var pal := rng.randi() % PALETTES.size()
	if cold_at.is_valid() and float(cold_at.call(c.y)) > 0.55:
		pal = COLD_PALETTES[pal % COLD_PALETTES.size()] # the north builds in slate and dark stone, under moss
	return {"c": c, "r": r, "v": v, "trade": trade, "pal": pal, "style": style,
		"describe": "%s village, %s" % [trade, PALETTES[pal]["name"]] if r > 0.0 else "farmstead"}

static func _near(points: Array, c: Vector2, d: float) -> bool:
	for p in points:
		if (p as Vector2).distance_to(c) < d:
			return true
	return false

## Sea (ground below sea level) or a lake within `d` of `c`.
static func _near_water(world: RefCounted, c: Vector2, d: float) -> bool:
	if world.has_sea:
		for k in 16:
			var a := TAU * k / 16.0
			for r: float in [d * 0.35, d * 0.7, d]:
				var p := c + Vector2(cos(a), sin(a)) * r
				if world.height(p.x, p.y) < world.sea_level - 0.2:
					return true
	for l in world.lakes:
		var b: Array = l["b"]
		var q := Vector2(clampf(c.x, float(b[0]), float(b[2])), clampf(c.y, float(b[1]), float(b[3])))
		if q.distance_to(c) < d:
			return true
	return false

extends RefCounted
## What the land is like between the things the world file names: how cold it is (the north is), where the ground is
## wet by water or shaded by trees, the fields round farmsteads, and where country roads end and meet. All of it follows from
## the world file and positions alone, so every client sees the same land; nothing here is solid or on the server.
##
## The ground shader reads a small map per cell (ground_map): r how much road, g paved where there is road and a
## field where there is none (0.5 furrows along x, 1.0 along z), b how near water, a how much under trees.

const MAP_N := 64                 # map texels along a cell's edge (2 units each)
const WET_REACH := 9.0            # how far past a river's or lake's edge the ground stays lush
const SHADE_R := 4.5              # how far round a tree (at scale 1) the ground is woodland floor
const FIELD_GAP := 3.0            # between a farmhouse and its field

var world: RefCounted
var towns: RefCounted
## The band of latitude (world y, north is small) where the cold sets in: fully cold at x, none by y.
var cold_band := Vector2(-1e9, -1e9)
var _fields := {}                 # cell key -> fields_of_cell, for the farmhouses in it
var _junctions := {}              # cell key -> [[Vector2, [unit directions...]], ...] where roads end or meet
var _river_segs := {}             # cell key -> [[a, b, half width, level at a, level at b], ...] reaching near it

func setup(p_world: RefCounted, p_towns: RefCounted) -> void:
	world = p_world
	towns = p_towns
	var ext: Rect2 = world.extent()
	if world.has_heights:
		cold_band = Vector2(ext.position.y + ext.size.y * 0.2, ext.position.y + ext.size.y * 0.48)
	_find_junctions()
	_index_rivers()

## Every river segment, by the cells it comes within WET_REACH of, so nothing scans every river for one spot.
func _index_rivers() -> void:
	for rv in world.rivers:
		var pts: Array = rv["pts"]
		var hw := float(rv.get("half_width", 5.0))
		for k in pts.size() - 1:
			var a := Vector2(pts[k][0], pts[k][1])
			var b := Vector2(pts[k + 1][0], pts[k + 1][1])
			var box := Rect2(a, Vector2.ZERO).expand(b).grow(hw + WET_REACH + 1.0)
			var seg := [a, b, hw, float(pts[k][2]), float(pts[k + 1][2])]
			for cx in range(int(floor(box.position.x / world.CELL)), int(floor(box.end.x / world.CELL)) + 1):
				for cy in range(int(floor(box.position.y / world.CELL)), int(floor(box.end.y / world.CELL)) + 1):
					var ck: Variant = world.key(cx, cy)
					if not _river_segs.has(ck):
						_river_segs[ck] = []
					_river_segs[ck].append(seg)

## The river segments near a cell: [[a, b, half width, level at a, level at b], ...].
func river_segments(cx: int, cy: int) -> Array:
	return _river_segs.get(world.key(cx, cy), [])

## 1 in the far north, 0 from the middle of the map south (the shader adds a ragged edge to the same band).
func cold_at(y: float) -> float:
	return 1.0 - smoothstep(cold_band.x, cold_band.y, y)

# ------------------------------------------------------------------------------------- where roads end and meet

## Every country road runs from one place to another, and roads meet only end to end: so a road's ends are where a
## traveller arrives somewhere or the way divides, and that is where a lantern and a signpost stand.
func _find_junctions() -> void:
	var ends := {}  # rounded point -> [point, directions of the roads leaving it]
	for r in world.roads:
		var pts: Array = r["pts"]
		if pts.size() < 2 or float(r.get("half_width", 3.0)) >= 3.4:
			continue # the city's paved streets have their own lamps and squares
		for e in [[pts[0], pts[1]], [pts[-1], pts[-2]]]:
			var p := Vector2(e[0][0], e[0][1])
			var q := Vector2(e[1][0], e[1][1])
			if p.distance_to(q) < 0.01:
				continue
			var key := Vector2i(roundi(p.x), roundi(p.y))
			if not ends.has(key):
				ends[key] = [p, []]
			ends[key][1].append((q - p).normalized())
	for key in ends:
		var p: Vector2 = ends[key][0]
		var ck: Variant = world.key(int(floor(p.x / world.CELL)), int(floor(p.y / world.CELL)))
		if not _junctions.has(ck):
			_junctions[ck] = []
		_junctions[ck].append([p, ends[key][1]])

## Where roads end or meet in a cell: [[point, [directions of the roads leaving it]], ...].
func junctions(cx: int, cy: int) -> Array:
	return _junctions.get(world.key(cx, cy), [])

# ------------------------------------------------------------------------------------- farm fields

## The fields of the farmhouses in a cell (farmsteads, and the houses of farming villages): one or two strips beside
## each where there is room, kept clear
## of roads, water, buildings and steep ground. [[Rect2, furrows along z, side of the house (0 N, 1 S, 2 W, 3 E)], ...]
## A world file that lays out its own fields (world.farms) gives those instead, as [bounding Rect2, furrows nearer z
## than x, -1 (no house side: hedgerows, not fences, bound them), corners, crop, furrow angle].
func fields_of_cell(cx: int, cy: int) -> Array:
	var k: Variant = world.key(cx, cy)
	if _fields.has(k):
		return _fields[k]
	var out := []
	if world.farms:
		for o in world.in_cell(cx, cy):
			if String(o.get("t", "")) != "field":
				continue
			var q: Array = o.get("q", [])
			if q.size() != 8:
				continue
			var quad := PackedVector2Array([Vector2(q[0], q[1]), Vector2(q[2], q[3]), Vector2(q[4], q[5]), Vector2(q[6], q[7])])
			var box := Rect2(quad[0], Vector2.ZERO)
			for p in quad:
				box = box.expand(p)
			var ang := float(o.get("r", 0.0))
			out.append([box, absf(sin(ang)) > absf(cos(ang)), -1, quad, String(o.get("k", "pasture")), ang])
		# Fields stream in with the trees: an empty list is only kept once this cell's points are in.
		if world.points_in(cx, cy):
			_fields[k] = out
		return out
	var boxes := _boxes_near(cx, cy)
	for o in world.in_cell(cx, cy):
		if String(o.get("t", "")) != "house":
			continue
		var b: Array = o["b"]
		var house := Rect2(float(b[0]), float(b[1]), float(b[2]) - float(b[0]), float(b[3]) - float(b[1]))
		var town: Dictionary = towns.at(house.get_center())
		if not town.get("farmstead", false) and String(town.get("trade", "")) != "farming":
			continue
		var v := absi(int(o.get("v", 0)))
		var rng := RandomNumberGenerator.new()
		rng.seed = v * 7919 + 13
		var tries := [0, 1, 2, 3]
		var start := v % 4
		var made := 0
		for t in 4:
			if made >= 1 + v % 2:
				break
			var side: int = tries[(start + t) % 4]
			var long := rng.randf_range(18.0, 30.0)
			var deep := rng.randf_range(12.0, 20.0)
			var c := house.get_center()
			var r: Rect2
			match side:
				0: r = Rect2(c.x - long / 2.0, house.position.y - FIELD_GAP - deep, long, deep)       # north
				1: r = Rect2(c.x - long / 2.0, house.end.y + FIELD_GAP, long, deep)                   # south
				2: r = Rect2(house.position.x - FIELD_GAP - deep, c.y - long / 2.0, deep, long)       # west
				3: r = Rect2(house.end.x + FIELD_GAP, c.y - long / 2.0, deep, long)                   # east
			if _field_ok(r, boxes, out):
				out.append([r, side < 2 if v % 3 != 0 else side >= 2, side])
				made += 1
	_fields[k] = out
	return out

## The fields reaching into an area (a cell's map, the grass round us).
func fields_in(area: Rect2) -> Array:
	var out := []
	var pad := 100.0 if world.farms else 40.0 # a world file's fields are bigger, and kept by their middles
	var c0 := Vector2i(int(floor((area.position.x - pad) / world.CELL)), int(floor((area.position.y - pad) / world.CELL)))
	var c1 := Vector2i(int(floor((area.end.x + pad) / world.CELL)), int(floor((area.end.y + pad) / world.CELL)))
	for cx in range(c0.x, c1.x + 1):
		for cy in range(c0.y, c1.y + 1):
			for f in fields_of_cell(cx, cy):
				if (f[0] as Rect2).intersects(area):
					out.append(f)
	return out

func _boxes_near(cx: int, cy: int) -> Array:
	var out := []
	for dx in [-1, 0, 1]:
		for dy in [-1, 0, 1]:
			for o in world.in_cell(cx + dx, cy + dy):
				if o.has("b"):
					var b: Array = o["b"]
					out.append(Rect2(float(b[0]), float(b[1]), float(b[2]) - float(b[0]), float(b[3]) - float(b[1])).grow(1.5))
	return out

func _field_ok(r: Rect2, boxes: Array, made: Array) -> bool:
	for b in boxes:
		if (b as Rect2).intersects(r):
			return false
	for f in made:
		if (f[0] as Rect2).grow(2.0).intersects(r):
			return false
	var pts := [r.position, Vector2(r.end.x, r.position.y), r.end, Vector2(r.position.x, r.end.y), r.get_center()]
	var lo := INF
	var hi := -INF
	for p in pts:
		var h: float = world.height(p.x, p.y)
		lo = minf(lo, h)
		hi = maxf(hi, h)
		if world.has_sea and h < world.sea_level + 1.5:
			return false
	if hi - lo > 3.0:
		return false
	if near_road(r.grow(2.0)) or wet_at_rect(r.grow(3.0)):
		return false
	return true

## Whether any road runs through a box.
func near_road(r: Rect2) -> bool:
	var c0 := Vector2i(int(floor(r.position.x / world.CELL)), int(floor(r.position.y / world.CELL)))
	var c1 := Vector2i(int(floor(r.end.x / world.CELL)), int(floor(r.end.y / world.CELL)))
	for cx in range(c0.x, c1.x + 1):
		for cy in range(c0.y, c1.y + 1):
			for s in world.road_segments(cx, cy):
				var a := Vector2(s[0], s[1])
				var b := Vector2(s[2], s[3])
				var g := r.grow(float(s[4]))
				if g.has_point(a) or g.has_point(b) or _seg_hits_rect(a, b, g):
					return true
	return false

## Whether water (a river's channel or a lake) is in a box.
func wet_at_rect(r: Rect2) -> bool:
	for l in world.lakes:
		var lb: Array = l["b"]
		if Rect2(float(lb[0]), float(lb[1]), float(lb[2]) - float(lb[0]), float(lb[3]) - float(lb[1])).intersects(r):
			return true
	for cx in range(int(floor(r.position.x / world.CELL)), int(floor(r.end.x / world.CELL)) + 1):
		for cy in range(int(floor(r.position.y / world.CELL)), int(floor(r.end.y / world.CELL)) + 1):
			for sg in river_segments(cx, cy):
				var g := r.grow(float(sg[2]))
				if g.has_point(sg[0]) or g.has_point(sg[1]) or _seg_hits_rect(sg[0], sg[1], g):
					return true
	return false

static func _seg_hits_rect(a: Vector2, b: Vector2, r: Rect2) -> bool:
	var corners := [r.position, Vector2(r.end.x, r.position.y), r.end, Vector2(r.position.x, r.end.y)]
	for i in 4:
		if Geometry2D.segment_intersects_segment(a, b, corners[i], corners[(i + 1) % 4]) != null:
			return true
	return false

# ------------------------------------------------------------------------------------- the ground map

## A cell's ground map (MAP_N x MAP_N, RGBA8; see the top), or an empty array when the cell is plain meadow.
func ground_map(cx: int, cy: int) -> PackedByteArray:
	var n := MAP_N
	var px: float = world.CELL / n
	var ox: float = cx * world.CELL
	var oy: float = cy * world.CELL
	var cell := Rect2(ox, oy, world.CELL, world.CELL)
	var map := PackedByteArray()
	map.resize(n * n * 4)
	var any := false
	var stamp := func(box: Rect2, ch: int, f: Callable) -> void:
		var i0 := clampi(int((box.position.x - ox) / px), 0, n - 1)
		var i1 := clampi(int((box.end.x - ox) / px), 0, n - 1)
		var j0 := clampi(int((box.position.y - oy) / px), 0, n - 1)
		var j1 := clampi(int((box.end.y - oy) / px), 0, n - 1)
		for j in range(j0, j1 + 1):
			for i in range(i0, i1 + 1):
				var v: float = f.call(Vector2(ox + (i + 0.5) * px, oy + (j + 0.5) * px))
				if v <= 0.0:
					continue
				var at := (j * n + i) * 4 + ch
				var q := int(clampf(v, 0.0, 1.0) * 255.0)
				if q > map[at]:
					map[at] = q
	# Roads: how much road (r) and, for the wide city streets, paved (g).
	for s in world.road_segments(cx, cy):
		var a := Vector2(s[0], s[1])
		var b := Vector2(s[2], s[3])
		var hw: float = s[4]
		var box := Rect2(a, Vector2.ZERO).expand(b).grow(hw + 1.5)
		if not box.intersects(cell):
			continue
		any = true
		var road := func(p: Vector2) -> float: return (hw + 0.75 - Geometry2D.get_closest_point_to_segment(p, a, b).distance_to(p)) / 1.5
		stamp.call(box, 0, road)
		if hw >= 3.4:
			stamp.call(box, 1, road)
	# Fields (g where there is no road): 0.5 for furrows along x, 1.0 along z. A world file's own fields go in the
	# field map instead (field_map), which carries their crop and the angle of their furrows.
	for f in ([] if world.farms else fields_in(cell)):
		var r: Rect2 = f[0]
		var along: float = 1.0 if f[1] else 0.5
		any = true
		var i0 := clampi(int((r.position.x - ox) / px), 0, n - 1)
		var i1 := clampi(int((r.end.x - ox) / px), 0, n - 1)
		var j0 := clampi(int((r.position.y - oy) / px), 0, n - 1)
		var j1 := clampi(int((r.end.y - oy) / px), 0, n - 1)
		for j in range(j0, j1 + 1):
			for i in range(i0, i1 + 1):
				var at := (j * n + i) * 4
				if map[at] == 0 and r.has_point(Vector2(ox + (i + 0.5) * px, oy + (j + 0.5) * px)):
					map[at + 1] = int(along * 255.0)
	# Wet ground along rivers and round lakes (b).
	for sg in river_segments(cx, cy):
		var a: Vector2 = sg[0]
		var b: Vector2 = sg[1]
		var hw: float = sg[2]
		var box := Rect2(a, Vector2.ZERO).expand(b).grow(hw + WET_REACH)
		if not box.intersects(cell):
			continue
		any = true
		stamp.call(box, 2, func(p: Vector2) -> float: return 1.0 - (Geometry2D.get_closest_point_to_segment(p, a, b).distance_to(p) - hw) / WET_REACH)
	for l in world.lakes:
		var lb: Array = l["b"]
		var lr := Rect2(float(lb[0]), float(lb[1]), float(lb[2]) - float(lb[0]), float(lb[3]) - float(lb[1]))
		var box := lr.grow(WET_REACH)
		if not box.intersects(cell):
			continue
		any = true
		# From the water itself, not the box: worldgen rounds the basin and lets its shore wander inside the box.
		# Water texels first, then how far each texel is from one (a two-pass chamfer over the texels round it).
		var reg := box.intersection(cell.grow(WET_REACH))
		var gw := int(ceil(reg.size.x / px)) + 1
		var gh := int(ceil(reg.size.y / px)) + 1
		var level := float(l.get("level", 0.0))
		var dist := PackedFloat32Array()
		dist.resize(gw * gh)
		var wet := false
		for j in gh:
			for i in gw:
				var p := reg.position + Vector2(i, j) * px
				var water: bool = lr.has_point(p) and world.height(p.x, p.y) < level
				dist[j * gw + i] = 0.0 if water else INF
				wet = wet or water
		if not wet:
			continue # no water within reach of this cell (the box's dry margin): nothing to wet
		var diag := px * 1.4142
		for j in gh:
			for i in gw:
				var d := dist[j * gw + i]
				if i > 0: d = minf(d, dist[j * gw + i - 1] + px)
				if j > 0: d = minf(d, dist[(j - 1) * gw + i] + px)
				if i > 0 and j > 0: d = minf(d, dist[(j - 1) * gw + i - 1] + diag)
				if i < gw - 1 and j > 0: d = minf(d, dist[(j - 1) * gw + i + 1] + diag)
				dist[j * gw + i] = d
		for j in range(gh - 1, -1, -1):
			for i in range(gw - 1, -1, -1):
				var d := dist[j * gw + i]
				if i < gw - 1: d = minf(d, dist[j * gw + i + 1] + px)
				if j < gh - 1: d = minf(d, dist[(j + 1) * gw + i] + px)
				if i < gw - 1 and j < gh - 1: d = minf(d, dist[(j + 1) * gw + i + 1] + diag)
				if i > 0 and j < gh - 1: d = minf(d, dist[(j + 1) * gw + i - 1] + diag)
				dist[j * gw + i] = d
		stamp.call(reg, 2, func(p: Vector2) -> float:
			var i := clampi(roundi((p.x - reg.position.x) / px), 0, gw - 1)
			var j := clampi(roundi((p.y - reg.position.y) / px), 0, gh - 1)
			return 1.0 - dist[j * gw + i] / WET_REACH)
	# Woodland floor under and round trees and bushes (a), from this cell and the edges of its neighbours.
	for dx in [-1, 0, 1]:
		for dy in [-1, 0, 1]:
			for o in world.in_cell(cx + dx, cy + dy):
				var t := String(o.get("t", ""))
				if t != "tree" and t != "tree_bare" and t != "bush":
					continue
				var p := Vector2(o["p"][0], o["p"][1])
				var rr := SHADE_R * float(o.get("s", 1.0)) * (0.6 if t == "bush" else 1.0)
				var box := Rect2(p, Vector2.ZERO).grow(rr)
				if not box.intersects(cell):
					continue
				any = true
				stamp.call(box, 3, func(q: Vector2) -> float: return 1.0 - q.distance_to(p) / rr)
	return map if any else PackedByteArray()

# ------------------------------------------------------------------------------------- the field map

## What grows in a world file's fields, as the field map's red channel (sampled nearest, so codes stay whole).
const CROPS := {"pasture": 1, "hay": 2, "wheat": 3, "barley": 4, "oats": 5, "potato": 6}

## A cell's field map (MAP_N x MAP_N, RG8): r the crop (CROPS, times 40), g the furrows' angle (0..PI as 0..255).
## Empty when no world-file field reaches the cell.
func field_map(cx: int, cy: int) -> PackedByteArray:
	if not world.farms:
		return PackedByteArray()
	var n := MAP_N
	var px: float = world.CELL / n
	var ox: float = cx * world.CELL
	var oy: float = cy * world.CELL
	var cell := Rect2(ox, oy, world.CELL, world.CELL)
	var map := PackedByteArray()
	var any := false
	for f in fields_in(cell):
		if f.size() < 6:
			continue
		var box: Rect2 = f[0]
		if not box.intersects(cell):
			continue
		if not any:
			map.resize(n * n * 2)
			any = true
		var quad: PackedVector2Array = f[3]
		var code := int(CROPS.get(f[4], 1)) * 40
		var ang := fposmod(float(f[5]), PI)
		var g := int(ang / PI * 255.0)
		var i0 := clampi(int((box.position.x - ox) / px), 0, n - 1)
		var i1 := clampi(int((box.end.x - ox) / px), 0, n - 1)
		var j0 := clampi(int((box.position.y - oy) / px), 0, n - 1)
		var j1 := clampi(int((box.end.y - oy) / px), 0, n - 1)
		for j in range(j0, j1 + 1):
			for i in range(i0, i1 + 1):
				if Geometry2D.is_point_in_polygon(Vector2(ox + (i + 0.5) * px, oy + (j + 0.5) * px), quad):
					var at := (j * n + i) * 2
					map[at] = code
					map[at + 1] = g
	return map

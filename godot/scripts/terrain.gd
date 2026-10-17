extends RefCounted
## The shape of the world, worked out the same way on every client from the world seed and the server's walls.
##
## The server's world is flat and only its wall boxes block movement, so everything here is drawn to agree with that:
## - Each wall box is a keep, a crag (a rocky mountain) or a lake, chosen from its coordinates. All three are
##   impassable on the server, so you can neither walk into the lake nor up the mountain.
## - Rivers meander from the seed, always run downhill (their level never rises along the course) and cut a valley
##   or a gorge through the hills. They are shallow: the server lets you walk through, so you wade.
## - Hills roll everywhere, are levelled around buildings, and rise into mountains past the edge of the playable area.
## Box kinds and river courses use only integer maths and plain arithmetic, so the console's map (ui/src/components/
## mapLook.ts) draws the same ones. Its hills are its own approximation.

const CELL := 128.0
const FLAT_PAD := 10.0
const RIVER_HALF := 3.5   # half the width of the water
const RIVER_DEPTH := 1.6  # bed below the river's level
const KEEP := 0
const CRAG := 1
const LAKE := 2

var seed := 1
var size := 1024.0
var boxes: Array = []
var kinds := PackedInt32Array()
var base := PackedFloat32Array() # ground height each box stands on
var rivers: Array = []           # [{pts: PackedVector2Array, lvl: PackedFloat32Array}]
var hills := FastNoiseLite.new()
var bumps := FastNoiseLite.new()
var peaks := FastNoiseLite.new()
var _box_cells := {}             # cell -> box indices (padded)
var _river_cells := {}           # cell -> [river, segment] pairs (padded)

func setup(world_seed: int, world_size: float, wall_boxes: Array) -> void:
	seed = world_seed
	size = world_size
	boxes = wall_boxes
	hills.seed = seed
	hills.frequency = 1.0 / 170.0
	hills.fractal_octaves = 3
	bumps.seed = seed + 1
	bumps.frequency = 1.0 / 35.0
	peaks.seed = seed + 2
	peaks.frequency = 1.0 / 60.0
	peaks.fractal_octaves = 4
	kinds.resize(boxes.size())
	base.resize(boxes.size())
	_box_cells.clear()
	for i in boxes.size():
		var b: Array = boxes[i]
		kinds[i] = box_kind(b)
		base[i] = raw((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
		for cx in range(_cell(b[0] - FLAT_PAD), _cell(b[2] + FLAT_PAD) + 1):
			for cy in range(_cell(b[1] - FLAT_PAD), _cell(b[3] + FLAT_PAD) + 1):
				_push(_box_cells, _key(cx, cy), i)
	_make_rivers()

## Keep, crag or lake, from the box's own corner, so every client (and the console) agrees.
static func box_kind(b: Array) -> int:
	var w: float = b[2] - b[0]
	var d: float = b[3] - b[1]
	if minf(w, d) < 10.0:
		return KEEP
	var h := (int(b[0]) * 7 + int(b[1]) * 13) % 10
	if h >= 8 and minf(w, d) >= 12.0:
		return LAKE
	if h >= 5:
		return CRAG
	return KEEP

func _cell(v: float) -> int:
	return int(floor(v / CELL))

func _key(cx: int, cy: int) -> int:
	return cx * 100003 + cy

func _push(d: Dictionary, k: int, v) -> void:
	if not d.has(k):
		d[k] = []
	d[k].append(v)

# ---------------------------------------------------------------- height

## Hills, plus mountains rising past the playable edge.
func raw(x: float, y: float) -> float:
	var h := hills.get_noise_2d(x, y) * 15.0 + bumps.get_noise_2d(x, y) * 0.9
	var out := maxf(maxf(-x, x - size), maxf(-y, y - size))
	if out > 0.0:
		h += smoothstep(0.0, 110.0, out) * (55.0 + 30.0 * peaks.get_noise_2d(x, y))
	return h

## The ground: hills, cut by rivers, levelled around buildings, sunk into lake beds.
func height(x: float, y: float) -> float:
	var h := _carve(x, y, raw(x, y))
	var best := 0.0
	var at := h
	for i in _box_cells.get(_key(_cell(x), _cell(y)), []):
		var b: Array = boxes[i]
		var dx := maxf(maxf(b[0] - x, x - b[2]), 0.0)
		var dy := maxf(maxf(b[1] - y, y - b[3]), 0.0)
		if dx == 0.0 and dy == 0.0 and kinds[i] == LAKE:
			return _lake_bed(b, base[i], x, y)
		var w := 1.0 - smoothstep(0.0, FLAT_PAD, sqrt(dx * dx + dy * dy))
		if w > best:
			best = w
			at = base[i]
	return lerpf(h, at, best)

## A lake bed: a rounded, ragged shoreline inside the box (a strip of beach between water and the box's edge, which
## is where the server stops you), shelving down to deep water in the middle.
func _lake_bed(b: Array, ground: float, x: float, y: float) -> float:
	var c := Vector2((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
	var half := Vector2((b[2] - b[0]) / 2.0, (b[3] - b[1]) / 2.0)
	var r := minf(8.0, minf(half.x, half.y) * 0.6)
	var q := (Vector2(x, y) - c).abs() - (half - Vector2(r, r))
	var inside := r - Vector2(maxf(q.x, 0.0), maxf(q.y, 0.0)).length() - minf(maxf(q.x, q.y), 0.0)
	var shore := 1.5 + 3.0 * (0.5 + 0.5 * bumps.get_noise_2d(x * 3.0, y * 3.0))
	var depth := inside - shore
	if depth <= 0.0:
		return ground - 0.15 * clampf(inside / shore, 0.0, 1.0)
	return ground - 0.6 - minf(6.0, depth * 0.9)

## Where a lake's surface is (for a box of that kind).
func lake_level(i: int) -> float:
	return base[i] - 0.45

# ---------------------------------------------------------------- rivers

## A small deterministic generator (an LCG on exact integers) so the console can follow the same courses.
var _s := 0
func _next() -> float:
	_s = (_s * 1664525 + 1013904223) % 4294967296
	return float(_s) / 4294967296.0

func _make_rivers() -> void:
	rivers.clear()
	_river_cells.clear()
	_s = (seed * 2654435761 + 12345) % 4294967296
	var count := clampi(2 + int(size / 2000.0), 2, 6)
	var step := 10.0
	for r in count:
		# Start on an edge, heading roughly across the world.
		var side := int(_next() * 4.0)
		var along := (0.15 + 0.7 * _next()) * size
		var starts: Array[Vector2] = [Vector2(along, 0), Vector2(size, along), Vector2(along, size), Vector2(0, along)]
		var aims: Array[float] = [PI / 2.0, PI, -PI / 2.0, 0.0]
		var p: Vector2 = starts[side]
		var aim: float = aims[side]
		var heading: float = aim + (_next() - 0.5) * 0.8
		var pts := PackedVector2Array([p])
		var lvl := PackedFloat32Array([raw(p.x, p.y)])
		for n in 2000:
			heading += (_next() - 0.5) * 0.35
			# A gentle pull back towards the far side, so a river crosses the land instead of doubling back.
			var off := fmod(aim - heading + PI, TAU) - PI
			if off < -PI:
				off += TAU
			heading += off * 0.06
			var next := p + Vector2(cos(heading), sin(heading)) * step
			# Bend away from buildings rather than run under them: try either side of the course, wider each time.
			if _blocked(next):
				var course := heading
				for t in 8:
					var turn := 0.4 * float(t / 2 + 1) * (1.0 if t % 2 == 0 else -1.0)
					heading = course + turn
					next = p + Vector2(cos(heading), sin(heading)) * step
					if not _blocked(next):
						break
			var outside := next.x < 0.0 or next.y < 0.0 or next.x > size or next.y > size
			if outside and n < 30:
				heading = aim # too soon to leave: turn back towards the far side
				continue
			if outside:
				pts.append(next)
				lvl.append(minf(lvl[lvl.size() - 1], raw(next.x, next.y)))
				break
			p = next
			pts.append(p)
			# Water never runs uphill: the level only falls, so a river cuts a gorge through a rise.
			lvl.append(minf(lvl[lvl.size() - 1], raw(p.x, p.y)))
		var ri := rivers.size()
		rivers.append({"pts": pts, "lvl": lvl})
		var pad := RIVER_HALF + 40.0
		for k in pts.size() - 1:
			var a := pts[k]
			var b := pts[k + 1]
			for cx in range(_cell(minf(a.x, b.x) - pad), _cell(maxf(a.x, b.x) + pad) + 1):
				for cy in range(_cell(minf(a.y, b.y) - pad), _cell(maxf(a.y, b.y) + pad) + 1):
					_push(_river_cells, _key(cx, cy), [ri, k])

func _blocked(p: Vector2) -> bool:
	for i in _box_cells.get(_key(_cell(p.x), _cell(p.y)), []):
		var b: Array = boxes[i]
		if p.x > b[0] - 14.0 and p.x < b[2] + 14.0 and p.y > b[1] - 14.0 and p.y < b[3] + 14.0:
			return true
	return false

## Distance to the nearest river and that river's level there, or [INF, 0].
func river_at(x: float, y: float) -> Array:
	var best := INF
	var level := 0.0
	var q := Vector2(x, y)
	for e in _river_cells.get(_key(_cell(x), _cell(y)), []):
		var pts: PackedVector2Array = rivers[e[0]]["pts"]
		var lvl: PackedFloat32Array = rivers[e[0]]["lvl"]
		var a := pts[e[1]]
		var b := pts[e[1] + 1]
		var ab := b - a
		var t := clampf((q - a).dot(ab) / maxf(ab.length_squared(), 0.0001), 0.0, 1.0)
		var d := q.distance_to(a + ab * t)
		if d < best:
			best = d
			level = lerpf(lvl[e[1]], lvl[e[1] + 1], t)
	return [best, level]

## Cut the river's channel and its banks into the ground.
func _carve(x: float, y: float, h: float) -> float:
	var r := river_at(x, y)
	var d: float = r[0]
	if d == INF:
		return h
	var bed: float = r[1] - RIVER_DEPTH
	if d <= RIVER_HALF:
		return minf(h, bed + d / RIVER_HALF * 0.4)
	var bank := maxf(8.0, (h - bed) * 1.3) # steep ground gives a gorge, flat ground a gentle valley
	return minf(h, lerpf(bed + 0.4, h, smoothstep(RIVER_HALF, RIVER_HALF + bank, d)))

## True where there is water, or a riverbank too wet for trees.
func wet(x: float, y: float) -> bool:
	return river_at(x, y)[0] < RIVER_HALF + 3.0

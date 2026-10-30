extends RefCounted
## A world, as data (WORLD.md): the heightfield, the water and the objects, read from the server's world file.
## Themes draw from this; nothing here decides how anything looks.
##
## With no world file (a server started without --world), the world is the server's plain walls on flat ground:
## each becomes a solid object of type "block", which a theme draws as something neutral.

const CELL := 128.0 # objects are indexed by this cell, which is also the size of a theme's streaming chunk

var size := 1024.0
var objects: Array = []          # every object, as in the file
var rivers: Array = []           # [{half_width, pts: [[x, y, level], ...]}]
var lakes: Array = []            # [{b, level}]
var roads: Array = []            # [{half_width, pts: [[x, y], ...]}]
var has_sea := false             # land with a coast: open sea round it at sea_level
var farms := false               # the file lays out its own fields (objects of type "field"); a theme draws those
var sea_level := 0.0
var _road_segs := {}             # cell key -> [[ax, ay, bx, by, half_width], ...] of the road segments reaching into it
var has_heights := false
var height_top := 0.0            # the highest ground in the heights
var _hx0 := 0.0
var _hy0 := 0.0
var _hcell := 1.0
var _hw := 0
var _hh := 0
var _heights := PackedFloat32Array()
var _cells := {}                 # cell key -> indices of objects whose position (or footprint centre) is in it
var _streamed := {}              # ids of walls added by add_boxes
var _points := {}                # cell key -> point objects (trees, rocks) streamed in by tile
var _point_tiles := {}           # tile key -> true, for the tiles whose points are in

## Point objects arrive by tile (/play/world/tiles); a tile is exactly 2 x 2 cells.
const TILE := 256.0

static func from_file(text: String) -> RefCounted:
	var d = JSON.parse_string(text)
	if not d is Dictionary or int(d.get("format", 0)) != 1:
		return null
	var w = load("res://scripts/world_data.gd").new()
	w.size = float(d.get("size", 1024))
	w.objects = d.get("objects", [])
	var water: Dictionary = d.get("water", {})
	w.rivers = water.get("rivers", [])
	w.lakes = water.get("lakes", [])
	if water.get("sea") is Dictionary:
		w.has_sea = true
		w.sea_level = float(water["sea"].get("level", 0.0))
	w.roads = d.get("roads", [])
	w.farms = bool(d.get("farms", false))
	w._index_roads()
	var h: Dictionary = d.get("heights", {})
	if h.has("u16le"):
		w._hx0 = float(h["x0"])
		w._hy0 = float(h["y0"])
		w._hcell = float(h["cell"])
		w._hw = int(h["w"])
		w._hh = int(h["h"])
		var raw := Marshalls.base64_to_raw(String(h["u16le"]))
		var lo := float(h["lo"])
		var span := float(h["hi"]) - lo
		w.height_top = float(h["hi"])
		w._heights.resize(w._hw * w._hh)
		for i in w._hw * w._hh:
			w._heights[i] = lo + span * float(raw.decode_u16(i * 2)) / 65535.0
		w.has_heights = true
	w._index()
	return w

## The server's walls with nothing else: solid blocks on flat ground.
static func from_boxes(world_size: float, boxes: Array) -> RefCounted:
	var w = load("res://scripts/world_data.gd").new()
	w.size = world_size
	for b in boxes:
		w.objects.append({"t": "block", "b": b, "solid": true, "y": 0.0, "v": absi(int(b[0]) * 73856093 ^ int(b[1]) * 19349663)})
	w._index()
	return w

## Walls streamed in by tile ([id, x0, y0, x1, y1, class] each), for a world with no file: each new one becomes a
## block. Returns the keys of the cells that gained one, for a theme to redraw.
func add_boxes(list: Array) -> Array:
	var touched := {}
	for e in list:
		if _streamed.has(e[0]):
			continue
		_streamed[e[0]] = true
		var b := [e[1], e[2], e[3], e[4]]
		var o := {"t": "block", "b": b, "solid": true, "y": 0.0, "v": absi(int(b[0]) * 73856093 ^ int(b[1]) * 19349663)}
		objects.append(o)
		var at := centre(o)
		var k := key(int(floor(at.x / CELL)), int(floor(at.y / CELL)))
		if not _cells.has(k):
			_cells[k] = []
		_cells[k].append(objects.size() - 1)
		touched[k] = true
	return touched.keys()

func _index() -> void:
	_cells.clear()
	for i in objects.size():
		var o: Dictionary = objects[i]
		var at := centre(o)
		var k := key(int(floor(at.x / CELL)), int(floor(at.y / CELL)))
		if not _cells.has(k):
			_cells[k] = []
		_cells[k].append(i)

## Each road segment goes in every cell it (with its width and a little more) reaches.
func _index_roads() -> void:
	_road_segs.clear()
	for r in roads:
		var hw := float(r.get("half_width", 3.0))
		var pts: Array = r.get("pts", [])
		for k in pts.size() - 1:
			var ax := float(pts[k][0])
			var ay := float(pts[k][1])
			var bx := float(pts[k + 1][0])
			var by := float(pts[k + 1][1])
			var pad := hw + 2.0
			for i in range(floori((minf(ax, bx) - pad) / CELL), floori((maxf(ax, bx) + pad) / CELL) + 1):
				for j in range(floori((minf(ay, by) - pad) / CELL), floori((maxf(ay, by) + pad) / CELL) + 1):
					var kk := key(i, j)
					if not _road_segs.has(kk):
						_road_segs[kk] = []
					_road_segs[kk].append([ax, ay, bx, by, hw])

## The road segments reaching into a cell.
func road_segments(cx: int, cy: int) -> Array:
	return _road_segs.get(key(cx, cy), [])

static func key(cx: int, cy: int) -> int:
	return cx * 100003 + cy

static func centre(o: Dictionary) -> Vector2:
	if o.has("p"):
		return Vector2(float(o["p"][0]), float(o["p"][1]))
	var b: Array = o.get("b", [0, 0, 0, 0])
	return Vector2((float(b[0]) + float(b[2])) / 2.0, (float(b[1]) + float(b[3])) / 2.0)

## Objects whose position (or footprint centre) is in this cell.
func in_cell(cx: int, cy: int) -> Array:
	var out := []
	for i in _cells.get(key(cx, cy), []):
		out.append(objects[i])
	out.append_array(_points.get(key(cx, cy), []))
	return out

## A tile's point objects, as /play/world/tiles sends them (replacing any it had). Returns the cells to redraw.
func set_points(tx: int, ty: int, list: Array) -> Array:
	var cells := _tile_cells(tx, ty)
	for k in cells:
		_points.erase(k)
	for o in list:
		var at := centre(o)
		var k := key(int(floor(at.x / CELL)), int(floor(at.y / CELL)))
		if not _points.has(k):
			_points[k] = []
		_points[k].append(o)
	_point_tiles[key(tx, ty)] = true
	return cells

## Let go of a tile's point objects. Returns the cells to redraw.
func drop_points(tx: int, ty: int) -> Array:
	var cells := _tile_cells(tx, ty)
	for k in cells:
		_points.erase(k)
	_point_tiles.erase(key(tx, ty))
	return cells

## How far apart the heights are kept, in world units.
func height_cell() -> float:
	return _hcell

func has_points(tx: int, ty: int) -> bool:
	return _point_tiles.has(key(tx, ty))

## Whether a cell's point objects are all here: always, for a world read whole; once its tile is in, when streamed.
func points_in(cx: int, cy: int) -> bool:
	if _point_tiles.is_empty() and _points.is_empty():
		return true
	return has_points(int(floor(cx * CELL / TILE)), int(floor(cy * CELL / TILE)))

## How many point objects are in.
func point_count() -> int:
	var n := 0
	for l in _points.values():
		n += l.size()
	return n

static func _tile_cells(tx: int, ty: int) -> Array:
	return [key(tx * 2, ty * 2), key(tx * 2 + 1, ty * 2), key(tx * 2, ty * 2 + 1), key(tx * 2 + 1, ty * 2 + 1)]

func cells_in_use() -> Array:
	var all := _cells.duplicate()
	all.merge(_points)
	return all.keys()

## Ground height, interpolated between samples; 0 where there are no heights.
func height(x: float, y: float) -> float:
	if not has_heights:
		return 0.0
	var fx := clampf((x - _hx0) / _hcell, 0.0, float(_hw - 1))
	var fy := clampf((y - _hy0) / _hcell, 0.0, float(_hh - 1))
	var i := mini(int(fx), _hw - 2)
	var j := mini(int(fy), _hh - 2)
	var tx := fx - i
	var ty := fy - j
	var a := _heights[j * _hw + i]
	var b := _heights[j * _hw + i + 1]
	var c := _heights[(j + 1) * _hw + i]
	var d := _heights[(j + 1) * _hw + i + 1]
	return lerpf(lerpf(a, b, tx), lerpf(c, d, tx), ty)

## The area the heights cover (the world plus whatever margin the generator gave it).
func extent() -> Rect2:
	if not has_heights:
		return Rect2(0, 0, size, size)
	return Rect2(_hx0, _hy0, _hcell * (_hw - 1), _hcell * (_hh - 1))

## True on or right beside water, where scenery would look wrong.
func wet(x: float, y: float) -> bool:
	for r in rivers:
		var pts: Array = r["pts"]
		var reach := float(r.get("half_width", 5.0)) + 3.0
		for k in pts.size() - 1:
			var a := Vector2(pts[k][0], pts[k][1])
			var b := Vector2(pts[k + 1][0], pts[k + 1][1])
			if Geometry2D.get_closest_point_to_segment(Vector2(x, y), a, b).distance_to(Vector2(x, y)) < reach:
				return true
	return false

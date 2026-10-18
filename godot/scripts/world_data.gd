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
var has_heights := false
var _hx0 := 0.0
var _hy0 := 0.0
var _hcell := 1.0
var _hw := 0
var _hh := 0
var _heights := PackedFloat32Array()
var _cells := {}                 # cell key -> indices of objects whose position (or footprint centre) is in it

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

func _index() -> void:
	_cells.clear()
	for i in objects.size():
		var o: Dictionary = objects[i]
		var at := centre(o)
		var k := key(int(floor(at.x / CELL)), int(floor(at.y / CELL)))
		if not _cells.has(k):
			_cells[k] = []
		_cells[k].append(i)

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
	return out

func cells_in_use() -> Array:
	return _cells.keys()

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

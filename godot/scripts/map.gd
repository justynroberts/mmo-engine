extends Control
## The world map (M): the theme's picture of the world, with you and what is around you drawn on top each frame
## while it is open. The wheel zooms (towards the pointer), dragging pans, a double click shows it all again; names
## of smaller places come in as it zooms.

var built := false
var tex: ImageTexture
var world := 1024.0
var me := Vector2.ZERO
var facing := 0.0
var others: Array = []
## Named places ([{title, x, y, r}]) and marks ([[Vector2, "!" | "?" | "trade"], ...]: a quest to take, one to hand in,
## a trader; or [Vector2, "dungeon", label]) drawn over the picture.
var places: Array = []
var marks: Array = []
## Fog of war (explored.gd): laid over the picture, and places and marks in it left unnamed.
var explored: RefCounted

const DUNGEON := Color("e07a5f")
const GOAL := Color("ffe08a")
const YOU := Color("3ff0ff")    # nothing else on the map is this colour, so you are found at a glance

## Where a quest wants you: a bright ring with a dot (the minimap draws it too).
static func draw_goal(on: CanvasItem, at: Vector2, s: float) -> void:
	on.draw_circle(at, s + 1.5, Color(0, 0, 0, 0.75))
	on.draw_arc(at, s, 0.0, TAU, 24, GOAL, 2.0, true)
	on.draw_circle(at, s * 0.35, GOAL)

## A dungeon's mark: a dark red diamond with crossed blades (the minimap draws it too).
static func draw_dungeon(on: CanvasItem, at: Vector2, s: float) -> void:
	var halo := PackedVector2Array([at + Vector2(0, -s - 2.5), at + Vector2(s + 2.5, 0), at + Vector2(0, s + 2.5), at + Vector2(-s - 2.5, 0)])
	on.draw_colored_polygon(halo, Color(0, 0, 0, 0.8))
	var d := PackedVector2Array([at + Vector2(0, -s), at + Vector2(s, 0), at + Vector2(0, s), at + Vector2(-s, 0)])
	on.draw_colored_polygon(d, Color("5a1a14"))
	d.append(d[0])
	on.draw_polyline(d, DUNGEON, 1.5)
	var k := s * 0.5
	on.draw_line(at + Vector2(-k, -k), at + Vector2(k, k), Color("f3ead2"), 1.5)
	on.draw_line(at + Vector2(k, -k), at + Vector2(-k, k), Color("f3ead2"), 1.5)

const INK := Color("f3ead2")
const MUTED := Color(0.95, 0.92, 0.82, 0.55)
const WATER_INK := Color("bfe3f5")
const FRAME := Color("8a7448")
const DOCK := Color("8fc3e8")
const TRADE := Color("c9a227")
const OTHERS := Color("ffd27a")

var img: Image
## The part of the world drawn (world units), square: `home` is the land and a little sea round it, `view` that
## zoomed and panned.
var home := Rect2()
var view := Rect2()
var _map_rect := Rect2()             # where the picture is on screen, from the last draw
var _dragging := false
const MAX_ZOOM := 12.0
## Close up (from DETAIL_ZOOM), the part in view is drawn again sharply by `detail_source` (the theme's map_area:
## area, pixels, spread), a moment after the view stops moving, and laid over the picture.
const DETAIL_ZOOM := 2.0
var detail_source: Callable
var _detail_tex: ImageTexture
var _detail_rect := Rect2()
var _detail_asked := 0
var _detail_due := -1.0

func _ready() -> void:
	set_anchors_preset(Control.PRESET_FULL_RECT)
	mouse_filter = Control.MOUSE_FILTER_STOP # the wheel and drags are the map's while it is open
	texture_filter = CanvasItem.TEXTURE_FILTER_LINEAR

## The picture comes from the theme (theme.map_image); this only frames it and draws the markers.
func build(picture: Image, size: float) -> void:
	world = size
	img = picture
	tex = ImageTexture.create_from_image(picture)
	home = _land_view()
	view = home
	built = true

func _gui_input(ev: InputEvent) -> void:
	if not _map_rect.has_area():
		return
	if ev is InputEventMouseButton and ev.pressed and ev.button_index in [MOUSE_BUTTON_WHEEL_UP, MOUSE_BUTTON_WHEEL_DOWN]:
		_zoom(1.25 if ev.button_index == MOUSE_BUTTON_WHEEL_UP else 0.8, ev.position)
		accept_event()
	elif ev is InputEventMouseButton and ev.button_index == MOUSE_BUTTON_LEFT:
		if ev.double_click:
			view = home
			queue_redraw()
		_dragging = ev.pressed and _map_rect.has_point(ev.position)
		accept_event()
	elif ev is InputEventMouseMotion and _dragging:
		_pan(-ev.relative / _map_rect.size.x * view.size.x)
		accept_event()
	elif ev is InputEventMagnifyGesture:
		_zoom(ev.factor, ev.position)
		accept_event()
	elif ev is InputEventPanGesture:
		_pan(ev.delta * 8.0 / _map_rect.size.x * view.size.x)
		accept_event()

## Zoom by `factor` (over 1 is closer), keeping the world point under `screen` where it is.
func _zoom(factor: float, screen: Vector2) -> void:
	var side := clampf(view.size.x / factor, home.size.x / MAX_ZOOM, home.size.x)
	var f := ((screen - _map_rect.position) / _map_rect.size).clamp(Vector2.ZERO, Vector2.ONE)
	var under := view.position + f * view.size.x
	view = Rect2(under - f * side, Vector2(side, side))
	_pan(Vector2.ZERO)

## Zoom `times` in on the world point `at`.
func zoom_on(at: Vector2, times: float) -> void:
	var side := clampf(home.size.x / times, home.size.x / MAX_ZOOM, home.size.x)
	view = Rect2(at - Vector2(side, side) * 0.5, Vector2(side, side))
	_pan(Vector2.ZERO)

## Move the view by `by` world units, kept inside the home square.
func _pan(by: Vector2) -> void:
	var hi := home.position + home.size - view.size
	view.position = (view.position + by).clamp(home.position, hi)
	_detail_due = 0.35
	queue_redraw()

func _process(delta: float) -> void:
	if _detail_due < 0.0 or not visible:
		return
	_detail_due -= delta
	if _detail_due < 0.0 and home.size.x / view.size.x >= DETAIL_ZOOM and detail_source.is_valid():
		_ask_detail()

## Draw the view again close up; a later ask (the view moved on) wins over this one.
func _ask_detail() -> void:
	_detail_asked += 1
	var mine := _detail_asked
	var area := view
	var pic: Image = await detail_source.call(area, 512, true)
	if mine != _detail_asked or pic == null:
		return
	_detail_tex = ImageTexture.create_from_image(pic)
	_detail_rect = area
	queue_redraw()

## Whether a pixel of the picture is open water (bluer than it is green or red).
static func _wet(c: Color) -> bool:
	return c.b > c.g + 0.04 and c.b > c.r + 0.08

## The square round the land in the picture, with a margin of sea; the whole world if it is all land or all sea.
func _land_view() -> Rect2:
	var w := img.get_width()
	var h := img.get_height()
	var lo := Vector2(w, h)
	var hi := Vector2(-1, -1)
	var step := maxi(1, w / 128)
	for y in range(0, h, step):
		for x in range(0, w, step):
			if not _wet(img.get_pixel(x, y)):
				lo = Vector2(minf(lo.x, x), minf(lo.y, y))
				hi = Vector2(maxf(hi.x, x), maxf(hi.y, y))
	if hi.x < 0 or (hi - lo).x >= w * 0.9 and (hi - lo).y >= h * 0.9:
		return Rect2(0, 0, world, world)
	var k := world / float(w)
	var c := (lo + hi) * 0.5 * k
	var side := minf(world, maxf(hi.x - lo.x, hi.y - lo.y) * k * 1.12)
	var o := (c - Vector2(side, side) * 0.5).clamp(Vector2.ZERO, Vector2(world - side, world - side))
	return Rect2(o, Vector2(side, side))

func _is_water_at(p: Vector2) -> bool:
	if img == null:
		return false
	var x := clampi(int(p.x / world * img.get_width()), 0, img.get_width() - 1)
	var y := clampi(int(p.y / world * img.get_height()), 0, img.get_height() - 1)
	return _wet(img.get_pixel(x, y))

func update_markers(at: Vector2, yaw: float, around: Array) -> void:
	me = at
	facing = yaw
	others = around
	queue_redraw()

## A label centred on `at` (its baseline `dy` below); where that would overlap one already put down, it tries the
## other side of the point, then either side of it, and is left off if all four are taken.
func _label(font: Font, at: Vector2, dy: float, text: String, size: int, col: Color, taken: Array[Rect2], clear := 12.0) -> bool:
	if text == "":
		return false
	var w := font.get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, size).x
	var other := size + clear if dy <= 0.0 else -clear
	for o in [Vector2(-w / 2.0, dy), Vector2(-w / 2.0, other), Vector2(clear, size * 0.4), Vector2(-w - clear, size * 0.4),
			Vector2(clear * 0.6, -clear * 0.8), Vector2(clear * 0.6, size + clear * 0.8), Vector2(-w - clear * 0.6, -clear * 0.8), Vector2(-w - clear * 0.6, size + clear * 0.8)]:
		var box := Rect2(at + o + Vector2(-4, -size - 1), Vector2(w + 8, size + 6))
		if not _map_rect.encloses(box) or taken.any(func(r): return r.intersects(box)):
			continue
		taken.append(box)
		draw_string_outline(font, at + o, text, HORIZONTAL_ALIGNMENT_LEFT, -1, size, 4, Color(0, 0, 0, 0.8))
		draw_string(font, at + o, text, HORIZONTAL_ALIGNMENT_LEFT, -1, size, col)
		return true
	return false

func _draw() -> void:
	if tex == null:
		return
	var area := get_viewport_rect().size # this sits straight under a CanvasLayer, so take the screen's size
	var side := minf(area.x - 300.0, area.y - 90.0)
	var origin := Vector2(maxf(24.0, (area.x - side - 220.0) / 2.0), (area.y - side) / 2.0 + 14.0)
	draw_rect(Rect2(Vector2.ZERO, area), Color(0.03, 0.035, 0.03, 0.9))
	var px := Vector2(img.get_width(), img.get_height()) / world
	draw_texture_rect_region(tex, Rect2(origin, Vector2(side, side)), Rect2(view.position * px, view.size * px))
	_map_rect = Rect2(origin, Vector2(side, side))
	var zoom := home.size.x / view.size.x
	if _detail_tex != null and zoom >= DETAIL_ZOOM and _detail_rect.intersects(view):
		# The close-up, clipped to the view: the overlap's share of it, put where the overlap is on screen.
		var both := _detail_rect.intersection(view)
		var dpx := float(_detail_tex.get_width()) / _detail_rect.size.x
		var on := Rect2(origin + (both.position - view.position) / view.size.x * side, both.size / view.size.x * side)
		draw_texture_rect_region(_detail_tex, on, Rect2((both.position - _detail_rect.position) * dpx, both.size * dpx))
	if explored != null and explored.tex != null:
		var fpx: float = float(explored.GRID) / world
		draw_texture_rect_region(explored.tex, Rect2(origin, Vector2(side, side)), Rect2(view.position * fpx, view.size * fpx))
	draw_rect(Rect2(origin - Vector2(3, 3), Vector2(side + 6, side + 6)), Color(0, 0, 0, 0.6), false, 3.0)
	draw_rect(Rect2(origin - Vector2(1, 1), Vector2(side + 2, side + 2)), FRAME, false, 1.5)
	var to := func(p: Vector2) -> Vector2:
		return origin + (p - view.position) / view.size.x * side
	var font := get_theme_default_font()
	var hud := preload("res://scripts/hud.gd")
	var title_font: Font = hud.TITLE if hud.TITLE else font
	var taken: Array[Rect2] = []
	var inside := Rect2(origin, Vector2(side, side))
	# Icons first, so labels keep clear of them.
	var icon_at := func(m: Array) -> Vector2:
		return to.call(m[0])
	# Under the fog only what you would know without going there: dungeons (talked of) and where quests send you.
	var known := func(p: Vector2) -> bool:
		return explored == null or explored.seen(p) > 0.25
	var shown := marks.filter(func(m): return String(m[1]) in ["dungeon", "goal"] or known.call(m[0]))
	for m in shown:
		var at: Vector2 = icon_at.call(m)
		if not inside.has_point(at):
			continue
		match String(m[1]):
			"dock":
				draw_circle(at, 6.5, Color(0.05, 0.12, 0.18, 0.85))
				draw_arc(at + Vector2(0, 1), 3.5, 0.15 * PI, 0.85 * PI, 10, DOCK, 1.5, true)
				draw_line(at + Vector2(0, -4.5), at + Vector2(0, 4.5), DOCK, 1.5)
				draw_line(at + Vector2(-2.5, -2.5), at + Vector2(2.5, -2.5), DOCK, 1.5)
			"hold":
				var col := Color(String(m[3])) if m.size() > 3 and String(m[3]).begins_with("#") else Color("b8b0a0")
				draw_line(at + Vector2(0, 6), at + Vector2(0, -9), Color(0, 0, 0, 0.85), 2.0)
				draw_colored_polygon(PackedVector2Array([at + Vector2(1, -9), at + Vector2(10, -6), at + Vector2(1, -3)]), col)
			"goal":
				draw_goal(self, at, 6.0)
			"dungeon":
				draw_dungeon(self, at, 11.0)
			"trade":
				var d := PackedVector2Array([at + Vector2(0, -4), at + Vector2(3.5, 0), at + Vector2(0, 4), at + Vector2(-3.5, 0)])
				draw_colored_polygon(d, TRADE)
			_:
				draw_string_outline(title_font, at + Vector2(-4, 7), m[1], HORIZONTAL_ALIGNMENT_LEFT, -1, 18, 4, Color(0, 0, 0, 0.9))
				draw_string(title_font, at + Vector2(-4, 7), m[1], HORIZONTAL_ALIGNMENT_LEFT, -1, 18, OTHERS)
		var half := 13.0 if String(m[1]) == "dungeon" else 7.0
		taken.append(Rect2(at - Vector2(half, half + 2.0), Vector2(half * 2.0, half * 2.0 + 2.0)))
	for p in others:
		var op: Vector2 = to.call(p)
		if inside.has_point(op):
			draw_circle(op, 2.5, Color(0, 0, 0, 0.7))
			draw_circle(op, 1.8, OTHERS)
	# You: an arrow pointing the way you are going (the way you last went, standing still); at the edge, pointing
	# out, when the view is zoomed elsewhere.
	var c: Vector2 = (to.call(me) as Vector2).clamp(origin + Vector2(10, 10), origin + Vector2(side - 10, side - 10))
	var fwd := Vector2(-sin(facing), -cos(facing))
	var right := Vector2(fwd.y, -fwd.x)
	# Faint lines through you to the map's edges, a dark disc and a bright ring under a large arrow of your own colour.
	var line_col := Color(YOU.r, YOU.g, YOU.b, 0.35)
	draw_line(Vector2(origin.x, c.y), Vector2(c.x - 22.0, c.y), line_col, 1.0)
	draw_line(Vector2(c.x + 22.0, c.y), Vector2(origin.x + side, c.y), line_col, 1.0)
	draw_line(Vector2(c.x, origin.y), Vector2(c.x, c.y - 22.0), line_col, 1.0)
	draw_line(Vector2(c.x, c.y + 22.0), Vector2(c.x, origin.y + side), line_col, 1.0)
	draw_circle(c, 19.0, Color(0, 0, 0, 0.55))
	draw_arc(c, 18.0, 0.0, TAU, 40, YOU, 2.5, true)
	var arrow := PackedVector2Array([c + fwd * 14.0, c - fwd * 8.0 + right * 8.0, c - fwd * 4.0, c - fwd * 8.0 - right * 8.0])
	draw_colored_polygon(arrow, YOU)
	arrow.append(arrow[0])
	draw_polyline(arrow, Color("06181a"), 2.0)
	taken.append(Rect2(c - Vector2(20, 20), Vector2(40, 40)))
	# Labels, most wanted first: where a quest sends you, dungeons, then places by size; one that would overlap is
	# left off, and a name used twice is written once.
	for m in shown:
		if String(m[1]) == "goal" and m.size() > 2:
			_label(font, icon_at.call(m), -11.0, String(m[2]), 13, GOAL, taken)
	for m in shown:
		if String(m[1]) == "dungeon" and m.size() > 2:
			var lvl := String(m[2]).get_slice("level ", 1).get_slice(",", 0).get_slice(")", 0)
			var dl := String(m[2]).get_slice("  (", 0) + ((" · lvl " + lvl) if lvl != "" else "")
			# Crowded out with its level, try the name alone, smaller.
			if not _label(title_font, icon_at.call(m), 32.0, dl, 14, DUNGEON.lightened(0.25), taken, 18.0):
				_label(font, icon_at.call(m), 30.0, String(m[2]).get_slice("  (", 0), 12, DUNGEON.lightened(0.25), taken, 16.0)
	var named := places.filter(func(l): return String(l.get("title", "")) != "" and float(l.get("r", 0.0)) * zoom >= 30.0)
	named.sort_custom(func(a, b): return float(a.get("r", 0.0)) > float(b.get("r", 0.0)))
	var seen := {}
	for l in named:
		var t := String(l["title"])
		if seen.has(t):
			continue
		seen[t] = true
		var pr := float(l.get("r", 20.0))
		var wp := Vector2(float(l["x"]), float(l["y"]))
		var at: Vector2 = to.call(wp)
		if not inside.has_point(at) or not known.call(wp):
			continue
		if _is_water_at(wp):
			_label(font, at, 4.0, t, 11, WATER_INK, taken)
		else:
			_label(title_font, at, -4.0, t, 15 if pr >= 100.0 else 13, INK, taken)
	for m in shown:
		if String(m[1]) == "hold" and m.size() > 2:
			var ht := String(m[2]).get_slice(" (", 0)
			if not seen.has(ht):
				_label(font, icon_at.call(m), 16.0, ht, 11, MUTED, taken)
	# Header and a compass.
	draw_string_outline(title_font, origin + Vector2(0, -12), "World map", HORIZONTAL_ALIGNMENT_LEFT, -1, 22, 4, Color(0, 0, 0, 0.8))
	draw_string(title_font, origin + Vector2(0, -12), "World map", HORIZONTAL_ALIGNMENT_LEFT, -1, 22, INK)
	var coords := "%d, %d" % [int(me.x), int(me.y)]
	var cw := font.get_string_size(coords, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x
	draw_string(font, origin + Vector2(side - cw, -12), coords, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, MUTED)
	var n := origin + Vector2(side - 26, 34)
	draw_colored_polygon(PackedVector2Array([n + Vector2(0, -14), n + Vector2(5, 0), n + Vector2(-5, 0)]), INK)
	draw_colored_polygon(PackedVector2Array([n + Vector2(0, 14), n + Vector2(5, 0), n + Vector2(-5, 0)]), Color(0.95, 0.92, 0.82, 0.35))
	draw_string(title_font, n + Vector2(-5, -18), "N", HORIZONTAL_ALIGNMENT_LEFT, -1, 14, INK)
	# The key, beside the map: only what is on it.
	var kinds := {}
	for m in shown:
		kinds[String(m[1])] = true
	var key := [["you", YOU, true]]
	if explored != null:
		key.append(["not yet explored", Color(0.25, 0.23, 0.2), true])
	if not others.is_empty():
		key.append(["others near", OTHERS, true])
	for k in [["!", "a quest to take", OTHERS], ["?", "a quest to hand in", OTHERS], ["goal", "where a quest sends you", GOAL], ["dungeon", "dungeon", DUNGEON], ["trade", "trader", TRADE], ["hold", "hold (faction war)", Color("b8b0a0")], ["dock", "boats: F at a dock", DOCK]]:
		if kinds.has(k[0]):
			key.append([k[1], k[2], true])
	var kx := origin.x + side + 28.0
	var ky := origin.y + 8.0
	draw_string(title_font, Vector2(kx, ky + 6), "Key", HORIZONTAL_ALIGNMENT_LEFT, -1, 16, INK)
	ky += 30.0
	for k in key:
		draw_circle(Vector2(kx + 5, ky - 4), 4.5, k[1])
		draw_string(font, Vector2(kx + 18, ky), k[0], HORIZONTAL_ALIGNMENT_LEFT, -1, 13, INK)
		ky += 22.0
	var help := ["Wheel to zoom (%.1fx)" % zoom, "Drag to move", "Double click: all of it", "M or Esc to close"]
	for i in help.size():
		draw_string(font, Vector2(kx, origin.y + side - (help.size() - 1 - i) * 18.0), help[i], HORIZONTAL_ALIGNMENT_LEFT, -1, 12, MUTED)

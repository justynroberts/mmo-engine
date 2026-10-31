extends Control
## The world map (M): the theme's picture of the world, with you and what is around you drawn on top each frame
## while it is open.

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

const DUNGEON := Color("e07a5f")
const GOAL := Color("ffe08a")

## Where a quest wants you: a bright ring with a dot (the minimap draws it too).
static func draw_goal(on: CanvasItem, at: Vector2, s: float) -> void:
	on.draw_circle(at, s + 1.5, Color(0, 0, 0, 0.75))
	on.draw_arc(at, s, 0.0, TAU, 24, GOAL, 2.0, true)
	on.draw_circle(at, s * 0.35, GOAL)

## A dungeon's mark: a dark red diamond with crossed blades (the minimap draws it too).
static func draw_dungeon(on: CanvasItem, at: Vector2, s: float) -> void:
	var d := PackedVector2Array([at + Vector2(0, -s), at + Vector2(s, 0), at + Vector2(0, s), at + Vector2(-s, 0)])
	on.draw_colored_polygon(d, Color("5a1a14"))
	d.append(d[0])
	on.draw_polyline(d, DUNGEON, 1.5)
	var k := s * 0.5
	on.draw_line(at + Vector2(-k, -k), at + Vector2(k, k), Color("f3ead2"), 1.5)
	on.draw_line(at + Vector2(k, -k), at + Vector2(-k, k), Color("f3ead2"), 1.5)

func _ready() -> void:
	set_anchors_preset(Control.PRESET_FULL_RECT)
	mouse_filter = Control.MOUSE_FILTER_IGNORE

## The picture comes from the theme (theme.map_image); this only frames it and draws the markers.
func build(img: Image, size: float) -> void:
	world = size
	tex = ImageTexture.create_from_image(img)
	built = true

func update_markers(at: Vector2, yaw: float, around: Array) -> void:
	me = at
	facing = yaw
	others = around
	queue_redraw()

func _draw() -> void:
	if tex == null:
		return
	var area := get_viewport_rect().size # this sits straight under a CanvasLayer, so take the screen's size
	var side := minf(area.x, area.y) - 120.0
	var origin := Vector2((area.x - side) / 2.0, (area.y - side) / 2.0)
	draw_rect(Rect2(Vector2.ZERO, area), Color(0.04, 0.05, 0.04, 0.72))
	draw_texture_rect(tex, Rect2(origin, Vector2(side, side)), false)
	draw_rect(Rect2(origin, Vector2(side, side)), Color("2a2620"), false, 2.0)
	var to := func(p: Vector2) -> Vector2:
		return origin + p / world * side
	for p in others:
		draw_circle(to.call(p), 2.5, Color("ffd27a"))
	# You: an arrow pointing the way you are going (the way you last went, standing still).
	var c: Vector2 = to.call(me)
	var fwd := Vector2(-sin(facing), -cos(facing))
	var right := Vector2(fwd.y, -fwd.x)
	draw_colored_polygon(PackedVector2Array([c + fwd * 11.0, c - fwd * 6.0 + right * 6.0, c - fwd * 3.0, c - fwd * 6.0 - right * 6.0]), Color("f3ead2"))
	draw_polyline(PackedVector2Array([c + fwd * 11.0, c - fwd * 6.0 + right * 6.0, c - fwd * 3.0, c - fwd * 6.0 - right * 6.0, c + fwd * 11.0]), Color("1a1a12"), 1.5)
	var font := get_theme_default_font()
	var title_font: Font = preload("res://scripts/hud.gd").TITLE if preload("res://scripts/hud.gd").TITLE else font
	# Place names, the biggest first; a name that would overlap one already drawn is left off.
	var named := places.filter(func(l): return String(l.get("title", "")) != "" and float(l.get("r", 0.0)) >= 30.0)
	named.sort_custom(func(a, b): return float(a.get("r", 0.0)) > float(b.get("r", 0.0)))
	var taken: Array[Rect2] = []
	for l in named:
		var t := String(l["title"])
		var pr := float(l.get("r", 20.0))
		var sz := 16 if pr >= 100.0 else (13 if pr >= 60.0 else 11)
		var at: Vector2 = to.call(Vector2(float(l["x"]), float(l["y"])))
		var w := title_font.get_string_size(t, HORIZONTAL_ALIGNMENT_LEFT, -1, sz).x
		var box := Rect2(at + Vector2(-w / 2.0 - 3, -6 - sz), Vector2(w + 6, sz + 6))
		if taken.any(func(r): return r.intersects(box)):
			continue
		taken.append(box)
		draw_string_outline(title_font, at + Vector2(-w / 2.0, -6), t, HORIZONTAL_ALIGNMENT_LEFT, -1, sz, 4, Color(0, 0, 0, 0.85))
		draw_string(title_font, at + Vector2(-w / 2.0, -6), t, HORIZONTAL_ALIGNMENT_LEFT, -1, sz, Color("f3ead2"))
	# Quests to take (!), to hand in (?), and traders (a small gold diamond).
	for m in marks:
		var at: Vector2 = to.call(m[0])
		if m[1] == "dock":
			# An anchor: a ring, a shank and a crossbar.
			var ac := Color("8fc3e8")
			draw_arc(at + Vector2(0, 2), 5.0, 0.15 * PI, 0.85 * PI, 12, ac, 2.0, true)
			draw_line(at + Vector2(0, -7), at + Vector2(0, 7), ac, 2.0)
			draw_line(at + Vector2(-4, -4), at + Vector2(4, -4), ac, 2.0)
		elif m[1] == "hold":
			var col := Color(String(m[3])) if m.size() > 3 and String(m[3]).begins_with("#") else Color("b8b0a0")
			draw_line(at + Vector2(0, 8), at + Vector2(0, -12), Color("2a2620"), 2.0)
			draw_colored_polygon(PackedVector2Array([at + Vector2(1, -12), at + Vector2(13, -8), at + Vector2(1, -4)]), col)
			var hl: String = m[2]
			var hw := font.get_string_size(hl, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x
			draw_string_outline(font, at + Vector2(-hw / 2.0, 22), hl, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, 4, Color(0, 0, 0, 0.9))
			draw_string(font, at + Vector2(-hw / 2.0, 22), hl, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, col.lightened(0.3))
		elif m[1] == "goal":
			draw_goal(self, at, 7.0)
			var gl: String = m[2] if m.size() > 2 else ""
			var gw := font.get_string_size(gl, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x
			draw_string_outline(font, at + Vector2(-gw / 2.0, -12), gl, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, 4, Color(0, 0, 0, 0.9))
			draw_string(font, at + Vector2(-gw / 2.0, -12), gl, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, GOAL)
		elif m[1] == "dungeon":
			draw_dungeon(self, at, 8.0)
			var label: String = m[2] if m.size() > 2 else ""
			var w := font.get_string_size(label, HORIZONTAL_ALIGNMENT_LEFT, -1, 13).x
			draw_string_outline(font, at + Vector2(-w / 2.0, 22), label, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, 4, Color(0, 0, 0, 0.9))
			draw_string(font, at + Vector2(-w / 2.0, 22), label, HORIZONTAL_ALIGNMENT_LEFT, -1, 13, DUNGEON)
		elif m[1] == "trade":
			draw_colored_polygon(PackedVector2Array([at + Vector2(0, -6), at + Vector2(5, 0), at + Vector2(0, 6), at + Vector2(-5, 0)]), Color("c9a227"))
			draw_polyline(PackedVector2Array([at + Vector2(0, -6), at + Vector2(5, 0), at + Vector2(0, 6), at + Vector2(-5, 0), at + Vector2(0, -6)]), Color(0, 0, 0, 0.8), 1.0)
		else:
			draw_string_outline(title_font, at + Vector2(-5, 8), m[1], HORIZONTAL_ALIGNMENT_LEFT, -1, 22, 5, Color(0, 0, 0, 0.9))
			draw_string(title_font, at + Vector2(-5, 8), m[1], HORIZONTAL_ALIGNMENT_LEFT, -1, 22, Color("ffd27a"))
	draw_string(font, origin + Vector2(0, -12), "WORLD MAP   you are at %d, %d   M or Esc to close" % [int(me.x), int(me.y)], HORIZONTAL_ALIGNMENT_LEFT, -1, 16, Color("f3ead2"))
	var legend := [["you", Color("f3ead2")], ["others nearby", Color("ffd27a")], ["! a quest   ? hand one in", Color("ffd27a")], ["trader", Color("c9a227")], ["dungeon", DUNGEON], ["quest goal", GOAL], ["hold (faction war)", Color("b8b0a0")], ["boats (F at a dock)", Color("8fc3e8")]]
	var lx := origin.x
	for l in legend:
		draw_rect(Rect2(Vector2(lx, origin.y + side + 14), Vector2(12, 12)), l[1])
		draw_string(font, Vector2(lx + 18, origin.y + side + 25), l[0], HORIZONTAL_ALIGNMENT_LEFT, -1, 14, Color("f3ead2"))
		lx += 30 + font.get_string_size(l[0], HORIZONTAL_ALIGNMENT_LEFT, -1, 14).x

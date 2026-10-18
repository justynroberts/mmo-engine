extends Control
## The world map (M). The picture (ground, rivers, lakes, crags, keeps, houses) is drawn once into a texture;
## you and what is around you are drawn on top each frame while it is open.

var built := false
var tex: ImageTexture
var world := 1024.0
var me := Vector2.ZERO
var facing := 0.0
var others: Array = []
const PX := 640 # texture size

func _ready() -> void:
	set_anchors_preset(Control.PRESET_FULL_RECT)
	mouse_filter = Control.MOUSE_FILTER_IGNORE

func build(land, boxes: Array, kinds: Array, size: float) -> void:
	world = size
	var img := Image.create(PX, PX, false, Image.FORMAT_RGB8)
	var step := size / PX
	# Ground: the meadow green with the hills shaded from the north west, as on the console's map.
	for j in PX:
		for i in PX:
			var x := (i + 0.5) * step
			var y := (j + 0.5) * step
			var e := maxf(step, 2.0)
			var slope: float = 15.0 * (land.hills.get_noise_2d(x - e, y - e) - land.hills.get_noise_2d(x + e, y + e)) / (2.0 * e) # hills only, not the small bumps
			var lit := 0.9 * (1.0 + clampf(slope * 1.2, -0.25, 0.25))
			img.set_pixel(i, j, Color(0.36 * lit, 0.54 * lit, 0.2 * lit))
	var px := func(x: float, y: float) -> Vector2i:
		return Vector2i(clampi(int(x / step), 0, PX - 1), clampi(int(y / step), 0, PX - 1))
	# Rivers: a few pixels wide along each course.
	for r in land.rivers:
		var pts: PackedVector2Array = r["pts"]
		for k in pts.size() - 1:
			var a := pts[k]
			var b := pts[k + 1]
			for t in 6:
				var p: Vector2i = px.call(lerpf(a.x, b.x, t / 6.0), lerpf(a.y, b.y, t / 6.0))
				img.fill_rect(Rect2i(p - Vector2i(1, 1), Vector2i(3, 3)), Color("2f7f8f"))
	var colours := {"keep": Color("8d867d"), "house": Color("b5523b"), "crag": Color("6e675e"), "lake": Color("2f7f8f")}
	for i in boxes.size():
		var b: Array = boxes[i]
		var p0: Vector2i = px.call(b[0], b[1])
		var p1: Vector2i = px.call(b[2], b[3])
		var r := Rect2i(p0, (p1 - p0).max(Vector2i(1, 1)))
		img.fill_rect(r, colours.get(kinds[i], Color.GRAY))
		if kinds[i] != "lake" and r.size.x > 3 and r.size.y > 3:
			img.fill_rect(Rect2i(r.position, Vector2i(r.size.x, 1)), Color("2a2620"))
			img.fill_rect(Rect2i(r.position + Vector2i(0, r.size.y - 1), Vector2i(r.size.x, 1)), Color("2a2620"))
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
	# You: an arrow pointing the way the camera faces (forward is away from the camera).
	var c: Vector2 = to.call(me)
	var fwd := Vector2(-sin(facing), -cos(facing))
	var right := Vector2(fwd.y, -fwd.x)
	draw_colored_polygon(PackedVector2Array([c + fwd * 11.0, c - fwd * 6.0 + right * 6.0, c - fwd * 3.0, c - fwd * 6.0 - right * 6.0]), Color("f3ead2"))
	draw_polyline(PackedVector2Array([c + fwd * 11.0, c - fwd * 6.0 + right * 6.0, c - fwd * 3.0, c - fwd * 6.0 - right * 6.0, c + fwd * 11.0]), Color("1a1a12"), 1.5)
	var font := get_theme_default_font()
	draw_string(font, origin + Vector2(0, -12), "WORLD MAP   you are at %d, %d   M or Esc to close" % [int(me.x), int(me.y)], HORIZONTAL_ALIGNMENT_LEFT, -1, 16, Color("f3ead2"))
	var legend := [["houses", Color("b5523b")], ["keeps", Color("8d867d")], ["crags", Color("6e675e")], ["water", Color("2f7f8f")], ["others nearby", Color("ffd27a")]]
	var lx := origin.x
	for l in legend:
		draw_rect(Rect2(Vector2(lx, origin.y + side + 14), Vector2(12, 12)), l[1])
		draw_string(font, Vector2(lx + 18, origin.y + side + 25), l[0], HORIZONTAL_ALIGNMENT_LEFT, -1, 14, Color("f3ead2"))
		lx += 30 + font.get_string_size(l[0], HORIZONTAL_ALIGNMENT_LEFT, -1, 14).x

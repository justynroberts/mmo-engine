extends CanvasLayer
## Speech bubbles: what someone says, in a rounded bubble over their head with a tail pointing down at them. Drawn
## on screen (crisp at any distance) and kept over the speaker as they and the camera move; one per speaker, a new
## line replacing the last. They fade in, stay as long as the line takes to read, and fade out, smaller and
## fainter with distance, hidden behind the camera or out past `FAR`.

const HUD := preload("res://scripts/hud.gd")

const FAR := 45.0            # beyond this (world units) a bubble is not drawn
const NEAR := 8.0            # nearer than this it is drawn at full size
const ABOVE := 3.35          # how far over the speaker's feet the tail points
const WIDTH := 230.0         # longest line before the text wraps, in pixels
const PAPER := Color("f3e9d2")
const INK := Color("2a2016")
const EDGE := Color("8a6f4d")
const SHOUT := Color("fff3c4")

var cam: Camera3D
var _live := {}              # speaker id -> {node, panel, tail, until, born, root}

func _ready() -> void:
	layer = 4 # over the world, under the HUD's panels

## `who` says `text` (an NPC's remark, a player's line). A shout is bolder and stays a little longer.
func say(id: int, who: Node3D, text: String, shout := false) -> void:
	if who == null or text.strip_edges() == "":
		return
	drop(id)
	var root := Control.new()
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var panel := PanelContainer.new()
	panel.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var box := StyleBoxFlat.new()
	box.bg_color = SHOUT if shout else PAPER
	box.border_color = EDGE
	box.set_border_width_all(2 if shout else 1)
	box.set_corner_radius_all(12)
	box.content_margin_left = 11
	box.content_margin_right = 11
	box.content_margin_top = 6
	box.content_margin_bottom = 7
	box.shadow_color = Color(0, 0, 0, 0.28)
	box.shadow_size = 4
	box.shadow_offset = Vector2(0, 2)
	panel.add_theme_stylebox_override("panel", box)
	var label := Label.new()
	label.text = text
	label.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	label.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	label.add_theme_color_override("font_color", INK)
	if HUD.BODY:
		label.add_theme_font_override("font", HUD.BODY)
	label.add_theme_font_size_override("font_size", 17 if shout else 16)
	# Wrap only long lines: a short one keeps its own width.
	var w := label.get_theme_font("font").get_string_size(text, HORIZONTAL_ALIGNMENT_LEFT, -1, label.get_theme_font_size("font_size")).x
	label.custom_minimum_size.x = minf(w + 2.0, WIDTH)
	panel.add_child(label)
	root.add_child(panel)
	var tail := Polygon2D.new()
	tail.color = box.bg_color
	tail.polygon = PackedVector2Array([Vector2(-8, -1), Vector2(8, -1), Vector2(0, 11)])
	var tail_edge := Line2D.new()
	tail_edge.points = PackedVector2Array([Vector2(-8, 0), Vector2(0, 11), Vector2(8, 0)])
	tail_edge.width = 1.0
	tail_edge.default_color = EDGE
	tail.add_child(tail_edge)
	root.add_child(tail)
	root.modulate.a = 0.0
	add_child(root)
	# Long enough to read: about a fifth of a second a word, at least three seconds, at most nine.
	var secs := clampf(2.5 + text.length() * 0.06, 3.0, 9.0) + (1.0 if shout else 0.0)
	var now := Time.get_ticks_msec() / 1000.0
	_live[id] = {"node": who, "panel": panel, "tail": tail, "root": root, "until": now + secs, "born": now}

## Take down `id`'s bubble at once (a new line, or the speaker gone).
func drop(id: int) -> void:
	if _live.has(id):
		(_live[id]["root"] as Node).queue_free()
		_live.erase(id)

func _process(_delta: float) -> void:
	if cam == null:
		return
	var now := Time.get_ticks_msec() / 1000.0
	var order := []
	for id in _live.keys():
		var b: Dictionary = _live[id]
		var node = b["node"]
		var root: Control = b["root"]
		if not is_instance_valid(node) or now > float(b["until"]) + 0.6:
			drop(id)
			continue
		var at: Vector3 = (node as Node3D).global_position + Vector3(0, ABOVE, 0)
		var dist := cam.global_position.distance_to(at)
		if cam.is_position_behind(at) or dist > FAR or not (node as Node3D).is_visible_in_tree():
			root.visible = false
			continue
		root.visible = true
		# In over a fifth of a second, out over half a second, fainter far off.
		var fade_in := clampf((now - float(b["born"])) / 0.2, 0.0, 1.0)
		var fade_out := clampf((float(b["until"]) + 0.5 - now) / 0.5, 0.0, 1.0)
		var far := clampf(1.0 - (dist - NEAR * 2.0) / (FAR - NEAR * 2.0), 0.35, 1.0)
		root.modulate.a = fade_in * fade_out * far
		var s := clampf(NEAR / maxf(dist, 0.01), 0.62, 1.0) * (0.92 + 0.08 * fade_in)
		root.scale = Vector2(s, s)
		var p := cam.unproject_position(at)
		var panel: PanelContainer = b["panel"]
		var size := panel.get_combined_minimum_size()
		panel.position = Vector2(-size.x / 2.0, -size.y - 10.0)
		(b["tail"] as Polygon2D).position = Vector2(0, -10.0)
		root.position = p.round()
		order.append([dist, root])
	# The nearest on top.
	order.sort_custom(func(a, b): return a[0] > b[0])
	for i in order.size():
		move_child(order[i][1], i)

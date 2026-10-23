extends CanvasLayer
## The first screen of a packaged client: which server, and what to be called. Started without --host, the client asks
## here instead of assuming this machine. What was typed is kept in user://join.cfg for next time; the server field
## starts from res://default_server.txt (written by scripts/package_godot.sh) the first time.

signal chosen(server: String, name: String)

const HUD := preload("res://scripts/hud.gd")
const SAVED := "user://join.cfg"
## The title art, behind the join screen and (dimmed) the loading screen.
const ART := "res://ui/title.jpg"

var server_in := LineEdit.new()
var name_in := LineEdit.new()
var note := Label.new()

func _ready() -> void:
	layer = 10 # over the HUD
	HUD.use_fonts()
	var cfg := ConfigFile.new()
	cfg.load(SAVED)
	var fallback := ""
	if FileAccess.file_exists("res://default_server.txt"):
		fallback = FileAccess.get_file_as_string("res://default_server.txt").strip_edges()
	server_in.text = String(cfg.get_value("join", "server", fallback))
	name_in.text = String(cfg.get_value("join", "name", ""))

	var back := ColorRect.new()
	back.color = Color(0.03, 0.035, 0.04)
	back.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(back)
	# The title art fills the screen (cropped, never stretched); the form sits under its lettering, on a fade.
	var art := make_art()
	if art:
		back.add_child(art)
	back.add_child(fade_rect(0.42))

	var col := VBoxContainer.new()
	col.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE)
	col.grow_horizontal = Control.GROW_DIRECTION_BOTH
	col.grow_vertical = Control.GROW_DIRECTION_BEGIN
	col.offset_bottom = -36
	col.add_theme_constant_override("separation", 6)
	back.add_child(col)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	col.add_child(row)
	var where := VBoxContainer.new()
	where.add_child(_label("Server"))
	server_in.placeholder_text = "192.168.1.20"
	where.add_child(_field(server_in))
	server_in.custom_minimum_size = Vector2(240, 34)
	row.add_child(where)
	var who := VBoxContainer.new()
	who.add_child(_label("Your name"))
	name_in.placeholder_text = "Who you will be known as"
	name_in.max_length = 24
	who.add_child(_field(name_in))
	name_in.custom_minimum_size = Vector2(260, 34)
	row.add_child(who)
	var go := Button.new()
	go.text = "Enter the world"
	go.custom_minimum_size = Vector2(170, 34)
	go.size_flags_vertical = Control.SIZE_SHRINK_END
	go.add_theme_font_override("font", HUD.TITLE)
	go.add_theme_font_size_override("font_size", 17)
	go.pressed.connect(_go)
	for state in ["normal", "hover", "pressed", "focus"]:
		var box := StyleBoxFlat.new()
		box.bg_color = Color(0.05, 0.055, 0.065, 0.92 if state != "hover" else 0.98)
		box.border_color = HUD.ACCENT if state == "hover" else HUD.EDGE
		box.set_border_width_all(1)
		box.set_corner_radius_all(3)
		box.set_content_margin_all(6)
		go.add_theme_stylebox_override(state, box)
	go.add_theme_color_override("font_color", HUD.ACCENT)
	go.add_theme_color_override("font_hover_color", Color.WHITE)
	row.add_child(go)
	note.add_theme_color_override("font_color", HUD.ACCENT)
	note.add_theme_constant_override("outline_size", 4)
	note.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.9))
	note.autowrap_mode = TextServer.AUTOWRAP_WORD
	note.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	note.custom_minimum_size = Vector2(680, 0)
	col.add_child(note)
	server_in.text_submitted.connect(func(_t): name_in.grab_focus())
	name_in.text_submitted.connect(func(_t): _go())
	(name_in if server_in.text != "" else server_in).grab_focus.call_deferred()

## The title art, alive: the sun's rays turn and breathe, mist drifts through the valleys, the water glints, and
## the picture slowly pushes in. Null when the art is missing.
static func make_art() -> TextureRect:
	if not ResourceLoader.exists(ART):
		return null
	var art := TextureRect.new()
	art.texture = load(ART)
	art.set_anchors_preset(Control.PRESET_FULL_RECT)
	art.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	art.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT_COVERED
	art.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var sh := Shader.new()
	sh.code = ART_SHADER
	var m := ShaderMaterial.new()
	m.shader = sh
	art.material = m
	return art

## Where things are in the picture (texture UV): the sun, the band the mist lies in, and the water.
const ART_SHADER := """
shader_type canvas_item;
uniform vec2 sun = vec2(0.692, 0.165);
uniform float aspect = 1.777;

float hash(vec2 p) { return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453); }
float noise(vec2 p) {
	vec2 i = floor(p), f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) {
	float v = 0.0, a = 0.5;
	for (int k = 0; k < 5; k++) { v += a * noise(p); p *= 2.03; a *= 0.5; }
	return v;
}

void fragment() {
	// A slow push in and back, centred a little above the middle.
	float zoom = 1.0 + 0.035 * (0.5 - 0.5 * cos(TIME * 0.05));
	vec2 uv = (UV - vec2(0.5, 0.45)) / zoom + vec2(0.5, 0.45);
	vec4 c = texture(TEXTURE, uv);

	// Rays out of the sun: a fan of soft beams that slowly turns, each brightening and fading on its own.
	vec2 d = (uv - sun) * vec2(aspect, 1.0);
	float r = length(d);
	float ang = atan(d.y, d.x);
	float beams = 0.0;
	beams += pow(0.5 + 0.5 * sin(ang * 14.0 + TIME * 0.18), 6.0) * (0.6 + 0.4 * sin(TIME * 0.7 + ang * 3.0));
	beams += pow(0.5 + 0.5 * sin(ang * 23.0 - TIME * 0.11 + 1.3), 8.0) * (0.6 + 0.4 * sin(TIME * 0.45 - ang * 5.0));
	// They reach across the sky and fade out over the land, so the castle and the sea keep their own light.
	float reach = exp(-r * 3.2) * smoothstep(0.03, 0.14, r) * (1.0 - smoothstep(0.24, 0.42, uv.y));
	float glow = exp(-r * 10.0) * (0.85 + 0.15 * sin(TIME * 0.9));
	vec3 warm = vec3(1.0, 0.8, 0.5);
	c.rgb += warm * (beams * reach * 0.16 + glow * 0.14);

	// Mist: two layers drifting at different speeds through the middle distance, thinner high and low.
	float band = smoothstep(0.22, 0.38, uv.y) * (1.0 - smoothstep(0.52, 0.72, uv.y));
	float m1 = fbm(vec2(uv.x * 3.0 + TIME * 0.012, uv.y * 7.0));
	float m2 = fbm(vec2(uv.x * 5.0 - TIME * 0.02 + 9.0, uv.y * 11.0 + 3.0));
	float mist = smoothstep(0.45, 0.85, m1 * 0.6 + m2 * 0.5) * band;
	c.rgb = mix(c.rgb, vec3(0.92, 0.9, 0.86), mist * 0.28);

	// The water glints where it is bright already (the sea and the river under the sun).
	float lum = dot(c.rgb, vec3(0.3, 0.5, 0.2));
	float water = smoothstep(0.7, 0.95, lum) * step(0.25, uv.y) * step(uv.x, 0.98) * step(0.55, uv.x);
	float glint = pow(noise(vec2(uv.x * 160.0, uv.y * 60.0 + TIME * 0.6)), 8.0);
	c.rgb += vec3(1.0, 0.9, 0.7) * water * glint * 0.9;
	COLOR = c;
}
"""

## A fade from clear to near black over the lower part of the screen (from `from`, a share of the height down),
## so text there reads over the art.
static func fade_rect(from: float) -> TextureRect:
	var g := Gradient.new()
	g.set_color(0, Color(0.02, 0.022, 0.026, 0.0))
	g.set_color(1, Color(0.02, 0.022, 0.026, 0.92))
	var t := GradientTexture2D.new()
	t.gradient = g
	t.fill_from = Vector2(0, from)
	t.fill_to = Vector2(0, 1)
	t.width = 4
	t.height = 256
	var r := TextureRect.new()
	r.texture = t
	r.set_anchors_preset(Control.PRESET_FULL_RECT)
	r.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	r.stretch_mode = TextureRect.STRETCH_SCALE
	r.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return r

func _label(t: String) -> Label:
	var l := Label.new()
	l.text = t
	l.add_theme_color_override("font_color", HUD.TEXT)
	l.add_theme_constant_override("outline_size", 4)
	l.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.9))
	return l

func _field(e: LineEdit) -> LineEdit:
	e.custom_minimum_size = Vector2(0, 34)
	e.add_theme_font_size_override("font_size", 18)
	return e

func _go() -> void:
	var server := server_in.text.strip_edges().trim_prefix("http://").trim_suffix("/")
	var who := name_in.text.strip_edges()
	if server == "":
		note.text = "Which server? Ask whoever runs it for its address."
		return
	if who == "":
		note.text = "Pick a name."
		return
	var cfg := ConfigFile.new()
	cfg.set_value("join", "server", server)
	cfg.set_value("join", "name", who)
	cfg.save(SAVED)
	chosen.emit(server, who)
	queue_free()

## Shown again when the server could not be reached, with why.
func fail(msg: String) -> void:
	note.text = msg

extends CanvasLayer
## The first screen of a packaged client: which server, and what to be called. Started without --host, the client asks
## here instead of assuming this machine. What was typed is kept in user://join.cfg for next time; the server field
## starts from res://default_server.txt (written by scripts/package_godot.sh) the first time.

signal chosen(server: String, name: String)

const HUD := preload("res://scripts/hud.gd")
const SAVED := "user://join.cfg"
## The public server, when nothing else is saved or built in (a packaged app may carry its own in default_server.txt).
const DEFAULT_SERVER := "hibernian.apps.fintonlabs.com"
## The title art, behind the join screen and (dimmed) the loading screen.
const ART := "res://ui/title.jpg"

## The version this app was packaged as (scripts/package_godot.sh writes it); run from the project there is none,
## and it never updates itself.
const VERSION_FILE := "res://version.txt"
const UPDATE_ZIP := "user://update.zip"

var server_in := LineEdit.new()
var update_btn := Button.new()
var _update := {}          # what the server offers: {version, size, sha256}
var _dl: HTTPRequest       # the download, while it runs
var name_in := LineEdit.new()
var note := Label.new()

func _ready() -> void:
	layer = 10 # over the HUD
	HUD.use_fonts()
	var cfg := ConfigFile.new()
	cfg.load(SAVED)
	var fallback := DEFAULT_SERVER
	if FileAccess.file_exists("res://default_server.txt"):
		fallback = FileAccess.get_file_as_string("res://default_server.txt").strip_edges()
	server_in.text = String(cfg.get_value("join", "server", fallback))
	name_in.text = String(cfg.get_value("join", "name", ""))
	# The server remembered may be on a network we have left: if it does not answer, offer the built-in one when
	# that does.
	var other := fallback if fallback != "" else DEFAULT_SERVER
	if server_in.text != "" and server_in.text != other:
		_check_saved.call_deferred(server_in.text, other)

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
	server_in.placeholder_text = DEFAULT_SERVER
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
	update_btn.visible = false
	update_btn.custom_minimum_size = Vector2(260, 32)
	update_btn.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	update_btn.focus_mode = Control.FOCUS_NONE
	update_btn.pressed.connect(_start_update)
	col.add_child(update_btn)
	_check_update.call_deferred()
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
	// Heat haze: the air round the sun wavers, most near it.
	vec2 d0 = (uv - sun) * vec2(aspect, 1.0);
	float near_sun = exp(-length(d0) * 7.0);
	vec2 haze = vec2(noise(uv * 38.0 + vec2(0.0, TIME * 1.6)), noise(uv * 38.0 + vec2(5.2, TIME * 1.3))) - 0.5;
	uv += haze * 0.0045 * near_sun;
	vec4 c = texture(TEXTURE, uv);

	// Rays out of the sun: a fan of soft beams that slowly turns, each brightening and fading on its own.
	vec2 d = (uv - sun) * vec2(aspect, 1.0);
	float r = length(d);
	float ang = atan(d.y, d.x);
	float beams = 0.0;
	beams += pow(0.5 + 0.5 * sin(ang * 14.0 + TIME * 0.18), 6.0) * (0.6 + 0.4 * sin(TIME * 0.7 + ang * 3.0));
	beams += pow(0.5 + 0.5 * sin(ang * 23.0 - TIME * 0.11 + 1.3), 8.0) * (0.6 + 0.4 * sin(TIME * 0.45 - ang * 5.0));
	// A brighter beam sweeps slowly round through the others.
	float sweep = pow(0.5 + 0.5 * cos(ang - TIME * 0.13), 40.0) + pow(0.5 + 0.5 * cos(ang + 2.4 - TIME * 0.09), 60.0) * 0.7;
	// They reach across the sky and fade out over the land, so the castle and the sea keep their own light.
	float reach = exp(-r * 3.2) * smoothstep(0.03, 0.14, r) * (1.0 - smoothstep(0.24, 0.42, uv.y));
	// The sun shimmers: an uneven flicker on top of a slow breath, never the same twice.
	float flicker = 0.82 + 0.1 * sin(TIME * 0.9) + 0.08 * (noise(vec2(TIME * 3.1, 0.0)) - 0.5) * 2.0 + 0.05 * sin(TIME * 7.3 + noise(vec2(TIME, 4.0)) * 6.0);
	float glow = exp(-r * 10.0) * flicker;
	float core = exp(-r * 38.0) * (0.9 + 0.25 * noise(vec2(TIME * 5.0, 2.0)));
	// A corona of fine glints that twinkle as they turn.
	float fine = pow(0.5 + 0.5 * sin(ang * 61.0 + TIME * 0.6 + noise(vec2(ang * 9.0, TIME * 2.0)) * 3.0), 18.0);
	float twinkle = fine * smoothstep(0.015, 0.05, r) * exp(-r * 9.0) * (0.5 + 0.5 * noise(vec2(ang * 30.0, TIME * 4.0)));
	vec3 warm = vec3(1.0, 0.8, 0.5);
	c.rgb += warm * (beams * reach * 0.18 + sweep * reach * 0.14 + glow * 0.16 + twinkle * 0.35) + vec3(1.0, 0.95, 0.85) * core * 0.35;

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

func _answers(server: String) -> bool:
	var web := HUD.web_base(server)
	var r := HTTPRequest.new()
	r.timeout = 1.5
	add_child(r)
	if r.request("%s/play/info" % web) != OK:
		r.queue_free()
		return false
	var res: Array = await r.request_completed
	r.queue_free()
	return res[0] == HTTPRequest.RESULT_SUCCESS and res[1] == 200

func _check_saved(saved: String, other: String) -> void:
	if await _answers(saved):
		return
	if server_in.text == saved and await _answers(other):
		server_in.text = other
		note.text = "%s did not answer; %s does." % [saved, other]

# ---------------------------------------------------------------- updating

static func version() -> String:
	return FileAccess.get_file_as_string(VERSION_FILE).strip_edges() if FileAccess.file_exists(VERSION_FILE) else ""

func _web(server: String) -> String:
	return HUD.web_base(server)

## Ask the server for the newest client; offer it when it is newer than this one (versions are dates, so they
## sort as text). Only a packaged app updates itself.
func _check_update() -> void:
	await get_tree().create_timer(1.6).timeout # after the saved-server check has had its say
	var mine := version()
	if mine == "" or not OS.has_feature("template") or server_in.text.strip_edges() == "":
		return
	var r := HTTPRequest.new()
	r.timeout = 3.0
	add_child(r)
	if r.request("%s/play/client" % _web(server_in.text)) != OK:
		r.queue_free()
		return
	var res: Array = await r.request_completed
	r.queue_free()
	if res[0] != HTTPRequest.RESULT_SUCCESS or res[1] != 200:
		return
	var m = JSON.parse_string((res[3] as PackedByteArray).get_string_from_utf8())
	if not (m is Dictionary) or String(m.get("version", "")) <= mine:
		return
	_update = m
	update_btn.text = "Update to the newest version (%d MB)" % int(float(m.get("size", 0)) / 1048576.0)
	update_btn.visible = true
	note.text = "A newer version of the game is on this server."

func _start_update() -> void:
	if _dl != null or _update.is_empty():
		return
	update_btn.disabled = true
	_dl = HTTPRequest.new()
	_dl.download_file = UPDATE_ZIP
	_dl.body_size_limit = -1
	_dl.timeout = 600.0
	add_child(_dl)
	_dl.request_completed.connect(_downloaded)
	if _dl.request("%s/play/client/mac.zip" % _web(server_in.text)) != OK:
		_update_failed("could not start the download")

func _process(_delta: float) -> void:
	if _dl != null and _update.has("size"):
		var pct := 100.0 * float(_dl.get_downloaded_bytes()) / maxf(float(_update["size"]), 1.0)
		update_btn.text = "Downloading... %d%%" % int(pct)

func _downloaded(result: int, code: int, _h: PackedStringArray, _b: PackedByteArray) -> void:
	_dl.queue_free()
	_dl = null
	if result != HTTPRequest.RESULT_SUCCESS or code != 200:
		return _update_failed("the download did not finish")
	update_btn.text = "Checking..."
	await get_tree().process_frame
	if FileAccess.get_sha256(UPDATE_ZIP) != String(_update.get("sha256", "")):
		DirAccess.remove_absolute(ProjectSettings.globalize_path(UPDATE_ZIP))
		return _update_failed("the download was damaged")
	_install()

## Swap the app for the new one and start it: a small script waits for this process to end, unpacks the new app
## next to the old, moves it into place (putting the old one back if that fails) and opens it. An app macOS runs
## from a hidden copy (one never moved out of Downloads) cannot replace itself, so the new one goes to
## ~/Applications instead.
func _install() -> void:
	var app := OS.get_executable_path().get_base_dir().get_base_dir().get_base_dir()
	if not app.ends_with(".app"):
		return _update_failed("cannot find the app to replace")
	if app.contains("/AppTranslocation/"):
		app = OS.get_environment("HOME").path_join("Applications").path_join(app.get_file())
	var script := """#!/bin/sh
pid="$1"; zip="$2"; app="$3"
while kill -0 "$pid" 2>/dev/null; do sleep 0.2; done
tmp=$(mktemp -d) || exit 1
ditto -x -k "$zip" "$tmp" || { open "$app"; exit 1; }
new=$(find "$tmp" -maxdepth 1 -name '*.app' | head -1)
mkdir -p "$(dirname "$app")"
rm -rf "$app.old"
[ -e "$app" ] && mv "$app" "$app.old"
if mv "$new" "$app"; then rm -rf "$app.old"; else mv "$app.old" "$app"; fi
rm -rf "$tmp" "$zip"
open "$app"
"""
	var path := ProjectSettings.globalize_path("user://update.sh")
	var f := FileAccess.open(path, FileAccess.WRITE)
	if f == null:
		return _update_failed("cannot write the installer")
	f.store_string(script)
	f.close()
	var pid := OS.create_process("/bin/sh", [path, str(OS.get_process_id()), ProjectSettings.globalize_path(UPDATE_ZIP), app])
	if pid <= 0:
		return _update_failed("cannot start the installer")
	update_btn.text = "Restarting with the new version..."
	await get_tree().create_timer(0.4).timeout
	get_tree().quit()

func _update_failed(why: String) -> void:
	update_btn.disabled = false
	update_btn.text = "Try the update again"
	note.text = "Update failed: %s." % why

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
	var server := server_in.text.strip_edges().trim_suffix("/")
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

extends CanvasLayer
## Settings (from the Esc menu): how loud the music, the ambience and the effects are, how much the graphics do,
## and full screen. Kept in user://settings.cfg; main.gd applies them at start and as they change.

signal changed(s: Dictionary)

const HUD := preload("res://scripts/hud.gd")
const SAVED := "user://settings.cfg"
const DEFAULTS := {"music": 0.8, "ambience": 0.8, "effects": 0.9, "graphics": 2, "fullscreen": true}
const GRAPHICS := ["Low", "Medium", "High"]

var panel := PanelContainer.new()
var values := DEFAULTS.duplicate()

func _ready() -> void:
	layer = 8
	HUD.use_fonts()
	load_saved()
	panel.visible = false
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.97)
	b.border_color = HUD.EDGE
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(20)
	panel.add_theme_stylebox_override("panel", b)
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(420, 0)
	panel.position = Vector2(-210, -180)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 12)
	panel.add_child(v)
	var t := Label.new()
	t.text = "Settings"
	t.add_theme_font_override("font", HUD.TITLE)
	t.add_theme_font_size_override("font_size", 26)
	t.add_theme_color_override("font_color", HUD.ACCENT)
	v.add_child(t)
	for pair in [["music", "Music"], ["ambience", "Sounds of the world"], ["effects", "Effects"]]:
		var row := HBoxContainer.new()
		var l := Label.new()
		l.text = pair[1]
		l.custom_minimum_size = Vector2(170, 0)
		l.add_theme_color_override("font_color", HUD.TEXT)
		row.add_child(l)
		var s := HSlider.new()
		s.min_value = 0.0
		s.max_value = 1.0
		s.step = 0.05
		s.value = float(values[pair[0]])
		s.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		s.focus_mode = Control.FOCUS_NONE
		var key: String = pair[0]
		s.value_changed.connect(func(x: float): _store(key, x))
		row.add_child(s)
		v.add_child(row)
	var g := HBoxContainer.new()
	var gl := Label.new()
	gl.text = "Graphics"
	gl.custom_minimum_size = Vector2(170, 0)
	gl.add_theme_color_override("font_color", HUD.TEXT)
	g.add_child(gl)
	var opt := OptionButton.new()
	for name in GRAPHICS:
		opt.add_item(name)
	opt.selected = int(values["graphics"])
	opt.focus_mode = Control.FOCUS_NONE
	opt.item_selected.connect(func(i: int): _store("graphics", i))
	g.add_child(opt)
	v.add_child(g)
	var fs := CheckButton.new()
	fs.text = "Full screen"
	fs.button_pressed = bool(values["fullscreen"])
	fs.focus_mode = Control.FOCUS_NONE
	fs.toggled.connect(func(on: bool): _store("fullscreen", on))
	v.add_child(fs)
	var hint := Label.new()
	hint.text = "Low turns off shadows and smoothing and draws at a lower resolution, for slower machines. N still cycles sound on and off, F11 full screen."
	hint.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	hint.custom_minimum_size = Vector2(380, 0)
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	v.add_child(hint)
	var close := Button.new()
	close.text = "Done"
	close.focus_mode = Control.FOCUS_NONE
	close.custom_minimum_size = Vector2(120, 30)
	close.pressed.connect(func(): panel.visible = false)
	v.add_child(close)

## What was saved (with the defaults for anything new): the game starts full screen until told otherwise.
func load_saved() -> void:
	var cfg := ConfigFile.new()
	cfg.load(SAVED)
	for k in DEFAULTS:
		values[k] = cfg.get_value("settings", k, DEFAULTS[k])

func _store(key: String, v) -> void:
	values[key] = v
	var cfg := ConfigFile.new()
	for k in values:
		cfg.set_value("settings", k, values[k])
	cfg.save(SAVED)
	changed.emit(values)

func open() -> void:
	panel.visible = true

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

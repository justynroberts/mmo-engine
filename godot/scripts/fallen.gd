extends CanvasLayer
## When we fall: the world dims, a line says so, and a button (or Enter) asks the server to let us rise again in
## the nearest safe place. Until then we stay down where we fell, out of sight, and nothing can hurt us.

signal rise

const HUD := preload("res://scripts/hud.gd")
const LINES := [
	"The ground is cold, and the crows are patient.",
	"Even Cú Chulainn tied himself to a standing stone.",
	"The sídhe will not have you yet.",
	"Brave, certainly. Wise, the bards are still arguing.",
]

var _dim := ColorRect.new()
var _panel := VBoxContainer.new()
var _quip := Label.new()
var _button := Button.new()
var down := false
## Seconds the server makes us wait before rising (`rules.rise_delay_secs`), set by main from /play/info.
var wait_secs := 0.0
var _left := 0.0

func _ready() -> void:
	layer = 9
	HUD.use_fonts()
	_dim.color = Color(0.02, 0.0, 0.0, 0.55)
	_dim.set_anchors_preset(Control.PRESET_FULL_RECT)
	_dim.mouse_filter = Control.MOUSE_FILTER_IGNORE
	_dim.visible = false
	add_child(_dim)
	_panel.set_anchors_and_offsets_preset(Control.PRESET_CENTER)
	_panel.offset_left = -260
	_panel.offset_right = 260
	_panel.offset_top = -90
	_panel.alignment = BoxContainer.ALIGNMENT_CENTER
	_panel.add_theme_constant_override("separation", 14)
	_panel.visible = false
	add_child(_panel)
	var t := Label.new()
	t.text = "You have fallen"
	t.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	t.add_theme_font_override("font", HUD.TITLE)
	t.add_theme_font_size_override("font_size", 44)
	t.add_theme_color_override("font_color", HUD.ACCENT)
	t.add_theme_constant_override("outline_size", 8)
	t.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.8))
	_panel.add_child(t)
	_quip.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	_quip.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_quip.add_theme_color_override("font_color", HUD.TEXT)
	_panel.add_child(_quip)
	var b := _button
	b.text = "Rise again"
	b.focus_mode = Control.FOCUS_NONE
	b.custom_minimum_size = Vector2(200, 40)
	b.size_flags_horizontal = Control.SIZE_SHRINK_CENTER
	b.pressed.connect(func(): rise.emit())
	_panel.add_child(b)
	var hint := Label.new()
	hint.text = "or press Enter. You will wake in the nearest safe town."
	hint.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	_panel.add_child(hint)

## Down or up, from our health each frame.
func set_down(on: bool) -> void:
	if on == down:
		return
	down = on
	if on:
		_quip.text = LINES[randi() % LINES.size()]
		_left = wait_secs
		_count()
	_dim.visible = on
	_panel.visible = on
	if on:
		_dim.modulate.a = 0.0
		create_tween().tween_property(_dim, "modulate:a", 1.0, 0.8)

func _process(delta: float) -> void:
	if down and _left > 0.0:
		_left -= delta
		_count()

## The button waits out the delay, with the seconds left on it.
func _count() -> void:
	_button.disabled = _left > 0.0
	_button.text = "Rise in %d" % ceili(_left) if _left > 0.0 else "Rise again"

func _unhandled_key_input(event: InputEvent) -> void:
	if down and _left <= 0.0 and event is InputEventKey and event.pressed and not event.echo and event.keycode in [KEY_ENTER, KEY_KP_ENTER]:
		rise.emit()
		get_viewport().set_input_as_handled()

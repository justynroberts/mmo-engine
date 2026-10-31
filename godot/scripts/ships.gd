extends CanvasLayer
## Taking ship (F at a dock): the other docks with what passage to each costs, a button for each that sends
## "/sail Name" (crates/server/src/boats.rs). Fares are worked out the server's way from /play/info realm "fares"
## (a base and one coin more for every `per` units); the server takes the coin and says if it is short.

signal command(text: String)

const HUD := preload("res://scripts/hud.gd")
const BagScript := preload("res://scripts/bag.gd")

var panel := PanelContainer.new()
var _title := Label.new()
var _list := VBoxContainer.new()

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	panel.add_theme_stylebox_override("panel", BagScript.box())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(480, 0)
	panel.position = Vector2(-240, -200)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	_title = BagScript.title("Take ship", 26, false)
	v.add_child(_title)
	_list.add_theme_constant_override("separation", 6)
	v.add_child(_list)
	var hint := Label.new()
	hint.text = "Esc to close."
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	v.add_child(hint)

static func _shown(d: Dictionary) -> String:
	return String(d.get("title", "")) if String(d.get("title", "")) != "" else String(d.get("name", ""))

## Open at the dock `here` (its shown name), listing the rest of `docks` (/play/info realm docks) with their fares.
func open(here: String, docks: Array, fares: Dictionary, currency: String) -> void:
	for c in _list.get_children():
		c.queue_free()
	var from := {}
	for d in docks:
		if _shown(d) == here:
			from = d
	if from.is_empty():
		return
	_title.text = "Take ship from %s" % here
	var base := int(fares.get("base", 2))
	var per := maxf(float(fares.get("per", 400.0)), 1.0)
	var at := Vector2(float(from["x"]), float(from["y"]))
	for d in docks:
		if d == from:
			continue
		var to := Vector2(float(d["x"]), float(d["y"]))
		var fare := base + int(at.distance_to(to) / per)
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var l := Label.new()
		l.text = "%s   %d %s" % [_shown(d), fare, currency]
		l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		l.add_theme_color_override("font_color", HUD.TEXT)
		l.add_theme_font_size_override("font_size", 15)
		row.add_child(l)
		var b := Button.new()
		b.text = "Sail"
		b.focus_mode = Control.FOCUS_NONE
		var name := String(d.get("name", ""))
		b.pressed.connect(func():
			command.emit("/sail " + name)
			panel.visible = false)
		row.add_child(b)
		_list.add_child(row)
	if _list.get_child_count() == 0:
		var none := Label.new()
		none.text = "No boats go anywhere from here."
		none.add_theme_color_override("font_color", HUD.MUTED)
		_list.add_child(none)
	panel.visible = true

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

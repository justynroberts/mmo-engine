extends CanvasLayer
## Choosing a class, the first time a character comes into a game that has them: each class's title, what it is,
## its strengths and its spells, and a button to take it. The server keeps the choice for good.

signal chosen(index: int)

const HUD := preload("res://scripts/hud.gd")

var panel := PanelContainer.new()
var _list := VBoxContainer.new()
var _buttons: Array[Button] = []
var _armed := -1               # the class clicked once, waiting for the confirming click
var _opened_at := 0

func _ready() -> void:
	layer = 7
	HUD.use_fonts()
	panel.visible = false
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.97)
	b.border_color = HUD.EDGE
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(22)
	panel.add_theme_stylebox_override("panel", b)
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(760, 0)
	panel.position = Vector2(-380, -280)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 12)
	panel.add_child(v)
	var t := Label.new()
	t.text = "Choose your path"
	t.add_theme_font_override("font", HUD.TITLE)
	t.add_theme_font_size_override("font_size", 32)
	t.add_theme_color_override("font_color", HUD.ACCENT)
	v.add_child(t)
	var sub := Label.new()
	sub.text = "Your class is yours for good: it decides your spells and how hard you hit and how much you can take."
	sub.add_theme_color_override("font_color", HUD.MUTED)
	sub.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	sub.custom_minimum_size = Vector2(716, 0)
	v.add_child(sub)
	_list.add_theme_constant_override("separation", 14)
	v.add_child(_list)

## Show the classes ([{name, title, desc, might, toughness, spells}]) with spell titles from `spell_title`.
func open(classes: Array, spell_title: Callable) -> void:
	for c in _list.get_children():
		c.queue_free()
	_buttons.clear()
	_armed = -1
	_opened_at = Time.get_ticks_msec()
	for i in classes.size():
		var c: Dictionary = classes[i]
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 16)
		var info := VBoxContainer.new()
		info.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var t := Label.new()
		t.text = String(c["title"])
		t.add_theme_font_override("font", HUD.TITLE)
		t.add_theme_font_size_override("font_size", 22)
		t.add_theme_color_override("font_color", HUD.ACCENT)
		info.add_child(t)
		var d := Label.new()
		d.text = String(c.get("desc", ""))
		d.add_theme_color_override("font_color", HUD.TEXT)
		d.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		d.custom_minimum_size = Vector2(560, 0)
		info.add_child(d)
		var strengths := []
		if float(c.get("might", 0.0)) > 0.0:
			strengths.append("hits %d%% harder" % int(round(float(c["might"]) * 100.0)))
		if float(c.get("toughness", 0.0)) > 0.0:
			strengths.append("%d%% tougher" % int(round(float(c["toughness"]) * 100.0)))
		var names := []
		for n in c.get("spells", []):
			names.append(spell_title.call(String(n)))
		var line := Label.new()
		line.text = ("  ·  ".join(strengths) + ("    Spells: " + ", ".join(names) if not names.is_empty() else "")).strip_edges()
		line.add_theme_font_size_override("font_size", 13)
		line.add_theme_color_override("font_color", HUD.MUTED)
		line.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		line.custom_minimum_size = Vector2(560, 0)
		info.add_child(line)
		row.add_child(info)
		var go := Button.new()
		go.text = "Choose"
		go.focus_mode = Control.FOCUS_NONE
		go.custom_minimum_size = Vector2(150, 34)
		go.size_flags_vertical = Control.SIZE_SHRINK_CENTER
		var title := String(c["title"]).get_slice(" (", 0)
		# A class is for good: a click right as the panel opens is ignored, and the choice is confirmed.
		go.pressed.connect(func():
			if Time.get_ticks_msec() - _opened_at < 1000:
				return
			if _armed != i:
				_armed = i
				for b in _buttons:
					b.text = "Choose"
				go.text = "Confirm: " + title
				return
			panel.visible = false
			chosen.emit(i))
		_buttons.append(go)
		row.add_child(go)
		_list.add_child(row)
	panel.visible = true

func is_open() -> bool:
	return panel.visible

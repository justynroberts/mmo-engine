extends CanvasLayer
## The spell journal and the game menu, over the HUD.
##
## The journal lists every spell the game has, grouped by school, with what each does in plain words, and the
## spells on your bar in order: add one, take one off, move one up or down. main.gd keeps the order and saves it.
## The menu (Esc) logs out to the join screen or quits.

signal bar_changed(order: Array)        # action names, in bar order
signal log_out
signal quit
signal unequip(gear_slot: int)

const HUD := preload("res://scripts/hud.gd")

var journal := PanelContainer.new()
var menu := PanelContainer.new()
var _bar_list := VBoxContainer.new()
var _gear_row := HBoxContainer.new()   # what is worn, with "Take off"
var _gear := {}
var _all_list := VBoxContainer.new()
var _spells: Array = []                 # every spell: {name, title, desc, school, cost, color, key}
var _order: Array = []                  # names on the bar
var _keys: Array = []                   # the key for each bar position
var _max := 12

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	_build_journal()
	_build_menu()

func _panel_style() -> StyleBoxFlat:
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.96)
	b.border_color = HUD.EDGE
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(18)
	return b

func _title(text: String, size := 24) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_override("font", HUD.TITLE)
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", HUD.ACCENT)
	return l

func _text(text: String, size := 14, color := HUD.TEXT) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	return l

func _button(text: String, on_press: Callable, wide := 0.0) -> Button:
	var b := Button.new()
	b.text = text
	b.focus_mode = Control.FOCUS_NONE
	b.custom_minimum_size = Vector2(wide, 26)
	b.add_theme_font_size_override("font_size", 13)
	b.pressed.connect(on_press)
	return b

func _build_journal() -> void:
	journal.visible = false
	journal.add_theme_stylebox_override("panel", _panel_style())
	journal.set_anchors_preset(Control.PRESET_CENTER)
	journal.custom_minimum_size = Vector2(980, 560)
	journal.position = Vector2(-490, -300)
	add_child(journal)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	journal.add_child(v)
	var head := HBoxContainer.new()
	head.add_child(_title("Spell journal", 28))
	var gap := Control.new()
	gap.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	head.add_child(gap)
	head.add_child(_text("F2 or Esc to close", 12, HUD.MUTED))
	v.add_child(head)
	_gear_row.add_theme_constant_override("separation", 14)
	v.add_child(_gear_row)
	var cols := HBoxContainer.new()
	cols.add_theme_constant_override("separation", 22)
	cols.size_flags_vertical = Control.SIZE_EXPAND_FILL
	v.add_child(cols)
	# Left: your bar, in order.
	var left := VBoxContainer.new()
	left.custom_minimum_size = Vector2(330, 0)
	left.add_child(_title("Your bar", 18))
	left.add_child(_text("The keys cast them in this order.", 12, HUD.MUTED))
	left.add_child(_bar_list)
	cols.add_child(left)
	# Right: every spell.
	var right := VBoxContainer.new()
	right.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	right.add_child(_title("Every spell", 18))
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.custom_minimum_size = Vector2(0, 460)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_all_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_all_list.add_theme_constant_override("separation", 8)
	scroll.add_child(_all_list)
	right.add_child(scroll)
	cols.add_child(right)

func _build_menu() -> void:
	menu.visible = false
	menu.add_theme_stylebox_override("panel", _panel_style())
	menu.set_anchors_preset(Control.PRESET_CENTER)
	menu.custom_minimum_size = Vector2(260, 0)
	menu.position = Vector2(-130, -110)
	add_child(menu)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	menu.add_child(v)
	v.add_child(_title("Menu", 26))
	v.add_child(_button("Back to the game", func(): menu.visible = false, 220))
	v.add_child(_button("Spell journal", _menu_journal, 220))
	v.add_child(_button("Log out", func(): log_out.emit(), 220))
	v.add_child(_button("Quit", func(): quit.emit(), 220))

## What is worn: {weapon, armour, trinket (names, "" for nothing), riding}.
func set_gear(g: Dictionary) -> void:
	if g == _gear:
		return
	_gear = g
	for c in _gear_row.get_children():
		c.queue_free()
	_gear_row.add_child(_text("Worn:", 14, HUD.ACCENT))
	var any := false
	for k in ["weapon", "armour", "trinket"].size():
		var slot: String = ["weapon", "armour", "trinket"][k]
		var name := String(g.get(slot, ""))
		if name == "":
			continue
		any = true
		_gear_row.add_child(_text("%s: %s" % [slot.capitalize(), name], 14))
		_gear_row.add_child(_button("Take off", func(): unequip.emit(k), 76))
	if not any:
		_gear_row.add_child(_text("nothing (use a weapon, armour or a trinket from your bag to put it on)", 13, HUD.MUTED))
	if bool(g.get("riding", false)):
		_gear_row.add_child(_text("Riding", 14, HUD.ACCENT))

func _menu_journal() -> void:
	menu.visible = false
	open_journal()

## The spells and the bar: `spells` as above, `order` the names on the bar, `keys` a key name per position.
func set_spells(spells: Array, order: Array, keys: Array) -> void:
	_spells = spells
	_order = order.duplicate()
	_keys = keys
	_max = keys.size()
	_redraw()

func open_journal() -> void:
	_redraw()
	journal.visible = true

func toggle_journal() -> void:
	if journal.visible:
		journal.visible = false
	else:
		open_journal()

func is_open() -> bool:
	return journal.visible or menu.visible

## Esc: close whatever is open; with nothing open, open the menu.
func escape() -> bool:
	if journal.visible:
		journal.visible = false
		return true
	if menu.visible:
		menu.visible = false
		return true
	return false

func open_menu() -> void:
	menu.visible = true

func _spell(name: String) -> Dictionary:
	for s in _spells:
		if s["name"] == name:
			return s
	return {}

func _swatch(c: Color) -> ColorRect:
	var r := ColorRect.new()
	r.color = c
	r.custom_minimum_size = Vector2(4, 22)
	return r

func _redraw() -> void:
	for c in _bar_list.get_children():
		c.queue_free()
	for c in _all_list.get_children():
		c.queue_free()
	for i in _order.size():
		var s := _spell(_order[i])
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 6)
		var key := _text(String(_keys[i]) if i < _keys.size() else "", 13, HUD.ACCENT)
		key.custom_minimum_size = Vector2(22, 0)
		row.add_child(key)
		row.add_child(_swatch(s.get("color", HUD.MUTED)))
		var n := _text(String(s.get("title", _order[i])), 14)
		n.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		n.clip_text = true
		row.add_child(n)
		var up := _button("Up", func(): _move(i, -1), 40)
		up.disabled = i == 0
		row.add_child(up)
		var down := _button("Down", func(): _move(i, 1), 52)
		down.disabled = i == _order.size() - 1
		row.add_child(down)
		row.add_child(_button("Remove", func(): _remove(i), 64))
		_bar_list.add_child(row)
	if _order.is_empty():
		_bar_list.add_child(_text("Empty: add spells from the list.", 13, HUD.MUTED))
	var school := "-none-"
	for s in _spells:
		var sc := String(s.get("school", ""))
		if sc != school:
			school = sc
			_all_list.add_child(_title(("%s magic" % sc.capitalize()) if sc != "" else "Other", 16))
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		row.add_child(_swatch(s.get("color", HUD.MUTED)))
		var info := VBoxContainer.new()
		info.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		info.add_theme_constant_override("separation", 1)
		info.add_child(_text(String(s["title"]), 15))
		var d := _text(String(s.get("desc", "")), 13, HUD.MUTED)
		d.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		info.add_child(d)
		if String(s.get("cost", "")) != "":
			info.add_child(_text(String(s["cost"]), 12, HUD.MUTED))
		if not bool(s.get("ours", true)):
			info.add_child(_text("Not a spell of your class", 12, HUD.MUTED))
		elif not bool(s.get("learnt", true)):
			info.add_child(_text("Learnt at level %d" % int(s.get("level", 0)), 12, HUD.ACCENT))
		row.add_child(info)
		var on := _order.has(s["name"])
		var add := _button("On the bar" if on else "Add", func(): _add(String(s["name"])), 92)
		add.disabled = on or _order.size() >= _max or not bool(s.get("ours", true))
		row.add_child(add)
		_all_list.add_child(row)

func _move(i: int, by: int) -> void:
	var j := i + by
	if j < 0 or j >= _order.size():
		return
	var t = _order[i]
	_order[i] = _order[j]
	_order[j] = t
	_changed()

func _remove(i: int) -> void:
	_order.remove_at(i)
	_changed()

func _add(name: String) -> void:
	if not _order.has(name) and _order.size() < _max:
		_order.append(name)
		_changed()

func _changed() -> void:
	_redraw()
	bar_changed.emit(_order.duplicate())

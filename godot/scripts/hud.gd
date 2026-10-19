extends CanvasLayer
## The overlay: status, target, bag, chat. Built in code so there is no scene to keep in sync.

signal slot_pressed(slot: int)
signal chat_submitted(text: String)
signal material_pressed(kind: int)

var status := Label.new()
var target := Label.new()
var chat_log := RichTextLabel.new()
var chat_in := LineEdit.new()
var slots: Array[Button] = []
var help := Label.new()
var materials := HBoxContainer.new()   # the build bar, shown in build mode
var _material_buttons: Array[Button] = []
var _materials_shown := -2               # the chosen material as last drawn, -1 = hidden
const KEYS := ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "="]   # the keys that pick each material

func _ready() -> void:
	var root := Control.new()
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)

	status.position = Vector2(14, 10)
	status.add_theme_color_override("font_color", Color("9fe8e0"))
	root.add_child(status)

	target.set_anchors_preset(Control.PRESET_CENTER_TOP)
	target.position = Vector2(-320, 10)
	target.custom_minimum_size = Vector2(640, 0)
	target.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	target.add_theme_color_override("font_color", Color("ffd27a"))
	root.add_child(target)

	var bag := HBoxContainer.new()
	bag.set_anchors_preset(Control.PRESET_CENTER_BOTTOM)
	bag.position = Vector2(-4 * 84, -86)
	bag.add_theme_constant_override("separation", 6)
	root.add_child(bag)
	for i in 8:
		var b := Button.new()
		b.custom_minimum_size = Vector2(78, 56)
		b.focus_mode = Control.FOCUS_NONE
		b.clip_text = true
		b.pressed.connect(func(): slot_pressed.emit(i))
		bag.add_child(b)
		slots.append(b)

	materials.set_anchors_preset(Control.PRESET_CENTER_BOTTOM)
	materials.add_theme_constant_override("separation", 6)
	materials.visible = false
	root.add_child(materials)

	chat_log.bbcode_enabled = true
	chat_log.scroll_following = true
	chat_log.custom_minimum_size = Vector2(380, 120)
	chat_log.position = Vector2(14, 720 - 190)
	chat_log.mouse_filter = Control.MOUSE_FILTER_IGNORE
	root.add_child(chat_log)

	chat_in.position = Vector2(14, 720 - 60)
	chat_in.custom_minimum_size = Vector2(380, 0)
	chat_in.placeholder_text = "Enter to chat, start with ! to shout"
	chat_in.text_submitted.connect(func(t: String):
		chat_in.clear()
		chat_in.release_focus()
		if t.strip_edges() != "":
			chat_submitted.emit(t))
	root.add_child(chat_in)

	help.text = "WASD move   Q/E turn   wheel zoom   Tab target   Space attack (hold)   J jump   B build (click place, right-click break one of yours, 1-0 - = material; walk up one block at a time)   1-8 use   Shift+1-8 drop   F pick up   G god   M map   F11 full screen"
	help.position = Vector2(14, 720 - 28)
	help.add_theme_color_override("font_color", Color(1, 1, 1, 0.55))
	root.add_child(help)

func set_bag(bag: Array) -> void:
	for i in slots.size():
		var b := slots[i]
		var s: Dictionary = bag[i] if i < bag.size() else {}
		if s.get("item", "") == "":
			b.text = "%d" % (i + 1)
			b.self_modulate = Color(1, 1, 1, 0.35)
		else:
			b.text = "%d %s\nx%d" % [i + 1, s["item"], s["count"]]
			b.self_modulate = Color(str(s["color"]))

## The build bar: one button per block material, a picture of it and its key.
func set_materials(names: Array, swatches: Array) -> void:
	for b in _material_buttons:
		b.queue_free()
	_material_buttons.clear()
	for i in names.size():
		var b := Button.new()
		b.text = "%s\n%s" % [KEYS[i] if i < KEYS.size() else "", String(names[i]).to_upper()]
		b.icon = swatches[i]
		b.expand_icon = true
		b.add_theme_constant_override("icon_max_width", 30)
		b.add_theme_font_size_override("font_size", 11)
		b.custom_minimum_size = Vector2(98, 52)
		b.clip_text = true
		b.focus_mode = Control.FOCUS_NONE
		b.alignment = HORIZONTAL_ALIGNMENT_LEFT
		b.pressed.connect(func(): material_pressed.emit(i))
		materials.add_child(b)
		_material_buttons.append(b)
	materials.position = Vector2(-names.size() * 52, -150)

## Show the build bar (or not), with the chosen material marked.
func show_materials(on: bool, chosen: int) -> void:
	materials.visible = on and not _material_buttons.is_empty()
	if (chosen if on else -1) == _materials_shown:
		return
	_materials_shown = chosen if on else -1
	for i in _material_buttons.size():
		var b := _material_buttons[i]
		var picked := i == chosen
		var box := StyleBoxFlat.new()
		box.bg_color = Color(0.06, 0.07, 0.09, 0.85)
		box.set_border_width_all(2 if picked else 1)
		box.border_color = Color("ffd27a") if picked else Color(1, 1, 1, 0.18)
		box.set_corner_radius_all(3)
		box.set_content_margin_all(6)
		for state in ["normal", "hover", "pressed"]:
			b.add_theme_stylebox_override(state, box)
		b.add_theme_color_override("font_color", Color("ffd27a") if picked else Color(1, 1, 1, 0.7))

func say(line: String) -> void:
	chat_log.append_text(line + "\n")

func typing() -> bool:
	return chat_in.has_focus()

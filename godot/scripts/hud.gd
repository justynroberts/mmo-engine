extends CanvasLayer
## The overlay: status, target, bag, chat. Built in code so there is no scene to keep in sync.

signal slot_pressed(slot: int)
signal chat_submitted(text: String)

var status := Label.new()
var target := Label.new()
var chat_log := RichTextLabel.new()
var chat_in := LineEdit.new()
var slots: Array[Button] = []
var help := Label.new()

func _ready() -> void:
	var root := Control.new()
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)

	status.position = Vector2(14, 10)
	status.add_theme_color_override("font_color", Color("9fe8e0"))
	root.add_child(status)

	target.set_anchors_preset(Control.PRESET_CENTER_TOP)
	target.position = Vector2(-120, 10)
	target.custom_minimum_size = Vector2(240, 0)
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

	help.text = "WASD move   Q/E turn   wheel zoom   Tab target   Space attack   1-8 use   Shift+1-8 drop   F pick up"
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

func say(line: String) -> void:
	chat_log.append_text(line + "\n")

func typing() -> bool:
	return chat_in.has_focus()

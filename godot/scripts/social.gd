extends CanvasLayer
## The social window (F6): your guild, your letters and how the factions and towns think of you, side by side, with
## buttons for what is otherwise typed ("/guild", "/mail", "/mailbox"). Opening it asks the server ("/social"), which
## answers on chat channel 17 (crates/server/src/social.rs `social_window`); a button asks again a moment later.

signal command(text: String)
signal compose(prefill: String)    # open the chat with this typed in (a guild's name, a letter)

const HUD := preload("res://scripts/hud.gd")
const BagScript := preload("res://scripts/bag.gd")

var panel := PanelContainer.new()
var target_name := ""              # the player targeted, for "Invite to guild" (main.gd keeps it up)
var _guild := []                   # [name, leader, members, mine] or empty
var _members: Array = []
var _letters := 0
var _letter_lines: Array = []      # [[from, goods, text], ...]
var _standing: Array = []          # [[name, value, word], ...]
var _towns: Array = []
var _skills: Array = []            # [[title, level, max], ...]
var _feats: Array = []             # achievements: [[title, earned, so far, count, title it grants], ...]
var _worn := ""                    # the title worn
var _cols := HBoxContainer.new()

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	panel.add_theme_stylebox_override("panel", BagScript.box())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(1180, 460)
	panel.position = Vector2(-590, -260)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	v.add_child(BagScript.title("Company, standing, skills and deeds", 26, false))
	_cols.add_theme_constant_override("separation", 28)
	_cols.size_flags_vertical = Control.SIZE_EXPAND_FILL
	v.add_child(_cols)
	var hint := Label.new()
	hint.text = "F6 or Esc to close."
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	v.add_child(hint)

## A line on chat channel 17.
func on_line(text: String) -> void:
	var f := text.split("|")
	match f[0]:
		"X":
			_guild = []
			_members.clear()
			_letter_lines.clear()
			_standing.clear()
			_towns.clear()
			_skills.clear()
			_feats.clear()
			_worn = ""
		"G":
			_guild = [f[1], f[2], int(f[3]), f[4] == "1"] if f.size() >= 5 else []
		"GM":
			_members.append(f[1])
		"M":
			_letters = int(f[1])
		"ML":
			if f.size() >= 4:
				_letter_lines.append([f[1], f[2], f[3]])
		"S":
			if f.size() >= 4:
				_standing.append([f[1], int(f[2]), f[3]])
		"T":
			if f.size() >= 4:
				_towns.append([f[1], int(f[2]), f[3]])
		"K":
			if f.size() >= 4:
				_skills.append([f[1], int(f[2]), int(f[3])])
		"A":
			if f.size() >= 6:
				_feats.append([f[1], f[2] == "1", int(f[3]), int(f[4]), f[5]])
		"W":
			_worn = f[1] if f.size() > 1 else ""
		"E":
			_rebuild()

func _column(title: String) -> VBoxContainer:
	var c := VBoxContainer.new()
	c.add_theme_constant_override("separation", 6)
	c.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	c.add_child(BagScript.title(title, 17, false))
	_cols.add_child(c)
	return c

func _text(c: Control, t: String, col := HUD.TEXT, size := 14) -> void:
	var l := Label.new()
	l.text = t
	l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	l.add_theme_color_override("font_color", col)
	l.add_theme_font_size_override("font_size", size)
	c.add_child(l)

func _button(c: Control, t: String, on: Callable) -> void:
	var b := Button.new()
	b.text = t
	b.focus_mode = Control.FOCUS_NONE
	b.pressed.connect(func():
		on.call()
		get_tree().create_timer(0.6).timeout.connect(refresh))
	c.add_child(b)

func _rebuild() -> void:
	for c in _cols.get_children():
		c.queue_free()
	# The guild.
	var g := _column("Guild")
	if _guild.is_empty():
		_text(g, "You are in no guild. A guild shares chat (/g) and its members may build on each other's land.", HUD.MUTED, 13)
		_button(g, "Found a guild...", func(): compose.emit("/guild create "))
	else:
		_text(g, String(_guild[0]), HUD.ACCENT, 16)
		_text(g, "Led by %s, %d member%s" % [_guild[1], _guild[2], "" if _guild[2] == 1 else "s"], HUD.MUTED, 13)
		_text(g, ", ".join(_members), HUD.TEXT, 13)
		if target_name != "":
			_button(g, "Invite %s" % target_name, func(): command.emit("/guild invite " + target_name))
		_button(g, "Guild chat...", func(): compose.emit("/g "))
		_button(g, "Leave the guild", func(): command.emit("/guild leave"))
	# Letters.
	var m := _column("Letters")
	_text(m, "%d waiting" % _letters if _letters > 0 else "No letters.", HUD.ACCENT if _letters > 0 else HUD.MUTED, 14)
	for l in _letter_lines:
		var goods: String = l[1]
		_text(m, "From %s%s: %s" % [l[0], (" (with " + goods + ")") if goods != "" else "", l[2]], HUD.TEXT, 13)
	_button(m, "Write a letter...", func(): compose.emit("/mail "))
	if _letters > 0:
		_button(m, "Read and take goods (at a market)", func(): command.emit("/mailbox"))
	# Standing.
	var s := _column("Standing")
	if _standing.is_empty() and _towns.is_empty():
		_text(s, "Nobody knows you yet.", HUD.MUTED, 13)
	for e in _standing:
		_text(s, "%s  %d  (%s)" % [e[0], e[1], e[2]], _mood_colour(int(e[1])), 13)
	if not _towns.is_empty():
		_text(s, "Towns", HUD.ACCENT, 14)
	for e in _towns:
		_text(s, "%s  %d  (%s)" % [e[0], e[1], e[2]], _mood_colour(int(e[1])), 13)
	if not _skills.is_empty():
		var k := _column("Skills")
		for e in _skills:
			_text(k, "%s  %d / %d" % [e[0], e[1], e[2]], HUD.ACCENT if int(e[1]) > 0 else HUD.TEXT, 14)
		_text(k, "Skills rise with use: making (the bag's Make row), harvesting and gathering.", HUD.MUTED, 12)
	if not _feats.is_empty():
		var a := _column("Achievements")
		_text(a, ("Called " + _worn) if _worn != "" else "No title worn.", HUD.ACCENT if _worn != "" else HUD.MUTED, 13)
		for e in _feats:
			var grants: String = e[4]
			_text(a, "%s %s  %d/%d%s" % ["✓" if e[1] else "·", e[0], e[2], e[3], ("  (" + grants + ")") if grants != "" else ""], HUD.ACCENT if e[1] else HUD.TEXT, 13)
			if e[1] and grants != "" and grants != _worn:
				_button(a, "Be called " + grants, func(): command.emit("/title " + grants))
		if _worn != "":
			_button(a, "Wear no title", func(): command.emit("/title none"))

static func _mood_colour(v: int) -> Color:
	return Color("8fd18a") if v > 0 else (Color("ff8a70") if v < 0 else HUD.TEXT)

func refresh() -> void:
	if panel.visible:
		command.emit("/social")

func toggle() -> void:
	panel.visible = not panel.visible
	refresh()

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

extends CanvasLayer
## The overlay, laid out the way MMO players expect: your frame and your target's top left, the minimap top right,
## chat bottom left, the action bar bottom centre, announcements across the top. The keys live behind F1.
## Built in code so there is no scene to keep in sync.

signal slot_pressed(slot: int)
signal chat_submitted(text: String)
signal material_pressed(kind: int)
signal mode_pressed                     # the mode chip was clicked (toggle Action / Build)
signal ability_pressed(name: String)    # attack, jump, torch, map, build
signal spell_pressed(i: int)            # a slot on the spell bar

const ACCENT := Color("ffd27a")
const INK := Color(0.05, 0.055, 0.065, 0.84)
const EDGE := Color(0.79, 0.64, 0.36, 0.55)
const TEXT := Color("f3ead2")
const MUTED := Color(0.95, 0.92, 0.82, 0.55)
const KEYS := ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0", "-", "=", "[", "]", "\\"]   # the keys that pick each material
const HELP_ACTION := [["WASD", "move"], ["Q / E", "turn"], ["Mouse wheel", "zoom, in past the closest for first person"],
	["V", "first person"], ["Click", "target"], ["Tab", "next target"], ["Space", "attack (hold to keep swinging)"],
	["J", "jump"], ["R T Y U O P Z C ' , . ;", "spells, on the bar over the action bar"], ["H", "hail: a trader or the like stops for you"], ["L", "torch: auto, always, out"], ["1-8", "use an item"], ["Shift + 1-8", "drop it"], ["F", "talk to someone near, trade with the player targeted, or pick up"], ["F3", "quests"], ["I or F4", "bag and bank"],
	["Enter", "chat, start with ! to shout"], ["M", "world map"], ["N", "sound: all, no music, off"], ["G", "god mode"], ["F11", "full screen"]]
const HELP_BUILD := [["B", "build mode on and off (Esc leaves)"], ["1-0 - = [ ] \\", "pick a material (\\ is tilled soil: plant seeds beside it)"],
	["Click", "place a block"], ["Hold right click, X or Shift+click", "break one of yours (sweep to keep going)"]]

## The words: Alegreya for what you read (chat, bars, help, nameplates), Uncial Antiqua's Celtic lettering for titles (names,
## banners, places). Both SIL Open Font License, in res://fonts.
static var BODY: Font
static var TITLE: Font
## Items' English names by item name (/play/info "items" "title"), for every panel that shows one: `item`.
static var ITEM_TITLES := {}

## Text from anyone else (chat, names, a plot's name in a server line) made safe for the BBCode chat log, so
## nobody can restyle, resize or fake lines on another player's screen.
static func esc(text: String) -> String:
	return text.replace("[", "[lb]")

## The base URL of a server's web side from what a player typed: "host:port" and bare IPs or localhost are plain
## http (port 7790 if none), a bare domain name is https on 443 (a server behind a TLS proxy), and a given
## http:// or https:// is kept.
static func web_base(server: String) -> String:
	var s := server.strip_edges().trim_suffix("/")
	if s.begins_with("http://") or s.begins_with("https://"):
		return s
	if ":" in s:
		return "http://" + s
	var local := s == "localhost" or s.is_valid_ip_address() or not "." in s
	return ("http://%s:7790" if local else "https://%s") % s

## The host a server's UDP game port is on, from the same text.
static func game_host(server: String) -> String:
	return server.strip_edges().trim_prefix("http://").trim_prefix("https://").get_slice("/", 0).get_slice(":", 0)

## What players call an item: its title, else its name.
static func item(name: String) -> String:
	return String(ITEM_TITLES.get(name, name))

## Load the fonts (res://fonts/body.tres: Alegreya, medium, level figures for numbers; title.tres: Uncial Antiqua). The
## project's default font is body.tres too, so every control and 3D label that names none uses it.
static func use_fonts() -> void:
	if BODY != null:
		return
	BODY = load("res://fonts/body.tres")
	TITLE = load("res://fonts/title.tres")

var status := Label.new()               # errors and the smoke test; empty while all is well
var me_frame := Frame.new()
var target_frame := Frame.new()
var minimap := Minimap.new()
var where := Label.new()                # under the minimap: the zone or the wilds
var when := Label.new()                 # time of day and weather
var net_line := Label.new()             # ping, and a note when the server is showing only the nearest
var chat_box := PanelContainer.new()
var chat_log := RichTextLabel.new()
var chat_in := LineEdit.new()
var chat_hint := Label.new()
var bar := HBoxContainer.new()          # the action bar: abilities, the bag, map and build
var slots: Array[Slot] = []
var spellbar := HBoxContainer.new()     # the game's spells (its actions after the first), over the action bar
var spell_slots: Array[Slot] = []
var party_box := VBoxContainer.new()    # party mates under your frame: name and health
var spell_tip := PanelContainer.new()   # what the spell under the mouse does, over the bars
var _tip_title := Label.new()
var _tip_body := Label.new()
var abilities := {}                     # name -> Slot
var materials := HBoxContainer.new()   # the build bar, shown in build mode
var _material_buttons: Array[Button] = []
var _materials_shown := -2               # the chosen material as last drawn, -1 = hidden
var _material_counts := []              # what each material's count was as last drawn
var mode_chip := Button.new()           # BUILD <material>, over the build bar; click to leave
var _mode_shown := ""
var crosshair := Control.new()          # first person: where clicks aim
var location := VBoxContainer.new()     # a place's title as you walk in or out, fading in and away
var loc_title := Label.new()
var loc_sub := Label.new()              # "Leaving" over the title on the way out, empty on the way in
var travel_cover := ColorRect.new()     # zone lines: the screen goes dark while the next zone loads
var travel_text := Label.new()
var banner := Label.new()               # big centred text: a zone, the weather
var toast := Label.new()                # small centred text: why a swing did nothing
var help := PanelContainer.new()
var loader := CanvasLayer.new()         # over everything until the world round you is in and drawn
var load_bar := ColorRect.new()         # the filled part of the bar
var load_track := ColorRect.new()
var load_text := Label.new()
var _load_frac := 0.0                   # how far loading has got
var _load_shown := 0.0                  # how far the bar is drawn: it eases towards _load_frac
var _load_since := 0.0                  # seconds the screen has been up
var _load_done := false                 # loaded() was called; it fades once the bar is full and it has been up a while
const LOAD_MIN := 2.2                   # long enough to read a line, however fast the world arrives
var help_hint := Label.new()
var _banner_tw: Tween
var _loc_tw: Tween
var _toast_tw: Tween

func _ready() -> void:
	use_fonts()
	for f in [me_frame, target_frame]:
		f.title_font = TITLE
	var root := Control.new()
	root.set_anchors_preset(Control.PRESET_FULL_RECT)
	root.mouse_filter = Control.MOUSE_FILTER_IGNORE
	add_child(root)

	# Top left: you, then your target.
	me_frame.position = Vector2(14, 14)
	root.add_child(me_frame)
	target_frame.position = Vector2(14 + Frame.W + 12, 14)
	target_frame.visible = false
	target_frame.hostile = true
	root.add_child(target_frame)

	# Top right: the minimap and what it knows.
	var top_right := HBoxContainer.new()
	top_right.set_anchors_and_offsets_preset(Control.PRESET_TOP_RIGHT)
	top_right.offset_left = -Minimap.SIDE - 16 - 48
	top_right.offset_top = 12
	top_right.offset_right = -16
	top_right.add_theme_constant_override("separation", 6)
	top_right.mouse_filter = Control.MOUSE_FILTER_IGNORE
	root.add_child(top_right)
	# The map and build buttons sit beside the minimap, as minimap buttons do.
	var side := VBoxContainer.new()
	side.add_theme_constant_override("separation", 6)
	side.mouse_filter = Control.MOUSE_FILTER_IGNORE
	top_right.add_child(side)
	for a in [["map", "Map", "M"], ["build", "Build", "B"], ["journal", "Spells", "K"], ["quests", "Quests", "F3"]]:
		var s := _ability(a[0], a[1], a[2], side)
		s.custom_minimum_size = Vector2(42, 42)
	var mm := VBoxContainer.new()
	mm.add_theme_constant_override("separation", 1)
	mm.mouse_filter = Control.MOUSE_FILTER_IGNORE
	top_right.add_child(mm)
	mm.add_child(minimap)
	for l in [where, when, net_line]:
		l.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
		l.add_theme_font_size_override("font_size", 13 if l != where else 15)
		if l == where:
			l.add_theme_font_override("font", TITLE)
		l.add_theme_color_override("font_color", TEXT if l == where else MUTED)
		l.text_overrun_behavior = TextServer.OVERRUN_TRIM_ELLIPSIS # a long place name is cut short, never pushes off screen
		_outline(l, 4)
		mm.add_child(l)

	# Bottom left: chat. The line to type in shows only while you type.
	chat_box.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_LEFT)
	chat_box.offset_left = 14
	chat_box.offset_right = 14 + 340
	chat_box.offset_top = -14 - 180
	chat_box.offset_bottom = -14
	chat_box.add_theme_stylebox_override("panel", _box(Color(0.03, 0.035, 0.04, 0.35), Color(1, 1, 1, 0.0), 8))
	chat_box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	root.add_child(chat_box)
	var cv := VBoxContainer.new()
	cv.mouse_filter = Control.MOUSE_FILTER_IGNORE
	chat_box.add_child(cv)
	var tab := Label.new()
	tab.text = "General"
	tab.add_theme_font_override("font", TITLE)
	tab.add_theme_font_size_override("font_size", 12)
	tab.add_theme_color_override("font_color", ACCENT)
	cv.add_child(tab)
	chat_log.bbcode_enabled = true
	chat_log.scroll_following = true
	chat_log.size_flags_vertical = Control.SIZE_EXPAND_FILL
	chat_log.mouse_filter = Control.MOUSE_FILTER_IGNORE
	chat_log.add_theme_font_size_override("normal_font_size", 14)
	chat_log.add_theme_constant_override("outline_size", 3)
	chat_log.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.8))
	cv.add_child(chat_log)
	chat_in.placeholder_text = "Say something, start with ! to shout. Esc to cancel."
	chat_in.visible = false
	chat_in.add_theme_stylebox_override("normal", _box(Color(0, 0, 0, 0.55), EDGE, 6))
	chat_in.add_theme_stylebox_override("focus", _box(Color(0, 0, 0, 0.55), ACCENT, 6))
	chat_in.text_submitted.connect(func(t: String):
		close_chat()
		if t.strip_edges() != "":
			chat_submitted.emit(t))
	chat_in.gui_input.connect(func(ev: InputEvent):
		if ev is InputEventKey and ev.pressed and ev.keycode == KEY_ESCAPE:
			close_chat()
			chat_in.accept_event())
	chat_in.focus_exited.connect(func():
		if chat_in.text == "":
			close_chat())
	cv.add_child(chat_in)
	chat_hint.text = "Enter to chat"
	chat_hint.add_theme_font_size_override("font_size", 12)
	chat_hint.add_theme_color_override("font_color", MUTED)
	cv.add_child(chat_hint)

	# Bottom centre: the action bar.
	bar.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE)
	bar.add_theme_constant_override("separation", 4)
	bar.alignment = BoxContainer.ALIGNMENT_CENTER
	root.add_child(bar)
	for a in [["attack", "Attack", "Spc"], ["jump", "Jump", "J"], ["torch", "Torch", "L"]]:
		_ability(a[0], a[1], a[2], bar)
	bar.add_child(_gap())
	for i in 8:
		var s := Slot.new()
		s.key = str(i + 1)
		s.pressed.connect(func(): slot_pressed.emit(i))
		bar.add_child(s)
		slots.append(s)
	_place_bar.call_deferred()
	spellbar.add_theme_constant_override("separation", 4)
	spellbar.alignment = BoxContainer.ALIGNMENT_CENTER
	spellbar.visible = false
	root.add_child(spellbar)
	party_box.position = Vector2(16, 120)
	party_box.add_theme_constant_override("separation", 4)
	party_box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	root.add_child(party_box)
	spell_tip.visible = false
	spell_tip.mouse_filter = Control.MOUSE_FILTER_IGNORE
	spell_tip.add_theme_stylebox_override("panel", _box(Color(0.04, 0.045, 0.05, 0.94), EDGE, 12))
	spell_tip.custom_minimum_size = Vector2(320, 0)
	var tv := VBoxContainer.new()
	tv.mouse_filter = Control.MOUSE_FILTER_IGNORE
	spell_tip.add_child(tv)
	_tip_title.add_theme_font_override("font", TITLE)
	_tip_title.add_theme_font_size_override("font_size", 18)
	_tip_title.add_theme_color_override("font_color", ACCENT)
	tv.add_child(_tip_title)
	_tip_body.add_theme_font_size_override("font_size", 14)
	_tip_body.add_theme_color_override("font_color", TEXT)
	_tip_body.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_tip_body.custom_minimum_size = Vector2(296, 0)
	tv.add_child(_tip_body)
	root.add_child(spell_tip)

	materials.add_theme_constant_override("separation", 4)
	materials.visible = false
	root.add_child(materials)
	mode_chip.focus_mode = Control.FOCUS_NONE
	mode_chip.add_theme_font_size_override("font_size", 13)
	mode_chip.tooltip_text = "Back to action mode (B or Esc)"
	mode_chip.pressed.connect(func(): mode_pressed.emit())
	mode_chip.visible = false
	root.add_child(mode_chip)

	# Across the top: announcements, and a line for anything that went wrong.
	for l in [banner, toast, status]:
		l.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP)
		l.offset_left = -420
		l.offset_right = 420
		l.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
		l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
		_outline(l, 6)
		root.add_child(l)
	banner.offset_top = 150
	banner.add_theme_font_override("font", TITLE)
	banner.add_theme_font_size_override("font_size", 28)
	banner.modulate.a = 0.0
	toast.offset_top = 196
	toast.add_theme_font_size_override("font_size", 16)
	toast.add_theme_color_override("font_color", Color("ffb4a0"))
	toast.modulate.a = 0.0
	status.offset_top = 236

	# Above them, the place you are in: its title fades in as you cross its edge, and fades away again.
	location.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP)
	location.offset_left = -420
	location.offset_right = 420
	location.offset_top = 64
	location.mouse_filter = Control.MOUSE_FILTER_IGNORE
	location.add_theme_constant_override("separation", 0)
	location.modulate.a = 0.0
	for l in [loc_sub, loc_title]:
		l.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
		_outline(l, 8)
		location.add_child(l)
	loc_sub.add_theme_font_size_override("font_size", 14)
	loc_sub.add_theme_color_override("font_color", MUTED)
	loc_sub.add_theme_font_override("font", TITLE)
	loc_title.add_theme_font_override("font", TITLE)
	loc_title.add_theme_font_size_override("font_size", 38)
	loc_title.add_theme_color_override("font_color", ACCENT)
	root.add_child(location)

	# Over everything: the dark between zones, with where you are going.
	travel_cover.set_anchors_preset(Control.PRESET_FULL_RECT)
	travel_cover.color = Color(0.02, 0.022, 0.026)
	travel_cover.mouse_filter = Control.MOUSE_FILTER_IGNORE
	travel_cover.modulate.a = 0.0
	travel_cover.visible = false
	travel_text.set_anchors_and_offsets_preset(Control.PRESET_CENTER)
	travel_text.offset_left = -400
	travel_text.offset_right = 400
	travel_text.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	travel_text.add_theme_font_override("font", TITLE)
	travel_text.add_theme_font_size_override("font_size", 26)
	travel_text.add_theme_color_override("font_color", ACCENT)
	travel_cover.add_child(travel_text)
	root.add_child(travel_cover)
	status.add_theme_color_override("font_color", Color("ff8a70"))

	# Loading: a dark screen, the stage, and a thin bar, over the HUD and the travel cover alike.
	loader.layer = 20
	loader.visible = false
	var lc := ColorRect.new()
	lc.set_anchors_preset(Control.PRESET_FULL_RECT)
	lc.color = Color(0.02, 0.022, 0.026)
	loader.add_child(lc)
	# The title art behind it, dimmed (res://ui/title.jpg); the bar sits low, under the art's lettering.
	var art: TextureRect = preload("res://scripts/join.gd").make_art()
	if art:
		art.modulate = Color(0.55, 0.55, 0.6)
		lc.add_child(art)
		lc.add_child(preload("res://scripts/join.gd").fade_rect(0.5))
	var lcol := VBoxContainer.new()
	lcol.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM)
	lcol.offset_left = -180
	lcol.offset_right = 180
	lcol.offset_top = -130
	lcol.offset_bottom = -40
	lcol.add_theme_constant_override("separation", 10)
	lc.add_child(lcol)
	var lt := Label.new()
	lt.text = "Entering the world"
	lt.add_theme_font_override("font", TITLE)
	lt.add_theme_font_size_override("font_size", 30)
	lt.add_theme_color_override("font_color", ACCENT)
	lcol.add_child(lt)
	load_track.color = Color(1, 1, 1, 0.08)
	load_track.custom_minimum_size = Vector2(360, 4)
	lcol.add_child(load_track)
	load_bar.color = ACCENT
	load_bar.size = Vector2(0, 4)
	load_track.add_child(load_bar)
	load_text.add_theme_color_override("font_color", MUTED)
	lcol.add_child(load_text)
	add_child(loader)

	# Two short bars with a gap in the middle, so the crosshair never hides what it is on.
	crosshair.set_anchors_preset(Control.PRESET_CENTER)
	crosshair.mouse_filter = Control.MOUSE_FILTER_IGNORE
	crosshair.visible = false
	for r in [Rect2(-9, -1, 6, 2), Rect2(3, -1, 6, 2), Rect2(-1, -9, 2, 6), Rect2(-1, 3, 2, 6)]:
		var b := ColorRect.new()
		b.color = Color(1, 1, 1, 0.85)
		b.position = r.position
		b.size = r.size
		b.mouse_filter = Control.MOUSE_FILTER_IGNORE
		crosshair.add_child(b)
	root.add_child(crosshair)

	# The keys, behind F1.
	help_hint.text = "F1  controls"
	help_hint.set_anchors_and_offsets_preset(Control.PRESET_BOTTOM_RIGHT, Control.PRESET_MODE_MINSIZE, 16)
	help_hint.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	help_hint.grow_vertical = Control.GROW_DIRECTION_BEGIN
	help_hint.add_theme_font_size_override("font_size", 12)
	help_hint.add_theme_color_override("font_color", MUTED)
	_outline(help_hint, 3)
	root.add_child(help_hint)
	help.add_theme_stylebox_override("panel", _box(INK, EDGE, 18))
	help.visible = false
	help.mouse_filter = Control.MOUSE_FILTER_IGNORE
	var hv := VBoxContainer.new()
	help.add_child(hv)
	for part in [["Controls", HELP_ACTION], ["Building", HELP_BUILD]]:
		var h := Label.new()
		h.text = part[0]
		h.add_theme_font_override("font", TITLE)
		h.add_theme_font_size_override("font_size", 17)
		h.add_theme_color_override("font_color", ACCENT)
		hv.add_child(h)
		var g := GridContainer.new()
		g.columns = 2
		g.add_theme_constant_override("h_separation", 18)
		for row in part[1]:
			for j in 2:
				var c := Label.new()
				c.text = row[j]
				c.add_theme_font_size_override("font_size", 14)
				c.add_theme_color_override("font_color", TEXT if j == 0 else MUTED)
				g.add_child(c)
		hv.add_child(g)
	var close := Label.new()
	close.text = "F1 to close"
	close.add_theme_font_size_override("font_size", 12)
	close.add_theme_color_override("font_color", MUTED)
	hv.add_child(close)
	root.add_child(help)

func _ability(name: String, title: String, key: String, into: Container) -> Slot:
	var s := Slot.new()
	s.key = key
	s.title = title
	s.glyph = true
	s.tooltip_text = "%s (%s)" % [title, key]
	s.pressed.connect(func(): ability_pressed.emit(name))
	into.add_child(s)
	abilities[name] = s
	return s

func _gap() -> Control:
	var c := Control.new()
	c.custom_minimum_size = Vector2(10, 0)
	c.mouse_filter = Control.MOUSE_FILTER_IGNORE
	return c

## The bar's width is only known once its slots are in.
func _place_bar() -> void:
	# Centred whatever their width turns out to be: they grow both ways from the middle, and up from the bottom.
	for b in [bar, spellbar]:
		b.grow_horizontal = Control.GROW_DIRECTION_BOTH
		b.grow_vertical = Control.GROW_DIRECTION_BEGIN
	bar.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE, 16)
	spellbar.set_anchors_and_offsets_preset(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE, 16 + Slot.SIDE + 6)
	for b in [bar, spellbar]:
		b.reset_size()

## The spell bar: [{title, key, color, tip}, ...]; empty hides it.
func set_spells(list: Array) -> void:
	for s in spell_slots:
		s.queue_free()
	spell_slots.clear()
	for i in list.size():
		var d: Dictionary = list[i]
		var s := Slot.new()
		s.key = String(d["key"])
		s.title = String(d["title"])
		s.glyph = true
		s.color = d["color"]
		s.pressed.connect(func(): spell_pressed.emit(i))
		s.hovered.connect(_show_tip.bind(s, String(d["tip_title"]), String(d["tip"])))
		spellbar.add_child(s)
		spell_slots.append(s)
	spellbar.visible = not list.is_empty()
	_place_bar.call_deferred()

## The spell under the mouse: its name and what it does, in a panel over the bars (clear of the buttons).
func _show_tip(on: bool, s: Slot, title: String, body: String) -> void:
	spell_tip.visible = on
	if not on:
		return
	_tip_title.text = title
	_tip_body.text = body
	spell_tip.reset_size()
	var r := s.get_global_rect()
	var top := spellbar.get_global_rect().position.y - 10.0
	spell_tip.position = Vector2(clampf(r.get_center().x - spell_tip.size.x * 0.5, 8.0, get_viewport().get_visible_rect().size.x - spell_tip.size.x - 8.0), top - spell_tip.size.y)

## Party mates: [[name, health 0..1, or -1 when out of sight], ...].
func set_party(mates: Array) -> void:
	if party_box.get_child_count() != mates.size():
		for c in party_box.get_children():
			c.queue_free()
		for m in mates:
			var row := VBoxContainer.new()
			row.add_theme_constant_override("separation", 1)
			var n := Label.new()
			n.add_theme_font_size_override("font_size", 13)
			n.add_theme_color_override("font_color", Color("8fd3ff"))
			_outline(n, 3)
			row.add_child(n)
			var bar := ProgressBar.new()
			bar.custom_minimum_size = Vector2(160, 6)
			bar.show_percentage = false
			bar.max_value = 1.0
			var fill := StyleBoxFlat.new()
			fill.bg_color = Color("c23b2e")
			var bg := StyleBoxFlat.new()
			bg.bg_color = Color(0, 0, 0, 0.6)
			bar.add_theme_stylebox_override("fill", fill)
			bar.add_theme_stylebox_override("background", bg)
			row.add_child(bar)
			party_box.add_child(row)
	for i in mates.size():
		var row := party_box.get_child(i)
		(row.get_child(0) as Label).text = String(mates[i][0]) + ("" if float(mates[i][1]) >= 0.0 else "  (far)")
		(row.get_child(1) as ProgressBar).value = maxf(float(mates[i][1]), 0.0)

## Each spell's cooldown still to run (0..1) and whether it can be paid for now.
func update_spells(cools: Array, can: Array) -> void:
	for i in mini(spell_slots.size(), cools.size()):
		var s := spell_slots[i]
		var c := float(cools[i])
		var dim := not bool(can[i])
		if absf(s.cool - c) > 0.01 or s.dim != dim:
			s.cool = c
			s.dim = dim
			s.queue_redraw()

static func _box(bg: Color, edge: Color, pad: float) -> StyleBoxFlat:
	var b := StyleBoxFlat.new()
	b.bg_color = bg
	b.border_color = edge
	b.set_border_width_all(1 if edge.a > 0.0 else 0)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(pad)
	return b

static func _outline(l: Label, px: int) -> void:
	l.add_theme_constant_override("outline_size", px)
	l.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.85))
	l.mouse_filter = Control.MOUSE_FILTER_IGNORE

# ---------------------------------------------------------------- what main.gd calls

## Your frame. bars: [[label, fraction, text, colour], ...]; badges: short words shown under it (GOD, SNEAKING).
func set_player(title: String, sub: String, initial: String, bars: Array, badges: Array) -> void:
	me_frame.set_data(title, sub, initial, bars, badges)

## Your target's frame; an empty title hides it.
func set_target(title: String, sub: String, bars: Array) -> void:
	target_frame.visible = title != ""
	if target_frame.visible:
		target_frame.set_data(title, sub, "?", bars, [])

## The picture under the minimap: the theme's map of the whole world.
func set_map(img: Image, world_size: float) -> void:
	minimap.tex = ImageTexture.create_from_image(img)
	minimap.world = world_size

func update_minimap(me: Vector2, heading: float, others: Array) -> void:
	minimap.me = me
	minimap.facing = heading
	minimap.others = others
	minimap.queue_redraw()

func set_info(place: String, time_and_sky: String, ping_ms: int, crowd: String) -> void:
	where.text = place
	when.text = time_and_sky
	net_line.text = "%d ms%s" % [ping_ms, crowd]
	net_line.add_theme_color_override("font_color", Color("8fd18a") if ping_ms < 100 else Color("ffd27a") if ping_ms < 250 else Color("ff8a70"))

## Big text across the top, fading out: walking into a zone, the weather turning.
func announce(text: String, color := ACCENT) -> void:
	banner.text = text
	banner.add_theme_color_override("font_color", color)
	_banner_tw = _fade(banner, _banner_tw, 3.0)

## On arriving: "<name> begins their quest in <place>" and, in Irish, "Good luck, adventurer", held a while and
## faded away.
func welcome(who: String, where: String) -> void:
	var box := VBoxContainer.new()
	box.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP)
	box.offset_top = 150
	box.offset_left = -500
	box.offset_right = 500
	box.mouse_filter = Control.MOUSE_FILTER_IGNORE
	box.add_theme_constant_override("separation", 6)
	var a := Label.new()
	a.text = "%s begins their quest %s" % [who, where]
	a.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	a.add_theme_font_override("font", TITLE)
	a.add_theme_font_size_override("font_size", 30)
	a.add_theme_color_override("font_color", TEXT)
	_outline(a, 6)
	box.add_child(a)
	var b := Label.new()
	b.text = "Ádh mór ort, a eachtránaí!"
	b.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	b.add_theme_font_override("font", TITLE)
	b.add_theme_font_size_override("font_size", 40)
	b.add_theme_color_override("font_color", ACCENT)
	_outline(b, 7)
	box.add_child(b)
	var c := Label.new()
	c.text = "Good luck, adventurer"
	c.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	c.add_theme_font_size_override("font_size", 15)
	c.add_theme_color_override("font_color", MUTED)
	_outline(c, 4)
	box.add_child(c)
	box.modulate.a = 0.0
	add_child(box)
	var t := create_tween()
	t.tween_interval(0.6) # after the loading screen has begun to fade
	t.tween_property(box, "modulate:a", 1.0, 1.4).set_trans(Tween.TRANS_SINE)
	t.tween_interval(4.0)
	t.tween_property(box, "modulate:a", 0.0, 2.5).set_trans(Tween.TRANS_SINE)
	t.tween_callback(box.queue_free)

## A place's title, eased in over a second, held, then eased away; one already showing fades out first.
## `leaving` shows it smaller and dimmer under "Leaving".
func show_location(title: String, leaving := false) -> void:
	if _loc_tw:
		_loc_tw.kill()
	var t := create_tween()
	if location.modulate.a > 0.0:
		t.tween_property(location, "modulate:a", 0.0, 0.5 * location.modulate.a)
	t.tween_callback(func() -> void:
		loc_sub.text = "Leaving" if leaving else ""
		loc_sub.visible = leaving
		loc_title.text = title
		loc_title.add_theme_font_size_override("font_size", 26 if leaving else 38)
		loc_title.add_theme_color_override("font_color", MUTED if leaving else ACCENT))
	t.tween_property(location, "modulate:a", 1.0, 1.2).set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_OUT)
	t.tween_interval(2.0 if leaving else 3.5)
	t.tween_property(location, "modulate:a", 0.0, 2.0).set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN)
	_loc_tw = t

## Zone lines: darken the screen over `secs` (0 for at once) and say where we are going.
func travel(text: String, secs := 0.6) -> void:
	travel_text.text = text
	travel_cover.visible = true
	if secs <= 0.0:
		travel_cover.modulate.a = 1.0
		return
	create_tween().tween_property(travel_cover, "modulate:a", 1.0, secs)

## Loading: show the screen at `frac` (0..1) with what is happening. The bar never goes backwards.
func loading(frac: float, text: String) -> void:
	if not loader.visible:
		loader.visible = true
		(loader.get_child(0) as Control).modulate.a = 1.0
		_load_frac = 0.0
		_load_shown = 0.0
		_load_since = 0.0
		_load_done = false
	_load_frac = maxf(_load_frac, clampf(frac, 0.0, 1.0))
	load_text.text = text

## Loaded: the bar runs to the end and the screen fades, once it has been up at least LOAD_MIN.
func loaded() -> void:
	if loader.visible:
		_load_frac = 1.0
		_load_done = true

func _process(delta: float) -> void:
	if not loader.visible or _load_done and _load_shown < 0.0:
		return
	_load_since += delta
	_load_shown = move_toward(_load_shown, minf(_load_frac, _load_since / LOAD_MIN), delta * 1.5)
	load_bar.size = Vector2(load_track.size.x * _load_shown, 4)
	if _load_done and _load_shown >= 1.0:
		_load_shown = -1.0 # fading
		var t := create_tween()
		t.tween_property(loader.get_child(0), "modulate:a", 0.0, 0.5).set_trans(Tween.TRANS_SINE)
		t.tween_callback(func() -> void: loader.visible = false)

## Arrived: lift the dark.
func arrive() -> void:
	var t := create_tween()
	t.tween_property(travel_cover, "modulate:a", 0.0, 0.8).set_trans(Tween.TRANS_SINE)
	t.tween_callback(func() -> void: travel_cover.visible = false)

## Small text under it: why something did not happen, a mode switched.
func notify(text: String) -> void:
	toast.text = text
	_toast_tw = _fade(toast, _toast_tw, 2.2)

func _fade(l: Label, tw: Tween, hold: float) -> Tween:
	if tw:
		tw.kill()
	l.modulate.a = 1.0
	var t := create_tween()
	t.tween_interval(hold)
	t.tween_property(l, "modulate:a", 0.0, 0.6)
	return t

func toggle_help() -> void:
	help.visible = not help.visible
	if help.visible:
		help.reset_size()
		help.position = (help.get_viewport_rect().size - help.size) / 2.0

func open_chat() -> void:
	chat_in.visible = true
	chat_hint.visible = false
	chat_box.add_theme_stylebox_override("panel", _box(Color(0.03, 0.035, 0.04, 0.8), EDGE, 8))
	chat_in.grab_focus()

func close_chat() -> void:
	chat_in.clear()
	chat_in.release_focus()
	chat_in.visible = false
	chat_hint.visible = true
	chat_box.add_theme_stylebox_override("panel", _box(Color(0.03, 0.035, 0.04, 0.35), Color(1, 1, 1, 0.0), 8))

## torch: 0 auto (lit in the dark), 1 always, 2 out; lit: burning now.
func set_abilities(torch: int, lit: bool, building: bool, map_open: bool, can_build: bool) -> void:
	var t: Slot = abilities["torch"]
	t.title = ["Torch", "Torch", "Torch"][torch]
	t.sub = ["auto", "on", "off"][torch]
	t.active = lit
	t.queue_redraw()
	(abilities["map"] as Slot).active = map_open
	(abilities["map"] as Slot).queue_redraw()
	var b: Slot = abilities["build"]
	b.visible = can_build
	b.active = building
	b.queue_redraw()

## Item name -> how rare it is ("rare", "epic", ...), for anything better than common (from /play/info).
var rarity := {}

func set_bag(contents: Array) -> void:
	for i in slots.size():
		var s := slots[i]
		var d: Dictionary = contents[i] if i < contents.size() else {}
		var item := String(d.get("item", ""))
		var count := int(d.get("count", 0))
		var col := Color(str(d["color"])) if item != "" else Color(0, 0, 0, 0)
		if s.title == item(item) and s.count == count and s.color == col:
			continue
		s.title = item(item)
		s.count = count
		s.color = col
		var rare := String(rarity.get(item, ""))
		var named := "%s (%s)" % [item(item), rare] if rare != "" else item(item)
		s.tooltip_text = "%s x%d  (%d to use, Shift+%d to drop)" % [named, count, i + 1, i + 1] if item != "" else "Empty"
		s.queue_redraw()

## The build bar: one button per block material, a picture of it and its key.
func set_materials(names: Array, swatches: Array) -> void:
	for b in _material_buttons:
		b.queue_free()
	_material_buttons.clear()
	for i in names.size():
		var b := Button.new()
		b.text = KEYS[i] if i < KEYS.size() else ""
		b.tooltip_text = String(names[i])
		b.icon = swatches[i]
		b.expand_icon = true
		b.add_theme_constant_override("icon_max_width", 32)
		b.add_theme_font_size_override("font_size", 12)
		b.custom_minimum_size = Vector2(60, 50)
		b.focus_mode = Control.FOCUS_NONE
		b.alignment = HORIZONTAL_ALIGNMENT_LEFT
		b.pressed.connect(func(): material_pressed.emit(i))
		materials.add_child(b)
		_material_buttons.append(b)
	# Where the action bar sits (the two never show together). Placed from the minimum size once the buttons are in.
	materials.set_anchors_and_offsets_preset.call_deferred(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE, 16)

## How many of each material the bag holds (-1, free): shown under its key, and a material you have none of dimmed.
func set_material_counts(counts: Array) -> void:
	if counts == _material_counts:
		return
	_material_counts = counts
	for i in mini(counts.size(), _material_buttons.size()):
		var b := _material_buttons[i]
		var n := int(counts[i])
		var key: String = KEYS[i] if i < KEYS.size() else ""
		b.text = key if n < 0 else "%s\n%d" % [key, n]
		b.modulate.a = 0.4 if n == 0 else 1.0

## Show the build bar (or not), with the chosen material marked.
func show_materials(on: bool, chosen: int) -> void:
	materials.visible = on and not _material_buttons.is_empty()
	if (chosen if on else -1) == _materials_shown:
		return
	_materials_shown = chosen if on else -1
	for i in _material_buttons.size():
		var b := _material_buttons[i]
		var picked := i == chosen
		var box := _box(INK, ACCENT if picked else Color(1, 1, 1, 0.18), 6)
		box.set_border_width_all(2 if picked else 1)
		for state in ["normal", "hover", "pressed"]:
			b.add_theme_stylebox_override(state, box)
		b.add_theme_color_override("font_color", ACCENT if picked else Color(1, 1, 1, 0.7))

## Action mode shows the action bar; build mode swaps it for the material bar with a chip naming the material.
func set_mode(building: bool, can_build: bool, material: String) -> void:
	var on := building and can_build
	bar.visible = not on
	spellbar.visible = not on and not spell_slots.is_empty()
	mode_chip.visible = on
	var key := "%s|%s" % [on, material]
	if key == _mode_shown:
		return
	_mode_shown = key
	mode_chip.text = "BUILDING   %s   click to place, hold X to break   (B to stop)" % material.to_upper()
	var box := _box(INK, ACCENT, 0)
	box.border_width_left = 4
	box.content_margin_left = 12
	box.content_margin_right = 12
	box.content_margin_top = 4
	box.content_margin_bottom = 4
	for state in ["normal", "hover", "pressed", "disabled"]:
		mode_chip.add_theme_stylebox_override(state, box)
	for c in ["font_color", "font_hover_color", "font_pressed_color"]:
		mode_chip.add_theme_color_override(c, ACCENT)
	mode_chip.set_anchors_and_offsets_preset.call_deferred(Control.PRESET_CENTER_BOTTOM, Control.PRESET_MODE_MINSIZE, 78)

func set_crosshair(on: bool) -> void:
	crosshair.visible = on

func say(line: String) -> void:
	chat_log.append_text(line + "\n")

func typing() -> bool:
	return chat_in.has_focus()

## A phase of the day in words (ui/src/daylight.ts describe).
static func time_words(phase: float, night: float) -> String:
	var day := 1.0 - night
	if phase >= day:
		return "Dusk" if phase < day + 0.03 else "Before dawn" if phase > 0.97 else "Night"
	var u := phase / day
	return "Dawn" if u < 0.06 else "Morning" if u < 0.4 else "Noon" if u < 0.6 else "Afternoon" if u < 0.94 else "Sunset"

# ---------------------------------------------------------------- the pieces

## A unit frame: a portrait with an initial, a name, bars, and badges underneath.
class Frame extends Control:
	const W := 290.0
	var title := ""
	var sub := ""
	var initial := ""
	var bars: Array = []
	var badges: Array = []
	var hostile := false

	func _init() -> void:
		custom_minimum_size = Vector2(W, 64)
		size = custom_minimum_size
		mouse_filter = Control.MOUSE_FILTER_IGNORE

	func set_data(t: String, s: String, i: String, b: Array, g: Array) -> void:
		if t == title and s == sub and i == initial and b == bars and g == badges:
			return
		title = t
		sub = s
		initial = i
		bars = b
		badges = g
		queue_redraw()

	var title_font: Font  # the HUD's title face, for the name and the portrait's letter

	func _draw() -> void:
		var font := get_theme_default_font()
		var h := 64.0
		draw_rect(Rect2(0, 0, W, h), INK)
		draw_rect(Rect2(0, 0, W, h), EDGE, false, 1.0)
		# The portrait.
		var pr := Rect2(6, 6, h - 12, h - 12)
		draw_rect(pr, Color("3a1f1a") if hostile else Color("23201b"))
		draw_rect(pr, Color("c8503c") if hostile else ACCENT, false, 1.5)
		var isz := 26
		var tf: Font = title_font if title_font else font
		var iw := tf.get_string_size(initial, HORIZONTAL_ALIGNMENT_LEFT, -1, isz).x
		draw_string(tf, pr.position + Vector2((pr.size.x - iw) / 2.0, pr.size.y / 2.0 + 9), initial, HORIZONTAL_ALIGNMENT_LEFT, -1, isz, Color("ff9a80") if hostile else ACCENT)
		var x := h
		var bw := W - x - 8
		draw_string(tf, Vector2(x, 20), title, HORIZONTAL_ALIGNMENT_LEFT, bw - 60, 15, TEXT)
		draw_string(font, Vector2(x, 20), sub, HORIZONTAL_ALIGNMENT_RIGHT, bw, 12, MUTED)
		var y := 27.0
		for i in bars.size():
			var b: Array = bars[i]
			var bh := 16.0 if i == 0 else 10.0
			draw_rect(Rect2(x, y, bw, bh), Color(0, 0, 0, 0.6))
			var col: Color = b[3]
			draw_rect(Rect2(x, y, bw * clampf(float(b[1]), 0.0, 1.0), bh), col.darkened(0.15))
			draw_rect(Rect2(x, y, bw * clampf(float(b[1]), 0.0, 1.0), bh * 0.4), col.lightened(0.15))
			draw_rect(Rect2(x, y, bw, bh), Color(0, 0, 0, 0.9), false, 1.0)
			if i == 0:
				draw_string_outline(font, Vector2(x, y + 12.5), String(b[2]), HORIZONTAL_ALIGNMENT_CENTER, bw, 12, 3, Color(0, 0, 0, 0.9))
				draw_string(font, Vector2(x, y + 12.5), String(b[2]), HORIZONTAL_ALIGNMENT_CENTER, bw, 12, TEXT)
			y += bh + 3.0
		# Badges under the frame, like buffs.
		var bx := 0.0
		for g in badges:
			var tw := font.get_string_size(String(g), HORIZONTAL_ALIGNMENT_LEFT, -1, 11).x + 12
			draw_rect(Rect2(bx, h + 4, tw, 18), INK)
			draw_rect(Rect2(bx, h + 4, tw, 18), ACCENT, false, 1.0)
			draw_string(font, Vector2(bx + 6, h + 17), String(g), HORIZONTAL_ALIGNMENT_LEFT, -1, 11, ACCENT)
			bx += tw + 4

## One square on the action bar: an item from the bag, or an ability.
class Slot extends Control:
	signal pressed
	signal hovered(on: bool)
	const SIDE := 46.0
	var key := ""
	var title := ""
	var sub := ""
	var count := 0
	var color := Color(0, 0, 0, 0)
	var glyph := false     # an ability: a word, not an item
	var cool := 0.0        # a spell's cooldown still to run, 0..1 (drawn as a shade falling away)
	var dim := false       # a spell that cannot be paid for now
	var active := false
	var _hover := false

	func _init() -> void:
		custom_minimum_size = Vector2(SIDE, SIDE)
		mouse_filter = Control.MOUSE_FILTER_STOP
		mouse_entered.connect(_set_hover.bind(true))
		mouse_exited.connect(_set_hover.bind(false))

	func _set_hover(on: bool) -> void:
		_hover = on
		hovered.emit(on)
		queue_redraw()

	func _gui_input(ev: InputEvent) -> void:
		if ev is InputEventMouseButton and ev.pressed and ev.button_index == MOUSE_BUTTON_LEFT:
			pressed.emit()
			accept_event()

	func _draw() -> void:
		var font := get_theme_default_font()
		var r := Rect2(Vector2.ZERO, size)
		draw_rect(r, INK)
		var empty := title == ""
		if not empty and not glyph:
			var inner := r.grow(-5)
			draw_rect(inner, color.darkened(0.45))
			draw_rect(Rect2(inner.position, Vector2(inner.size.x, inner.size.y * 0.45)), color.darkened(0.2))
			draw_rect(inner, color, false, 1.0)
		if glyph:
			var ty := size.y * 0.5 + (8.0 if sub == "" else 4.0)
			draw_string_outline(font, Vector2(0, ty), title, HORIZONTAL_ALIGNMENT_CENTER, size.x, 11, 3, Color(0, 0, 0, 0.9))
			draw_string(font, Vector2(0, ty), title, HORIZONTAL_ALIGNMENT_CENTER, size.x, 11, ACCENT if active else TEXT)
			if sub != "":
				draw_string(font, Vector2(0, ty + 11), sub, HORIZONTAL_ALIGNMENT_CENTER, size.x, 10, MUTED)
		elif not empty:
			var short := title if title.length() <= 7 else title.substr(0, 6) + "."
			draw_string_outline(font, Vector2(0, size.y * 0.5 + 6), short, HORIZONTAL_ALIGNMENT_CENTER, size.x, 11, 3, Color(0, 0, 0, 0.95))
			draw_string(font, Vector2(0, size.y * 0.5 + 6), short, HORIZONTAL_ALIGNMENT_CENTER, size.x, 11, TEXT)
			if count > 1:
				draw_string_outline(font, Vector2(0, size.y - 4), str(count), HORIZONTAL_ALIGNMENT_RIGHT, size.x - 4, 12, 3, Color(0, 0, 0, 0.95))
				draw_string(font, Vector2(0, size.y - 4), str(count), HORIZONTAL_ALIGNMENT_RIGHT, size.x - 4, 12, Color.WHITE)
		if glyph and color.a > 0.0:
			draw_rect(Rect2(3, size.y - 6, size.x - 6, 3), color.darkened(0.55) if dim else color)
		if dim:
			draw_rect(r, Color(0, 0, 0, 0.45))
		if cool > 0.0:
			draw_rect(Rect2(0, 0, size.x, size.y * cool), Color(0, 0, 0, 0.55))
		draw_string_outline(font, Vector2(4, 12), key, HORIZONTAL_ALIGNMENT_LEFT, -1, 10, 3, Color(0, 0, 0, 0.9))
		draw_string(font, Vector2(4, 12), key, HORIZONTAL_ALIGNMENT_LEFT, -1, 10, Color(1, 1, 1, 0.75))
		var edge := ACCENT if active else (Color(1, 1, 1, 0.45) if _hover else Color(1, 1, 1, 0.16))
		draw_rect(r, edge, false, 2.0 if active else 1.0)

## The minimap: a round window on the world map, north up, you in the middle pointing the way you are going.
## The mouse wheel over it zooms.
class Minimap extends Control:
	const SIDE := 180.0
	var tex: ImageTexture
	var world := 1024.0
	var me := Vector2.ZERO
	var facing := 0.0
	var others: Array = []
	var span := 240.0       # world units from the middle to the rim

	func _init() -> void:
		custom_minimum_size = Vector2(SIDE, SIDE)
		mouse_filter = Control.MOUSE_FILTER_STOP
		tooltip_text = "Minimap: scroll to zoom, M for the world map"

	func _gui_input(ev: InputEvent) -> void:
		if ev is InputEventMouseButton and ev.pressed:
			if ev.button_index == MOUSE_BUTTON_WHEEL_UP:
				span = maxf(span / 1.25, 40.0)
			elif ev.button_index == MOUSE_BUTTON_WHEEL_DOWN:
				span = minf(span * 1.25, maxf(world / 2.0, 200.0))
			else:
				return
			accept_event()
			queue_redraw()

	func _draw() -> void:
		var c := Vector2(SIDE, SIDE) / 2.0
		var rad := SIDE / 2.0 - 4.0
		var pts := PackedVector2Array()
		var uvs := PackedVector2Array()
		for i in 64:
			var a := TAU * i / 64.0
			var d := Vector2(cos(a), sin(a))
			pts.append(c + d * rad)
			uvs.append((me + d * span) / world)
		draw_circle(c, rad + 4.0, Color(0.03, 0.03, 0.03, 0.9))
		if tex:
			draw_colored_polygon(pts, Color(1, 1, 1), uvs, tex)
		else:
			draw_colored_polygon(pts, Color(0.2, 0.25, 0.18))
		var k := rad / span
		for p in others:
			var o: Vector2 = (p - me) * k
			if o.length() < rad - 3.0:
				draw_circle(c + o, 3.0, Color(0, 0, 0, 0.8))
				draw_circle(c + o, 2.2, ACCENT)
		var fwd := Vector2(-sin(facing), -cos(facing))
		var right := Vector2(fwd.y, -fwd.x)
		var arrow := PackedVector2Array([c + fwd * 9.0, c - fwd * 5.0 + right * 5.5, c - fwd * 2.5, c - fwd * 5.0 - right * 5.5])
		draw_colored_polygon(arrow, Color.WHITE)
		arrow.append(arrow[0])
		draw_polyline(arrow, Color(0, 0, 0, 0.9), 1.5)
		draw_arc(c, rad + 1.0, 0.0, TAU, 72, Color("2a2218"), 5.0, true)
		draw_arc(c, rad + 3.5, 0.0, TAU, 72, EDGE, 1.5, true)
		draw_arc(c, rad - 1.5, 0.0, TAU, 72, Color(0, 0, 0, 0.6), 1.0, true)
		var font := get_theme_default_font()
		draw_circle(c + Vector2(0, -rad - 1.0), 9.0, Color("2a2218"))
		draw_arc(c + Vector2(0, -rad - 1.0), 9.0, 0.0, TAU, 24, EDGE, 1.0, true)
		draw_string(font, c + Vector2(-9, -rad + 3.5), "N", HORIZONTAL_ALIGNMENT_CENTER, 18, 12, ACCENT)

extends CanvasLayer
## The group finder (F5): the realm's dungeons with their level, boss and group size, a role to queue as, and how
## the queue stands. Queueing is "/queue Name role" and leaving "/unqueue" (crates/server/src/finder.rs); when the
## finder has a group the server makes it a party and puts it at the dungeon's door. The queue's state comes on chat
## channel 16 ("Q|dungeon|queued|wanted|role", "Q|" once out).

signal command(text: String)

const HUD := preload("res://scripts/hud.gd")
const BagScript := preload("res://scripts/bag.gd")
const ROLES := ["tank", "healer", "damage"]

var panel := PanelContainer.new()
var role := "damage"
var _dungeons: Array = []          # [[Vector2, name, level, boss, foes, group], ...] (main.gd _dungeons)
var _queued := []                  # [dungeon, queued, wanted, role] while in a queue, else empty
var _since := 0.0                  # seconds in the queue
var _list := VBoxContainer.new()
var _status := Label.new()
var _role_buttons: Array[Button] = []

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	panel.add_theme_stylebox_override("panel", BagScript.box())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(620, 0)
	panel.position = Vector2(-310, -220)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	v.add_child(BagScript.title("Group finder", 26, false))
	var about := Label.new()
	about.text = "Queue for a dungeon and the finder makes you a company, a tank and a healer first, and takes you to its door. Your party queues with you."
	about.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	about.add_theme_color_override("font_color", HUD.TEXT)
	about.add_theme_font_size_override("font_size", 13)
	v.add_child(about)
	var roles := HBoxContainer.new()
	roles.add_theme_constant_override("separation", 6)
	var rl := Label.new()
	rl.text = "Queue as"
	rl.add_theme_color_override("font_color", HUD.MUTED)
	roles.add_child(rl)
	for r in ROLES:
		var b := Button.new()
		b.text = String(r).capitalize()
		b.toggle_mode = true
		b.focus_mode = Control.FOCUS_NONE
		b.button_pressed = r == role
		b.pressed.connect(func(): _pick_role(r))
		roles.add_child(b)
		_role_buttons.append(b)
	v.add_child(roles)
	_list.add_theme_constant_override("separation", 6)
	v.add_child(_list)
	_status.add_theme_color_override("font_color", HUD.ACCENT)
	_status.add_theme_font_size_override("font_size", 15)
	v.add_child(_status)
	var hint := Label.new()
	hint.text = "F5 or Esc to close. The queue keeps going with this closed."
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	v.add_child(hint)

func _pick_role(r: String) -> void:
	role = r
	for i in _role_buttons.size():
		_role_buttons[i].button_pressed = ROLES[i] == r

## The dungeons, from /play/info (main.gd works them out).
func set_dungeons(list: Array) -> void:
	if list != _dungeons:
		_dungeons = list
		_rebuild()

## A line on chat channel 16.
func on_line(text: String) -> void:
	var f := text.split("|")
	var was := not _queued.is_empty()
	_queued = [f[1], int(f[2]), int(f[3]), f[4]] if f.size() >= 5 else []
	if not _queued.is_empty() and not was:
		_since = 0.0
	_rebuild()

func queued() -> bool:
	return not _queued.is_empty()

## Each frame: the time in the queue.
func tick(delta: float) -> void:
	if _queued.is_empty():
		return
	_since += delta
	if panel.visible:
		_show_status()

func _rebuild() -> void:
	for c in _list.get_children():
		c.queue_free()
	if _dungeons.is_empty():
		var none := Label.new()
		none.text = "There are no dungeons in this world."
		none.add_theme_color_override("font_color", HUD.MUTED)
		_list.add_child(none)
	for d in _dungeons:
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 10)
		var l := Label.new()
		l.text = "%s   level %d%s, a group of %d" % [d[1], d[2], (", " + String(d[3])) if String(d[3]) != "" else "", int(d[5])]
		l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		l.add_theme_color_override("font_color", HUD.TEXT)
		row.add_child(l)
		var name: String = d[1]
		var mine: bool = not _queued.is_empty() and _queued[0] == name
		var b := Button.new()
		b.text = "Leave queue" if mine else "Queue"
		b.focus_mode = Control.FOCUS_NONE
		b.pressed.connect(func(): command.emit("/unqueue" if mine else "/queue %s %s" % [name, role]))
		row.add_child(b)
		_list.add_child(row)
	_show_status()

func _show_status() -> void:
	if _queued.is_empty():
		_status.text = "Not queued."
	else:
		_status.text = "Queued for %s as %s: %d of %d found, %d:%02d waiting" % [_queued[0], _queued[3], _queued[1], _queued[2], int(_since) / 60, int(_since) % 60]

func toggle() -> void:
	panel.visible = not panel.visible
	_rebuild()

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

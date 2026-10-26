extends CanvasLayer
## Trading with another player: an invitation when someone asks, then both sides of the table, what is in your bag
## to put on it, and the money. The server keeps the table (a `channel::TRADE` line from the other player each
## time it changes: "?" they ask, "-" it is over, else "mine|coin|ready|theirs|coin|ready") and makes the swap
## when both have accepted, so this only shows it and passes on what you choose.

signal trade_op(op: int, slot: int, target: int)   # proto::game::trade_op, sent as ACT_TRADE

const HUD := preload("res://scripts/hud.gd")
const Bag := preload("res://scripts/bag.gd")
const OP_ASK := 0
const OP_OFFER := 1
const OP_TAKE_BACK := 2
const OP_COIN := 3
const OP_READY := 4
const OP_CANCEL := 5

var panel := PanelContainer.new()
var ask := PanelContainer.new()
var _ask_text := Label.new()
var _asker := 0
var _title := Label.new()
var _mine := VBoxContainer.new()
var _theirs := VBoxContainer.new()
var _goods := VBoxContainer.new()
var _coin := SpinBox.new()
var _their_coin := Label.new()
var _state := Label.new()
var _accept := Button.new()
var _items: Array = []
var _currency := ""
var _partner := 0
var _table: Array = []            # [mine stacks, my coin, my ready, their stacks, their coin, their ready]

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	panel.add_theme_stylebox_override("panel", Bag.box())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(860, 440)
	panel.position = Vector2(-430, -250)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	_title.add_theme_font_override("font", HUD.TITLE)
	_title.add_theme_font_size_override("font_size", 24)
	_title.add_theme_color_override("font_color", HUD.ACCENT)
	v.add_child(_title)
	var cols := HBoxContainer.new()
	cols.add_theme_constant_override("separation", 24)
	cols.size_flags_vertical = Control.SIZE_EXPAND_FILL
	v.add_child(cols)
	for pair in [["Your bag", _goods], ["You give", _mine], ["You get", _theirs]]:
		var c := VBoxContainer.new()
		c.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		c.add_child(Bag.title(pair[0], 16, false))
		(pair[1] as VBoxContainer).add_theme_constant_override("separation", 4)
		c.add_child(pair[1])
		cols.add_child(c)
		if pair[1] == _mine:
			var m := HBoxContainer.new()
			_coin.max_value = 999999
			_coin.custom_minimum_size = Vector2(110, 0)
			m.add_child(_coin)
			var give := Button.new()
			give.text = "Offer money"
			give.focus_mode = Control.FOCUS_NONE
			give.pressed.connect(func(): trade_op.emit(OP_COIN, 0, int(_coin.value)))
			m.add_child(give)
			c.add_child(m)
		elif pair[1] == _theirs:
			_their_coin.add_theme_color_override("font_color", HUD.ACCENT)
			c.add_child(_their_coin)
	_state.add_theme_color_override("font_color", HUD.TEXT)
	v.add_child(_state)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	_accept.text = "Accept"
	_accept.focus_mode = Control.FOCUS_NONE
	_accept.pressed.connect(func(): trade_op.emit(OP_READY, 0, 0))
	row.add_child(_accept)
	var off := Button.new()
	off.text = "Call it off"
	off.focus_mode = Control.FOCUS_NONE
	off.pressed.connect(func(): trade_op.emit(OP_CANCEL, 0, 0))
	row.add_child(off)
	v.add_child(row)
	# Someone asks: a small question at the top of the screen.
	ask.visible = false
	ask.add_theme_stylebox_override("panel", Bag.box())
	ask.set_anchors_preset(Control.PRESET_CENTER_TOP)
	ask.position = Vector2(-230, 90)
	ask.custom_minimum_size = Vector2(460, 0)
	add_child(ask)
	var a := VBoxContainer.new()
	ask.add_child(a)
	_ask_text.add_theme_color_override("font_color", HUD.TEXT)
	a.add_child(_ask_text)
	var ar := HBoxContainer.new()
	ar.add_theme_constant_override("separation", 8)
	for pair in [["Trade", true], ["No", false]]:
		var b := Button.new()
		b.text = pair[0]
		b.focus_mode = Control.FOCUS_NONE
		var yes: bool = pair[1]
		b.pressed.connect(func():
			ask.visible = false
			trade_op.emit(OP_ASK if yes else OP_CANCEL, 0, _asker if yes else 0))
		ar.add_child(b)
	a.add_child(ar)

func set_items(items: Array, currency: String) -> void:
	_items = items
	_currency = currency

## A TRADE line from `from` (named `who`).
func on_line(from: int, who: String, text: String, bag: Array) -> void:
	if text == "?":
		_asker = from
		_ask_text.text = "%s would trade with you." % who
		ask.visible = true
		return
	if text == "-":
		panel.visible = false
		_partner = 0
		return
	var p := text.split("|")
	if p.size() != 6:
		return
	_partner = from
	_table = [_stacks(p[0]), int(p[1]), p[2] == "1", _stacks(p[3]), int(p[4]), p[5] == "1"]
	_title.text = "Trading with %s" % who
	panel.visible = true
	ask.visible = false
	refresh(bag)

func _stacks(s: String) -> Array:
	var out := []
	for part in s.split(",", false):
		var kv := part.split(":")
		if kv.size() == 2:
			out.append([int(kv[0]), int(kv[1])])
	return out

func _item(tag: int) -> Dictionary:
	return _items[tag - 1] if tag >= 1 and tag <= _items.size() else {"name": "?", "color": "#888888"}

func refresh(bag: Array) -> void:
	if not panel.visible or _table.is_empty():
		return
	for box in [_mine, _theirs, _goods]:
		for c in box.get_children():
			c.queue_free()
	for k in _table[0].size():
		var st: Array = _table[0][k]
		var idx: int = k
		_mine.add_child(_row(_item(st[0]), st[1], "Back", func(): trade_op.emit(OP_TAKE_BACK, idx, 0)))
	for st in _table[3]:
		_theirs.add_child(_row(_item(st[0]), st[1], "", Callable()))
	for i in mini(bag.size(), Bag.SLOTS):
		var d: Dictionary = bag[i]
		var name := String(d.get("item", ""))
		if name == "" or name == _currency:
			continue
		var slot := i
		_goods.add_child(_row({"name": name, "color": String(d.get("color", "#888888"))}, int(d.get("count", 0)), "Offer", func(): trade_op.emit(OP_OFFER, slot, 0)))
	_their_coin.text = "and %d %s" % [_table[4], _currency] if _currency != "" else ""
	var me_ready: bool = _table[2]
	var they_ready: bool = _table[5]
	_state.text = "Both have accepted." if me_ready and they_ready else ("You have accepted. Waiting for them." if me_ready else ("They have accepted. Your turn." if they_ready else "Put things on the table, then accept. Any change means both accept again."))
	_accept.disabled = me_ready
	_coin.editable = true

func _row(it: Dictionary, n: int, verb: String, on_press: Callable) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	var sw := ColorRect.new()
	sw.color = Color(String(it.get("color", "#888888")))
	sw.custom_minimum_size = Vector2(4, 22)
	row.add_child(sw)
	var l := Label.new()
	l.text = "%s  x%d" % [HUD.item(String(it.get("name", "?"))), n]
	l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	l.add_theme_color_override("font_color", HUD.TEXT)
	row.add_child(l)
	if verb != "":
		var b := Button.new()
		b.text = verb
		b.focus_mode = Control.FOCUS_NONE
		b.custom_minimum_size = Vector2(64, 24)
		b.pressed.connect(on_press)
		row.add_child(b)
	return row

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if ask.visible:
		ask.visible = false
		trade_op.emit(OP_CANCEL, 0, 0)
		return true
	if panel.visible:
		trade_op.emit(OP_CANCEL, 0, 0)
		return true
	return false

extends CanvasLayer
## The bag (F4): every slot of it, the bank beside it, and what can be done with the one picked. Click a thing to
## pick it, then another slot to move it there (bag to bank and back too); right click uses it. The bag and the bank
## come in one list from the server (the bank's slots after the bag's), which checks everything; the bank opens only
## in towns and near merchants, and the server says so when it will not.

signal bag_op(op: int, slot: int, other: int)   # proto::game::bag_op, sent as ACT_BAG
signal offered(slot: int)                        # put into the trade that is open
signal auction_requested

const HUD := preload("res://scripts/hud.gd")
const SLOTS := 24
const BANK := 100                 # picked bank slots are BANK + slot
const OP_USE := 0
const OP_DROP := 1
const OP_MOVE := 2
const OP_SPLIT := 3
const OP_DEPOSIT := 4
const OP_WITHDRAW := 5
const OP_BANK_MOVE := 6

var panel := PanelContainer.new()
var trading := false              # a trade is open: picked things can be offered
var _money := Label.new()
var _bag_grid := GridContainer.new()
var _bank_grid := GridContainer.new()
var _about := Label.new()
var _buttons := HBoxContainer.new()
var _slots: Array[Button] = []
var _picked := -1
var _list: Array = []
var _items: Array = []            # /play/info items: tag - 1 -> {name, color, value, rarity}
var _currency := ""

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	panel.add_theme_stylebox_override("panel", box())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(980, 470)
	panel.position = Vector2(-490, -260)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	var head := HBoxContainer.new()
	head.add_child(title("Bag", 26, true))
	_money.add_theme_color_override("font_color", HUD.ACCENT)
	_money.add_theme_font_size_override("font_size", 15)
	head.add_child(_money)
	v.add_child(head)
	var cols := HBoxContainer.new()
	cols.add_theme_constant_override("separation", 28)
	v.add_child(cols)
	for pair in [["Carried", _bag_grid, 0], ["The bank (in towns and near merchants)", _bank_grid, BANK]]:
		var c := VBoxContainer.new()
		c.add_child(title(pair[0], 16, false))
		var g: GridContainer = pair[1]
		g.columns = 6
		g.add_theme_constant_override("h_separation", 4)
		g.add_theme_constant_override("v_separation", 4)
		for i in SLOTS:
			var b := Button.new()
			b.custom_minimum_size = Vector2(72, 50)
			b.focus_mode = Control.FOCUS_NONE
			b.clip_text = true
			b.add_theme_font_size_override("font_size", 11)
			var at: int = pair[2] + i
			b.pressed.connect(func(): _press(at))
			b.gui_input.connect(func(ev: InputEvent):
				if ev is InputEventMouseButton and ev.pressed and ev.button_index == MOUSE_BUTTON_RIGHT and at < BANK:
					bag_op.emit(OP_USE, at, 0))
			g.add_child(b)
			_slots.append(b)
		c.add_child(g)
		cols.add_child(c)
	_about.add_theme_color_override("font_color", HUD.TEXT)
	v.add_child(_about)
	_buttons.add_theme_constant_override("separation", 8)
	v.add_child(_buttons)
	var hint := Label.new()
	hint.text = "Click a thing, then a slot to move it there. Right click uses it. I, F4 or Esc to close."
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	v.add_child(hint)

static func box() -> StyleBoxFlat:
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.96)
	b.border_color = HUD.EDGE
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(18)
	return b

static func title(text: String, size: int, grow: bool) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_font_override("font", HUD.TITLE)
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", HUD.ACCENT)
	if grow:
		l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	return l

## The game's items and money (from /play/info).
func set_items(items: Array, currency: String) -> void:
	_items = items
	_currency = currency

func toggle(bag: Array) -> void:
	panel.visible = not panel.visible
	_picked = -1
	refresh(bag)

func refresh(bag: Array) -> void:
	if not panel.visible:
		return
	_list = bag
	var money := 0
	for i in _slots.size():
		var at := i if i < SLOTS else BANK + i - SLOTS
		var d: Dictionary = bag[i] if i < bag.size() else {}
		var name := String(d.get("item", ""))
		var n := int(d.get("count", 0))
		if i < SLOTS and name == _currency and name != "":
			money += n
		var b := _slots[i]
		b.text = "" if name == "" else ("%s\n%d" % [HUD.item(name), n] if n > 1 else HUD.item(name))
		b.tooltip_text = HUD.item(name)
		var s := StyleBoxFlat.new()
		s.bg_color = Color(0.09, 0.09, 0.1, 0.95) if name == "" else Color(d.get("color", "#888888")).darkened(0.62)
		s.border_color = HUD.ACCENT if at == _picked else Color(1, 1, 1, 0.08)
		s.set_border_width_all(2 if at == _picked else 1)
		s.set_corner_radius_all(3)
		for k in ["normal", "hover", "pressed"]:
			b.add_theme_stylebox_override(k, s)
		b.add_theme_color_override("font_color", HUD.TEXT)
	_money.text = "%d %s" % [money, HUD.item(_currency)] if _currency != "" else ""
	_show_picked()

func _entry(at: int) -> Dictionary:
	var i := at if at < BANK else SLOTS + at - BANK
	return _list[i] if i >= 0 and i < _list.size() else {}

func _press(at: int) -> void:
	var here := String(_entry(at).get("item", ""))
	if _picked < 0:
		_picked = at if here != "" else -1
	elif _picked == at:
		_picked = -1
	else:
		var from := _picked
		_picked = -1
		if from < BANK and at < BANK:
			bag_op.emit(OP_MOVE, from, at)
		elif from >= BANK and at >= BANK:
			bag_op.emit(OP_BANK_MOVE, from - BANK, at - BANK)
		elif from < BANK:
			bag_op.emit(OP_DEPOSIT, from, 0)
		else:
			bag_op.emit(OP_WITHDRAW, from - BANK, 0)
	refresh(_list)

func _show_picked() -> void:
	for c in _buttons.get_children():
		c.queue_free()
	var d := _entry(_picked) if _picked >= 0 else {}
	if d.is_empty() or String(d.get("item", "")) == "":
		_about.text = "Nothing picked."
		_button("Auction house", func(): auction_requested.emit())
		return
	var name := String(d["item"])
	var info: Dictionary = {}
	for it in _items:
		if String(it.get("name", "")) == name:
			info = it
	var bits := ["%s x%d" % [name, int(d["count"])]]
	if info.has("rarity"):
		bits.append(String(info["rarity"]))
	if int(info.get("value", 0)) > 0:
		bits.append("worth %d to a merchant" % int(info["value"]))
	_about.text = ",  ".join(bits)
	var at := _picked
	if at >= BANK:
		_button("Take out", func(): _do(OP_WITHDRAW, at - BANK))
		return
	_button("Use", func(): _do(OP_USE, at))
	_button("Split", func(): _do(OP_SPLIT, at))
	_button("Into the bank", func(): _do(OP_DEPOSIT, at))
	if trading and name != _currency:
		_button("Offer in trade", func():
			_picked = -1
			offered.emit(at))
	_button("Drop", func(): _do(OP_DROP, at))
	_button("Auction house", func(): auction_requested.emit())

func _do(op: int, slot: int) -> void:
	_picked = -1
	bag_op.emit(op, slot, 0)

func _button(text: String, on_press: Callable) -> void:
	var b := Button.new()
	b.text = text
	b.focus_mode = Control.FOCUS_NONE
	b.custom_minimum_size = Vector2(0, 28)
	b.pressed.connect(on_press)
	_buttons.add_child(b)

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

extends CanvasLayer
## The auction house, in the towns and at merchants: what others have put up (by item, cheapest first), your own
## listings, and things from your bag to sell at a price you set. Listings last two days; what does not sell, and
## the money for what does, is brought to you by the server when you are about with room in your bag. The server
## answers with `channel::AUCTION` lines ("H page pages total mine", then "R id tag count price secs mine seller"
## each, then "E"), which `on_line` gathers into a page.

signal auction_op(op: int, a: int, target: int)    # proto::game::auction_op, sent as ACT_AUCTION

const HUD := preload("res://scripts/hud.gd")
const Bag := preload("res://scripts/bag.gd")
const OP_BROWSE := 0
const OP_BUY := 1
const OP_SELL := 2
const OP_CANCEL := 3
const OP_MINE := 4

var panel := PanelContainer.new()
var _money := Label.new()
var _filter := OptionButton.new()
var _rows := VBoxContainer.new()
var _pager := Label.new()
var _tabs: Array[Button] = []
var _tab := 0                     # 0 browse, 1 yours, 2 sell
var _page := 0
var _pages := 1
var _incoming: Array = []
var _items: Array = []
var _currency := ""
var _bag: Array = []

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	panel.add_theme_stylebox_override("panel", Bag.box())
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(820, 500)
	panel.position = Vector2(-410, -270)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	var head := HBoxContainer.new()
	head.add_child(Bag.title("The auction house", 26, true))
	_money.add_theme_color_override("font_color", HUD.ACCENT)
	_money.add_theme_font_size_override("font_size", 15)
	head.add_child(_money)
	v.add_child(head)
	var bar := HBoxContainer.new()
	bar.add_theme_constant_override("separation", 8)
	for i in 3:
		var b := Button.new()
		b.text = ["For sale", "Yours", "Sell"][i]
		b.toggle_mode = true
		b.focus_mode = Control.FOCUS_NONE
		var k := i
		b.pressed.connect(func(): _show_tab(k))
		bar.add_child(b)
		_tabs.append(b)
	var gap := Control.new()
	gap.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	bar.add_child(gap)
	_filter.focus_mode = Control.FOCUS_NONE
	_filter.item_selected.connect(func(_i: int):
		_page = 0
		_ask())
	bar.add_child(_filter)
	v.add_child(bar)
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_rows.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_rows.add_theme_constant_override("separation", 4)
	scroll.add_child(_rows)
	v.add_child(scroll)
	var foot := HBoxContainer.new()
	foot.add_theme_constant_override("separation", 8)
	for pair in [["<", -1], [">", 1]]:
		var b := Button.new()
		b.text = pair[0]
		b.focus_mode = Control.FOCUS_NONE
		var step: int = pair[1]
		b.pressed.connect(func():
			_page = clampi(_page + step, 0, _pages - 1)
			_ask())
		foot.add_child(b)
		if step < 0:
			_pager.add_theme_color_override("font_color", HUD.MUTED)
			foot.add_child(_pager)
	var hint := Label.new()
	hint.text = "  Listings last two days. Money and unsold things come to your bag. Esc to close."
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	foot.add_child(hint)
	v.add_child(foot)

func set_items(items: Array, currency: String) -> void:
	_items = items
	_currency = currency
	_filter.clear()
	_filter.add_item("Everything", 0)
	for i in items.size():
		var n := String(items[i].get("name", ""))
		if n != currency:
			_filter.add_item(n, i + 1)

func open(bag: Array) -> void:
	panel.visible = true
	_bag = bag
	_show_tab(0)

func _show_tab(k: int) -> void:
	_tab = k
	_page = 0
	for i in _tabs.size():
		_tabs[i].button_pressed = i == k
	_filter.visible = k == 0
	if k == 2:
		_draw_sell()
	else:
		_clear()
		_rows.add_child(_note("Asking... (the auction house answers in towns and near merchants)"))
		_ask()

func _ask() -> void:
	if _tab == 0:
		auction_op.emit(OP_BROWSE, _page, _filter.get_selected_id() if _filter.selected >= 0 else 0)
	elif _tab == 1:
		auction_op.emit(OP_MINE, _page, 0)

## An AUCTION line from the server.
func on_line(text: String) -> void:
	var p := text.split(" ", true, 7)
	match p[0]:
		"H":
			_incoming.clear()
			if p.size() >= 3:
				_page = int(p[1])
				_pages = maxi(1, int(p[2]))
		"R":
			if p.size() >= 8:
				_incoming.append({"id": int(p[1]), "tag": int(p[2]), "count": int(p[3]), "price": int(p[4]), "secs": int(p[5]), "mine": p[6] == "1", "seller": p[7]})
		"E":
			if panel.visible and _tab != 2:
				_draw_rows()

func _item(tag: int) -> Dictionary:
	return _items[tag - 1] if tag >= 1 and tag <= _items.size() else {"name": "?", "color": "#888888"}

func _clear() -> void:
	for c in _rows.get_children():
		c.queue_free()

func _draw_rows() -> void:
	_clear()
	_pager.text = "page %d of %d" % [_page + 1, _pages]
	if _incoming.is_empty():
		_rows.add_child(_note("Nothing up for sale." if _tab == 0 else "You have nothing up for sale."))
	for r in _incoming:
		var it := _item(r["tag"])
		var left: int = r["secs"]
		var when := "%dh" % (left / 3600) if left >= 3600 else "%dm" % maxi(1, left / 60)
		var id: int = r["id"]
		var mine: bool = r["mine"]
		var row := _row(it, "%s  x%d" % [String(it.get("name", "?")), r["count"]])
		row.add_child(_cell(String(r["seller"]), 150, HUD.MUTED))
		row.add_child(_cell(when, 44, HUD.MUTED))
		row.add_child(_cell("%d" % r["price"], 70, HUD.ACCENT))
		row.add_child(_btn("Take down" if mine else "Buy", func(): auction_op.emit(OP_CANCEL if mine else OP_BUY, 0, id)))
		_rows.add_child(row)

func _draw_sell() -> void:
	_clear()
	_pager.text = ""
	var any := false
	for i in mini(_bag.size(), Bag.SLOTS):
		var d: Dictionary = _bag[i]
		var name := String(d.get("item", ""))
		if name == "" or name == _currency:
			continue
		any = true
		var value := 0
		for it in _items:
			if String(it.get("name", "")) == name:
				value = int(it.get("value", 0))
		var row := _row({"name": name, "color": String(d.get("color", "#888888"))}, "%s  x%d" % [HUD.item(name), int(d.get("count", 0))])
		var price := SpinBox.new()
		price.min_value = 1
		price.max_value = 999999
		price.value = maxi(1, value * 2 * int(d.get("count", 1)))
		price.custom_minimum_size = Vector2(110, 0)
		row.add_child(price)
		var slot := i
		row.add_child(_btn("Put up", func(): auction_op.emit(OP_SELL, slot, int(price.value))))
		_rows.add_child(row)
	if not any:
		_rows.add_child(_note("Nothing in your bag to sell."))

## Keep the money and the sell list up to date with the bag.
func refresh(bag: Array) -> void:
	if not panel.visible:
		return
	var changed := bag.size() != _bag.size()
	if not changed:
		for i in bag.size():
			if String(bag[i].get("item", "")) != String(_bag[i].get("item", "")) or int(bag[i].get("count", 0)) != int(_bag[i].get("count", 0)):
				changed = true
				break
	_bag = bag
	var money := 0
	for i in mini(bag.size(), Bag.SLOTS):
		if String(bag[i].get("item", "")) == _currency and _currency != "":
			money += int(bag[i].get("count", 0))
	_money.text = "%d %s" % [money, _currency] if _currency != "" else ""
	if changed and _tab == 2:
		_draw_sell()
	elif changed:
		_ask()

func _row(it: Dictionary, text: String) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	var sw := ColorRect.new()
	sw.color = Color(String(it.get("color", "#888888")))
	sw.custom_minimum_size = Vector2(4, 24)
	row.add_child(sw)
	var l := _cell(text, 0, HUD.TEXT)
	l.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(l)
	return row

func _cell(text: String, width: int, col: Color) -> Label:
	var l := Label.new()
	l.text = text
	l.clip_text = true
	l.custom_minimum_size = Vector2(width, 0)
	l.add_theme_color_override("font_color", col)
	return l

func _btn(text: String, on_press: Callable) -> Button:
	var b := Button.new()
	b.text = text
	b.focus_mode = Control.FOCUS_NONE
	b.custom_minimum_size = Vector2(84, 26)
	b.pressed.connect(on_press)
	return b

func _note(text: String) -> Label:
	var l := Label.new()
	l.text = text
	l.add_theme_color_override("font_color", HUD.MUTED)
	return l

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

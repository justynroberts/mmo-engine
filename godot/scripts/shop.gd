extends CanvasLayer
## A merchant's shop: what they sell (Buy) and what in your bag they will buy (Sell), with your money. The goods
## and prices come from /play/info "shops" and "items"; the server checks every sale. main.gd opens it when you
## talk (F) to someone with a shop, and passes on what you choose.

signal bought(index: int)
signal sold(bag_slot: int)

const HUD := preload("res://scripts/hud.gd")

var panel := PanelContainer.new()
var _title := Label.new()
var _money := Label.new()
var _buy := VBoxContainer.new()
var _sell := VBoxContainer.new()
var _goods: Array = []          # [{item, price}]
var _values := {}               # item name -> what a merchant pays
var _colors := {}               # item name -> colour
var _currency := ""

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	panel.visible = false
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.96)
	b.border_color = HUD.EDGE
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(18)
	panel.add_theme_stylebox_override("panel", b)
	panel.set_anchors_preset(Control.PRESET_CENTER)
	panel.custom_minimum_size = Vector2(640, 420)
	panel.position = Vector2(-320, -240)
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	panel.add_child(v)
	var head := HBoxContainer.new()
	_title.add_theme_font_override("font", HUD.TITLE)
	_title.add_theme_font_size_override("font_size", 26)
	_title.add_theme_color_override("font_color", HUD.ACCENT)
	_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	head.add_child(_title)
	_money.add_theme_color_override("font_color", HUD.ACCENT)
	_money.add_theme_font_size_override("font_size", 15)
	head.add_child(_money)
	v.add_child(head)
	var cols := HBoxContainer.new()
	cols.add_theme_constant_override("separation", 24)
	v.add_child(cols)
	for pair in [["For sale", _buy], ["Yours to sell", _sell]]:
		var c := VBoxContainer.new()
		c.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		var h := Label.new()
		h.text = pair[0]
		h.add_theme_font_override("font", HUD.TITLE)
		h.add_theme_font_size_override("font_size", 17)
		h.add_theme_color_override("font_color", HUD.ACCENT)
		c.add_child(h)
		(pair[1] as VBoxContainer).add_theme_constant_override("separation", 4)
		c.add_child(pair[1])
		cols.add_child(c)
	var hint := Label.new()
	hint.text = "Esc to close."
	hint.add_theme_font_size_override("font_size", 12)
	hint.add_theme_color_override("font_color", HUD.MUTED)
	v.add_child(hint)

## The game's items and money (from /play/info).
func set_items(items: Array, currency: String) -> void:
	_currency = currency
	for it in items:
		_values[String(it["name"])] = int(it.get("value", 0))
		_colors[String(it["name"])] = Color.html(String(it.get("color", "#cccccc")))

## Open on a merchant: their name and goods, and our bag ([{slot, item, color, count}]).
func open(merchant: String, goods: Array, bag: Array) -> void:
	_title.text = merchant
	_goods = goods
	panel.visible = true
	refresh(bag)

func refresh(bag: Array) -> void:
	if not panel.visible:
		return
	var money := 0
	for b in bag:
		if String(b["item"]) == _currency:
			money += int(b["count"])
	_money.text = "%d %s" % [money, _currency]
	for c in _buy.get_children():
		c.queue_free()
	for c in _sell.get_children():
		c.queue_free()
	for i in _goods.size():
		var g: Dictionary = _goods[i]
		_buy.add_child(_row(String(g["item"]), "%d" % int(g["price"]), "Buy", int(g["price"]) <= money, func(): bought.emit(i)))
	var any := false
	for b in bag:
		var name := String(b["item"])
		var value := int(_values.get(name, 0))
		if name == "" or name == _currency or value <= 0:
			continue
		any = true
		var slot := int(b["slot"])
		_sell.add_child(_row("%s  x%d" % [name, int(b["count"])], "%d" % value, "Sell", true, func(): sold.emit(slot)))
	if not any:
		var e := Label.new()
		e.text = "Nothing they want."
		e.add_theme_color_override("font_color", HUD.MUTED)
		_sell.add_child(e)

func _row(name: String, price: String, verb: String, ok: bool, on_press: Callable) -> HBoxContainer:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 8)
	var sw := ColorRect.new()
	sw.color = _colors.get(name.get_slice("  ", 0), HUD.MUTED)
	sw.custom_minimum_size = Vector2(4, 22)
	row.add_child(sw)
	var n := Label.new()
	n.text = name
	n.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	n.add_theme_color_override("font_color", HUD.TEXT)
	row.add_child(n)
	var p := Label.new()
	p.text = price
	p.add_theme_color_override("font_color", HUD.ACCENT)
	row.add_child(p)
	var b := Button.new()
	b.text = verb
	b.focus_mode = Control.FOCUS_NONE
	b.disabled = not ok
	b.custom_minimum_size = Vector2(64, 26)
	b.pressed.connect(on_press)
	row.add_child(b)
	return row

func is_open() -> bool:
	return panel.visible

func escape() -> bool:
	if panel.visible:
		panel.visible = false
		return true
	return false

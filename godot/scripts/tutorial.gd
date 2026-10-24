extends CanvasLayer
## A first walk through the game for a new character: a few short steps (move, pick a target, strike, cast, talk
## to someone with a quest, the keys worth knowing), each ticked off as soon as the player does it. Shown once on
## this machine (user://tutorial.cfg), and skippable. main.gd tells it what the player has done.

const HUD := preload("res://scripts/hud.gd")
const SAVED := "user://tutorial.cfg"
const STEPS := [
	["Walk", "Walk with W A S D or the arrow keys. Q and E turn the view; the mouse wheel zooms."],
	["Pick a target", "Click someone, or press Tab for whoever is nearest. Their name and health show at the top."],
	["Strike", "Space strikes your target. Hold it to keep swinging."],
	["Cast a spell", "Your spells are on the bar above the others, on R T Y and the rest. Hover one to read it, then try it."],
	["Find work", "Someone with a gold ! over their head has a quest for you. Walk up close and press F."],
	["The rest", "M is the map, F2 your spells, F3 your quests, F1 every key, Esc the menu. Ádh mór ort!"],
]

var panel := PanelContainer.new()
var _title := Label.new()
var _text := Label.new()
var _count := Label.new()
var step := -1                 # -1 when not running
var _last_for := 0.0           # time on the last step (it closes itself)

static func done() -> bool:
	var cfg := ConfigFile.new()
	cfg.load(SAVED)
	return bool(cfg.get_value("tutorial", "done", false))

func _ready() -> void:
	layer = 5
	HUD.use_fonts()
	panel.visible = false
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.9)
	b.border_color = HUD.ACCENT
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(14)
	panel.add_theme_stylebox_override("panel", b)
	panel.set_anchors_and_offsets_preset(Control.PRESET_CENTER_TOP)
	panel.offset_top = 96
	panel.offset_left = -270
	panel.offset_right = 270
	panel.mouse_filter = Control.MOUSE_FILTER_PASS
	add_child(panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 4)
	panel.add_child(v)
	var head := HBoxContainer.new()
	_title.add_theme_font_override("font", HUD.TITLE)
	_title.add_theme_font_size_override("font_size", 20)
	_title.add_theme_color_override("font_color", HUD.ACCENT)
	_title.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	head.add_child(_title)
	_count.add_theme_font_size_override("font_size", 12)
	_count.add_theme_color_override("font_color", HUD.MUTED)
	head.add_child(_count)
	var skip := Button.new()
	skip.text = "Skip"
	skip.focus_mode = Control.FOCUS_NONE
	skip.flat = true
	skip.add_theme_font_size_override("font_size", 12)
	skip.pressed.connect(finish)
	head.add_child(skip)
	v.add_child(head)
	_text.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	_text.custom_minimum_size = Vector2(510, 0)
	_text.add_theme_color_override("font_color", HUD.TEXT)
	v.add_child(_text)

func start() -> void:
	step = 0
	_show()

func running() -> bool:
	return step >= 0

func _show() -> void:
	_title.text = STEPS[step][0]
	_text.text = STEPS[step][1]
	_count.text = "%d of %d  " % [step + 1, STEPS.size()]
	panel.visible = true
	panel.modulate.a = 0.0
	create_tween().tween_property(panel, "modulate:a", 1.0, 0.3)

## The player did something: if it is what this step asks ("walk", "target", "strike", "cast", "talk"), on to
## the next.
func did(what: String) -> void:
	if step < 0 or step >= STEPS.size() - 1:
		return
	var want: String = ["walk", "target", "strike", "cast", "talk"][step]
	if what == want:
		step += 1
		_last_for = 0.0
		_show()

func _process(delta: float) -> void:
	if step == STEPS.size() - 1:
		_last_for += delta
		if _last_for > 14.0:
			finish()

func finish() -> void:
	step = -1
	panel.visible = false
	var cfg := ConfigFile.new()
	cfg.set_value("tutorial", "done", true)
	cfg.save(SAVED)

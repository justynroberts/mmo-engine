extends CanvasLayer
## Quests on screen: what a giver says when they put a quest to you (take it or not), your quest log (F3), and a
## tracker under the minimap with what to do next. What a quest is comes from /play/info "quests"; where you
## stand in each comes from the server (MmoClient.quests()). main.gd feeds both in and sends what you choose.

signal accepted(index: int)
signal declined
signal abandoned(index: int)

const HUD := preload("res://scripts/hud.gd")

var dialog := PanelContainer.new()
var log_panel := PanelContainer.new()
var tracker := VBoxContainer.new()
var _d_title := Label.new()
var _d_text := Label.new()
var _d_steps := Label.new()
var _d_reward := Label.new()
var _log_list := VBoxContainer.new()
var _quests: Array = []                 # /play/info quests
var _mine: Array = []                   # [{quest, step, count, state}]
var _offer := -1
var _step_labels: Array = []            # the tracker's "what next" line per quest in _mine order (null for none)

func _ready() -> void:
	layer = 6
	HUD.use_fonts()
	_build_dialog()
	_build_log()
	tracker.set_anchors_and_offsets_preset(Control.PRESET_TOP_RIGHT, Control.PRESET_MODE_MINSIZE, 16)
	tracker.grow_horizontal = Control.GROW_DIRECTION_BEGIN
	tracker.offset_top = 292
	tracker.custom_minimum_size = Vector2(250, 0)
	tracker.mouse_filter = Control.MOUSE_FILTER_IGNORE
	tracker.add_theme_constant_override("separation", 2)
	add_child(tracker)

func _style() -> StyleBoxFlat:
	var b := StyleBoxFlat.new()
	b.bg_color = Color(0.04, 0.045, 0.05, 0.96)
	b.border_color = HUD.EDGE
	b.set_border_width_all(1)
	b.set_corner_radius_all(3)
	b.set_content_margin_all(18)
	return b

func _label(size: int, color: Color, title := false, wrap := true) -> Label:
	var l := Label.new()
	l.add_theme_font_size_override("font_size", size)
	l.add_theme_color_override("font_color", color)
	if title:
		l.add_theme_font_override("font", HUD.TITLE)
	if wrap:
		l.autowrap_mode = TextServer.AUTOWRAP_WORD_SMART
	return l

func _button(text: String, on_press: Callable) -> Button:
	var b := Button.new()
	b.text = text
	b.focus_mode = Control.FOCUS_NONE
	b.custom_minimum_size = Vector2(120, 30)
	b.pressed.connect(on_press)
	return b

func _build_dialog() -> void:
	dialog.visible = false
	dialog.add_theme_stylebox_override("panel", _style())
	dialog.set_anchors_preset(Control.PRESET_CENTER)
	dialog.custom_minimum_size = Vector2(520, 0)
	dialog.position = Vector2(-260, -220)
	add_child(dialog)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	dialog.add_child(v)
	_d_title = _label(26, HUD.ACCENT, true, false)
	v.add_child(_d_title)
	_d_text = _label(15, HUD.TEXT)
	_d_text.custom_minimum_size = Vector2(484, 0)
	v.add_child(_d_text)
	_d_steps = _label(14, HUD.MUTED)
	v.add_child(_d_steps)
	_d_reward = _label(14, HUD.ACCENT)
	v.add_child(_d_reward)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 10)
	row.add_child(_button("Take it", func(): accepted.emit(_offer)))
	row.add_child(_button("Not now", func(): declined.emit()))
	v.add_child(row)

func _build_log() -> void:
	log_panel.visible = false
	log_panel.add_theme_stylebox_override("panel", _style())
	log_panel.set_anchors_preset(Control.PRESET_CENTER)
	log_panel.custom_minimum_size = Vector2(640, 480)
	log_panel.position = Vector2(-320, -260)
	add_child(log_panel)
	var v := VBoxContainer.new()
	v.add_theme_constant_override("separation", 10)
	log_panel.add_child(v)
	var head := HBoxContainer.new()
	var t := _label(28, HUD.ACCENT, true, false)
	t.text = "Quests"
	head.add_child(t)
	var gap := Control.new()
	gap.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	head.add_child(gap)
	var hint := _label(12, HUD.MUTED, false, false)
	hint.text = "F3 or Esc to close. Talk to people with F."
	head.add_child(hint)
	v.add_child(head)
	var scroll := ScrollContainer.new()
	scroll.size_flags_vertical = Control.SIZE_EXPAND_FILL
	scroll.custom_minimum_size = Vector2(0, 400)
	scroll.horizontal_scroll_mode = ScrollContainer.SCROLL_MODE_DISABLED
	_log_list.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	_log_list.add_theme_constant_override("separation", 12)
	scroll.add_child(_log_list)
	v.add_child(scroll)

## The game's quests (from /play/info).
func set_quests(quests: Array) -> void:
	_quests = quests

## Where we stand, from the server: redraw what shows it, and put up an offer that has just come.
func update(mine: Array, offer: int) -> void:
	var changed := mine != _mine
	_mine = mine
	if offer != _offer:
		_offer = offer
		if offer >= 0 and offer < _quests.size():
			_show_offer(_quests[offer])
		else:
			dialog.visible = false
	if changed:
		_redraw_tracker()
		if log_panel.visible:
			_redraw_log()

func _show_offer(q: Dictionary) -> void:
	_d_title.text = String(q["title"])
	_d_text.text = String(q.get("text", ""))
	var steps := []
	for st in q.get("steps", []):
		steps.append("  •  " + String(st["text"]))
	_d_steps.text = "\n".join(steps)
	_d_reward.text = _reward_text(q)
	dialog.visible = true

func _reward_text(q: Dictionary) -> String:
	var r := []
	if int(q.get("xp", 0)) > 0:
		r.append("%d experience" % int(q["xp"]))
	for it in q.get("reward", []):
		r.append("%d %s" % [int(it["count"]), HUD.item(String(it["item"]))])
	return ("Reward: " + ", ".join(r)) if not r.is_empty() else ""

## What to do next on a quest of ours, in a few words.
func next_step(m: Dictionary) -> String:
	var q: Dictionary = _quests[int(m["quest"])] if int(m["quest"]) < _quests.size() else {}
	if q.is_empty():
		return ""
	match int(m["state"]):
		1:
			return "Return to the %s" % q.get("turn_in", "giver")
		2:
			return "Done"
	var steps: Array = q.get("steps", [])
	var i := int(m["step"])
	if i >= steps.size():
		return ""
	var need := int(steps[i].get("need", 1))
	return String(steps[i]["text"]) + ("  %d/%d" % [int(m["count"]), need] if need > 1 else "")

## Where each of our quests wants us next: [[Vector2, quest title, radius], ...] in _mine order: the current step's
## place (/play/info steps' "at": the place, or where the bots to beat or speak to live), or the one to hand it in
## to, from `homes` (/play/info "homes"). A step with nowhere to show (bring N of something) is left out.
func goals(homes: Dictionary) -> Array:
	var out := []
	for m in _mine:
		out.append(_goal(m, homes))
	return out

func _goal(m: Dictionary, homes: Dictionary) -> Array:
	var qi := int(m["quest"])
	if qi >= _quests.size() or int(m["state"]) == 2:
		return []
	var q: Dictionary = _quests[qi]
	var at = null
	if int(m["state"]) == 1:
		at = homes.get(String(q.get("turn_in", "")))
	else:
		var steps: Array = q.get("steps", [])
		if int(m["step"]) < steps.size():
			at = steps[int(m["step"])].get("at")
	if not (at is Array) or at.size() < 2:
		return []
	return [Vector2(float(at[0]), float(at[1])), String(q["title"]), float(at[2]) if at.size() > 2 else 0.0]

## The tracker's lines with how far and which way each goal is from `me` (called a couple of times a second).
func update_distances(me: Vector2, homes: Dictionary) -> void:
	for i in mini(_mine.size(), _step_labels.size()):
		var l = _step_labels[i]
		if l == null or not is_instance_valid(l):
			continue
		var g := _goal(_mine[i], homes)
		var text := next_step(_mine[i])
		if not g.is_empty():
			var d: float = maxf(0.0, me.distance_to(g[0]) - float(g[2]))
			text += "   (here)" if d < 1.0 else "   %d u %s" % [int(d), _compass(g[0] - me)]
		(l as Label).text = text

## Which way, as a compass point (north is -y).
static func _compass(v: Vector2) -> String:
	const POINTS := ["E", "SE", "S", "SW", "W", "NW", "N", "NE"]
	return POINTS[posmod(int(round(atan2(v.y, v.x) / (PI / 4.0))), 8)]

func _redraw_tracker() -> void:
	for c in tracker.get_children():
		c.queue_free()
	_step_labels.clear()
	for m in _mine:
		_step_labels.append(null)
		if int(m["state"]) == 2 or int(m["quest"]) >= _quests.size():
			continue
		var t := _label(14, HUD.ACCENT, true, false)
		t.text = String(_quests[int(m["quest"])]["title"])
		t.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		t.add_theme_constant_override("outline_size", 4)
		t.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.9))
		tracker.add_child(t)
		var s := _label(13, HUD.TEXT, false, false)
		s.text = next_step(m)
		s.horizontal_alignment = HORIZONTAL_ALIGNMENT_RIGHT
		s.add_theme_constant_override("outline_size", 4)
		s.add_theme_color_override("font_outline_color", Color(0, 0, 0, 0.9))
		tracker.add_child(s)
		_step_labels[_step_labels.size() - 1] = s

func _redraw_log() -> void:
	for c in _log_list.get_children():
		c.queue_free()
	var any := false
	for want_done in [false, true]:
		for m in _mine:
			var done := int(m["state"]) == 2
			if done != want_done or int(m["quest"]) >= _quests.size():
				continue
			any = true
			var q: Dictionary = _quests[int(m["quest"])]
			var box := VBoxContainer.new()
			var head := HBoxContainer.new()
			var t := _label(18, HUD.MUTED if done else HUD.ACCENT, true, false)
			t.text = String(q["title"]) + ("  (done)" if done else "")
			t.size_flags_horizontal = Control.SIZE_EXPAND_FILL
			head.add_child(t)
			if not done:
				var idx := int(m["quest"])
				head.add_child(_button("Give up", func(): abandoned.emit(idx)))
			box.add_child(head)
			if not done:
				var body := _label(13, HUD.MUTED)
				body.text = String(q.get("text", ""))
				box.add_child(body)
				var steps: Array = q.get("steps", [])
				for i in steps.size():
					var line := _label(14, HUD.TEXT, false, false)
					var mark := "✓" if i < int(m["step"]) or int(m["state"]) == 1 else ("▸" if i == int(m["step"]) else "·")
					var need := int(steps[i].get("need", 1))
					var tally := ("  %d/%d" % [int(m["count"]), need]) if i == int(m["step"]) and need > 1 and int(m["state"]) == 0 else ""
					line.text = "%s  %s%s" % [mark, steps[i]["text"], tally]
					box.add_child(line)
				if int(m["state"]) == 1:
					var back := _label(14, HUD.ACCENT, false, false)
					back.text = "▸  Return to the %s" % q.get("turn_in", "giver")
					box.add_child(back)
				var rw := _label(13, HUD.ACCENT)
				rw.text = _reward_text(q)
				box.add_child(rw)
			_log_list.add_child(box)
	if not any:
		var e := _label(14, HUD.MUTED)
		e.text = "No quests yet. Look for a gold ! over someone's head, and talk to them with F."
		_log_list.add_child(e)

func toggle_log() -> void:
	log_panel.visible = not log_panel.visible
	if log_panel.visible:
		_redraw_log()

func is_open() -> bool:
	return log_panel.visible or dialog.visible

## Esc: close what is open (an offer counts as "not now").
func escape() -> bool:
	if dialog.visible:
		declined.emit()
		dialog.visible = false
		return true
	if log_panel.visible:
		log_panel.visible = false
		return true
	return false

## Whether a quest could be offered to us now (for the ! over a giver): not held, or done and repeatable; our
## level high enough; the quest before it done.
func available(index: int, my_level: int) -> bool:
	var q: Dictionary = _quests[index]
	for m in _mine:
		if int(m["quest"]) == index and not (int(m["state"]) == 2 and bool(q.get("repeatable", false))):
			return false
	if my_level > 0 and int(q.get("level", 0)) > my_level:
		return false
	var after = q.get("after")
	if after is String and after != "":
		var ok := false
		for m in _mine:
			if int(m["quest"]) < _quests.size() and _quests[int(m["quest"])]["name"] == after and int(m["state"]) == 2:
				ok = true
		if not ok:
			return false
	return true

## For markers: by giver name, "!" (a quest to take) or "?" (one to hand in, which wins).
func marks(my_level: int) -> Dictionary:
	var out := {}
	for i in _quests.size():
		if available(i, my_level):
			out[String(_quests[i]["giver"])] = "!"
	for m in _mine:
		if int(m["quest"]) >= _quests.size():
			continue
		var q: Dictionary = _quests[int(m["quest"])]
		if int(m["state"]) == 1:
			out[String(q["turn_in"])] = "?"
		elif int(m["state"]) == 0:
			var steps: Array = q.get("steps", [])
			if int(m["step"]) < steps.size() and steps[int(m["step"])].has("who"):
				out[String(steps[int(m["step"])]["who"])] = "?" # someone to speak to
	return out

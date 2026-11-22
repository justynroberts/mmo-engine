extends RefCounted
## --autoplay SECONDS: the client plays as a new player would, through the same calls the keys and windows make,
## and says what happened. It chooses a class, walks to whoever has work for it (the "!" over them, or on the map),
## talks and takes what it is offered, follows the tracker to each step's goal, strikes what the step names there
## (and whatever is fighting it), rises after falling, and hands in at the "?". Every change of quest, level and
## fall is printed as an AUTO line, and a summary at the end (AUTO DONE), then the client quits. For finding where a
## player gets stuck: a goal not reached, a beast that cannot be caught, a fight lost.
##   godot --path godot -- --guest --host H --udp U --web H:W --autoplay 900

var m: Node                    # main.gd
var started := 0.0
var took := 0
var done := 0
var falls := 0
var kills := 0
var _last := {}                # quest index -> "step/count/state" last printed
var _talked := {}              # entity id -> when we last talked to them (seconds)
var _goal_since := 0.0         # when we set out for the current goal
var _goal_key := ""
var _given_up := {}            # goal key -> true: not reached in time
var _from := Vector2.INF       # for noticing we are stuck against something
var _check_at := 0.0
var _side := 1.0
var _dodge_until := 0.0
var _hp_was := 1.0
var _struck := 0               # what we went for last
var _cast_at := 0.0
var _bought_at := 0.0
var _bought_for := ""
var _buy_tries := {}           # step text -> purchases tried
var _was_at := Vector2.INF     # where we were a moment ago
var _look_at := Vector2.ZERO   # where we are looking about a town for someone
var _look_at_t := -99.0

const GOAL_SECS := 600.0       # this long on the way to one goal and it is given up
const TALK_AGAIN := 20.0       # seconds before talking to the same one again

func now() -> float:
	return Time.get_ticks_msec() / 1000.0 - started

func say(what: String) -> void:
	print("AUTO %6.1f  lvl %d  %s" % [now(), m.my_level, what])

func run(main: Node, secs: float) -> void:
	m = main
	started = Time.get_ticks_msec() / 1000.0
	var ok: bool = await m._wait(20.0, func(): return m.net.get_status() == 1 and m.net.get_self_id() != 0 and m.ents.has(m.net.get_self_id()))
	if not ok:
		say("never got into the world")
		m.get_tree().quit(1)
		return
	say("in the world at %d, %d" % [int(m.me_pos.x), int(m.me_pos.y)])
	await m.get_tree().create_timer(2.0).timeout
	if m.classes.size() > 0:
		m.net.act_arg(m.ACT_CLASS, 0, 0)
		say("chose the class %s" % String(m.classes[0].get("title", m.classes[0].get("name", "?"))))
		if m.classui and m.classui.visible:
			m.classui.visible = false
	var level_was: int = m.my_level
	while now() < secs:
		await m.get_tree().create_timer(0.1).timeout
		if m.my_level != level_was:
			say("reached level %d" % m.my_level)
			level_was = m.my_level
		_report_quests()
		if m.fallen.down:
			m.net.set_move(Vector2.ZERO)
			if _hp_was > 0.0:
				falls += 1
				say("fell (%s)" % _doing())
				_hp_was = 0.0
			await m.get_tree().create_timer(3.0).timeout
			m.fallen.rise.emit()
			continue
		_hp_was = _hp()
		if _was_at != Vector2.INF and m.me_pos.distance_to(_was_at) > 150.0:
			say("carried %d m in a moment (a stone, or rising), now at %d, %d" % [int(m.me_pos.distance_to(_was_at)), int(m.me_pos.x), int(m.me_pos.y)])
		_was_at = m.me_pos
		if _buy():
			continue
		if _fight():
			continue
		if _talk_to_giver():
			continue
		_follow_goal()
	m.net.set_move(Vector2.ZERO)
	var held := []
	for q in m.questui._mine:
		if int(q["state"]) != 2:
			held.append("%s (%s)" % [_title(q), m.questui.next_step(q)])
	say("DONE after %d s: %d quests taken, %d done, %d beaten, %d falls, %d xp; in hand: %s" % [int(now()), took, done, kills, falls, int(m._progress.get("xp", 0)), ", ".join(held)])
	print("AUTO DONE level %d quests %d/%d falls %d" % [m.my_level, done, took, falls])
	m.get_tree().quit(0)

func _hp() -> float:
	var me = m.net.get_self_id()
	for e in m.net.entities():
		if e["id"] == me and m.health_idx != "":
			return float(e.get(m.health_idx, 1.0)) / maxf(float(m._hp_max.get(me, 1.0)), 1.0)
	return 1.0

func _title(q: Dictionary) -> String:
	var i := int(q["quest"])
	return String(m.questui._quests[i]["title"]) if i < m.questui._quests.size() else "?"

func _doing() -> String:
	for q in m.questui._mine:
		if int(q["state"]) == 0:
			return "%s: %s" % [_title(q), m.questui.next_step(q)]
	return "nothing in hand"

## Print each quest whose step, count or state changed since last looked.
func _report_quests() -> void:
	for q in m.questui._mine:
		var key := "%d/%d/%d" % [int(q["step"]), int(q["count"]), int(q["state"])]
		var i := int(q["quest"])
		if _last.get(i, "") == key:
			continue
		var was: String = _last.get(i, "")
		_last[i] = key
		match int(q["state"]):
			0:
				if was == "":
					took += 1
					say("took %s: %s" % [_title(q), m.questui.next_step(q)])
				else:
					say("%s: %s" % [_title(q), m.questui.next_step(q)])
			1:
				say("%s: ready to hand in" % _title(q))
			2:
				if was != "":
					done += 1
					say("handed in %s" % _title(q))

## Who is near, by entity id: [id, kind (the name its kind goes by), position].
func _near() -> Array:
	var out := []
	var me = m.net.get_self_id()
	for e in m.net.entities():
		if e["id"] != me:
			out.append([e["id"], m._name(e["id"]).get_slice("|", 0), e["position"]])
	return out

## Strike back at what is fighting us, or at what the step we are on asks for when one is in sight: in reach a
## blow (and now and then the first two spells of the bar), else after it.
func _fight() -> bool:
	var near := _near()
	if _struck != 0 and not near.any(func(n): return n[0] == _struck):
		kills += 1 # gone from sight while we were at it: down (or fled out of sight)
		_struck = 0
	var foe: int = m._nearest_foe_in(40.0)
	if foe == 0:
		var want := _step_kinds()
		var best := 60.0
		for n in near:
			if want.has(n[1]) and m.me_pos.distance_to(n[2]) < best:
				best = m.me_pos.distance_to(n[2])
				foe = n[0]
	if foe == 0:
		_struck = 0
		return false
	var at: Vector2 = Vector2.INF
	for n in near:
		if n[0] == foe:
			at = n[2]
	if m.target_id != foe:
		m.target_id = foe
		say("going for a %s, %d m off" % [m._name(foe).get_slice("|", 0), int(m.me_pos.distance_to(at))])
	_struck = foe
	if m.me_pos.distance_to(at) > m.action_range - 0.5:
		_steer(at)
	else:
		m.net.set_move(Vector2.ZERO)
		m._attack()
		if now() - _cast_at > 4.0:
			_cast_at = now()
			m._cast(randi() % 2)
	return true

## A "bring N" step for something a trader in reach sells: buy one (a second later the next).
func _buy() -> bool:
	if now() - _bought_at < 0.7:
		return false
	var shops: Dictionary = m.info.get("shops", {}) if m.info.get("shops") is Dictionary else {}
	for q in m.questui._mine:
		if int(q["state"]) != 0 or int(q["quest"]) >= m.questui._quests.size():
			continue
		var steps: Array = m.questui._quests[int(q["quest"])].get("steps", [])
		if int(q["step"]) >= steps.size():
			continue
		var st: Dictionary = steps[int(q["step"])]
		var text := String(st.get("text", ""))
		# Out of coin (or the trader refuses us): a few more tries than it needs and it is left to other means.
		if not text.begins_with("Bring ") or int(_buy_tries.get(text, 0)) > int(st.get("need", 1)) + 4:
			continue
		var shown := text.get_slice(" ", 2)
		for w in range(3, text.get_slice_count(" ")):
			shown += " " + text.get_slice(" ", w)
		var item := ""
		for it in m.info.get("items", []):
			if String(it.get("title", it["name"])) == shown:
				item = String(it["name"])
		for n in _near():
			if not shops.has(n[1]) or m.me_pos.distance_to(n[2]) > 150.0:
				continue
			var stock: Array = shops[n[1]]
			for i in stock.size():
				if String(stock[i]["item"]) == item:
					# In sight: up to them (they wander their stall), then buy.
					if m.me_pos.distance_to(n[2]) > m._rule("talk_reach", 8.0) - 1.5:
						_steer(n[2])
						return true
					m.net.set_move(Vector2.ZERO)
					m.net.buy(n[0], i)
					_bought_at = now()
					_buy_tries[text] = int(_buy_tries.get(text, 0)) + 1
					if _bought_for != text:
						_bought_for = text
						say("buying %s from the %s at %d" % [shown, n[1], int(stock[i]["price"])])
					return true
	return false

## The kinds the step in hand asks to beat (or that drop what it asks to bring), as their bots are named: not the
## traders that sell it (bought from, never struck: the watch would fall on us).
func _step_kinds() -> Array:
	var shops: Dictionary = m.info.get("shops", {}) if m.info.get("shops") is Dictionary else {}
	var out := []
	for q in m.questui._mine:
		if int(q["state"]) != 0:
			continue
		var i := int(q["quest"])
		if i >= m.questui._quests.size():
			continue
		var steps: Array = m.questui._quests[i].get("steps", [])
		if int(q["step"]) < steps.size() and steps[int(q["step"])].get("kinds") is Array and not steps[int(q["step"])].has("who"):
			for k in steps[int(q["step"])]["kinds"]:
				if not shops.has(k):
					out.append(k)
	return out

## Someone in sight with a "!" or "?" for us: walk up and talk, and take what is offered.
func _talk_to_giver() -> bool:
	var marks: Dictionary = m.questui.marks(m.my_level, m.me_pos)
	var best := 120.0
	var who := 0
	var at := Vector2.ZERO
	for n in _near():
		if marks.has(n[1]) and now() - float(_talked.get(n[0], -999.0)) > TALK_AGAIN and m.me_pos.distance_to(n[2]) < best:
			best = m.me_pos.distance_to(n[2])
			who = n[0]
			at = n[2]
	if who == 0:
		return false
	if best > m._rule("talk_reach", 8.0) - 1.5:
		_steer(at)
		return true
	m.net.set_move(Vector2.ZERO)
	_talked[who] = now()
	m.net.talk(who)
	m.get_tree().create_timer(1.0).timeout.connect(func():
		if m.questui._offer >= 0:
			m.questui.accepted.emit(m.questui._offer)
			m.questui.dialog.visible = false)
	return true

## On to the goal of the quest in hand (the tracker's), or failing one, to the nearest "!" on the map.
func _follow_goal() -> void:
	var homes: Dictionary = m.info.get("homes", {}) if m.info.get("homes") is Dictionary else {}
	var to := Vector2.INF
	var key := ""
	var r := 0.0
	# The nearest goal of those in hand, as a player would.
	var best_g := INF
	var goals: Array = m.questui.goals(homes)
	for gi in goals.size():
		var g: Array = goals[gi]
		if g.is_empty() or gi >= m.questui._mine.size() or not _can_do(m.questui._mine[gi]):
			continue
		var k := "%s@%d,%d" % [g[1], int(g[0].x), int(g[0].y)]
		if _given_up.has(k) or m.me_pos.distance_to(g[0]) >= best_g:
			continue
		best_g = m.me_pos.distance_to(g[0])
		to = g[0]
		r = float(g[2])
		key = k
	if to == Vector2.INF:
		var best := INF
		for mk in m._map_marks():
			if String(mk[1]) in ["!", "?"] and m.me_pos.distance_to(mk[0]) < best:
				var k := "giver@%d,%d" % [int(mk[0].x), int(mk[0].y)]
				if not _given_up.has(k):
					best = m.me_pos.distance_to(mk[0])
					to = mk[0]
					key = k
	if to == Vector2.INF:
		m.net.set_move(Vector2.ZERO)
		return
	if key != _goal_key:
		_goal_key = key
		_goal_since = now()
		say("setting out for %s, %d m off" % [key, int(m.me_pos.distance_to(to))])
	if now() - _goal_since > GOAL_SECS:
		say("gave up on %s, still %d m off" % [key, int(m.me_pos.distance_to(to))])
		_given_up[key] = true
		return
	# Inside the goal: walk about it, so what lives there comes into sight (not in a town, where the travel stones
	# stand and the folk are in sight anyway).
	if m.me_pos.distance_to(to) < maxf(r * 0.6, 3.0):
		if _in_town(to):
			# Looking about the town for whoever it is (a trader at their stall, the smith at the forge): a new
			# spot every so often, out from the middle where the travel stones stand.
			if now() - _look_at_t > 15.0 or m.me_pos.distance_to(_look_at) < 4.0:
				_look_at_t = now()
				var a := randf() * TAU
				_look_at = to + Vector2(cos(a), sin(a)) * randf_range(40.0, maxf(r * 0.85, 45.0))
			_steer(_look_at)
			return
		var a := now() * 0.15
		to += Vector2(cos(a), sin(a)) * maxf(r * 0.5, 10.0)
	_steer(to)

## Whether a quest in hand is one this can do: its step is to beat, go, speak, or bring what a trader sells or a
## beast drops (not crops to grow, nor building).
func _can_do(q: Dictionary) -> bool:
	if int(q["state"]) != 0:
		return true # ready: to hand in
	var i := int(q["quest"])
	if i >= m.questui._quests.size():
		return false
	var steps: Array = m.questui._quests[i].get("steps", [])
	if int(q["step"]) >= steps.size():
		return false
	var text := String(steps[int(q["step"])].get("text", ""))
	if text.begins_with("Build "):
		return false
	if text.begins_with("Bring "):
		# Not one we tried to buy and could not (out of coin): there is nothing to go to it for.
		return steps[int(q["step"])].get("kinds") is Array and int(_buy_tries.get(text, 0)) <= int(steps[int(q["step"])].get("need", 1)) + 4
	return true

## Whether a point is in a town: a titled place (/play/info locations) the size of a village or town, not a
## hunting ground.
func _in_town(p: Vector2) -> bool:
	for l in m.locations:
		if float(l.get("r", 0.0)) >= 60.0 and float(l.get("r", 0.0)) < 400.0 and p.distance_to(Vector2(float(l["x"]), float(l["y"]))) <= float(l["r"]):
			if not String(l.get("title", "")).begins_with("Talamh Seilge"):
				return true
	return false

## Walk towards `to`; when we stop getting anywhere (a wall, a tree), sidestep for a moment.
func _steer(to: Vector2) -> void:
	var dir: Vector2 = (to - m.me_pos).normalized()
	if now() - _check_at > 0.8:
		if _from != Vector2.INF and m.me_pos.distance_to(_from) < 2.0:
			_dodge_until = now() + 1.5
			_side = -_side
		_from = m.me_pos
		_check_at = now()
	if now() < _dodge_until:
		dir = (dir.orthogonal() * _side + dir * 0.3).normalized()
	m.net.set_move(dir)

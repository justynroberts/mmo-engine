extends RefCounted
## --autoplay SECONDS: the client plays as a new player would, through the same calls the keys and windows make,
## and says what happened. It chooses a class, walks to whoever has work for it (the "!" over them, or on the map),
## talks and takes what it is offered, follows the tracker to each step's goal, strikes what the step names there
## (and whatever is fighting it), rises after falling, and hands in at the "?". With nothing it can do near (no goal
## within HUNT_IF_FAR), it hunts beasts about its level in sight or out on the nearest hunting ground; hurt and out
## of a fight it sits until whole; it leaves alone what is three levels over it unless that comes for it. Every change of quest, level and
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
var _goal_at := Vector2.INF
var _goal_key := ""
var _given_up := {}            # goal key -> true: not reached in time
var _from := Vector2.INF       # for noticing we are stuck against something
var _check_at := 0.0
var _side := 1.0
var _dodge_until := 0.0
var _hp_was := 1.0
var _xp_struck := 0
var _struck := 0               # what we went for last
var _cast_at := 0.0
var _bought_at := 0.0
var _bought_for := ""
var _buy_tries := {}           # step text -> purchases tried
var _was_at := Vector2.INF     # where we were a moment ago
var _look_at := Vector2.ZERO   # where we are looking about a town for someone
var _look_at_t := -99.0
var _resting := false
var _xp_said := 0.0
var _hunt_at := Vector2.INF     # the hunting ground we are roaming
var _xp_was := -1
var _xp_moved := 0.0           # when our experience last went up
var _hunt_until := 0.0         # stuck for experience: hunting till then
var _walking_to := 0            # the one we are walking up to, and since when
var _walking_since := 0.0

const GOAL_SECS := 240.0       # this long on the way to one goal and it is given up (for a while)
const TALK_AGAIN := 20.0       # seconds before talking to the same one again
const HUNT_IF_FAR := 1200.0    # the nearest goal further than this: hunt instead, nearer home
const REST_BELOW := 0.45       # out of a fight below this share of health: sit until whole

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
			for k in _given_up.keys():
				if String(k).begins_with("giver@"):
					_given_up.erase(k) # new work is offered at a new level
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
		if now() - _xp_said > 60.0:
			_xp_said = now()
			say("%d xp, %d beaten so far, at %d, %d" % [int(m._progress.get("xp", 0)), kills, int(m.me_pos.x), int(m.me_pos.y)])
		# Earning next to nothing (a point a minute for being about) for three minutes: hunt a while.
		var xp := int(m._progress.get("xp", 0))
		if _xp_was < 0 or xp - _xp_was >= 60:
			_xp_was = xp
			_xp_moved = now()
		if now() - _xp_moved > 180.0 and now() > _hunt_until:
			_start_hunt()
			_xp_moved = now()
			say("nothing earned in three minutes: hunting a while")
		if _rest():
			continue
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
		# Gone from sight while we were at it: down, if it paid (else fled out of sight).
		if int(m._progress.get("xp", 0)) > _xp_struck:
			kills += 1
		_struck = 0
	var foe: int = m._nearest_foe_in(40.0)
	if foe == 0:
		var want := _step_kinds()
		var kept := _keep_chasing(near)
		for n in near:
			if n[0] == kept and want.has(n[1]):
				return _strike(kept)
		var best := 60.0
		for n in near:
			if want.has(n[1]) and not _deadly(n[0]) and not _chased(n[0]) and not _in_pack(n, near) and m.me_pos.distance_to(n[2]) < best:
				best = m.me_pos.distance_to(n[2])
				foe = n[0]
	if foe == 0:
		_struck = 0
		return false
	return _strike(foe)

## The quarry already being chased, while it is still in sight and getting nearer: keep after it rather than turn
## to whichever is nearest this moment (fleeing deer scatter, and chasing each in turn catches none). One not
## caught up with in CHASE_SECS is let go a while.
const CHASE_SECS := 30.0
var _chase_id := 0
var _chase_best := INF
var _chase_t := 0.0
var _let_go := {}
func _keep_chasing(near: Array) -> int:
	if _chase_id == 0:
		return 0
	for n in near:
		if n[0] == _chase_id:
			var d: float = m.me_pos.distance_to(n[2])
			if d < _chase_best - 2.0:
				_chase_best = d
				_chase_t = now()
			if now() - _chase_t > CHASE_SECS:
				_let_go[_chase_id] = now() + 120.0
				say("could not catch the %s: letting it go" % String(n[1]))
				_chase_id = 0
				return 0
			return _chase_id
	_chase_id = 0
	return 0

## Two or more of its kind about it (within the reach a beast calls for help in): a pull that brings the pack.
## Something a level or more under us is fine to take on in company.
func _in_pack(n: Array, near: Array) -> bool:
	if _level_of(n[0]) < m.my_level:
		return false
	var reach: float = m._rule("assist_reach", 25.0)
	var others := 0
	for o in near:
		if o[0] != n[0] and o[1] == n[1] and Vector2(o[2]).distance_to(n[2]) < reach + 3.0:
			others += 1
	return others >= 2

func _chased(id: int) -> bool:
	return _let_go.has(id) and now() < float(_let_go[id])

## Go for `foe`: up to it, then blows (and now and then the first two spells of the bar).
func _strike(foe: int) -> bool:
	var near := _near()
	var at: Vector2 = Vector2.INF
	for n in near:
		if n[0] == foe:
			at = n[2]
	if _chase_id != foe:
		_chase_id = foe
		_chase_best = INF
		_chase_t = now()
	if m.target_id != foe:
		m.target_id = foe
		say("going for a %s, %d m off" % [m._name(foe).get_slice("|", 0), int(m.me_pos.distance_to(at))])
	if _struck != foe:
		_xp_struck = int(m._progress.get("xp", 0))
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

## A level of an entity in sight (0 for none: townsfolk, players).
func _level_of(id: int) -> int:
	for e in m.net.entities():
		if e["id"] == id:
			return int(round(float(e.get("level", 0.0))))
	return 0

## Three or more levels over us: a fight to leave alone.
func _deadly(id: int) -> bool:
	return _level_of(id) >= m.my_level + 3

## Hurt and nobody fighting us: sit until whole (resting comes back faster sitting).
func _rest() -> bool:
	if m._nearest_foe_in(60.0) != 0:
		_resting = false
		return false
	var hp := _hp()
	if not _resting and hp < REST_BELOW:
		_resting = true
		m.net.set_move(Vector2.ZERO)
		m.net.say("/sit", false)
		say("resting at %d%% health" % int(hp * 100.0))
	if _resting and hp >= 0.95:
		_resting = false
		say("whole again")
	if _resting:
		m.net.set_move(Vector2.ZERO)
	return _resting

## A hunt: till three are beaten or eight minutes are up (the hunting grounds are a walk out of town).
var _hunt_kills := 0
func _start_hunt() -> void:
	_hunt_until = now() + 480.0
	_hunt_kills = kills + 3

## Nothing near to do: a beast about our level in sight (not a trader, not three over us, not townsfolk), else out
## to the nearest hunting ground and round it.
func _hunt() -> void:
	var shops: Dictionary = m.info.get("shops", {}) if m.info.get("shops") is Dictionary else {}
	var game := _game_kinds()
	var best := 90.0
	var foe := _keep_chasing(_near())
	if foe != 0:
		_strike(foe)
		return
	var all_near := _near()
	for n in all_near:
		var lv := _level_of(n[0])
		if lv < 1 or _chased(n[0]) or _in_pack(n, all_near) or not game.has(n[1]) or shops.has(n[1]) or m._players.has(n[0]) or lv > m.my_level + 2 or lv < m.my_level - 4 or _in_town(n[2]):
			continue
		if m.me_pos.distance_to(n[2]) < best:
			best = m.me_pos.distance_to(n[2])
			foe = n[0]
	if foe != 0:
		_strike(foe)
		return
	if _hunt_at == Vector2.INF or m.me_pos.distance_to(_hunt_at) > 3000.0:
		var near_d := INF
		for l in m.locations:
			if String(l.get("title", "")).begins_with("Talamh Seilge"):
				var p := Vector2(float(l["x"]), float(l["y"]))
				if m.me_pos.distance_to(p) < near_d:
					near_d = m.me_pos.distance_to(p)
					_hunt_at = p
		if _hunt_at != Vector2.INF:
			say("off hunting at %d, %d, %d m off" % [int(_hunt_at.x), int(_hunt_at.y), int(near_d)])
	if _hunt_at == Vector2.INF:
		m.net.set_move(Vector2.ZERO)
		return
	var a := now() * 0.1
	_steer(_hunt_at + Vector2(cos(a), sin(a)) * 60.0)

## Game: the kinds some quest asks a player to beat (beasts, brigands, the restless dead), never the townsfolk,
## their flocks or their hens.
var _game_cache := {}
func _game_kinds() -> Dictionary:
	if _game_cache.is_empty():
		for q in m.questui._quests:
			for st in q.get("steps", []):
				if String(st.get("text", "")).begins_with("Beat ") and st.get("kinds") is Array:
					for k in st["kinds"]:
						_game_cache[String(k)] = true
	return _game_cache

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
						if now() - float(_talked.get(n[0], -999.0)) < 0.0:
							continue # one we could not reach a moment ago
						return _walk_up(n[0], n[2])
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

## Up to someone to talk to or buy from; false (and they are left a few minutes) when not reached in 45 s:
## indoors, or round a wall we cannot find the way past.
var _walk_last := 0.0
func _walk_up(who: int, at: Vector2) -> bool:
	if who != _walking_to or now() - _walk_last > 3.0:
		_walking_to = who
		_walking_since = now()
	_walk_last = now()
	if now() - _walking_since > 45.0:
		say("cannot reach the %s, %d m off: later" % [m._name(who).get_slice("|", 0), int(m.me_pos.distance_to(at))])
		_talked[who] = now() + 180.0
		_walking_to = 0
		return false
	_steer(at)
	return true

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
		return _walk_up(who, at)
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
	if now() < _hunt_until and kills >= _hunt_kills:
		_hunt_until = 0.0
		say("three beaten: back to the quests")
	if now() < _hunt_until:
		_hunt()
		return
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
	if to == Vector2.INF or best_g > HUNT_IF_FAR:
		var best := INF
		to = Vector2.INF
		for mk in m._map_marks():
			if String(mk[1]) in ["!", "?"] and m.me_pos.distance_to(mk[0]) < best:
				var k := "giver@%d,%d" % [int(mk[0].x), int(mk[0].y)]
				if not _given_up.has(k):
					best = m.me_pos.distance_to(mk[0])
					to = mk[0]
					key = k
	if to == Vector2.INF or m.me_pos.distance_to(to) > HUNT_IF_FAR:
		_hunt()
		return
	# A goal whose point shifts a step (the nearest of a kind's homes as we walk) is the same goal.
	if key != _goal_key and (key.get_slice("@", 0) != _goal_key.get_slice("@", 0) or to.distance_to(_goal_at) > 30.0):
		_goal_at = to
		_goal_key = key
		_goal_since = now()
		say("setting out for %s, %d m off" % [key, int(m.me_pos.distance_to(to))])
	# Looking for someone with work for us gets a minute and a half; then on with something else a while.
	if key.begins_with("giver@") and now() - _goal_since > 90.0:
		say("no giver found at %s: hunting a while" % key)
		_given_up[key] = true
		_start_hunt()
		return
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

## Walk towards `to`; when we stop getting anywhere (a wall, a tree), sidestep a moment, and stuck again and again
## (a long wall, a town's walls), walk to a point well off to one side first, further and on the other side each time.
var _stalls := 0
var _detour_path: Array = []

## The solid footprints (walls, towers, houses) within `r` of `c`, from the world file the client drew.
func _solid_near(c: Vector2, r: float) -> Array:
	var out := []
	var w = m.world
	if w == null:
		return out
	var cell: float = w.CELL
	for i in range(floori((c.x - r) / cell), floori((c.x + r) / cell) + 1):
		for j in range(floori((c.y - r) / cell), floori((c.y + r) / cell) + 1):
			for o in w.in_cell(i, j):
				if o.has("b") and o.get("solid", false):
					out.append(o["b"])
	# Built blocks too (a player's house of planks): a column two or more blocks high over its ground is a wall.
	var bs := 2.0
	if m.build is Dictionary and m.build.has("block"):
		bs = float(m.build["block"])
	for key in m.blocks:
		var cx: int = int(key) & 0x7fff
		var cy: int = int(key) >> 15
		var p := Vector2((cx + 0.5) * bs, (cy + 0.5) * bs)
		if absf(p.x - c.x) > r or absf(p.y - c.y) > r:
			continue
		var col: Dictionary = m.blocks[key]
		var gl: int = m.theme.block_layer(cx, cy) if m.theme else int(col["b"])
		var high := 0
		for z in range(gl, gl + 4):
			if m._solid(cx, cy, z):
				high += 1
		if high >= 2:
			out.append([cx * bs, cy * bs, (cx + 1) * bs, (cy + 1) * bs])
	return out

## Whether a walk from a to b keeps clear of every box (with a body's width to spare).
func _clear(a: Vector2, b: Vector2, boxes: Array) -> bool:
	var n := maxi(int(a.distance_to(b) / 2.0), 1)
	for k in n + 1:
		var p := a.lerp(b, float(k) / n)
		for bx in boxes:
			if p.x > float(bx[0]) - PAD and p.x < float(bx[2]) + PAD and p.y > float(bx[1]) - PAD and p.y < float(bx[3]) + PAD:
				return false
	return true

## A path round what is solid towards `to` (at most PLAN_REACH ahead), on a grid over the world file's
## footprints (walls, towers, houses): the points to walk through, or [] when there is none.
const PLAN_REACH := 500.0
const PLAN_CELL := 4.0
const PAD := 0.8 # a body's half width, and a little over
func _plan(to: Vector2) -> Array:
	# Round a long wall the way in may be far off to the side: look wider when nothing is found near.
	for margin in [150.0, 450.0, 900.0]:
		# Fine near (gaps between bushes in a hedge are a body wide), coarser further out.
		var path := _plan_in(to, margin, 2.0 if margin < 200.0 else PLAN_CELL)
		if not path.is_empty():
			return path
	return []

func _plan_in(to: Vector2, margin: float, cell: float) -> Array:
	var a: Vector2 = m.me_pos
	var goal := to
	if a.distance_to(goal) > PLAN_REACH:
		goal = a + (to - a).normalized() * PLAN_REACH
	var lo := Vector2(minf(a.x, goal.x), minf(a.y, goal.y)) - Vector2(margin, margin)
	var hi := Vector2(maxf(a.x, goal.x), maxf(a.y, goal.y)) + Vector2(margin, margin)
	var g := AStarGrid2D.new()
	g.region = Rect2i(0, 0, int(ceil((hi.x - lo.x) / cell)), int(ceil((hi.y - lo.y) / cell)))
	g.cell_size = Vector2(cell, cell)
	g.offset = lo
	g.diagonal_mode = AStarGrid2D.DIAGONAL_MODE_ONLY_IF_NO_OBSTACLES
	g.update()
	var centre := (lo + hi) / 2.0
	var boxes := _solid_near(centre, centre.distance_to(hi) + 50.0)
	for b in boxes:
		var i0 := maxi(int(floor((float(b[0]) - PAD - lo.x) / cell)), 0)
		var j0 := maxi(int(floor((float(b[1]) - PAD - lo.y) / cell)), 0)
		var i1 := mini(int(floor((float(b[2]) + PAD - lo.x) / cell)), g.region.size.x - 1)
		var j1 := mini(int(floor((float(b[3]) + PAD - lo.y) / cell)), g.region.size.y - 1)
		for i in range(i0, i1 + 1):
			for j in range(j0, j1 + 1):
				g.set_point_solid(Vector2i(i, j))
	var from := Vector2i(int((a.x - lo.x) / cell), int((a.y - lo.y) / cell))
	var dest := Vector2i(int((goal.x - lo.x) / cell), int((goal.y - lo.y) / cell))
	# Where we stand is open, whatever the padding says (pressed into a hedge, every cell about us looks shut).
	var near_r := maxi(int(ceil(1.5 / cell)), 1)
	for di in range(-near_r, near_r + 1):
		for dj in range(-near_r, near_r + 1):
			var c := from + Vector2i(di, dj)
			if not g.is_in_boundsv(c):
				continue
			# Only the padding: a cell whose middle is inside a wall itself stays shut (or a path goes through
			# the join where a castle meets its tower).
			var mid := lo + (Vector2(c) + Vector2(0.5, 0.5)) * cell
			var inside := false
			for b in boxes:
				if mid.x > float(b[0]) and mid.x < float(b[2]) and mid.y > float(b[1]) and mid.y < float(b[3]):
					inside = true
					break
			if not inside:
				g.set_point_solid(c, false)
	g.set_point_solid(from, false) # where we stand, whatever the grid makes of it
	if g.is_point_solid(dest):
		# The goal is in a footprint (someone indoors): the nearest open cell to it.
		var best := dest
		for r in range(1, 12):
			for d in [Vector2i(r, 0), Vector2i(-r, 0), Vector2i(0, r), Vector2i(0, -r)]:
				var c: Vector2i = dest + d
				if g.is_in_boundsv(c) and not g.is_point_solid(c):
					best = c
					break
			if best != dest:
				break
		dest = best
	var pts := g.get_point_path(from, dest, true)
	if pts.size() < 2:
		return []
	# Every few cells is enough to steer by.
	var out := []
	for k in range(2, pts.size(), 3):
		out.append(pts[k] + Vector2(cell, cell) / 2.0)
	out.append(pts[pts.size() - 1] + Vector2(cell, cell) / 2.0)
	return out

## Against a wall on the way to `to`: back off a step, then look along it either side for the nearest place a
## straight walk on gets through (a gate, the wall's end); the path [along to it, through], or [] for none.
func _find_gap(to: Vector2) -> Array:
	var along: Vector2 = (to - m.me_pos).normalized()
	var perp := along.orthogonal()
	var boxes := _solid_near(m.me_pos, 500.0)
	var base: Vector2 = m.me_pos - along * 6.0
	if not _clear(m.me_pos, base, boxes):
		base = m.me_pos
	for d in range(0, 480, 4):
		for side in [1.0, -1.0]:
			var p: Vector2 = base + perp * side * float(d)
			if _clear(p, p + along * 30.0, boxes) and _clear(base, p, boxes):
				return [p, p + along * 30.0]
	return []
var _detour := Vector2.INF
var _detour_until := 0.0
var _wedged_at := Vector2.INF
var _wriggle_until := 0.0
## Progress towards the goal, not just movement: dodging left and right in front of something counts as moving,
## so a walk that has come no nearer in PROGRESS_SECS is stuck too.
const PROGRESS_SECS := 8.0
var _aim := Vector2.INF
var _best_d := INF
var _best_t := 0.0
func _steer(to: Vector2) -> void:
	# A new goal, not the same one shifting (a deer in sight moves; the nearest of a kind's homes changes as we go).
	if _aim == Vector2.INF or _aim.distance_to(to) > 150.0:
		_aim = to
		_best_d = INF
		_best_t = now()
	var d: float = m.me_pos.distance_to(to)
	if d < _best_d - 3.0 or d < 15.0 or _detour != Vector2.INF:
		_best_d = minf(d, _best_d) # a detour may lead away a while; close by, walking about is the point
		_best_t = now()
	var no_nearer := now() - _best_t > PROGRESS_SECS
	if now() - _check_at > 0.8:
		if no_nearer and _detour == Vector2.INF:
			_stalls = maxi(_stalls, 2) # going nowhere: plan round it now
			_best_t = now()
		if _from != Vector2.INF and (m.me_pos.distance_to(_from) < 2.0 or _stalls >= 2 and no_nearer):
			_dodge_until = now() + 1.5
			_side = -_side
			_stalls += 1
			if _detour != Vector2.INF and _stalls >= 6:
				say("stuck on the way round at %d, %d heading for %d, %d" % [int(m.me_pos.x), int(m.me_pos.y), int(_detour.x), int(_detour.y)])
				# Stuck twice in one spot: wedged (a pocket in a hedge narrower than the planner's grid sees).
				if _wedged_at != Vector2.INF and m.me_pos.distance_to(_wedged_at) < 3.0:
					_wriggle_until = now() + 8.0
					say("wedged: wriggling out")
				_wedged_at = m.me_pos
				_detour = Vector2.INF # stuck on the path too: plan again from here
				_detour_path = []
				_stalls = 3
			if _stalls >= 3 and _detour == Vector2.INF:
				var along: Vector2 = (to - m.me_pos).normalized()
				var gap := _plan(to)
				if gap.is_empty():
					gap = _find_gap(to)
				if gap.is_empty():
					var reach := 40.0 + 30.0 * float(_stalls - 4)
					_detour_path = [m.me_pos + along.orthogonal() * _side * reach - along * 10.0]
				else:
					_detour_path = gap
					var shown := []
					for k in range(0, gap.size(), maxi(gap.size() / 6, 1)):
						shown.append("%d,%d" % [int(gap[k].x), int(gap[k].y)])
					say("in the way at %d, %d: a path of %d steps round it (%s)" % [int(m.me_pos.x), int(m.me_pos.y), gap.size(), " ".join(shown)])
				# Time enough to walk it (a way round a city's walls is near a thousand units), and some over.
				var length := 0.0
				var prev: Vector2 = m.me_pos
				for pt in _detour_path:
					length += prev.distance_to(pt)
					prev = pt
				_detour = _detour_path.pop_front()
				_detour_until = now() + 20.0 + length / 6.0
		elif _from != Vector2.INF and m.me_pos.distance_to(_from) > 4.0 and _detour == Vector2.INF:
			_stalls = maxi(_stalls - 1, 0)
		_from = m.me_pos
		_check_at = now()
	if _detour != Vector2.INF:
		if m.me_pos.distance_to(_detour) < 2.5 and not _detour_path.is_empty():
			_detour = _detour_path.pop_front()
		elif m.me_pos.distance_to(_detour) < 3.0 or now() > _detour_until:
			_detour = Vector2.INF
			_detour_path = []
			if _stalls > 12:
				_stalls = 4 # round again from near
		else:
			to = _detour
	var dir: Vector2 = (to - m.me_pos).normalized()
	if now() < _wriggle_until:
		# Sweep round every way, slowly: sliding along the bushes finds the gap we came in by.
		var a := now() * 1.6
		m.net.set_move(Vector2(cos(a), sin(a)))
		return
	if now() < _dodge_until and _detour == Vector2.INF:
		dir = (dir.orthogonal() * _side + dir * 0.3).normalized()
	m.net.set_move(dir)

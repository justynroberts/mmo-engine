extends Node
## What the player hears: ambience the land makes round them (the sea, a river, birds or crickets, wind, rain, fire, a
## town), the ambience of a place that has one of its own (a crypt, a church, a market), music for the moment, and one-shot
## effects. Ambience and effects are made by scripts/make_audio.py; music is <mood>.ogg (made there too) plus any
## <mood>_N.ogg pieces beside it (see sounds/music/CREDITS.md). Nothing here decides what the world is, it only listens.
##
## The client sets `want` (ambience name -> 0..1) and `music_for` a few times a second; layers ease towards what is wanted,
## so walking to the shore brings the sea up slowly. "sea" is the broad sea all round; "shore" is surf breaking close by
## (a recording, see sounds/ambience/CREDITS.md), loud at the water's edge and gone a few dozen units inland. Music plays
## one piece of the mood through, then another (never the same twice running when there is a choice), after a little
## silence for the quieter moods; a change of mood crosses over.

const AMBIENCE := ["sea", "shore", "river", "forest", "night", "wind", "rain", "fire", "town", "market", "crypt", "holy",
	"battle"]
const MUSIC := ["explore", "town", "night", "battle", "eerie", "holy", "city"]
## How loud each layer is at full strength, in dB, so they sit together.
const AMB_DB := {"sea": -4.0, "shore": -3.0, "river": -6.0, "forest": -7.0, "night": -8.0, "wind": -16.0, "rain": -5.0,
	"fire": -6.0, "town": -8.0, "market": -6.0, "crypt": -5.0, "holy": -7.0, "battle": -8.0}
const MUSIC_DB := -13.0
## Moods that play louder (or softer) than MUSIC_DB, in dB: a fight should be heard over the din.
const MUSIC_MOOD_DB := {"battle": 6.0}
const SFX_DB := -4.0
const FADE := 0.6          # ambience: share of the way to what is wanted, per second
const MUSIC_FADE := 5.0    # seconds to cross over when the mood changes
const TRACK_FADE := 4.0    # seconds of overlap when one piece of a mood runs into the next
## Silence (seconds, min..max) between pieces for the quieter moods; the others run straight on with a crossfade.
const MUSIC_GAP := {"city": Vector2(8.0, 20.0), "explore": Vector2(4.0, 18.0), "night": Vector2(6.0, 20.0), "holy": Vector2(3.0, 10.0),
	"eerie": Vector2(5.0, 16.0)}
## Pieces made for this game, picked twice as often as the others of their mood.
const MUSIC_FAVOURED := {"explore_2": 2.0, "explore_3": 2.0, "eerie_5": 2.0, "battle_5": 2.0}
## Fights start suddenly: battle music comes up this fast (it still fades out over MUSIC_FADE when the fight ends).
const BATTLE_IN := 1.5
const MUSIC_MAX_TAKES := 24
const VOICES := 14         # effects that can sound at once
## Incidental sounds: now and then one real recording from somewhere round about (sounds/sfx/amb_<name>_N.ogg, see
## sounds/sfx/CREDITS.md): a smith at the anvil or a dog in a town, a cow or a rooster by a farm. The client sets
## `incidental` to a set below ("" for none) and `daylight`. One at a time, a long random wait between them, quiet, off
## to one side and a little dulled, so they are colour, never a din. Weights: how often each is picked.
const INCIDENTAL := {
	"town": {"anvil": 5.0, "dog": 2.0, "horse": 1.5, "hens": 1.0},
	"farm": {"cow": 3.0, "sheep": 3.0, "hens": 2.0, "rooster": 1.5, "pig": 1.5, "goat": 1.5},
}
## Seconds of quiet (min..max) between one incidental ending and the next; half as long again at night.
const INCIDENTAL_GAP := {"town": Vector2(12.0, 40.0), "farm": Vector2(18.0, 50.0)}
const INCIDENTAL_DAY := ["anvil", "rooster", "hens"] # abed at night: the smith, and the birds
const INCIDENTAL_DB := Vector2(-11.0, -4.0)          # loudness, picked at random (the files sit at about -24 LUFS)
const INCIDENTAL_PITCH := 0.04
const INCIDENTAL_PAN := [-0.85, -0.45, 0.45, 0.85]   # one bus each; never dead centre, so nothing is in your head
const INCIDENTAL_AGAIN := 0.35 # chance the smith strikes again after a short pause
const INCIDENTAL_REARM := 8.0  # off this long (left town) and the next arrival waits afresh before the first sound

var want := {}             # ambience -> 0..1
var music_for := "explore" # the piece the moment calls for ("" for silence)
## What is on: 0 everything, 1 effects and ambience but no music, 2 nothing.
var mode := 0
var incidental := ""       # "town", "farm" or "" (INCIDENTAL)
## The player's volume for each kind of sound (Settings), in dB on top of the levels here.
var gain := {"music": 0.0, "ambience": 0.0, "effects": 0.0}
var daylight := 1.0        # 0 night .. 1 day; under 0.35 the day-only incidentals keep quiet
var _amb := {}             # name -> AudioStreamPlayer
var _vol := {}             # name -> 0..1 now
var _music: Array[AudioStreamPlayer] = []  # three, so a quick change back and forth never cuts a piece off
var _music_gain: Array[float] = []        # 0..1 per player, heard through an equal-power curve
var _music_rate: Array[float] = []        # gain per second (+ coming up, - going down)
var _music_mood: Array[String] = []       # the mood each player is playing (for MUSIC_MOOD_DB)
var _music_cur := 0        # the player holding the mood's current piece
var _music_now := ""
var _music_wait := -1.0    # seconds of silence left before the next piece (-1: none pending)
var _music_tracks := {}    # mood -> Array of paths
var _music_last := {}      # mood -> path played last
var _voices: Array[AudioStreamPlayer] = []
var _next_voice := 0
var _streams := {}         # path -> AudioStream (or null when missing)
var _takes := {}           # effect -> how many takes it has (name_1, name_2, ...)
var _inc: AudioStreamPlayer
var _inc_buses: Array[StringName] = []
var _inc_wait := 0.0       # seconds before the next incidental may start
var _inc_off := INF        # seconds since `incidental` was last set
var _inc_last := ""        # the one played last, not picked twice running when there is a choice
var _inc_again := -1.0     # seconds before the smith's next burst (-1: none pending)
var _inc_bus := 0

func _ready() -> void:
	for n in AMBIENCE:
		var p := AudioStreamPlayer.new()
		p.stream = _loop("res://sounds/ambience/%s.ogg" % n)
		p.volume_db = -80.0
		add_child(p)
		_amb[n] = p
		_vol[n] = 0.0
	for i in 3:
		var m := AudioStreamPlayer.new()
		m.volume_db = -80.0
		add_child(m)
		_music.append(m)
		_music_gain.append(0.0)
		_music_rate.append(0.0)
		_music_mood.append("")
	for i in VOICES:
		var v := AudioStreamPlayer.new()
		add_child(v)
		_voices.append(v)
	# Incidentals go through buses of their own: dulled a touch, a little room round them, and panned to one side.
	for i in INCIDENTAL_PAN.size():
		var bus := StringName("Incidental%d" % i)
		if AudioServer.get_bus_index(bus) < 0:
			var k := AudioServer.bus_count
			AudioServer.add_bus(k)
			AudioServer.set_bus_name(k, bus)
			AudioServer.set_bus_send(k, &"Master")
			var lp := AudioEffectLowPassFilter.new()
			lp.cutoff_hz = 5500.0
			AudioServer.add_bus_effect(k, lp)
			var room := AudioEffectReverb.new()
			room.room_size = 0.35
			room.damping = 0.6
			room.dry = 1.0
			room.wet = 0.12
			AudioServer.add_bus_effect(k, room)
			var pan := AudioEffectPanner.new()
			pan.pan = INCIDENTAL_PAN[i]
			AudioServer.add_bus_effect(k, pan)
		_inc_buses.append(bus)
	_inc = AudioStreamPlayer.new()
	add_child(_inc)

func _loop(path: String) -> AudioStream:
	var s := _stream(path)
	if s is AudioStreamOggVorbis:
		(s as AudioStreamOggVorbis).loop = true
	return s

func _stream(path: String) -> AudioStream:
	if not _streams.has(path):
		_streams[path] = load(path) if ResourceLoader.exists(path) else null
	return _streams[path]

## Cycle what is on: everything, no music, nothing. Returns a word for it.
func cycle() -> String:
	mode = (mode + 1) % 3
	return ["sound and music on", "music off", "sound off"][mode]

func _process(delta: float) -> void:
	var quiet := mode == 2
	for n in AMBIENCE:
		var target: float = 0.0 if quiet else clampf(float(want.get(n, 0.0)), 0.0, 1.0)
		var v: float = _vol[n]
		v += (target - v) * minf(1.0, delta * FADE)
		if absf(v - target) < 0.002:
			v = target
		_vol[n] = v
		var p: AudioStreamPlayer = _amb[n]
		if v > 0.003 and p.stream:
			if not p.playing:
				p.play(randf() * maxf(0.0, p.stream.get_length() - 1.0)) # start each loop somewhere new
			p.volume_db = AMB_DB.get(n, -6.0) + linear_to_db(v) + float(gain["ambience"])
		elif p.playing:
			p.stop()
	_music_step(delta, "" if mode != 0 else music_for)
	_incidental_step(delta)

func _incidental_step(delta: float) -> void:
	if mode == 2:
		if _inc.playing:
			_inc.stop()
		_inc_again = -1.0
		return
	if not INCIDENTAL.has(incidental):
		_inc_off += delta
		_inc_again = -1.0
		return
	if _inc_off > INCIDENTAL_REARM:
		# Just arrived: a while before the first one, so walking past a house is not a farmyard.
		var g0: Vector2 = INCIDENTAL_GAP[incidental]
		_inc_wait = randf_range(g0.x * 0.3, g0.y * 0.6)
	_inc_off = 0.0
	if _inc.playing:
		return
	if _inc_again >= 0.0:
		_inc_again -= delta
		if _inc_again < 0.0:
			_incidental_play("anvil", _inc_bus, -2.0)
		return
	_inc_wait -= delta
	if _inc_wait > 0.0:
		return
	var g: Vector2 = INCIDENTAL_GAP[incidental] * (1.5 if daylight < 0.35 else 1.0)
	_inc_wait = randf_range(g.x, g.y)
	var what := _incidental_pick()
	if what == "":
		return
	_incidental_play(what, randi() % _inc_buses.size(), 0.0)
	if what == "anvil" and randf() < INCIDENTAL_AGAIN:
		_inc_again = randf_range(0.8, 2.5)

## A random incidental from the current set by its weight, minus the day-only ones at night and the last one played.
func _incidental_pick() -> String:
	var pool: Dictionary = INCIDENTAL[incidental]
	var choice := {}
	for n in pool:
		if daylight < 0.35 and n in INCIDENTAL_DAY:
			continue
		choice[n] = float(pool[n])
	if choice.size() > 1:
		choice.erase(_inc_last)
	var total := 0.0
	for n in choice:
		total += choice[n]
	var r := randf() * total
	for n in choice:
		r -= choice[n]
		if r <= 0.0:
			return n
	return ""

func _incidental_play(what: String, bus: int, db: float) -> void:
	var s := _take("amb_" + what)
	if s == null:
		return
	_inc_last = what
	_inc_bus = bus
	_inc.stream = s
	_inc.bus = _inc_buses[bus]
	_inc.volume_db = randf_range(INCIDENTAL_DB.x, INCIDENTAL_DB.y) + db + float(gain["ambience"])
	_inc.pitch_scale = 1.0 + randf_range(-INCIDENTAL_PITCH, INCIDENTAL_PITCH)
	_inc.play()

## The incidental sounding now ("" when none is), for the smoke test.
func incidental_playing() -> String:
	return _inc_last if _inc.playing else ""

func _music_step(delta: float, piece: String) -> void:
	if piece != _music_now:
		# The old piece fades from where it is; the new one comes up from silence.
		_music_now = piece
		_music_wait = -1.0
		_fade_out(_music_cur, MUSIC_FADE)
		if piece != "":
			_music_start(BATTLE_IN if piece == "battle" else MUSIC_FADE)
	elif piece != "":
		var p := _music[_music_cur]
		var left := 0.0
		if p.playing and p.stream:
			left = p.stream.get_length() - p.get_playback_position()
		if _music_wait >= 0.0:
			_music_wait -= delta
			if _music_wait < 0.0:
				_music_start(1.5)
		elif not p.playing or (left < TRACK_FADE and _music_rate[_music_cur] >= 0.0):
			if MUSIC_GAP.has(piece):
				# Let this one end on its own fade, then a little quiet.
				var g: Vector2 = MUSIC_GAP[piece]
				_music_wait = left + randf_range(g.x, g.y)
			else:
				_fade_out(_music_cur, TRACK_FADE)
				_music_start(TRACK_FADE)
	for i in _music.size():
		var m := _music[i]
		_music_gain[i] = clampf(_music_gain[i] + _music_rate[i] * delta, 0.0, 1.0)
		if _music_rate[i] < 0.0 and _music_gain[i] <= 0.0 and m.playing:
			m.stop()
		m.volume_db = MUSIC_DB + float(MUSIC_MOOD_DB.get(_music_mood[i], 0.0)) + linear_to_db(maxf(sin(_music_gain[i] * PI * 0.5), 0.0001)) + float(gain["music"])

func _fade_out(i: int, secs: float) -> void:
	if _music[i].playing:
		_music_rate[i] = -1.0 / secs

## Start a piece of the current mood on the quietest other player, coming up over `secs`; it becomes the current one.
func _music_start(secs: float) -> void:
	var path := _music_pick(_music_now)
	var s := _stream(path) if path != "" else null
	if s == null:
		return
	if s is AudioStreamOggVorbis:
		(s as AudioStreamOggVorbis).loop = false
	var i := -1
	for k in _music.size():
		if k != _music_cur and (i < 0 or _music_gain[k] < _music_gain[i]):
			i = k
	var m := _music[i]
	m.stream = s
	m.play()
	_music_gain[i] = 0.0
	_music_rate[i] = 1.0 / secs
	_music_mood[i] = _music_now
	_music_cur = i
	_music_last[_music_now] = path

## A random piece for `mood` (<mood>.ogg and <mood>_N.ogg), not the last one played when there is another.
func _music_pick(mood: String) -> String:
	if not _music_tracks.has(mood):
		var found: Array[String] = []
		var base := "res://sounds/music/%s.ogg" % mood
		if ResourceLoader.exists(base):
			found.append(base)
		for k in range(1, MUSIC_MAX_TAKES + 1):
			var p := "res://sounds/music/%s_%d.ogg" % [mood, k]
			if ResourceLoader.exists(p):
				found.append(p)
		_music_tracks[mood] = found
	var all: Array = _music_tracks[mood]
	var choice := all.filter(func(p): return p != _music_last.get(mood, ""))
	if choice.is_empty():
		choice = all
	if choice.is_empty():
		return ""
	var total := 0.0
	for p in choice:
		total += _music_weight(p)
	var r := randf() * total
	for p in choice:
		r -= _music_weight(p)
		if r <= 0.0:
			return p
	return choice[-1]

func _music_weight(path: String) -> float:
	return float(MUSIC_FAVOURED.get(path.get_file().get_basename(), 1.0))

## Play an effect (a random take of it, if it has several) at `db` from its usual loudness, pitched a little at random.
func play(name: String, db := 0.0, spread := 0.06) -> void:
	if mode == 2:
		return
	var s := _take(name)
	if s == null:
		return
	var v := _voices[_next_voice]
	_next_voice = (_next_voice + 1) % VOICES
	v.stream = s
	v.volume_db = SFX_DB + db + float(gain["effects"])
	v.pitch_scale = 1.0 + randf_range(-spread, spread)
	v.play()

## A random take of effect `name` (sounds/sfx/<name>_N.ogg, or <name>.ogg when it has only one), or null.
func _take(effect: String) -> AudioStream:
	if not _takes.has(effect):
		var k := 0
		while ResourceLoader.exists("res://sounds/sfx/%s_%d.ogg" % [effect, k + 1]):
			k += 1
		_takes[effect] = k
	var n: int = _takes[effect]
	return _stream("res://sounds/sfx/%s.ogg" % effect if n == 0 else "res://sounds/sfx/%s_%d.ogg" % [effect, randi_range(1, n)])

## Loudness for something `d` units away: full within 4, nothing past `far`.
static func at_distance(d: float, far := 40.0) -> float:
	return linear_to_db(clampf(1.0 - (d - 4.0) / (far - 4.0), 0.0, 1.0) + 0.0001)

## How loud each ambience is now (for the smoke test and the console readout).
func levels() -> Dictionary:
	return _vol.duplicate()

## The mood whose music is sounding now ("" when none is, including the quiet between pieces).
func music_playing() -> String:
	for m in _music:
		if m.playing and _music_now != "":
			return _music_now
	return ""

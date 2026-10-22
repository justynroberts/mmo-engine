extends Node
## What the player hears: ambience the land makes round them (the sea, a river, birds or crickets, wind, rain, fire, a
## town), the ambience of a place that has one of its own (a crypt, a church, a market), music for the moment, and one-shot
## effects. Every sound is made by scripts/make_audio.py; nothing here decides what the world is, it only listens.
##
## The client sets `want` (ambience name -> 0..1) and `music_for` a few times a second; layers ease towards what is wanted,
## so walking to the shore brings the sea up slowly, and music crosses over between pieces.

const AMBIENCE := ["sea", "river", "forest", "night", "wind", "rain", "fire", "town", "market", "crypt", "holy", "battle"]
const MUSIC := ["explore", "town", "night", "battle", "eerie", "holy"]
## How loud each layer is at full strength, in dB, so they sit together.
const AMB_DB := {"sea": -4.0, "river": -6.0, "forest": -7.0, "night": -8.0, "wind": -9.0, "rain": -5.0, "fire": -6.0,
	"town": -8.0, "market": -6.0, "crypt": -5.0, "holy": -7.0, "battle": -8.0}
const MUSIC_DB := -13.0
const SFX_DB := -4.0
const FADE := 0.6          # ambience: share of the way to what is wanted, per second
const MUSIC_FADE := 4.0    # seconds to cross from one piece to the next
const VOICES := 14         # effects that can sound at once

var want := {}             # ambience -> 0..1
var music_for := "explore" # the piece the moment calls for ("" for silence)
## What is on: 0 everything, 1 effects and ambience but no music, 2 nothing.
var mode := 0
var _amb := {}             # name -> AudioStreamPlayer
var _vol := {}             # name -> 0..1 now
var _music: Array[AudioStreamPlayer] = []
var _music_now := ""
var _music_mix := 1.0      # how far the newest piece has come up
var _voices: Array[AudioStreamPlayer] = []
var _next_voice := 0
var _streams := {}         # path -> AudioStream (or null when missing)
var _takes := {}           # effect -> how many takes it has (name_1, name_2, ...)

func _ready() -> void:
	for n in AMBIENCE:
		var p := AudioStreamPlayer.new()
		p.stream = _loop("res://sounds/ambience/%s.ogg" % n)
		p.volume_db = -80.0
		add_child(p)
		_amb[n] = p
		_vol[n] = 0.0
	for i in 2:
		var m := AudioStreamPlayer.new()
		m.volume_db = -80.0
		add_child(m)
		_music.append(m)
	for i in VOICES:
		var v := AudioStreamPlayer.new()
		add_child(v)
		_voices.append(v)

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
			p.volume_db = AMB_DB.get(n, -6.0) + linear_to_db(v)
		elif p.playing:
			p.stop()
	_music_step(delta, "" if mode != 0 else music_for)

func _music_step(delta: float, piece: String) -> void:
	if piece != _music_now:
		# The old piece fades from where it is; the new one comes up from silence.
		_music.reverse()
		_music_now = piece
		_music_mix = 0.0
		var s := _loop("res://sounds/music/%s.ogg" % piece) if piece != "" else null
		if s:
			_music[0].stream = s
			_music[0].play()
	_music_mix = minf(1.0, _music_mix + delta / MUSIC_FADE)
	var up := _music_mix if _music_now != "" else 0.0
	_music[0].volume_db = MUSIC_DB + linear_to_db(maxf(up, 0.0001))
	_music[1].volume_db = MUSIC_DB + linear_to_db(maxf(1.0 - _music_mix, 0.0001))
	if _music_mix >= 1.0 and _music[1].playing:
		_music[1].stop()
	if _music[0].playing and up <= 0.0:
		_music[0].stop()

## Play an effect (a random take of it, if it has several) at `db` from its usual loudness, pitched a little at random.
func play(name: String, db := 0.0, spread := 0.06) -> void:
	if mode == 2:
		return
	if not _takes.has(name):
		var k := 0
		while ResourceLoader.exists("res://sounds/sfx/%s_%d.ogg" % [name, k + 1]):
			k += 1
		_takes[name] = k
	var n: int = _takes[name]
	var path := "res://sounds/sfx/%s.ogg" % name if n == 0 else "res://sounds/sfx/%s_%d.ogg" % [name, randi_range(1, n)]
	var s := _stream(path)
	if s == null:
		return
	var v := _voices[_next_voice]
	_next_voice = (_next_voice + 1) % VOICES
	v.stream = s
	v.volume_db = SFX_DB + db
	v.pitch_scale = 1.0 + randf_range(-spread, spread)
	v.play()

## Loudness for something `d` units away: full within 4, nothing past `far`.
static func at_distance(d: float, far := 40.0) -> float:
	return linear_to_db(clampf(1.0 - (d - 4.0) / (far - 4.0), 0.0, 1.0) + 0.0001)

## How loud each ambience is now (for the smoke test and the console readout).
func levels() -> Dictionary:
	return _vol.duplicate()

func music_playing() -> String:
	return _music_now if _music[0].playing else ""

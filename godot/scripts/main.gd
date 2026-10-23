extends Node3D
## The client: sign in as a guest, walk the world, target things, loot, chat. It knows nothing about how
## anything looks: the world is data (the server's world file, WORLD.md, read by scripts/world_data.gd) and a
## theme (res://themes/<name>/theme.gd, the contract is scripts/theme.gd) draws it. --theme NAME picks one;
## the default is fantasy when it is there, else plain.
##
## Run:  godot --path godot -- --host 127.0.0.1 --udp 7777 --web 127.0.0.1:8081 (without --host it asks: scripts/join.gd)
## Test: add --smoke --admin 127.0.0.1:8080 to run a scripted session headless and exit 0 on success.
## Class: --class knight (default), barbarian, mage or rogue. --guest plays a throwaway character, not this machine's own.
## Screenshots: --breaking holds the break button (build mode), --zoom N sets the camera distance, --walk X,Y (or near) steers to that world point (or beside the nearest building) first, --build starts in build mode, --fpv in first person (--pitch R to look up or down), --time 0..1 pins the time of day (0.25 noon, 0.58 sunset, 0.75 midnight), --shot FILE --shot_after SECONDS saves a frame and quits.

const HudScript := preload("res://scripts/hud.gd")
const WorldData := preload("res://scripts/world_data.gd")
const SoundScript := preload("res://scripts/sound.gd")
const JoinScript := preload("res://scripts/join.gd")
const SpellFx := preload("res://scripts/spellfx.gd")
const JournalScript := preload("res://scripts/journal.gd")
const QuestsScript := preload("res://scripts/quests.gd")
const ShopScript := preload("res://scripts/shop.gd")
## Logged out (the menu): the scene reloads to the join screen, even when started with --host.
static var _logged_out := false
## The title theme: it plays over the join and loading screens and fades as the world appears.
const TITLE_THEME := "res://sounds/theme/ar_aghaidh.ogg"
## The keys for the spell bar, in order.
const SPELL_KEYS := [KEY_R, KEY_T, KEY_Y, KEY_U, KEY_I, KEY_O, KEY_P, KEY_Z, KEY_C, KEY_H, KEY_K, KEY_COMMA]
const HOLD_WORDS := ["", "You are mesmerised.", "You are stunned.", "You are rooted to the spot."]
var theme: Node3D        # the look: see scripts/theme.gd
var world: RefCounted    # the world as data: see scripts/world_data.gd

var args := {}
## Zone lines: after an exit hands us to another zone, the scene reloads to load that zone's world. This says where
## to connect (host, udp, web), which identity to keep (a test's throwaway one too) and the zone's name. It is static,
## so it outlives the reload.
static var _hop := {}
var _hopping := false     # leaving: the dark is coming down
var _arriving := false    # arrived: lift the dark once we are in the world
var my_identity := ""
## Loading: the screen stays dark (scripts/hud.gd loading) and the keys do nothing until we are in, the walls and land
## round us have come and been drawn, and a few frames have gone by to compile what they need.
var _loading := false
var _walls_ready := false
var _blocks_ready := false
var _load_frames := 0
var _load_since := 0.0
var _final_for := 0.0     # seconds in the last stage of loading (it lifts after 8 regardless)
var _smooth := 0           # frames in a row that came quickly (no stall from compiling), while loading
var _was_in := false       # we have seen ourselves in the world on this connection's server
var _retries := 0          # reconnects tried before ever getting in
var join: CanvasLayer      # the join screen (scripts/join.gd) while it is up, for a client started without --host
var info := {}
var info_text := ""
var net: Object
var hud: CanvasLayer
var sound: Node           # what the player hears: scripts/sound.gd
var _listen_in := 0.0     # until the next look round for what to hear
var _music_pick := ""     # the piece the moment calls for, and since when (music changes only once it has settled)
var _music_since := 0.0
var _fetch_pile := 0       # a pile of loot clicked, which we walk to and pick up (its ground id; 0 none)
var _fetch_until := 0.0
const PICKUP_REACH := 2.6  # a little inside the server's 3 units, so it is in reach when we ask
var _fight_at := -100.0   # when we last struck or were struck: the music turns to battle for a while after
var _step_from := Vector2.INF
var _step_dist := 0.0
var _surface := ""
var _bag_count := -1
var cam := Camera3D.new()
var yaw := 0.0
var zoom := 17.0
var heading := 0.0        # the way we are going, in the camera's terms (yaw), kept while we stand still
var has_lit := false      # the game has a `lit` attribute: lights are known to the server, so you can sneak
var lit_of := {}          # entity id -> whether its light is on (as the snapshots say)
var my_lit := true        # ours, as the server has it

var _heading_from := Vector2.ZERO
var fpv := false          # first person: the camera at our eyes, the mouse looks (V, or scroll all the way in)
var look_pitch := -0.12   # up and down in first person, radians
const LOOK := 0.0035      # radians per pixel of mouse movement
var ents := {}      # id -> Node3D
var gems := {}      # ground id -> Node3D
const MapScript := preload("res://scripts/map.gd")
var world_map: Control
var weather := {}         # what /play/info says now: {name, color, intensity}, empty for clear sky
var _shown_weather := {}  # what the theme is drawing: a zone's weather where we stand, else `weather`
var _in_zone := ""        # the weather zone we are in, "" for none
var locations: Array = [] # /play/info "locations": titled places, {title, x, y, r}
var _here := ""           # the title of the location we are in, "" for none
var weather_poll := 0.0
var boxes: Array = []
var last_stream := Vector2(-1e9, -1e9)
var stream_t := 0.0
var track := {}     # entity id -> {hp, pos, vel, face_until}
var corpses: Array = [] # [node, seconds left]
var entity_scale := 1.0
const ACT_JUMP := 251
const JUMP_SECS := 0.6
const JUMP_HEIGHT := 2.2
var air_max := 0.0 # max of the server's "airborne" attribute; 0 when the game has none
var action_range := 8.0
var target_ring := MeshInstance3D.new()
var me_pos := Vector2.ZERO
var target_id := 0
var health_max := 100.0
var health_idx := ""
var attack_id := -1
## Spells: the game's actions after the first (which is the plain attack), on the spell bar. By id: when each is ready.
var spells: Array = []
var all_spells: Array = []  # every spell the game has (its actions after the first)
var bar_order: Array = []   # the names on the spell bar, in key order: the player's choice, kept per server
var journal: CanvasLayer    # the spell journal and the menu: scripts/journal.gd
var questui: CanvasLayer    # quest offers, the quest log and the tracker: scripts/quests.gd
var shopui: CanvasLayer     # a merchant's shop: scripts/shop.gd
var _shop_with := 0         # the merchant the shop is open on
var _horses := {}           # entity id -> the horse drawn under a rider
var spell_ready := {}
var _gcd_until := 0.0      # the short pause after any spell (the server's GCD_SECS) before the next
var actions_by_id := {}
var spellfx: Node3D
var my_level := 0          # our level (0 when the game has none)
var _progress := {}        # the server's last word on our level and experience: {level, xp, at, next}
var my_held := 0           # how we are held (proto::game::HoldKind), 0 when free
var title_music: AudioStreamPlayer
var _title_on := false     # the title theme is up: the game's own music waits
var _aloft := {}           # entity id -> {air, since}: how long its "airborne" has stayed put (levitating)
var t_total := 0.0
var smoke := false
var walk_from := Vector2.ZERO
var walk_check := 0.0
var walk_dodge := 0.0
var walk_side := 1.0
var god := false
var next_swing := 0.0
var notice_until := 0.0
var notice := ""
var smoke_log: Array[String] = []
# Building: blocks on one grid in all three directions (/play/info "build"), fetched from /play/blocks. Layer z
# fills heights z * block to (z + 1) * block everywhere, so blocks line up however the ground runs.
const GROUND := -128      # the layer of something on the bare ground
const MATERIAL_KEYS := [KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, KEY_9, KEY_0, KEY_MINUS, KEY_EQUAL, KEY_BRACKETLEFT, KEY_BRACKETRIGHT]
var build := {}           # /play/info "build"; empty if the server does not offer it
var building := false     # build mode (B toggles, Esc leaves); otherwise action mode. Only with build offered.
var build_kind := 0
var blocks := {}          # column key (cy << 15 | cx) -> {"b": base layer, "k": "..01" (a layer each), "o": owners}
var block_owners := []    # owner tags, as /play/blocks numbers them (1 = the first)
var my_tag := ""          # the tag our own blocks carry
var my_layer := GROUND    # the layer our feet are in
var blocks_ver := 0
var blocks_poll := 0.0
var tiles_poll := 0.0
var tiles_busy := false
var world_from_boxes := false # no world file: the walls are what is drawn
var world_parts := false      # the world file comes as a head and point objects by tile
var world_ver := ""
var world_tiles := {}         # Vector2i tile -> true, for the tiles whose point objects are in
var world_poll := 0.0
var world_busy := false
var blocks_busy := false
var ghost := MeshInstance3D.new()       # where the next block would go
var break_mark := MeshInstance3D.new()  # the block holding the right button would take out
# Breaking takes a moment of holding the right button: cracks spread over the block, then it crumbles. Holding on
# while sweeping across blocks carries on to the next.
const BREAK_SECS := 0.45
var cracks := MeshInstance3D.new()      # the cracks, over the block being broken
var _crack_tex: Array[Texture2D] = []   # four stages, each with more cracks than the last
var _break_cell := Vector3i(-1, -1, 0)
var _break_t := 0.0
var _break_why := Vector3i(-1, -1, 0)   # the block we last said could not be broken, so it is said once
var _break_sent := {}                   # cell -> when we broke it, so it crumbles once and is not sent twice
var _debris: Array = []                 # flying pieces: {node, vel, spin, life, age, floor}
var crumbled := 0                       # blocks seen to crumble (the smoke test checks it)
# Names: the web gateway's /play/names, asked for the ids we do not know yet and refreshed now and then (players rename).
var names := {}           # entity id -> name
var _names_want := {}     # ids to ask about next
var _names_busy := false
var _names_poll := 0.0
var _names_refresh := 0.0
var aim_place := Vector3i(-1, -1, 0)    # cell x, cell y, layer; x < 0 for none
var aim_break := Vector3i(-1, -1, 0)

func _ready() -> void:
	args = _parse_args()
	for k in ["host", "udp", "web"]:
		if _hop.has(k):
			args[k] = _hop[k]
	if _hop.has("to"):
		args.erase("walk") # where --walk pointed was on the map we left
	smoke = args.has("smoke")
	# The packaged app opens full screen (F11 or --windowed for a window); run from the project it stays windowed.
	if args.has("fullscreen") or (OS.has_feature("template") and not args.has("windowed")):
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)
	if args.has("zoom"):
		zoom = clampf(float(args["zoom"]), 8.0, 60.0)
	_build_scene()
	hud = HudScript.new()
	add_child(hud)
	sound = SoundScript.new()
	add_child(sound)
	# Screenshots, smoke tests and --mute play nothing.
	if smoke or args.has("shot") or args.has("mute"):
		sound.mode = 2
	sound.music_for = "" # nothing from the game until we are in it
	if sound.mode != 2 and _hop.is_empty() and ResourceLoader.exists(TITLE_THEME):
		var theme_stream: AudioStream = load(TITLE_THEME)
		if theme_stream is AudioStreamOggVorbis:
			(theme_stream as AudioStreamOggVorbis).loop = true
		title_music = AudioStreamPlayer.new()
		title_music.stream = theme_stream
		title_music.volume_db = -7.0
		add_child(title_music)
		title_music.play()
		_title_on = true
	world_map = MapScript.new()
	world_map.visible = false
	hud.add_child(world_map)
	hud.slot_pressed.connect(func(i: int): net.use_slot(i))
	hud.material_pressed.connect(func(i: int): build_kind = i)
	hud.mode_pressed.connect(func(): _set_building(not building))
	hud.chat_submitted.connect(_on_chat)
	hud.ability_pressed.connect(_on_ability)
	hud.spell_pressed.connect(_cast)
	spellfx = SpellFx.new()
	add_child(spellfx)
	journal = JournalScript.new()
	add_child(journal)
	journal.bar_changed.connect(func(order: Array):
		bar_order = order
		_save_bar()
		_apply_bar())
	journal.log_out.connect(_log_out)
	questui = QuestsScript.new()
	add_child(questui)
	questui.accepted.connect(func(i: int): net.accept_quest(i))
	questui.declined.connect(func(): net.abandon_quest(-1))
	questui.abandoned.connect(func(i: int): net.abandon_quest(i))
	shopui = ShopScript.new()
	add_child(shopui)
	shopui.bought.connect(func(i: int): net.buy(_shop_with, i))
	shopui.sold.connect(func(slot: int): net.sell(_shop_with, slot))
	journal.unequip.connect(func(g: int): net.unequip(g))
	journal.quit.connect(func(): get_tree().quit())
	if smoke:
		hud.status.text = "smoke test"
	if _hop.has("to"):
		hud.travel("Travelling to %s" % _hop["to"], 0.0)
		_arriving = true
	# Packaged and double-clicked, there are no arguments: ask which server and what to be called.
	if (_logged_out or not OS.get_cmdline_user_args().has("--host")) and _hop.is_empty() and not smoke and not args.has("shot"):
		_logged_out = false
		_ask_join("")
		return
	_start()

func _ask_join(why: String) -> void:
	join = JoinScript.new()
	add_child(join)
	join.fail(why)
	join.chosen.connect(func(server: String, who: String):
		join = null
		# "host" alone means the usual web port; the game's UDP port comes from /play/info.
		args["web"] = server if ":" in server else "%s:7790" % server
		args["host"] = server.get_slice(":", 0)
		args["name"] = who
		args["joined"] = true
		_start())

func _parse_args() -> Dictionary:
	var a := {"host": "127.0.0.1", "udp": "7777", "web": "127.0.0.1:8081", "admin": "127.0.0.1:7780"}
	var u := OS.get_cmdline_user_args()
	var i := 0
	while i < u.size():
		var k := u[i].trim_prefix("--")
		if i + 1 < u.size() and not u[i + 1].begins_with("--"):
			a[k] = u[i + 1]
			i += 2
		else:
			a[k] = true
			i += 1
	return a

# ---------------------------------------------------------------- start up

func _fetch(url: String) -> String:
	var r := HTTPRequest.new()
	add_child(r)
	if r.request(url) != OK:
		r.queue_free()
		return ""
	var res: Array = await r.request_completed
	r.queue_free()
	if res[1] != 200:
		return ""
	return (res[3] as PackedByteArray).get_string_from_utf8()

## A GET's status and body as bytes (gzip is taken and undone by HTTPRequest).
func _fetch_raw(url: String) -> Array:
	var r := HTTPRequest.new()
	add_child(r)
	if r.request(url) != OK:
		r.queue_free()
		return [0, PackedByteArray()]
	var res: Array = await r.request_completed
	r.queue_free()
	return [res[1], res[3]]

func _fail(msg: String) -> void:
	push_error(msg)
	if _loading:
		_loading = false
		hud.loaded()
	if args.has("joined") and net == null:
		# Not in yet: back to the join screen to try another address.
		args.erase("joined")
		_ask_join(msg)
		return
	hud.status.text = msg
	if smoke:
		print("SMOKE FAIL: ", msg)
		get_tree().quit(1)
	elif args.has("shot"):
		get_tree().quit(1) # a screenshot run would otherwise wait forever

func _start() -> void:
	if not smoke and not args.has("shot"):
		_loading = true
		hud.loading(0.02, _quip("server"))
	info_text = await _fetch("http://%s/play/info" % args["web"])
	if _loading:
		hud.loading(0.1, _quip("world"))
	if info_text == "":
		return _fail("could not reach http://%s/play/info" % args["web"])
	info = JSON.parse_string(info_text)
	if args.has("joined"):
		args["udp"] = str(int(info.get("udp_port", 7777)))
	# A big world streams its walls by tile as you move (_poll_tiles); a small one sends them here.
	boxes = info.get("boxes", [])
	# The world: the server's world file if it has one, else just its walls on flat ground.
	if info.get("world") is Dictionary:
		# A server that splits its world file sends the head (buildings, heights, water) now and the trees and rocks
		# by tile as we move (_poll_world), so a big world costs what is near, not the whole file.
		var wi: Dictionary = info["world"]
		world_parts = bool(wi.get("parts", false))
		world_ver = String(wi.get("ver", ""))
		var text := await _fetch("http://%s/play/world%s" % [args["web"], "/head" if world_parts else ""])
		world = WorldData.from_file(text) if text != "" else null
		if world == null:
			return _fail("could not read the world file at http://%s/play/world" % args["web"])
	else:
		world = WorldData.from_boxes(float(info.get("world_size", 1024)), boxes)
		world_from_boxes = true
	var theme_name := String(args.get("theme", "fantasy" if ResourceLoader.exists("res://themes/fantasy/theme.gd") else "plain"))
	if not ResourceLoader.exists("res://themes/%s/theme.gd" % theme_name):
		return _fail("no theme called '%s' (looked for res://themes/%s/theme.gd)" % [theme_name, theme_name])
	theme = load("res://themes/%s/theme.gd" % theme_name).new()
	add_child(theme)
	if args.has("time"):
		theme.time_override = float(args["time"])
	for a in info.get("attrs", []):
		has_lit = has_lit or String(a.get("name", "")) == "lit"
	theme.setup(info, world)
	theme.set_zones(info.get("weather_zones", []) if info.get("weather_zones") is Array else [])
	locations = info.get("locations", []) if info.get("locations") is Array else []
	# One picture of the world serves the minimap and the world map (M).
	var map_img: Image = theme.map_image(512)
	hud.set_map(map_img, world.size)
	world_map.build(map_img, world.size)
	var w0 = info.get("weather")
	if w0 is Dictionary:
		_apply_weather.call_deferred(w0)
	# KayKit characters are about one unit across at the hips, so they match a radius of 0.5.
	entity_scale = float(info.get("entity_radius", 0.5)) / 0.5
	for a in info["attrs"]:
		if a["name"] == "airborne" and a["replicated"]:
			air_max = maxf(float(a["max"]), 1.0)
			continue
		if a["replicated"] and (health_idx == "" or a["name"] == "health"):
			health_idx = a["name"]
			health_max = max(float(a["max"]), 1.0)
	if info.get("actions", []).size() > 0:
		attack_id = int(info["actions"][0]["id"])
		action_range = float(info["actions"][0].get("range", 8.0))
	for a in info.get("actions", []):
		actions_by_id[int(a["id"])] = a
	questui.set_quests(info.get("quests", []) if info.get("quests") is Array else [])
	shopui.set_items(info.get("items", []), String(info.get("currency", "")) if info.get("currency") is String else "")
	all_spells = info.get("actions", []).slice(1)
	bar_order = _load_bar()
	_apply_bar()
	build = info.get("build", {})
	if not build.is_empty():
		var gm := BoxMesh.new()
		var bs := float(build["block"])
		gm.size = Vector3(bs, bs, bs) * 1.04
		ghost.mesh = gm
		var mat := StandardMaterial3D.new()
		mat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
		mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		ghost.material_override = mat
		ghost.visible = false
		add_child(ghost)
		var bm := BoxMesh.new()
		bm.size = Vector3(bs, bs, bs) * 1.08
		break_mark.mesh = bm
		break_mark.material_override = mat.duplicate()
		break_mark.visible = false
		add_child(break_mark)
		var cm := BoxMesh.new()
		cm.size = Vector3(bs, bs, bs) * 1.02
		cracks.mesh = cm
		var cmat := StandardMaterial3D.new()
		cmat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		cmat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
		cracks.material_override = cmat
		cracks.visible = false
		add_child(cracks)
		_crack_tex = _make_cracks()
		var names := []
		var swatches := []
		for i in int(build.get("kinds", 4)):
			names.append(theme.block_name(i))
			swatches.append(_swatch(i))
		hud.set_materials(names, swatches)
	if not ClassDB.class_exists("MmoClient"):
		return _fail("the mmo_godot extension is not loaded (build it: scripts/build_godot.sh)")
	net = ClassDB.instantiate("MmoClient")
	add_child(net)
	net.chat_heard.connect(func(from: int, channel: int, text: String):
		if from == 0:
			hud.say("[color=#ffd27a]%s[/color]" % text)
			if channel == 3: # a banner: an event's news, across the screen too, with a horn
				hud.announce(text)
				sound.play("horn", -3.0, 0.0)
		elif channel == 4: # a whisper to us
			hud.say("[color=#e8a0ff]%s whispers:[/color] [color=#f0d8ff]%s[/color]" % [_name(from), text])
			sound.play("chime", -8.0, 0.0)
		elif channel == 5: # our own whisper, echoed
			hud.say("[color=#e8a0ff]To %s:[/color] [color=#f0d8ff]%s[/color]" % [_name(from), text])
		elif channel == 6: # party chat
			hud.say("[color=#8fd3ff][Party] %s:[/color] %s" % ["You" if from == net.get_self_id() else _name(from), text])
		else:
			var who := "You" if from == net.get_self_id() else _name(from)
			hud.say("[color=#9fe8e0]%s%s:[/color] %s" % [who, " shouts" if channel == 1 else "", text]))
	var id := _identity()
	my_identity = id
	if _loading:
		hud.loading(0.25, _quip("connect"))
	var err: String = net.connect_to(String(args["host"]), int(args["udp"]), info_text, id)
	if err != "":
		return _fail(err)
	# Zone lines: a handoff lands on another map, so we load it ourselves (_hop_zone) rather than just reconnect.
	if info.get("zone_lines", false) == true:
		net.set_follow_redirects(false)
	my_tag = net.account_tag()
	if args.has("name") and args["name"] is String:
		net.set_display_name(String(args["name"]))
	if smoke:
		_smoke()
	if args.has("map"):
		_toggle_map.call_deferred()
	if args.has("build"):
		_set_building(true)
	if args.has("fpv"):
		_set_fpv(true)
		if args.has("pitch"):
			look_pitch = clampf(float(args["pitch"]), -1.4, 1.4)
	if args.has("shot"):
		await get_tree().create_timer(float(args.get("shot_after", "5"))).timeout
		get_viewport().get_texture().get_image().save_png(String(args["shot"]))
		print("SHOT %s: phase %.2f darkness %.2f, torch %s, %d light(s) burning, %d in view" % [args["shot"], theme.day_phase(), theme.darkness, theme.torch_lit(), theme.lights_burning(), ents.size()])
		get_tree().quit(0)

func _identity() -> String:
	var path := "user://identity.txt"
	if _hop.has("identity"):
		return String(_hop["identity"])
	if smoke or args.has("shot") or args.has("guest"):
		return net.new_identity() # a test or a screenshot never plays as (or renames) the player's own character
	if FileAccess.file_exists(path):
		return FileAccess.get_file_as_string(path).strip_edges()
	var id: String = net.new_identity()
	var f := FileAccess.open(path, FileAccess.WRITE)
	f.store_string(id)
	return id

# ---------------------------------------------------------------- scene

## Name over the head and a health bar that shows once the entity is hurt. The bar is a billboard sprite
## cropped from the left as health falls, offset by half its width so it stays centred from any camera angle.
func _nameplate(n: Node3D, id: int, mine: bool) -> void:
	var top := 2.75
	var tag := Label3D.new()
	tag.text = "#%d" % (id & 0xFFFFFF) # the low bits are the slot, the high ones its generation
	tag.visible = not mine            # your own name is on your frame, not over your head
	tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	tag.pixel_size = 0.01
	tag.font_size = 30
	tag.font = HudScript.BODY
	tag.outline_size = 8
	tag.modulate = Color("ffe9a8") if mine else Color("f2f2f2")
	tag.position.y = top + 0.4
	tag.no_depth_test = true
	tag.name = "tag"
	n.add_child(tag)
	for part in ["bar_bg", "bar"]:
		var b := Sprite3D.new()
		b.texture = _white()
		b.billboard = BaseMaterial3D.BILLBOARD_ENABLED
		b.pixel_size = 0.02
		b.centered = false
		b.region_enabled = true
		b.region_rect = Rect2(0, 0, 64 if part == "bar" else 66, 6 if part == "bar" else 8)
		b.offset = Vector2(-32, -3) if part == "bar" else Vector2(-33, -4)
		b.no_depth_test = true
		b.shaded = false
		b.render_priority = 1 if part == "bar" else 0
		b.modulate = Color(0.06, 0.05, 0.05, 0.85) if part == "bar_bg" else Color("5ac85a")
		b.position.y = top
		b.name = part
		b.visible = false
		n.add_child(b)

var _white_tex: ImageTexture
func _white() -> ImageTexture:
	if _white_tex == null:
		var img := Image.create(66, 8, false, Image.FORMAT_RGBA8)
		img.fill(Color.WHITE)
		_white_tex = ImageTexture.create_from_image(img)
	return _white_tex

func _set_health_bar(n: Node3D, hp: float, show: bool) -> void:
	if not n.has_node("bar"):
		return
	var bar := n.get_node("bar") as Sprite3D
	var bg := n.get_node("bar_bg") as Sprite3D
	bar.visible = show and hp > 0.0
	bg.visible = show
	bar.region_rect = Rect2(0, 0, maxf(1.0, round(64.0 * hp)), 6)
	bar.modulate = Color("d9412e").lerp(Color("5ac85a"), clampf(hp * 1.4 - 0.2, 0.0, 1.0))

var _perf_t := 0.0

func _process(delta: float) -> void:
	if args.has("perf"): # --perf: frame rate, frame time, draw calls and the like every two seconds
		_perf_t += delta
		if _perf_t >= 2.0:
			_perf_t = 0.0
			print("PERF fps %d  frame %.1f ms  process %.1f ms  draws %d  prims %dk  objects %d  nodes %d  ents %d  vram %d MB" % [
				Performance.get_monitor(Performance.TIME_FPS), 1000.0 / maxf(Performance.get_monitor(Performance.TIME_FPS), 1.0),
				Performance.get_monitor(Performance.TIME_PROCESS) * 1000.0, Performance.get_monitor(Performance.RENDER_TOTAL_DRAW_CALLS_IN_FRAME),
				Performance.get_monitor(Performance.RENDER_TOTAL_PRIMITIVES_IN_FRAME) / 1000, Performance.get_monitor(Performance.RENDER_TOTAL_OBJECTS_IN_FRAME),
				Performance.get_monitor(Performance.OBJECT_NODE_COUNT), ents.size(), Performance.get_monitor(Performance.RENDER_VIDEO_MEM_USED) / 1048576])
			var vp := get_viewport().get_viewport_rid()
			RenderingServer.viewport_set_measure_render_time(vp, true)
			print("PERF render cpu %.1f ms  lights %d  darkness %.2f  weather %s" % [RenderingServer.viewport_get_measured_render_time_cpu(vp),
				theme.lights_burning(), theme.darkness, _shown_weather.get("name", "clear")])
	if net == null:
		return
	t_total += delta
	var status: int = net.get_status()
	if status == 1 and not _was_in and ents.has(net.get_self_id()):
		_was_in = true
	# Never got in: the first launch compiles shaders and can stall long enough for the handshake to lapse. Try again.
	if status == 3 and not _was_in and _retries < 3 and not smoke:
		_retries += 1
		if net.connect_to(String(args["host"]), int(args["udp"]), info_text, my_identity) == "":
			if args.has("name") and args["name"] is String:
				net.set_display_name(String(args["name"]))
			return
	if _loading:
		_load_step(delta, status)
	if status == 4:
		if not _hopping:
			_hop_zone()
		return
	var list: Array = net.entities()
	var self_id: int = net.get_self_id()
	var seen := {}
	for e in list:
		var id: int = e["id"]
		seen[id] = true
		var mine := id == self_id
		if not ents.has(id):
			var n := _entity_node(id, mine)
			add_child(n)
			ents[id] = n
		var node: Node3D = ents[id]
		var p: Vector2 = e["position"]
		var layer := int(e.get("layer", GROUND))
		var np := Vector3(p.x, _stand_height(p, layer), p.y)
		var hp := clampf(float(e.get(health_idx, health_max)) / health_max, 0.0, 1.0) if health_idx != "" else 1.0
		var air := clampf(float(e.get("airborne", 0.0)) / air_max, 0.0, 1.0) if air_max > 0.0 else 0.0
		_animate(id, node, np, hp, mine, delta, self_id, air)
		# Sneaking: in the dark, someone whose light is out shows no name and no health bar (and carries no torch).
		if not mine and node.has_node("tag"):
			var label := node.get_node("tag") as Label3D
			var nm := _name(id)
			if e.has("level") and float(e["level"]) >= 1.0:
				nm = "%s  %d" % [nm, int(round(float(e["level"])))] # their level beside their name
			if info.get("shops") is Dictionary and (info["shops"] as Dictionary).has(_name(id)):
				nm += "  · trader"
			if label.text != nm:
				label.text = nm
		var unlit := has_lit and float(e.get("lit", 1.0)) < 0.5
		lit_of[id] = not unlit
		if not mine and node.has_node("tag"):
			var hide: bool = unlit and theme.darkness > 0.5
			(node.get_node("tag") as Label3D).visible = not hide
			if hide:
				_set_health_bar(node, hp, false)
		if mine:
			my_lit = not unlit
			me_pos = p
			my_layer = layer
		elif node.has_node("body"):
			var mat := (node.get_node("body") as MeshInstance3D).material_override as StandardMaterial3D
			if mat:
				mat.albedo_color = Color("c8503c").lerp(Color("5ac85a"), hp)
	for id in ents.keys():
		if not seen.has(id):
			# Gone from view. If it was nearly dead it was most likely killed (the server respawns it elsewhere at
			# once), so it falls where it stood; otherwise it just walked out of range.
			var t: Dictionary = track.get(id, {})
			if t.get("hp", 1.0) < 0.35 and t_total - t.get("hit_at", -100.0) < 3.0 and ents[id].has_method("set_dead"):
				_corpse(ents[id])
			else:
				ents[id].queue_free()
			ents.erase(id)
			track.erase(id)
	for c in corpses.duplicate():
		c[1] -= delta
		if c[1] <= 0.0:
			c[0].queue_free()
			corpses.erase(c)
	_sync_ground()
	_poll_names(delta, list)
	_poll_blocks(delta)
	_show_weather()
	if self_id != 0:
		_where()
	_poll_tiles(delta)
	_poll_world(delta)
	_aim()
	_mine(delta)
	_fly_debris(delta)
	var bag: Array = net.bag()
	hud.set_bag(bag)
	_hear_bag(bag)
	_footsteps()
	_listen(delta)
	_camera()
	_weather(delta)
	if world_map.visible:
		var around := []
		for e in list:
			if e["id"] != self_id:
				around.append(e["position"])
		world_map.update_markers(me_pos, heading, around)
	_drive()
	if me_pos.distance_to(_heading_from) > 0.05:
		var d := me_pos - _heading_from
		heading = atan2(-d.x, -d.y) # so (-sin, -cos) of it is the way we went, as forward is for yaw
		_heading_from = me_pos
	stream_t += delta
	if stream_t > 0.4 and me_pos.distance_to(last_stream) > 20.0:
		stream_t = 0.0
		last_stream = me_pos
		theme.update(me_pos)
	_spells_frame(list, self_id)
	# Everyone near us carries a torch at night; the theme gives the nearest ones real light.
	var me_node: Node3D = ents.get(self_id)
	var carried := []
	for id in ents:
		var p: Vector3 = (ents[id] as Node3D).position
		if id != self_id and lit_of.get(id, true) and Vector2(p.x, p.z).distance_squared_to(me_pos) < 1600.0:
			carried.append(p)
	# Our light, for the server (the library sends it when no other action is going, and repeats it).
	if has_lit:
		net.set_light(theme.torch_lit())
	theme.set_characters(me_node.position if me_node else Vector3(me_pos.x, _height(me_pos.x, me_pos.y), me_pos.y), carried)
	theme.frame(me_pos, zoom)
	target_ring.visible = target_id != 0 and ents.has(target_id)
	if target_ring.visible:
		target_ring.position = (ents[target_id] as Node3D).position + Vector3(0, 0.06, 0)
	var me_e := {}
	var tgt_e := {}
	for e in list:
		if e["id"] == self_id:
			me_e = e
		elif e["id"] == target_id:
			tgt_e = e
	if target_id != 0 and ents.has(target_id) and not tgt_e.is_empty():
		hud.set_target(_name(target_id), "%d u away" % int(_dist_to(target_id)), _bars(tgt_e, false))
	else:
		target_id = 0
		hud.set_target("", "", [])
	var cls := String(args.get("class", "knight"))
	var badges := []
	if god:
		badges.append("GOD")
	if has_lit and not my_lit and theme.darkness > 0.5:
		badges.append("SNEAKING")
	if _in_zone != "":
		badges.append(_in_zone.to_upper())
	if info.get("pvp", false) == true:
		badges.append("SAFE" if _in_safe() else "PVP") # other players can hurt you here, or cannot
	var sub := cls.capitalize() if my_level <= 0 else "%s, level %d" % [cls.capitalize(), my_level]
	hud.set_player(_name(self_id) if self_id != 0 else cls.capitalize(), sub, cls.left(1).to_upper(), _bars(me_e, true), badges)
	var others := []
	for e in list:
		if e["id"] != self_id:
			others.append(e["position"])
	hud.update_minimap(me_pos, heading, others)
	if Input.is_key_pressed(KEY_SPACE) and not building and not hud.typing() and not smoke:
		_attack() # holding Space keeps swinging, at the action's cooldown
	# The server sends only the nearest max_visible within interest_radius; say so when that is what you are seeing.
	var cap := int(info.get("max_visible", 0))
	var crowd := ", nearest %d shown" % cap if cap > 0 and list.size() >= cap else ""
	var sky := String(_shown_weather.get("name", "clear")).capitalize() if not _shown_weather.is_empty() else "Clear"
	var when := HudScript.time_words(theme.day_phase(), theme.daylight_night(theme.daylight))
	var place := _here if _here != "" else _in_zone.capitalize() if _in_zone != "" else "The wilds"
	hud.set_info(place, "%s, %s   %d, %d" % [when, sky, me_pos.x, me_pos.y], int(net.get_rtt_ms()), crowd)
	hud.set_mode(building, not build.is_empty(), theme.block_name(build_kind))
	hud.show_materials(building, build_kind)
	hud.set_abilities(theme.torch_mode, theme.torch_lit(), building, world_map.visible, not build.is_empty())
	if status > 1:
		hud.status.text = ["", "", "The server refused the connection.", "Disconnected from the server."][clampi(status, 0, 3)]
	if _arriving and status == 1 and ents.has(self_id):
		_arriving = false
		hud.arrive()

## What the loading screen says at each stage, a new line every few seconds so a slow stage has something to read.
const QUIPS := {
	"server": ["Sending a raven to the server", "Asking the druids for directions", "Knocking politely on the server's door"],
	"world": ["Unrolling the map, carefully", "Counting the sheep (there are many)", "Persuading the rivers to run downhill"],
	"connect": ["Paying the ferryman", "Shaking hands with the gatekeeper", "Swearing it was not us who started the fight"],
	"you": ["Finding where you left yourself", "Polishing your helmet", "Waking your character, who was napping"],
	"walls": ["Stacking stones on stones", "Reminding the walls to be solid", "Building the round towers, again"],
	"land": ["Planting oaks, one at a time", "Placing every rock by hand", "Telling the wolves to wait their turn",
		"Herding stray cattle back into the fields", "Mowing the grass to a reasonable height"],
	"light": ["Lighting the torches", "Hanging the sun up", "Tuning the harps", "Warning the brigands you are coming"],
}

func _quip(stage: String) -> String:
	var lines: Array = QUIPS[stage]
	return lines[int(Time.get_ticks_msec() / 2500) % lines.size()]

## While loading: how far along we are, and lift the screen once everything round us is in and drawn. A slow server
## or a missing piece does not hold it forever: 30 s after getting in it lifts anyway.
func _load_step(delta: float, status: int) -> void:
	if status > 1:
		# Refused, or closed for good: lift the screen so the reason shows. A lapsed first handshake is retried.
		if status == 2 or _was_in or _retries >= 3:
			_loading = false
			hud.loaded()
		return
	if not _was_in:
		hud.loading(0.35, _quip("you"))
		return
	_load_since += delta
	var parts := 0.0
	var have := 0.0
	parts += 1.0
	have += 1.0 if _walls_ready else 0.0
	if world_parts:
		var t := Vector2i(int(floor(me_pos.x / WorldData.TILE)), int(floor(me_pos.y / WorldData.TILE)))
		var n := 0
		for j in range(t.y - 2, t.y + 3):
			for i in range(t.x - 2, t.x + 3):
				n += 1 if world_tiles.has(Vector2i(i, j)) else 0
		parts += 1.0
		have += n / 25.0
	if not build.is_empty():
		parts += 1.0
		have += 1.0 if _blocks_ready else 0.0
	var frac := have / parts
	var text := _quip("walls" if not _walls_ready else "land" if frac < 1.0 else "light")
	if frac < 1.0 and _load_since < 30.0:
		hud.loading(0.4 + 0.45 * frac, text)
		return
	if _load_frames == 0:
		theme.update(me_pos) # draw what has come before we are shown
		theme.flush()
		last_stream = me_pos
		spellfx.prewarm(Vector3(me_pos.x, _height(me_pos.x, me_pos.y), me_pos.y))
	_load_frames += 1
	# The rest that makes the first look right: our own name, our level (when the game has levels), the names of
	# whoever is near, and a few smooth frames in a row (nothing still compiling). 30 s in, it lifts regardless.
	var me_id: int = net.get_self_id()
	var has_levels := false
	for a in info.get("attrs", []):
		has_levels = has_levels or String(a.get("name", "")) == "level"
	var named := names.has(me_id)
	for id in ents:
		if id != me_id and _dist_to(id) < 60.0 and not names.has(id):
			_names_want[id] = true
			named = false
	var levelled := not has_levels or int(net.progress().get("level", 0)) > 0
	_smooth = _smooth + 1 if delta < 0.1 else 0 # a compile stall takes longer than a slow frame
	_final_for += delta
	var ready := named and levelled and _smooth >= 12
	hud.loading(0.85 + 0.15 * minf(1.0, _load_frames / 20.0), text)
	if _load_frames >= 20 and (ready or _final_for >= 8.0 or _load_since >= 30.0):
		_loading = false
		hud.loaded()
		_fade_title()
		_welcome()

## A zone line handed us to another zone: darken the screen, then reload the scene against that zone, keeping our
## identity, so its world loads and its server restores the character it was just handed.
func _hop_zone() -> void:
	_hopping = true
	var m: Dictionary = net.moved()
	var z := int(m.get("zone", -1))
	var webs = info.get("zones")
	var web := String(webs[z]) if webs is Array and z >= 0 and z < webs.size() and webs[z] is String else ""
	if web == "":
		return _fail("handed to zone %d, which has no web address (the server's --zone-web)" % z)
	var names = info.get("zone_names")
	var where := String(names[z]).capitalize() if names is Array and z >= 0 and z < names.size() else "zone %d" % z
	_hop = {"host": String(m["host"]), "udp": str(m["port"]), "web": web.trim_prefix("http://").trim_prefix("https://").trim_suffix("/"), "identity": my_identity, "to": where}
	hud.travel("Travelling to %s" % where)
	await get_tree().create_timer(0.7).timeout
	get_tree().reload_current_scene()

# ---------------------------------------------------------------- names

## What to call an entity: its name once the gateway has told us, its number until then.
func _name(id: int) -> String:
	if names.has(id):
		return names[id]
	_names_want[id] = true
	return "#%d" % (id & 0xFFFFFF)

func _poll_names(delta: float, list: Array) -> void:
	_names_refresh -= delta
	if _names_refresh <= 0.0:
		# Players can rename at any time: ask again about everyone in view every few seconds.
		_names_refresh = 6.0
		for e in list:
			_names_want[int(e["id"])] = true
	_names_poll -= delta
	if _names_busy or _names_poll > 0.0 or _names_want.is_empty():
		return
	_names_poll = 0.5
	_names_busy = true
	var ids: Array = _names_want.keys().slice(0, 256)
	for id in ids:
		_names_want.erase(id)
	var text := await _fetch("http://%s/play/names?ids=%s" % [args["web"], ",".join(ids.map(func(i): return str(i)))])
	_names_busy = false
	var got = JSON.parse_string(text) if text != "" else null
	if got is Dictionary:
		for k in got:
			names[int(k)] = String(got[k])

# ---------------------------------------------------------------- walls by tile

## A world that streams its walls: fetch the /play/tiles the library asks for around us and hand them to it, so
## prediction collides with them. With no world file the walls are also what we draw, so they are added to the
## world as they come and the cells they land in are redrawn.
func _poll_tiles(delta: float) -> void:
	tiles_poll -= delta
	if tiles_busy or tiles_poll > 0.0:
		return
	tiles_poll = 0.25
	var path: String = net.tiles_wanted(me_pos)
	if path == "":
		_walls_ready = _was_in
		return
	tiles_busy = true
	var res := await _fetch_raw("http://%s%s" % [args["web"], path])
	tiles_busy = false
	if int(res[0]) == 200 and net.put_tiles(res[1]):
		if world_from_boxes:
			theme.invalidate(world.add_boxes(_tile_boxes(res[1])))
		return
	if int(res[0]) == 200 or int(res[0]) == 409:
		# The walls changed on the server: start again from a fresh /play/info.
		var text := await _fetch("http://%s/play/info" % args["web"])
		if text != "":
			net.set_walls(text)
	else:
		net.tiles_failed()

## The world file's point objects (trees, rocks) for the 5x5 tiles around us, fetched as we move; tiles more than
## three away are let go. Each tile that arrives or goes has its cells redrawn.
func _poll_world(delta: float) -> void:
	if not world_parts or world_busy or world == null:
		return
	world_poll -= delta
	if world_poll > 0.0:
		return
	world_poll = 0.3
	var t := Vector2i(int(floor(me_pos.x / WorldData.TILE)), int(floor(me_pos.y / WorldData.TILE)))
	for k in world_tiles.keys():
		if absi(k.x - t.x) > 3 or absi(k.y - t.y) > 3:
			world_tiles.erase(k)
			theme.invalidate(world.drop_points(k.x, k.y))
	var want: PackedStringArray = []
	for j in range(t.y - 2, t.y + 3):
		for i in range(t.x - 2, t.x + 3):
			if not world_tiles.has(Vector2i(i, j)):
				want.append("%d,%d" % [i, j])
	if want.is_empty():
		return
	world_busy = true
	var text := await _fetch("http://%s/play/world/tiles?v=%s&c=%s" % [args["web"], world_ver, ";".join(want)])
	world_busy = false
	var d = JSON.parse_string(text) if text != "" else null
	if not d is Dictionary:
		world_poll = 2.0 # unreachable, or the world changed under us: try again later
		return
	var tiles: Dictionary = d.get("tiles", {})
	for k in tiles:
		var p := String(k).split(",")
		var at := Vector2i(int(p[0]), int(p[1]))
		world_tiles[at] = true
		theme.invalidate(world.set_points(at.x, at.y, tiles[k]))

## The boxes in a /play/tiles reply (proto::tiles): [id, x0, y0, x1, y1, class] each.
static func _tile_boxes(b: PackedByteArray) -> Array:
	var out := []
	if b.size() < 11 or b[0] != 1:
		return out
	var p := 11
	for t in b.decode_u16(9):
		if p + 8 > b.size():
			return out
		var k := b.decode_u32(p + 4)
		p += 8
		for i in k:
			if p + 21 > b.size():
				return out
			out.append([b.decode_u32(p), b.decode_float(p + 4), b.decode_float(p + 8), b.decode_float(p + 12), b.decode_float(p + 16), b[p + 20]])
			p += 21
	return out

# ---------------------------------------------------------------- building

## Fetch what changed in the built blocks: every second, and right after you build or break.
func _poll_blocks(delta: float) -> void:
	if build.is_empty() or blocks_busy:
		return
	blocks_poll -= delta
	if blocks_poll > 0.0:
		return
	blocks_poll = 1.0
	blocks_busy = true
	var text := await _fetch("http://%s/play/blocks?since=%d" % [args["web"], blocks_ver])
	blocks_busy = false
	var d = JSON.parse_string(text) if text != "" else null
	if not d is Dictionary:
		return
	var changed := {}
	block_owners = d.get("owners", block_owners)
	if d.get("full", false):
		for k in blocks:
			changed[k] = true
			net.set_column(k & 0x7fff, k >> 15, 0, 0)
		blocks.clear()
	for c in d.get("cols", []):
		var cx := int(c[0])
		var cy := int(c[1])
		var k := cy << 15 | cx
		var kinds := String(c[3])
		_crumble_gone(cx, cy, blocks.get(k, {}), int(c[2]), kinds)
		changed[k] = true
		if kinds == "":
			blocks.erase(k)
		else:
			blocks[k] = {"b": int(c[2]), "k": kinds, "o": c[4]}
		net.set_column(cx, cy, int(c[2]), _mask(kinds)) # prediction climbs, falls and walks under as the server does
	blocks_ver = int(d.get("ver", blocks_ver))
	_blocks_ready = true
	if not changed.is_empty():
		theme.set_blocks(blocks, changed.keys())

## Blocks that were in a column and are not now crumble, if they are near enough to see (anyone's, not only ours;
## ours already crumbled when we broke them).
func _crumble_gone(cx: int, cy: int, was: Dictionary, base: int, kinds: String) -> void:
	if was.is_empty():
		return
	var bs := float(build["block"])
	if me_pos.distance_to(Vector2((cx + 0.5) * bs, (cy + 0.5) * bs)) > 80.0:
		return
	var old := String(was["k"])
	var b0 := int(was["b"])
	for i in old.length():
		if old[i] == ".":
			continue
		var z := b0 + i
		var j := z - base
		if kinds != "" and j >= 0 and j < kinds.length() and kinds[j] != ".":
			continue
		var cell := Vector3i(cx, cy, z)
		if _break_sent.has(cell):
			_break_sent.erase(cell)
			continue
		_crumble(cell, theme.block_kind(old[i]))

## Holding the right button on a block (build mode): cracks spread, then it breaks and crumbles. The cracks start over
## on whatever block the pointer moves to, so sweeping clears a row.
func _mine(delta: float) -> void:
	# Right button, X, or Shift with the left button (easier on a trackpad).
	var pressed := Input.is_mouse_button_pressed(MOUSE_BUTTON_RIGHT) or Input.is_key_pressed(KEY_X) \
		or (Input.is_key_pressed(KEY_SHIFT) and Input.is_mouse_button_pressed(MOUSE_BUTTON_LEFT)) or args.has("breaking")
	var held: bool = building and pressed and not hud.typing()
	var cell := aim_break
	for c in _break_sent.keys():
		if t_total - float(_break_sent[c]) > 2.0:
			_break_sent.erase(c) # the server never took it away (or we missed the change): let it be tried again
	if held:
		ghost.visible = false # while breaking, the cracks are what matters, not where a block would go
	# Say once why the block under the pointer will not break.
	if held and cell.x >= 0 and cell != _break_why and not _break_sent.has(cell):
		if not _in_reach(cell):
			_notice("too far to reach that block: get closer")
			_break_why = cell
		elif not _breakable(cell):
			_notice("that block is someone else's")
			_break_why = cell
	if not held:
		_break_why = Vector3i(-1, -1, 0)
	if not held or cell.x < 0 or _break_sent.has(cell) or not _in_reach(cell) or not _breakable(cell):
		_break_t = 0.0
		_break_cell = Vector3i(-1, -1, 0)
		cracks.visible = false
		return
	if cell != _break_cell:
		_break_cell = cell
		_break_t = 0.0
	_break_t += delta
	var f := clampf(_break_t / BREAK_SECS, 0.0, 1.0)
	var bs := float(build["block"])
	var at := Vector3((cell.x + 0.5) * bs, (cell.z + 0.5) * bs, (cell.y + 0.5) * bs)
	# A small shudder that grows as it gives.
	cracks.position = at + Vector3(randf_range(-1, 1), randf_range(-1, 1), randf_range(-1, 1)) * 0.03 * f
	cracks.visible = true
	break_mark.visible = false # the cracks show which block it is
	(cracks.material_override as StandardMaterial3D).albedo_texture = _crack_tex[mini(int(f * 4.0), 3)]
	if f >= 1.0:
		_build(false)
		_break_sent[cell] = t_total
		_crumble(cell, _kind_at(cell))
		cracks.visible = false
		_break_t = 0.0

## The material of the block in a cell, by index (0 if there is none).
func _kind_at(cell: Vector3i) -> int:
	var c: Dictionary = blocks.get(cell.y << 15 | cell.x, {})
	if c.is_empty():
		return 0
	var i := cell.z - int(c["b"])
	var k := String(c["k"])
	return theme.block_kind(k[i]) if i >= 0 and i < k.length() and k[i] != "." else 0

## A block breaks apart: pieces of it in its own material burst out, tumble, land and fade, with a puff of dust.
func _crumble(cell: Vector3i, kind: int) -> void:
	var edge: float = theme.block_size() if theme else 2.0
	sound.play("break", SoundScript.at_distance(Vector2((cell.x + 0.5) * edge, (cell.y + 0.5) * edge).distance_to(me_pos), 50.0) - 2.0)
	crumbled += 1
	var bs := float(build["block"])
	var at := Vector3((cell.x + 0.5) * bs, (cell.z + 0.5) * bs, (cell.y + 0.5) * bs)
	var base_mat = theme.block_material(kind)
	var floor_y := _ground_under(at)
	for i in 10:
		var piece := MeshInstance3D.new()
		var m := BoxMesh.new()
		var sz := bs * randf_range(0.16, 0.3)
		m.size = Vector3(sz, sz * randf_range(0.6, 1.0), sz * randf_range(0.7, 1.0))
		piece.mesh = m
		if base_mat is StandardMaterial3D:
			var mat := (base_mat as StandardMaterial3D).duplicate() as StandardMaterial3D
			mat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
			piece.material_override = mat
		else:
			piece.material_override = base_mat
		piece.position = at + Vector3(randf_range(-0.35, 0.35), randf_range(-0.35, 0.35), randf_range(-0.35, 0.35)) * bs
		piece.rotation = Vector3(randf() * TAU, randf() * TAU, randf() * TAU)
		add_child(piece)
		var out := (piece.position - at).normalized()
		_debris.append({"node": piece, "vel": out * randf_range(1.5, 4.0) + Vector3.UP * randf_range(2.0, 5.0),
			"spin": Vector3(randf_range(-9, 9), randf_range(-9, 9), randf_range(-9, 9)), "life": randf_range(0.9, 1.4), "age": 0.0,
			"floor": floor_y + sz * 0.5, "size": sz})
	var dust := CPUParticles3D.new()
	dust.one_shot = true
	dust.explosiveness = 1.0
	dust.amount = 18
	dust.lifetime = 0.7
	dust.position = at
	dust.emission_shape = CPUParticles3D.EMISSION_SHAPE_BOX
	dust.emission_box_extents = Vector3.ONE * bs * 0.45
	dust.direction = Vector3.UP
	dust.spread = 80.0
	dust.initial_velocity_min = 0.5
	dust.initial_velocity_max = 1.6
	dust.gravity = Vector3(0, -1.0, 0)
	dust.scale_amount_min = 0.7
	dust.scale_amount_max = 1.6
	# Soft round puffs facing the camera, not balls.
	var dm := QuadMesh.new()
	dm.size = Vector2.ONE * bs * 0.5
	dust.mesh = dm
	var dmat := StandardMaterial3D.new()
	dmat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	dmat.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	dmat.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
	dmat.vertex_color_use_as_albedo = true
	dmat.albedo_texture = _puff_tex()
	dust.material_override = dmat
	var ramp := Gradient.new()
	ramp.set_color(0, Color(0.8, 0.76, 0.68, 0.45))
	ramp.set_color(1, Color(0.8, 0.76, 0.68, 0.0))
	dust.color_ramp = ramp
	add_child(dust)
	dust.emitting = true
	get_tree().create_timer(1.2).timeout.connect(dust.queue_free)

var _puff: Texture2D
## A soft round spot, white in the middle and clear at the edge, for dust.
func _puff_tex() -> Texture2D:
	if _puff == null:
		var g := Gradient.new()
		g.set_color(0, Color(1, 1, 1, 1))
		g.set_color(1, Color(1, 1, 1, 0))
		var t := GradientTexture2D.new()
		t.gradient = g
		t.fill = GradientTexture2D.FILL_RADIAL
		t.fill_from = Vector2(0.5, 0.5)
		t.fill_to = Vector2(1.0, 0.5)
		t.width = 64
		t.height = 64
		_puff = t
	return _puff

## Move the pieces of broken blocks: gravity, a bounce or two on the ground, then they shrink away.
func _fly_debris(delta: float) -> void:
	for i in range(_debris.size() - 1, -1, -1):
		var d: Dictionary = _debris[i]
		var n: MeshInstance3D = d["node"]
		d["age"] += delta
		var t: float = d["age"] / d["life"]
		if t >= 1.0:
			n.queue_free()
			_debris.remove_at(i)
			continue
		var v: Vector3 = d["vel"]
		v.y -= 18.0 * delta
		n.position += v * delta
		if n.position.y < d["floor"]:
			n.position.y = d["floor"]
			v = Vector3(v.x * 0.5, -v.y * 0.3, v.z * 0.5)
			d["spin"] = d["spin"] * 0.5
		d["vel"] = v
		n.rotation += d["spin"] * delta
		if t > 0.6:
			var k := 1.0 - (t - 0.6) / 0.4
			n.scale = Vector3.ONE * maxf(k, 0.01)
			if n.material_override is StandardMaterial3D:
				(n.material_override as StandardMaterial3D).albedo_color.a = k

## Where a falling piece lands: the top of the ground or of the blocks under it.
func _ground_under(at: Vector3) -> float:
	var bs := float(build["block"])
	var cx := int(floor(at.x / bs))
	var cy := int(floor(at.z / bs))
	var y := _height(at.x, at.z)
	var c: Dictionary = blocks.get(cy << 15 | cx, {})
	if not c.is_empty():
		var z := int(floor(at.y / bs))
		var k := String(c["k"])
		for i in range(z - int(c["b"]) - 1, -1, -1):
			if i < k.length() and k[i] != ".":
				return maxf(y, (int(c["b"]) + i + 1) * bs)
	return y

## Crack pictures for a block giving way: lines that branch out from near the middle, more at each stage.
func _make_cracks() -> Array[Texture2D]:
	var out: Array[Texture2D] = []
	var img := Image.create(64, 64, false, Image.FORMAT_RGBA8)
	img.fill(Color(0, 0, 0, 0))
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	for stage in 4:
		for line in 3 + stage * 2:
			var p := Vector2(32 + rng.randf_range(-10, 10), 32 + rng.randf_range(-10, 10))
			var a := rng.randf() * TAU
			for step in 10 + stage * 4:
				a += rng.randf_range(-0.6, 0.6)
				p += Vector2(cos(a), sin(a)) * 2.0
				if p.x < 0 or p.y < 0 or p.x >= 64 or p.y >= 64:
					break
				# Two pixels wide, so they read from a few blocks away.
				for o in [Vector2i(0, 0), Vector2i(1, 0), Vector2i(0, 1)]:
					var q: Vector2i = Vector2i(p) + o
					if q.x < 64 and q.y < 64:
						img.set_pixelv(q, Color(0.03, 0.025, 0.02, 0.95))
		out.append(ImageTexture.create_from_image(img.duplicate()))
	return out

## A column's layers as a bit mask, bit k for a block in its base layer + k.
func _mask(kinds: String) -> int:
	var m := 0
	for k in mini(kinds.length(), 16):
		if kinds[k] != ".":
			m |= 1 << k
	return m

func _solid(cx: int, cy: int, z: int) -> bool:
	var c: Dictionary = blocks.get(cy << 15 | cx, {})
	if c.is_empty():
		return false
	var k: int = z - int(c["b"])
	var kinds: String = c["k"]
	return k >= 0 and k < kinds.length() and kinds[k] != "."

## The layer a column starts in: its own if it is built, else where its ground is.
func _col_base(cx: int, cy: int) -> int:
	var c: Dictionary = blocks.get(cy << 15 | cx, {})
	return int(c["b"]) if not c.is_empty() else theme.block_layer(cx, cy)

## Who built a block: their tag, or "" for nobody.
func _owner_of(cx: int, cy: int, z: int) -> String:
	var c: Dictionary = blocks.get(cy << 15 | cx, {})
	if c.is_empty():
		return ""
	var k: int = z - int(c["b"])
	var o: Array = c["o"]
	var n := int(o[k]) if k >= 0 and k < o.size() else 0
	return String(block_owners[n - 1]) if n > 0 and n <= block_owners.size() else ""

## Whether a block is ours to break: ours, or nobody's.
func _breakable(cell: Vector3i) -> bool:
	var who := _owner_of(cell.x, cell.y, cell.z)
	return who == "" or who == my_tag

## In reach of a cell's layer, as the server measures it: sideways from the cell's centre, and up or down from
## our feet.
func _in_reach(cell: Vector3i) -> bool:
	var bs := float(build["block"])
	var reach := float(build["reach"])
	var centre := Vector2((cell.x + 0.5) * bs, (cell.y + 0.5) * bs)
	var feet := my_layer
	if feet == GROUND:
		feet = _col_base(int(floor(me_pos.x / bs)), int(floor(me_pos.y / bs)))
	return me_pos.distance_to(centre) <= reach and absf((cell.z - feet + 0.5) * bs) <= reach

## Whether a block in this cell would land on us: the server's own test (sim.rs cell_free), within half a block
## plus our radius and in our feet's layer. Such a build is refused, so it gets no ghost (in first person the
## ghost would be all round the camera).
func _on_me(cell: Vector3i) -> bool:
	var bs := float(build["block"])
	var r := float(info.get("entity_radius", 0.5))
	var feet := my_layer
	if feet == GROUND:
		feet = _col_base(int(floor(me_pos.x / bs)), int(floor(me_pos.y / bs)))
	return absf(me_pos.x - (cell.x + 0.5) * bs) < bs / 2.0 + r and absf(me_pos.y - (cell.y + 0.5) * bs) < bs / 2.0 + r and cell.z == feet

## In build mode: follow the mouse ray through the block grid. The first block it meets is the one a right click
## breaks, and the empty cell just before it is where a left click builds; where the ray meets the ground first,
## a block goes in that cell's lowest layer. Green in reach, red out of it (or, for a break, not yours).
func _aim() -> void:
	aim_place = Vector3i(-1, -1, 0)
	aim_break = Vector3i(-1, -1, 0)
	ghost.visible = false
	break_mark.visible = false
	if not building or build.is_empty() or world_map.visible:
		return
	var bs := float(build["block"])
	var mp := _pointer()
	var from := cam.project_ray_origin(mp)
	var dir := cam.project_ray_normal(mp)
	var prev := Vector3i(-1, -1, 0)
	var t := 0.0
	while t < 300.0:
		var p := from + dir * t
		var cell := Vector3i(int(floor(p.x / bs)), int(floor(p.z / bs)), int(floor(p.y / bs)))
		if cell.x >= 0 and cell.y >= 0:
			if _solid(cell.x, cell.y, cell.z):
				aim_break = cell
				aim_place = prev
				break
			if p.y <= _height(p.x, p.z):
				aim_place = Vector3i(cell.x, cell.y, _col_base(cell.x, cell.y))
				break
			prev = cell
		t += 0.1
	if aim_place.x >= 0 and _on_me(aim_place):
		aim_place = Vector3i(-1, -1, 0)
	if aim_place.x >= 0:
		ghost.visible = true
		ghost.position = Vector3((aim_place.x + 0.5) * bs, (aim_place.z + 0.5) * bs, (aim_place.y + 0.5) * bs)
		var near := _in_reach(aim_place)
		(ghost.material_override as StandardMaterial3D).albedo_color = Color(0.45, 1.0, 0.55, 0.4) if near else Color(1.0, 0.35, 0.3, 0.4)
	if aim_break.x >= 0:
		break_mark.visible = true
		break_mark.position = Vector3((aim_break.x + 0.5) * bs, (aim_break.z + 0.5) * bs, (aim_break.y + 0.5) * bs)
		var ok := _in_reach(aim_break) and _breakable(aim_break)
		(break_mark.material_override as StandardMaterial3D).albedo_color = Color(1, 1, 1, 0.18) if ok else Color(1.0, 0.35, 0.3, 0.25)

## Build where the ghost is, or break the block under the mouse.
func _build(place: bool) -> void:
	var cell := aim_place if place else aim_break
	if cell.x < 0:
		return
	if not _in_reach(cell):
		_notice("too far to build there" if place else "too far to reach that block")
		return
	if not place and not _breakable(cell):
		_notice("that block is someone else's")
		return
	net.act_arg(int(build["act_build"] if place else build["act_break"]), cell.x | cell.y << 15, _build_arg(cell.z, build_kind))
	if place:
		sound.play("place", -4.0)
	blocks_poll = minf(blocks_poll, 0.12)

## The input's argument for a build or break: the layer in the low byte, the material in the high one.
func _build_arg(layer: int, kind: int) -> int:
	return (layer & 0xff) | kind << 8

## Whether a straight walk from a to b (a body wide) meets no solid footprint the world data holds.
func _path_clear(a: Vector2, b: Vector2) -> bool:
	var r := float(info.get("entity_radius", 0.5)) + 0.3
	var lo := Vector2(minf(a.x, b.x) - r, minf(a.y, b.y) - r)
	var hi := Vector2(maxf(a.x, b.x) + r, maxf(a.y, b.y) + r)
	for cy in range(int(floor(lo.y / world.CELL)), int(floor(hi.y / world.CELL)) + 1):
		for cx in range(int(floor(lo.x / world.CELL)), int(floor(hi.x / world.CELL)) + 1):
			for o in world.in_cell(cx, cy):
				if not o.get("solid", false) or not o.has("b"):
					continue
				var f: Array = o["b"]
				# Sample the walk every half unit against the footprint grown by the body.
				for i in 21:
					var p := a.lerp(b, i / 20.0)
					if p.x > float(f[0]) - r and p.x < float(f[2]) + r and p.y > float(f[1]) - r and p.y < float(f[3]) + r:
						return false
	return true

## Move a character, face where it is going, and pick its animation from what changed since last frame.
## The protocol has no death or attack events, so they are read off the snapshots: health dropping is a hit,
## and a nearly dead character that jumps across the map was killed and respawned (the server does both in one tick).
func _animate(id: int, node: Node3D, np: Vector3, hp: float, mine: bool, delta: float, self_id: int, air := 0.0) -> void:
	var t: Dictionary = track.get(id, {})
	if t.is_empty():
		t = {"hp": hp, "pos": np, "vel": Vector3.ZERO, "face_until": 0.0, "since": t_total, "hit_at": -100.0, "air": 0.0}
		track[id] = t
		node.position = np
	var moved: Vector3 = np - t["pos"]
	moved.y = 0.0
	var animated := node.has_method("set_speed")
	if moved.length() > 12.0:
		# Teleport: a respawn if it was low on health and had just been hit. Joining also teleports (the server
		# restores a returning character's saved spot), which is why both conditions matter.
		if t["hp"] < 0.35 and t_total - t["hit_at"] < 3.0 and t_total - t["since"] > 1.0 and animated:
			var body := _entity_node(id, mine)
			add_child(body)
			body.position = node.position
			body.rotation = node.rotation
			_corpse(body)
			if mine:
				hud.say("[color=#ff8a70]You were slain. You wake at a safe spot.[/color]")
				node.set_dead(true)
				node.set_dead(false) # plays the stand-up
		t["vel"] = Vector3.ZERO
	elif delta > 0.0:
		t["vel"] = (t["vel"] as Vector3).lerp(moved / delta, minf(1.0, delta * 10.0))
	t["pos"] = np
	# The jump arc: `air` is the share of the jump still to come, so progress is 1 - air.
	if air > 0.0:
		var k := 1.0 - air
		np.y += 4.0 * JUMP_HEIGHT * entity_scale * k * (1.0 - k)
		if t["air"] <= 0.0 and node.has_method("jump"):
			node.jump(JUMP_SECS)
	t["air"] = air
	# Stepping onto a block or dropping off one is a jump in height: ease it, and follow slopes as they are.
	var y: float = t.get("y", np.y)
	if moved.length() <= 12.0 and absf(np.y - y) > 0.5:
		np.y = lerpf(y, np.y, minf(1.0, delta * 14.0))
	t["y"] = np.y
	node.position = np
	var vel: Vector3 = t["vel"]
	if t_total > t["face_until"] and vel.length() > 0.8:
		node.rotation.y = lerp_angle(node.rotation.y, atan2(vel.x, vel.z), minf(1.0, delta * 12.0))
	if hp < t["hp"] - 0.001 and moved.length() <= 12.0:
		t["hit_at"] = t_total
		var near := Vector2(np.x, np.z).distance_to(me_pos)
		if mine:
			sound.play("hurt", -2.0)
			sound.play("hit", -3.0)
			_fight_at = t_total
		elif near < 40.0:
			sound.play("hit", SoundScript.at_distance(near) - 3.0)
			if near < action_range + 4.0:
				_fight_at = t_total
		_damage_number(np, (t["hp"] - hp) * health_max, mine)
		if animated:
			node.hurt()
		if mine:
			_blame(np, self_id)
	t["hp"] = hp
	if animated:
		node.set_speed(vel.length())
		node.set_dead(hp <= 0.0)
	_set_health_bar(node, hp, hp < 0.999 or id == target_id)

## Something hit us. Snapshots do not say who, so the nearest other character within reach swings.
func _blame(at: Vector3, self_id: int) -> void:
	var best: Node3D = null
	var best_d := action_range + 1.5
	for id in ents:
		if id == self_id:
			continue
		var d: float = (ents[id] as Node3D).position.distance_to(at)
		if d < best_d:
			best_d = d
			best = ents[id]
	if best and best.has_method("attack"):
		_face(best, at)
		best.attack()

func _face(node: Node3D, at: Vector3) -> void:
	var d := at - node.position
	if d.length() > 0.01:
		node.rotation.y = atan2(d.x, d.z)
	for id in ents:
		if ents[id] == node and track.has(id):
			track[id]["face_until"] = t_total + 0.6

func _corpse(n: Node3D) -> void:
	n.set_dead(true)
	for part in ["bar", "bar_bg", "tag"]:
		if n.has_node(part):
			n.get_node(part).visible = false
	corpses.append([n, 6.0])

func _damage_number(at: Vector3, amount: float, mine: bool) -> void:
	if amount >= 0.5:
		_float_text(at, "-%d" % int(round(amount)), Color("ff5a4a") if mine else Color("ffd27a"))

func _float_text(at: Vector3, text: String, color: Color) -> void:
	var l := Label3D.new()
	l.text = text
	l.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	l.pixel_size = 0.012
	l.font_size = 40
	l.outline_size = 10
	l.no_depth_test = true
	l.modulate = color
	l.position = at + Vector3(randf_range(-0.4, 0.4), 3.3 * entity_scale, 0)
	add_child(l)
	var tw := create_tween()
	tw.tween_property(l, "position:y", l.position.y + 1.8, 0.9)
	tw.parallel().tween_property(l, "modulate:a", 0.0, 0.9).set_delay(0.3)
	tw.tween_callback(l.queue_free)

## Just south of the nearest building, so the camera looks at it (for --walk near).
func _beside_nearest_building() -> String:
	var best := "%f,%f" % [me_pos.x, me_pos.y]
	var best_d := 1e12
	for o in world.objects:
		if not o.get("solid", false) or o.get("scenery", false):
			continue
		var b: Array = o["b"]
		var c := Vector2((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
		if c.distance_to(me_pos) < best_d:
			best_d = c.distance_to(me_pos)
			best = "%f,%f" % [c.x + 4.0, b[3] + 11.0]
	return best


## The server's weather (set in the console's God tab or by an event) is in /play/info; look every few seconds,
## like the play page, and hand it to the theme to draw.
func _weather(delta: float) -> void:
	weather_poll -= delta
	if weather_poll > 0.0:
		return
	weather_poll = 5.0
	var text := await _fetch("http://%s/play/info" % args["web"])
	var fresh = JSON.parse_string(text) if text != "" else null
	if fresh is Dictionary and str(fresh.get("content_hash", "")) == str(info.get("content_hash", "")):
		var w = fresh.get("weather")
		_apply_weather(w if w is Dictionary else {})
		# The schedule can be edited live (the console's God tab): take it as it is now.
		if fresh.get("daylight") is Dictionary:
			theme.set_daylight(fresh["daylight"])
		theme.set_zones(fresh.get("weather_zones", []) if fresh.get("weather_zones") is Array else [])
		locations = fresh.get("locations", []) if fresh.get("locations") is Array else []
		info["pvp"] = fresh.get("pvp", false)
		if fresh.get("quests") is Array and fresh["quests"] != info.get("quests"):
			info["quests"] = fresh["quests"]
			questui.set_quests(fresh["quests"])

func _apply_weather(w: Dictionary) -> void:
	if w == weather:
		return
	weather = w
	if w.is_empty():
		hud.say("[color=#9fe8e0]The sky clears.[/color]")
		hud.announce("The sky clears")
	else:
		hud.say("[color=#9fe8e0]Weather: %s.[/color]" % str(w.get("name", "")))
		hud.announce(str(w.get("name", "")).capitalize(), Color(str(w.get("color", "#9fe8e0"))).lightened(0.3))
	_show_weather()

## Which titled place we stand in: the smallest one round us, so a village inside a forest is the village. We are
## still in a place until LEAVE_MARGIN past its edge, so walking along the edge does not flicker the title.
const LEAVE_MARGIN := 3.0
## What to hear here, a couple of times a second: how much sea is round us, how near a river is, the trees, the houses,
## the nearest fire, how high we stand, the weather and the hour; then a place's own sound and music, if we are in one.
func _listen(delta: float) -> void:
	_listen_in -= delta
	if _listen_in > 0.0 or sound == null or theme == null:
		return
	_listen_in = 0.4
	var p := me_pos
	var dark: float = theme.darkness
	var sea := 0.0
	var shore := 0.0
	var trees := 0
	var houses := 0
	var fire_d := INF
	if world:
		if world.has_sea:
			var wet := 0.0
			var all := 0.0
			for r in [18.0, 40.0, 75.0]:
				for k in 12:
					var a := k * TAU / 12.0
					var wgt: float = 18.0 / r
					all += wgt
					if theme.height(p.x + cos(a) * r, p.y + sin(a) * r) < world.sea_level:
						wet += wgt
			sea = clampf(wet / all * 2.4, 0.0, 1.0)
			shore = 1.0 - smoothstep(3.0, 32.0, _shore_distance(p)) # surf breaking, close to the edge
		var cx := floori(p.x / world.CELL)
		var cy := floori(p.y / world.CELL)
		for i in range(cx - 1, cx + 2):
			for j in range(cy - 1, cy + 2):
				for o in world.in_cell(i, j):
					var t := String(o.get("t", ""))
					var at: Vector2 = WorldData.centre(o)
					var d := at.distance_to(p)
					if t == "tree" and d < 45.0:
						trees += 1
					elif t == "house" and d < 70.0:
						houses += 1
					elif t == "square" or t == "keep" or t == "castle" or t == "tower":
						fire_d = minf(fire_d, d - (0.0 if t == "square" else 12.0))
	var forest := clampf(trees / 16.0, 0.0, 1.0)
	var kind := String(theme.weather_drawn())
	var w := {
		"sea": sea * (1.0 - shore * 0.5),
		"shore": shore,
		"river": (1.0 - smoothstep(6.0, 50.0, _river_distance(p))) * 0.9,
		"forest": forest * (1.0 - dark) * (1.0 - sea * 0.5),
		"night": dark * clampf(0.35 + forest, 0.0, 1.0) * (1.0 - sea * 0.6),
		"rain": 1.0 if kind == "rain" or kind == "storm" else 0.0,
		"wind": clampf(0.2 + clampf(theme.height(p.x, p.y) / 25.0, 0.0, 1.0) * 0.6 + (0.45 if kind in ["storm", "snow", "dust"] else 0.0) + sea * 0.2, 0.0, 1.0),
		"fire": 1.0 - smoothstep(3.0, 20.0, fire_d),
		"town": clampf(houses / 8.0, 0.0, 1.0) * (1.0 - dark * 0.6),
	}
	# A place with a sound of its own brings it up and takes the land's own sounds down by half.
	var zone := _sound_zone(p)
	var own := String(zone.get("sound", ""))
	if own != "":
		for k in w:
			w[k] = float(w[k]) * 0.5
		w[own] = 1.0
	sound.want = w
	# Music: a fight first, then the place's own, then a town, then the night or the day. It changes only once the
	# moment has held for a few seconds, so walking along a town's edge does not flip it back and forth.
	# In a fight: a bot is locked on us (it holds through a chase with no blows) or a player is fighting us, or a
	# blow passed lately.
	var fighting: bool = t_total - _fight_at < 8.0 or (net != null and not net.encounter().is_empty())
	var m := String(zone.get("music", ""))
	# Now and then a smith, a dog, a cow or a rooster (sound.gd INCIDENTAL): a town has houses all round, a farm a few.
	sound.daylight = 1.0 - dark
	sound.incidental = "" if fighting or own != "" else ("town" if houses >= 6 else ("farm" if houses >= 1 else ""))
	if fighting:
		m = "battle"
	elif m == "":
		m = "town" if houses >= 6 else ("night" if dark > 0.55 else "explore")
	if m != _music_pick:
		_music_pick = m
		_music_since = t_total
	# A change of mood waits a few seconds (passing a house is not a town); the first pick does not.
	if fighting or t_total - _music_since > 3.0 or (sound.music_for == "" and not _title_on):
		sound.music_for = "" if _title_on else _music_pick

## The smallest place round a point that has a sound or music of its own (/play/info locations), or {}.
func _sound_zone(p: Vector2) -> Dictionary:
	var best := {}
	var best_r := INF
	for l in locations:
		if String(l.get("sound", "")) == "" and String(l.get("music", "")) == "":
			continue
		var r := float(l.get("r", 0.0))
		if p.distance_to(Vector2(float(l.get("x", 0.0)), float(l.get("y", 0.0)))) <= r and r < best_r:
			best = l
			best_r = r
	return best

## Distance to the nearest river's middle (INF with none near).
## How far the water's edge is (land to sea, or sea to land when we stand in it), looking out to SHORE_FAR; INF when
## there is no edge that near, so far out at sea or well inland there is no surf.
const SHORE_FAR := 36.0
func _shore_distance(p: Vector2) -> float:
	var wet: bool = theme.height(p.x, p.y) < world.sea_level
	var r := 2.0
	while r <= SHORE_FAR:
		for k in 16:
			var a := k * TAU / 16.0
			if (theme.height(p.x + cos(a) * r, p.y + sin(a) * r) < world.sea_level) != wet:
				return r
		r += 3.0
	return INF

func _river_distance(p: Vector2) -> float:
	var best := INF
	if world == null:
		return best
	for r in world.rivers:
		var pts: Array = r["pts"]
		for k in range(0, pts.size() - 1):
			var a := Vector2(pts[k][0], pts[k][1])
			if absf(a.x - p.x) > 120.0 or absf(a.y - p.y) > 120.0:
				continue
			var b := Vector2(pts[k + 1][0], pts[k + 1][1])
			best = minf(best, Geometry2D.get_closest_point_to_segment(p, a, b).distance_to(p))
	return best

## What is underfoot: water (a river, the sea, a lake), a road, the cobbles of a town, or grass.
func _underfoot(p: Vector2) -> String:
	if world == null:
		return "grass"
	if world.has_sea and theme.height(p.x, p.y) < world.sea_level + 0.05:
		return "water"
	if _river_distance(p) < 5.0:
		return "water"
	var cx := floori(p.x / world.CELL)
	var cy := floori(p.y / world.CELL)
	for s in world.road_segments(cx, cy):
		if Geometry2D.get_closest_point_to_segment(p, Vector2(s[0], s[1]), Vector2(s[2], s[3])).distance_to(p) < float(s[4]):
			return "stone" if float(s[4]) >= 3.4 else "road"
	for o in world.in_cell(cx, cy):
		if (o.get("t", "") == "square" or o.get("t", "") == "city") and o.has("b"):
			var b: Array = o["b"]
			if p.x > float(b[0]) and p.x < float(b[2]) and p.y > float(b[1]) and p.y < float(b[3]):
				return "stone"
	return "grass"

## A footstep every couple of units walked, on whatever is underfoot; a splash stepping into water.
func _footsteps() -> void:
	if _step_from == Vector2.INF:
		_step_from = me_pos
	var d := me_pos.distance_to(_step_from)
	_step_from = me_pos
	if d > 6.0: # a teleport, not a step
		return
	_step_dist += d
	if _step_dist < 2.3:
		return
	_step_dist = 0.0
	var s := _underfoot(me_pos)
	if s == "water" and _surface != "water" and _surface != "":
		sound.play("splash", -6.0)
	_surface = s
	sound.play("step_" + s, -18.0 if s == "grass" else -16.0, 0.1)

## Something came into the bag or went out of it.
func _hear_bag(bag: Array) -> void:
	var n := 0
	for s in bag:
		n += int(s.get("count", 1))
	if _bag_count >= 0 and n > _bag_count:
		sound.play("pickup", -4.0)
	elif _bag_count >= 0 and n < _bag_count:
		sound.play("drop", -5.0)
	_bag_count = n

func _where() -> void:
	var best := ""
	var best_r := INF
	for l in locations:
		var r := float(l.get("r", 0.0)) + (LEAVE_MARGIN if String(l.get("title", "")) == _here else 0.0)
		if String(l.get("title", "")) != "" and me_pos.distance_to(Vector2(float(l.get("x", 0.0)), float(l.get("y", 0.0)))) <= r and r < best_r:
			best = String(l.get("title", ""))
			best_r = r
	if best == _here:
		return
	if best != "":
		hud.show_location(best)
		hud.say("[color=#ffd27a]%s[/color]" % best)
		sound.play("chime", -9.0, 0.0)
	else:
		hud.show_location(_here, true)
	_here = best

## What the theme draws: inside a weather zone its weather, elsewhere the weather everywhere (as the server applies
## their effects). Called every frame; only a change reaches the theme.
func _show_weather() -> void:
	var z: Dictionary = theme.zone_at(me_pos)
	var name := String(z.get("name", ""))
	if name != _in_zone:
		if name != "":
			hud.say("[color=%s]You walk into the %s.[/color]" % [str(z.get("color", "#9fe8e0")), name])
			hud.announce(name.capitalize(), Color(str(z.get("color", "#9fe8e0"))).lightened(0.3))
		elif _in_zone != "":
			hud.say("[color=#9fe8e0]You leave the %s.[/color]" % _in_zone)
		_in_zone = name
	var w: Dictionary = weather if z.is_empty() else {"name": z["weather"], "color": z["color"], "intensity": z["intensity"]}
	if w != _shown_weather:
		_shown_weather = w
		theme.apply_weather(w)
		# A change eases in over ten seconds, but the weather we find on joining is simply there.
		if t_total < 3.0:
			theme.snap_weather()

func _height(x: float, y: float) -> float:
	return theme.height(x, y) if theme else 0.0

## Where a character at `p` with its feet in `layer` stands: the ground, or the top of the block it is on (layer
## z is z blocks up from height 0, the same everywhere). The server sends the layer.
func _stand_height(p: Vector2, layer: int) -> float:
	var ground := _height(p.x, p.y)
	if layer == GROUND or build.is_empty():
		return ground
	return maxf(ground, layer * float(build["block"]))

## A small picture of a block material for the build bar: its texture, or a square of its colour.
func _swatch(kind: int) -> Texture2D:
	var m = theme.block_material(kind)
	if m is StandardMaterial3D and m.albedo_texture:
		# A tinted texture (one stone three ways) shows its tint, or the swatches would look alike.
		var src: Image = m.albedo_texture.get_image() if m.albedo_color != Color.WHITE else null
		if src == null:
			return m.albedo_texture
		var img := src.duplicate() as Image
		if img.is_compressed():
			img.decompress()
		img.convert(Image.FORMAT_RGBA8)
		img.resize(32, 32)
		for y in 32:
			for x in 32:
				img.set_pixel(x, y, img.get_pixel(x, y) * m.albedo_color)
		return ImageTexture.create_from_image(img)
	var img := Image.create(8, 8, false, Image.FORMAT_RGBA8)
	img.fill(m.albedo_color if m is StandardMaterial3D else Color.GRAY)
	return ImageTexture.create_from_image(img)

## A character: the theme's body, with the name and health bar on top.
func _entity_node(id: int, mine: bool) -> Node3D:
	var n: Node3D = theme.entity_node(id, mine, {"class": args["class"]} if args.has("class") else {})
	_nameplate(n, id, mine)
	return n

## The camera and the target ring are the client's own; everything else in the scene is the theme's.
func _build_scene() -> void:
	cam.current = true
	cam.fov = 55
	cam.far = 900
	add_child(cam)
	var ring := TorusMesh.new()
	ring.inner_radius = 0.95
	ring.outer_radius = 1.12
	ring.rings = 32
	target_ring.mesh = ring
	target_ring.scale = Vector3(1, 0.05, 1)
	var rmat := StandardMaterial3D.new()
	rmat.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	rmat.albedo_color = Color("ffd27a")
	target_ring.material_override = rmat
	target_ring.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	target_ring.visible = false
	add_child(target_ring)

## What lies on the ground near you, drawn by the theme.
func _sync_ground() -> void:
	var seen := {}
	for g in net.ground():
		var id: int = g["id"]
		seen[id] = true
		if not gems.has(id):
			# Rare things (an item that stacks to 3 or fewer) are marked so a theme can show them apart.
			var rare := false
			for it in info.get("items", []):
				if it["name"] == g["item"]:
					rare = int(it.get("stack_max", 99)) <= 3
			var n: Node3D = theme.loot_node({"item": g["item"], "color": g["color"], "count": g["count"], "rare": rare})
			add_child(n)
			gems[id] = n
		var p: Vector2 = g["position"]
		var n: Node3D = gems[id]
		n.position = Vector3(p.x, _height(p.x, p.y), p.y)
		n.rotation.y = float(id % 360) * 0.0174533
	for id in gems.keys():
		if not seen.has(id):
			gems[id].queue_free()
			gems.erase(id)

func _camera() -> void:
	var me: Node3D = ents.get(net.get_self_id())
	if me:
		me.visible = not fpv # our own body would fill the view
	# The mouse is ours to look with in first person, except while it is needed for the map or typing.
	var grab: bool = fpv and not world_map.visible and not hud.typing()
	var mode := Input.MOUSE_MODE_CAPTURED if grab else Input.MOUSE_MODE_VISIBLE
	if Input.mouse_mode != mode:
		Input.mouse_mode = mode
	if fpv:
		var feet := me.position.y if me else _height(me_pos.x, me_pos.y)
		var eye := Vector3(me_pos.x, feet + 1.7 * theme.entity_scale, me_pos.y)
		cam.position = eye
		# Looking along the way W walks: (-sin yaw, -cos yaw) in the ground plane, tipped by the pitch.
		cam.look_at(eye + Vector3(-sin(yaw) * cos(look_pitch), sin(look_pitch), -cos(yaw) * cos(look_pitch)))
		return
	var c := Vector3(me_pos.x, (me.position.y if me else _height(me_pos.x, me_pos.y)) + 1.0, me_pos.y)
	var pitch := deg_to_rad(42.0)
	cam.position = c + Vector3(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch)) * zoom
	cam.look_at(c)

func _drive() -> void:
	if smoke:
		return # the scripted session steers
	if _loading or my_held in [1, 2, 3]:
		net.set_move(Vector2.ZERO) # held: the server would refuse the steps anyway
		return
	if hud.typing():
		net.set_move(Vector2.ZERO)
		return
	if _fetch_pile != 0 and _fetch():
		return
	if args.has("walk"):
		if String(args["walk"]) == "near" and me_pos != Vector2.ZERO:
			args["walk"] = _beside_nearest_building()
		if String(args["walk"]) == "near":
			return
		# Waypoints separated by ";". When it stops getting anywhere (a wall in the way), sidestep for a moment.
		var legs := String(args["walk"]).split(";")
		var to := Vector2(float(legs[0].get_slice(",", 0)), float(legs[0].get_slice(",", 1)))
		if me_pos.distance_to(to) > 2.0:
			var dir := (to - me_pos).normalized()
			if t_total - walk_check > 0.6:
				if me_pos.distance_to(walk_from) < 2.0:
					walk_dodge = t_total + 1.2
					walk_side = -walk_side
				walk_from = me_pos
				walk_check = t_total
			if t_total < walk_dodge:
				dir = (dir.orthogonal() * walk_side + dir * 0.3).normalized()
			net.set_move(dir)
			return
		legs.remove_at(0)
		if legs.is_empty():
			args.erase("walk")
		else:
			args["walk"] = ";".join(legs)
		net.set_move(Vector2.ZERO)
	var f := 0.0
	var r := 0.0
	if Input.is_key_pressed(KEY_W) or Input.is_key_pressed(KEY_UP): f += 1.0
	if Input.is_key_pressed(KEY_S) or Input.is_key_pressed(KEY_DOWN): f -= 1.0
	if Input.is_key_pressed(KEY_D) or Input.is_key_pressed(KEY_RIGHT): r += 1.0
	if Input.is_key_pressed(KEY_A) or Input.is_key_pressed(KEY_LEFT): r -= 1.0
	if Input.is_key_pressed(KEY_Q): yaw -= 0.03
	if Input.is_key_pressed(KEY_E): yaw += 0.03
	# Camera relative: forward is away from the camera, in server coordinates (x east, y south).
	var fwd := Vector2(-sin(yaw), -cos(yaw))
	var right := Vector2(cos(yaw), -sin(yaw))
	net.set_move(fwd * f + right * r)

## Going for a pile of loot that was clicked: walk to it and pick it up there. Any movement key, the pile going
## (someone else took it) or a long time trying gives it up. True while it is steering us.
func _fetch() -> bool:
	var keys := [KEY_W, KEY_A, KEY_S, KEY_D, KEY_UP, KEY_DOWN, KEY_LEFT, KEY_RIGHT]
	var at := Vector2.INF
	for g in net.ground():
		if g["id"] == _fetch_pile:
			at = g["position"]
	if at == Vector2.INF or t_total > _fetch_until or keys.any(func(k): return Input.is_key_pressed(k)):
		_fetch_pile = 0
		return false
	if me_pos.distance_to(at) <= PICKUP_REACH:
		net.set_move(Vector2.ZERO)
		net.pickup_id(_fetch_pile)
		_fetch_pile = 0
		return true
	net.set_move((at - me_pos).normalized())
	return true

## First person on or off. Aiming (targeting, building) then goes through the crosshair in the middle.
func _set_fpv(on: bool) -> void:
	fpv = on
	look_pitch = -0.12
	hud.set_crosshair(on)

## Where the pointer aims: the mouse, or in first person the crosshair.
func _pointer() -> Vector2:
	return get_viewport().get_visible_rect().size / 2.0 if fpv else get_viewport().get_mouse_position()

func _unhandled_input(ev: InputEvent) -> void:
	if net == null or _loading:
		return
	if ev is InputEventMouseMotion and fpv and Input.mouse_mode == Input.MOUSE_MODE_CAPTURED:
		yaw -= ev.relative.x * LOOK
		look_pitch = clampf(look_pitch - ev.relative.y * LOOK, -1.4, 1.4)
		return
	if ev is InputEventMouseButton and ev.pressed:
		# Scrolling in past the closest view goes into first person, and scrolling out comes back.
		if ev.button_index == MOUSE_BUTTON_WHEEL_UP:
			if not fpv and zoom <= 8.0:
				_set_fpv(true)
			zoom = clampf(zoom - 2.0, 8.0, 60.0)
		elif ev.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			if fpv:
				_set_fpv(false)
			else:
				zoom = clampf(zoom + 2.0, 8.0, 60.0)
		elif building:
			# The right button breaks by being held (_mine), not by a click.
			if ev.button_index == MOUSE_BUTTON_LEFT and not ev.shift_pressed:
				_build(true)
		elif ev.button_index == MOUSE_BUTTON_LEFT:
			_pick(_pointer())
	elif ev is InputEventKey and ev.pressed and not ev.echo:
		var k: int = ev.keycode
		# The journal and the menu come first: Esc closes them, and while one is up the game keys rest.
		if k == KEY_F2:
			journal.toggle_journal()
			return
		if k == KEY_F3:
			questui.toggle_log()
			return
		if k == KEY_ESCAPE and (shopui.escape() or questui.escape() or journal.escape()):
			return
		if journal.is_open() or questui.is_open() or shopui.is_open():
			return
		# Keys every mode shares: move (polled in _drive), jump, chat, map, god, full screen, the mode switch.
		if k == KEY_ENTER or k == KEY_KP_ENTER:
			hud.open_chat()
		elif k == KEY_F1:
			hud.toggle_help()
		elif k == KEY_G:
			_toggle_god()
		elif k in SPELL_KEYS and not building:
			_cast(SPELL_KEYS.find(k))
		elif k == KEY_J:
			net.act(ACT_JUMP, 0)
		elif k == KEY_M:
			_toggle_map()
		elif k == KEY_N:
			_notice(sound.cycle())
		elif k == KEY_V:
			_set_fpv(not fpv)
		elif k == KEY_L:
			_cycle_torch()
		elif k == KEY_F11:
			var full := DisplayServer.window_get_mode() == DisplayServer.WINDOW_MODE_FULLSCREEN
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_WINDOWED if full else DisplayServer.WINDOW_MODE_FULLSCREEN)
		elif k == KEY_ESCAPE and world_map.visible:
			world_map.visible = false
		elif k == KEY_ESCAPE and hud.help.visible:
			hud.toggle_help()
		elif k == KEY_B:
			_set_building(not building)
		elif building:
			# Build mode: the number row picks a material and nothing else, so it never doubles as an item key.
			if k == KEY_ESCAPE:
				_set_building(false)
			elif MATERIAL_KEYS.has(k) and MATERIAL_KEYS.find(k) < int(build.get("kinds", 4)):
				build_kind = MATERIAL_KEYS.find(k)
		else:
			# Action mode: fight, target and items.
			if k == KEY_ESCAPE and fpv:
				_set_fpv(false)
			elif k == KEY_ESCAPE:
				journal.open_menu()
			elif k == KEY_TAB:
				_cycle_target()
			elif k == KEY_SPACE:
				_attack()
			elif k == KEY_F:
				_talk_or_pickup()
			elif k >= KEY_1 and k <= KEY_8:
				if ev.shift_pressed:
					net.drop_slot(k - KEY_1)
				else:
					net.use_slot(k - KEY_1)

## Switch between action mode and build mode. There is no build mode when the server offers no building.
func _set_building(on: bool) -> void:
	building = on and not build.is_empty()

func _on_chat(text: String) -> void:
	if text.begins_with("/name "):
		net.set_display_name(text.trim_prefix("/name ").strip_edges())
		return
	var shout := text.begins_with("!")
	net.say(text.trim_prefix("!").strip_edges(), shout)

## Attack the target if it is in reach, otherwise whatever is nearest within reach (and target that). The server
## checks range, cooldown and energy and ignores a request that fails, so the checks it can see are made here
## first and explained, instead of a swing that silently does nothing.
## The world map (M): the theme paints the picture once; you and what is around you are drawn on top.
func _toggle_map() -> void:
	if not world_map.visible and not world_map.built:
		world_map.build(theme.map_image(512), world.size)
	world_map.visible = not world_map.visible

func _attack() -> void:
	if attack_id < 0 or t_total < next_swing:
		return
	if target_id == 0 or not ents.has(target_id) or _dist_to(target_id) > action_range:
		var near := _nearest_in(action_range)
		if near == 0:
			if target_id != 0 and ents.has(target_id):
				_notice("Too far: %d units away, reach is %d." % [int(_dist_to(target_id)), int(action_range)])
			else:
				_notice("Nothing within reach (%d units). Get closer." % int(action_range))
			return
		target_id = near
	if track.has(target_id) and track[target_id].get("air", 0.0) > 0.0:
		_float_text((ents[target_id] as Node3D).position, "dodge", Color("9fe8e0"))
	net.act(attack_id, target_id)
	next_swing = t_total + 0.5
	sound.play("swing", -2.0)
	_fight_at = t_total
	var me: Node3D = ents.get(net.get_self_id())
	if me and me.has_method("attack"):
		_face(me, (ents[target_id] as Node3D).position)
		me.attack()

## The first thing seen in the world: who you are, where your quest begins, and luck for the road, fading away.
func _welcome() -> void:
	var where := ("in " + _here) if _here != "" else ""
	if where == "":
		# The nearest named place, if one is close; else the wilds.
		var best := 400.0
		for l in locations:
			if String(l.get("title", "")) == "":
				continue
			var d := Vector2(float(l["x"]), float(l["y"])).distance_to(me_pos)
			if d < best:
				best = d
				where = ("near " if d > float(l["r"]) else "in ") + String(l["title"])
	if where == "":
		where = "in the wilds"
	hud.welcome(_name(net.get_self_id()), where)

## In the world: the title theme fades out over a few seconds while the game's own music comes up under it.
func _fade_title() -> void:
	if not _title_on:
		return
	_title_on = false
	var t := create_tween()
	t.tween_property(title_music, "volume_db", -60.0, 5.0).set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN)
	t.tween_callback(title_music.stop)

## The spell bar from `bar_order`: the HUD's slots, and the journal's view of it.
func _apply_bar() -> void:
	spells = []
	for n in bar_order:
		for a in all_spells:
			if a["name"] == n:
				spells.append(a)
	var hz := maxf(float(info.get("hz", 20)), 1.0)
	var keys := []
	for k in SPELL_KEYS:
		keys.append(OS.get_keycode_string(k))
	var bar := []
	for i in spells.size():
		var a: Dictionary = spells[i]
		bar.append({"title": _spell_title(a).get_slice(" ", 0).left(9), "key": keys[i],
			"color": SpellFx.color(String(a.get("fx", ""))) if a.has("fx") else Color(0, 0, 0, 0),
			"tip_title": "%s  (%s)" % [_spell_title(a), keys[i]], "tip": _spell_text(a, hz)})
	hud.set_spells(bar)
	var book := []
	for a in all_spells:
		book.append({"name": a["name"], "title": _spell_title(a), "desc": String(a.get("desc", "")),
			"school": String(a.get("school", "")), "cost": _spell_cost(a, hz), "level": int(a.get("level", 0)), "learnt": my_level <= 0 or int(a.get("level", 0)) <= my_level,
			"color": SpellFx.color(String(a.get("fx", ""))) if a.has("fx") else HudScript.MUTED})
	journal.set_spells(book, bar_order, keys)

## What a spell does and costs, in words: its description, then its cost and cooldown.
func _spell_text(a: Dictionary, hz: float) -> String:
	var t := String(a.get("desc", ""))
	var c := _spell_cost(a, hz)
	return (t + "\n" + c).strip_edges() if c != "" else t

func _spell_cost(a: Dictionary, hz: float) -> String:
	var costs := []
	if a.has("requires"):
		costs.append("%d %s" % [int(a["requires"]["min"]), a["requires"]["attr"]])
	if int(a.get("cooldown_ticks", 0)) > 0:
		costs.append("ready again in %s s" % str(snappedf(float(a["cooldown_ticks"]) / hz, 0.1)))
	if costs.is_empty():
		return ""
	var t := ", ".join(costs)
	return t.left(1).to_upper() + t.substr(1)

## The bar kept for this server, or the first spells in the game's order; spells the game no longer has drop out.
func _load_bar() -> Array:
	var cfg := ConfigFile.new()
	cfg.load("user://spellbar.cfg")
	var names := []
	for a in all_spells:
		names.append(a["name"])
	var saved = cfg.get_value("bars", String(args["web"]), null)
	if saved is Array:
		return saved.filter(func(n): return names.has(n)).slice(0, SPELL_KEYS.size())
	return names.slice(0, SPELL_KEYS.size())

func _save_bar() -> void:
	var cfg := ConfigFile.new()
	cfg.load("user://spellbar.cfg")
	cfg.set_value("bars", String(args["web"]), bar_order)
	cfg.save("user://spellbar.cfg")

## Log out: leave the server and go back to the join screen.
func _log_out() -> void:
	if net != null:
		net.disconnect_from_server()
	_logged_out = true
	_hop = {}
	get_tree().reload_current_scene()

## Quests each frame: where we stand (the tracker, an offer just made), and a gold ! or ? over anyone with a quest
## to give us or to take back.
func _quests_frame(list: Array, self_id: int) -> void:
	_social_frame(list, self_id)
	var q: Dictionary = net.quests()
	questui.update(q.get("list", []), int(q.get("offer", -1)))
	var marks: Dictionary = questui.marks(my_level)
	for e in list:
		var id := int(e["id"])
		if id == self_id or not ents.has(id):
			continue
		var node: Node3D = ents[id]
		var want: String = marks.get(_name(id), "")
		var mark: Label3D = node.get_node_or_null("qmark")
		if want == "":
			if mark:
				mark.queue_free()
			continue
		if mark == null:
			mark = Label3D.new()
			mark.name = "qmark"
			mark.billboard = BaseMaterial3D.BILLBOARD_ENABLED
			mark.no_depth_test = true
			mark.font = HudScript.TITLE
			mark.font_size = 96
			mark.outline_size = 18
			mark.modulate = Color("ffd27a")
			mark.outline_modulate = Color(0, 0, 0, 0.9)
			mark.position = Vector3(0, 3.4, 0)
			mark.pixel_size = 0.006
			node.add_child(mark)
		mark.text = want

## Each frame: the party frames, what we wear (the journal), the shop's view of our bag, and horses under riders.
func _social_frame(list: Array, self_id: int) -> void:
	var by_id := {}
	for e in list:
		by_id[int(e["id"])] = e
	var mates := []
	for id in net.party():
		if int(id) == self_id:
			continue
		var e: Dictionary = by_id.get(int(id), {})
		var hp := clampf(float(e.get(health_idx, 0.0)) / health_max, 0.0, 1.0) if not e.is_empty() and health_idx != "" else -1.0
		mates.append([_name(int(id)), hp])
	hud.set_party(mates)
	var gear: Dictionary = net.gear()
	journal.set_gear(gear)
	if shopui.is_open():
		if not ents.has(_shop_with) or _dist_to(_shop_with) > 10.0:
			shopui.escape() # walked away
		elif int(t_total * 4.0) != int((t_total - get_process_delta_time()) * 4.0):
			shopui.refresh(net.bag())
	# Riders: our own from the server's word, others by their speed (a mount's worth of it).
	var riding := {}
	if bool(gear.get("riding", false)):
		riding[self_id] = true
	for e in list:
		if float(e.get("speed", 1.0)) >= 1.75:
			riding[int(e["id"])] = true
	for id in _horses.keys():
		if not riding.has(id) or not ents.has(id):
			(_horses[id] as Node3D).queue_free()
			_horses.erase(id)
	for id in riding:
		if not ents.has(id):
			continue
		var node: Node3D = ents[id]
		if not _horses.has(id):
			var h := _make_horse()
			add_child(h)
			_horses[id] = h
		var horse: Node3D = _horses[id]
		horse.position = node.position
		horse.rotation.y = node.rotation.y
		node.position.y += 0.95 # in the saddle
		horse.get_node("legs").rotation.x = sin(t_total * 14.0) * 0.35 * minf(1.0, (track.get(id, {}).get("vel", Vector3.ZERO) as Vector3).length() / 10.0)

## A plain low-poly horse, chestnut with a dark mane, facing -Z like the characters.
func _make_horse() -> Node3D:
	var root := Node3D.new()
	var coat := StandardMaterial3D.new()
	coat.albedo_color = Color("7a4a2a")
	coat.roughness = 0.85
	var dark := StandardMaterial3D.new()
	dark.albedo_color = Color("2a1a12")
	var part := func(mesh: Mesh, at: Vector3, rot := Vector3.ZERO, mat: Material = coat, parent: Node3D = root) -> MeshInstance3D:
		var m := MeshInstance3D.new()
		m.mesh = mesh
		m.material_override = mat
		m.position = at
		m.rotation = rot
		parent.add_child(m)
		return m
	var body := CapsuleMesh.new()
	body.radius = 0.38
	body.height = 1.7
	part.call(body, Vector3(0, 1.05, 0), Vector3(PI / 2, 0, 0))
	var neck := CapsuleMesh.new()
	neck.radius = 0.2
	neck.height = 0.9
	part.call(neck, Vector3(0, 1.45, -0.75), Vector3(-0.6, 0, 0))
	var head := BoxMesh.new()
	head.size = Vector3(0.24, 0.26, 0.6)
	part.call(head, Vector3(0, 1.75, -1.05), Vector3(0.35, 0, 0))
	var mane := BoxMesh.new()
	mane.size = Vector3(0.08, 0.5, 0.12)
	part.call(mane, Vector3(0, 1.6, -0.65), Vector3(-0.6, 0, 0), dark)
	var tail := CylinderMesh.new()
	tail.top_radius = 0.07
	tail.bottom_radius = 0.02
	tail.height = 0.7
	part.call(tail, Vector3(0, 1.0, 0.95), Vector3(0.5, 0, 0), dark)
	var legs := Node3D.new()
	legs.name = "legs"
	root.add_child(legs)
	var leg := CylinderMesh.new()
	leg.top_radius = 0.08
	leg.bottom_radius = 0.06
	leg.height = 0.85
	for x in [-0.22, 0.22]:
		for z in [-0.55, 0.55]:
			var l: MeshInstance3D = part.call(leg, Vector3(x, 0.45, z), Vector3.ZERO, coat, legs)
			l.rotation.x = 0.0
	return root

## F: talk to the one targeted (or the nearest with a quest mark) if they are in reach; otherwise pick up.
func _talk_or_pickup() -> void:
	var reach := 8.0
	var who := 0
	if target_id != 0 and ents.has(target_id) and _dist_to(target_id) <= reach and (ents[target_id] as Node3D).has_node("qmark"):
		who = target_id
	else:
		var best := reach
		for id in ents:
			if id != net.get_self_id() and (ents[id] as Node3D).has_node("qmark") and _dist_to(id) <= best:
				best = _dist_to(id)
				who = id
	if who != 0:
		net.talk(who)
		var me: Node3D = ents.get(net.get_self_id())
		if me:
			_face(me, (ents[who] as Node3D).position)
		return
	# A trader in reach (the one targeted first): open their shop.
	var shops: Dictionary = info.get("shops", {}) if info.get("shops") is Dictionary else {}
	var trader := 0
	if target_id != 0 and ents.has(target_id) and _dist_to(target_id) <= reach and shops.has(_name(target_id)):
		trader = target_id
	else:
		var best := reach
		for id in ents:
			if id != net.get_self_id() and shops.has(_name(id)) and _dist_to(id) <= best:
				best = _dist_to(id)
				trader = id
	if trader != 0:
		_shop_with = trader
		shopui.open(_name(trader), shops[_name(trader)], net.bag())
		return
	net.pickup()

## Our level and experience from the server: show what we earned over our head, and celebrate a new level.
func _watch_progress() -> void:
	var p: Dictionary = net.progress()
	var level := int(p.get("level", 0))
	if level <= 0:
		return
	var me: Node3D = ents.get(net.get_self_id())
	if not _progress.is_empty() and me != null:
		var gained := int(p["xp"]) - int(_progress.get("xp", 0))
		if gained > 0:
			_float_text(me.position + Vector3(0, 0.6, 0), "+%d xp" % gained, Color("c9a227"))
		if level > my_level and my_level > 0:
			hud.announce("Level %d" % level)
			sound.play("chime", 2.0, 0.0)
			sound.play("heal", 0.0, 0.0)
			spellfx.cast(me, null, {"fx": "holy", "hits": "friends", "area": 3.0}, func(_p): pass)
	var was := my_level
	my_level = level
	_progress = p
	if was != level:
		_apply_bar() # the journal shows what is learnt now

## A spell's name as players read it: "liathroid_tine" -> "Liathroid tine".
func _spell_title(a: Dictionary) -> String:
	return String(a["name"]).replace("_", " ").capitalize()

func _can_pay(a: Dictionary, me: Dictionary) -> bool:
	if not a.has("requires"):
		return true
	var attr := String(a["requires"]["attr"])
	return not me.has(attr) or float(me[attr]) >= float(a["requires"]["min"]) # not replicated: the server will say
	
## Cast spell `i` of the spell bar: at the target (or the nearest thing in reach), at a friend, or round yourself.
func _cast(i: int) -> void:
	if i < 0 or i >= spells.size() or net == null or _loading or my_held in [1, 2]:
		return
	var a: Dictionary = spells[i]
	var id := int(a["id"])
	var title := _spell_title(a)
	if t_total < float(spell_ready.get(id, 0.0)):
		return _notice("%s is not ready yet." % title)
	if t_total < _gcd_until:
		return
	if my_level > 0 and int(a.get("level", 0)) > my_level:
		return _notice("You learn %s at level %d." % [title, int(a["level"])])
	var me := _my_entity()
	if not _can_pay(a, me):
		return _notice("Not enough %s for %s." % [a["requires"]["attr"], title])
	var target := 0
	if String(a.get("aim", "")) != "self":
		var reach := float(a.get("range", 8.0))
		if String(a.get("hits", "")) == "friends":
			target = target_id # not a friend, or none: the server lands it on you
		else:
			if target_id == 0 or not ents.has(target_id) or _dist_to(target_id) > reach:
				var near := _nearest_in(reach)
				if near == 0:
					return _notice("Nothing within reach of %s (%d units)." % [title, int(reach)])
				target_id = near
			target = target_id
	net.act(id, target)
	var cd := float(a.get("cooldown_ticks", 0)) / maxf(float(info.get("hz", 20)), 1.0)
	spell_ready[id] = t_total + cd
	_gcd_until = t_total + minf(cd, 0.5)
	_fight_at = t_total
	var my: Node3D = ents.get(net.get_self_id())
	if my and target != 0 and ents.has(target):
		_face(my, (ents[target] as Node3D).position)

func _my_entity() -> Dictionary:
	for e in net.entities():
		if int(e["id"]) == net.get_self_id():
			return e
	return {}

## Each frame: draw the spells used near us (the server tells everyone near, us included), show who is held, and
## keep the spell bar's cooldowns and costs current.
func _spells_frame(list: Array, self_id: int) -> void:
	_watch_progress()
	_quests_frame(list, self_id)
	for k in net.take_casts():
		var a: Dictionary = actions_by_id.get(int(k["action"]), {})
		var caster: Node3D = ents.get(int(k["caster"]))
		if caster == null or a.is_empty():
			continue
		var target: Node3D = ents.get(int(k["target"])) if int(k["target"]) != 0 else null
		var fx := String(a.get("fx", ""))
		var d := Vector2(caster.position.x, caster.position.z).distance_to(me_pos)
		if int(k["caster"]) != self_id and caster.has_method("attack"):
			if target != null:
				_face(caster, target.position)
			caster.attack() # others swing and cast where we can see them
		if fx == "":
			continue
		sound.play("cast_" + fx, SoundScript.at_distance(d) - 2.0)
		var friendly := String(a.get("hits", "")) == "friends"
		spellfx.cast(caster, target, a, func(at: Vector3):
			var dd := Vector2(at.x, at.z).distance_to(me_pos)
			sound.play("heal" if friendly else "impact_" + fx, SoundScript.at_distance(dd)))
	var held := {}
	var floating := {}
	var hasted := {}
	var me := {}
	for e in list:
		var id := int(e["id"])
		var h := int(round(float(e.get("held", 0.0))))
		if h > 0:
			held[id] = h
		# Levitating: the server holds the jump at its height, so "airborne" stays put far longer than a jump.
		var air := float(e.get("airborne", 0.0)) / air_max if air_max > 0.0 else 0.0
		var was: Dictionary = _aloft.get(id, {"air": -1.0, "since": t_total})
		if absf(air - float(was["air"])) > 0.02 or air <= 0.0:
			was = {"air": air, "since": t_total}
		_aloft[id] = was
		if air > 0.0 and t_total - float(was["since"]) > 0.4:
			floating[id] = true
		if float(e.get("speed", 1.0)) > 1.05:
			hasted[id] = true
		if id == self_id:
			me = e
	for id in _aloft.keys():
		if not ents.has(id):
			_aloft.erase(id)
	spellfx.show_states(held, floating, hasted, ents)
	var now := int(held.get(self_id, 0))
	if now != my_held:
		if now > 0:
			_notice(HOLD_WORDS[clampi(now, 0, 3)])
			sound.play("held", 0.0, 0.0)
		my_held = now
	if spells.is_empty():
		return
	var cools := []
	var can := []
	var hz := maxf(float(info.get("hz", 20)), 1.0)
	for a in spells:
		var cd := float(a.get("cooldown_ticks", 0)) / hz
		var left := float(spell_ready.get(int(a["id"]), 0.0)) - t_total
		cools.append(clampf(left / cd, 0.0, 1.0) if cd > 0.0 else 0.0)
		can.append(_can_pay(a, me))
	hud.update_spells(cools, can)

func _in_safe() -> bool:
	for l in locations:
		if l.get("safe", false) == true and Vector2(float(l["x"]), float(l["y"])).distance_to(me_pos) <= float(l["r"]):
			return true
	return false

func _dist_to(id: int) -> float:
	var n: Node3D = ents.get(id)
	return Vector2(n.position.x, n.position.z).distance_to(me_pos) if n else INF

func _nearest_in(r: float) -> int:
	var best := 0
	var best_d := r
	for id in ents:
		if id == net.get_self_id():
			continue
		var d := _dist_to(id)
		if d <= best_d:
			best_d = d
			best = id
	return best

func _notice(text: String) -> void:
	notice = text
	notice_until = t_total + 2.5
	hud.notify(text)

func _cycle_torch() -> void:
	theme.torch_mode = (theme.torch_mode + 1) % 3
	_notice(["Torch: lit when it is dark", "Torch: always lit", "Torch: out"][theme.torch_mode])

## The action bar's ability buttons do what their keys do.
func _on_ability(name: String) -> void:
	match name:
		"attack":
			_attack()
		"jump":
			net.act(ACT_JUMP, 0)
		"torch":
			_cycle_torch()
		"map":
			_toggle_map()
		"build":
			_set_building(not building)
		"journal":
			journal.toggle_journal()
		"quests":
			questui.toggle_log()

## The frame bars for an entity: every replicated attribute except the bookkeeping ones (airborne, lit).
func _bars(e: Dictionary, mine: bool) -> Array:
	var out := []
	for a in info.get("attrs", []):
		var n := String(a["name"])
		if not a.get("replicated", false) or n in ["airborne", "lit", "held", "speed", "level"] or not e.has(n):
			continue
		var mx := maxf(float(a["max"]), 1.0)
		var v := float(e[n])
		var col := Color("c23b2e") if n == health_idx else Color("3f7fd0") if n.contains("energy") or n.contains("mana") else Color("d0a03f")
		out.append([n, v / mx, "%d / %d" % [int(v), int(mx)] if mine else "%d%%" % int(round(v / mx * 100.0)), col])
	# Experience, on our own frame: how far through this level.
	if mine and my_level > 0:
		var span := int(_progress.get("next", 0)) - int(_progress.get("at", 0))
		var into := int(_progress.get("xp", 0)) - int(_progress.get("at", 0))
		if span > 0:
			out.append(["xp", clampf(float(into) / span, 0.0, 1.0), "%d / %d xp" % [into, span], Color("c9a227")])
		else:
			out.append(["xp", 1.0, "Highest level", Color("c9a227")])
	return out

## God mode lives on the server (admin API, loopback only): your attributes are held at full every tick.
func _toggle_god() -> void:
	var code := await _post("http://%s/api/god" % args["admin"], {"id": net.get_self_id(), "on": not god})
	if code == 200:
		god = not god
		_notice("God mode on: you cannot die and never run out of energy." if god else "God mode off.")
	else:
		_notice("God mode needs the server's admin API on this machine (--admin %s did not answer)." % args["admin"])

func _cycle_target() -> void:
	var best := 0
	var best_d := 40.0
	var after := target_id
	var cands: Array = []
	for e in net.entities():
		if e["id"] == net.get_self_id():
			continue
		var d: float = (e["position"] as Vector2).distance_to(me_pos)
		if d < 40.0:
			cands.append([d, e["id"]])
	cands.sort()
	for c in cands:
		if c[1] != after:
			best = c[1]
			break
	target_id = best if best != 0 else target_id

func _pick(screen: Vector2) -> void:
	var from := cam.project_ray_origin(screen)
	var dir := cam.project_ray_normal(screen)
	if absf(dir.y) < 0.001:
		return
	# Where the ray meets the ground: on the flat first, then a few steps onto the hills' real height.
	var t := -from.y / dir.y
	var hit := from + dir * t
	for _i in 4:
		t = (_height(hit.x, hit.z) - from.y) / dir.y
		hit = from + dir * t
	# A pile of loot clicked: take it, walking over first when it is out of reach.
	var pile := 0
	var pile_d := 1.4
	for g in net.ground():
		var d := (g["position"] as Vector2).distance_to(Vector2(hit.x, hit.z))
		if d < pile_d:
			pile_d = d
			pile = g["id"]
	if pile != 0:
		_fetch_pile = pile
		_fetch_until = t_total + 12.0
		return
	var best := 0
	var best_d := 2.6
	for e in net.entities():
		if e["id"] == net.get_self_id():
			continue
		var d := (e["position"] as Vector2).distance_to(Vector2(hit.x, hit.z))
		if d < best_d:
			best_d = d
			best = e["id"]
	target_id = best

# ---------------------------------------------------------------- scripted session for CI

func _post(url: String, body: Dictionary) -> int:
	return await _request(url, HTTPClient.METHOD_POST, body)

func _request(url: String, method: int, body: Dictionary) -> int:
	return await _request_text(url, method, JSON.stringify(body))

func _request_text(url: String, method: int, body: String) -> int:
	var r := HTTPRequest.new()
	add_child(r)
	r.request(url, ["Content-Type: application/json", "X-Admin: 1"], method, body)
	var res: Array = await r.request_completed
	r.queue_free()
	if res[1] != 200:
		print("%s said %d: %s" % [url, res[1], (res[3] as PackedByteArray).get_string_from_utf8().left(300)])
	return res[1]

func _check(name: String, ok: bool, extra := "") -> void:
	smoke_log.append("%s %s %s  (%.1f s)" % ["PASS" if ok else "FAIL", name, extra, Time.get_ticks_msec() / 1000.0])
	print(smoke_log[-1])
	if not ok:
		print("SMOKE FAIL")
		get_tree().quit(1)

func _wait(seconds: float, cond: Callable) -> bool:
	var t := Time.get_ticks_msec()
	while Time.get_ticks_msec() - t < seconds * 1000.0:
		if cond.call():
			return true
		await get_tree().process_frame
	return cond.call()

func _smoke() -> void:
	var ok := await _wait(10.0, func(): return net.get_status() == 1 and net.get_self_id() != 0 and ents.has(net.get_self_id()))
	_check("joins and sees itself", ok, "id=%d" % net.get_self_id())
	_check("replicated attribute present", health_idx != "", health_idx)
	_check("world loaded and themed", world != null and theme != null, "%d objects, %d by tile so far" % [world.objects.size(), world.point_count()])
	# Sound: every file there, and the listener hearing the land (muted here, so it is what is wanted that counts).
	var missing := []
	for n in SoundScript.AMBIENCE:
		if not ResourceLoader.exists("res://sounds/ambience/%s.ogg" % n):
			missing.append(n)
	for n in SoundScript.MUSIC:
		if not ResourceLoader.exists("res://sounds/music/%s.ogg" % n):
			missing.append(n)
	for fx in SpellFx.COLORS:
		for kind in ["cast_", "impact_"]:
			if not ResourceLoader.exists("res://sounds/sfx/%s%s.ogg" % [kind, fx]):
				missing.append(kind + fx)
	for n in ["swing_1", "hit_1", "hurt", "pickup", "drop", "place", "break", "horn", "chime", "splash", "step_grass_1", "step_road_1", "step_stone_1", "step_water_1", "heal", "held"]:
		if not ResourceLoader.exists("res://sounds/sfx/%s.ogg" % n):
			missing.append(n)
	_check("every sound is there", missing.is_empty(), str(missing))
	ok = await _wait(3.0, func(): return float(sound.want.get("wind", 0.0)) > 0.0 and sound.music_for != "")
	_check("the land is heard and music is chosen", ok, "wind %.2f, music %s" % [float(sound.want.get("wind", 0.0)), sound.music_for])
	# Walk east; if a wall happens to be in the way, try the other directions.
	var start := me_pos
	for dir in [Vector2(1, 0), Vector2(-1, 0), Vector2(0, 1), Vector2(0, -1)]:
		var t := Time.get_ticks_msec()
		while Time.get_ticks_msec() - t < 1500:
			net.set_move(dir)
			await get_tree().process_frame
		if me_pos.distance_to(start) > 5.0:
			break
	net.set_move(Vector2.ZERO)
	_check("walks", me_pos.distance_to(start) > 5.0, "moved %.1f" % me_pos.distance_to(start))
	if world_parts:
		var streamed := await _wait(5.0, func(): return world.point_count() > 0 and world_tiles.size() >= 9)
		_check("trees and rocks stream in by tile", streamed, "%d objects in %d tiles" % [world.point_count(), world_tiles.size()])
	# Whatever the theme wants checked about its own drawing.
	for c in theme.smoke_checks():
		_check(String(c[0]), bool(c[1]), String(c[2]))
	if air_max > 0.0:
		net.act(ACT_JUMP, 0)
		ok = await _wait(3.0, func(): return track.get(net.get_self_id(), {}).get("air", 0.0) > 0.0)
		_check("jumps", ok, "")
		ok = await _wait(3.0, func(): return track.get(net.get_self_id(), {}).get("air", 1.0) == 0.0)
		_check("and lands", ok, "")
	# Building: put a block next to us in its column's lowest layer (trying a few sides, in case a wall is in the
	# way) and see it arrive, owned by us, and be drawn. Climb onto it and off again. Then a second block on top,
	# the bottom one broken out from under it (the top one stays up), and that one broken too.
	if not build.is_empty():
		var bs := float(build["block"])
		var built := -1
		var base := 0
		for off in [Vector2(4, 0), Vector2(-4, 0), Vector2(0, 4), Vector2(0, -4), Vector2(3, 3), Vector2(-3, -3), Vector2(3, -3), Vector2(-3, 3)]:
			var at: Vector2 = me_pos + off
			# Trees and rocks are solid: build where the walk onto the block is not through a trunk.
			if not _path_clear(me_pos, at):
				continue
			var c := Vector2i(int(floor(at.x / bs)), int(floor(at.y / bs)))
			base = _col_base(c.x, c.y)
			net.act_arg(int(build["act_build"]), c.x | c.y << 15, _build_arg(base, 2))
			blocks_poll = 0.2
			if await _wait(3.0, func(): return blocks.has(c.y << 15 | c.x)):
				built = c.y << 15 | c.x
				base = int(blocks[built]["b"])
				break
		_check("builds a block", built >= 0, "%d column(s)" % blocks.size())
		if built >= 0:
			var bx := built & 0x7fff
			var by := built >> 15
			_check("and the theme draws it", theme._block_nodes.size() > 0, "%d chunk(s)" % theme._block_nodes.size())
			_check("and it is ours", my_tag != "" and _owner_of(bx, by, base) == my_tag, "owner '%s', us '%s'" % [_owner_of(bx, by, base), my_tag])
			# One block high is a step: walk onto it, stand on top, and come back down.
			var top := Vector2((bx + 0.5) * bs, (by + 0.5) * bs)
			var back := me_pos
			net.set_move((top - me_pos).normalized())
			ok = await _wait(3.0, func(): return my_layer == base + 1)
			net.set_move(Vector2.ZERO)
			_check("climbs onto it", ok, "layer %d (block in %d, ground layer %d) at %.1f, %.1f, %.1f from the block, started %.1f away" % [my_layer, base, _col_base(int(floor(back.x / bs)), int(floor(back.y / bs))), me_pos.x, me_pos.y, me_pos.distance_to(top), back.distance_to(top)])
			var drawn: Node3D = ents.get(net.get_self_id())
			var block_top := (base + 1) * bs
			# The drawn character eases up onto the block, so give it a moment to arrive.
			ok = await _wait(2.0, func(): return drawn != null and drawn.position.y > block_top - 0.2)
			_check("and is drawn on top", ok, "y %.2f, block top %.2f" % [drawn.position.y if drawn else -1.0, block_top])
			net.set_move((back - me_pos).normalized())
			ok = await _wait(3.0, func(): return my_layer == GROUND)
			net.set_move(Vector2.ZERO)
			_check("and steps off", ok, "layer %d" % my_layer)
			net.act_arg(int(build["act_build"]), bx | by << 15, _build_arg(base + 1, 11))
			blocks_poll = 0.2
			ok = await _wait(3.0, func(): return _solid(bx, by, base + 1))
			_check("stacks a second block, in the last material", ok and String(blocks[built]["k"]).ends_with("b"), "%s" % [blocks.get(built, {})])
			net.act_arg(int(build["act_break"]), bx | by << 15, _build_arg(base, 0))
			blocks_poll = 0.2
			ok = await _wait(3.0, func(): return not _solid(bx, by, base) and _solid(bx, by, base + 1))
			_check("breaks the bottom one and the top one stays", ok, "%s" % [blocks.get(built, {})])
			net.act_arg(int(build["act_break"]), bx | by << 15, _build_arg(base + 1, 0))
			blocks_poll = 0.2
			ok = await _wait(3.0, func(): return not blocks.has(built))
			_check("and breaks that too", ok, "")
			_check("blocks that go crumble", crumbled >= 2, "%d crumbled" % crumbled)
			# Night: the time of day comes from the schedule as the server works it out, the torch lights, and a
			# lamp block gives light.
			var dl: Dictionary = info.get("daylight", {}) if info.get("daylight") is Dictionary else {}
			var smp: Array = dl.get("sample", [0, -1])
			var mine: float = theme.daylight_phase(dl, float(smp[0]))
			_check("works out the time of day as the server does", absf(mine - float(smp[1])) < 1e-6, "%.6f vs %.6f (%s)" % [mine, float(smp[1]), dl.get("mode", "?")])
			var was: float = theme.time_override
			theme.time_override = 0.8
			ok = await _wait(3.0, func(): return theme.darkness > 0.8 and theme.torch_lit())
			_check("at night our torch is lit", ok, "darkness %.2f" % theme.darkness)
			net.act_arg(int(build["act_build"]), bx | by << 15, _build_arg(base, 12))
			blocks_poll = 0.2
			ok = await _wait(3.0, func(): return blocks.has(built) and String(blocks[built]["k"]).begins_with("c"))
			await get_tree().create_timer(0.5).timeout
			_check("a lamp block gives light", ok and theme.lights_burning() > 0, "%s, %d light(s)" % [blocks.get(built, {}), theme.lights_burning()])
			if has_lit:
				# Sneak: put the light out at night and the server knows (everyone else then hides our name).
				theme.torch_mode = 2
				ok = await _wait(5.0, func(): return not my_lit)
				_check("putting the light out tells the server (sneaking)", ok, "lit %s" % my_lit)
				theme.torch_mode = 0
				ok = await _wait(5.0, func(): return my_lit)
				_check("and lighting it again", ok, "lit %s" % my_lit)
			theme.time_override = 0.25
			ok = await _wait(3.0, func(): return theme.lights_burning() == 0 and not theme.torch_lit())
			_check("and at noon the lights are out", ok, "%d light(s)" % theme.lights_burning())
			theme.time_override = was
			net.act_arg(int(build["act_break"]), bx | by << 15, _build_arg(base, 0))
			blocks_poll = 0.2
	# Weather: define one, start it from the admin side, and the client should pick it up and make it rain.
	var admin_url := "http://%s" % args["admin"]
	var got := await _fetch(admin_url + "/api/game")
	if got.begins_with("{"):
		# Spliced into the text rather than parsed and re-encoded: GDScript's JSON turns every number into a float,
		# and the server's whole-number fields refuse 12.0.
		var rain_def := '{"name":"smoke-rain","color":"#5b7fa6","intensity":0.6,"sight_mult":0.8,"drain":[]}'
		var body := got
		var at := body.find('"weather":[')
		if at >= 0:
			body = body.insert(at + 11, rain_def + ("," if body[at + 11] != "]" else ""))
		else:
			body = "{\"weather\":[" + rain_def + "]," + body.substr(1)
		var put := await _request_text(admin_url + "/api/game", HTTPClient.METHOD_PUT, body)
		_check("weather defined", put == 200, str(put))
		_check("weather started", await _post(admin_url + "/api/weather", {"name": "smoke-rain", "duration_secs": 60}) == 200, "")
		weather_poll = 0.0
		ok = await _wait(10.0, func(): return weather.get("name", "") == "smoke-rain")
		_check("client shows the weather", ok, str(weather))
		_check("and the theme draws it", theme.weather_drawn() != "", theme.weather_drawn())
		await _post(admin_url + "/api/weather", {"name": null})
		weather_poll = 0.0
		ok = await _wait(10.0, func(): return weather.is_empty())
		_check("and clears it", ok, str(weather))
		# A weather zone round us: drawn as mist, and inside it its weather is the one we see; switched off live.
		var g2 := await _fetch(admin_url + "/api/game")
		var pl := g2.find("[", g2.find('"places"')) + 1 # the list's first character, however it is spaced
		if pl > 0 and not g2.contains('"zones"'):
			var empty := g2.substr(pl).strip_edges().begins_with("]")
			g2 = g2.insert(pl, '{"name":"smoke-haunt","x":%d,"y":%d,"r":15,"title":"Smoke Hollow"}' % [int(me_pos.x), int(me_pos.y)] + ("" if empty else ","))
			g2 = '{"zones":[{"name":"smoke-mist","place":"smoke-haunt","weather":"smoke-rain"}],' + g2.substr(1)
			if not g2.contains('"events"'):
				g2 = '{"events":[{"name":"smoke-raid","place":"smoke-haunt","steps":[{"kind":"announce","text":"The smoke rises","banner":true,"nearby":true}]}],' + g2.substr(1)
			_check("a weather zone is defined", await _request_text(admin_url + "/api/game", HTTPClient.METHOD_PUT, g2) == 200, "")
			weather_poll = 0.0
			ok = await _wait(10.0, func(): return theme.zone_count() > 0 and _in_zone == "smoke-mist")
			_check("it is drawn and we are in it", ok, "%d zone(s), in '%s'" % [theme.zone_count(), _in_zone])
			_check("and its weather is what we see", theme.weather_drawn() != "", theme.weather_drawn())
			ok = await _wait(10.0, func(): return _here == "Smoke Hollow")
			_check("its title is the location we are in", ok, "'%s'" % _here)
			_check("and it fades in", await _wait(3.0, func(): return hud.location.modulate.a > 0.9), "alpha %.2f" % hud.location.modulate.a)
			# An event's banner, to those nearby: across our screen.
			_check("an event can be started", await _post(admin_url + "/api/events/fire", {"name": "smoke-raid"}) == 200, "")
			ok = await _wait(5.0, func(): return hud.banner.text == "The smoke rises" and hud.banner.modulate.a > 0.5)
			_check("its banner goes across the screen", ok, "'%s'" % hud.banner.text)
			_check("it switches off live", await _post(admin_url + "/api/zone", {"name": "smoke-mist", "on": false}) == 200, "")
			weather_poll = 0.0
			ok = await _wait(10.0, func(): return theme.zone_count() == 0 and _in_zone == "")
			_check("and is gone", ok, "%d zone(s)" % theme.zone_count())
	var items: Array = info.get("items", [])
	if items.is_empty():
		print("SMOKE ok (no items defined)")
		get_tree().quit(0)
		return
	var admin := "http://%s" % args["admin"]
	var name: String = items[0]["name"]
	var code := await _post(admin + "/api/give", {"id": net.get_self_id(), "item": name, "count": 2})
	_check("admin give accepted", code == 200, str(code))
	ok = await _wait(5.0, func(): return net.bag().any(func(s): return s["item"] == name))
	_check("item arrives in the bag", ok, name)
	net.drop_slot(_slot_of(name))
	# Wait for our own drop, not just anything: bots fighting nearby leave loot too, and it can arrive first.
	ok = await _wait(5.0, func(): return _dropped_here(name) != 0 and gems.has(_dropped_here(name)))
	_check("dropped item appears on the ground", ok, "%d gems" % gems.size())
	var mine_id := _dropped_here(name)
	# Items share a quarter-second cooldown, and the drop may show up sooner than that, so press again until it
	# goes, as a player would.
	var press := func():
		if Time.get_ticks_msec() % 300 < 20:
			net.pickup()
		return not net.ground().any(func(g): return g["id"] == mine_id)
	net.pickup()
	ok = await _wait(5.0, press)
	_check("pickup takes it off the ground", ok, "gem %d" % mine_id)
	ok = await _wait(5.0, func(): return net.bag().any(func(s): return s["item"] == name))
	_check("and puts it back in the bag", ok)
	print("SMOKE ok")
	get_tree().quit(0)

## The nearest pile of `item` within pickup reach (3 units), or 0.
func _dropped_here(item: String) -> int:
	var best := 0
	var best_d := 3.0
	for g in net.ground():
		var d := (g["position"] as Vector2).distance_to(me_pos)
		if g["item"] == item and d <= best_d:
			best_d = d
			best = g["id"]
	return best

func _slot_of(name: String) -> int:
	for s in net.bag():
		if s["item"] == name:
			return s["slot"]
	return 0

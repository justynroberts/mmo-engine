extends Node3D
## The client: sign in as a guest, walk the world, target things, loot, chat. It knows nothing about how
## anything looks: the world is data (the server's world file, WORLD.md, read by scripts/world_data.gd) and a
## theme (res://themes/<name>/theme.gd, the contract is scripts/theme.gd) draws it. --theme NAME picks one;
## the default is fantasy when it is there, else plain.
##
## Run:  godot --path godot -- --host 127.0.0.1 --udp 7777 --web 127.0.0.1:8081
## Test: add --smoke --admin 127.0.0.1:8080 to run a scripted session headless and exit 0 on success.
## Class: --class knight (default), barbarian, mage or rogue.
## Screenshots: --zoom N sets the camera distance, --walk X,Y (or near) steers to that world point (or beside the nearest building) first, --shot FILE --shot_after SECONDS saves a frame and quits.

const HudScript := preload("res://scripts/hud.gd")
const WorldData := preload("res://scripts/world_data.gd")
var theme: Node3D        # the look: see scripts/theme.gd
var world: RefCounted    # the world as data: see scripts/world_data.gd

var args := {}
var info := {}
var info_text := ""
var net: Object
var hud: CanvasLayer
var cam := Camera3D.new()
var yaw := 0.0
var zoom := 17.0
var ents := {}      # id -> Node3D
var gems := {}      # ground id -> Node3D
const MapScript := preload("res://scripts/map.gd")
var world_map: Control
var weather := {}         # what /play/info says now: {name, color, intensity}, empty for clear sky
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
const MATERIAL_KEYS := [KEY_1, KEY_2, KEY_3, KEY_4, KEY_5, KEY_6, KEY_7, KEY_8, KEY_9, KEY_0, KEY_MINUS, KEY_EQUAL]
var build := {}           # /play/info "build"; empty if the server does not offer it
var building := false     # build mode (B)
var build_kind := 0
var blocks := {}          # column key (cy << 15 | cx) -> {"b": base layer, "k": "..01" (a layer each), "o": owners}
var block_owners := []    # owner tags, as /play/blocks numbers them (1 = the first)
var my_tag := ""          # the tag our own blocks carry
var my_layer := GROUND    # the layer our feet are in
var blocks_ver := 0
var blocks_poll := 0.0
var blocks_busy := false
var ghost := MeshInstance3D.new()       # where the next block would go
var break_mark := MeshInstance3D.new()  # the block a right click would take out
var aim_place := Vector3i(-1, -1, 0)    # cell x, cell y, layer; x < 0 for none
var aim_break := Vector3i(-1, -1, 0)

func _ready() -> void:
	args = _parse_args()
	smoke = args.has("smoke")
	if args.has("fullscreen"):
		DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_FULLSCREEN)
	if args.has("zoom"):
		zoom = clampf(float(args["zoom"]), 8.0, 60.0)
	_build_scene()
	hud = HudScript.new()
	add_child(hud)
	world_map = MapScript.new()
	world_map.visible = false
	hud.add_child(world_map)
	hud.slot_pressed.connect(func(i: int): net.use_slot(i))
	hud.material_pressed.connect(func(i: int): build_kind = i)
	hud.chat_submitted.connect(_on_chat)
	if smoke:
		hud.status.text = "smoke test"
	_start()

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

func _fail(msg: String) -> void:
	push_error(msg)
	hud.status.text = msg
	if smoke:
		print("SMOKE FAIL: ", msg)
		get_tree().quit(1)
	elif args.has("shot"):
		get_tree().quit(1) # a screenshot run would otherwise wait forever

func _start() -> void:
	info_text = await _fetch("http://%s/play/info" % args["web"])
	if info_text == "":
		return _fail("could not reach http://%s/play/info" % args["web"])
	info = JSON.parse_string(info_text)
	var w: Dictionary = info.get("walls", {})
	if not w.get("inline", true):
		# A big world streams its walls. This slice fetches them all once, which is fine to a few thousand boxes.
		var all_text := await _fetch("http://%s/play/walls?all=1" % args["web"])
		var all: Array = JSON.parse_string(all_text).get("chunks", {}).get("all", [])
		var parts: PackedStringArray = []
		for b in all:
			parts.append("[%s,%s,%s,%s]" % [String.num(b[1]), String.num(b[2]), String.num(b[3]), String.num(b[4])])
		# The text is passed on untouched except for the boxes, because whole numbers must stay integers for the SDK.
		info_text = info_text.replace("\"boxes\":[]", "\"boxes\":[" + ",".join(parts) + "]")
		info = JSON.parse_string(info_text)
	boxes = info.get("boxes", [])
	# The world: the server's world file if it has one, else just its walls on flat ground.
	if info.get("world") is Dictionary:
		var text := await _fetch("http://%s/play/world" % args["web"])
		world = WorldData.from_file(text) if text != "" else null
		if world == null:
			return _fail("could not read the world file at http://%s/play/world" % args["web"])
	else:
		world = WorldData.from_boxes(float(info.get("world_size", 1024)), boxes)
	var theme_name := String(args.get("theme", "fantasy" if ResourceLoader.exists("res://themes/fantasy/theme.gd") else "plain"))
	if not ResourceLoader.exists("res://themes/%s/theme.gd" % theme_name):
		return _fail("no theme called '%s' (looked for res://themes/%s/theme.gd)" % [theme_name, theme_name])
	theme = load("res://themes/%s/theme.gd" % theme_name).new()
	add_child(theme)
	theme.setup(info, world)
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
		hud.say("[color=#9fe8e0]#%d%s[/color] %s" % [from, "!" if channel == 1 else "", text]))
	var id := _identity()
	var err: String = net.connect_to(String(args["host"]), int(args["udp"]), info_text, id)
	if err != "":
		return _fail(err)
	my_tag = net.account_tag()
	if smoke:
		_smoke()
	if args.has("map"):
		_toggle_map.call_deferred()
	if args.has("shot"):
		await get_tree().create_timer(float(args.get("shot_after", "5"))).timeout
		get_viewport().get_texture().get_image().save_png(String(args["shot"]))
		get_tree().quit(0)

func _identity() -> String:
	var path := "user://identity.txt"
	if smoke:
		return net.new_identity()
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
	tag.text = "#%d" % id
	tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	tag.pixel_size = 0.01
	tag.font_size = 30
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

func _process(delta: float) -> void:
	if net == null:
		return
	t_total += delta
	var status: int = net.get_status()
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
		if mine:
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
	_poll_blocks(delta)
	_aim()
	hud.set_bag(net.bag())
	_camera()
	_weather(delta)
	if world_map.visible:
		var others := []
		for e in list:
			if e["id"] != self_id:
				others.append(e["position"])
		world_map.update_markers(me_pos, yaw, others)
	_drive()
	stream_t += delta
	if stream_t > 0.4 and me_pos.distance_to(last_stream) > 20.0:
		stream_t = 0.0
		last_stream = me_pos
		theme.update(me_pos)
	if theme.has_method("frame"):
		theme.frame(me_pos, zoom)
	target_ring.visible = target_id != 0 and ents.has(target_id)
	if target_ring.visible:
		target_ring.position = (ents[target_id] as Node3D).position + Vector3(0, 0.06, 0)
	var tgt := ""
	if target_id != 0 and ents.has(target_id):
		tgt = "target #%d" % target_id
		for e in list:
			if e["id"] == target_id and health_idx != "":
				tgt += "   %s %d" % [health_idx, int(e.get(health_idx, 0))]
	else:
		target_id = 0
	if t_total < notice_until:
		tgt = notice
	hud.target.text = tgt
	if Input.is_key_pressed(KEY_SPACE) and not hud.typing() and not smoke:
		_attack() # holding Space keeps swinging, at the action's cooldown
	var names := ["connecting", "connected", "refused", "closed"]
	# The server sends only the nearest max_visible within interest_radius; say so when that is what you are seeing.
	var cap := int(info.get("max_visible", 0))
	var crowd := "   nearest %d shown, more around" % cap if cap > 0 and list.size() >= cap else ""
	var sky := "   %s" % String(weather["name"]).to_upper() if not weather.is_empty() else ""
	var mode := "   BUILD %s" % theme.block_name(build_kind).to_upper() if building else ""
	hud.show_materials(building, build_kind)
	hud.status.text = "%s   %d ms   %d, %d   %d in view%s%s%s%s" % [names[clampi(status, 0, 3)], int(net.get_rtt_ms()), me_pos.x, me_pos.y, list.size(), crowd, sky, "   GOD" if god else "", mode]

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
		changed[k] = true
		if kinds == "":
			blocks.erase(k)
		else:
			blocks[k] = {"b": int(c[2]), "k": kinds, "o": c[4]}
		net.set_column(cx, cy, int(c[2]), _mask(kinds)) # prediction climbs, falls and walks under as the server does
	blocks_ver = int(d.get("ver", blocks_ver))
	if not changed.is_empty():
		theme.set_blocks(blocks, changed.keys())

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
	var mp := get_viewport().get_mouse_position()
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
	blocks_poll = minf(blocks_poll, 0.12)

## The input's argument for a build or break: the layer in the low byte, the material in the high one.
func _build_arg(layer: int, kind: int) -> int:
	return (layer & 0xff) | kind << 8

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
		if not o.get("solid", false):
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

func _apply_weather(w: Dictionary) -> void:
	if w == weather:
		return
	weather = w
	theme.apply_weather(w)
	if w.is_empty():
		hud.say("[color=#9fe8e0]The sky clears.[/color]")
	else:
		hud.say("[color=#9fe8e0]Weather: %s.[/color]" % str(w.get("name", "")))

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
		return m.albedo_texture
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
	var c := Vector3(me_pos.x, (me.position.y if me else _height(me_pos.x, me_pos.y)) + 1.0, me_pos.y)
	var pitch := deg_to_rad(42.0)
	cam.position = c + Vector3(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch)) * zoom
	cam.look_at(c)

func _drive() -> void:
	if smoke:
		return # the scripted session steers
	if hud.typing():
		net.set_move(Vector2.ZERO)
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

func _unhandled_input(ev: InputEvent) -> void:
	if net == null:
		return
	if ev is InputEventMouseButton and ev.pressed:
		if ev.button_index == MOUSE_BUTTON_WHEEL_UP:
			zoom = clampf(zoom - 2.0, 8.0, 60.0)
		elif ev.button_index == MOUSE_BUTTON_WHEEL_DOWN:
			zoom = clampf(zoom + 2.0, 8.0, 60.0)
		elif ev.button_index == MOUSE_BUTTON_LEFT and building:
			_build(true)
		elif ev.button_index == MOUSE_BUTTON_RIGHT and building:
			_build(false)
		elif ev.button_index == MOUSE_BUTTON_LEFT:
			_pick(ev.position)
	elif ev is InputEventKey and ev.pressed and not ev.echo:
		var k: int = ev.keycode
		if k == KEY_ENTER or k == KEY_KP_ENTER:
			hud.chat_in.grab_focus()
		elif k == KEY_TAB:
			_cycle_target()
		elif k == KEY_SPACE:
			_attack()
		elif k == KEY_G:
			_toggle_god()
		elif k == KEY_J:
			net.act(ACT_JUMP, 0)
		elif k == KEY_M:
			_toggle_map()
		elif k == KEY_B and not build.is_empty():
			building = not building
		elif k == KEY_ESCAPE and building:
			building = false
		elif building and MATERIAL_KEYS.has(k) and MATERIAL_KEYS.find(k) < int(build.get("kinds", 4)):
			build_kind = MATERIAL_KEYS.find(k)
		elif k == KEY_ESCAPE and world_map.visible:
			world_map.visible = false
		elif k == KEY_F11:
			var full := DisplayServer.window_get_mode() == DisplayServer.WINDOW_MODE_FULLSCREEN
			DisplayServer.window_set_mode(DisplayServer.WINDOW_MODE_WINDOWED if full else DisplayServer.WINDOW_MODE_FULLSCREEN)
		elif k == KEY_F:
			net.pickup()
		elif k >= KEY_1 and k <= KEY_8:
			if ev.shift_pressed:
				net.drop_slot(k - KEY_1)
			else:
				net.use_slot(k - KEY_1)

func _on_chat(text: String) -> void:
	var shout := text.begins_with("!")
	net.say(text.trim_prefix("!").strip_edges(), shout)

## Attack the target if it is in reach, otherwise whatever is nearest within reach (and target that). The server
## checks range, cooldown and energy and ignores a request that fails, so the checks it can see are made here
## first and explained, instead of a swing that silently does nothing.
## The world map (M): the theme paints the picture once; you and what is around you are drawn on top.
func _toggle_map() -> void:
	if not world_map.visible and not world_map.built:
		world_map.build(theme.map_image(640), world.size)
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
	var me: Node3D = ents.get(net.get_self_id())
	if me and me.has_method("attack"):
		_face(me, (ents[target_id] as Node3D).position)
		me.attack()

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
	var t := -from.y / dir.y
	var hit := from + dir * t
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
	return res[1]

func _check(name: String, ok: bool, extra := "") -> void:
	smoke_log.append("%s %s %s" % ["PASS" if ok else "FAIL", name, extra])
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
	_check("world loaded and themed", world != null and theme != null, "%d objects" % world.objects.size())
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
		for off in [Vector2(4, 0), Vector2(-4, 0), Vector2(0, 4), Vector2(0, -4)]:
			var at: Vector2 = me_pos + off
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
			_check("climbs onto it", ok, "layer %d (block in %d) at %.1f, %.1f" % [my_layer, base, me_pos.x, me_pos.y])
			var drawn: Node3D = ents.get(net.get_self_id())
			await get_tree().create_timer(0.5).timeout
			var block_top := (base + 1) * bs
			_check("and is drawn on top", drawn != null and drawn.position.y > block_top - 0.2, "y %.2f, block top %.2f" % [drawn.position.y if drawn else -1.0, block_top])
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
		await _post(admin_url + "/api/weather", {"name": null})
		weather_poll = 0.0
		ok = await _wait(10.0, func(): return weather.is_empty())
		_check("and clears it", ok, str(weather))
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

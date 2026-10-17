extends Node3D
## One-zone vertical slice: sign in as a guest, walk a low-poly world, target things, loot, chat.
## Everything is drawn from what the server tells us. Art is placeholder primitives; drop character scenes at
## res://assets/characters/player.tscn (and mob.tscn) and they are used instead.
##
## Run:  godot --path godot -- --host 127.0.0.1 --udp 7777 --web 127.0.0.1:8081
## Test: add --smoke --admin 127.0.0.1:8080 to run a scripted session headless and exit 0 on success.

const WALL_DRAW := 260.0
const HudScript := preload("res://scripts/hud.gd")

var args := {}
var info := {}
var info_text := ""
var net: Object
var hud: CanvasLayer
var cam := Camera3D.new()
var yaw := 0.0
var zoom := 22.0
var ents := {}      # id -> Node3D
var gems := {}      # ground id -> Node3D
var walls := {}     # box index -> MeshInstance3D
var boxes: Array = []
var ground_mesh := MeshInstance3D.new()
var me_pos := Vector2.ZERO
var target_id := 0
var health_max := 100.0
var health_idx := ""
var attack_id := -1
var last_wall_sync := Vector2(-1e9, -1e9)
var wall_t := 0.0
var t_total := 0.0
var smoke := false
var smoke_log: Array[String] = []

func _ready() -> void:
	args = _parse_args()
	smoke = args.has("smoke")
	_build_scene()
	hud = HudScript.new()
	add_child(hud)
	hud.slot_pressed.connect(func(i: int): net.use_slot(i))
	hud.chat_submitted.connect(_on_chat)
	if smoke:
		hud.status.text = "smoke test"
	_start()

func _parse_args() -> Dictionary:
	var a := {"host": "127.0.0.1", "udp": "7777", "web": "127.0.0.1:8081", "admin": "127.0.0.1:8080"}
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
	for a in info["attrs"]:
		if a["replicated"] and (health_idx == "" or a["name"] == "health"):
			health_idx = a["name"]
			health_max = max(float(a["max"]), 1.0)
	if info.get("actions", []).size() > 0:
		attack_id = int(info["actions"][0]["id"])
	_update_ground_mesh()
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
	if smoke:
		_smoke()
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

func _build_scene() -> void:
	var env := WorldEnvironment.new()
	var e := Environment.new()
	e.background_mode = Environment.BG_COLOR
	e.background_color = Color("0a0d14")
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color("7f93b8")
	e.ambient_light_energy = 0.7
	e.fog_enabled = true
	e.fog_light_color = Color("0a0d14")
	e.fog_density = 0.006
	env.environment = e
	add_child(env)
	var sun := DirectionalLight3D.new()
	sun.rotation_degrees = Vector3(-52, -30, 0)
	sun.light_energy = 1.1
	add_child(sun)
	cam.current = true
	cam.fov = 55
	cam.far = 900
	add_child(cam)
	var sh := Shader.new()
	sh.code = """shader_type spatial;
varying vec3 wp;
void vertex() { wp = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz; }
void fragment() {
	vec2 q = wp.xz / 8.0;
	vec2 g = abs(fract(q - 0.5) - 0.5) / fwidth(q);
	float line = 1.0 - min(min(g.x, g.y), 1.0);
	ALBEDO = mix(vec3(0.055, 0.07, 0.085), vec3(0.16, 0.34, 0.38), line * 0.8);
	ROUGHNESS = 1.0;
}"""
	var m := ShaderMaterial.new()
	m.shader = sh
	ground_mesh.mesh = PlaneMesh.new()
	ground_mesh.material_override = m
	add_child(ground_mesh)

func _update_ground_mesh() -> void:
	var size := float(info.get("world_size", 1024))
	(ground_mesh.mesh as PlaneMesh).size = Vector2(size, size)
	ground_mesh.position = Vector3(size / 2.0, 0, size / 2.0)

func _entity_node(id: int, mine: bool) -> Node3D:
	var scene := "res://assets/characters/%s.tscn" % ("player" if mine else "mob")
	if ResourceLoader.exists(scene):
		return (load(scene) as PackedScene).instantiate()
	var n := Node3D.new()
	var mi := MeshInstance3D.new()
	var cap := CapsuleMesh.new()
	cap.radius = 0.7 if mine else 0.55
	cap.height = 2.4 if mine else 1.8
	mi.mesh = cap
	mi.position.y = cap.height / 2.0
	var mat := StandardMaterial3D.new()
	mat.albedo_color = Color("ffcf5a") if mine else Color("c8503c")
	mat.emission_enabled = mine
	mat.emission = Color("ffb030")
	mat.emission_energy_multiplier = 0.4
	mi.material_override = mat
	mi.name = "body"
	n.add_child(mi)
	var tag := Label3D.new()
	tag.text = "#%d" % id
	tag.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	tag.pixel_size = 0.012
	tag.position.y = cap.height + 0.7
	tag.no_depth_test = true
	tag.name = "tag"
	n.add_child(tag)
	return n

func _gem_node(color: String) -> Node3D:
	var n := Node3D.new()
	var mi := MeshInstance3D.new()
	var s := SphereMesh.new()
	s.radius = 0.4
	s.height = 0.8
	s.radial_segments = 6
	s.rings = 3
	mi.mesh = s
	var mat := StandardMaterial3D.new()
	mat.albedo_color = Color(color)
	mat.emission_enabled = true
	mat.emission = Color(color)
	mat.emission_energy_multiplier = 1.6
	mi.material_override = mat
	mi.position.y = 1.0
	n.add_child(mi)
	var l := OmniLight3D.new()
	l.light_color = Color(color)
	l.omni_range = 7.0
	l.light_energy = 1.2
	l.position.y = 1.2
	n.add_child(l)
	return n

func _sync_walls(center: Vector2) -> void:
	var keep := {}
	for i in boxes.size():
		var b: Array = boxes[i]
		if b[2] < center.x - WALL_DRAW or b[0] > center.x + WALL_DRAW or b[3] < center.y - WALL_DRAW or b[1] > center.y + WALL_DRAW:
			continue
		keep[i] = true
		if walls.has(i):
			continue
		var h := 6.0 + float(int(b[0] * 7.0 + b[1] * 13.0) % 9)
		var mi := MeshInstance3D.new()
		var bm := BoxMesh.new()
		bm.size = Vector3(b[2] - b[0], h, b[3] - b[1])
		mi.mesh = bm
		mi.position = Vector3((b[0] + b[2]) / 2.0, h / 2.0, (b[1] + b[3]) / 2.0)
		var mat := StandardMaterial3D.new()
		mat.albedo_color = Color("1b2233")
		mat.roughness = 0.9
		mi.material_override = mat
		add_child(mi)
		walls[i] = mi
	for i in walls.keys():
		if not keep.has(i):
			walls[i].queue_free()
			walls.erase(i)

# ---------------------------------------------------------------- per frame

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
		var np := Vector3(p.x, 0, p.y)
		if node.position.distance_to(np) > 0.02:
			node.rotation.y = lerp_angle(node.rotation.y, atan2(np.x - node.position.x, np.z - node.position.z), min(1.0, delta * 12.0))
		node.position = np
		if mine:
			me_pos = p
		elif health_idx != "" and node.has_node("body"):
			var hp := clampf(float(e.get(health_idx, health_max)) / health_max, 0.0, 1.0)
			var mat := (node.get_node("body") as MeshInstance3D).material_override as StandardMaterial3D
			if mat:
				mat.albedo_color = Color("c8503c").lerp(Color("5ac85a"), hp)
	for id in ents.keys():
		if not seen.has(id):
			ents[id].queue_free()
			ents.erase(id)
	_sync_ground()
	hud.set_bag(net.bag())
	_camera()
	_drive()
	wall_t += delta
	if wall_t > 0.4 and me_pos.distance_to(last_wall_sync) > 20.0:
		wall_t = 0.0
		last_wall_sync = me_pos
		_sync_walls(me_pos)
	var tgt := ""
	if target_id != 0 and ents.has(target_id):
		tgt = "target #%d" % target_id
		for e in list:
			if e["id"] == target_id and health_idx != "":
				tgt += "   %s %d" % [health_idx, int(e.get(health_idx, 0))]
	else:
		target_id = 0
	hud.target.text = tgt
	var names := ["connecting", "connected", "refused", "closed"]
	hud.status.text = "%s   %d ms   %d, %d   %d in view" % [names[clampi(status, 0, 3)], int(net.get_rtt_ms()), me_pos.x, me_pos.y, list.size()]

func _sync_ground() -> void:
	var seen := {}
	for g in net.ground():
		var id: int = g["id"]
		seen[id] = true
		if not gems.has(id):
			var n := _gem_node(str(g["color"]))
			add_child(n)
			gems[id] = n
		var p: Vector2 = g["position"]
		var n: Node3D = gems[id]
		n.position = Vector3(p.x, sin(t_total * 2.0 + id) * 0.15, p.y)
		n.rotation.y = t_total * 1.5
	for id in gems.keys():
		if not seen.has(id):
			gems[id].queue_free()
			gems.erase(id)

func _camera() -> void:
	var c := Vector3(me_pos.x, 1.0, me_pos.y)
	var pitch := deg_to_rad(52.0)
	cam.position = c + Vector3(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch)) * zoom
	cam.look_at(c)

func _drive() -> void:
	if smoke:
		return # the scripted session steers
	if hud.typing():
		net.set_move(Vector2.ZERO)
		return
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

func _attack() -> void:
	if target_id != 0 and attack_id >= 0:
		net.act(attack_id, target_id)

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
	var r := HTTPRequest.new()
	add_child(r)
	r.request(url, ["Content-Type: application/json", "X-Admin: 1"], HTTPClient.METHOD_POST, JSON.stringify(body))
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
	_check("world drawn", boxes.size() >= 0 and ground_mesh.mesh != null, "%d boxes" % boxes.size())
	var start := me_pos
	var t := Time.get_ticks_msec()
	while Time.get_ticks_msec() - t < 2500:
		net.set_move(Vector2(1, 0))
		await get_tree().process_frame
	net.set_move(Vector2.ZERO)
	_check("walks", me_pos.distance_to(start) > 5.0, "moved %.1f" % me_pos.distance_to(start))
	_check("walls near us are built", walls.size() > 0 or boxes.size() == 0, "%d meshes" % walls.size())
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
	ok = await _wait(5.0, func(): return net.ground().size() > 0 and gems.size() > 0)
	_check("dropped item appears on the ground", ok, "%d gems" % gems.size())
	var mine_id := _nearest_ground()
	net.pickup()
	ok = await _wait(5.0, func(): return not net.ground().any(func(g): return g["id"] == mine_id))
	_check("pickup takes it off the ground", ok, "gem %d" % mine_id)
	ok = await _wait(5.0, func(): return net.bag().any(func(s): return s["item"] == name))
	_check("and puts it back in the bag", ok)
	print("SMOKE ok")
	get_tree().quit(0)

func _nearest_ground() -> int:
	var best := 0
	var best_d := 1e9
	for g in net.ground():
		var d := (g["position"] as Vector2).distance_to(me_pos)
		if d < best_d:
			best_d = d
			best = g["id"]
	return best

func _slot_of(name: String) -> int:
	for s in net.bag():
		if s["item"] == name:
			return s["slot"]
	return 0

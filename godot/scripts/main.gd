extends Node3D
## One-zone vertical slice: sign in as a guest, walk a low-poly world, target things, loot, chat.
## Everything is drawn from what the server tells us. Art is placeholder primitives; drop character scenes at
## res://assets/characters/player.tscn (and mob.tscn) and they are used instead.
##
## Run:  godot --path godot -- --host 127.0.0.1 --udp 7777 --web 127.0.0.1:8081
## Test: add --smoke --admin 127.0.0.1:8080 to run a scripted session headless and exit 0 on success.
## Class: --class knight (default), barbarian, mage or rogue.
## Screenshots: --zoom N sets the camera distance, --walk X,Y (or near) steers to that world point (or beside the nearest building) first, --shot FILE --shot_after SECONDS saves a frame and quits.

const WALL_DRAW := 260.0
const PROP_DRAW := 230.0
const PROP_CHUNK := 128.0
const TERRAIN_RINGS := 3     # terrain chunks drawn this many chunks out from ours
const TERRAIN_STEP := 4.0    # terrain grid spacing
const FLAT_PAD := 10.0       # ground levels out this far around a building
const EDGE_CHUNKS := 2       # scenery chunks drawn past the world's edge
const HudScript := preload("res://scripts/hud.gd")
const DUNGEON := "res://assets/dungeon/"
const VILLAGE := "res://assets/village/"
const VS := 1.4 # Quaternius builds in metres; this brings the village up to the chibi characters' size
## Roofs by footprint [width along x, length along z], ridge along z, in kit units.
const ROOFS := [[4, 4], [4, 6], [4, 8], [6, 4], [6, 6], [6, 8], [6, 10], [6, 12], [6, 14], [8, 8], [8, 10], [8, 12], [8, 14]]
const FOREST := "res://assets/forest/"
## KayKit rocks for crags and mountains: [file, width, height, how far below its origin it reaches] at scale 1.
const SPIRES := [["Rock_1_O_Color1", 2.11, 4.58, 0.0], ["Rock_1_N_Color1", 2.3, 4.5, 0.0], ["Rock_1_P_Color1", 2.21, 4.01, 0.0], ["Rock_1_Q_Color1", 2.8, 4.28, 0.0]]
const BOULDERS := [["Rock_1_K_Color1", 3.69, 3.65, 0.0], ["Rock_1_M_Color1", 3.91, 3.52, 0.0], ["Rock_1_L_Color1", 3.29, 3.4, 0.0],
	["Rock_2_G_Color1", 3.73, 3.2, 0.0], ["Rock_2_E_Color1", 3.31, 2.86, 0.0], ["Rock_3_Q_Color1", 4.65, 3.72, -1.62],
	["Rock_3_R_Color1", 5.16, 3.85, -1.71], ["Rock_3_M_Color1", 3.43, 3.6, -1.64], ["Rock_2_F_Color1", 4.14, 2.86, 0.0],
	["Rock_2_H_Color1", 3.7, 2.76, 0.0], ["Rock_3_N_Color1", 3.42, 3.53, -1.59], ["Rock_3_O_Color1", 3.48, 3.47, -1.43],
	["Rock_3_P_Color1", 3.29, 3.68, -1.74]]
const ROCK_TINT := Color(0.72, 0.69, 0.64)
## Every tree, bush and rock in the KayKit Forest pack (the big rocks are kept for crags and mountains).
const TREES := ["Tree_1_A_Color1", "Tree_1_B_Color1", "Tree_1_C_Color1", "Tree_2_A_Color1", "Tree_2_B_Color1", "Tree_2_C_Color1", "Tree_2_D_Color1", "Tree_2_E_Color1", "Tree_3_A_Color1", "Tree_3_B_Color1", "Tree_3_C_Color1", "Tree_4_A_Color1", "Tree_4_B_Color1", "Tree_4_C_Color1"]
const BARE_TREES := ["Tree_Bare_1_A_Color1", "Tree_Bare_1_B_Color1", "Tree_Bare_1_C_Color1", "Tree_Bare_2_A_Color1", "Tree_Bare_2_B_Color1", "Tree_Bare_2_C_Color1"]
const BUSHES := ["Bush_1_A_Color1", "Bush_1_B_Color1", "Bush_1_C_Color1", "Bush_1_D_Color1", "Bush_1_E_Color1", "Bush_1_F_Color1", "Bush_1_G_Color1", "Bush_2_A_Color1", "Bush_2_B_Color1", "Bush_2_C_Color1", "Bush_2_D_Color1", "Bush_2_E_Color1", "Bush_2_F_Color1", "Bush_3_A_Color1", "Bush_3_B_Color1", "Bush_3_C_Color1", "Bush_4_A_Color1", "Bush_4_B_Color1", "Bush_4_C_Color1", "Bush_4_D_Color1", "Bush_4_E_Color1", "Bush_4_F_Color1"]
const SMALL_ROCKS := ["Rock_1_A_Color1", "Rock_1_B_Color1", "Rock_1_C_Color1", "Rock_1_D_Color1", "Rock_1_E_Color1", "Rock_1_F_Color1", "Rock_1_G_Color1", "Rock_1_H_Color1", "Rock_1_I_Color1", "Rock_1_J_Color1", "Rock_2_A_Color1", "Rock_2_B_Color1", "Rock_2_C_Color1", "Rock_2_D_Color1", "Rock_3_A_Color1", "Rock_3_B_Color1", "Rock_3_C_Color1", "Rock_3_D_Color1", "Rock_3_E_Color1", "Rock_3_F_Color1", "Rock_3_G_Color1", "Rock_3_H_Color1", "Rock_3_I_Color1", "Rock_3_J_Color1", "Rock_3_K_Color1", "Rock_3_L_Color1"]
## Scattered scenery per 128 unit chunk: [kind, how many, smallest scale, largest scale]. Each one picks a model of
## its kind at random, so no two stretches of meadow look alike.
const PROPS := [["trees", 14, 1.3, 1.9], ["bare", 1, 1.6, 2.1], ["rocks", 12, 1.2, 2.2], ["bushes", 22, 2.2, 4.0]]

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
var walls := {}     # box index -> Node3D
var props := {}     # chunk key -> Node3D
var terrain := {}   # chunk key -> MeshInstance3D
var terrain_mat: ShaderMaterial
const TerrainScript := preload("res://scripts/terrain.gd")
const MapScript := preload("res://scripts/map.gd")
var world_map: Control
var land: TerrainScript = TerrainScript.new() # hills, rivers, lakes and crags, from the seed and the walls
var water_mat: ShaderMaterial
var env_res: Environment
var sun_light: DirectionalLight3D
var rain := GPUParticles3D.new()
var weather := {}         # what /play/info says now: {name, color, intensity}, empty for clear sky
var weather_poll := 0.0
var clear_sky := {}       # the environment's own values, to return to when the weather clears
var boxes: Array = []
var box_home := {}  # chunk key -> boxes whose centre is in it (built together)
var box_grid := {}  # chunk key -> box indices overlapping it, to keep scenery out of buildings
var meshes := {}    # path -> Mesh, pulled out of the glTF pieces once
var track := {}     # entity id -> {hp, pos, vel, face_until}
var corpses: Array = [] # [node, seconds left]
var entity_scale := 1.0
const ACT_JUMP := 251
const JUMP_SECS := 0.6
const JUMP_HEIGHT := 2.2
var air_max := 0.0 # max of the server's "airborne" attribute; 0 when the game has none
var action_range := 8.0
var ground_mesh := MeshInstance3D.new()
var target_ring := MeshInstance3D.new()
var me_pos := Vector2.ZERO
var target_id := 0
var health_max := 100.0
var health_idx := ""
var attack_id := -1
var last_wall_sync := Vector2(-1e9, -1e9)
var wall_t := 0.0
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
	_index_boxes()
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

func _build_scene() -> void:
	var env := WorldEnvironment.new()
	var e := Environment.new()
	var sky_mat := ProceduralSkyMaterial.new()
	sky_mat.sky_top_color = Color("4d79b3")
	sky_mat.sky_horizon_color = Color("c9d8e0")
	sky_mat.ground_horizon_color = Color("c9d8e0")
	sky_mat.ground_bottom_color = Color("3d4a33")
	sky_mat.sun_angle_max = 20.0
	var sky := Sky.new()
	sky.sky_material = sky_mat
	e.background_mode = Environment.BG_SKY
	e.sky = sky
	e.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	e.ambient_light_color = Color("9aa7b8")
	e.ambient_light_energy = 0.38
	e.tonemap_mode = Environment.TONE_MAPPER_FILMIC
	e.tonemap_exposure = 0.95
	e.fog_enabled = true
	e.fog_light_color = Color("a9bfc9")
	e.fog_density = 0.0018
	e.fog_sky_affect = 0.0
	# Forward+ extras. The Compatibility renderer ignores what it cannot do, so these are safe either way.
	e.glow_enabled = true # bloom on torches, loot beams and anything brighter than white
	e.glow_intensity = 0.7
	e.glow_bloom = 0.04
	e.glow_hdr_threshold = 1.1
	e.glow_blend_mode = Environment.GLOW_BLEND_MODE_SOFTLIGHT
	e.ssao_enabled = true # contact shadow where walls, trees and feet meet the ground
	e.ssao_radius = 1.6
	e.ssao_intensity = 1.8
	e.volumetric_fog_enabled = true # a light haze that the sun shafts through
	e.volumetric_fog_density = 0.0035
	e.volumetric_fog_albedo = Color("dfe8ee")
	e.volumetric_fog_anisotropy = 0.35
	e.volumetric_fog_length = 140.0
	env.environment = e
	add_child(env)
	env_res = e
	var sun := DirectionalLight3D.new()
	sun_light = sun
	sun.rotation_degrees = Vector3(-48, -35, 0)
	sun.light_color = Color("fff1d6")
	sun.light_energy = 0.95
	sun.shadow_enabled = true
	sun.light_volumetric_fog_energy = 1.4
	sun.shadow_blur = 1.5
	sun.directional_shadow_max_distance = 90.0
	add_child(sun)
	clear_sky = {"fog": e.fog_density, "fog_color": e.fog_light_color, "vol": e.volumetric_fog_density, "vol_albedo": e.volumetric_fog_albedo,
		"sun": sun.light_energy, "ambient": e.ambient_light_energy}
	# Rain (or whatever falls in the current weather): streaks in a box that follows the camera.
	var pm := ParticleProcessMaterial.new()
	pm.emission_shape = ParticleProcessMaterial.EMISSION_SHAPE_BOX
	pm.emission_box_extents = Vector3(45, 1, 45)
	pm.direction = Vector3(0.15, -1, 0.05)
	pm.spread = 3.0
	pm.initial_velocity_min = 38.0
	pm.initial_velocity_max = 48.0
	pm.gravity = Vector3.ZERO
	rain.process_material = pm
	var streak := BoxMesh.new()
	streak.size = Vector3(0.025, 0.9, 0.025)
	var sm := StandardMaterial3D.new()
	sm.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	sm.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	sm.albedo_color = Color(0.8, 0.85, 0.95, 0.45)
	streak.material = sm
	rain.draw_pass_1 = streak
	rain.amount = 6000
	rain.lifetime = 0.45
	rain.visibility_aabb = AABB(Vector3(-50, -25, -50), Vector3(100, 30, 100))
	rain.emitting = false
	rain.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(rain)
	cam.current = true
	cam.fov = 55
	cam.far = 900
	add_child(cam)
	# Meadow: two greens, dry patches and worn dirt, all from world position so every client sees the same field.
	var sh := Shader.new()
	sh.code = """shader_type spatial;
render_mode specular_disabled;
uniform float rocky = 0.0; // 1 on crags: rock except in the flattest hollows
varying vec3 wp;
varying vec3 wn;
float h(vec2 p) { vec3 q = fract(vec3(p.xyx) * 0.1031); q += dot(q, q.yzx + 33.33); return fract((q.x + q.y) * q.z); }
float n(vec2 p) {
	vec2 i = floor(p); vec2 f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(h(i), h(i + vec2(1, 0)), u.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) { float v = 0.0; float a = 0.5; for (int k = 0; k < 4; k++) { v += a * n(p); p *= 2.03; a *= 0.5; } return v; }
void vertex() { wp = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz; wn = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz); }
void fragment() {
	vec2 q = wp.xz;
	vec3 c = mix(vec3(0.36, 0.56, 0.20), vec3(0.50, 0.66, 0.25), smoothstep(0.3, 0.7, fbm(q / 14.0)));
	c = mix(c, vec3(0.46, 0.43, 0.24), smoothstep(0.55, 0.75, fbm(q / 90.0)) * 0.5);
	c = mix(c, vec3(0.44, 0.36, 0.23), smoothstep(0.62, 0.78, fbm(q / 18.0 + 7.0)) * 0.45);
	c *= 0.94 + 0.12 * fbm(q * 0.6);
	// Steep ground is bare rock, high ground carries snow, and anything below a river's banks is wet silt.
	float slope = 1.0 - clamp(wn.y, 0.0, 1.0);
	vec3 rock = vec3(0.47, 0.45, 0.42) * (0.8 + 0.4 * fbm(q * 0.25 + wp.y * 0.1));
	c = mix(c, rock, max(smoothstep(0.28, 0.5, slope), rocky * smoothstep(0.04, 0.12, slope + 0.05 * n(q * 0.3))));
	c = mix(c, vec3(0.93, 0.95, 0.98), smoothstep(30.0, 42.0, wp.y + 6.0 * n(q * 0.05)) * (1.0 - smoothstep(0.55, 0.8, slope)));
	ALBEDO = pow(c, vec3(2.2)); // the colours above are sRGB; albedo is linear
	ROUGHNESS = 1.0;
}"""
	var m := ShaderMaterial.new()
	m.shader = sh
	terrain_mat = m
	# A flat sheet well below the hills fills the distance past the streamed terrain, inside the fog.
	ground_mesh.mesh = PlaneMesh.new()
	ground_mesh.material_override = m
	add_child(ground_mesh)
	var ring := TorusMesh.new()
	ring.inner_radius = 0.95 * entity_scale
	ring.outer_radius = 1.12 * entity_scale
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

func _update_ground_mesh() -> void:
	var size := float(info.get("world_size", 1024))
	(ground_mesh.mesh as PlaneMesh).size = Vector2(size + 4096.0, size + 4096.0)
	ground_mesh.position = Vector3(size / 2.0, -10.0, size / 2.0)
	land.setup(int(info.get("seed", 1)), size, boxes)
	for r in land.rivers:
		add_child(_river_mesh(r))

func _entity_node(id: int, mine: bool) -> Node3D:
	# You are the class you asked for (--class knight|barbarian|mage|rogue); everyone else is one of four
	# skeletons, chosen from the entity id so every client shows the same one.
	var scene := "res://assets/characters/player.tscn"
	if mine and args.has("class") and ResourceLoader.exists("res://assets/characters/player_%s.tscn" % args["class"]):
		scene = "res://assets/characters/player_%s.tscn" % args["class"]
	elif not mine:
		var kinds := _mob_scenes()
		scene = kinds[id % kinds.size()] if not kinds.is_empty() else "res://assets/characters/mob.tscn"
	if ResourceLoader.exists(scene):
		var c := (load(scene) as PackedScene).instantiate() as Node3D
		c.scale = Vector3.ONE * entity_scale
		_nameplate(c, id, mine)
		return c
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

## The first mesh in a glTF piece, kept so each piece is loaded once and drawn with MultiMesh.
func _mesh(path: String) -> Mesh:
	if meshes.has(path):
		return meshes[path]
	var m: Mesh = null
	if ResourceLoader.exists(path):
		var inst := (load(path) as PackedScene).instantiate()
		var found := inst.find_children("*", "MeshInstance3D", true, false)
		if not found.is_empty():
			m = (found[0] as MeshInstance3D).mesh
		inst.free()
	meshes[path] = m
	return m

func _multi(mesh: Mesh, xforms: Array, tint := Color.WHITE) -> MultiMeshInstance3D:
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = mesh
	mm.instance_count = xforms.size()
	for i in xforms.size():
		mm.set_instance_transform(i, xforms[i])
	var mmi := MultiMeshInstance3D.new()
	mmi.multimesh = mm
	if tint != Color.WHITE and mesh.get_surface_count() > 0 and mesh.surface_get_material(0) is StandardMaterial3D:
		var m := (mesh.surface_get_material(0) as StandardMaterial3D).duplicate() as StandardMaterial3D
		m.albedo_color = tint
		mmi.material_override = m
	return mmi

## Loot is a chest tinted by the item's colour, with a soft beam of the same colour so it can be found in grass.
## Every mob scene that is present: the four skeletons, and the Bestiary monsters when their (local only,
## not redistributable) models are there.
var _mobs: Array = []
func _mob_scenes() -> Array:
	if _mobs.is_empty():
		for p in ["res://assets/characters/mob.tscn", "res://assets/characters/mob_mage.tscn", "res://assets/characters/mob_minion.tscn",
				"res://assets/characters/mob_rogue.tscn", "res://assets/monsters/mob_imp.tscn", "res://assets/monsters/mob_puglin.tscn"]:
			var model: String = String(p).replace("mob_", "").replace(".tscn", ".glb").replace("imp.glb", "Imp.glb").replace("puglin.glb", "Puglin.glb")
			if ResourceLoader.exists(p) and (not p.contains("/monsters/") or ResourceLoader.exists(model)):
				_mobs.append(p)
	return _mobs

func _gem_node(color: String, rare := false) -> Node3D:
	var n := Node3D.new()
	var col := Color(color)
	var chest := _mesh(DUNGEON + ("chest_gold.gltf" if rare else "chest.gltf"))
	if chest:
		var mi := MeshInstance3D.new()
		mi.mesh = chest
		var src := chest.surface_get_material(0) as StandardMaterial3D
		if src:
			var tinted := src.duplicate() as StandardMaterial3D
			tinted.albedo_color = Color.WHITE.lerp(col, 0.55)
			mi.material_override = tinted
		mi.scale = Vector3.ONE * 1.05
		mi.position.y = 0.12
		n.add_child(mi)
	else:
		var s := MeshInstance3D.new()
		s.mesh = SphereMesh.new()
		s.position.y = 1.0
		n.add_child(s)
	var beam := MeshInstance3D.new()
	var cyl := CylinderMesh.new()
	cyl.top_radius = 0.1
	cyl.bottom_radius = 0.4
	cyl.height = 3.0
	cyl.radial_segments = 8
	cyl.cap_top = false
	cyl.cap_bottom = false
	beam.mesh = cyl
	var bm := StandardMaterial3D.new()
	bm.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	bm.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	bm.blend_mode = BaseMaterial3D.BLEND_MODE_ADD
	bm.cull_mode = BaseMaterial3D.CULL_DISABLED
	bm.albedo_color = Color(col.r * 1.6, col.g * 1.6, col.b * 1.6, 0.3) # over 1, so glow picks it up
	beam.material_override = bm
	beam.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	beam.position.y = 1.6
	n.add_child(beam)
	var l := OmniLight3D.new()
	l.light_color = col
	l.omni_range = 5.0
	l.light_energy = 0.9
	l.position.y = 1.4
	n.add_child(l)
	return n

## Which keeps are houses: short side 6 to 26 and an even parity of the corner (scripts/city.py relies on this).
func _is_house(b: Array) -> bool:
	var w: float = b[2] - b[0]
	var d: float = b[3] - b[1]
	return land.box_kind(b) == land.KEEP and minf(w, d) >= 6.0 and minf(w, d) <= 26.0 and (int(b[0]) * 3 + int(b[1]) * 5) % 2 == 0

## A village house from the Medieval Village kit: a stone ground floor (with the door) and half-timbered upper
## floors, corner posts, windows with shutters, a tiled roof with gable ends, chimney and dormers, vines, a
## balcony now and then, and a paved yard with a fence, a wagon and crates out front. The walls fill the box
## exactly, so the house is what the server's wall is.
func _build_house(b: Array, idx: int, batch: Dictionary = {}) -> Node3D:
	var x0: float = b[0]
	var y0: float = b[1]
	var x1: float = b[2]
	var y1: float = b[3]
	var hsh := absi(int(x0) * 92821 + int(y0) * 68917)
	var rng := RandomNumberGenerator.new()
	rng.seed = hsh
	var node := Node3D.new()
	node.position.y = land.base[idx]
	var parts := {}
	var put := func(name: String, xf: Transform3D):
		if not parts.has(name):
			parts[name] = []
		parts[name].append(xf)
	var tall := 3.12 * VS
	var w := x1 - x0
	var d := y1 - y0
	var tower := absf(w - d) < 2.5 and maxf(w, d) <= 14.0
	var floors := 3 if tower else 1 + hsh % 2
	var door_side := rng.randi() % 4
	# Style, all from the corner: a plaster or stone ground floor, round or flat door in a brick or timber frame,
	# and on two-storey houses sometimes a jettied upper floor that overhangs the street.
	var stone := (hsh / 7) % 3 != 0
	var round_door := (hsh / 3) % 2 == 0
	var timber_frame := (hsh / 5) % 2 == 0
	var jetty := floors == 2 and (hsh / 11) % 3 == 0
	var shape := "Round" if round_door else "Flat"
	var door_wall: String = ("Wall_UnevenBrick_Door_%s" % shape) if stone else (["Wall_Plaster_Door_Round", "Wall_Plaster_Door_RoundInset"][(hsh / 13) % 2] if round_door else "Wall_Plaster_Door_Flat")
	var frame := "DoorFrame_%s_%s" % [shape, "WoodDark" if timber_frame else "Brick"]
	var door := "Door_%d_%s" % [[1, 2, 4, 8][hsh % 4], shape]
	var plaster := ["Wall_Plaster_Straight", "Wall_Plaster_WoodGrid", "Wall_Plaster_Straight_L", "Wall_Plaster_Straight_R", "Wall_Plaster_Straight_Base"]
	var shutters := ["WindowShutters_Wide_Round_Open", "WindowShutters_Wide_Round_Closed", "WindowShutters_Wide_Flat_Open", "WindowShutters_Wide_Flat_Closed"]
	var sides := [
		[Vector3(x0, 0, y0), Vector3(1, 0, 0), w, Vector3(0, 0, -1)],
		[Vector3(x1, 0, y1), Vector3(-1, 0, 0), w, Vector3(0, 0, 1)],
		[Vector3(x0, 0, y1), Vector3(0, 0, -1), d, Vector3(-1, 0, 0)],
		[Vector3(x1, 0, y0), Vector3(0, 0, 1), d, Vector3(1, 0, 0)],
	]
	var door_at := Vector3.ZERO
	var door_out := Vector3.ZERO
	for si in sides.size():
		var sd: Array = sides[si]
		var start: Vector3 = sd[0]
		var dir: Vector3 = sd[1]
		var length: float = sd[2]
		var out: Vector3 = sd[3]
		var count := maxi(1, int(round(length / (2.0 * VS))))
		var seg := length / count
		var rot := Basis(Vector3.UP, atan2(out.x, out.z))
		var wall := rot * Basis.from_scale(Vector3(seg / 2.0, VS, VS))
		var plain := rot * Basis.from_scale(Vector3.ONE * VS)
		for i in count:
			var at := start + dir * (seg * (i + 0.5)) - out * (0.09 * VS)
			for f in floors:
				var lift := Vector3(0, tall * f, 0)
				var xf := Transform3D(wall, at + lift)
				if f == 0:
					if si == door_side and i == count / 2:
						put.call(door_wall, xf)
						put.call(frame, Transform3D(plain, at))
						put.call(door, Transform3D(plain, at - (rot * Vector3(0.55 * VS, 0, 0))))
						door_at = at
						door_out = out
					elif (i + si + hsh) % 3 == 1:
						var k := (i + si) % 3
						if stone:
							put.call(["Wall_UnevenBrick_Window_Thin_Round", "Wall_UnevenBrick_Window_Wide_Flat", "Wall_UnevenBrick_Window_Wide_Round"][k], xf)
						else:
							put.call(["Wall_Plaster_Window_Thin_Round", "Wall_Plaster_Window_Wide_Flat", "Wall_Plaster_Window_Wide_Round"][k], xf)
						put.call(["Window_Thin_Round1", "Window_Wide_Flat1", "Window_Wide_Round1"][k], Transform3D(plain, at))
					else:
						put.call("Wall_UnevenBrick_Straight" if stone else plaster[(i + si) % plaster.size()], xf)
					put.call("Wall_BottomCover", Transform3D(wall, at + Vector3(0, 0.05, 0)))
					put.call(["Prop_ExteriorBorder_Straight1", "Prop_ExteriorBorder_Straight2"][(i + si) % 2], Transform3D(wall, at + out * (0.09 * VS)))
				elif jetty:
					# The jetty: the upper floor leans out over the street on overhang pieces, with soffits under.
					var mat := "UnevenBrick" if stone and (hsh / 17) % 2 == 0 else "Plaster"
					var ends := "_L" if i == 0 else ("_R" if i == count - 1 else "")
					if ends != "":
						put.call("Overhang_Side_%s_%s%s" % [mat, "Long" if seg > 2.2 * VS else "Short", ends], xf)
					else:
						put.call("Overhang_%s_%s" % [mat, "Long" if seg > 2.2 * VS else "Short"], xf)
					put.call("Overhang_Roof_%s" % ("UnevenBricks" if mat == "UnevenBrick" else mat), Transform3D(wall, at + lift + out * VS))
				elif (i + f + si + hsh) % 2 == 0:
					var kind := (i + f * 3 + si) % 4
					put.call(["Wall_Plaster_Window_Wide_Round", "Wall_Plaster_Window_Wide_Flat", "Wall_Plaster_Window_Wide_Flat2", "Wall_Plaster_Window_Thin_Round"][kind], xf)
					var win: String = ["Window_Wide_Round1", "Window_Wide_Flat1", "Window_Wide_Flat1", "Window_Thin_Round1"][kind]
					put.call(win, Transform3D(plain, at + lift))
					if kind < 3:
						put.call(shutters[(i + f + hsh) % shutters.size()], Transform3D(plain, at + lift))
					else:
						put.call(["Window_Thin_Flat1", "WindowShutters_Thin_Round_Open", "WindowShutters_Thin_Round_Closed", "WindowShutters_Thin_Flat_Open", "WindowShutters_Thin_Flat_Closed"][(i + hsh) % 5], Transform3D(plain, at + lift))
				else:
					put.call(plaster[(i * 7 + f + si + hsh) % plaster.size()], xf)
				# Vines hang from the top of some walls; a balcony on the odd upper one.
				if f == floors - 1 and (i * 5 + si + hsh) % 7 == 0:
					put.call(["Prop_Vine1", "Prop_Vine2", "Prop_Vine4", "Prop_Vine5", "Prop_Vine6", "Prop_Vine9"][(i + si) % 6], Transform3D(plain, at + lift + Vector3(0, tall, 0) + out * 0.2))
				if f == 1 and (i + si * 2 + hsh) % 9 == 0:
					put.call(["Balcony_Simple_Straight", "Balcony_Cross_Straight"][(i + hsh) % 2], Transform3D(plain, at + lift))
					put.call("Prop_Support", Transform3D(plain, at + lift - Vector3(0, tall * 0.55, 0)))
	# Corner posts, stone below and timber above.
	var corners := [[Vector3(x0, 0, y0), PI], [Vector3(x1, 0, y0), PI / 2.0], [Vector3(x0, 0, y1), -PI / 2.0], [Vector3(x1, 0, y1), 0.0]]
	for cc in corners:
		var c: Vector3 = cc[0]
		var face := Basis(Vector3.UP, float(cc[1])) * Basis.from_scale(Vector3.ONE * VS)
		put.call("Prop_ExteriorBorder_Corner", Transform3D(face, c))
		for f in floors:
			if jetty and f == 1:
				put.call(["Overhang_Plaster_Corner", "Overhang_Plaster_Corner_Front", "Overhang_UnevenBrick_Corner", "Overhang_UnevenBrick_Corner_Front"][(hsh + int(float(cc[1]) * 2.0)) % 4], Transform3D(face, c + Vector3(0, tall, 0)))
				put.call(["Overhang_RoofIncline_Plaster", "Overhang_RoofIncline_UnevenBricks"][hsh % 2], Transform3D(face, c + Vector3(0, tall, 0)))
				continue
			var post := "Corner_ExteriorWide_Brick" if f == 0 and w > 16.0 else ("Corner_Exterior_Brick" if f == 0 else ("Corner_ExteriorWide_Wood" if w > 16.0 else "Corner_Exterior_Wood"))
			if f == floors - 1 and f > 0 and (hsh / 19) % 2 == 0:
				post = "Corner_Exterior_TopDown"
			put.call(post, Transform3D(Basis.from_scale(Vector3.ONE * VS), c + Vector3(0, tall * f, 0)))
		if tower:
			put.call(["Balcony_Simple_Corner", "Balcony_Cross_Corner"][hsh % 2], Transform3D(face, c + Vector3(0, tall * 2, 0)))
	put.call("Corner_Exterior_TopOnly", Transform3D(Basis.from_scale(Vector3.ONE * VS), Vector3(x0, tall * floors, y0)))
	_house_roof(put, b, tall * floors, tower, hsh)
	_house_yard(put, rng, door_at, door_out, hsh)
	_house_porch(put, door_at, door_out, hsh)
	if floors == 2 and not jetty:
		_house_stairs(put, sides[(door_side + 1) % 4], tall, hsh)
	for name in parts:
		_batch(batch, VILLAGE + String(name) + ".gltf", parts[name], node.position.y)
	return node

## A tiled roof chosen for the footprint's shape and stretched to fit it, gable ends, a chimney and dormers.
func _house_roof(put: Callable, b: Array, top: float, tower: bool, hsh: int) -> void:
	var w: float = b[2] - b[0]
	var d: float = b[3] - b[1]
	var centre := Vector3((b[0] + b[2]) / 2.0, top, (b[1] + b[3]) / 2.0)
	if tower:
		put.call("Roof_Tower_RoundTiles", Transform3D(Basis.from_scale(Vector3(w / 5.66, VS, d / 5.42)), centre))
		return
	# The ridge runs along the longer side.
	var along_x := w > d
	var span := d if along_x else w
	var length := w if along_x else d
	var best: Array = ROOFS[0]
	for r in ROOFS:
		if absf(float(r[1]) / float(r[0]) - length / span) < absf(float(best[1]) / float(best[0]) - length / span):
			best = r
	var name := "Roof_RoundTiles_%dx%d" % [best[0], best[1]]
	# The kit roof overhangs about one unit each way; stretch it to the box plus that overhang.
	var sx := (span + 1.0) / (float(best[0]) + 2.0)
	var sz := (length + 1.0) / (float(best[1]) + 2.0)
	var turn := Basis(Vector3.UP, PI / 2.0) if along_x else Basis()
	put.call(name, Transform3D(turn * Basis.from_scale(Vector3(sx, minf(sx, VS * 1.2), sz)), centre))
	# Gable ends, the brick triangle matching the roof's width, at both ends.
	var gable := "Roof_Front_Brick%d" % clampi(best[0], 4, 8)
	if best[0] == 4 or best[0] == 6 or best[0] == 8:
		for end in [-1.0, 1.0]:
			var face: Basis = turn * Basis(Vector3.UP, 0.0 if end > 0 else PI)
			var at: Vector3 = centre + turn * Vector3(0, 0, end * length / 2.0)
			put.call(gable, Transform3D(face * Basis.from_scale(Vector3(sx, minf(sx, VS * 1.2), VS)), at))
	# A beam under each gable, and a ridge log along the top of the bigger roofs.
	for end in [-1.0, 1.0]:
		put.call("Roof_FrontSupports", Transform3D(turn * Basis(Vector3.UP, 0.0 if end > 0 else PI) * Basis.from_scale(Vector3(sx, VS, VS)), centre + turn * Vector3(0, 0, end * length / 2.0)))
	if best[0] == 8:
		put.call("Roof_Log", Transform3D(turn * Basis.from_scale(Vector3(sx, minf(sx, VS * 1.2), (length + 1.0) / 10.7)), centre))
	# Ridge caps every couple of units along the ridge.
	var ridge_y := 6.0 * minf(sx, VS * 1.2) * (float(best[0]) / 8.0) * 0.95
	for k in int(length / (2.4 * VS)):
		put.call("Roof_Modular_RoundTiles", Transform3D(turn * Basis.from_scale(Vector3.ONE * VS), centre + turn * Vector3(0, ridge_y, -length / 2.0 + 1.2 * VS + k * 2.4 * VS)))
	# A chimney through the ridge and dormers on long roofs.
	put.call(["Prop_Chimney", "Prop_Chimney2"][hsh % 2], Transform3D(Basis.from_scale(Vector3.ONE * VS), centre + turn * Vector3(span * 0.15, 0, length * 0.25)))
	if length >= 12.0:
		for k in [-0.2, 0.2]:
			var at: Vector3 = centre + turn * Vector3(span * 0.25, 0.4, length * k)
			put.call("Roof_Dormer_RoundTile", Transform3D(turn * Basis.from_scale(Vector3.ONE * VS), at))
			put.call("Window_Roof_Wide" if k < 0 else "Window_Roof_Thin", Transform3D(turn * Basis(Vector3.UP, PI / 2.0) * Basis.from_scale(Vector3.ONE * VS), at + turn * Vector3(1.9 * VS, 0, 0)))

## A small tiled roof on brackets over the door, its own little gable to the front.
func _house_porch(put: Callable, door: Vector3, out: Vector3, hsh: int) -> void:
	if out == Vector3.ZERO:
		return
	var face := Basis(Vector3.UP, atan2(out.x, out.z)) * Basis.from_scale(Vector3.ONE * VS)
	var at := door + Vector3(0, 2.75 * VS, 0)
	put.call(["Roof_RoundTile_2x1", "Roof_RoundTile_2x1_Long", "Roof_2x4_RoundTile"][hsh % 3], Transform3D(face, at))
	put.call("Roof_Front_Brick2", Transform3D(face, at + out * (1.4 * VS)))
	for sx in [-1.2, 1.2]:
		put.call("Roof_Support2", Transform3D(face, at + face * Vector3(sx / VS, 0, 0)))

## An outside staircase up one side of a two-storey house to a balcony: a flight of steps, its railings, and a
## landing at the top, each picked from the kit's variants.
func _house_stairs(put: Callable, side: Array, tall: float, hsh: int) -> void:
	var start: Vector3 = side[0]
	var dir: Vector3 = side[1]
	var length: float = side[2]
	var out: Vector3 = side[3]
	if length < 10.0:
		return
	var face := Basis(Vector3.UP, atan2(dir.x, dir.z)) * Basis.from_scale(Vector3.ONE * VS)
	var flights := ["Stairs_Exterior_Straight", "Stairs_Exterior_Straight_L", "Stairs_Exterior_Straight_R", "Stairs_Exterior_Straight_Center", "Stairs_Exterior_NoFirstStep"]
	var rails := ["Stairs_Exterior_Sides", "Stairs_Exterior_SingleSide", "Stairs_Exterior_SingleSideThick", "Stairs_Exterior_Sides45", "Stairs_Exterior_SidesU"]
	var tops := ["Stairs_Exterior_Platform", "Stairs_Exterior_Platform45", "Stairs_Exterior_Platform45Clean", "Stairs_Exterior_PlatformU", "Stairs_Exterior_SidePlatform"]
	var steps := int(ceil(tall / VS))
	var base := start + dir * (length * 0.2) + out * (1.0 * VS)
	for k in steps:
		var at := base + dir * (2.0 * VS * k / 2.0) + Vector3(0, VS * k, 0)
		put.call(flights[(hsh + k) % flights.size()] if k > 0 else flights[(hsh / 3) % flights.size()], Transform3D(face, at))
		put.call(rails[(hsh / 5) % rails.size()], Transform3D(face, at))
	put.call(tops[(hsh / 7) % tops.size()], Transform3D(face, base + dir * (VS * steps) + Vector3(0, tall - VS, 0)))
	put.call("Balcony_Simple_Straight", Transform3D(Basis(Vector3.UP, atan2(out.x, out.z)) * Basis.from_scale(Vector3.ONE * VS), base + dir * (VS * (steps + 1.5)) - out * (1.0 * VS) + Vector3(0, tall, 0)))

## The yard in front of the door: a paved path, a fence each side, and a wagon, crates and bricks about. All
## outside the box, so you walk through them (the server only knows the house).
func _house_yard(put: Callable, rng: RandomNumberGenerator, door: Vector3, out: Vector3, hsh: int) -> void:
	if out == Vector3.ZERO:
		return
	var rot := Basis(Vector3.UP, atan2(out.x, out.z))
	var side := rot * Vector3(1, 0, 0)
	var tile := Basis.from_scale(Vector3.ONE * VS)
	var paving: String = ["Floor_UnevenBrick", "Floor_Brick", "Floor_RedBrick"][hsh % 3]
	for k in 3:
		put.call(paving, Transform3D(tile, door + out * (VS * (1.0 + 2.0 * k)) + Vector3(0, 0.05, 0)))
	for s2 in [-1.0, 1.0]:
		for k in 3:
			var at: Vector3 = door + out * (VS * (1.0 + 2.0 * k)) + side * s2 * (VS * 2.6)
			var piece: String = "Prop_WoodenFence_Single" if k == 0 else ["Prop_WoodenFence_Extension1", "Prop_WoodenFence_Extension2"][k % 2]
			put.call(piece, Transform3D(rot * Basis(Vector3.UP, PI / 2.0) * tile, at))
	if hsh % 3 == 0:
		put.call("Prop_MetalFence_Ornament", Transform3D(rot * tile, door + out * (VS * 6.2) + side * (VS * 1.6)))
		put.call("Prop_MetalFence_Simple", Transform3D(rot * tile, door + out * (VS * 6.2) - side * (VS * 1.6)))
	put.call("Wall_Arch", Transform3D(rot * tile, door + out * (VS * 6.0)))
	put.call("Prop_Wagon", Transform3D(rot * Basis(Vector3.UP, 0.4) * tile, door + out * (VS * 4.0) + side * (VS * 5.0)))
	# A wooden lean-to shed, or a market stall with a roof both ways, beside the path.
	var shed := door + out * (VS * 4.5) - side * (VS * 5.5)
	var shed_face := rot * Basis(Vector3.UP, PI / 2.0) * tile
	for sx2 in [-1.0, 1.0]:
		put.call("Prop_Support", Transform3D(shed_face, shed + side * sx2 * (VS * 2.2) + Vector3(0, -1.2 * VS, 0)))
	if hsh % 2 == 0:
		var run := ["Roof_Wooden_2x1_L", "Roof_Wooden_2x1_Middle", "Roof_Wooden_2x1_R"]
		for k in run.size():
			put.call(run[k], Transform3D(shed_face, shed + side * (VS * 2.2 * (k - 1)) + Vector3(0, 2.0 * VS, 0)))
		put.call("Roof_Wooden_2x1_Corner", Transform3D(shed_face, shed + side * (VS * 3.4) + Vector3(0, 2.0 * VS, 0)))
		var half: int = [4, 6, 8][hsh % 3]
		put.call("Roof_Front_Brick%d_Half_L" % half, Transform3D(shed_face * Basis.from_scale(Vector3.ONE * 0.35), shed - side * (VS * 3.4) + Vector3(0, 0.6, 0)))
		put.call("Roof_Front_Brick%d_Half_R" % half, Transform3D(shed_face * Basis.from_scale(Vector3.ONE * 0.35), shed + side * (VS * 3.4) + Vector3(0, 0.6, 0)))
	else:
		put.call("Roof_Wooden_2x1_Center", Transform3D(shed_face, shed + Vector3(0, 2.0 * VS, 0)))
		put.call("Roof_Wooden_2x1_Center_Mirror", Transform3D(shed_face, shed + side * (VS * 2.3) + Vector3(0, 2.0 * VS, 0)))
		put.call("Roof_Wooden_2x1", Transform3D(shed_face, shed - side * (VS * 2.3) + Vector3(0, 2.0 * VS, 0)))
	for k in 3:
		put.call("Prop_Crate", Transform3D(Basis(Vector3.UP, rng.randf() * TAU) * tile, door + out * (VS * rng.randf_range(1.0, 4.0)) - side * (VS * rng.randf_range(3.6, 5.0))))
	for k in 4:
		put.call("Prop_Brick%d" % (k + 1), Transform3D(Basis(Vector3.UP, rng.randf() * TAU) * tile, door + out * (VS * rng.randf_range(0.6, 5.0)) + side * (VS * rng.randf_range(-2.0, 2.0)) + Vector3(0, 0.1, 0)))

## A flame that blooms (brighter than white, so glow picks it up) and the warm light it casts.
func _flame(node: Node3D, at: Vector3, r: float) -> void:
	var flame := MeshInstance3D.new()
	var fs := SphereMesh.new()
	fs.radius = r
	fs.height = r * 2.3
	flame.mesh = fs
	var fm := StandardMaterial3D.new()
	fm.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	fm.albedo_color = Color(3.0, 1.5, 0.45)
	flame.material_override = fm
	flame.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	flame.position = at
	node.add_child(flame)
	var tl := OmniLight3D.new()
	tl.light_color = Color("ffb35c")
	tl.light_energy = 1.6
	tl.omni_range = 9.0
	tl.position = at + Vector3(0, 0.2, 0)
	node.add_child(tl)

## A crag: rock rising from cliffs at the wall line to a peak, so the box you cannot walk into is a mountain.
## Drawn with the ground's material, so its steep faces come out as rock and a tall one gets a snowy top.
func _build_crag(b: Array, idx: int) -> Node3D:
	var x0: float = b[0]
	var y0: float = b[1]
	var x1: float = b[2]
	var y1: float = b[3]
	var w := x1 - x0
	var d := y1 - y0
	var r := minf(w, d) / 2.0
	var peak := clampf(minf(w, d) * 1.4, 14.0, 56.0)
	var mound := peak * 0.5 # the rock pile makes the silhouette; the mound under it fills the gaps
	var ground: float = land.base[idx]
	var nx := maxi(2, int(ceil(w / 2.0)))
	var ny := maxi(2, int(ceil(d / 2.0)))
	var hs := PackedFloat32Array()
	hs.resize((nx + 1) * (ny + 1))
	for j in ny + 1:
		for i in nx + 1:
			var x := x0 + w * i / nx
			var y := y0 + d * j / ny
			var inside := minf(minf(x - x0, x1 - x), minf(y - y0, y1 - y))
			var hgt := ground - 1.0 # the rim sinks into the ground, so the cliff meets it cleanly
			if inside > 0.01:
				var cliff := smoothstep(0.0, 3.0, inside)
				var body := smoothstep(0.0, r, inside)
				var crags := 0.7 + 0.6 * land.peaks.get_noise_2d(x * 4.0, y * 4.0) + 0.25 * land.bumps.get_noise_2d(x * 9.0, y * 9.0)
				hgt = ground + mound * (0.35 * cliff + 0.65 * pow(body, 0.8)) * crags
			hs[j * (nx + 1) + i] = hgt
	var verts := PackedVector3Array()
	var norms := PackedVector3Array()
	var idxs := PackedInt32Array()
	var sx := w / nx
	var sy := d / ny
	for j in ny + 1:
		for i in nx + 1:
			var at := j * (nx + 1) + i
			verts.append(Vector3(x0 + sx * i, hs[at], y0 + sy * j))
			var l := hs[at - 1] if i > 0 else hs[at]
			var rr := hs[at + 1] if i < nx else hs[at]
			var u := hs[at - (nx + 1)] if j > 0 else hs[at]
			var dn := hs[at + (nx + 1)] if j < ny else hs[at]
			norms.append(Vector3((l - rr) / (2.0 * sx), 1.0, (u - dn) / (2.0 * sy)).normalized())
	for j in ny:
		for i in nx:
			var a := j * (nx + 1) + i
			idxs.append_array([a, a + 1, a + nx + 1, a + 1, a + nx + 2, a + nx + 1])
	var arr := []
	arr.resize(Mesh.ARRAY_MAX)
	arr[Mesh.ARRAY_VERTEX] = verts
	arr[Mesh.ARRAY_NORMAL] = norms
	arr[Mesh.ARRAY_INDEX] = idxs
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arr)
	var node := Node3D.new()
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.material_override = _crag_mat()
	node.add_child(mi)
	node.add_child(_crag_rocks(b, ground, peak, r))
	node.add_child(_rim_rocks(b, ground, 9.0))
	return node

## The crag itself: KayKit spires towards the middle and boulders round the flanks, sized to the height the
## crag should reach at each spot and kept inside the box.
func _crag_rocks(b: Array, ground: float, peak: float, r: float) -> Node3D:
	var x0: float = b[0]
	var y0: float = b[1]
	var x1: float = b[2]
	var y1: float = b[3]
	var rng := RandomNumberGenerator.new()
	rng.seed = (int(x0) * 83492791) ^ (int(y0) * 19349663)
	var g := clampf(minf(x1 - x0, y1 - y0) / 5.0, 3.0, 9.0)
	var gx := maxi(1, int((x1 - x0) / g))
	var gy := maxi(1, int((y1 - y0) / g))
	var per := {}
	for j in gy:
		for i in gx:
			var x := x0 + (i + 0.5 + rng.randf_range(-0.35, 0.35)) * (x1 - x0) / gx
			var y := y0 + (j + 0.5 + rng.randf_range(-0.35, 0.35)) * (y1 - y0) / gy
			var inside := minf(minf(x - x0, x1 - x), minf(y - y0, y1 - y))
			if inside < 1.0:
				continue
			var body := smoothstep(0.0, r, inside)
			var target := peak * (0.3 + 0.7 * body) * rng.randf_range(0.7, 1.05)
			var pick: Array = SPIRES[rng.randi() % SPIRES.size()] if body > 0.55 else BOULDERS[rng.randi() % BOULDERS.size()]
			var s: float = minf(target / float(pick[2]), 2.0 * (inside + 1.0) / float(pick[1]))
			if s < 0.6:
				continue
			_add_rock(per, pick, s, Vector3(x, ground - 0.4, y), rng.randf() * TAU)
	return _rock_node(per)

func _add_rock(per: Dictionary, pick: Array, s: float, at: Vector3, turn: float) -> void:
	var name: String = pick[0]
	if not per.has(name):
		per[name] = []
	at.y -= float(pick[3]) * s # sit the rock's own base on the ground
	per[name].append(Transform3D(Basis(Vector3.UP, turn).scaled(Vector3.ONE * s), at))

func _rock_node(per: Dictionary) -> Node3D:
	var node := Node3D.new()
	for name in per:
		var m := _mesh(FOREST + String(name) + ".gltf")
		if m:
			node.add_child(_multi(m, per[name], ROCK_TINT))
	return node

## The ground's material with rock everywhere but the lowest gentle hollows, for crags.
var _crag: ShaderMaterial
func _crag_mat() -> ShaderMaterial:
	if _crag == null:
		_crag = terrain_mat.duplicate() as ShaderMaterial
		_crag.set_shader_parameter("rocky", 1.0)
	return _crag

## A lake: deep water filling the box (the server will not let you in), its bed sunk into the ground, and a
## broken line of rocks along the shore so it does not read as a rectangle.
func _build_lake(b: Array, idx: int) -> Node3D:
	var node := Node3D.new()
	var water := MeshInstance3D.new()
	var pm := PlaneMesh.new()
	pm.size = Vector2(b[2] - b[0] + 1.0, b[3] - b[1] + 1.0)
	pm.subdivide_width = 8
	pm.subdivide_depth = 8
	water.mesh = pm
	water.material_override = _water()
	water.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	water.position = Vector3((b[0] + b[2]) / 2.0, land.lake_level(idx), (b[1] + b[3]) / 2.0)
	node.add_child(water)
	node.add_child(_rim_rocks(b, land.base[idx], 5.0))
	node.add_child(_dock(b, land.lake_level(idx), idx))
	return node

## A wooden dock from the middle of one shore out over the water: plank floor (dark or light, with half and
## corner boards at the end) on posts. Decoration over the lake, which the server keeps you out of.
func _dock(b: Array, water: float, idx: int) -> Node3D:
	var hsh := absi(int(b[0]) * 7 + int(b[1]) * 31 + idx)
	var tone := "WoodDark" if hsh % 2 == 0 else "WoodLight"
	var sides := [[Vector3((b[0] + b[2]) / 2.0, 0, b[1]), Vector3(0, 0, 1)], [Vector3((b[0] + b[2]) / 2.0, 0, b[3]), Vector3(0, 0, -1)],
		[Vector3(b[0], 0, (b[1] + b[3]) / 2.0), Vector3(1, 0, 0)], [Vector3(b[2], 0, (b[1] + b[3]) / 2.0), Vector3(-1, 0, 0)]]
	var sd: Array = sides[hsh % 4]
	var shore: Vector3 = sd[0]
	var into: Vector3 = sd[1]
	var face := Basis(Vector3.UP, atan2(into.x, into.z)) * Basis.from_scale(Vector3.ONE * VS)
	var per := {}
	var add := func(name: String, xf: Transform3D):
		if not per.has(name):
			per[name] = []
		per[name].append(xf)
	var planks := 3
	for k in planks:
		var at := shore + into * (VS * (1.0 + 2.0 * k)) + Vector3(0, water + 0.45, 0)
		add.call("Floor_%s" % tone if k < planks - 1 else "Floor_%s_Half%d" % ["WoodDark", 1 + hsh % 3] if tone == "WoodDark" else "Floor_WoodLight", Transform3D(face, at))
		add.call("Prop_Support", Transform3D(face * Basis(Vector3.UP, PI), at - Vector3(0, 2.9 * VS, 0)))
	var end := shore + into * (VS * 2.0 * planks) + Vector3(0, water + 0.45, 0)
	add.call("Floor_%s_OverhangCorner" % tone, Transform3D(face, end))
	add.call("Floor_%s_OverhangCorner2" % tone, Transform3D(face * Basis(Vector3.UP, PI / 2.0), end))
	var node := Node3D.new()
	for name in per:
		var m := _mesh(VILLAGE + String(name) + ".gltf")
		if m:
			node.add_child(_multi(m, per[name]))
	return node

## Rocks along a box's edge, every `gap` units or so, from the box's corner so every client agrees.
func _rim_rocks(b: Array, ground: float, gap: float) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = (int(b[0]) * 73856093) ^ (int(b[1]) * 19349663)
	var picks := ["Rock_1_A_Color1", "Rock_1_J_Color1", "Rock_2_A_Color1", "Rock_3_E_Color1"]
	var per := {}
	var sides := [[Vector2(b[0], b[1]), Vector2(b[2], b[1])], [Vector2(b[2], b[1]), Vector2(b[2], b[3])],
		[Vector2(b[2], b[3]), Vector2(b[0], b[3])], [Vector2(b[0], b[3]), Vector2(b[0], b[1])]]
	for sd in sides:
		var a: Vector2 = sd[0]
		var c: Vector2 = sd[1]
		var n := maxi(1, int(a.distance_to(c) / gap))
		for k in n + 1:
			var p := a.lerp(c, (k + rng.randf() * 0.6) / (n + 1))
			var pick: String = picks[rng.randi() % picks.size()]
			if not per.has(pick):
				per[pick] = []
			var s := rng.randf_range(1.6, 3.2)
			per[pick].append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * s), Vector3(p.x, ground - 0.3, p.y)))
	var node := Node3D.new()
	for pick in per:
		var m := _mesh(FOREST + String(pick) + ".gltf")
		if m:
			node.add_child(_multi(m, per[pick], Color(0.72, 0.69, 0.64)))
	return node

## A river's water: a ribbon along its course at its level, a little wider than the channel so the banks hide the edge.
func _river_mesh(r: Dictionary) -> Node3D:
	var pts: PackedVector2Array = r["pts"]
	var lvl: PackedFloat32Array = r["lvl"]
	var verts := PackedVector3Array()
	var norms := PackedVector3Array()
	var idxs := PackedInt32Array()
	var half: float = land.RIVER_HALF + 1.0 # inside the lip, where the bank already stands above the water
	for k in pts.size():
		var a := pts[maxi(0, k - 1)]
		var c := pts[mini(pts.size() - 1, k + 1)]
		var dir := (c - a).normalized()
		var side := Vector2(-dir.y, dir.x) * half
		var y: float = land.water_level(lvl[k])
		verts.append(Vector3(pts[k].x + side.x, y, pts[k].y + side.y))
		verts.append(Vector3(pts[k].x - side.x, y, pts[k].y - side.y))
		norms.append(Vector3.UP)
		norms.append(Vector3.UP)
		if k > 0:
			var i := k * 2
			idxs.append_array([i - 2, i - 1, i, i - 1, i + 1, i])
	var arr := []
	arr.resize(Mesh.ARRAY_MAX)
	arr[Mesh.ARRAY_VERTEX] = verts
	arr[Mesh.ARRAY_NORMAL] = norms
	arr[Mesh.ARRAY_INDEX] = idxs
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arr)
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.material_override = _water()
	mi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	# KayKit rocks and bushes along the banks, every dozen units or so, either side in turn.
	var node := Node3D.new()
	node.add_child(mi)
	var rng := RandomNumberGenerator.new()
	rng.seed = int(pts[0].x) * 7919 + int(pts[0].y)
	var rocks := {}
	var bushes: Array = []
	for k in range(1, pts.size() - 1, 1):
		if rng.randf() > 0.8:
			continue
		var dir := (pts[k + 1] - pts[k - 1]).normalized()
		var side := Vector2(-dir.y, dir.x) * (1.0 if rng.randf() < 0.5 else -1.0)
		var p := pts[k] + side * (land.RIVER_HALF + land.RIVER_LIP + rng.randf_range(0.3, 2.3))
		if rng.randf() < 0.55:
			var small := [["Rock_1_A_Color1", 0.61, 0.54, 0.0], ["Rock_3_E_Color1", 1.18, 1.29, -0.59], ["Rock_1_J_Color1", 3.76, 3.36, 0.0]]
			var pick: Array = small[rng.randi() % small.size()]
			_add_rock(rocks, pick, rng.randf_range(1.0, 2.6) / float(pick[1]) * 1.5, Vector3(p.x, land.height(p.x, p.y), p.y), rng.randf() * TAU)
		else:
			bushes.append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * rng.randf_range(2.2, 3.4)), Vector3(p.x, land.height(p.x, p.y), p.y)))
	node.add_child(_rock_node(rocks))
	var bm := _mesh(FOREST + "Bush_2_A_Color1.gltf")
	if bm and not bushes.is_empty():
		node.add_child(_multi(bm, bushes))
	return node

## Water: clear and turquoise where shallow, deep blue where deep (read from the depth buffer), foam at the
## shoreline, and two drifting ripple layers. Forward+ only, like the rest of the effects.
func _water() -> ShaderMaterial:
	if water_mat:
		return water_mat
	var sh := Shader.new()
	sh.code = """shader_type spatial;
render_mode cull_disabled, depth_draw_always;
uniform sampler2D depth_tex : hint_depth_texture, filter_linear;
varying vec3 wp;
float h(vec2 p) { vec3 q = fract(vec3(p.xyx) * 0.1031); q += dot(q, q.yzx + 33.33); return fract((q.x + q.y) * q.z); }
float n(vec2 p) {
	vec2 i = floor(p); vec2 f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(h(i), h(i + vec2(1, 0)), u.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), u.x), u.y);
}
float waves(vec2 q, float t) { return n(q * 0.35 + vec2(t, t * 0.7)) + 0.5 * n(q * 0.9 - vec2(t * 1.3, t * 0.4)); }
void vertex() { wp = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz; }
void fragment() {
	float t = TIME * 0.5;
	vec2 q = wp.xz;
	float e = 0.2;
	float a = waves(q, t);
	vec3 wn = normalize(vec3((a - waves(q + vec2(e, 0.0), t)) / e * 0.3, 1.0, (a - waves(q + vec2(0.0, e), t)) / e * 0.3));
	NORMAL = normalize((VIEW_MATRIX * vec4(wn, 0.0)).xyz);
	float depth = texture(depth_tex, SCREEN_UV).r;
	vec4 v = INV_PROJECTION_MATRIX * vec4(SCREEN_UV * 2.0 - 1.0, depth, 1.0);
	v.xyz /= v.w;
	float thick = clamp(VERTEX.z - v.z, 0.0, 20.0);
	float k = smoothstep(0.0, 3.0, thick);
	vec3 c = mix(vec3(0.14, 0.44, 0.47), vec3(0.03, 0.14, 0.24), k);
	float foam = (1.0 - smoothstep(0.0, 0.45, thick)) * (0.6 + 0.4 * n(q * 2.0 + t * 2.0));
	ALBEDO = mix(c, vec3(0.92, 0.95, 0.95), foam * 0.8);
	ALPHA = clamp(0.55 + k * 0.4 + foam * 0.4, 0.0, 0.96);
	ROUGHNESS = 0.06;
	SPECULAR = 0.7;
}"""
	water_mat = ShaderMaterial.new()
	water_mat.shader = sh
	return water_mat

func _chunk_key(cx: int, cy: int) -> int:
	return cx * 100003 + cy

func _index_boxes() -> void:
	box_grid.clear()
	box_home.clear()
	for i in boxes.size():
		var bb: Array = boxes[i]
		var hk := _chunk_key(int(floor((bb[0] + bb[2]) / 2.0 / PROP_CHUNK)), int(floor((bb[1] + bb[3]) / 2.0 / PROP_CHUNK)))
		if not box_home.has(hk):
			box_home[hk] = []
		box_home[hk].append(i)
	for i in boxes.size():
		var b: Array = boxes[i]
		for cx in range(int(floor((b[0] - FLAT_PAD) / PROP_CHUNK)), int(floor((b[2] + FLAT_PAD) / PROP_CHUNK)) + 1):
			for cy in range(int(floor((b[1] - FLAT_PAD) / PROP_CHUNK)), int(floor((b[3] + FLAT_PAD) / PROP_CHUNK)) + 1):
				var k := _chunk_key(cx, cy)
				if not box_grid.has(k):
					box_grid[k] = []
				box_grid[k].append(i)

## Each wall box becomes a little keep: dungeon wall pieces tiled along every side (stretched slightly so a
## whole number fits), stone pillars at the corners and every other joint, a roof, and a banner or a torch on
## long sides. Everything stays inside the box, so what you see is exactly what you collide with.
func _build_box(b: Array, idx: int, batch: Dictionary = {}) -> Node3D:
	if land.kinds[idx] == land.CRAG:
		return _build_crag(b, idx)
	if land.kinds[idx] == land.LAKE:
		return _build_lake(b, idx)
	# Half the keeps are village houses instead (the console draws both as roofs). A long box is a terraced
	# row, cut along its length into houses that share their walls.
	var w: float = b[2] - b[0]
	var d: float = b[3] - b[1]
	if _is_house(b) and ResourceLoader.exists(VILLAGE + "Wall_Plaster_Straight.gltf"):
		var row := Node3D.new()
		var n := maxi(1, int(ceil(maxf(w, d) / 18.0)))
		for k in n:
			var part: Array = [b[0] + w * k / n, b[1], b[0] + w * (k + 1) / n, b[3]] if w >= d else [b[0], b[1] + d * k / n, b[2], b[1] + d * (k + 1) / n]
			row.add_child(_build_house(part, idx, batch))
		return row
	var x0: float = b[0]
	var y0: float = b[1]
	var x1: float = b[2]
	var y1: float = b[3]
	var hsh := absi(int(x0 * 7.0) * 31 + int(y0 * 13.0))
	var layers := 1 + hsh % 2
	var height := 4.0 * layers
	var node := Node3D.new()
	node.position.y = land.base[idx]
	var pieces := {"torch_mounted.gltf": []}
	var put := func(name: String, xf: Transform3D):
		if not pieces.has(name):
			pieces[name] = []
		pieces[name].append(xf)
	var gate_side := hsh % 4 # one gated entrance per keep, on a side long enough for it
	# Sides as start, direction, length and outward normal (server x is world x, server y is world z).
	var sides := [
		[Vector3(x0, 0, y0), Vector3(1, 0, 0), x1 - x0, Vector3(0, 0, -1)],
		[Vector3(x1, 0, y1), Vector3(-1, 0, 0), x1 - x0, Vector3(0, 0, 1)],
		[Vector3(x0, 0, y1), Vector3(0, 0, -1), y1 - y0, Vector3(-1, 0, 0)],
		[Vector3(x1, 0, y0), Vector3(0, 0, 1), y1 - y0, Vector3(1, 0, 0)],
	]
	for si in sides.size():
		var s: Array = sides[si]
		var start: Vector3 = s[0]
		var dir: Vector3 = s[1]
		var length: float = s[2]
		var out: Vector3 = s[3]
		var count := maxi(1, int(round(length / 4.0)))
		var seg := length / count
		var rot := Basis(Vector3.UP, atan2(out.x, out.z))
		var stretch := rot * Basis.from_scale(Vector3(seg / 4.0, 1, 1))
		for i in count:
			var at := start + dir * (seg * (i + 0.5)) - out * 0.5
			for layer in layers:
				var top := layer == layers - 1
				var v := (hsh + i * 7 + si * 3 + layer * 5) % 17
				var piece := "wall.gltf"
				if si == gate_side and i == count / 2 and layer == 0 and length >= 12.0:
					piece = "wall_gated.gltf" # the way in, barred (the server's wall is solid)
				elif v == 0:
					piece = "wall_cracked.gltf"
				elif v == 1 and layer == 0:
					piece = "wall_arched.gltf"
				elif v == 2 and top:
					piece = "wall_window_closed.gltf"
				elif v == 3 and top:
					piece = "wall_window_open.gltf"
				elif v == 4 and top:
					piece = "wall_archedwindow_gated.gltf"
				put.call(piece, Transform3D(stretch, at + Vector3(0, 4.0 * layer, 0)))
			if i % 2 == 0 and i > 0:
				var joint := start + dir * (seg * i) - out * 0.75
				for layer in layers:
					put.call("pillar.gltf", Transform3D(Basis(), joint + Vector3(0, 4.0 * layer, 0)))
		if length >= 16.0:
			var mid := start + dir * (length / 2.0) - out * 0.5
			var banners := ["banner_red.gltf", "banner_blue.gltf", "banner_patternA_red.gltf", "banner_patternB_blue.gltf",
				"banner_shield_green.gltf", "banner_triple_yellow.gltf", "banner_thin_white.gltf"]
			var deco: String = "torch_mounted.gltf" if (hsh + si) % 3 == 2 else banners[(hsh + si * 5) % banners.size()]
			var lift := 0.0 if deco.begins_with("banner") else 2.6
			put.call(deco, Transform3D(rot, mid + out * (0.0 if deco.begins_with("banner") else 0.5) + Vector3(0, lift + 4.0 * (layers - 1), 0)))
	for c in [Vector3(x0 + 0.75, 0, y0 + 0.75), Vector3(x1 - 0.75, 0, y0 + 0.75), Vector3(x0 + 0.75, 0, y1 - 0.75), Vector3(x1 - 0.75, 0, y1 - 0.75)]:
		for layer in layers:
			put.call("pillar_decorated.gltf" if layer == layers - 1 else "pillar.gltf", Transform3D(Basis(), c + Vector3(0, 4.0 * layer, 0)))
	# The roof: KayKit floor tiles stretched to fit, with a few stores stood on top (out of reach of collision).
	var tx := maxi(1, int(round((x1 - x0) / 4.0)))
	var ty := maxi(1, int(round((y1 - y0) / 4.0)))
	var tile := Basis.from_scale(Vector3((x1 - x0) / tx / 4.0, 1, (y1 - y0) / ty / 4.0))
	for j in ty:
		for i in tx:
			var at := Vector3(x0 + (x1 - x0) * (i + 0.5) / tx, height + 0.05, y0 + (y1 - y0) * (j + 0.5) / ty)
			put.call("floor_tile_large_rocks.gltf" if (hsh + i * 3 + j * 7) % 9 == 0 else "floor_tile_large.gltf", Transform3D(tile, at))
	var stores := ["barrel_small.gltf", "barrel_small_stack.gltf", "box_large.gltf", "box_stacked.gltf", "crates_stacked.gltf",
		"keg_decorated.gltf", "trunk_large_A.gltf", "barrel_large.gltf"]
	var rng := RandomNumberGenerator.new()
	rng.seed = hsh
	for k in clampi(int((x1 - x0) * (y1 - y0) / 140.0), 1, 7):
		var at := Vector3(rng.randf_range(x0 + 2.5, maxf(x0 + 2.6, x1 - 2.5)), height + 0.1, rng.randf_range(y0 + 2.5, maxf(y0 + 2.6, y1 - 2.5)))
		put.call(stores[rng.randi() % stores.size()], Transform3D(Basis(Vector3.UP, rng.randf() * TAU), at))
	# Standing torches at the roof's corners, twice life size so they read from the camera.
	for c in [Vector3(x0 + 1.4, 0, y0 + 1.4), Vector3(x1 - 1.4, 0, y0 + 1.4), Vector3(x0 + 1.4, 0, y1 - 1.4), Vector3(x1 - 1.4, 0, y1 - 1.4)]:
		put.call("torch_lit.gltf", Transform3D(Basis.from_scale(Vector3.ONE * 2.0), c + Vector3(0, height + 0.85, 0)))
		_flame(node, c + Vector3(0, height + 0.85 + 1.5, 0), 0.22)
	# Each wall torch gets a flame that blooms and a warm light (Forward+ handles many lights cheaply).
	for xf in pieces["torch_mounted.gltf"]:
		_flame(node, (xf as Transform3D) * Vector3(0, 0.75, 0.35), 0.14)
	for p in pieces:
		if not pieces[p].is_empty():
			_batch(batch, DUNGEON + String(p), pieces[p], node.position.y)
	# Roof: a flat slate cap so the hollow inside never shows from the camera's height.
	var roof := MeshInstance3D.new()
	var rb := BoxMesh.new()
	rb.size = Vector3(x1 - x0, 0.4, y1 - y0)
	roof.mesh = rb
	roof.position = Vector3((x0 + x1) / 2.0, height - 0.25, (y0 + y1) / 2.0) # under the tiles, filling any gap
	var rm := StandardMaterial3D.new()
	rm.albedo_color = Color("5a5560")
	rm.roughness = 1.0
	roof.material_override = rm
	node.add_child(roof)
	return node

## Buildings are built a 128 unit chunk at a time (a box belongs to the chunk its centre is in), and every
## piece of the same kind in a chunk shares one MultiMesh, so a town costs a few dozen draws, not thousands.
func _sync_walls(center: Vector2) -> void:
	var keep := {}
	var r := int(ceil(WALL_DRAW / PROP_CHUNK))
	var ccx := int(floor(center.x / PROP_CHUNK))
	var ccy := int(floor(center.y / PROP_CHUNK))
	for cx in range(ccx - r, ccx + r + 1):
		for cy in range(ccy - r, ccy + r + 1):
			var k := _chunk_key(cx, cy)
			if not box_home.has(k):
				continue
			keep[k] = true
			if walls.has(k):
				continue
			var node := Node3D.new()
			var batch := {}
			for i in box_home[k]:
				node.add_child(_build_box(boxes[i], i, batch))
			for path in batch:
				var m := _mesh(path)
				if m:
					node.add_child(_multi(m, batch[path], _tint(path)))
			add_child(node)
			walls[k] = node
	for k in walls.keys():
		if not keep.has(k):
			walls[k].queue_free()
			walls.erase(k)

## Add a builder's pieces to its chunk's shared batch, lifting them by the building's base height.
func _batch(batch: Dictionary, path: String, xforms: Array, lift: float) -> void:
	if not batch.has(path):
		batch[path] = []
	for xf in xforms:
		batch[path].append(Transform3D((xf as Transform3D).basis, (xf as Transform3D).origin + Vector3(0, lift, 0)))

## The dungeon stone is very pale in daylight; a warm grey keeps it from glaring against the grass.
func _tint(path: String) -> Color:
	var f := path.get_file()
	if path.begins_with(DUNGEON) and (f.begins_with("wall") or f.begins_with("pillar") or f.begins_with("floor")):
		return Color(0.78, 0.74, 0.70)
	return Color.WHITE

## Scenery is scattered per chunk from the world seed, so every client draws the same trees in the same places.
## Nothing is placed in or right next to a wall box. Props are only looks: the server has no trees, so
## characters walk through them.
func _sync_props(center: Vector2) -> void:
	var size := float(info.get("world_size", 1024))
	var seed := int(info.get("seed", 1))
	var keep := {}
	var r := int(ceil(PROP_DRAW / PROP_CHUNK))
	var ccx := int(floor(center.x / PROP_CHUNK))
	var ccy := int(floor(center.y / PROP_CHUNK))
	var last := int(ceil(size / PROP_CHUNK)) - 1
	for cx in range(maxi(ccx - r, -EDGE_CHUNKS), mini(ccx + r, last + EDGE_CHUNKS) + 1):
		for cy in range(maxi(ccy - r, -EDGE_CHUNKS), mini(ccy + r, last + EDGE_CHUNKS) + 1):
			var k := _chunk_key(cx, cy)
			keep[k] = true
			if props.has(k):
				continue
			var n := _build_props(cx, cy, seed, size)
			add_child(n)
			props[k] = n
	for k in props.keys():
		if not keep.has(k):
			props[k].queue_free()
			props.erase(k)

func _build_props(cx: int, cy: int, seed: int, size: float) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = (seed * 73856093) ^ (cx * 19349663) ^ (cy * 83492791)
	var node := Node3D.new()
	var near: Array = box_grid.get(_chunk_key(cx, cy), [])
	# Past the playable edge, where the ground rises into mountains, KayKit spires and boulders crown the slopes.
	var outside_chunk := cx < 0 or cy < 0 or cx * PROP_CHUNK >= size or cy * PROP_CHUNK >= size
	if outside_chunk:
		var per := {}
		for k in 22:
			var x := (cx + rng.randf()) * PROP_CHUNK
			var y := (cy + rng.randf()) * PROP_CHUNK
			var out := maxf(maxf(-x, x - size), maxf(-y, y - size))
			if out < 25.0:
				continue
			var pick: Array = SPIRES[rng.randi() % SPIRES.size()] if out > 70.0 else BOULDERS[rng.randi() % BOULDERS.size()]
			_add_rock(per, pick, rng.randf_range(2.5, 6.0), Vector3(x, _height(x, y) - 0.5, y), rng.randf() * TAU)
		node.add_child(_rock_node(per))
	var kinds := {"trees": TREES, "bare": BARE_TREES, "rocks": SMALL_ROCKS, "bushes": BUSHES}
	for p in PROPS:
		var by_model := {}
		var models: Array = kinds[p[0]]
		var tree := String(p[0]) == "trees" or String(p[0]) == "bare"
		var count := int(p[1])
		if tree and (cx < 0 or cy < 0 or (cx + 1) * PROP_CHUNK > size or (cy + 1) * PROP_CHUNK > size):
			count *= 6 # a forest edge around the playable area
		for i in count:
			var x := (cx + rng.randf()) * PROP_CHUNK
			var y := (cy + rng.randf()) * PROP_CHUNK
			var s := rng.randf_range(p[2], p[3])
			var turn := rng.randf() * TAU
			var pad := maxf(7.0, 1.6 * s) if tree else 2.5 # trees keep out of streets (a street is 10 wide)
			var outside := x < 0.0 or y < 0.0 or x > size or y > size
			if tree and count > int(p[1]) and not outside and minf(minf(x, y), minf(size - x, size - y)) > 12.0:
				continue # the extra trees only grow in the last few units inside the edge and beyond it
			if _near_box(near, x, y, pad) or land.wet(x, y):
				continue
			var model: String = models[rng.randi() % models.size()]
			if not by_model.has(model):
				by_model[model] = []
			by_model[model].append(Transform3D(Basis(Vector3.UP, turn).scaled(Vector3.ONE * s), Vector3(x, _height(x, y) - 0.15, y)))
		for model in by_model:
			var m := _mesh(FOREST + String(model) + ".gltf")
			if m:
				node.add_child(_multi(m, by_model[model], ROCK_TINT if String(p[0]) == "rocks" else Color.WHITE))
	return node

func _near_box(idx: Array, x: float, y: float, pad: float) -> bool:
	for i in idx:
		var b: Array = boxes[i]
		if x > b[0] - pad and x < b[2] + pad and y > b[1] - pad and y < b[3] + pad:
			return true
	return false

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
		var np := Vector3(p.x, _height(p.x, p.y), p.y)
		var hp := clampf(float(e.get(health_idx, health_max)) / health_max, 0.0, 1.0) if health_idx != "" else 1.0
		var air := clampf(float(e.get("airborne", 0.0)) / air_max, 0.0, 1.0) if air_max > 0.0 else 0.0
		_animate(id, node, np, hp, mine, delta, self_id, air)
		if mine:
			me_pos = p
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
	wall_t += delta
	if wall_t > 0.4 and me_pos.distance_to(last_wall_sync) > 20.0:
		wall_t = 0.0
		last_wall_sync = me_pos
		_sync_walls(me_pos)
		_sync_props(me_pos)
		_sync_terrain(me_pos)
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
	hud.status.text = "%s   %d ms   %d, %d   %d in view%s%s%s" % [names[clampi(status, 0, 3)], int(net.get_rtt_ms()), me_pos.x, me_pos.y, list.size(), crowd, sky, "   GOD" if god else ""]

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
	for b in boxes:
		var c := Vector2((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)
		if c.distance_to(me_pos) < best_d:
			best_d = c.distance_to(me_pos)
			best = "%f,%f" % [c.x + 4.0, b[3] + 11.0]
	return best

# ---------------------------------------------------------------- terrain

## Ground height: see terrain.gd (hills, river channels, lake beds, level ground round buildings).
func _height(x: float, y: float) -> float:
	return land.height(x, y)

func _sync_terrain(center: Vector2) -> void:
	var size := float(info.get("world_size", 1024))
	var keep := {}
	var ccx := int(floor(center.x / PROP_CHUNK))
	var ccy := int(floor(center.y / PROP_CHUNK))
	# The ground carries on past the playable area (the server still stops you at the edge), so the world ends
	# in woodland rather than in a void.
	var last := int(ceil(size / PROP_CHUNK)) - 1
	for cx in range(maxi(ccx - TERRAIN_RINGS, -EDGE_CHUNKS), mini(ccx + TERRAIN_RINGS, last + EDGE_CHUNKS) + 1):
		for cy in range(maxi(ccy - TERRAIN_RINGS, -EDGE_CHUNKS), mini(ccy + TERRAIN_RINGS, last + EDGE_CHUNKS) + 1):
			var k := _chunk_key(cx, cy)
			keep[k] = true
			if not terrain.has(k):
				terrain[k] = _build_terrain(cx, cy, size)
				add_child(terrain[k])
	for k in terrain.keys():
		if not keep.has(k):
			terrain[k].queue_free()
			terrain.erase(k)

## One chunk of ground. Heights are sampled one step past each edge so normals come from the same samples on
## both sides of a seam and neighbouring chunks shade without a visible join.
func _build_terrain(cx: int, cy: int, size: float) -> MeshInstance3D:
	var n := int(ceil(PROP_CHUNK / TERRAIN_STEP))
	var ox := cx * PROP_CHUNK
	var oy := cy * PROP_CHUNK
	var hs := PackedFloat32Array()
	hs.resize((n + 3) * (n + 3))
	for j in n + 3:
		for i in n + 3:
			hs[j * (n + 3) + i] = _height(ox + (i - 1) * TERRAIN_STEP, oy + (j - 1) * TERRAIN_STEP)
	var verts := PackedVector3Array()
	var norms := PackedVector3Array()
	var idx := PackedInt32Array()
	for j in n + 1:
		for i in n + 1:
			var at := (j + 1) * (n + 3) + (i + 1)
			verts.append(Vector3(ox + i * TERRAIN_STEP, hs[at], oy + j * TERRAIN_STEP))
			norms.append(Vector3(hs[at - 1] - hs[at + 1], 2.0 * TERRAIN_STEP, hs[at - (n + 3)] - hs[at + (n + 3)]).normalized())
	for j in n:
		for i in n:
			var a := j * (n + 1) + i
			idx.append_array([a, a + 1, a + n + 1, a + 1, a + n + 2, a + n + 1])
	var arr := []
	arr.resize(Mesh.ARRAY_MAX)
	arr[Mesh.ARRAY_VERTEX] = verts
	arr[Mesh.ARRAY_NORMAL] = norms
	arr[Mesh.ARRAY_INDEX] = idx
	var mesh := ArrayMesh.new()
	mesh.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arr)
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.material_override = terrain_mat
	return mi

# ---------------------------------------------------------------- weather

## The server's weather (set in the console's God tab or by an event) is in /play/info; look every few seconds,
## like the play page, and draw it: the air thickens towards its colour, the sun dims, and streaks fall.
func _weather(delta: float) -> void:
	# Streaks start below the camera so none of them brushes past the lens.
	rain.global_position = Vector3(me_pos.x, _height(me_pos.x, me_pos.y) + minf(14.0, zoom * 0.5), me_pos.y)
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
	var e := env_res
	if w.is_empty():
		e.fog_density = clear_sky["fog"]
		e.fog_light_color = clear_sky["fog_color"]
		e.volumetric_fog_density = clear_sky["vol"]
		e.volumetric_fog_albedo = clear_sky["vol_albedo"]
		e.ambient_light_energy = clear_sky["ambient"]
		sun_light.light_energy = clear_sky["sun"]
		rain.emitting = false
		hud.say("[color=#9fe8e0]The sky clears.[/color]")
		return
	var k := clampf(float(w.get("intensity", 0.5)), 0.0, 1.0)
	var col := Color(str(w.get("color", "#8899aa")))
	e.fog_density = clear_sky["fog"] + 0.012 * k
	e.fog_light_color = (clear_sky["fog_color"] as Color).lerp(col, 0.6)
	e.volumetric_fog_density = clear_sky["vol"] + 0.02 * k
	e.volumetric_fog_albedo = (clear_sky["vol_albedo"] as Color).lerp(col, 0.5)
	e.ambient_light_energy = clear_sky["ambient"] * (1.0 - 0.35 * k)
	sun_light.light_energy = clear_sky["sun"] * (1.0 - 0.55 * k)
	var streak := (rain.draw_pass_1 as BoxMesh).material as StandardMaterial3D
	streak.albedo_color = Color(col.lightened(0.4), 0.35 + 0.3 * k)
	rain.amount_ratio = clampf(0.25 + k, 0.0, 1.0)
	rain.emitting = true
	hud.say("[color=#9fe8e0]Weather: %s.[/color]" % str(w.get("name", "")))

func _sync_ground() -> void:
	var seen := {}
	for g in net.ground():
		var id: int = g["id"]
		seen[id] = true
		if not gems.has(id):
			# Rare things (an item that stacks to 3 or fewer) lie in a gold chest.
			var rare := false
			for it in info.get("items", []):
				if it["name"] == g["item"]:
					rare = int(it.get("stack_max", 99)) <= 3
			var n := _gem_node(str(g["color"]), rare)
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
	var c := Vector3(me_pos.x, _height(me_pos.x, me_pos.y) + 1.0, me_pos.y)
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
## The world map (M): drawn once per world from the same land the 3D view uses; the markers update while open.
func _toggle_map() -> void:
	if not world_map.visible and not world_map.built:
		var kinds := []
		for b in boxes:
			kinds.append("house" if _is_house(b) else ["keep", "crag", "lake"][land.box_kind(b)])
		world_map.build(land, boxes, kinds, float(info.get("world_size", 1024)))
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
	_check("world drawn", boxes.size() >= 0 and ground_mesh.mesh != null, "%d boxes" % boxes.size())
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
	_check("walls near us are built", walls.size() > 0 or boxes.size() == 0, "%d meshes" % walls.size())
	var me: Node3D = ents.get(net.get_self_id())
	if ResourceLoader.exists("res://assets/characters/player.tscn"):
		_check("player is the KayKit character", me != null and me.has_method("set_speed") and me.anim != null and me.anim.has_animation(me.run), "")
	if not walls.is_empty() and ResourceLoader.exists(DUNGEON + "wall.gltf"):
		var batched := 0
		for chunk in walls.values():
			batched += (chunk as Node).find_children("*", "MultiMeshInstance3D", true, false).size()
		_check("walls are built from kit pieces", batched > 0, "%d batches" % batched)
	_check("scenery is scattered", props.size() > 0, "%d chunks" % props.size())
	if air_max > 0.0:
		net.act(ACT_JUMP, 0)
		ok = await _wait(3.0, func(): return track.get(net.get_self_id(), {}).get("air", 0.0) > 0.0)
		_check("jumps", ok, "")
		ok = await _wait(3.0, func(): return track.get(net.get_self_id(), {}).get("air", 1.0) == 0.0)
		_check("and lands", ok, "")
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
		ok = await _wait(10.0, func(): return weather.get("name", "") == "smoke-rain" and rain.emitting)
		_check("client shows the weather", ok, str(weather))
		await _post(admin_url + "/api/weather", {"name": null})
		weather_poll = 0.0
		ok = await _wait(10.0, func(): return weather.is_empty() and not rain.emitting)
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

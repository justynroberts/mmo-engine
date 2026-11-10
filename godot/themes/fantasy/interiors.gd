extends RefCounted
## Dungeon interiors (crates/server/src/interior.rs), drawn where the server lays them out: KayKit Dungeon floor
## tiles over the rooms and corridors, walls two storeys high along every wall, torches on the long walls, pillars,
## braziers and a gold chest in the boss's room, a lit arch over the way out, and a black floor of nothing round it
## all so the sea it stands in never shows. From /play/info realm dungeons' `interior`; built once, at setup.

const DUNGEON := "res://themes/fantasy/assets/dungeon/"
const PROPS := "res://themes/fantasy/assets/props/"
const STOREY := 4.0
const STOREYS := 2

var theme: Node   # the fantasy theme: its _mesh, _flame and _tint

## The walls between the camera and the player melt away (a dither), so the camera can follow them round the rooms
## from any side. `world_interior_eye` is the player's head, set each frame by the theme.
const CUTAWAY := """shader_type spatial;
uniform sampler2D tex : source_color, filter_linear_mipmap;
uniform vec4 tint : source_color = vec4(1.0);
global uniform vec3 world_interior_eye;
varying vec3 wpos;
void vertex() {
	wpos = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz;
}
void fragment() {
	vec3 cam = CAMERA_POSITION_WORLD;
	vec3 d = world_interior_eye - cam;
	float t = dot(wpos - cam, d) / max(dot(d, d), 0.001);
	if (t > 0.0 && t < 0.97 && wpos.y > world_interior_eye.y - 1.6) {
		float r = length((wpos - (cam + d * t)).xz);
		float n = fract(sin(dot(floor(FRAGCOORD.xy), vec2(12.9898, 78.233))) * 43758.5453);
		if (n < smoothstep(7.0, 3.5, r)) {
			discard;
		}
	}
	ALBEDO = texture(tex, UV).rgb * tint.rgb;
	ROUGHNESS = 0.92;
}"""
static var _cutaway: Shader

static func ensure_globals() -> void:
	if Engine.has_meta("interior_globals"):
		return
	Engine.set_meta("interior_globals", true)
	RenderingServer.global_shader_parameter_add("world_interior_eye", RenderingServer.GLOBAL_VAR_TYPE_VEC3, Vector3(0, -1e6, 0))

## One interior's node, its floor at height `ground`.
func build(t: Node, d: Dictionary, ground: float) -> Node3D:
	theme = t
	ensure_globals()
	var it: Dictionary = d["interior"]
	var node := Node3D.new()
	node.name = "interior_" + String(d.get("name", "")).validate_node_name()
	var pieces := {}
	var put := func(file: String, xf: Transform3D):
		if not pieces.has(file):
			pieces[file] = []
		pieces[file].append(xf)
	var rect := func(a: Array) -> Rect2:
		return Rect2(float(a[0]), float(a[1]), float(a[2]) - float(a[0]), float(a[3]) - float(a[1]))
	var rooms: Array = it.get("rooms", []).map(rect)
	var corridors: Array = it.get("corridors", []).map(rect)
	var bounds: Rect2 = rect.call(it["bounds"])
	# Nothing round it: a black ground far past the walls, just under the tiles, over the water and the sea bed.
	var nothing := MeshInstance3D.new()
	var vp := PlaneMesh.new()
	vp.size = bounds.size + Vector2(320, 320)
	nothing.mesh = vp
	nothing.position = Vector3(bounds.get_center().x, ground - 0.06, bounds.get_center().y)
	var vm := StandardMaterial3D.new()
	vm.albedo_color = Color(0.015, 0.012, 0.012)
	vm.roughness = 1.0
	vm.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	nothing.material_override = vm
	nothing.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	node.add_child(nothing)
	# Floors: large tiles stretched to fit each room and corridor exactly, a rocky one now and then.
	var tile: Mesh = theme._mesh(DUNGEON + "floor_tile_large.gltf")
	var tile_top: float = tile.get_aabb().end.y if tile else 0.0
	var k := 0
	for r: Rect2 in rooms + corridors:
		var tx := maxi(1, int(round(r.size.x / 4.0)))
		var ty := maxi(1, int(round(r.size.y / 4.0)))
		var b := Basis.from_scale(Vector3(r.size.x / tx / 4.0, 1, r.size.y / ty / 4.0))
		for j in ty:
			for i in tx:
				k += 1
				var at := Vector3(r.position.x + r.size.x * (i + 0.5) / tx, ground - tile_top, r.position.y + r.size.y * (j + 0.5) / ty)
				put.call("floor_tile_large_rocks.gltf" if (k * 7) % 11 == 0 else "floor_tile_large.gltf", Transform3D(b, at))
	# Walls: pieces along each wall's length, as thick as the wall, two storeys; a cracked one here and there.
	var wall: Mesh = theme._mesh(DUNGEON + "wall.gltf")
	var depth: float = wall.get_aabb().size.z if wall else 1.0
	var n := 0
	for a in it.get("walls", []):
		var w: Rect2 = rect.call(a)
		var along_x := w.size.x >= w.size.y
		var length := w.size.x if along_x else w.size.y
		var thick := w.size.y if along_x else w.size.x
		var count := maxi(1, int(round(length / 4.0)))
		var seg := length / count
		var rot := Basis(Vector3.UP, 0.0 if along_x else PI / 2.0)
		var shape := rot * Basis.from_scale(Vector3(seg / 4.0, 1.0, thick / maxf(depth, 0.1)))
		for i in count:
			var c := w.position + (Vector2(seg * (i + 0.5), thick / 2.0) if along_x else Vector2(thick / 2.0, seg * (i + 0.5)))
			for s in STOREYS:
				n += 1
				var piece := "wall_cracked.gltf" if n % 9 == 4 else "wall.gltf"
				put.call(piece, Transform3D(shape, Vector3(c.x, ground + STOREY * s, c.y)))
	# Torches on each room's long north and south walls, facing in; rubble and stores about the ground.
	var lit := Node3D.new()
	node.add_child(lit)
	for ri in rooms.size():
		var r: Rect2 = rooms[ri]
		var each := clampi(int(r.size.x / 14.0), 1, 3)
		for side in [0, 1]:
			for e in each:
				var x := r.position.x + r.size.x * (e + 1.0) / (each + 1.0)
				var z := r.position.y + 0.15 if side == 0 else r.end.y - 0.15
				var xf := Transform3D(Basis(Vector3.UP, 0.0 if side == 0 else PI), Vector3(x, ground + 2.6, z))
				put.call("torch_mounted.gltf", xf)
				theme._flame(lit, xf * Vector3(0, 0.75, 0.35), 0.14)
		var rng := RandomNumberGenerator.new()
		rng.seed = int(r.position.x * 31.0 + r.position.y * 17.0)
		var bits := ["rubble_large.gltf", "rubble_half.gltf", "rubble_half.gltf", "barrel_small.gltf", "crates_stacked.gltf"]
		for b in 2 + rng.randi() % 3:
			var along := rng.randf()
			var at := Vector3(r.position.x + r.size.x * (0.3 + 0.4 * along), ground, (r.position.y + 2.5) if rng.randf() < 0.5 else (r.end.y - 2.5))
			put.call(bits[rng.randi() % bits.size()], Transform3D(Basis(Vector3.UP, rng.randf() * TAU), at))
		_furnish(put, r, ri, rooms.size(), ground, rng)
	# The boss's room: pillars two storeys high in from its corners, braziers, banners and a chest at the far end.
	if not rooms.is_empty():
		var br: Rect2 = rooms[rooms.size() - 1]
		# A ring of pillars round the middle, where the boss stands, and braziers between them.
		var ring := minf(br.size.x, br.size.y) * 0.3
		var pm: Mesh = theme._mesh(DUNGEON + "pillar.gltf")
		var tall: float = pm.get_aabb().size.y if pm else STOREY
		for i in 6:
			var a := TAU * i / 6.0 + PI / 6.0
			var c := br.get_center() + Vector2(cos(a), sin(a)) * ring
			put.call("pillar.gltf", Transform3D(Basis(), Vector3(c.x, ground, c.y)))
			put.call("pillar_decorated.gltf", Transform3D(Basis(), Vector3(c.x, ground + tall, c.y)))
		for i in 3:
			var a := TAU * i / 3.0
			var c := br.get_center() + Vector2(cos(a), sin(a)) * ring * 0.62
			put.call("torch_lit.gltf", Transform3D(Basis.from_scale(Vector3.ONE * 2.0), Vector3(c.x, ground + 0.85, c.y)))
			theme._flame(lit, Vector3(c.x, ground + 0.85 + 1.5, c.y), 0.25)
		var far := Vector3(br.end.x - 3.5, ground, br.get_center().y)
		put.call("chest_gold.gltf", Transform3D(Basis(Vector3.UP, -PI / 2.0), far))
		put.call("candle_triple.gltf", Transform3D(Basis(), far + Vector3(0, 0, 2.2)))
		put.call("candle_triple.gltf", Transform3D(Basis(), far + Vector3(0, 0, -2.2)))
		put.call("banner_triple_red.gltf", Transform3D(Basis(Vector3.UP, -PI / 2.0), Vector3(br.end.x - 0.2, ground + 1.0, br.get_center().y - 6.0)))
		put.call("banner_triple_red.gltf", Transform3D(Basis(Vector3.UP, -PI / 2.0), Vector3(br.end.x - 0.2, ground + 1.0, br.get_center().y + 6.0)))
		# The hoard round the chest: coin, a trunk either side, a gilded sword and shield, the keys.
		var y := Basis(Vector3.UP, -PI / 2.0)
		put.call("coin_stack_large.gltf", Transform3D(y, far + Vector3(-1.8, 0, 0.9)))
		put.call("coin_stack_medium.gltf", Transform3D(y.rotated(Vector3.UP, 0.7), far + Vector3(-1.5, 0, -1.1)))
		put.call("coin_stack_medium.gltf", Transform3D(y.rotated(Vector3.UP, 2.1), far + Vector3(0.6, 0, 3.4)))
		put.call("trunk_medium_A.gltf", Transform3D(y.rotated(Vector3.UP, 0.25), far + Vector3(0.4, 0, 4.4)))
		put.call("trunk_small_C.gltf", Transform3D(y.rotated(Vector3.UP, -0.3), far + Vector3(0.4, 0, -4.4)))
		put.call("sword_shield_gold.gltf", Transform3D(y, far + Vector3(-0.6, 0, -3.3)))
		put.call("keyring.gltf", Transform3D(Basis(), far + Vector3(-2.4, 0, -0.2)))
		var red := OmniLight3D.new()
		red.light_color = Color("ff5a3c")
		red.light_energy = 3.0
		red.omni_range = br.size.x * 0.75
		red.position = Vector3(br.get_center().x, ground + 8.0, br.get_center().y)
		lit.add_child(red)
	# The way out: an arch against the first room's west wall over a pool of pale light.
	var ex: Array = it.get("exit", [])
	if ex.size() == 2:
		var e := Vector3(float(ex[0]), ground, float(ex[1]))
		var west: float = rooms[0].position.x if not rooms.is_empty() else e.x - 5.0
		put.call("wall_arched.gltf", Transform3D(Basis(Vector3.UP, PI / 2.0), Vector3(west + 0.4, ground, e.z)))
		for side in [-1.0, 1.0]: # torches either side of the way out
			var txf := Transform3D(Basis(Vector3.UP, PI / 2.0), Vector3(west + 0.15, ground + 2.6, e.z + side * 3.2))
			put.call("torch_mounted.gltf", txf)
			theme._flame(lit, txf * Vector3(0, 0.75, 0.35), 0.14)
		var glow := MeshInstance3D.new()
		var disc := CylinderMesh.new()
		disc.top_radius = 2.6
		disc.bottom_radius = 2.6
		disc.height = 0.04
		glow.mesh = disc
		var gm := StandardMaterial3D.new()
		gm.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
		gm.albedo_color = Color(0.45, 0.85, 1.0, 0.3)
		gm.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		gm.emission_enabled = true
		gm.emission = Color(0.4, 0.8, 1.0)
		gm.emission_energy_multiplier = 1.5
		glow.material_override = gm
		glow.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		glow.position = e + Vector3(0, 0.03, 0)
		node.add_child(glow)
		var l := OmniLight3D.new()
		l.light_color = Color("9fdcff")
		l.light_energy = 2.2
		l.omni_range = 10.0
		l.position = e + Vector3(1.0, 2.5, 0)
		lit.add_child(l)
	for file in pieces:
		var path := String(file) if String(file).begins_with("res://") else DUNGEON + String(file)
		var m: Mesh = theme._mesh(path)
		if m == null:
			continue
		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = m
		var xs: Array = pieces[file]
		mm.instance_count = xs.size()
		for i in xs.size():
			mm.set_instance_transform(i, xs[i])
		var mmi := MultiMeshInstance3D.new()
		mmi.multimesh = mm
		var tint: Color = theme._tint(path)
		var src: StandardMaterial3D = m.surface_get_material(0) as StandardMaterial3D if m.get_surface_count() > 0 else null
		if String(file).begins_with("wall") and src and src.albedo_texture:
			if _cutaway == null:
				_cutaway = Shader.new()
				_cutaway.code = CUTAWAY
			var cut := ShaderMaterial.new()
			cut.shader = _cutaway
			cut.set_shader_parameter("tex", src.albedo_texture)
			cut.set_shader_parameter("tint", tint * 0.8)
			mmi.material_override = cut
		elif tint != Color.WHITE and src:
			var mat := src.duplicate() as StandardMaterial3D
			mat.albedo_color = tint * 0.8 # the cave's stone, darker than a keep's in daylight
			mmi.material_override = mat
		node.add_child(mmi)
	return node

## A room's corners: a guardroom's table with its chairs and leavings, a store's shelves and trunks, or a hall
## fallen in (a heap of wall, columns standing and down, a broken table); banners either side of the way on.
## Kept out of the band across the middle, where the corridors come in. The boss's room (`last`) gets a fallen
## corner only; its hoard is by the chest. Seeded by the room, so every client lays it out the same.
func _furnish(put: Callable, r: Rect2, ri: int, count: int, ground: float, rng: RandomNumberGenerator) -> void:
	if r.size.x < 18.0 or r.size.y < 16.0:
		return
	var last := ri == count - 1
	var corners := [[0, 1.0], [1, -1.0], [1, 1.0], [0, -1.0]] # [west (0) or east (1), north (1) or south (-1)]
	var first := rng.randi() % 4
	var kinds: Array = ["ruin"] if last else (["table", "store"] if rng.randf() < 0.5 else ["table", "ruin"])
	if not last and rng.randf() < 0.3:
		kinds = ["store", "ruin"]
	for k in kinds.size():
		var kind: String = kinds[k]
		var c: Array = corners[(first + k) % 4]               # the next corner round: across the room from the last
		var s: float = c[1]                                   # into the room from its wall
		var wall_z := r.position.y if s > 0 else r.end.y
		var cx := (r.position.x + 5.0) if c[0] == 0 else (r.end.x - 5.0)
		var at := Vector3(cx, ground, wall_z + s * 3.4)
		var face := 0.0 if s > 0 else PI                      # a piece's front (+z) turned into the room
		var inward := 1.0 if c[0] == 0 else -1.0              # towards the room's middle, along x
		match kind:
			"table":
				var cloth: String = ["table_long.gltf", "table_long_decorated_A.gltf", "table_long_tablecloth.gltf"][rng.randi() % 3]
				put.call(cloth, Transform3D(Basis(Vector3.UP, PI / 2.0), at))
				for side in [-1.0, 1.0]:
					for dx in [-1.0, 1.0]:
						if rng.randf() < 0.8:
							var chair_at := at + Vector3(dx + rng.randf_range(-0.15, 0.15), 0, side * 1.45)
							put.call("chair.gltf", Transform3D(Basis(Vector3.UP, (0.0 if side < 0 else PI) + rng.randf_range(-0.3, 0.3)), chair_at))
				var top: Array = ["plate_food_B.gltf", "plate.gltf", "bottle_A_green.gltf", "bottle_B_brown.gltf", "plate_small.gltf", "bottle_C_brown.gltf"]
				for dx in [-1.3, -0.4, 0.5, 1.4]:
					put.call(top[rng.randi() % top.size()], Transform3D(Basis(Vector3.UP, rng.randf() * TAU), at + Vector3(dx, 1.0, rng.randf_range(-0.35, 0.35))))
				put.call("candle_lit.gltf", Transform3D(Basis(), at + Vector3(0.0, 1.0, 0.0)))
				put.call("sword_shield.gltf", Transform3D(Basis(Vector3.UP, face + PI / 2.0), at + Vector3(inward * 2.9, 0, 0)))
			"store":
				for dx in [-1.05, 1.05]:
					put.call("shelves.gltf", Transform3D(Basis(Vector3.UP, face), Vector3(cx + dx, ground, wall_z + s * 0.05)))
				put.call("trunk_medium_A.gltf", Transform3D(Basis(Vector3.UP, face + rng.randf_range(-0.3, 0.3)), at + Vector3(-1.2, 0, s * -0.4)))
				put.call("trunk_small_A.gltf", Transform3D(Basis(Vector3.UP, face + rng.randf_range(-0.5, 0.5)), at + Vector3(0.6, 0, s * -0.2)))
				put.call("box_small_decorated.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU), at + Vector3(inward * 2.4, 0, s * 0.6)))
				put.call("barrel_small.gltf", Transform3D(Basis(), at + Vector3(inward * 3.4, 0, s * -0.9)))
				# A stores room is one of three: an alchemist's (cauldron, bottles), a scholar's (books) or an armoury.
				var props := Basis.from_scale(Vector3.ONE * 1.3)
				match rng.randi() % 3:
					0:
						put.call(PROPS + "Cauldron.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU) * props, at + Vector3(inward * 0.6, 0, s * 1.6)))
						put.call(PROPS + "Shelf_Small_Bottles.gltf", Transform3D(Basis(Vector3.UP, face) * props, Vector3(cx + inward * 3.0, ground + 1.2, wall_z + s * 0.2)))
						put.call(PROPS + "Potion_2.gltf", Transform3D(props, at + Vector3(inward * 2.4, 0.95, s * 0.6)))
					1:
						put.call(PROPS + "Bookcase_2.gltf", Transform3D(Basis(Vector3.UP, face) * props, Vector3(cx + inward * 3.4, ground, wall_z + s * 0.3)))
						put.call(PROPS + "BookStand.gltf", Transform3D(Basis(Vector3.UP, face + PI) * props, at + Vector3(inward * 0.8, 0, s * 1.8)))
						put.call(PROPS + "Book_Stack_2.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU) * props, at + Vector3(inward * 2.4, 0.95, s * 0.6)))
					_:
						put.call(PROPS + "WeaponStand.gltf", Transform3D(Basis(Vector3.UP, face) * props, at + Vector3(inward * 0.6, 0, s * 1.4)))
						put.call(PROPS + "Shield_Wooden.gltf", Transform3D(Basis(Vector3.UP, face) * Basis(Vector3.RIGHT, -0.3) * props, Vector3(cx - inward * 2.2, ground + 0.4, wall_z + s * 0.35)))
						put.call(PROPS + "Crate_Metal.gltf", Transform3D(Basis(Vector3.UP, rng.randf_range(-0.3, 0.3)) * props, at + Vector3(-inward * 2.6, 0, s * 1.2)))
			"ruin":
				put.call("wall_broken.gltf", Transform3D(Basis(Vector3.UP, face).scaled(Vector3.ONE * 0.65), at + Vector3(0, -0.4, s * -1.2)))
				put.call("column.gltf", Transform3D(Basis().scaled(Vector3.ONE * 1.6), at + Vector3(inward * 2.6, 0, s * 0.4)))
				var down := Basis(Vector3.UP, rng.randf() * TAU) * Basis(Vector3.RIGHT, PI / 2.0)
				put.call("column.gltf", Transform3D(down.scaled(Vector3.ONE * 1.6), at + Vector3(-inward * 1.6, 0.55, s * 1.4)))
				if not last:
					put.call("table_long_broken.gltf", Transform3D(Basis(Vector3.UP, PI / 2.0 + rng.randf_range(-0.4, 0.4)), at + Vector3(inward * 0.8, 0, s * 2.2)))
				put.call("sword_shield_broken.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU), at + Vector3(inward * 1.6, 0, s * 3.2)))
				put.call(PROPS + "Vase_Rubble_Medium.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * 1.3), at + Vector3(-inward * 0.4, 0, s * 2.8)))
				if rng.randf() < 0.5:
					put.call(PROPS + "Cage_Small.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * 1.3), at + Vector3(inward * 3.4, 0, s * 1.0)))
				put.call(PROPS + "Chain_Coil.gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * 1.3), at + Vector3(inward * 2.2, 0.02, s * -0.6)))
	# Banners either side of the way on into the next room.
	if not last and r.size.y >= 18.0:
		var colours: Array = ["banner_blue.gltf", "banner_green.gltf", "banner_brown.gltf", "banner_yellow.gltf", "banner_white.gltf", "banner_triple_blue.gltf", "banner_patternC_green.gltf", "banner_shield_yellow.gltf"]
		var b: String = colours[(ri * 3 + int(r.position.y)) % colours.size()]
		for dz in [-6.0, 6.0]:
			put.call(b, Transform3D(Basis(Vector3.UP, -PI / 2.0), Vector3(r.end.x - 0.2, ground + 1.0, r.get_center().y + dz)))

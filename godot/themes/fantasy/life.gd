extends Node3D
## Life round the player that only this client sees, so it costs the server nothing: birds pecking on open ground
## that scatter when you run at them, gulls over the coast, deer at the edge of the woods that bolt, sheep and
## cattle grazing by farms, smoke from the nearest chimneys and fireflies over water on a dry night. Everything is
## a fixed pool placed round the camera and moved on as it goes; nothing is made or freed while playing.
##
## The fantasy theme makes this in setup and calls step() every frame. It also eases how wet the ground is and how
## much snow lies (the theme's weather), as global shader values the ground, roofs and blocks read.

var theme: Node3D          # the fantasy theme: height(), world, darkness, the weather (_rain_kind, _rain_ratio, _wx)
var low := false           # Graphics: Low (fewer of everything)

const NEAR := 120.0        # how far round the camera things live
const SURVEY_SECS := 1.5   # how often the land round us is looked over again
const FLOCKS := 6
const PER_FLOCK := 7
const DEER := 8
const STOCK := 24
const SMOKES := 8

var _center := Vector2.ZERO
var _last_center := Vector2.INF
var _speed := 0.0          # how fast the player is moving, units a second
var _survey_in := 0.0
var _survey_at := Vector2.INF
var _clock := 0.0
var _rng := RandomNumberGenerator.new()

# What the last survey found round us.
var _houses: Array = []    # [Rect2, ...]
var _farms: Array = []     # [[Vector2 pen centre, seed int, cattle bool], ...]
var _woods: Array = []     # [Vector2, ...] ground beside trees, well away from houses
var _shore := Vector2.INF  # the nearest land beside the sea
var _water := Vector2.INF  # the nearest water of any kind (fireflies)

# Weather on the ground, eased.
var wet := 0.0             # how wet the ground is: darker, glossier
var puddles := 0.0         # standing water on roads and in hollows, slower to go than the wet
var snow := 0.0            # how much snow lies on roofs, tops and ground
var season_snow := 0.0     # snow the season leaves lying whatever the sky does (winter), set by the theme
var season_autumn := 0.0   # the meadows' turn to gold (autumn), and how far it has gone
var autumn := 0.0
var _snow_shown := false

# ---------------------------------------------------------------------------- birds
var _bird_body: MultiMeshInstance3D
var _bird_wing: MultiMeshInstance3D
var _flocks: Array = []    # per flock: {mode, home: Vector2, t, gull, circle: Vector3, land: Vector2}
var _bird_pos := PackedVector3Array()
var _bird_vel := PackedVector3Array()
var _bird_yaw := PackedFloat32Array()
var _bird_phase := PackedFloat32Array()
enum { OFF, PERCHED, FLYING, LANDING }

# ---------------------------------------------------------------------------- deer and stock
var _deer: Array = []      # per deer: {node, neck, legs: [Node3D], mode, at: Vector2, to: Vector2, t, yaw, away: Vector2}
var _stock: Array = []     # per animal: {node, neck, legs, pen: int (index in _pens) or -1, at, to, t, yaw, cattle}
var _pens := {}            # seed -> [Vector2 centre, cattle bool, [animal indices]]

# ---------------------------------------------------------------------------- smoke and fireflies
var _smoke: Array[GPUParticles3D] = []
var chimneys := {}         # cell key -> [Vector3, ...] chimney tops (the theme fills it as it builds cells)
var _flies: GPUParticles3D

## The global shader values the ground, roofs and blocks read; added once a run (the list of them is editor-only).
static func ensure_globals() -> void:
	if Engine.has_meta("life_globals"):
		return
	Engine.set_meta("life_globals", true)
	for n in ["world_wet", "world_puddles", "world_snow", "world_autumn"]:
		RenderingServer.global_shader_parameter_add(n, RenderingServer.GLOBAL_VAR_TYPE_FLOAT, 0.0)
	# land.gd: the band of latitude where the north turns cold (set by the theme once it knows the world).
	RenderingServer.global_shader_parameter_add("world_cold_band", RenderingServer.GLOBAL_VAR_TYPE_VEC2, Vector2(-1e9, -1e9))

var seen := {}              # what the wildlife has done ("scatter", "crows", "bolt", "graze", "leap"): times, for the screenshot run
var _placing: Array = []    # what is left to place after a survey, one a frame

func setup(p_theme: Node3D) -> void:
	theme = p_theme
	_rng.seed = 7151
	_make_birds()
	for i in DEER:
		var d := _make_deer(i % 3 == 0)
		d["node"].visible = false
		add_child(d["node"])
		_deer.append(d)
	for i in STOCK:
		var cattle := i % 3 == 0
		var a := _make_stock(cattle)
		a["node"].visible = false
		add_child(a["node"])
		_stock.append(a)
	for i in SMOKES:
		var s := _make_smoke()
		add_child(s)
		_smoke.append(s)
	_flies = _make_flies()
	add_child(_flies)

## How many of each are about (for the smoke test).
func _saw(what: String) -> void:
	seen[what] = int(seen.get(what, 0)) + 1

func counts() -> Dictionary:
	var birds := 0
	for f in _flocks:
		if f["mode"] != OFF:
			birds += PER_FLOCK
	var deer := _deer.filter(func(d): return d["node"].visible).size()
	var stock := _stock.filter(func(a): return a["node"].visible).size()
	var smoke := _smoke.filter(func(s): return s.emitting).size()
	return {"birds": birds, "deer": deer, "stock": stock, "smoke": smoke, "flies": _flies.emitting}

func step(center: Vector2, dt: float) -> void:
	if dt <= 0.0:
		return
	_clock += dt
	low = get_viewport().scaling_3d_scale < 1.0
	if _last_center != Vector2.INF:
		var moved := center.distance_to(_last_center)
		_speed = lerpf(_speed, (moved / dt) if moved < 20.0 else 0.0, minf(1.0, dt * 6.0))
	_last_center = center
	_center = center
	_survey_in -= dt
	if _survey_in <= 0.0 or _survey_at == Vector2.INF or center.distance_to(_survey_at) > 24.0:
		_survey_in = SURVEY_SECS
		_survey_at = center
		var t := Time.get_ticks_usec()
		_survey()
		theme.lap("life survey", t)
		_placing = [_place_flocks, _place_deer, _place_stock, _place_smoke]
	elif not _placing.is_empty():
		var t := Time.get_ticks_usec()
		var f: Callable = _placing.pop_front()
		f.call() # one a frame, so looking round never stalls a frame
		theme.lap("life " + f.get_method(), t)
	_birds(dt)
	_fish_step(dt)
	_deer_step(dt)
	_stock_step(dt)
	_smoke_frame()
	_flies_frame()
	_ground_weather(dt)

# ================================================================== looking over the land

func _h(x: float, y: float) -> float:
	return theme.height(x, y)

func _sea_at(p: Vector2) -> bool:
	return theme.world.has_sea and _h(p.x, p.y) < theme.world.sea_level - 0.2

func _lake_at(p: Vector2) -> bool:
	for l in theme.world.lakes:
		var b: Array = l["b"]
		if p.x > float(b[0]) - 2.0 and p.x < float(b[2]) + 2.0 and p.y > float(b[1]) - 2.0 and p.y < float(b[3]) + 2.0:
			return true
	return false

func _near_house(p: Vector2, gap: float) -> bool:
	for r in _houses:
		if (r as Rect2).grow(gap).has_point(p):
			return true
	return false

func _on_road(p: Vector2, gap: float) -> bool:
	var segs: Array = theme.world.road_segments(int(floor(p.x / theme.world.CELL)), int(floor(p.y / theme.world.CELL)))
	for s in segs:
		if Geometry2D.get_closest_point_to_segment(p, Vector2(s[0], s[1]), Vector2(s[2], s[3])).distance_to(p) < float(s[4]) + gap:
			return true
	return false

## Somewhere a creature can stand: dry land, off the roads, clear of houses, not too steep.
func _open(p: Vector2) -> bool:
	if _sea_at(p) or _lake_at(p) or theme.world.wet(p.x, p.y) or _near_house(p, 3.0) or _on_road(p, 1.5):
		return false
	var e := Rect2(theme.world.extent())
	if not e.grow(-4.0).has_point(p):
		return false
	var h := _h(p.x, p.y)
	return absf(_h(p.x + 2.0, p.y) - h) < 1.2 and absf(_h(p.x, p.y + 2.0) - h) < 1.2

func _survey() -> void:
	_houses.clear()
	_farms.clear()
	_woods.clear()
	var world = theme.world
	var cx := int(floor(_center.x / world.CELL))
	var cy := int(floor(_center.y / world.CELL))
	var trees: Array = []
	for i in range(cx - 1, cx + 2):
		for j in range(cy - 1, cy + 2):
			for o in world.in_cell(i, j):
				var t := String(o.get("t", ""))
				if t == "house":
					var b: Array = o["b"]
					_houses.append(Rect2(float(b[0]), float(b[1]), float(b[2]) - float(b[0]), float(b[3]) - float(b[1])))
				elif t == "tree" and o.has("p"):
					var p := Vector2(float(o["p"][0]), float(o["p"][1]))
					if p.distance_to(_center) < NEAR + 20.0:
						trees.append(p)
	# A farm is a house with few others round it (a town has many: main.gd hears a town from six).
	for r in _houses:
		var c: Vector2 = (r as Rect2).get_center()
		if c.distance_to(_center) > NEAR + 30.0:
			continue
		var close := 0
		for q in _houses:
			if (q as Rect2).get_center().distance_to(c) < 45.0:
				close += 1
		if close > 4:
			continue
		var sd := int(c.x) * 7919 + int(c.y)
		var prng := RandomNumberGenerator.new()
		prng.seed = sd
		var cattle := prng.randi() % 3 == 0
		for _k in 8:
			var a := prng.randf() * TAU
			var pen := c + Vector2(cos(a), sin(a)) * prng.randf_range(13.0, 22.0)
			if _open(pen) and _open(pen + Vector2(5, 0)) and _open(pen - Vector2(5, 0)):
				_farms.append([pen, sd, cattle])
				break
	# Woods: open ground beside a tree that has a few others near it, well away from houses.
	for p in trees:
		if _woods.size() >= 12:
			break
		if _near_house(p, 50.0):
			continue
		var a := _rng.randf() * TAU
		var spot: Vector2 = p + Vector2(cos(a), sin(a)) * 5.0
		if _open(spot):
			_woods.append(spot)
	# The sea's edge and the nearest water.
	_shore = Vector2.INF
	_water = Vector2.INF
	var best := INF
	if world.has_sea:
		for k in 16:
			var a := TAU * k / 16.0
			var dir := Vector2(cos(a), sin(a))
			var prev := _center
			for r in range(8, int(NEAR), 8):
				var p := _center + dir * r
				if _sea_at(p):
					var d := float(r)
					if d < best:
						best = d
						_shore = prev
						_water = p
					break
				prev = p
	for rv in world.rivers:
		var pts: Array = rv["pts"]
		for k in pts.size() - 1:
			var a := Vector2(pts[k][0], pts[k][1])
			var b := Vector2(pts[k + 1][0], pts[k + 1][1])
			if a.distance_to(_center) > NEAR * 2.0 and b.distance_to(_center) > NEAR * 2.0:
				continue
			var q := Geometry2D.get_closest_point_to_segment(_center, a, b)
			var d := q.distance_to(_center)
			if d < best:
				best = d
				_water = q
	for l in world.lakes:
		var b: Array = l["b"]
		var q := Vector2(clampf(_center.x, float(b[0]), float(b[2])), clampf(_center.y, float(b[1]), float(b[3])))
		var d := q.distance_to(_center)
		if d < best:
			best = d
			_water = q

## Somewhere open near us, out of the way: past `from` units and inside NEAR.
## Somewhere in a farm field round us (land.gd), at least `from` away, or INF.
func _field_spot(from: float) -> Vector2:
	if theme.land == null or not theme.land.has_method("fields_in"):
		return Vector2.INF
	var fields: Array = theme.land.fields_in(Rect2(_center - Vector2(NEAR, NEAR), Vector2(NEAR, NEAR) * 2.0))
	fields.shuffle()
	for f in fields:
		var r: Rect2 = f[0]
		var p := r.position + Vector2(_rng.randf(), _rng.randf()) * r.size
		if p.distance_to(_center) >= from and p.distance_to(_center) < NEAR * 0.85 and _open(p):
			return p
	return Vector2.INF

func _spot(from: float) -> Vector2:
	for _k in 12:
		var a := _rng.randf() * TAU
		var p := _center + Vector2(cos(a), sin(a)) * _rng.randf_range(from, NEAR * 0.85)
		if _open(p):
			return p
	return Vector2.INF

# ================================================================== fish

var _fish: MeshInstance3D
var _fish_t := -1.0         # how far through its leap a fish is (0..1), -1 for none in the air
var _fish_wait := 4.0       # seconds till the next leaps
var _fish_at := Vector3.ZERO
var _fish_dir := Vector2.RIGHT
var _splash: CPUParticles3D

## Now and then a fish leaps from the sea off the shore near us: an arc, a flash of silver, a ring of spray.
func _fish_step(dt: float) -> void:
	var world = theme.world
	if _fish == null:
		_fish = MeshInstance3D.new()
		var m := CapsuleMesh.new()
		m.radius = 0.09
		m.height = 0.5
		_fish.mesh = m
		var mat := StandardMaterial3D.new()
		mat.albedo_color = Color("b8c4cc")
		mat.metallic = 0.6
		mat.roughness = 0.3
		_fish.material_override = mat
		_fish.visible = false
		add_child(_fish)
		_splash = CPUParticles3D.new()
		_splash.emitting = false
		_splash.one_shot = true
		_splash.amount = 18
		_splash.lifetime = 0.7
		_splash.explosiveness = 0.9
		_splash.direction = Vector3.UP
		_splash.spread = 40.0
		_splash.initial_velocity_min = 1.5
		_splash.initial_velocity_max = 3.0
		_splash.gravity = Vector3(0, -9.8, 0)
		_splash.scale_amount_min = 0.05
		_splash.scale_amount_max = 0.1
		var drop := SphereMesh.new()
		drop.radius = 0.5
		drop.height = 1.0
		var dm := StandardMaterial3D.new()
		dm.albedo_color = Color(0.85, 0.92, 1.0, 0.8)
		dm.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
		drop.material = dm
		_splash.mesh = drop
		add_child(_splash)
	if _fish_t >= 0.0:
		_fish_t += dt / 0.8
		if _fish_t >= 1.0:
			_fish_t = -1.0
			_fish.visible = false
			_splash.position = _fish_at + Vector3(_fish_dir.x, 0, _fish_dir.y) * 1.6
			_splash.restart()
			_saw("leap")
			return
		var along := Vector3(_fish_dir.x, 0, _fish_dir.y) * (_fish_t * 1.6)
		_fish.position = _fish_at + along + Vector3(0, sin(_fish_t * PI) * 0.9, 0)
		_fish.rotation = Vector3(0, atan2(_fish_dir.x, _fish_dir.y), 0)
		_fish.rotate_object_local(Vector3.RIGHT, PI / 2.0 + (_fish_t - 0.5) * 2.2)
		return
	_fish_wait -= dt
	if _fish_wait > 0.0:
		return
	_fish_wait = _rng.randf_range(5.0, 14.0)
	if not world.has_sea or _shore == Vector2.INF or _shore.distance_to(_center) > 70.0:
		return
	# Out from the shore, over water.
	var out := (_shore - _center).normalized()
	if out == Vector2.ZERO:
		return
	var p := _shore + out * _rng.randf_range(5.0, 16.0) + Vector2(-out.y, out.x) * _rng.randf_range(-10.0, 10.0)
	if not _sea_at(p):
		return
	_fish_at = Vector3(p.x, float(world.sea_level), p.y)
	var a := _rng.randf() * TAU
	_fish_dir = Vector2(cos(a), sin(a))
	_fish_t = 0.0
	_fish.visible = true
	_splash.position = _fish_at
	_splash.restart()

# ================================================================== birds

func _v_mesh(wing: bool) -> ArrayMesh:
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	if wing:
		# Two wings from the body's line out to their tips; flapping moves the tips up and down (scale y).
		for s in [-1.0, 1.0]:
			st.add_vertex(Vector3(0, 0, 0.09))
			st.add_vertex(Vector3(s * 0.36, 0.1, -0.02))
			st.add_vertex(Vector3(0, 0, -0.07))
	else:
		# A small body with a head forward: a stretched tetrahedron.
		var tip := Vector3(0, 0.02, -0.16)
		var tail := Vector3(0, 0.03, 0.15)
		var l := Vector3(-0.05, 0.0, 0.0)
		var r := Vector3(0.05, 0.0, 0.0)
		var top := Vector3(0, 0.08, -0.02)
		for tri in [[tip, l, top], [tip, top, r], [tail, top, l], [tail, r, top], [tip, r, l], [tail, l, r]]:
			for v in tri:
				st.add_vertex(v)
	st.generate_normals()
	return st.commit()

func _make_birds() -> void:
	var n := FLOCKS * PER_FLOCK
	var mat := StandardMaterial3D.new()
	mat.vertex_color_use_as_albedo = true
	mat.cull_mode = BaseMaterial3D.CULL_DISABLED
	mat.roughness = 1.0
	for wing in [false, true]:
		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.use_colors = true
		mm.mesh = _v_mesh(wing)
		mm.instance_count = n
		var mmi := MultiMeshInstance3D.new()
		mmi.multimesh = mm
		mmi.material_override = mat
		mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		mmi.custom_aabb = AABB(Vector3(-1e5, -1e3, -1e5), Vector3(2e5, 2e3, 2e5)) # moved every frame: never culled
		add_child(mmi)
		if wing:
			_bird_wing = mmi
		else:
			_bird_body = mmi
	_bird_pos.resize(n)
	_bird_vel.resize(n)
	_bird_yaw.resize(n)
	_bird_phase.resize(n)
	for i in n:
		_bird_phase[i] = _rng.randf() * TAU
	for f in FLOCKS:
		_flocks.append({"mode": OFF, "home": Vector2.ZERO, "t": 0.0, "gull": f >= FLOCKS - 2, "circle": Vector3.ZERO, "land": Vector2.ZERO})
		var c := Color("e8e8e4") if f >= FLOCKS - 2 else Color("2a2622").lerp(Color("6b5a44"), _rng.randf() * 0.6)
		for b in PER_FLOCK:
			_bird_body.multimesh.set_instance_color(f * PER_FLOCK + b, c)
			_bird_wing.multimesh.set_instance_color(f * PER_FLOCK + b, c)
			_bird_body.multimesh.set_instance_transform(f * PER_FLOCK + b, Transform3D(Basis.from_scale(Vector3.ONE * 0.001), Vector3(0, -500, 0)))
			_bird_wing.multimesh.set_instance_transform(f * PER_FLOCK + b, Transform3D(Basis.from_scale(Vector3.ONE * 0.001), Vector3(0, -500, 0)))

func _perch(f: int, home: Vector2) -> void:
	var fl: Dictionary = _flocks[f]
	fl["mode"] = PERCHED
	fl["home"] = home
	fl["t"] = 0.0
	for b in PER_FLOCK:
		var i := f * PER_FLOCK + b
		var p := home + Vector2(_rng.randf_range(-2.5, 2.5), _rng.randf_range(-2.5, 2.5))
		_bird_pos[i] = Vector3(p.x, _h(p.x, p.y), p.y)
		_bird_vel[i] = Vector3.ZERO
		_bird_yaw[i] = _rng.randf() * TAU

func _place_flocks() -> void:
	var land_flocks := 2 if low else FLOCKS - 2
	for f in FLOCKS:
		var fl: Dictionary = _flocks[f]
		if fl["gull"]:
			# Gulls wheel over the shore when there is one near.
			if _shore == Vector2.INF or (low and f == FLOCKS - 1):
				fl["mode"] = OFF
				continue
			if fl["mode"] == OFF or Vector2(fl["circle"].x, fl["circle"].z).distance_to(_shore) > 60.0:
				var at: Vector2 = _shore + Vector2(_rng.randf_range(-20, 20), _rng.randf_range(-20, 20))
				fl["circle"] = Vector3(at.x, _h(_shore.x, _shore.y) + _rng.randf_range(7.0, 12.0), at.y)
				if fl["mode"] == OFF:
					for b in PER_FLOCK:
						var i := f * PER_FLOCK + b
						_bird_pos[i] = fl["circle"] + Vector3(_rng.randf_range(-8, 8), _rng.randf_range(-2, 2), _rng.randf_range(-8, 8))
				fl["mode"] = FLYING
			continue
		if f >= land_flocks:
			fl["mode"] = OFF
			continue
		# A perched flock left far behind (or none yet) settles somewhere new round us, out of sight.
		var gone: bool = fl["mode"] == OFF or (fl["mode"] == PERCHED and (fl["home"] as Vector2).distance_to(_center) > NEAR)
		if gone:
			# The first flock is crows, and crows go to the fields when there are any round us.
			var p := _field_spot(35.0) if f == 0 else Vector2.INF
			if p != Vector2.INF:
				_saw("crows")
			if p == Vector2.INF:
				p = _spot(35.0)
			if p != Vector2.INF:
				_perch(f, p)
			else:
				fl["mode"] = OFF

func _birds(dt: float) -> void:
	var body := _bird_body.multimesh
	var wing := _bird_wing.multimesh
	var me := Vector3(_center.x, _h(_center.x, _center.y), _center.y)
	for f in FLOCKS:
		var fl: Dictionary = _flocks[f]
		var mode: int = fl["mode"]
		fl["t"] = float(fl["t"]) + dt
		if mode == PERCHED:
			# Running at them (or walking right up) sends them up.
			var d := (fl["home"] as Vector2).distance_to(_center)
			if (d < 9.0 and _speed > 3.0) or d < 3.5:
				_saw("scatter")
				fl["mode"] = FLYING
				fl["t"] = 0.0
				var away := ((fl["home"] as Vector2) - _center).normalized()
				var c: Vector2 = (fl["home"] as Vector2) + away * 18.0
				fl["circle"] = Vector3(c.x, _h(c.x, c.y) + _rng.randf_range(9.0, 14.0), c.y)
				for b in PER_FLOCK:
					var i := f * PER_FLOCK + b
					var out := Vector3(away.x, 0, away.y) + Vector3(_rng.randf_range(-0.6, 0.6), 0, _rng.randf_range(-0.6, 0.6))
					_bird_vel[i] = out.normalized() * 7.0 + Vector3(0, _rng.randf_range(5.0, 7.0), 0)
		elif mode == FLYING and not fl["gull"] and float(fl["t"]) > 9.0:
			# Done circling: down somewhere open, away from the player.
			var p := Vector2.INF
			for _k in 8:
				var a := _rng.randf() * TAU
				var q := Vector2(fl["circle"].x, fl["circle"].z) + Vector2(cos(a), sin(a)) * _rng.randf_range(15.0, 35.0)
				if q.distance_to(_center) > 25.0 and _open(q):
					p = q
					break
			if p != Vector2.INF:
				fl["mode"] = LANDING
				fl["land"] = p
				fl["t"] = 0.0
			else:
				fl["t"] = 5.0
		elif mode == LANDING and float(fl["t"]) > 5.0:
			_perch(f, fl["land"])
		mode = fl["mode"]
		for b in PER_FLOCK:
			var i := f * PER_FLOCK + b
			if mode == OFF:
				body.set_instance_transform(i, Transform3D(Basis.from_scale(Vector3.ONE * 0.001), Vector3(0, -500, 0)))
				wing.set_instance_transform(i, Transform3D(Basis.from_scale(Vector3.ONE * 0.001), Vector3(0, -500, 0)))
				continue
			var size := 1.6 if fl["gull"] else 1.0
			var p := _bird_pos[i]
			var flap := 1.0
			var pitch := 0.0
			if mode == PERCHED:
				# Pecking: a hop now and then, and a bob of the head.
				_bird_phase[i] += dt * _rng.randf_range(1.5, 3.0)
				var ph := fmod(_bird_phase[i], TAU)
				if ph < 0.25:
					p.y = _h(p.x, p.z) + sin(ph / 0.25 * PI) * 0.12
				else:
					p.y = _h(p.x, p.z)
				if _rng.randf() < dt * 0.3:
					_bird_yaw[i] += _rng.randf_range(-1.5, 1.5)
				pitch = 0.5 if sin(_bird_phase[i] * 3.0) > 0.6 else 0.0
				flap = 0.15
			else:
				var target: Vector3
				if mode == LANDING:
					var l: Vector2 = fl["land"]
					var jitter := Vector3(sin(i * 1.7) * 2.0, 0, cos(i * 2.3) * 2.0)
					target = Vector3(l.x, _h(l.x, l.y), l.y) + jitter
				else:
					var c: Vector3 = fl["circle"]
					var a := _clock * (0.35 if fl["gull"] else 0.6) + i * 0.9
					var r := 10.0 + 3.0 * sin(i * 1.3)
					target = c + Vector3(cos(a) * r, sin(_clock * 0.7 + i) * 1.5, sin(a) * r)
				var steer := (target - p)
				var want := steer.normalized() * minf(steer.length() * 1.5, 9.0 if not fl["gull"] else 7.0)
				_bird_vel[i] = _bird_vel[i].lerp(want, minf(1.0, dt * 1.6))
				p += _bird_vel[i] * dt
				var floor_y := _h(p.x, p.z)
				if p.y < floor_y:
					p.y = floor_y
				var v := _bird_vel[i]
				if Vector2(v.x, v.z).length() > 0.3:
					_bird_yaw[i] = atan2(-v.x, -v.z)
				_bird_phase[i] += dt * (9.0 if not fl["gull"] else 4.0)
				# Gulls glide more than they flap.
				var beat := sin(_bird_phase[i])
				flap = beat * (1.4 if not fl["gull"] else (0.8 if sin(_bird_phase[i] * 0.21) > 0.0 else 0.15))
				if mode == LANDING and p.distance_to(target) < 1.0:
					flap = 0.15
			_bird_pos[i] = p
			var basis := Basis(Vector3.UP, _bird_yaw[i]) * Basis(Vector3.RIGHT, pitch) * Basis.from_scale(Vector3.ONE * size)
			body.set_instance_transform(i, Transform3D(basis, p + Vector3(0, 0.04, 0)))
			var fold := 0.35 if mode == PERCHED else 1.0
			wing.set_instance_transform(i, Transform3D(basis * Basis.from_scale(Vector3(fold, flap, 1.0)), p + Vector3(0, 0.07, 0)))

# ================================================================== deer, sheep and cattle

func _box(parent: Node3D, size: Vector3, at: Vector3, mat: Material, rot := Vector3.ZERO) -> MeshInstance3D:
	var m := MeshInstance3D.new()
	var b := BoxMesh.new()
	b.size = size
	m.mesh = b
	m.material_override = mat
	m.position = at
	m.rotation = rot
	parent.add_child(m)
	return m

func _mat(c: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = c
	m.roughness = 0.95
	return m

## Four legs under a body, each hung from its hip so it can swing.
func _legs(root: Node3D, w: float, l: float, hip: float, thick: float, mat: Material) -> Array:
	var legs := []
	for c in [Vector2(-w, -l), Vector2(w, -l), Vector2(-w, l), Vector2(w, l)]:
		var pivot := Node3D.new()
		pivot.position = Vector3(c.x, hip, c.y)
		root.add_child(pivot)
		_box(pivot, Vector3(thick, hip, thick), Vector3(0, -hip / 2.0, 0), mat)
		legs.append(pivot)
	return legs

func _make_deer(stag: bool) -> Dictionary:
	var root := Node3D.new()
	var coat := _mat(Color("8a5a36"))
	var pale := _mat(Color("d9c6a5"))
	var dark := _mat(Color("2b2018"))
	_box(root, Vector3(0.42, 0.42, 1.0), Vector3(0, 0.95, 0), coat)
	_box(root, Vector3(0.3, 0.12, 0.6), Vector3(0, 0.76, 0.05), pale) # pale belly
	_box(root, Vector3(0.16, 0.2, 0.08), Vector3(0, 1.05, 0.52), pale, Vector3(0.4, 0, 0)) # tail
	var neck := Node3D.new()
	neck.position = Vector3(0, 1.08, -0.42)
	root.add_child(neck)
	_box(neck, Vector3(0.18, 0.55, 0.2), Vector3(0, 0.22, -0.08), coat, Vector3(-0.35, 0, 0))
	_box(neck, Vector3(0.2, 0.2, 0.36), Vector3(0, 0.5, -0.24), coat, Vector3(0.25, 0, 0))
	_box(neck, Vector3(0.1, 0.08, 0.1), Vector3(0, 0.44, -0.44), dark)
	for s in [-1.0, 1.0]:
		_box(neck, Vector3(0.05, 0.14, 0.04), Vector3(s * 0.12, 0.66, -0.16), coat, Vector3(0, 0, s * 0.5)) # ears
		if stag:
			_box(neck, Vector3(0.035, 0.4, 0.035), Vector3(s * 0.12, 0.82, -0.14), pale, Vector3(0.2, 0, s * 0.35))
			_box(neck, Vector3(0.03, 0.2, 0.03), Vector3(s * 0.2, 0.9, -0.24), pale, Vector3(0.8, 0, s * 0.3))
	var legs := _legs(root, 0.14, 0.38, 0.78, 0.07, coat)
	root.scale = Vector3.ONE * (1.1 if stag else 0.95)
	return {"node": root, "neck": neck, "legs": legs, "mode": OFF, "at": Vector2.ZERO, "to": Vector2.ZERO, "t": 0.0, "yaw": 0.0, "away": Vector2.ZERO, "phase": 0.0}

func _make_stock(cattle: bool) -> Dictionary:
	var root := Node3D.new()
	var neck := Node3D.new()
	var legs: Array
	if cattle:
		var coats := [Color("6b3f22"), Color("2a2522"), Color("c9b9a0")]
		var coat := _mat(coats[_rng.randi() % coats.size()])
		var patch := _mat(Color("ece6da"))
		_box(root, Vector3(0.62, 0.62, 1.35), Vector3(0, 1.0, 0), coat)
		_box(root, Vector3(0.64, 0.4, 0.4), Vector3(0, 1.08, 0.2), patch) # a white saddle
		_box(root, Vector3(0.06, 0.6, 0.06), Vector3(0, 0.9, 0.7), coat, Vector3(0.15, 0, 0)) # tail
		neck.position = Vector3(0, 1.05, -0.62)
		root.add_child(neck)
		_box(neck, Vector3(0.38, 0.4, 0.5), Vector3(0, 0.05, -0.2), coat)
		_box(neck, Vector3(0.3, 0.22, 0.14), Vector3(0, -0.08, -0.48), _mat(Color("caa08a"))) # muzzle
		for s in [-1.0, 1.0]:
			_box(neck, Vector3(0.05, 0.05, 0.22), Vector3(s * 0.22, 0.24, -0.16), _mat(Color("e8dcc0")), Vector3(0, s * 0.9, 0)) # horns
		legs = _legs(root, 0.22, 0.5, 0.7, 0.13, coat)
	else:
		var wool := _mat(Color("e9e4d6").darkened(_rng.randf() * 0.15))
		var face := _mat(Color("2a2420"))
		var sm := SphereMesh.new()
		sm.radius = 0.42
		sm.height = 0.72
		var body := MeshInstance3D.new()
		body.mesh = sm
		body.material_override = wool
		body.position = Vector3(0, 0.7, 0)
		body.scale = Vector3(0.95, 0.95, 1.35)
		root.add_child(body)
		neck.position = Vector3(0, 0.78, -0.48)
		root.add_child(neck)
		_box(neck, Vector3(0.2, 0.24, 0.32), Vector3(0, 0.0, -0.12), face)
		for s in [-1.0, 1.0]:
			_box(neck, Vector3(0.14, 0.05, 0.06), Vector3(s * 0.14, 0.06, -0.02), face)
		legs = _legs(root, 0.16, 0.3, 0.42, 0.07, face)
	return {"node": root, "neck": neck, "legs": legs, "pen": -1, "at": Vector2.ZERO, "to": Vector2.ZERO, "t": 0.0, "yaw": 0.0, "cattle": cattle, "phase": 0.0}

## Put an animal's node where it stands, facing `yaw`, legs swinging with `stride` (0 still).
func _pose(a: Dictionary, stride: float, head_down: float, dt: float) -> void:
	var n: Node3D = a["node"]
	var p: Vector2 = a["at"]
	n.position = Vector3(p.x, _h(p.x, p.y), p.y)
	n.rotation.y = lerp_angle(n.rotation.y, float(a["yaw"]), minf(1.0, dt * 5.0))
	a["phase"] = float(a["phase"]) + dt * stride * 7.0
	var legs: Array = a["legs"]
	for k in legs.size():
		var sw := sin(float(a["phase"]) + (PI if k == 1 or k == 2 else 0.0)) * 0.55 * minf(stride, 1.0)
		(legs[k] as Node3D).rotation.x = sw
	var neck: Node3D = a["neck"]
	neck.rotation.x = lerpf(neck.rotation.x, -head_down * 0.9, minf(1.0, dt * 3.0))

func _place_deer() -> void:
	var want := 4 if low else DEER
	for i in DEER:
		var d: Dictionary = _deer[i]
		var n: Node3D = d["node"]
		if i >= want:
			n.visible = false
			d["mode"] = OFF
			continue
		if d["mode"] == OFF or (d["at"] as Vector2).distance_to(_center) > NEAR + 15.0:
			# Deer keep to the woods: in twos and threes beside the trees.
			if _woods.is_empty() or (d["mode"] == OFF and float(d["t"]) > 0.0):
				n.visible = false
				d["mode"] = OFF
				continue
			var herd: Vector2 = _woods[(i / 3) % _woods.size()]
			if herd.distance_to(_center) < 30.0:
				n.visible = false
				d["mode"] = OFF
				continue
			d["at"] = herd + Vector2(_rng.randf_range(-4, 4), _rng.randf_range(-4, 4))
			d["to"] = d["at"]
			d["yaw"] = _rng.randf() * TAU
			d["mode"] = PERCHED # grazing
			d["t"] = _rng.randf_range(2.0, 6.0)
			n.visible = true
			n.rotation.y = d["yaw"]

func _deer_step(dt: float) -> void:
	for d in _deer:
		var mode: int = d["mode"]
		if mode == OFF:
			# Gone after bolting: back in a while, somewhere else.
			d["t"] = maxf(0.0, float(d["t"]) - dt)
			continue
		var at: Vector2 = d["at"]
		var n: Node3D = d["node"]
		if mode == PERCHED:
			if at.distance_to(_center) < (16.0 if _speed > 0.5 else 9.0):
				d["mode"] = FLYING # bolting
				_saw("bolt")
				d["away"] = (at - _center).normalized().rotated(_rng.randf_range(-0.5, 0.5))
				d["t"] = _rng.randf_range(2.5, 3.5)
				continue
			d["t"] = float(d["t"]) - dt
			var to: Vector2 = d["to"]
			if at.distance_to(to) > 0.3:
				var step := (to - at).normalized() * 0.9 * dt
				d["at"] = at + step
				d["yaw"] = atan2(-step.x, -step.y)
				_pose(d, 0.5, 0.0, dt)
			else:
				var grazing := sin(_clock * 0.4 + n.get_instance_id()) > -0.3
				if grazing and not d.get("grazed", false):
					_saw("graze")
				d["grazed"] = grazing
				_pose(d, 0.0, 1.0 if grazing else 0.0, dt)
			if float(d["t"]) <= 0.0:
				d["t"] = _rng.randf_range(4.0, 10.0)
				var nxt: Vector2 = at + Vector2(_rng.randf_range(-3, 3), _rng.randf_range(-3, 3))
				if _open(nxt):
					d["to"] = nxt
		else:
			# Bolting: bounding away, then out of sight.
			var away: Vector2 = d["away"]
			var nxt: Vector2 = at + away * 10.0 * dt
			if _sea_at(nxt) or _lake_at(nxt) or _near_house(nxt, 1.0):
				d["away"] = away.rotated(1.2)
			else:
				d["at"] = nxt
			d["yaw"] = atan2(-away.x, -away.y)
			d["t"] = float(d["t"]) - dt
			_pose(d, 2.2, 0.0, dt)
			n.position.y += absf(sin(float(d["phase"]) * 0.5)) * 0.45
			if float(d["t"]) <= 0.0:
				d["mode"] = OFF
				d["t"] = 25.0
				n.visible = false

func _place_stock() -> void:
	var cap := 12 if low else STOCK
	# Pens that are near now; animals in pens that are not are let go.
	var near := {}
	for f in _farms:
		if (f[0] as Vector2).distance_to(_center) < NEAR:
			near[int(f[1])] = f
	for s in _pens.keys():
		if not near.has(s):
			for i in _pens[s][2]:
				_stock[i]["pen"] = -1
				_stock[i]["node"].visible = false
			_pens.erase(s)
	for s in near:
		if _pens.has(s):
			continue
		var f: Array = near[s]
		var cattle: bool = f[2]
		var prng := RandomNumberGenerator.new()
		prng.seed = s
		var want := prng.randi_range(3, 6)
		var got := []
		for i in cap:
			if got.size() >= want:
				break
			var a: Dictionary = _stock[i]
			if a["pen"] == -1 and a["cattle"] == cattle:
				got.append(i)
		if got.is_empty():
			continue
		_pens[s] = [f[0], cattle, got]
		for i in got:
			var a: Dictionary = _stock[i]
			a["pen"] = s
			a["at"] = (f[0] as Vector2) + Vector2(_rng.randf_range(-6, 6), _rng.randf_range(-6, 6))
			a["to"] = a["at"]
			a["yaw"] = _rng.randf() * TAU
			a["t"] = _rng.randf_range(1.0, 8.0)
			a["node"].visible = true
			a["node"].rotation.y = a["yaw"]

func _stock_step(dt: float) -> void:
	for a in _stock:
		if a["pen"] == -1:
			continue
		var at: Vector2 = a["at"]
		var to: Vector2 = a["to"]
		var pen: Vector2 = _pens[a["pen"]][0]
		a["t"] = float(a["t"]) - dt
		if at.distance_to(to) > 0.25:
			var step := (to - at).normalized() * (0.55 if a["cattle"] else 0.7) * dt
			a["at"] = at + step
			a["yaw"] = atan2(-step.x, -step.y)
			_pose(a, 0.45, 0.0, dt)
		else:
			_pose(a, 0.0, 1.0 if sin(_clock * 0.3 + float(a["node"].get_instance_id() % 97)) > -0.5 else 0.2, dt)
		if float(a["t"]) <= 0.0:
			a["t"] = _rng.randf_range(5.0, 14.0)
			var nxt := pen + Vector2(_rng.randf_range(-8, 8), _rng.randf_range(-8, 8))
			# Stay off the player's feet: wander away from them when they are close.
			if nxt.distance_to(_center) < 3.0:
				nxt = at + (at - _center).normalized() * 3.0
			if _open(nxt) and nxt.distance_to(pen) < 10.0:
				a["to"] = nxt

# ================================================================== chimney smoke

static var _soft: GradientTexture2D
static func soft_dot() -> GradientTexture2D:
	if _soft == null:
		var g := Gradient.new()
		g.set_color(0, Color(1, 1, 1, 1))
		g.set_color(1, Color(1, 1, 1, 0))
		var t := GradientTexture2D.new()
		t.gradient = g
		t.fill = GradientTexture2D.FILL_RADIAL
		t.fill_from = Vector2(0.5, 0.5)
		t.fill_to = Vector2(1.0, 0.5)
		t.width = 32
		t.height = 32
		_soft = t
	return _soft

func _make_smoke() -> GPUParticles3D:
	var p := GPUParticles3D.new()
	var pm := ParticleProcessMaterial.new()
	pm.direction = Vector3(0, 1, 0)
	pm.spread = 8.0
	pm.initial_velocity_min = 0.9
	pm.initial_velocity_max = 1.4
	pm.gravity = Vector3(0.3, 0.15, 0.1)
	pm.damping_min = 0.1
	pm.damping_max = 0.3
	pm.emission_shape = ParticleProcessMaterial.EMISSION_SHAPE_SPHERE
	pm.emission_sphere_radius = 0.15
	var grow := Curve.new()
	grow.add_point(Vector2(0, 0.35))
	grow.add_point(Vector2(1, 1.6))
	var gt := CurveTexture.new()
	gt.curve = grow
	pm.scale_curve = gt
	pm.scale_min = 2.6
	pm.scale_max = 3.6
	var fade := Gradient.new()
	fade.set_color(0, Color(1, 1, 1, 0.0))
	fade.add_point(0.12, Color(1, 1, 1, 0.42))
	fade.set_color(fade.get_point_count() - 1, Color(1, 1, 1, 0.0))
	var ft := GradientTexture1D.new()
	ft.gradient = fade
	pm.color_ramp = ft
	pm.angle_min = 0.0
	pm.angle_max = 360.0
	p.process_material = pm
	var quad := QuadMesh.new()
	quad.size = Vector2(1, 1)
	var m := StandardMaterial3D.new()
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
	m.vertex_color_use_as_albedo = true
	m.albedo_color = Color("aaa59e")
	m.albedo_texture = soft_dot()
	m.depth_draw_mode = BaseMaterial3D.DEPTH_DRAW_DISABLED
	quad.material = m
	p.draw_pass_1 = quad
	p.amount = 48
	p.lifetime = 8.0
	p.preprocess = 8.0
	p.visibility_aabb = AABB(Vector3(-16, -2, -16), Vector3(32, 24, 32))
	p.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	p.emitting = false
	return p

func _place_smoke() -> void:
	var all: Array = []
	for list in chimneys.values():
		for c in list:
			var d := Vector2(c.x, c.z).distance_to(_center)
			if d < 90.0:
				all.append([d, c])
	all.sort_custom(func(a, b): return a[0] < b[0])
	var n := (SMOKES / 2) if low else SMOKES
	for i in SMOKES:
		var s := _smoke[i]
		if i < n and i < all.size():
			var at: Vector3 = all[i][1]
			if not s.emitting or s.global_position.distance_to(at) > 0.5:
				s.global_position = at
				s.restart()
			s.emitting = true
		else:
			s.emitting = false

func _smoke_frame() -> void:
	# More smoke when it is dark or cold; the wind bends it.
	var wind: float = theme._wx.get("wind", 0.3)
	var want := clampf(0.6 + 0.4 * maxf(theme.darkness, snow), 0.0, 1.0)
	var push := Vector3(0.25 + 1.6 * wind, 0.12, 0.08 + 0.4 * wind)
	# Unshaded, so it takes the light by hand: grey by day, dim at night.
	var tint := Color("aaa59e").darkened(0.78 * theme.darkness)
	for s in _smoke:
		if s.emitting:
			s.amount_ratio = want
			(s.process_material as ParticleProcessMaterial).gravity = push
			((s.draw_pass_1 as QuadMesh).material as StandardMaterial3D).albedo_color = tint

# ================================================================== fireflies

func _make_flies() -> GPUParticles3D:
	var p := GPUParticles3D.new()
	var pm := ParticleProcessMaterial.new()
	pm.emission_shape = ParticleProcessMaterial.EMISSION_SHAPE_BOX
	pm.emission_box_extents = Vector3(18, 0.8, 18)
	pm.direction = Vector3(0, 1, 0)
	pm.spread = 180.0
	pm.initial_velocity_min = 0.1
	pm.initial_velocity_max = 0.4
	pm.gravity = Vector3.ZERO
	pm.turbulence_enabled = true
	pm.turbulence_noise_strength = 1.6
	pm.turbulence_noise_scale = 2.5
	pm.turbulence_influence_min = 0.08
	pm.turbulence_influence_max = 0.16
	var glow := Gradient.new()
	glow.set_color(0, Color(1, 1, 1, 0))
	glow.add_point(0.2, Color(1, 1, 1, 1))
	glow.add_point(0.45, Color(1, 1, 1, 0.15))
	glow.add_point(0.7, Color(1, 1, 1, 1))
	glow.set_color(glow.get_point_count() - 1, Color(1, 1, 1, 0))
	var gt := GradientTexture1D.new()
	gt.gradient = glow
	pm.color_ramp = gt
	p.process_material = pm
	var quad := QuadMesh.new()
	quad.size = Vector2(0.14, 0.14)
	var m := StandardMaterial3D.new()
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	m.blend_mode = BaseMaterial3D.BLEND_MODE_ADD
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
	m.vertex_color_use_as_albedo = true
	m.albedo_color = Color(2.4, 2.8, 0.9) # brighter than white, so the bloom picks them up
	m.albedo_texture = soft_dot()
	m.depth_draw_mode = BaseMaterial3D.DEPTH_DRAW_DISABLED
	quad.material = m
	p.draw_pass_1 = quad
	p.amount = 80
	p.lifetime = 5.0
	p.preprocess = 5.0
	p.visibility_aabb = AABB(Vector3(-22, -4, -22), Vector3(44, 10, 44))
	p.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	p.emitting = false
	return p

func _flies_frame() -> void:
	var dry: bool = theme._rain_kind == "" and snow < 0.1
	var on: bool = dry and theme.darkness > 0.5 and _water != Vector2.INF and _water.distance_to(_center) < 60.0
	if on:
		var at := Vector3(_water.x, _h(_water.x, _water.y) + 1.2, _water.y)
		if not _flies.emitting or _flies.global_position.distance_to(at) > 15.0:
			_flies.global_position = at
		_flies.amount_ratio = 0.5 if low else 1.0
	_flies.emitting = on

# ================================================================== the weather on the ground

## Joining in weather that has been going a while: the ground already looks it.
func snap() -> void:
	var kind: String = theme._rain_kind
	var falling: float = theme._rain_ratio
	wet = clampf(falling * 1.2, 0.0, 1.0) if kind in ["rain", "storm"] else 0.0
	puddles = clampf(falling, 0.0, 1.0) if kind in ["rain", "storm"] else 0.0
	snow = clampf(falling * 1.1, 0.0, 1.0) if kind == "snow" else 0.0

func _ground_weather(dt: float) -> void:
	var kind: String = theme._rain_kind
	var falling: float = theme._rain_ratio
	var raining := kind in ["rain", "storm"] and falling > 0.05
	var snowing := kind == "snow" and falling > 0.05
	# Wet within a minute of rain; dries over three. Puddles fill a little slower and last about six.
	if raining:
		wet = move_toward(wet, clampf(falling * 1.2, 0.0, 1.0), dt / 40.0)
		puddles = move_toward(puddles, clampf(falling, 0.0, 1.0), dt / 70.0)
	else:
		wet = move_toward(wet, 0.0, dt / 180.0)
		puddles = move_toward(puddles, 0.0, dt / 360.0)
	# Snow settles over a minute and a half; melts over four (faster in rain), down to what the season leaves lying.
	if snowing:
		snow = move_toward(snow, maxf(clampf(falling * 1.1, 0.0, 1.0), season_snow), dt / 90.0)
	elif snow > season_snow:
		snow = move_toward(snow, season_snow, dt / (120.0 if raining else 240.0))
	else:
		snow = move_toward(snow, season_snow, dt / 90.0)
	autumn = move_toward(autumn, season_autumn, dt / 60.0)
	RenderingServer.global_shader_parameter_set("world_autumn", autumn)
	RenderingServer.global_shader_parameter_set("world_wet", wet)
	RenderingServer.global_shader_parameter_set("world_puddles", puddles)
	RenderingServer.global_shader_parameter_set("world_snow", snow)
	var show := snow > 0.005
	if show != _snow_shown:
		_snow_shown = show
		theme.show_snow(show)

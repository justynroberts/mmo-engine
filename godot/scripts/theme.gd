extends Node3D
## The look of the world, kept apart from the client (main.gd), which knows nothing about it.
##
## The client hands a theme the world (scripts/world_data.gd: heights, water, typed objects) and asks it to draw;
## everything visual lives behind the functions below. A theme is a folder res://themes/<name>/ with a theme.gd
## that extends this script, picked with --theme NAME (default: fantasy when present, else plain).
##
## This base class is itself the plain theme: grey ground, solid footprints as grey blocks, capsules for people,
## a ball for loot. A theme overrides whatever it draws differently; anything it leaves alone still works.

var world: RefCounted     # scripts/world_data.gd
var info: Dictionary      # /play/info
var entity_scale := 1.0   # characters are scaled to the server's entity_radius
var time_override := -1.0 # 0 .. 1 pins the time of day (--time), for themes that have one; below 0 follows the clock

var _env: Environment
var _sun: DirectionalLight3D
var _clear := {}
var _chunks := {}         # cell key -> Node3D
var _terrain := {}        # cell key -> MeshInstance3D
const DRAW := 260.0       # how far out things are built
const GROUND_RINGS := 3   # ground cells drawn this many cells out
const GROUND_STEP := 4.0  # ground mesh spacing

## Called once, when the world is known: sky, light, ground, anything static.
func setup(p_info: Dictionary, p_world: RefCounted) -> void:
	info = p_info
	world = p_world
	entity_scale = float(info.get("entity_radius", 0.5)) / 0.5
	var we := WorldEnvironment.new()
	_env = Environment.new()
	_env.background_mode = Environment.BG_COLOR
	_env.background_color = Color("2b2f36")
	_env.ambient_light_source = Environment.AMBIENT_SOURCE_COLOR
	_env.ambient_light_color = Color("b8bec8")
	_env.ambient_light_energy = 0.6
	_env.fog_enabled = true
	_env.fog_light_color = Color("2b2f36")
	_env.fog_density = 0.004
	we.environment = _env
	add_child(we)
	_sun = DirectionalLight3D.new()
	_sun.rotation_degrees = Vector3(-50, -30, 0)
	_sun.shadow_enabled = true
	add_child(_sun)
	_clear = {"fog": _env.fog_density, "fog_color": _env.fog_light_color, "sun": _sun.light_energy, "ambient": _env.ambient_light_energy,
		"bg": _env.background_color}
	set_daylight(info.get("daylight", {}) if info.get("daylight") is Dictionary else {})
	_ground_mat = StandardMaterial3D.new()
	(_ground_mat as StandardMaterial3D).albedo_color = Color("5a6068")

var _ground_mat: Material

## The material the ground is drawn with; a theme overrides it (the mesh itself comes from the heights).
func ground_material() -> Material:
	return _ground_mat

## Called as the player moves: build what has come near and drop what is far away, ground included.
func update(center: Vector2) -> void:
	_update_ground(center)
	var keep := {}
	var r := int(ceil(DRAW / world.CELL))
	var cx := int(floor(center.x / world.CELL))
	var cy := int(floor(center.y / world.CELL))
	for i in range(cx - r, cx + r + 1):
		for j in range(cy - r, cy + r + 1):
			var k: int = world.key(i, j)
			keep[k] = true
			if not _chunks.has(k):
				_chunks[k] = build_cell(i, j)
				add_child(_chunks[k])
	for k in _chunks.keys():
		if not keep.has(k):
			_chunks[k].queue_free()
			_chunks.erase(k)
			remove_lights(k)

## These cells (world keys) have changed: they are drawn again on the next update.
func invalidate(keys: Array) -> void:
	for k in keys:
		if _chunks.has(k):
			_chunks[k].queue_free()
			_chunks.erase(k)
			remove_lights(k)

func _update_ground(center: Vector2) -> void:
	var ext: Rect2 = world.extent()
	var keep := {}
	var cx := int(floor(center.x / world.CELL))
	var cy := int(floor(center.y / world.CELL))
	for i in range(cx - GROUND_RINGS, cx + GROUND_RINGS + 1):
		for j in range(cy - GROUND_RINGS, cy + GROUND_RINGS + 1):
			if not ext.intersects(Rect2(i * world.CELL, j * world.CELL, world.CELL, world.CELL)):
				continue
			var k: int = world.key(i, j)
			keep[k] = true
			if not _terrain.has(k):
				_terrain[k] = _ground_cell(i, j)
				add_child(_terrain[k])
	for k in _terrain.keys():
		if not keep.has(k):
			_terrain[k].queue_free()
			_terrain.erase(k)

## One cell of ground from the heights. Heights are sampled one step past each edge so normals come from the
## same samples on both sides of a seam and neighbouring cells shade without a visible join.
func _ground_cell(cx: int, cy: int) -> MeshInstance3D:
	var n := int(ceil(world.CELL / GROUND_STEP))
	var ox: float = cx * world.CELL
	var oy: float = cy * world.CELL
	var hs := PackedFloat32Array()
	hs.resize((n + 3) * (n + 3))
	for j in n + 3:
		for i in n + 3:
			hs[j * (n + 3) + i] = height(ox + (i - 1) * GROUND_STEP, oy + (j - 1) * GROUND_STEP)
	var verts := PackedVector3Array()
	var norms := PackedVector3Array()
	var idx := PackedInt32Array()
	for j in n + 1:
		for i in n + 1:
			var at := (j + 1) * (n + 3) + (i + 1)
			verts.append(Vector3(ox + i * GROUND_STEP, hs[at], oy + j * GROUND_STEP))
			norms.append(Vector3(hs[at - 1] - hs[at + 1], 2.0 * GROUND_STEP, hs[at - (n + 3)] - hs[at + (n + 3)]).normalized())
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
	mi.material_override = ground_material()
	return mi

## Everything in one cell of the world. The plain look: solid footprints as grey blocks, nothing else.
func build_cell(cx: int, cy: int) -> Node3D:
	var node := Node3D.new()
	for o in world.in_cell(cx, cy):
		if o.get("solid", false):
			node.add_child(block(o))
	return node

## A solid footprint as a plain block: what any theme can fall back on for a type it does not know.
func block(o: Dictionary) -> MeshInstance3D:
	var b: Array = o["b"]
	var mi := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = Vector3(float(b[2]) - float(b[0]), 6.0, float(b[3]) - float(b[1]))
	mi.mesh = bm
	mi.position = Vector3((float(b[0]) + float(b[2])) / 2.0, float(o.get("y", 0.0)) + 3.0, (float(b[1]) + float(b[3])) / 2.0)
	var m := StandardMaterial3D.new()
	m.albedo_color = Color("8a9099")
	mi.material_override = m
	return mi

## Ground height at a point, so characters and the camera stand on the land.
func height(x: float, y: float) -> float:
	return world.height(x, y)

## A character's body. The client adds the name and health bar on top, and drives it through these optional
## methods if the node has them: set_speed(float), attack(), hurt(), jump(seconds), set_dead(bool).
## opts: {"class": String} for the player's own character.
func entity_node(id: int, mine: bool, opts: Dictionary) -> Node3D:
	var n := Node3D.new()
	var mi := MeshInstance3D.new()
	var cap := CapsuleMesh.new()
	cap.radius = 0.5 * entity_scale
	cap.height = 2.0 * entity_scale
	mi.mesh = cap
	mi.position.y = cap.height / 2.0
	var m := StandardMaterial3D.new()
	m.albedo_color = Color("e8c55a") if mine else Color("c8503c")
	mi.material_override = m
	mi.name = "body"
	n.add_child(mi)
	return n

## Something lying on the ground: {"item", "color", "count", "rare"}.
func loot_node(item: Dictionary) -> Node3D:
	var mi := MeshInstance3D.new()
	var s := SphereMesh.new()
	s.radius = 0.4
	s.height = 0.8
	mi.mesh = s
	mi.position.y = 0.6
	var m := StandardMaterial3D.new()
	m.albedo_color = Color(str(item.get("color", "#ffffff")))
	m.emission_enabled = true
	m.emission = m.albedo_color
	mi.material_override = m
	var n := Node3D.new()
	n.add_child(mi)
	return n

## What a weather looks like, from its name (the server knows only a name, a colour and an intensity): snow,
## storm, fog or dust when the name says so, else rain. "" for clear sky.
static func weather_kind(w: Dictionary) -> String:
	if w.is_empty():
		return ""
	var n := String(w.get("name", "")).to_lower()
	# In this order, so a sandstorm is dust and a snowstorm is snow, not a thunderstorm.
	var words := {"snow": ["snow", "blizzard", "sleet", "frost"], "dust": ["dust", "sand", "ash"],
			"fog": ["fog", "mist", "haze", "smog"], "storm": ["storm", "thunder", "lightning", "tempest"]}
	for kind in words:
		for word in words[kind]:
			if n.contains(word):
				return kind
	return "rain"

# ------------------------------------------------------------------------------------- daylight and lights

## The server's schedule (/play/info "daylight", proto::daylight): when it is light and dark.
var daylight := {}
## 0 broad day .. 1 deep night, with the weather's gloom in it: what turns the lights on.
var darkness := 0.0
var _last_ms := 0

func set_daylight(d: Dictionary) -> void:
	daylight = d

## proto::daylight::Daylight::phase, ported: where the day is at unix_ms (0 sunrise, 1 - night_share sunset).
static func daylight_phase(d: Dictionary, unix_ms: float) -> float:
	match String(d.get("mode", "cycle")):
		"fixed":
			return fposmod(float(d.get("phase", 0.25)), 1.0)
		"clock":
			var rise := _hm(String(d.get("sunrise", "06:00")))
			var night := daylight_night(d)
			var local := fposmod(unix_ms / 60000.0 + float(d.get("utc_offset_minutes", 0)), 1440.0)
			var since := fposmod(local - rise, 1440.0)
			var day_len := (1.0 - night) * 1440.0
			if since < day_len:
				return since / day_len * (1.0 - night)
			return 1.0 - night + (since - day_len) / maxf(night * 1440.0, 1e-9) * night
		_:
			return fposmod(unix_ms / (float(d.get("day_minutes", 24.0)) * 60000.0) + float(d.get("offset", 0.0)), 1.0)

## The share of the day that is night (proto::daylight::Daylight::night_share).
static func daylight_night(d: Dictionary) -> float:
	match String(d.get("mode", "cycle")):
		"clock":
			return fposmod(_hm(String(d.get("sunrise", "06:00"))) - _hm(String(d.get("sunset", "19:00"))), 1440.0) / 1440.0
		"fixed":
			return 1.0 / 3.0
		_:
			return float(d.get("night_share", 1.0 / 3.0))

static func _hm(t: String) -> float:
	var p := t.split(":")
	return float(p[0]) * 60.0 + (float(p[1]) if p.size() > 1 else 0.0)

## Where the day is now, 0 .. 1, from the schedule and this machine's clock, or pinned by --time.
func day_phase() -> float:
	if time_override >= 0.0:
		return fposmod(time_override, 1.0)
	return daylight_phase(daylight, Time.get_unix_time_from_system() * 1000.0)

## Towards the sun: up through the day (highest half way), below the horizon through the night.
func sun_direction() -> Vector3:
	var t := day_phase()
	var night := daylight_night(daylight)
	var el: float
	if t < 1.0 - night:
		el = sin(PI * t / (1.0 - night)) * 1.15
	else:
		el = -sin(PI * (t - (1.0 - night)) / maxf(night, 1e-6)) * 0.6
	var az := TAU * t + 0.6
	return Vector3(cos(el) * sin(az), sin(el), cos(el) * cos(az)).normalized()

## Called every frame with where we are: the day's light, then the lights for the dark.
func frame(center: Vector2, _camera_zoom: float) -> void:
	var now := Time.get_ticks_msec()
	var dt := clampf((now - _last_ms) / 1000.0, 0.0, 0.25) if _last_ms > 0 else 0.0
	_last_ms = now
	_daylight_frame(dt)
	_lights(Vector3(center.x, height(center.x, center.y), center.y), dt)
	var mist_light := lerpf(1.0, 0.45, darkness)
	for n in _zone_nodes.values():
		for c in n.get_children():
			if c is MeshInstance3D and c.material_override is ShaderMaterial:
				(c.material_override as ShaderMaterial).set_shader_parameter("light", mist_light)

## The plain look's day and night: the background, the ambient light and the sun go down together. A theme with
## a sky overrides this, and sets `darkness` itself.
func _daylight_frame(_dt: float) -> void:
	var sun := sun_direction()
	var day := smoothstep(-0.1, 0.12, sun.y)
	var gloom := 0.3 * clampf(float(_weather.get("intensity", 0.0)), 0.0, 1.0) if not _weather.is_empty() else 0.0
	darkness = clampf(1.0 - day + gloom, 0.0, 1.0)
	_env.background_color = Color("0c0f16").lerp(_clear["bg"], day)
	_env.ambient_light_energy = _clear["ambient"] * (0.35 + 0.65 * day)
	_sun.light_energy = _clear["sun"] * day * (1.0 - gloom)
	if sun.y > 0.02:
		_sun.transform.basis = Basis.looking_at(-sun, Vector3.UP)

## Light sources: fires, lamps, lanterns and torches. Every source in view asks for light, and a small pool of
## real lights goes to the nearest of them, so a town (or a player who builds a thousand lamps) costs the same.
const LIGHT_POOL := 24
const LIGHT_KINDS := {
	"fire": {"color": Color("ff8a3a"), "energy": 3.0, "range": 11.0, "flicker": 0.35},
	"torch": {"color": Color("ffa255"), "energy": 2.0, "range": 9.0, "flicker": 0.25},
	"lantern": {"color": Color("ffc978"), "energy": 1.6, "range": 7.0, "flicker": 0.08},
	"lamp": {"color": Color("fff0c8"), "energy": 2.4, "range": 12.0, "flicker": 0.0},
}
## Our own torch: 0 lit when it is dark, 1 always, 2 never (L cycles).
var torch_mode := 0
var _sources := {}          # owner (a cell, a block chunk) -> [[Vector3, kind], ...]
var _carried: Array = []    # characters near us, each carrying a torch at night: [Vector3 feet, ...]
var _me := Vector3.ZERO
var _pool: Array[OmniLight3D] = []
var _assigned: Array = []   # per pool light: [distance², position, kind] or null
var _pick_in := 0.0
var _torch: OmniLight3D

func add_lights(owner, list: Array) -> void:
	if list.is_empty():
		_sources.erase(owner)
	else:
		_sources[owner] = list

func remove_lights(owner) -> void:
	_sources.erase(owner)

## Where we and the characters around us stand this frame (they carry torches at night).
func set_characters(me: Vector3, others: Array) -> void:
	_me = me
	_carried = others

func torch_lit() -> bool:
	return _torch != null and _torch.visible

## How many of the pool's lights are burning now.
func lights_burning() -> int:
	var n := 0
	for l in _pool:
		n += 1 if l.visible else 0
	return n

func _lights(center: Vector3, dt: float) -> void:
	if _pool.is_empty():
		for i in LIGHT_POOL:
			var l := OmniLight3D.new()
			l.shadow_enabled = false
			l.omni_attenuation = 1.2
			l.visible = false
			add_child(l)
			_pool.append(l)
			_assigned.append(null)
		_torch = OmniLight3D.new()
		_torch.light_color = LIGHT_KINDS["torch"]["color"]
		_torch.omni_range = 11.0
		# No shadow: held at the head, the body's own shadow would swallow it (and a moving shadow costs every frame).
		_torch.shadow_enabled = false
		add_child(_torch)
	var on := smoothstep(0.2, 0.65, darkness)
	_pick_in -= dt
	if _pick_in <= 0.0:
		_pick_in = 0.3
		var cand := []
		for owner in _sources:
			for src in _sources[owner]:
				var d: float = (src[0] - center).length_squared()
				if d < 8100.0:
					cand.append([d, src[0], src[1]])
		if on > 0.0:
			for p in _carried:
				cand.append([(p - center).length_squared(), p + Vector3(0.0, 3.2 * entity_scale, 0.0), "torch"])
		cand.sort_custom(func(a, b): return a[0] < b[0])
		for i in LIGHT_POOL:
			_assigned[i] = cand[i] if i < cand.size() else null
	var t := Time.get_ticks_msec() / 1000.0
	for i in LIGHT_POOL:
		var l := _pool[i]
		var a = _assigned[i]
		if a == null or on <= 0.01:
			l.visible = false
			continue
		var k: Dictionary = LIGHT_KINDS[a[2]]
		l.visible = true
		l.position = a[1]
		l.light_color = k["color"]
		l.omni_range = k["range"]
		l.light_energy = k["energy"] * on * (1.0 - k["flicker"] * (0.5 + 0.5 * sin(t * 13.0 + i * 7.1) * sin(t * 7.3 + i * 3.3)))
	var lit := torch_mode == 1 or (torch_mode == 0 and darkness > 0.4)
	_torch.visible = lit
	if lit:
		_torch.position = _me + Vector3(0.0, 3.2 * entity_scale, 0.0) # above the tallest head
		_torch.light_energy = 2.2 * maxf(on, 0.5) * (1.0 - 0.2 * (0.5 + 0.5 * sin(t * 11.0) * sin(t * 6.1)))

## A flickering flame (fire blocks, campfires, braziers): orange sparks rising off a glowing core, `size` across.
func flame(size: float) -> Node3D:
	var n := Node3D.new()
	var p := GPUParticles3D.new()
	var pm := ParticleProcessMaterial.new()
	pm.emission_shape = ParticleProcessMaterial.EMISSION_SHAPE_SPHERE
	pm.emission_sphere_radius = size * 0.25
	pm.direction = Vector3.UP
	pm.spread = 12.0
	pm.initial_velocity_min = size * 0.8
	pm.initial_velocity_max = size * 1.6
	pm.gravity = Vector3(0, size * 0.6, 0)
	pm.scale_min = 0.6
	pm.scale_max = 1.0
	var fade := Gradient.new()
	fade.set_color(0, Color(1.0, 0.75, 0.3, 1.0))
	fade.set_color(1, Color(0.9, 0.2, 0.05, 0.0))
	var ramp := GradientTexture1D.new()
	ramp.gradient = fade
	pm.color_ramp = ramp
	p.process_material = pm
	var q := QuadMesh.new()
	q.size = Vector2(size * 0.35, size * 0.35)
	var m := StandardMaterial3D.new()
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	m.blend_mode = BaseMaterial3D.BLEND_MODE_ADD
	m.vertex_color_use_as_albedo = true
	m.albedo_color = Color(2.2, 1.4, 0.7) # over white: it blooms
	q.material = m
	p.draw_pass_1 = q
	p.amount = 24
	p.lifetime = 0.7
	p.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	n.add_child(p)
	return n

# ------------------------------------------------------------------------------------------- weather zones

## Weather zones that are on (/play/info "weather_zones"): [{name, x, y, r, height, weather, color, intensity}, ...].
var _zones: Array = []
var _zone_nodes := {}       # what a zone looks like (its fields) -> its node

## The server's zones changed (or might have): draw new ones, drop gone ones.
func set_zones(list: Array) -> void:
	_zones = list
	var keep := {}
	for z in list:
		var k := JSON.stringify(z)
		keep[k] = true
		if not _zone_nodes.has(k):
			var n := zone_mist(z)
			n.position = Vector3(float(z["x"]), height(float(z["x"]), float(z["y"])), float(z["y"]))
			add_child(n)
			_zone_nodes[k] = n
	for k in _zone_nodes.keys():
		if not keep.has(k):
			_zone_nodes[k].queue_free()
			_zone_nodes.erase(k)

## The zone (smallest, where they overlap) whose place holds `p`, or {} for none.
func zone_at(p: Vector2) -> Dictionary:
	var best := {}
	for z in _zones:
		var r := float(z["r"])
		if p.distance_squared_to(Vector2(float(z["x"]), float(z["y"]))) <= r * r and (best.is_empty() or r < float(best["r"])):
			best = z
	return best

func zone_count() -> int:
	return _zone_nodes.size()

const MIST_SHADER := """shader_type spatial;
render_mode unshaded, cull_disabled, depth_draw_never, shadows_disabled;
uniform vec4 tint : source_color = vec4(0.8, 0.1, 0.1, 1.0);
uniform float strength = 0.6;
uniform float light = 1.0; // dimmer by night, with a little glow left
varying float up;
float hash(vec2 p) { p = fract(p * vec2(123.34, 456.21)); p += dot(p, p + 45.32); return fract(p.x * p.y); }
float noise(vec2 p) {
	vec2 i = floor(p); vec2 f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(hash(i), hash(i + vec2(1, 0)), u.x), mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), u.x), u.y);
}
void vertex() { up = VERTEX.y + 0.5; } // the mesh is one unit tall, centred
void fragment() {
	// Thickest low down, thinning to nothing at the top; drifting in slow wisps.
	float rise = clamp(up, 0.0, 1.0);
	float body = smoothstep(0.0, 0.08, rise) * (1.0 - smoothstep(0.35, 1.0, rise));
	float n = noise(vec2(UV.x * 18.0 + TIME * 0.05, rise * 4.0 - TIME * 0.08)) * 0.6 + noise(vec2(UV.x * 41.0 - TIME * 0.03, rise * 9.0)) * 0.4;
	ALBEDO = tint.rgb * light;
	ALPHA = clamp(strength * body * (0.2 + 1.1 * n * n), 0.0, 0.9); // wisps, not a wall
}
"""

## A zone's look: a soft wall of mist in its colour standing over the place, thick enough at full intensity to hide
## what is inside from outside. A theme with volumetric fog adds fog inside it (see the fantasy theme).
func zone_mist(z: Dictionary) -> Node3D:
	var n := Node3D.new()
	var r := float(z["r"])
	var h := float(z.get("height", 30.0))
	var mi := MeshInstance3D.new()
	var cyl := CylinderMesh.new()
	cyl.top_radius = r
	cyl.bottom_radius = r * 1.05
	cyl.height = 1.0
	cyl.cap_top = false
	cyl.cap_bottom = false
	cyl.radial_segments = 48
	mi.mesh = cyl
	# The mesh is one unit tall and centred, so `up` in the shader runs 0 at the ground to 1 at the top.
	mi.scale = Vector3(1, h, 1)
	mi.position.y = h / 2.0
	mi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	var m := ShaderMaterial.new()
	var sh := Shader.new()
	sh.code = MIST_SHADER
	m.shader = sh
	m.set_shader_parameter("tint", Color(str(z.get("color", "#888888"))))
	m.set_shader_parameter("strength", 0.25 + 0.65 * clampf(float(z.get("intensity", 0.5)), 0.0, 1.0))
	mi.material_override = m
	n.add_child(mi)
	return n

## What this theme shows for the current weather ("" for nothing); the plain theme only thickens its fog.
func weather_drawn() -> String:
	return "fog" if _env and _env.fog_density > _clear.get("fog", 0.0) else ""

var _weather := {}        # the server's weather, empty for clear

var _weather_tw: Tween

## The server's weather ({name, color, intensity}, empty for clear sky), eased in over ten seconds.
func apply_weather(w: Dictionary) -> void:
	_weather = w
	var k := clampf(float(w.get("intensity", 0.5)), 0.0, 1.0) if not w.is_empty() else 0.0
	if _weather_tw:
		_weather_tw.kill()
	_weather_tw = create_tween().set_parallel().set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)
	_weather_tw.tween_property(_env, "fog_density", _clear["fog"] + 0.015 * k, 10.0)
	if not w.is_empty():
		_weather_tw.tween_property(_env, "fog_light_color", Color(str(w.get("color", "#888888"))), 10.0)
	else:
		_weather_tw.tween_property(_env, "fog_light_color", _clear["fog_color"], 10.0)
	_weather_tw.tween_property(_sun, "light_energy", _clear["sun"] * (1.0 - 0.5 * k), 10.0)

## Show the weather as it is now with no easing in: on joining, when it was already like this.
func snap_weather() -> void:
	if _weather_tw:
		_weather_tw.custom_step(60.0)

# ------------------------------------------------------------------------------------------- built blocks

var _block_nodes := {}    # chunk key -> Node3D of that chunk's blocks
var _block_index := {}    # chunk key -> {column key: true}
var _blocks := {}         # column key (cy << 15 | cx) -> {"b": base layer, "k": a character per layer, "o": owners}
var _block_mats := {}     # material -> Material

const BLOCK_NAMES := ["stone", "timber", "red brick", "plaster", "brick", "rough brick", "roof tiles", "iron", "glass", "slate", "mossy stone", "sandstone", "lamp", "fire"]
const BLOCK_COLOURS := ["8a8f96", "8a5a34", "a6463a", "d8cfb8", "9a6a4a", "7d6b5a", "8c3b2e", "4a4d52", "bfe3ee", "4b5160", "6f7f58", "c9a66b", "ffe6a8", "5a4a40"]
## The material glass is: see-through in any theme.
const GLASS := 8
## Blocks that give light at night: a lamp glows all over; a fire is a hearth stone with a flame on top.
const LAMP := 12
const FIRE := 13

## Names of the block materials (0..11, as /play/info build.kinds says), for the HUD.
func block_name(kind: int) -> String:
	return BLOCK_NAMES[clampi(kind, 0, BLOCK_NAMES.size() - 1)]

## What a block of each material is drawn with.
func block_material(kind: int) -> Material:
	var m := StandardMaterial3D.new()
	m.albedo_color = Color(BLOCK_COLOURS[clampi(kind, 0, BLOCK_COLOURS.size() - 1)])
	if kind == GLASS:
		m.albedo_color.a = 0.45
		m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	if kind == LAMP:
		m.emission_enabled = true
		m.emission = Color("ffd88a")
		m.emission_energy_multiplier = 1.6
	return m

## A material from its character in block text ("0"-"9", then "a"-"z"), or -1 for an empty layer.
static func block_kind(ch: String) -> int:
	var k := "0123456789abcdefghijklmnopqrstuvwxyz".find(ch)
	return k if k >= 0 and k < BLOCK_NAMES.size() else -1

func block_size() -> float:
	return float(info.get("build", {}).get("block", 2.0))

## The lowest corner of a cell's ground: a new column's first block goes in the layer this is in, so it never
## floats on a slope (the server decides the same way).
func block_base(cx: int, cy: int) -> float:
	var bs := block_size()
	var x0 := cx * bs
	var y0 := cy * bs
	return minf(minf(height(x0, y0), height(x0 + bs, y0)), minf(height(x0, y0 + bs), height(x0 + bs, y0 + bs)))

## The layer a new column in this cell starts in. Layer z fills heights z * block_size() to (z + 1) * block_size(),
## the same everywhere, so blocks line up however the ground runs.
func block_layer(cx: int, cy: int) -> int:
	return int(floor(block_base(cx, cy) / block_size()))

## The client's blocks changed: `all` is every column, `changed` the column keys to redraw.
func set_blocks(all: Dictionary, changed: Array) -> void:
	_blocks = all
	var bs := block_size()
	var dirty := {}
	for k in changed:
		var cx: int = k & 0x7fff
		var cy: int = k >> 15
		var ck: int = world.key(int(floor(cx * bs / world.CELL)), int(floor(cy * bs / world.CELL)))
		dirty[ck] = true
		if all.has(k):
			if not _block_index.has(ck):
				_block_index[ck] = {}
			_block_index[ck][k] = true
		elif _block_index.has(ck):
			_block_index[ck].erase(k)
	for ck in dirty:
		if _block_nodes.has(ck):
			_block_nodes[ck].queue_free()
			_block_nodes.erase(ck)
		var cols: Array = []
		for k in _block_index.get(ck, {}):
			cols.append([k & 0x7fff, k >> 15, int(_blocks[k]["b"]), String(_blocks[k]["k"])])
		var glow := []
		if not cols.is_empty():
			_block_nodes[ck] = build_blocks(cols)
			add_child(_block_nodes[ck])
			for c in cols:
				for i in String(c[3]).length():
					var kind := block_kind(c[3][i])
					if kind == LAMP or kind == FIRE:
						glow.append([Vector3((c[0] + 0.5) * bs, (c[2] + i + 1.0) * bs + 0.3, (c[1] + 0.5) * bs), "lamp" if kind == LAMP else "fire"])
		add_lights("blocks %d" % ck, glow)

## One chunk's blocks ([[cx, cy, base layer, "layers"], ...], "." for an empty layer): a MultiMesh of cubes per
## material, each in its layer.
func build_blocks(cols: Array) -> Node3D:
	var node := Node3D.new()
	var bs := block_size()
	var per_kind := {}
	for c in cols:
		var cx: int = c[0]
		var cy: int = c[1]
		var base: int = c[2]
		var kinds: String = c[3]
		for i in kinds.length():
			var kind := block_kind(kinds[i])
			if kind < 0:
				continue
			if not per_kind.has(kind):
				per_kind[kind] = []
			per_kind[kind].append(Vector3((cx + 0.5) * bs, (base + i + 0.5) * bs, (cy + 0.5) * bs))
	for kind in per_kind:
		if not _block_mats.has(kind):
			_block_mats[kind] = block_material(kind)
		var at: Array = per_kind[kind]
		var cube := BoxMesh.new()
		cube.size = Vector3(bs, bs, bs)
		var mm := MultiMesh.new()
		mm.transform_format = MultiMesh.TRANSFORM_3D
		mm.mesh = cube
		mm.instance_count = at.size()
		for i in at.size():
			mm.set_instance_transform(i, Transform3D(Basis(), at[i]))
		var mi := MultiMeshInstance3D.new()
		mi.multimesh = mm
		mi.material_override = _block_mats[kind]
		node.add_child(mi)
		if kind == FIRE:
			# A flame on every fire block (only the top one of a stack would show, but blocks rarely stack fires).
			for p in at.slice(0, 64):
				var f := flame(bs)
				f.position = p + Vector3(0, bs * 0.5, 0)
				node.add_child(f)
	return node

## A picture of the whole playable world, px by px, for the map (M).
func map_image(px: int) -> Image:
	var img := Image.create(px, px, false, Image.FORMAT_RGB8)
	img.fill(Color("4a4f57"))
	var step: float = world.size / px
	for o in world.objects:
		if o.get("solid", false) and not o.get("scenery", false):
			var b: Array = o["b"]
			var p0 := Vector2i(int(b[0] / step), int(b[1] / step))
			var p1 := Vector2i(int(b[2] / step), int(b[3] / step))
			img.fill_rect(Rect2i(p0, (p1 - p0).max(Vector2i(1, 1))), Color("9aa0a8"))
	return img

## Extra checks for the scripted smoke test: [[name, passed, detail], ...].
func smoke_checks() -> Array:
	return [["theme drew the world", _chunks.size() > 0 and _terrain.size() > 0, "%d cells, %d ground" % [_chunks.size(), _terrain.size()]]]

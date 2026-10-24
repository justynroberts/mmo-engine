extends "res://scripts/theme.gd"
## The fantasy look: KayKit characters, dungeon keeps, Medieval Village houses, forests, crags, rivers and lakes
## under a sky that runs through day, dusk and night with the weather in it, on Forward+ (bloom, SSAO, volumetric
## fog, water with depth and foam).
##
## It only draws: where things are and what they are comes from the world file (WORLD.md), so a different
## world is a different run of the generator, not a change here. Each object type maps to a builder below, and
## an object's variant number (v) picks among the kit's pieces, so every client draws the same thing.

const PROP_CHUNK := 128.0
var terrain_mat: ShaderMaterial
var water_mat: ShaderMaterial
var rain := GPUParticles3D.new()   # whatever the weather drops: rain, snow or dust
var sky_mat: ShaderMaterial
var _kind := ""           # what the weather looks like: "" clear, rain, snow, storm, fog, dust
var _flash := 0.0         # lightning, fading
# Weather changes ease in over WEATHER_SECS: every look below heads for what the new weather wants a little each
# frame, and what falls from the sky thins out before it turns into something else.
const WEATHER_SECS := 10.0
var _wx_want := {"k": 0.0, "cover": 0.5, "dark": 0.0, "wind": 0.3, "storm": 0.0, "thick": 1.0, "sky": 0.0, "col": Color("8899aa")}
var _wx := _wx_want.duplicate()   # where the look is now, on its way to _wx_want
var _rain_kind := ""      # what the particles are set up to drop now ("" none)
var _rain_ratio := 0.0    # how hard they fall now, 0 .. 1
var _rain_want := 0.0     # how hard the weather wants them to fall
var _next_flash := 0.0
var meshes := {}             # path -> Mesh, pulled out of the glTF pieces once
var _built_bodies := 0       # how many KayKit characters were made (for the smoke test)

func setup(p_info: Dictionary, p_world: RefCounted) -> void:
	info = p_info
	world = p_world
	entity_scale = float(info.get("entity_radius", 0.5)) / 0.5
	var env := WorldEnvironment.new()
	var e := Environment.new()
	sky_mat = ShaderMaterial.new()
	sky_mat.shader = load("res://themes/fantasy/sky.gdshader")
	var sky := Sky.new()
	sky.sky_material = sky_mat
	# The sky changes every frame (the clouds drift), and the light comes from colours, not the sky, so a small
	# radiance map redrawn as it goes is all reflections need.
	sky.process_mode = Sky.PROCESS_MODE_REALTIME
	sky.radiance_size = Sky.RADIANCE_SIZE_64
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
	e.volumetric_fog_sky_affect = 0.0 # the haze is in the air around us, not painted over the sky
	env.environment = e
	add_child(env)
	_env = e
	var sun := DirectionalLight3D.new()
	_sun = sun
	sun.rotation_degrees = Vector3(-48, -35, 0)
	sun.light_color = Color("fff1d6")
	sun.light_energy = 0.95
	sun.shadow_enabled = true
	sun.light_volumetric_fog_energy = 1.4
	sun.shadow_blur = 1.5
	sun.directional_shadow_max_distance = 90.0
	add_child(sun)
	_clear = {"fog": e.fog_density, "fog_color": e.fog_light_color, "vol": e.volumetric_fog_density, "vol_albedo": e.volumetric_fog_albedo,
		"sun": sun.light_energy, "ambient": e.ambient_light_energy, "ambient_color": e.ambient_light_color, "sun_color": sun.light_color}
	# Rain (or whatever falls in the current weather): streaks in a box that follows the camera.
	var pm := ParticleProcessMaterial.new()
	pm.emission_shape = ParticleProcessMaterial.EMISSION_SHAPE_BOX
	pm.emission_box_extents = Vector3(50, 1, 50)
	pm.direction = Vector3(0.15, -1, 0.05)
	pm.spread = 3.0
	pm.initial_velocity_min = 18.0
	pm.initial_velocity_max = 22.0
	pm.gravity = Vector3.ZERO
	rain.process_material = pm
	var streak := BoxMesh.new()
	streak.size = Vector3(0.025, 0.55, 0.025)
	# The drops thin out towards the edge of a circle round us, so the box they fall in never shows its corners.
	var fall := Shader.new()
	fall.code = """shader_type spatial;
render_mode unshaded, cull_disabled, depth_draw_never, shadows_disabled;
uniform vec4 tint : source_color = vec4(0.8, 0.85, 0.95, 0.45);
uniform vec3 center;
uniform float radius = 50.0;
void fragment() {
	vec3 w = (INV_VIEW_MATRIX * vec4(VERTEX, 1.0)).xyz;
	ALBEDO = tint.rgb;
	ALPHA = tint.a * (1.0 - smoothstep(radius * 0.45, radius, length(w.xz - center.xz)));
}"""
	var sm := ShaderMaterial.new()
	sm.shader = fall
	streak.material = sm
	rain.draw_pass_1 = streak
	rain.amount = 6000
	rain.lifetime = 0.9
	rain.visibility_aabb = AABB(Vector3(-50, -25, -50), Vector3(100, 30, 100))
	rain.emitting = false
	rain.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	add_child(rain)
	# Meadow: two greens, dry patches and worn dirt, all from world position so every client sees the same field.
	var sh := Shader.new()
	sh.code = """shader_type spatial;
render_mode specular_disabled;
uniform float rocky = 0.0; // 1 on crags: rock except in the flattest hollows
uniform sampler2D road_map : filter_linear, repeat_disable; // a cell's roads: r how much road, g paved (ground_material_at)
uniform vec2 road_origin;
uniform float road_on = 0.0;
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
	// Roads: packed earth with a ragged grassy edge in the country, cobbles in the city.
	if (road_on > 0.5) {
		vec2 rm = texture(road_map, (q - road_origin) / 128.0).rg;
		float road = smoothstep(0.2, 0.6, rm.r + 0.25 * (n(q * 0.7) - 0.5));
		vec3 dirt = vec3(0.52, 0.43, 0.30) * (0.86 + 0.22 * fbm(q * 0.9)) * (0.92 + 0.12 * n(q * 3.0));
		vec2 cq = q * vec2(1.25, 1.6);
		cq.x += 0.5 * floor(cq.y);
		vec2 cf = abs(fract(cq) - 0.5);
		float stone = smoothstep(0.5, 0.38, max(cf.x, cf.y));
		vec3 cob = mix(vec3(0.27, 0.26, 0.24), vec3(0.55, 0.53, 0.49) * (0.8 + 0.35 * h(floor(cq))), stone);
		c = mix(c, mix(dirt, cob, smoothstep(0.4, 0.6, rm.g)), road);
	}
	c = mix(c, vec3(0.93, 0.95, 0.98), smoothstep(30.0, 42.0, wp.y + 6.0 * n(q * 0.05)) * (1.0 - smoothstep(0.55, 0.8, slope)));
	ALBEDO = pow(c, vec3(2.2)); // the colours above are sRGB; albedo is linear
	ROUGHNESS = 1.0;
}"""
	var m := ShaderMaterial.new()
	m.shader = sh
	terrain_mat = m

	# A flat sheet well below the hills fills the distance past the streamed terrain, inside the fog.
	var sheet := MeshInstance3D.new()
	var plane := PlaneMesh.new()
	var ext: Rect2 = world.extent()
	plane.size = ext.size + Vector2(4096, 4096)
	sheet.mesh = plane
	sheet.material_override = terrain_mat
	sheet.position = Vector3(ext.get_center().x, -20.0, ext.get_center().y)
	add_child(sheet)
	# The open sea round land with a coast: one surface at sea level reaching far past the world, under the fog.
	if world.has_sea:
		var sea := MeshInstance3D.new()
		var sea_plane := PlaneMesh.new()
		sea_plane.size = ext.size + Vector2(8192, 8192)
		sea_plane.subdivide_width = 64
		sea_plane.subdivide_depth = 64
		sea.mesh = sea_plane
		sea.material_override = _water()
		sea.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		sea.position = Vector3(ext.get_center().x, world.sea_level, ext.get_center().y)
		add_child(sea)
	# Water: every river's ribbon and every lake's surface, once.
	for r in world.rivers:
		add_child(_river_mesh(r))
	for l in world.lakes:
		var b: Array = l["b"]
		var water := MeshInstance3D.new()
		var lake_plane := PlaneMesh.new()
		lake_plane.size = Vector2(float(b[2]) - float(b[0]) + 1.0, float(b[3]) - float(b[1]) + 1.0)
		lake_plane.subdivide_width = 8
		lake_plane.subdivide_depth = 8
		water.mesh = lake_plane
		water.material_override = _water()
		water.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		water.position = Vector3((float(b[0]) + float(b[2])) / 2.0, float(l["level"]), (float(b[1]) + float(b[3])) / 2.0)
		add_child(water)

func ground_material() -> Material:
	return terrain_mat

## Road map texels along a ground cell's edge (2 units each).
const ROAD_PX := 64
## A cell with roads in it gets its own copy of the ground material, with a small map of where the roads run: how
## much road each texel is (1 on it, fading over a unit and a half at the edge) and whether it is paved (the city's
## streets and avenues, which are the wide ones).
func ground_material_at(cx: int, cy: int) -> Material:
	var segs: Array = world.road_segments(cx, cy)
	if segs.is_empty():
		return terrain_mat
	var n := ROAD_PX
	var px: float = world.CELL / n
	var ox: float = cx * world.CELL
	var oy: float = cy * world.CELL
	var map := PackedByteArray()
	map.resize(n * n * 2)
	for s in segs:
		var a := Vector2(s[0], s[1])
		var b := Vector2(s[2], s[3])
		var hw: float = s[4]
		var paved := hw >= 3.4
		var pad := hw + 1.5
		var i0 := clampi(int((minf(a.x, b.x) - pad - ox) / px), 0, n - 1)
		var i1 := clampi(int((maxf(a.x, b.x) + pad - ox) / px), 0, n - 1)
		var j0 := clampi(int((minf(a.y, b.y) - pad - oy) / px), 0, n - 1)
		var j1 := clampi(int((maxf(a.y, b.y) + pad - oy) / px), 0, n - 1)
		for j in range(j0, j1 + 1):
			for i in range(i0, i1 + 1):
				var p := Vector2(ox + (i + 0.5) * px, oy + (j + 0.5) * px)
				var v := clampf((hw + 0.75 - Geometry2D.get_closest_point_to_segment(p, a, b).distance_to(p)) / 1.5, 0.0, 1.0)
				if v <= 0.0:
					continue
				var at := (j * n + i) * 2
				var q := int(v * 255.0)
				if q > map[at]:
					map[at] = q
				if paved and q > map[at + 1]:
					map[at + 1] = q
	var m := terrain_mat.duplicate() as ShaderMaterial
	m.set_shader_parameter("road_map", ImageTexture.create_from_image(Image.create_from_data(n, n, false, Image.FORMAT_RG8, map)))
	m.set_shader_parameter("road_origin", Vector2(ox, oy))
	m.set_shader_parameter("road_on", 1.0)
	return m

## Building blocks from the Medieval Village textures, tiled so each block shows one texture repeat: the eight
## textures as they are, then glass, and the dressed stone tinted three ways (slate, moss, sandstone).
const BLOCK_TEXTURES := ["RockTrim", "WoodTrim", "RedBrick", "Plaster", "Brick", "UnevenBrick", "RoundTiles", "MetalOrnaments", "", "RockTrim", "RockTrim", "RockTrim", "", "RockTrim"]
const BLOCK_TINTS := {9: Color(0.55, 0.6, 0.72), 10: Color(0.7, 0.85, 0.55), 11: Color(1.15, 0.95, 0.68), 13: Color(0.45, 0.38, 0.34)}

func block_material(kind: int) -> Material:
	kind = clampi(kind, 0, BLOCK_TEXTURES.size() - 1)
	if BLOCK_TEXTURES[kind] == "":
		return super.block_material(kind) # glass
	var dir := "res://themes/fantasy/assets/village/T_%s_" % BLOCK_TEXTURES[kind]
	var m := StandardMaterial3D.new()
	m.albedo_color = BLOCK_TINTS.get(kind, Color.WHITE)
	m.albedo_texture = load(dir + "BaseColor.png")
	if ResourceLoader.exists(dir + "Normal.png"):
		m.normal_enabled = true
		m.normal_texture = load(dir + "Normal.png")
	m.uv1_triplanar = true
	m.uv1_world_triplanar = true
	m.uv1_scale = Vector3.ONE / block_size()
	m.roughness = 0.9
	return m

func frame(center: Vector2, camera_zoom: float) -> void:
	# Rain starts below the camera so no streak brushes past the lens; snow and dust fill the air around us.
	var above := minf(14.0, camera_zoom * 0.5) if _rain_kind in ["rain", "storm"] else 6.0
	rain.global_position = Vector3(center.x, height(center.x, center.y) + above, center.y)
	((rain.draw_pass_1 as BoxMesh).material as ShaderMaterial).set_shader_parameter("center", rain.global_position)
	super.frame(center, camera_zoom)

## How dark the night is: the moon's light, and the share of the day's ambient light left at night. Torches and
## fires carry the dark hours.
const NIGHT_MOON := 0.12
const NIGHT_AMBIENT := 0.22
const NIGHT_EXPOSURE := 0.6

## Sun, moon, light, fog and sky for the time of day (the server's schedule) and the weather.
func _daylight_frame(dt: float) -> void:
	var sun_dir := sun_direction()
	var day := smoothstep(-0.1, 0.12, sun_dir.y)
	var dusk := 1.0 - smoothstep(0.0, 0.3, absf(sun_dir.y))
	_ease_weather(dt)
	var k: float = _wx["k"]
	var col: Color = _wx["col"]
	var cover: float = _wx["cover"]
	var dark: float = _wx["dark"]
	var storm: float = _wx["storm"]
	darkness = clampf(1.0 - day + dark * 0.6, 0.0, 1.0)
	# Lightning in a storm: a double flash every few seconds, once the storm has built up.
	if storm > 0.6:
		_next_flash -= dt
		if _next_flash <= 0.0:
			_flash = 1.0
			_next_flash = randf_range(3.0, 10.0) if randf() > 0.3 else 0.18
	_flash = maxf(0.0, _flash - dt * 7.0)
	sky_mat.set_shader_parameter("sun_dir", sun_dir)
	sky_mat.set_shader_parameter("day", day)
	sky_mat.set_shader_parameter("dusk", dusk)
	sky_mat.set_shader_parameter("cover", cover)
	sky_mat.set_shader_parameter("dark", dark)
	sky_mat.set_shader_parameter("tint", Vector3(col.r, col.g, col.b).lerp(Vector3(0.55, 0.58, 0.62), 0.5))
	sky_mat.set_shader_parameter("flash", _flash)
	sky_mat.set_shader_parameter("clock", Time.get_ticks_msec() / 1000.0)
	sky_mat.set_shader_parameter("wind", _wx["wind"])
	# By day the sun lights the world; by night a dim blue moon from the other side does.
	var light_dir := sun_dir if sun_dir.y > -0.05 else -sun_dir
	_sun.transform.basis = Basis.looking_at(-light_dir, Vector3.FORWARD if absf(light_dir.y) > 0.99 else Vector3.UP)
	var warm := (_clear["sun_color"] as Color).lerp(Color("ffb27a"), dusk * 0.7)
	_sun.light_color = warm if sun_dir.y > -0.05 else Color("9db4ff")
	var sun_energy: float = _clear["sun"] * (0.25 + 0.75 * day) if sun_dir.y > -0.05 else NIGHT_MOON
	_sun.light_energy = sun_energy * (1.0 - (0.55 + 0.2 * storm) * k)
	var e := _env
	e.tonemap_exposure = lerpf(NIGHT_EXPOSURE, 0.95, day) # the whole picture darker at night, the sky with it
	e.ambient_light_color = Color("1a2238").lerp(_clear["ambient_color"], day)
	e.ambient_light_energy = _clear["ambient"] * (NIGHT_AMBIENT + (1.0 - NIGHT_AMBIENT) * day) * (1.0 - 0.35 * k) + _flash * 1.5
	var fog_day: Color = (_clear["fog_color"] as Color).lerp(Color("e0a07a"), dusk * 0.35)
	var fog := Color("0a0e17").lerp(fog_day, day)
	var thick: float = _wx["thick"]
	e.fog_light_color = fog.lerp(col.darkened(0.2 * (1.0 - day)), 0.6 * k)
	e.fog_density = _clear["fog"] + 0.012 * k * thick
	e.volumetric_fog_density = _clear["vol"] + 0.02 * k * thick
	e.volumetric_fog_albedo = (_clear["vol_albedo"] as Color).lerp(col, 0.5 * k).darkened(0.6 * (1.0 - day))
	# Weather closes over the sky as it thickens; fog all but hides it.
	var sky_cover: float = _wx["sky"]
	e.volumetric_fog_sky_affect = clampf(0.35 * k * thick, 0.0, 0.9) * sky_cover
	e.fog_sky_affect = clampf(0.25 * k * thick, 0.0, 0.8) * sky_cover

## Move every part of the weather's look a step toward the new weather, so a change takes about WEATHER_SECS: most of
## the way in the first few seconds and settling gently, the way weather really rolls in.
func _ease_weather(dt: float) -> void:
	var a := 1.0 - exp(-dt * 3.0 / WEATHER_SECS) # 95% of the way after WEATHER_SECS
	for key in _wx_want:
		if key == "col":
			_wx["col"] = (_wx["col"] as Color).lerp(_wx_want["col"], a)
		else:
			_wx[key] = lerpf(_wx[key], _wx_want[key], a)
	# Rain, snow or dust: fades in and out at the same pace, and thins away before turning into something else.
	var want_kind := _kind if _kind in ["rain", "storm", "snow", "dust"] else ""
	if want_kind != _rain_kind:
		_rain_ratio = maxf(0.0, _rain_ratio - dt / (WEATHER_SECS * 0.4))
		if _rain_ratio <= 0.0:
			_rain_kind = want_kind
			if _rain_kind != "":
				_set_rain(_rain_kind, float(_wx_want["k"]), _wx_want["col"])
	else:
		_rain_ratio = move_toward(_rain_ratio, _rain_want if _rain_kind != "" else 0.0, dt / WEATHER_SECS)
	rain.emitting = _rain_kind != "" and _rain_ratio > 0.0
	rain.amount_ratio = _rain_ratio

## One cell of the world: each object drawn by its type, and every piece of the same kind in the cell sharing
## one MultiMesh, so a town is a few dozen draws.
func build_cell(cx: int, cy: int) -> Node3D:
	var node := Node3D.new()
	var batch := {}
	var lights := []
	for o in world.in_cell(cx, cy):
		var t := String(o.get("t", ""))
		match t:
			"house":
				node.add_child(_build_house(o["b"], float(o.get("y", 0.0)), int(o.get("v", 0)), batch))
				# Every third house hangs a lantern by its south wall.
				if absi(int(o.get("v", 0))) % 3 == 0:
					var b: Array = o["b"]
					var at := Vector3((float(b[0]) + float(b[2])) / 2.0, 0.0, float(b[3]) + 0.35)
					at.y = height(at.x, at.z) + 2.4
					node.add_child(_lantern(at))
					lights.append([at, "lantern"])
			"keep", "castle", "tower", "wall", "block":
				node.add_child(_keep(o, batch))
				# Braziers burn at two corners of a keep, a castle or a tower.
				if t in ["keep", "castle", "tower"]:
					var b: Array = o["b"]
					for c in [Vector2(float(b[0]) - 1.2, float(b[1]) - 1.2), Vector2(float(b[2]) + 1.2, float(b[3]) + 1.2)]:
						var at := Vector3(c.x, height(c.x, c.y), c.y)
						node.add_child(_brazier(at))
						lights.append([at + Vector3(0, 2.0, 0), "torch"])
			"crag":
				var b: Array = o["b"]
				var short := minf(float(b[2]) - float(b[0]), float(b[3]) - float(b[1]))
				node.add_child(_crag_rocks(b, clampf(short * 0.7, 8.0, 28.0), short / 2.0))
				node.add_child(_rim_rocks(b, float(o.get("y", 0.0)), 9.0))
			"lake":
				var b: Array = o["b"]
				node.add_child(_rim_rocks(b, float(o.get("y", 0.0)), 5.0))
				node.add_child(_dock(b, float(o.get("y", 0.0)) - 0.45, int(o.get("v", 0))))
			"tree", "tree_bare", "bush", "rock", "boulder", "spire":
				_prop(o, batch)
			"square":
				# A campfire in the middle of every square (the heights already level it).
				var b: Array = o["b"]
				var at := Vector3((float(b[0]) + float(b[2])) / 2.0, 0.0, (float(b[1]) + float(b[3])) / 2.0)
				at.y = height(at.x, at.z)
				node.add_child(_campfire(at))
				lights.append([at + Vector3(0, 1.2, 0), "fire"])
			"city":
				pass # open ground: the heights already level it
			"sea", "ford":
				pass # the sea is drawn once, as a surface (setup); a ford is the road's own ground under the water
			_:
				if o.get("solid", false):
					node.add_child(block(o)) # a type this theme does not know: draw it plainly
	for path in batch:
		var m := _mesh(path)
		if m:
			node.add_child(_multi(m, batch[path], _tint(path)))
	add_lights(world.key(cx, cy), lights)
	return node

# ------------------------------------------------------------------------------------- what burns at night

## A ring of stones round a fire.
func _campfire(at: Vector3) -> Node3D:
	var n := Node3D.new()
	n.position = at
	var ring := MeshInstance3D.new()
	var tm := TorusMesh.new()
	tm.inner_radius = 0.55
	tm.outer_radius = 0.95
	ring.mesh = tm
	ring.scale = Vector3(1, 0.45, 1)
	ring.material_override = _plain_mat(Color("5c5650"))
	n.add_child(ring)
	var logs := MeshInstance3D.new()
	var lm := CylinderMesh.new()
	lm.top_radius = 0.09
	lm.bottom_radius = 0.09
	lm.height = 1.1
	logs.mesh = lm
	logs.rotation = Vector3(0, 0.4, PI / 2)
	logs.position.y = 0.12
	logs.material_override = _plain_mat(Color("4a3020"))
	n.add_child(logs)
	var f := flame(1.6)
	f.position.y = 0.2
	n.add_child(f)
	return n

## An iron bowl of fire on a post.
func _brazier(at: Vector3) -> Node3D:
	var n := Node3D.new()
	n.position = at
	var post := MeshInstance3D.new()
	var pm := CylinderMesh.new()
	pm.top_radius = 0.07
	pm.bottom_radius = 0.1
	pm.height = 1.6
	post.mesh = pm
	post.position.y = 0.8
	post.material_override = _plain_mat(Color("2e2c2a"))
	n.add_child(post)
	var bowl := MeshInstance3D.new()
	var bm := CylinderMesh.new()
	bm.top_radius = 0.45
	bm.bottom_radius = 0.2
	bm.height = 0.3
	bowl.mesh = bm
	bowl.position.y = 1.7
	bowl.material_override = post.material_override
	n.add_child(bowl)
	var f := flame(1.1)
	f.position.y = 1.85
	n.add_child(f)
	return n

## A glowing lantern on a short bracket.
func _lantern(at: Vector3) -> Node3D:
	var n := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = Vector3(0.28, 0.4, 0.28)
	n.mesh = bm
	n.position = at
	var m := StandardMaterial3D.new()
	m.albedo_color = Color("ffd98c")
	m.emission_enabled = true
	m.emission = Color("ffc060")
	m.emission_energy_multiplier = 2.5
	n.material_override = m
	n.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	return n

var _plain_mats := {}
func _plain_mat(c: Color) -> StandardMaterial3D:
	if not _plain_mats.has(c):
		var m := StandardMaterial3D.new()
		m.albedo_color = c
		m.roughness = 0.9
		_plain_mats[c] = m
	return _plain_mats[c]

## Scenery: a model of the object's kind, picked by its variant, at its scale and turn, on the ground.
func _prop(o: Dictionary, batch: Dictionary) -> void:
	var t := String(o["t"])
	var v := absi(int(o.get("v", 0)))
	var x := float(o["p"][0])
	var y := float(o["p"][1])
	var s := float(o.get("s", 1.0))
	var turn := Basis(Vector3.UP, float(o.get("r", 0.0)))
	var path := ""
	match t:
		"tree": path = FOREST + TREES[v % TREES.size()] + ".gltf"
		"tree_bare": path = FOREST + BARE_TREES[v % BARE_TREES.size()] + ".gltf"
		"bush": path = FOREST + BUSHES[v % BUSHES.size()] + ".gltf"
		"rock": path = FOREST + SMALL_ROCKS[v % SMALL_ROCKS.size()] + ".gltf"
		"boulder", "spire":
			var pick: Array = BOULDERS[v % BOULDERS.size()] if t == "boulder" else SPIRES[v % SPIRES.size()]
			path = FOREST + String(pick[0]) + ".gltf"
			if not batch.has(path):
				batch[path] = []
			batch[path].append(Transform3D(turn.scaled(Vector3.ONE * s), Vector3(x, height(x, y) - 0.5 - float(pick[3]) * s, y)))
			return
	if not batch.has(path):
		batch[path] = []
	batch[path].append(Transform3D(turn.scaled(Vector3.ONE * s), Vector3(x, height(x, y) - 0.15, y)))

## You are the class you asked for (knight, barbarian, mage or rogue); everyone else is one of the mobs,
## chosen from the entity id so every client shows the same one.
func entity_node(id: int, mine: bool, opts: Dictionary) -> Node3D:
	# A player (us, or anyone with a class): by their class's look, the knight when there is no model for it.
	var scene := CHARS + "player.tscn"
	if opts.has("class") and ResourceLoader.exists(CHARS + "player_%s.tscn" % opts["class"]):
		scene = CHARS + "player_%s.tscn" % opts["class"]
	elif not mine and not opts.has("class"):
		var kinds := _mob_scenes()
		scene = kinds[id % kinds.size()] if not kinds.is_empty() else CHARS + "mob.tscn"
	if not ResourceLoader.exists(scene):
		return super.entity_node(id, mine, opts)
	var c := (load(scene) as PackedScene).instantiate() as Node3D
	c.scale = Vector3.ONE * entity_scale
	_built_bodies += 1
	return c

func loot_node(item: Dictionary) -> Node3D:
	return _gem_node(str(item.get("color", "#ffffff")), bool(item.get("rare", false)))

## The server's weather: where the look should end up. _ease_weather takes it there over WEATHER_SECS.
func apply_weather(w: Dictionary) -> void:
	_weather = w
	_kind = weather_kind(w)
	var k := clampf(float(w.get("intensity", 0.5)), 0.0, 1.0) if not w.is_empty() else 0.0
	var clear := _kind == ""
	_wx_want = {
		"k": k,
		"cover": 0.5 if clear else clampf(0.55 + 0.45 * k, 0.0, 1.0), # fair-weather cumulus when clear
		"dark": 0.0 if clear else (0.35 + 0.45 * k if _kind == "storm" else 0.15 + 0.35 * k),
		"wind": 0.3 if clear else (1.0 if _kind == "storm" else 0.4 + 0.5 * k),
		"storm": 1.0 if _kind == "storm" else 0.0,
		"thick": 2.5 if _kind == "fog" else 1.0,
		"sky": 0.0 if clear else 1.0,
		# Clearing keeps the old tint while it thins out, rather than flashing grey.
		"col": Color(str(w.get("color", "#8899aa"))) if not clear else _wx_want["col"],
	}
	_rain_want = 0.0 if not _kind in ["rain", "storm", "snow", "dust"] else (1.0 if _kind == "storm" else clampf(0.25 + k, 0.0, 1.0))
	if _kind == _rain_kind and _rain_kind != "":
		_set_rain(_rain_kind, k, _wx_want["col"]) # the same kind, harder or softer: no need to thin out first

func snap_weather() -> void:
	_wx = _wx_want.duplicate()
	_rain_kind = _kind if _kind in ["rain", "storm", "snow", "dust"] else ""
	_rain_ratio = _rain_want if _rain_kind != "" else 0.0
	if _rain_kind != "":
		_set_rain(_rain_kind, float(_wx_want["k"]), _wx_want["col"])

## Set the particles up for a kind of weather (they fall at amount_ratio, which _ease_weather moves).
func _set_rain(kind: String, k: float, col: Color) -> void:
	var pm := rain.process_material as ParticleProcessMaterial
	var mesh := rain.draw_pass_1 as BoxMesh
	var m := mesh.material as ShaderMaterial
	match kind:
		"snow": # slow flakes that drift and swirl
			pm.direction = Vector3(0.25, -1, 0.1)
			pm.spread = 18.0
			pm.initial_velocity_min = 3.0
			pm.initial_velocity_max = 5.0
			pm.turbulence_enabled = true
			pm.turbulence_noise_strength = 2.0
			pm.emission_box_extents = Vector3(40, 8, 40)
			mesh.size = Vector3(0.09, 0.09, 0.09)
			m.set_shader_parameter("tint", Color(1, 1, 1, 0.85))
			rain.lifetime = 5.0
		"dust": # grit blown sideways
			pm.direction = Vector3(1, -0.08, 0.25)
			pm.spread = 10.0
			pm.initial_velocity_min = 12.0
			pm.initial_velocity_max = 18.0
			pm.turbulence_enabled = true
			pm.turbulence_noise_strength = 1.0
			pm.emission_box_extents = Vector3(45, 6, 45)
			mesh.size = Vector3(0.6, 0.06, 0.06) # streaks along the wind
			m.set_shader_parameter("tint", Color(col.lightened(0.15), 0.55))
			rain.lifetime = 2.5
		_: # rain, and a storm's heavier rain
			# About 20 units a second (a storm's a little faster and slanted harder), falling the same height as before.
			var storm := kind != "rain"
			pm.direction = Vector3(0.35, -1, 0.1) if storm else Vector3(0.15, -1, 0.05)
			pm.spread = 3.0
			pm.initial_velocity_min = 22.0 if storm else 17.0
			pm.initial_velocity_max = 27.0 if storm else 21.0
			pm.turbulence_enabled = false
			pm.emission_box_extents = Vector3(50, 1, 50)
			mesh.size = Vector3(0.025, 0.55, 0.025)
			m.set_shader_parameter("tint", Color(col.lightened(0.4), 0.35 + 0.3 * k))
			rain.lifetime = 0.75 if storm else 0.95

func weather_drawn() -> String:
	return _kind

## A zone up close: fog that fills the place (volumetric, so lights and the sun shine through it), glowing faintly in
## its colour, inside the mist wall the base theme draws for seeing it from afar.
func zone_mist(z: Dictionary) -> Node3D:
	var n := super.zone_mist(z)
	var r := float(z["r"])
	var h := float(z.get("height", 30.0))
	var k := clampf(float(z.get("intensity", 0.5)), 0.0, 1.0)
	var col := Color(str(z.get("color", "#888888")))
	var fv := FogVolume.new()
	fv.shape = RenderingServer.FOG_VOLUME_SHAPE_CYLINDER
	fv.size = Vector3(r * 2.0, h, r * 2.0)
	fv.position.y = h / 2.0
	var fm := FogMaterial.new()
	fm.density = 0.04 + 0.3 * k
	fm.albedo = col
	fm.emission = col * (0.08 + 0.12 * k)
	fm.height_falloff = 0.08
	fm.edge_fade = 0.25
	fv.material = fm
	n.add_child(fv)
	return n


## The map (M): the meadow shaded by the hills, water, and each object coloured by its type.
func map_image(px: int) -> Image:
	var img := Image.create(px, px, false, Image.FORMAT_RGB8)
	var step: float = world.size / px
	for j in px:
		for i in px:
			var x := (i + 0.5) * step
			var y := (j + 0.5) * step
			var e := maxf(step, 2.0)
			var slope := (height(x - e, y - e) - height(x + e, y + e)) / (2.0 * e)
			var lit := 0.9 * (1.0 + clampf(slope * 1.2, -0.25, 0.25))
			if world.has_sea and height(x, y) < world.sea_level + 0.1:
				img.set_pixel(i, j, Color("2f6f8f"))
			else:
				img.set_pixel(i, j, Color(0.36 * lit, 0.54 * lit, 0.2 * lit))
	var px_of := func(x: float, y: float) -> Vector2i:
		return Vector2i(clampi(int(x / step), 0, px - 1), clampi(int(y / step), 0, px - 1))
	for r in world.rivers:
		var pts: Array = r["pts"]
		for k in pts.size() - 1:
			for t in 6:
				var p: Vector2i = px_of.call(lerpf(pts[k][0], pts[k + 1][0], t / 6.0), lerpf(pts[k][1], pts[k + 1][1], t / 6.0))
				img.fill_rect(Rect2i(p - Vector2i(1, 1), Vector2i(3, 3)), Color("2f7f8f"))
	for r in world.roads:
		var pts: Array = r["pts"]
		var wide := maxi(1, int(2.0 * float(r.get("half_width", 3.0)) / step))
		for k in pts.size() - 1:
			var segs := maxi(1, int(Vector2(pts[k][0], pts[k][1]).distance_to(Vector2(pts[k + 1][0], pts[k + 1][1])) / step))
			for t in segs + 1:
				var p: Vector2i = px_of.call(lerpf(pts[k][0], pts[k + 1][0], float(t) / segs), lerpf(pts[k][1], pts[k + 1][1], float(t) / segs))
				img.fill_rect(Rect2i(p, Vector2i(wide, wide)), Color("c9b48a"))
	var colours := {"house": Color("b5523b"), "keep": Color("8d867d"), "castle": Color("8d867d"), "tower": Color("6f6a62"),
		"wall": Color("5d5850"), "crag": Color("6e675e"), "lake": Color("2f7f8f"), "square": Color("b8a77e"), "city": Color("9b8f72")}
	for pass_ in 2: # open ground first, then what stands on it
		for o in world.objects:
			var t := String(o.get("t", ""))
			if not o.has("b") or not colours.has(t) or (pass_ == 0) != (t == "city" or t == "square"):
				continue
			var b: Array = o["b"]
			var p0: Vector2i = px_of.call(b[0], b[1])
			var p1: Vector2i = px_of.call(b[2], b[3])
			img.fill_rect(Rect2i(p0, (p1 - p0).max(Vector2i(1, 1))), colours[t])
	return img

func smoke_checks() -> Array:
	var batched := 0
	for c in _chunks.values():
		batched += (c as Node).find_children("*", "MultiMeshInstance3D", true, false).size()
	return [
		["the sky is the day and night sky", sky_mat != null and sky_mat.shader != null, "phase %.2f" % day_phase()],
		["the world has fires, braziers and lanterns", _sources.size() > 0, "%d places with lights" % _sources.size()],
		["fantasy theme built kit pieces", batched > 0, "%d batches" % batched],
		["characters are KayKit models", _built_bodies > 0, "%d made" % _built_bodies],
		["ground built from the heights", _terrain.size() > 0, "%d chunks" % _terrain.size()],
	]

# ---------------------------------------------------------------- builders and pieces

const CHARS := "res://themes/fantasy/assets/characters/"

const DUNGEON := "res://themes/fantasy/assets/dungeon/"

const VILLAGE := "res://themes/fantasy/assets/village/"

const VS := 1.4 # Quaternius builds in metres; this brings the village up to the chibi characters' size

## Roofs by footprint [width along x, length along z], ridge along z, in kit units.
const ROOFS := [[4, 4], [4, 6], [4, 8], [6, 4], [6, 6], [6, 8], [6, 10], [6, 12], [6, 14], [8, 8], [8, 10], [8, 12], [8, 14]]

const FOREST := "res://themes/fantasy/assets/forest/"

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
		for p in ["res://themes/fantasy/assets/characters/mob.tscn", "res://themes/fantasy/assets/characters/mob_mage.tscn", "res://themes/fantasy/assets/characters/mob_minion.tscn",
				"res://themes/fantasy/assets/characters/mob_rogue.tscn", "res://themes/fantasy/assets/monsters/mob_imp.tscn", "res://themes/fantasy/assets/monsters/mob_puglin.tscn"]:
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

## A village house from the Medieval Village kit: a stone ground floor (with the door) and half-timbered upper
## floors, corner posts, windows with shutters, a tiled roof with gable ends, chimney and dormers, vines, a
## balcony now and then, and a paved yard with a fence, a wagon and crates out front. The walls fill the box
## exactly, so the house is what the server's wall is.
func _build_house(b: Array, base: float, v: int, batch: Dictionary) -> Node3D:
	var x0: float = b[0]
	var y0: float = b[1]
	var x1: float = b[2]
	var y1: float = b[3]
	var hsh := absi(v)
	var rng := RandomNumberGenerator.new()
	rng.seed = hsh
	var node := Node3D.new()
	node.position.y = base
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

## The crag itself: KayKit spires towards the middle and boulders round the flanks, sized to the height the
## crag should reach at each spot and kept inside the box.
func _crag_rocks(b: Array, peak: float, r: float) -> Node3D:
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
			_add_rock(per, pick, s * 0.6, Vector3(x, height(x, y) - 0.6, y), rng.randf() * TAU)
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

## A wooden dock from the middle of one shore out over the water: plank floor (dark or light, with half and
## corner boards at the end) on posts. Decoration over the lake, which the server keeps you out of.
func _dock(b: Array, water: float, v: int) -> Node3D:
	var hsh := absi(v)
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
	var pts := PackedVector2Array()
	var lvl := PackedFloat32Array()
	for q in r["pts"]:
		pts.append(Vector2(q[0], q[1]))
		lvl.append(float(q[2]))
	var verts := PackedVector3Array()
	var norms := PackedVector3Array()
	var idxs := PackedInt32Array()
	var half: float = float(r.get("half_width", 5.0)) + 1.0 # inside the lip, where the bank already stands above the water
	for k in pts.size():
		var a := pts[maxi(0, k - 1)]
		var c := pts[mini(pts.size() - 1, k + 1)]
		var dir := (c - a).normalized()
		var side := Vector2(-dir.y, dir.x) * half
		var y: float = lvl[k]
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
		var p := pts[k] + side * (half + 0.5 + rng.randf_range(0.3, 2.3))
		if rng.randf() < 0.55:
			var small := [["Rock_1_A_Color1", 0.61, 0.54, 0.0], ["Rock_3_E_Color1", 1.18, 1.29, -0.59], ["Rock_1_J_Color1", 3.76, 3.36, 0.0]]
			var pick: Array = small[rng.randi() % small.size()]
			_add_rock(rocks, pick, rng.randf_range(1.0, 2.6) / float(pick[1]) * 1.5, Vector3(p.x, height(p.x, p.y), p.y), rng.randf() * TAU)
		else:
			bushes.append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * rng.randf_range(2.2, 3.4)), Vector3(p.x, height(p.x, p.y), p.y)))
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

func _keep(o: Dictionary, batch: Dictionary) -> Node3D:
	var b: Array = o["b"]
	var x0: float = b[0]
	var y0: float = b[1]
	var x1: float = b[2]
	var y1: float = b[3]
	var hsh := absi(int(o.get("v", 0)))
	# Storeys: towers and the castle stand tall, a curtain wall two high, a keep one or two.
	var layers: int = {"tower": 3, "castle": 3, "wall": 2}.get(String(o.get("t", "keep")), 1 + hsh % 2)
	var top := 4.0 * layers
	var node := Node3D.new()
	node.position.y = float(o.get("y", 0.0))
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
				var upper := layer == layers - 1
				var v := (hsh + i * 7 + si * 3 + layer * 5) % 17
				var piece := "wall.gltf"
				if si == gate_side and i == count / 2 and layer == 0 and length >= 12.0:
					piece = "wall_gated.gltf" # the way in, barred (the server's wall is solid)
				elif v == 0:
					piece = "wall_cracked.gltf"
				elif v == 1 and layer == 0:
					piece = "wall_arched.gltf"
				elif v == 2 and upper:
					piece = "wall_window_closed.gltf"
				elif v == 3 and upper:
					piece = "wall_window_open.gltf"
				elif v == 4 and upper:
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
			var at := Vector3(x0 + (x1 - x0) * (i + 0.5) / tx, top + 0.05, y0 + (y1 - y0) * (j + 0.5) / ty)
			put.call("floor_tile_large_rocks.gltf" if (hsh + i * 3 + j * 7) % 9 == 0 else "floor_tile_large.gltf", Transform3D(tile, at))
	var stores := ["barrel_small.gltf", "barrel_small_stack.gltf", "box_large.gltf", "box_stacked.gltf", "crates_stacked.gltf",
		"keg_decorated.gltf", "trunk_large_A.gltf", "barrel_large.gltf"]
	var rng := RandomNumberGenerator.new()
	rng.seed = hsh
	for k in clampi(int((x1 - x0) * (y1 - y0) / 140.0), 1, 7):
		var at := Vector3(rng.randf_range(x0 + 2.5, maxf(x0 + 2.6, x1 - 2.5)), top + 0.1, rng.randf_range(y0 + 2.5, maxf(y0 + 2.6, y1 - 2.5)))
		put.call(stores[rng.randi() % stores.size()], Transform3D(Basis(Vector3.UP, rng.randf() * TAU), at))
	# Standing torches at the roof's corners, twice life size so they read from the camera.
	for c in [Vector3(x0 + 1.4, 0, y0 + 1.4), Vector3(x1 - 1.4, 0, y0 + 1.4), Vector3(x0 + 1.4, 0, y1 - 1.4), Vector3(x1 - 1.4, 0, y1 - 1.4)]:
		put.call("torch_lit.gltf", Transform3D(Basis.from_scale(Vector3.ONE * 2.0), c + Vector3(0, top + 0.85, 0)))
		_flame(node, c + Vector3(0, top + 0.85 + 1.5, 0), 0.22)
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
	roof.position = Vector3((x0 + x1) / 2.0, top - 0.25, (y0 + y1) / 2.0) # under the tiles, filling any gap
	var rm := StandardMaterial3D.new()
	rm.albedo_color = Color("5a5560")
	rm.roughness = 1.0
	roof.material_override = rm
	node.add_child(roof)
	return node

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

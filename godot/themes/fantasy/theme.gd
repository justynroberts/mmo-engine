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
var sea_node: MeshInstance3D     # the open sea, raised and lowered by the tide
var _cloud_off := Vector2.ZERO   # how far the clouds' shadows have drifted (world_cloud_off)
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
const LifeScript := preload("res://themes/fantasy/life.gd")
const TownsScript := preload("res://themes/fantasy/towns.gd")
const LandScript := preload("res://themes/fantasy/land.gd")
var life: Node3D             # birds, deer, stock, chimney smoke, fireflies, and the weather on the ground (life.gd)
var _life_ms := 0
var _chimney_at := Vector3.INF # set by _house_roof: where the last house's chimney stands (before its base height)
var _snow_on := false        # snow lies: every batch and block wears the snow overlay
var _snow_overlay: ShaderMaterial
var grass: MultiMeshInstance3D  # themes/fantasy/grass.gd, round the player (null with --no-grass or no heights)
var air: Node3D                 # themes/fantasy/air.gd: mist, haze and wisps between the weathers
var _here := Vector2.ZERO        # where we are, for the air
var towns = TownsScript.new()     # every settlement's look and trade (towns.gd)
var land = LandScript.new()       # cold north, wet and wooded ground, fields, road junctions (land.gd)
var _windows := {}               # "x,y" (a house's middle, rounded) -> its lit windows, shown while someone is in
var _lit := {}                   # what main.gd was last told is lit: "x,y" -> [who, an inn]
var _pal_meshes := {}            # "path#palette" -> a piece with its materials tinted to that palette
var _pal_mats := {}

func setup(p_info: Dictionary, p_world: RefCounted) -> void:
	info = p_info
	world = p_world
	land.setup(world, towns)
	towns.cold_at = land.cold_at
	towns.survey(world)
	_prefetch()
	# Every palette's tinted materials and washed textures now, while loading, not as the first village comes near;
	# its copy of each piece over the next frames (_warm).
	for pal in TownsScript.PALETTES.size():
		for piece in ["Roof_RoundTiles_6x8", "Wall_UnevenBrick_Window_Wide_Round", "Roof_Front_Brick6"]:
			_mesh_pal(VILLAGE + piece + ".gltf", pal)
		if pal > 0:
			for f in _fetching:
				if String(f).begins_with(VILLAGE):
					_warming.append([f, pal])
	LifeScript.ensure_globals() # before any shader that reads them is made
	RenderingServer.global_shader_parameter_set("world_cold_band", land.cold_band)
	# A world whose mountains stand far above 40 (a big one) keeps its snow and heath for its heights, not the
	# hills of a small world: the lines are scaled by how much taller it is.
	RenderingServer.global_shader_parameter_set("world_relief", maxf(1.0, world.height_top / 40.0))
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
	air = preload("res://themes/fantasy/air.gd").new()
	add_child(air)
	if world.has_heights and not OS.get_cmdline_user_args().has("--no-grass"):
		grass = preload("res://themes/fantasy/grass.gd").new()
		grass.land = land
		grass.setup(world)
		add_child(grass)
	# The ground: meadow in the south giving way to cold heath and frost in the north (world_cold_band), lush by water,
	# leaf litter under trees, fields by farmsteads, sand and shingle on the shore, rock on slopes and snow up high,
	# all from world position and the cell's land map (land.gd ground_map) so every client sees the same land.
	var sh := Shader.new()
	sh.code = """shader_type spatial;
uniform float rocky = 0.0; // 1 on crags: rock except in the flattest hollows
uniform sampler2D land_map : filter_linear, repeat_disable; // land.gd ground_map: r road, g paved or field, b wet, a woodland
uniform vec2 land_origin;
uniform float land_on = 0.0;
uniform sampler2D field_map : filter_nearest, repeat_disable; // land.gd field_map: r crop (times 40), g furrow angle
uniform float field_on = 0.0;
uniform float sea_level = -1000.0;
global uniform float world_wet;     // life.gd: how wet the ground is after rain
global uniform float world_puddles; // standing water, which lasts longer
global uniform float world_snow;    // snow lying
global uniform vec2 world_cold_band; // land.gd: fully cold north of x, none south of y (world z)
global uniform float world_relief;   // the snow and heath lines scaled for a world with taller land (1 for small ones)
global uniform float world_autumn;  // life.gd: the year's turn to gold, 0 .. 1
global uniform float world_tide;    // theme.gd: how far the tide stands above or below sea level now
global uniform vec2 world_clouds;   // theme.gd: cloud shadows, x how dark, y the cover threshold
global uniform vec2 world_cloud_off; // theme.gd: how far they have drifted
varying vec3 wp;
varying vec3 wn;
// Which stretches of coast are sand (1) and which shingle and rock (0): the same as theme.gd coast_sand.
uint uh(ivec2 c) { uint u = uint(c.x) * 1664525u + uint(c.y) * 1013904223u; u ^= u >> 16u; u *= 2146121005u; u ^= u >> 15u; u *= 2221713035u; u ^= u >> 16u; return u; }
float ch(ivec2 c) { return float(uh(c) >> 8u) / 16777215.0; }
float coast_sand(vec2 p) {
	vec2 g = p / 260.0; vec2 i = floor(g); vec2 f = g - i; vec2 u = f * f * (3.0 - 2.0 * f); ivec2 k = ivec2(i);
	return smoothstep(0.42, 0.58, mix(mix(ch(k), ch(k + ivec2(1, 0)), u.x), mix(ch(k + ivec2(0, 1)), ch(k + ivec2(1, 1)), u.x), u.y));
}
float h(vec2 p) { vec3 q = fract(vec3(p.xyx) * 0.1031); q += dot(q, q.yzx + 33.33); return fract((q.x + q.y) * q.z); }
float n(vec2 p) {
	vec2 i = floor(p); vec2 f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(h(i), h(i + vec2(1, 0)), u.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) { float v = 0.0; float a = 0.5; for (int k = 0; k < 4; k++) { v += a * n(p); p *= 2.03; a *= 0.5; } return v; }
void vertex() { wp = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz; wn = normalize((MODEL_MATRIX * vec4(NORMAL, 0.0)).xyz); }
void fragment() {
	vec2 q = wp.xz;
	vec4 lm = land_on > 0.5 ? texture(land_map, (q - land_origin) / 128.0) : vec4(0.0);
	float cold = 1.0 - smoothstep(world_cold_band.x, world_cold_band.y, wp.z + 220.0 * (fbm(q / 300.0) - 0.5));
	// Meadow: two greens, dry patches and worn dirt.
	vec3 c = mix(vec3(0.36, 0.56, 0.20), vec3(0.50, 0.66, 0.25), smoothstep(0.3, 0.7, fbm(q / 14.0)));
	c = mix(c, vec3(0.46, 0.43, 0.24), smoothstep(0.55, 0.75, fbm(q / 90.0)) * 0.5);
	c = mix(c, vec3(0.44, 0.36, 0.23), smoothstep(0.62, 0.78, fbm(q / 18.0 + 7.0)) * 0.45);
	// Clover and darker sward in drifts, so the meadow is never one flat green.
	c = mix(c, vec3(0.30, 0.50, 0.22), smoothstep(0.58, 0.72, fbm(q / 6.0 + 21.0)) * 0.35);
	// High ground in the warm south: heath and dry gold.
	float up = smoothstep(17.0 * world_relief, 27.0 * world_relief, wp.y + 4.0 * n(q * 0.05)) * (1.0 - cold);
	c = mix(c, mix(vec3(0.56, 0.52, 0.29), vec3(0.45, 0.36, 0.31), smoothstep(0.4, 0.65, fbm(q / 20.0 + 5.0))), up * 0.4);
	// The cold north: grey-green tussock, heather, stony ground and frost.
	vec3 tun = mix(vec3(0.42, 0.48, 0.37), vec3(0.54, 0.56, 0.44), smoothstep(0.3, 0.7, fbm(q / 16.0)));
	tun = mix(tun, vec3(0.46, 0.36, 0.38), smoothstep(0.55, 0.72, fbm(q / 22.0 + 3.0)) * 0.55);
	tun = mix(tun, vec3(0.52, 0.52, 0.50), smoothstep(0.62, 0.8, fbm(q / 9.0 + 11.0)) * 0.5);
	c = mix(c, tun, cold);
	// Autumn: the meadow goes over to straw and rust in drifts.
	c = mix(c, mix(vec3(0.62, 0.52, 0.24), vec3(0.55, 0.36, 0.18), smoothstep(0.4, 0.7, fbm(q / 24.0 + 9.0))), world_autumn * (1.0 - cold) * 0.55);
	c *= 0.94 + 0.12 * fbm(q * 0.6);
	c = mix(c, vec3(0.80, 0.85, 0.90), cold * cold * smoothstep(0.55, 0.72, fbm(q * 0.9 + 4.0)) * 0.3);
	// By water the grass is lush and dark, and the very edge is mud.
	float wet = lm.b;
	c = mix(c, mix(vec3(0.24, 0.45, 0.17), vec3(0.32, 0.41, 0.27), cold), smoothstep(0.05, 0.6, wet) * 0.7);
	c = mix(c, vec3(0.38, 0.33, 0.24), smoothstep(0.88, 1.0, wet + 0.1 * (n(q * 0.8) - 0.5)) * 0.8);
	// Under trees: leaf litter and moss (needles and stones in the north).
	float wood = smoothstep(0.1, 0.7, lm.a + 0.3 * (n(q * 0.5) - 0.5));
	vec3 litter = mix(vec3(0.31, 0.37, 0.17), vec3(0.40, 0.34, 0.21), smoothstep(0.4, 0.6, fbm(q * 0.8)));
	litter = mix(litter, vec3(0.36, 0.36, 0.32), cold * 0.6) * (0.88 + 0.24 * h(floor(q * 3.0)));
	c = mix(c, litter, wood * 0.55);
	// Fields by farmsteads: strips of wheat, greens, barley and plough, furrowed one way or the other.
	float field = lm.g * (1.0 - smoothstep(0.0, 0.08, lm.r));
	float across = lm.g > 0.75 ? q.x : q.y;
	float along = lm.g > 0.75 ? q.y : q.x;
	float strip = h(vec2(floor(across / 6.5), floor(along / 45.0)) + 3.1);
	vec3 crop = strip < 0.35 ? vec3(0.80, 0.67, 0.33) : strip < 0.6 ? vec3(0.37, 0.55, 0.20) : strip < 0.82 ? vec3(0.46, 0.35, 0.23) : vec3(0.64, 0.64, 0.31);
	crop = mix(crop, crop * vec3(0.85, 0.9, 0.92) + vec3(0.04), cold);
	crop *= 0.8 + 0.28 * (0.5 + 0.5 * sin(across * 3.4)) + 0.08 * (n(q * 1.3) - 0.5);
	c = mix(c, crop, smoothstep(0.05, 0.35, field + 0.1 * (n(q * 0.9) - 0.5)) * step(0.02, field));
	// A world file's own fields: each its crop, furrowed (or mown) along its own angle.
	if (field_on > 0.5) {
		vec2 fm = texture(field_map, (q - land_origin) / 128.0).rg;
		float kind = floor(fm.r * 255.0 / 40.0 + 0.5);
		if (kind > 0.5) {
			float ang = fm.g * 3.14159265;
			float across = dot(q, vec2(-sin(ang), cos(ang)));
			float rows = 0.5 + 0.5 * sin(across * 3.4);
			vec3 fc;
			if (kind < 1.5) { // pasture: lush, grazed in patches
				fc = mix(vec3(0.30, 0.52, 0.19), vec3(0.40, 0.60, 0.22), smoothstep(0.3, 0.7, fbm(q / 9.0 + 4.0)));
				fc = mix(fc, vec3(0.45, 0.50, 0.26), smoothstep(0.6, 0.8, fbm(q / 4.0)) * 0.3);
			} else if (kind < 2.5) { // hay: mown in broad stripes
				fc = mix(vec3(0.52, 0.60, 0.27), vec3(0.62, 0.64, 0.32), 0.5 + 0.5 * sin(across * 0.45));
			} else if (kind < 3.5) { // wheat
				fc = vec3(0.80, 0.67, 0.33) * (0.82 + 0.25 * rows);
			} else if (kind < 4.5) { // barley
				fc = vec3(0.71, 0.67, 0.39) * (0.84 + 0.22 * rows);
			} else if (kind < 5.5) { // oats
				fc = vec3(0.62, 0.64, 0.38) * (0.84 + 0.22 * rows);
			} else { // potatoes: ridged earth with green tops
				float ridge = 0.5 + 0.5 * sin(across * 2.6);
				fc = mix(vec3(0.40, 0.31, 0.21), vec3(0.30, 0.47, 0.19), smoothstep(0.45, 0.7, ridge + 0.2 * (n(q * 2.0) - 0.5)));
			}
			fc = mix(fc, mix(vec3(0.62, 0.52, 0.24), vec3(0.50, 0.40, 0.24), step(2.5, kind)), world_autumn * 0.5);
			fc = mix(fc, fc * vec3(0.85, 0.9, 0.92) + vec3(0.04), cold);
			fc *= 0.94 + 0.12 * n(q * 1.3);
			c = mix(c, fc, 0.94);
		}
	}
	// Steep ground is bare rock.
	float slope = 1.0 - clamp(wn.y, 0.0, 1.0);
	vec3 rock = vec3(0.47, 0.45, 0.42) * (0.8 + 0.4 * fbm(q * 0.25 + wp.y * 0.1));
	rock = mix(rock, rock * vec3(0.92, 0.96, 1.04), cold);
	c = mix(c, rock, max(smoothstep(0.28, 0.5, slope), rocky * smoothstep(0.04, 0.12, slope + 0.05 * n(q * 0.3))));
	// The shore: wide sand beaches along some stretches of coast (coast_sand), a narrow strip of shingle elsewhere,
	// grey in the north. The sea wets it up to where the tide stands now, the falling tide leaves it damp below high
	// water, and a dark line of wrack marks high water on the sand.
	// Above the highest beach (top + 0.3 with the 0.3 of noise) there is none: most of the land skips all this.
	if (wp.y < sea_level + 2.6) {
		float sandy = coast_sand(q);
		float top = sea_level + mix(0.6, 2.0, sandy);
		float beach = 1.0 - smoothstep(top - 0.5, top + 0.3, wp.y + 0.6 * (n(q * 0.3) - 0.5));
		vec3 sand = mix(vec3(0.79, 0.72, 0.54), vec3(0.56, 0.55, 0.53), cold) * (0.9 + 0.15 * fbm(q * 1.5));
		vec3 shingle = vec3(0.50, 0.49, 0.47) * (0.72 + 0.45 * n(q * 4.0)) * (0.9 + 0.15 * fbm(q * 0.8));
		sand = mix(shingle, sand, sandy);
		float tidewet = 1.0 - smoothstep(sea_level + world_tide + 0.05, sea_level + world_tide + 0.45, wp.y + 0.15 * n(q * 0.8));
		float damp = 1.0 - smoothstep(sea_level + world_tide, sea_level + 0.55, wp.y);
		sand *= 1.0 - 0.3 * max(tidewet, damp * 0.45);
		float wrack = smoothstep(0.1, 0.0, abs(wp.y - (sea_level + 0.5 + 0.1 * n(q * 0.3)))) * smoothstep(0.35, 0.65, n(q * 0.08)) * sandy;
		sand = mix(sand, vec3(0.30, 0.27, 0.17), wrack * 0.45);
		c = mix(c, sand, beach * (1.0 - smoothstep(0.35, 0.6, slope)));
	}
	// Roads: packed earth with a ragged grassy edge in the country, cobbles in the city.
	float road = 0.0;
	if (land_on > 0.5) {
		road = smoothstep(0.2, 0.6, lm.r + 0.25 * (n(q * 0.7) - 0.5));
		vec3 dirt = vec3(0.52, 0.43, 0.30) * (0.86 + 0.22 * fbm(q * 0.9)) * (0.92 + 0.12 * n(q * 3.0));
		dirt = mix(dirt, vec3(0.47, 0.44, 0.39) * (0.86 + 0.22 * fbm(q * 0.9)), cold * 0.6);
		vec2 cq = q * vec2(1.25, 1.6);
		cq.x += 0.5 * floor(cq.y);
		vec2 cf = abs(fract(cq) - 0.5);
		float stone = smoothstep(0.5, 0.38, max(cf.x, cf.y));
		vec3 cob = mix(vec3(0.27, 0.26, 0.24), vec3(0.55, 0.53, 0.49) * (0.8 + 0.35 * h(floor(cq))), stone);
		c = mix(c, mix(dirt, cob, smoothstep(0.4, 0.6, lm.g)), road);
	}
	// Snow on high ground, coming down far lower in the north, and lying in drifts on the level up there.
	float snow_line = mix(30.0, 18.0, cold) * world_relief;
	c = mix(c, vec3(0.93, 0.95, 0.98), smoothstep(snow_line, snow_line + 12.0, wp.y + 6.0 * n(q * 0.05)) * (1.0 - smoothstep(0.55, 0.8, slope)));
	// Weather that lies: snow on the level ground (roads trodden through), then rain darkening everything and
	// shining on the roads, with puddles in the ruts and hollows that outlast it.
	float lvl = smoothstep(0.75, 0.95, wn.y);
	float drifts = smoothstep(0.85, 1.0, cold) * smoothstep(0.6, 0.72, fbm(q / 14.0 + 2.0)) * 0.6;
	float sn = lvl * max(smoothstep(0.15, 0.5, world_snow * 1.2 + 0.4 * (fbm(q * 0.35) - 0.5)), drifts) * (1.0 - 0.5 * road);
	c = mix(c, vec3(0.9, 0.92, 0.96) * (0.95 + 0.05 * n(q * 2.0)), sn);
	float wk = world_wet * (1.0 - sn);
	c *= 1.0 - 0.3 * wk * (0.6 + 0.4 * road);
	float pud = world_puddles * (1.0 - sn) * lvl * smoothstep(0.6, 0.7, fbm(q / 2.6 + 3.1)) * (0.3 + 0.7 * road);
	c = mix(c, c * 0.4 + vec3(0.02, 0.03, 0.045), pud);
	// Clouds' shadows sweeping over the land with the wind (the same in the water shader).
	if (world_clouds.x > 0.0) {
		vec2 cq = q + world_cloud_off;
		float cl = 0.65 * n(cq / 90.0) + 0.35 * n(cq / 37.0 + 5.3);
		c *= 1.0 - 0.32 * world_clouds.x * smoothstep(world_clouds.y, world_clouds.y + 0.12, cl);
	}
	ALBEDO = pow(c, vec3(2.2)); // the colours above are sRGB; albedo is linear
	ROUGHNESS = mix(mix(1.0, mix(0.6, 0.28, road), wk), 0.04, pud);
	SPECULAR = 0.5 * max(wk * (0.3 + 0.7 * road), pud);
}"""
	var m := ShaderMaterial.new()
	m.shader = sh
	m.set_shader_parameter("sea_level", float(world.sea_level) if world.has_sea else -1000.0)
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
		var sea_mat := _water().duplicate() as ShaderMaterial
		sea_mat.set_shader_parameter("swell", 1.0)
		sea.material_override = sea_mat
		sea.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		sea.position = Vector3(ext.get_center().x, world.sea_level, ext.get_center().y)
		add_child(sea)
		sea_node = sea
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
	_start_life()

func _start_life() -> void:
	life = LifeScript.new()
	add_child(life)
	life.setup(self)
	var sh := Shader.new()
	sh.code = """shader_type spatial;
render_mode blend_mix, depth_draw_never;
global uniform float world_snow;
varying vec3 wn;
varying vec3 wp;
float h(vec2 p) { vec3 q = fract(vec3(p.xyx) * 0.1031); q += dot(q, q.yzx + 33.33); return fract((q.x + q.y) * q.z); }
void vertex() { wn = normalize(MODEL_NORMAL_MATRIX * NORMAL); wp = (MODEL_MATRIX * vec4(VERTEX, 1.0)).xyz; }
void fragment() {
	// Snow on whatever faces up: roof slopes, wall tops, the tops of blocks, branches; thin and patchy as it starts.
	float up = smoothstep(0.3, 0.62, wn.y);
	float a = up * smoothstep(0.0, 0.5, world_snow * 1.4 - 0.35 * h(floor(wp.xz * 3.0)));
	if (a < 0.02) { discard; }
	ALBEDO = vec3(0.86, 0.88, 0.92);
	ROUGHNESS = 0.85;
	ALPHA = a;
}"""
	_snow_overlay = ShaderMaterial.new()
	_snow_overlay.shader = sh

## A house's windows lit from within, one on each wall, and a word on who is in for those close by: hidden till
## the server says someone is home (set_lit).
func _house_windows(b: Array, y: float) -> Node3D:
	var key := "%d,%d" % [roundi((float(b[0]) + float(b[2])) / 2.0), roundi((float(b[1]) + float(b[3])) / 2.0)]
	var root := Node3D.new()
	var glow := StandardMaterial3D.new()
	glow.albedo_color = Color("ffcf7a")
	glow.emission_enabled = true
	glow.emission = Color("ffb04a")
	glow.emission_energy_multiplier = 3.0
	glow.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	var x0 := float(b[0]); var z0 := float(b[1]); var x1 := float(b[2]); var z1 := float(b[3])
	var mx := (x0 + x1) / 2.0
	var mz := (z0 + z1) / 2.0
	var ground := y + height(mx, mz)
	for side in [[Vector3(mx, 0, z0 - 0.06), Vector3(0.55, 0.6, 0.02)], [Vector3(mx, 0, z1 + 0.06), Vector3(0.55, 0.6, 0.02)],
			[Vector3(x0 - 0.06, 0, mz), Vector3(0.02, 0.6, 0.55)], [Vector3(x1 + 0.06, 0, mz), Vector3(0.02, 0.6, 0.55)]]:
		var w := MeshInstance3D.new()
		var bm := BoxMesh.new()
		bm.size = side[1]
		w.mesh = bm
		w.material_override = glow
		w.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
		w.position = Vector3(side[0].x, ground + 1.55, side[0].z)
		root.add_child(w)
	var l := OmniLight3D.new()
	l.light_color = Color("ffb35a")
	l.light_energy = 0.9
	l.omni_range = 7.0
	l.position = Vector3(mx, ground + 1.6, z1 + 1.2)
	l.distance_fade_enabled = true
	l.distance_fade_begin = 60.0
	l.distance_fade_length = 20.0
	root.add_child(l)
	var label := Label3D.new()
	label.billboard = BaseMaterial3D.BILLBOARD_ENABLED
	label.font_size = 28
	label.pixel_size = 0.008
	label.outline_size = 6
	label.modulate = Color("ffe2b0")
	label.position = Vector3(mx, ground + 3.4, mz)
	label.visibility_range_end = 14.0
	label.name = "who"
	root.add_child(label)
	root.set_meta("key", key)
	_windows[key] = root
	_show_lit(root, key)
	return root

func _show_lit(root: Node3D, key: String) -> void:
	var on := _lit.has(key)
	root.visible = on
	if on:
		var who := String(_lit[key][0])
		(root.get_node("who") as Label3D).text = ("The inn" + (": " + who + " within" if who != "" else "")) if bool(_lit[key][1]) else (who + " is home" if who != "" else "")

## The lit windows built and shown now, and what the nearest few say (the screenshot run reports it).
func lit_report(at: Vector2) -> Dictionary:
	var shown := []
	for key in _windows:
		var root = _windows[key]
		if is_instance_valid(root) and root.visible:
			var xy := String(key).split(",")
			shown.append([Vector2(float(xy[0]), float(xy[1])).distance_to(at), (root.get_node("who") as Label3D).text])
	shown.sort_custom(func(a, b): return a[0] < b[0])
	return {"built": _windows.size(), "told": _lit.size(), "lit": shown.size(), "nearest": shown.slice(0, 3).map(func(e): return "%s (%d away)" % [e[1], int(e[0])])}

## Which houses have someone in now (main.gd, from the server): their windows light, the rest go dark.
func set_lit(lit: Dictionary) -> void:
	_lit = lit.duplicate()
	for key in _windows.keys():
		var root = _windows[key]
		if not is_instance_valid(root):
			_windows.erase(key)
			continue
		_show_lit(root, key)

## The time of year (main.gd, from the schedule): autumn turns the meadows gold, winter leaves snow lying.
func set_season(s: String) -> void:
	if life:
		life.season_snow = 0.55 if s == "winter" else 0.0
		life.season_autumn = 1.0 if s == "autumn" else 0.0

## Snow lies (or has melted): roofs, tops and blocks wear the snow overlay, or stop (life.gd says when).
func show_snow(on: bool) -> void:
	_snow_on = on
	for n in get_tree().get_nodes_in_group("snowable"):
		(n as GeometryInstance3D).material_overlay = _snow_overlay if on else null

func _snowable(g: GeometryInstance3D) -> void:
	g.add_to_group("snowable")
	if _snow_on:
		g.material_overlay = _snow_overlay

func build_blocks_from(res: Dictionary) -> Node3D:
	var n := super.build_blocks_from(res)
	for m in n.find_children("*", "MeshInstance3D", true, false):
		_snowable(m as GeometryInstance3D)
	return n

func remove_lights(owner) -> void:
	super.remove_lights(owner)
	if life:
		life.chimneys.erase(owner)

func ground_material() -> Material:
	return terrain_mat

## A cell with roads, water, trees or fields in it gets its own copy of the ground material, with the land map that
## land.gd draws for it (refreshed by build_cell once the cell's trees have streamed in).
func ground_material_at(cx: int, cy: int) -> Material:
	var map := land.ground_map(cx, cy)
	var fields := land.field_map(cx, cy)
	if map.is_empty() and fields.is_empty():
		return terrain_mat
	var m := terrain_mat.duplicate() as ShaderMaterial
	m.set_shader_parameter("land_origin", Vector2(cx * world.CELL, cy * world.CELL))
	if not map.is_empty():
		m.set_shader_parameter("land_map", ImageTexture.create_from_image(Image.create_from_data(land.MAP_N, land.MAP_N, false, Image.FORMAT_RGBA8, map)))
		m.set_shader_parameter("land_on", 1.0)
	if not fields.is_empty():
		m.set_shader_parameter("field_map", ImageTexture.create_from_image(Image.create_from_data(land.MAP_N, land.MAP_N, false, Image.FORMAT_RG8, fields)))
		m.set_shader_parameter("field_on", 1.0)
	return m

## Building blocks from the Medieval Village textures, tiled so each block shows one texture repeat: the eight
## textures as they are, then glass, and the dressed stone tinted three ways (slate, moss, sandstone).
const BLOCK_TEXTURES := ["RockTrim", "WoodTrim", "RedBrick", "Plaster", "Brick", "UnevenBrick", "RoundTiles", "MetalOrnaments", "", "RockTrim", "RockTrim", "RockTrim", "", "RockTrim", "", "", ""]
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
	var t := Time.get_ticks_usec()
	if grass:
		grass.follow(center, float(_wx["wind"]))
	t = lap("grass", t)
	_here = center
	super.frame(center, camera_zoom)
	t = lap("theme", t)
	var now := Time.get_ticks_msec()
	var dt := clampf((now - _life_ms) / 1000.0, 0.0, 0.25) if _life_ms > 0 else 0.0
	_life_ms = now
	if life:
		life.step(center, dt)
	t = lap("life", t)
	_warm(center, 1.5)
	lap("warm", t)
	var tide := tide_now()
	RenderingServer.global_shader_parameter_set("world_tide", tide)
	if sea_node:
		sea_node.position.y = world.sea_level + tide

## The tide: the sea rises and falls by TIDE_RISE over TIDE_SECS, timed by the clock so every client sees the same
## tide. It is only drawn; the server knows nothing of it.
const TIDE_SECS := 480.0
const TIDE_RISE := 0.45

static func tide_now() -> float:
	return TIDE_RISE * sin(TAU * fmod(Time.get_unix_time_from_system(), TIDE_SECS) / TIDE_SECS)

## Which stretches of coast are sand (1) and which shingle and rock (0), changing every few hundred units: the
## ground shader's coast_sand, the same hash in integers, so the props and the ground agree.
static func coast_sand(p: Vector2) -> float:
	var g := p / 260.0
	var i := g.floor()
	var f := g - i
	var u := f * f * (Vector2(3, 3) - 2.0 * f)
	var kx := int(i.x)
	var ky := int(i.y)
	var v := lerpf(lerpf(_coast_hash(kx, ky), _coast_hash(kx + 1, ky), u.x), lerpf(_coast_hash(kx, ky + 1), _coast_hash(kx + 1, ky + 1), u.x), u.y)
	return smoothstep(0.42, 0.58, v)

static func _coast_hash(x: int, y: int) -> float:
	var u := (x * 1664525 + y * 1013904223) & 0xffffffff
	u ^= u >> 16
	u = (u * 2146121005) & 0xffffffff
	u ^= u >> 15
	u = (u * 2221713035) & 0xffffffff
	u ^= u >> 16
	return float(u >> 8) / 16777215.0

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
	# The clouds' shadows: drifting the way the sky's clouds do, most marked under a broken sky, none at night,
	# fading out under full overcast (the light is already flat).
	_cloud_off += Vector2(1.0, 0.35) * (2.0 + 9.0 * float(_wx["wind"])) * dt
	var shade := (1.0 - darkness) * smoothstep(0.05, 0.3, cover) * (1.0 - smoothstep(0.75, 1.0, cover))
	RenderingServer.global_shader_parameter_set("world_clouds", Vector2(shade, lerpf(0.72, 0.36, cover)))
	RenderingServer.global_shader_parameter_set("world_cloud_off", _cloud_off)
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
	# The north's light is thinner and bluer, its haze grey.
	var chill: float = land.cold_at(_here.y)
	var warm := (_clear["sun_color"] as Color).lerp(Color("e6eefa"), 0.45 * chill).lerp(Color("ffb27a"), dusk * 0.7)
	_sun.light_color = warm if sun_dir.y > -0.05 else Color("9db4ff")
	var sun_energy: float = _clear["sun"] * (0.25 + 0.75 * day) if sun_dir.y > -0.05 else NIGHT_MOON
	_sun.light_energy = sun_energy * (1.0 - (0.55 + 0.2 * storm) * k)
	var e := _env
	e.tonemap_exposure = lerpf(NIGHT_EXPOSURE, 0.95, day) # the whole picture darker at night, the sky with it
	e.ambient_light_color = Color("1a2238").lerp((_clear["ambient_color"] as Color).lerp(Color("a8bad2"), 0.5 * chill), day)
	e.ambient_light_energy = _clear["ambient"] * (NIGHT_AMBIENT + (1.0 - NIGHT_AMBIENT) * day) * (1.0 - 0.35 * k) + _flash * 1.5
	var fog_day: Color = (_clear["fog_color"] as Color).lerp(Color("c3ccd6"), 0.6 * chill).lerp(Color("e0a07a"), dusk * 0.35)
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
	# Between the weathers, the air's own: dawn mist in the hollows, a haze that comes and goes, drifting wisps.
	var morning := fposmod(day_phase() - 0.25 + 0.5, 1.0) < 0.5 # before noon
	air.frame(e, dt, _here, height(_here.x, _here.y), sun_dir.y, morning, day, dusk, k, float(_wx["wind"]))

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
	var chimneys := []
	var pt := Time.get_ticks_usec()
	for o in world.in_cell(cx, cy):
		var t := String(o.get("t", ""))
		match t:
			"house":
				_chimney_at = Vector3.INF
				node.add_child(_build_house(o["b"], float(o.get("y", 0.0)), int(o.get("v", 0)), batch))
				node.add_child(_house_windows(o["b"], float(o.get("y", 0.0))))
				if _chimney_at != Vector3.INF:
					chimneys.append(_chimney_at + Vector3(0, float(o.get("y", 0.0)), 0))
				# Every third house hangs a lantern by its south wall.
				if absi(int(o.get("v", 0))) % 3 == 0:
					var b: Array = o["b"]
					var at := Vector3((float(b[0]) + float(b[2])) / 2.0, 0.0, float(b[3]) + 0.35)
					at.y = height(at.x, at.z) + 2.4
					node.add_child(_lantern(at))
					lights.append([at, "lantern"])
			"keep", "castle", "tower", "wall", "block":
				var ob: Array = o.get("b", [0, 0, 0, 0])
				var oc := Vector2((float(ob[0]) + float(ob[2])) / 2.0, (float(ob[1]) + float(ob[3])) / 2.0)
				if t in ["keep", "tower"] and not towns.in_city(oc):
					# A manor, a fort or a church tower: built in stone from the village kit, like the houses round it.
					node.add_child(_build_house(ob, float(o.get("y", 0.0)), int(o.get("v", 0)), batch, {}, true))
				else:
					node.add_child(_keep(o, batch))
				# Braziers burn at two corners of a keep or a castle, and of the city's towers.
				if t in ["keep", "castle"] or (t == "tower" and towns.in_city(oc)):
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
				_crag_diggings(b, int(o.get("v", 0)), batch)
			"lake":
				var b: Array = o["b"]
				node.add_child(_rim_rocks(b, float(o.get("y", 0.0)), 5.0, float(o.get("y", 0.0)) - 0.45))
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
				_square_dress(o, batch)
			"city":
				pass # open ground: the heights already level it
			"sea":
				pass # the sea is drawn once, as a surface (setup)
			"ford":
				_ford(o, cx, cy, node, batch, lights) # the road's own ground under the water, with stepping stones
			_:
				if o.get("solid", false):
					node.add_child(block(o)) # a type this theme does not know: draw it plainly
	pt = lap("cell objects", pt)
	_land_dress(cx, cy, node, batch, lights)
	pt = lap("cell land", pt)
	# Leaves and needles go grey-green towards the cold north.
	var chill := Color.WHITE.lerp(Color(0.62, 0.72, 0.72), land.cold_at((cy + 0.5) * world.CELL))
	for key in batch:
		var path := String(key).get_slice("#", 0)
		var fresh := not meshes.has(path)
		var m := _mesh_pal(path, int(String(key).get_slice("#", 1)) if String(key).contains("#") else 0)
		pt = lap("cell load" if fresh else "cell palette", pt)
		var f := path.get_file()
		var tint := _tint(path) * (chill if path.begins_with(FOREST) and (f.begins_with("Tree_") or f.begins_with("Bush_")) else Color.WHITE)
		if m:
			node.add_child(_multi(m, batch[key], tint))
		pt = lap("cell multi", pt)
	pt = lap("cell multimesh", pt)
	# The ground under the cell again, now its trees have streamed in (woodland floor).
	var gk: int = world.key(cx, cy)
	if _terrain.has(gk):
		(_terrain[gk] as MeshInstance3D).material_override = ground_material_at(cx, cy)
	pt = lap("cell ground map", pt)
	add_lights(world.key(cx, cy), lights)
	if life:
		if chimneys.is_empty():
			life.chimneys.erase(world.key(cx, cy))
		else:
			life.chimneys[world.key(cx, cy)] = chimneys
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

## A lantern (the RPG Tools one) on a short bracket, with a glow inside it.
func _lantern(at: Vector3) -> Node3D:
	var root := Node3D.new()
	root.position = at
	var model := _mesh(TOOLS + "lantern.gltf")
	if model:
		var mi := MeshInstance3D.new()
		mi.mesh = model
		mi.scale = Vector3.ONE * 0.75
		mi.position.y = -0.35
		root.add_child(mi)
	var n := MeshInstance3D.new()
	var bm := BoxMesh.new()
	bm.size = Vector3(0.18, 0.26, 0.18)
	n.mesh = bm
	root.add_child(n)
	var m := StandardMaterial3D.new()
	m.albedo_color = Color("ffd98c")
	m.emission_enabled = true
	m.emission = Color("ffc060")
	m.emission_energy_multiplier = 2.5
	n.material_override = m
	n.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	return root

# ------------------------------------------------------------------------------------- the land between (land.gd)

const WOOD_POST := Color(0.36, 0.26, 0.18)
const WOOD_BOARD := Color(0.55, 0.42, 0.28)

## What the land has between the things the world file names, in one cell: a lantern post and a signpost where
## country roads meet, stones along the verges, reeds by rivers and lakes, driftwood and shingle on the shore, and
## fences round the fields. None of it is solid; it is all placed from positions, so every client agrees.
func _land_dress(cx: int, cy: int, node: Node3D, batch: Dictionary, lights: Array) -> void:
	var cell := Rect2(cx * world.CELL, cy * world.CELL, world.CELL, world.CELL)
	var cold: float = land.cold_at(cell.get_center().y)
	var boxes := []
	for o in world.in_cell(cx, cy):
		if o.has("b"):
			var b: Array = o["b"]
			boxes.append(Rect2(float(b[0]), float(b[1]), float(b[2]) - float(b[0]), float(b[3]) - float(b[1])).grow(1.0))
	var free := func(p: Vector2) -> bool:
		for r in boxes:
			if (r as Rect2).has_point(p):
				return false
		return true
	var put := func(path: String, xf: Transform3D) -> void:
		if not batch.has(path):
			batch[path] = []
		batch[path].append(xf)
	# Where country roads end or meet: a lantern on a post in the widest gap between them, and a signpost with an arm
	# down each road.
	for j in land.junctions(cx, cy):
		var p: Vector2 = j[0]
		var dirs: Array = j[1]
		var angs := []
		for d in dirs:
			angs.append(atan2((d as Vector2).y, (d as Vector2).x))
		angs.sort()
		var best := 0.0
		var mid := 0.0
		for k in angs.size():
			var a0: float = angs[k]
			var a1: float = angs[(k + 1) % angs.size()] + (TAU if k == angs.size() - 1 else 0.0)
			if a1 - a0 > best:
				best = a1 - a0
				mid = (a0 + a1) / 2.0
		var corner := p + Vector2(cos(mid), sin(mid)) * 5.0
		if free.call(corner):
			var lamp := _lamp_post(Vector3(corner.x, height(corner.x, corner.y), corner.y), mid + PI)
			node.add_child(lamp[0])
			lights.append([lamp[1], "lantern"])
		var sign_at := p + Vector2(cos(mid + 0.5), sin(mid + 0.5)) * 6.5
		if free.call(sign_at):
			node.add_child(_signpost(Vector3(sign_at.x, height(sign_at.x, sign_at.y), sign_at.y), dirs))
	# Verge stones and the odd bush along country roads, out of the settlements.
	for sg in world.road_segments(cx, cy):
		var a := Vector2(sg[0], sg[1])
		var b := Vector2(sg[2], sg[3])
		var hw: float = sg[4]
		if hw >= 3.4 or a.distance_to(b) < 1.0:
			continue
		var dir := (b - a).normalized()
		var side := Vector2(-dir.y, dir.x)
		var rng := RandomNumberGenerator.new()
		rng.seed = hash(Vector2i(roundi(a.x * 4.0), roundi(a.y * 4.0)))
		var t := rng.randf() * 4.0
		while t < a.distance_to(b):
			for sd in [-1.0, 1.0]:
				var roll := rng.randf()
				var q: Vector2 = a + dir * t + side * sd * (hw + rng.randf_range(0.5, 1.4))
				if roll > 0.34 or not cell.has_point(q) or not free.call(q) or not towns.at(q).get("farmstead", false):
					continue
				if _on_other_road(q, sg, cx, cy) or (world.has_sea and height(q.x, q.y) < world.sea_level + 0.8):
					continue
				var pick: String = BUSHES[rng.randi() % BUSHES.size()] if roll < 0.04 * (1.0 - cold) else SMALL_ROCKS[rng.randi() % SMALL_ROCKS.size()]
				var sc := rng.randf_range(0.35, 0.6) if pick.begins_with("Bush") else rng.randf_range(0.4, 0.85)
				put.call(FOREST + pick + ".gltf", Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * sc), Vector3(q.x, height(q.x, q.y) - 0.12, q.y)))
			t += rng.randf_range(3.0, 5.5)
	_reeds(cell, cold, node)
	_shore(cell, cold, put)
	_fences(cx, cy, put)
	_rubble(cell, free, put)

## Now and then, out in the country, the tumbled stones of something long fallen: low enough to walk over, so it needs
## no footprint on the server.
func _rubble(cell: Rect2, free: Callable, put: Callable) -> void:
	var rng := RandomNumberGenerator.new()
	rng.seed = hash(Vector2i(roundi(cell.position.x), roundi(cell.position.y))) + 4242
	if rng.randf() > 0.12:
		return
	var c := cell.position + Vector2(rng.randf_range(16.0, cell.size.x - 16.0), rng.randf_range(16.0, cell.size.y - 16.0))
	var spot := Rect2(c, Vector2.ZERO).grow(7.0)
	if not towns.at(c).get("farmstead", false) or land.near_road(spot) or land.wet_at_rect(spot) or not land.fields_in(spot).is_empty():
		return
	if (world.has_sea and height(c.x, c.y) < world.sea_level + 1.5) or absf(height(c.x - 6.0, c.y) - height(c.x + 6.0, c.y)) > 2.5:
		return
	for k in rng.randi_range(4, 7):
		var a := rng.randf() * TAU
		var q := c + Vector2(cos(a), sin(a)) * rng.randf_range(0.0, 6.0)
		if not free.call(q):
			continue
		var path := _dress_path(["d:rubble_large", "d:rubble_half", "d:rubble_half"][rng.randi() % 3])
		var m := _mesh(path)
		if m == null:
			continue
		var sc := rng.randf_range(0.8, 1.2)
		put.call(path, Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * sc), Vector3(q.x, height(q.x, q.y) - m.get_aabb().position.y * sc - 0.1, q.y)))

## Whether a point is on a road other than segment `sg` (a junction's mouth, where verge stones would sit in the way).
func _on_other_road(q: Vector2, sg: Array, cx: int, cy: int) -> bool:
	for o in world.road_segments(cx, cy):
		if o == sg:
			continue
		if Geometry2D.get_closest_point_to_segment(q, Vector2(o[0], o[1]), Vector2(o[2], o[3])).distance_to(q) < float(o[4]) + 0.6:
			return true
	return false

## A lantern hung from the arm of a wooden post, the arm towards `face`. Returns [node, where the light is].
func _lamp_post(at: Vector3, face: float) -> Array:
	var root := Node3D.new()
	root.position = at
	var post := MeshInstance3D.new()
	var pm := BoxMesh.new()
	pm.size = Vector3(0.18, 3.0, 0.18)
	post.mesh = pm
	post.material_override = _plain_mat(WOOD_POST)
	post.position.y = 1.5
	root.add_child(post)
	var arm := MeshInstance3D.new()
	var am := BoxMesh.new()
	am.size = Vector3(1.0, 0.12, 0.12)
	arm.mesh = am
	arm.material_override = _plain_mat(WOOD_POST)
	var out := Vector3(cos(face), 0.0, sin(face))
	arm.position = Vector3(0, 2.9, 0) + out * 0.45
	arm.rotation.y = -face
	root.add_child(arm)
	var hang := Vector3(0, 2.55, 0) + out * 0.85
	var l := _lantern(Vector3.ZERO)
	l.position = hang
	root.add_child(l)
	return [root, at + hang]

## A signpost: a post with an arm pointing down each road from the junction (up to three).
func _signpost(at: Vector3, dirs: Array) -> Node3D:
	var root := Node3D.new()
	root.position = at
	var post := MeshInstance3D.new()
	var pm := BoxMesh.new()
	pm.size = Vector3(0.14, 2.3, 0.14)
	post.mesh = pm
	post.material_override = _plain_mat(WOOD_POST)
	post.position.y = 1.15
	root.add_child(post)
	var bm := BoxMesh.new()
	bm.size = Vector3(0.95, 0.2, 0.05)
	for k in mini(dirs.size(), 3):
		var d: Vector2 = dirs[k]
		var arm := MeshInstance3D.new()
		arm.mesh = bm
		arm.material_override = _plain_mat(WOOD_BOARD)
		var ang := atan2(d.y, d.x)
		arm.rotation.y = -ang
		arm.position = Vector3(d.x * 0.45, 2.05 - 0.3 * k, d.y * 0.45)
		root.add_child(arm)
	return root

## A ford: stepping stones across the water along the road, and a lantern post on each bank.
func _ford(o: Dictionary, cx: int, cy: int, node: Node3D, batch: Dictionary, lights: Array) -> void:
	var p := Vector2(o["p"][0], o["p"][1])
	var dir := Vector2(cos(float(o.get("r", 0.0))), sin(float(o.get("r", 0.0))))
	var best := INF
	for sg in world.road_segments(cx, cy):
		var a := Vector2(sg[0], sg[1])
		var b := Vector2(sg[2], sg[3])
		var d := Geometry2D.get_closest_point_to_segment(p, a, b).distance_to(p)
		if d < best and a.distance_to(b) > 0.5:
			best = d
			dir = (b - a).normalized()
	var level := -INF
	var reach := 6.0
	var near := INF
	for rv in world.rivers:
		for pt in rv["pts"]:
			var d := Vector2(pt[0], pt[1]).distance_to(p)
			if d < near:
				near = d
				level = float(pt[2])
				reach = float(rv.get("half_width", 5.0)) + 2.0
	if level == -INF:
		return
	var rng := RandomNumberGenerator.new()
	rng.seed = absi(int(o.get("v", 0)))
	var side := Vector2(-dir.y, dir.x)
	var t := -reach
	var k := 0
	while t <= reach:
		var q := p + dir * t + side * (0.7 if k % 2 == 0 else -0.7) + side * rng.randf_range(-0.2, 0.2)
		if height(q.x, q.y) < level - 0.05:
			var pick: String = FOREST + ["Rock_1_A_Color1", "Rock_1_J_Color1", "Rock_2_A_Color1"][rng.randi() % 3] + ".gltf"
			if not batch.has(pick):
				batch[pick] = []
			var sc := rng.randf_range(0.48, 0.62)
			batch[pick].append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3(sc, sc * 0.6, sc)), Vector3(q.x, level - 0.12, q.y)))
		t += 1.25
		k += 1
	for sd in [-1.0, 1.0]:
		var bank: Vector2 = p + dir * sd * (reach + 1.5) + side * 3.6
		var lamp := _lamp_post(Vector3(bank.x, height(bank.x, bank.y), bank.y), atan2(-side.y, -side.x))
		node.add_child(lamp[0])
		lights.append([lamp[1], "lantern"])

var _reed_mesh: ArrayMesh
var _reed_mats := {}
## A clump of reeds: seven tapered blades leaning out from the middle, 1 to 1.6 tall.
func _reed_clump() -> ArrayMesh:
	if _reed_mesh:
		return _reed_mesh
	var st := SurfaceTool.new()
	st.begin(Mesh.PRIMITIVE_TRIANGLES)
	var rng := RandomNumberGenerator.new()
	rng.seed = 41
	for i in 7:
		var a := TAU * i / 7.0 + rng.randf() * 0.6
		var lean := Vector3(cos(a), 0, sin(a)) * rng.randf_range(0.1, 0.35)
		var tall := rng.randf_range(1.0, 1.6)
		var base := Vector3(cos(a), 0, sin(a)) * rng.randf_range(0.0, 0.18)
		var w := Vector3(-sin(a), 0, cos(a)) * 0.05
		var top := base + lean + Vector3(0, tall, 0)
		for v in [base - w, base + w, top]:
			st.set_normal(Vector3.UP)
			st.add_vertex(v)
	_reed_mesh = st.commit()
	var m := StandardMaterial3D.new()
	m.albedo_color = Color(0.45, 0.52, 0.25)
	m.roughness = 1.0
	m.cull_mode = BaseMaterial3D.CULL_DISABLED
	_reed_mesh.surface_set_material(0, m)
	return _reed_mesh

## Reeds along the rivers' banks and round the lakes, thinning out in the north (and browner there).
func _reeds(cell: Rect2, cold: float, node: Node3D) -> void:
	var xf := []
	var add := func(q: Vector2, floor_y: float, rng: RandomNumberGenerator) -> void:
		if not cell.has_point(q) or land.near_road(Rect2(q, Vector2.ZERO).grow(1.5)):
			return
		var y := maxf(height(q.x, q.y), floor_y)
		var sc := rng.randf_range(1.1, 1.7)
		xf.append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3(sc, sc * rng.randf_range(0.8, 1.2), sc)), Vector3(q.x, y - 0.05, q.y)))
	var thin := 0.55 * (1.0 - 0.7 * cold)
	for sg in land.river_segments(int(floor(cell.position.x / world.CELL)), int(floor(cell.position.y / world.CELL))):
		var a: Vector2 = sg[0]
		var b: Vector2 = sg[1]
		var hw: float = sg[2]
		if not Rect2(a, Vector2.ZERO).expand(b).grow(hw + 3.0).intersects(cell) or a.distance_to(b) < 0.5:
			continue
		var dir := (b - a).normalized()
		var side := Vector2(-dir.y, dir.x)
		var rng := RandomNumberGenerator.new()
		rng.seed = hash(Vector2i(roundi(a.x * 4.0), roundi(a.y * 4.0)))
		var t := 0.0
		while t < a.distance_to(b):
			for sd in [-1.0, 1.0]:
				if rng.randf() < thin:
					add.call(a + dir * t + side * sd * (hw + rng.randf_range(-0.9, 0.6)), lerpf(float(sg[3]), float(sg[4]), t / a.distance_to(b)) - 0.7, rng)
			t += rng.randf_range(1.6, 3.0)
	for l in world.lakes:
		var lb: Array = l["b"]
		var lr := Rect2(float(lb[0]), float(lb[1]), float(lb[2]) - float(lb[0]), float(lb[3]) - float(lb[1]))
		if not lr.grow(3.0).intersects(cell):
			continue
		var rng := RandomNumberGenerator.new()
		rng.seed = hash(Vector2i(roundi(lr.position.x), roundi(lr.position.y)))
		var level := float(l.get("level", 0.0))
		var reach := minf(lr.size.x, lr.size.y) / 2.0
		var joined := _lakes_touching(lb)
		var corners := [lr.position, Vector2(lr.end.x, lr.position.y), lr.end, Vector2(lr.position.x, lr.end.y)]
		for i in 4:
			var a: Vector2 = corners[i]
			var b: Vector2 = corners[(i + 1) % 4]
			var inward := (lr.get_center() - (a + b) / 2.0).normalized()
			var t := 0.0
			while t < a.distance_to(b):
				if rng.randf() < thin:
					var e := a.lerp(b, t / a.distance_to(b))
					var off := rng.randf_range(-1.4, 0.6)
					# Only where the walk in from the edge passes through this cell can a reed land in it.
					if Rect2(e, Vector2.ZERO).expand(e + inward * reach).grow(1.5).intersects(cell) and not _in_any(e - inward * 2.0, joined):
						var w := _lake_edge(level, e, inward, reach)
						if w.is_finite():
							add.call(w + inward * off, level - 0.7, rng)
				t += rng.randf_range(1.6, 3.0)
	if xf.is_empty():
		return
	var mmi := _multi(_reed_clump(), xf)
	mmi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	var shade := roundi(cold * 4.0) # a few shades from green to the north's winter brown, each made once
	if shade > 0:
		if not _reed_mats.has(shade):
			var rm := (_reed_clump().surface_get_material(0) as StandardMaterial3D).duplicate() as StandardMaterial3D
			rm.albedo_color = Color(0.45, 0.52, 0.25).lerp(Color(0.58, 0.5, 0.34), shade / 4.0)
			_reed_mats[shade] = rm
		mmi.material_override = _reed_mats[shade]
	node.add_child(mmi)

## The sea's edge, by the kind of coast (coast_sand). A beach: driftwood, flat pebbles, rocks half buried in the
## sand, dune grass along its top, a rowing boat drawn up now and then, and once in a long while a wreck. A rocky
## shore: boulders standing in the water's edge, sea stacks out in the shallows, and shingle. More stone and less
## wood in the north. None of it is solid.
const DUNE_GRASS := ["grass_large.glb", "grass_leafsLarge.glb"]
const SEA_STACKS := ["rock_tallA.glb", "rock_tallB.glb", "rock_tallC.glb", "rock_tallG.glb"]
const SHORE_BOULDERS := ["rocks-a.glb", "rocks-b.glb", "rocks-c.glb"]
const SAND_ROCKS := ["rocks-sand-a.glb", "rocks-sand-b.glb", "rocks-sand-c.glb"]
const PEBBLES := ["stone_smallFlatA.glb", "stone_smallFlatB.glb", "stone_smallFlatC.glb", "ground_riverRocks.glb"]

func _shore(cell: Rect2, cold: float, put: Callable) -> void:
	if not world.has_sea:
		return
	# Most cells are nowhere near the sea: a coarse look first.
	var near_sea := false
	for i in 5:
		for j in 5:
			var p := cell.position + Vector2(i + 0.5, j + 0.5) * cell.size.x / 5.0
			if height(p.x, p.y) < world.sea_level + 4.0:
				near_sea = true
	if not near_sea:
		return
	var rng := RandomNumberGenerator.new()
	rng.seed = hash(Vector2i(roundi(cell.position.x), roundi(cell.position.y))) + 77
	var sl: float = world.sea_level
	var turn := func() -> Basis: return Basis(Vector3.UP, rng.randf() * TAU)
	var step := 5.0
	var y := cell.position.y + step * 0.5
	while y < cell.end.y:
		var x := cell.position.x + step * 0.5
		while x < cell.end.x:
			var q := Vector2(x + rng.randf_range(-2.0, 2.0), y + rng.randf_range(-2.0, 2.0))
			var hq := height(q.x, q.y)
			var roll := rng.randf()
			if hq < sl - 3.0 or hq > sl + 2.4: # nothing is placed outside these heights (the same draws either way)
				x += step
				continue
			var at := Vector3(q.x, hq, q.y)
			if coast_sand(q) > 0.5:
				if hq > sl + 1.3 and hq < sl + 2.4:
					if roll < 0.35 * (1.0 - 0.5 * cold):
						var sc := rng.randf_range(1.6, 2.6)
						put.call(COAST + DUNE_GRASS[rng.randi() % DUNE_GRASS.size()], Transform3D(turn.call().scaled(Vector3(sc, sc * rng.randf_range(0.9, 1.4), sc)), at - Vector3(0, 0.05, 0)))
				elif hq > sl - 0.2 and hq < sl + 1.3:
					if roll < 0.03 * (1.0 - 0.6 * cold):
						put.call(RES + ["Wood_Log_A", "Wood_Log_B"][rng.randi() % 2] + ".gltf",
							Transform3D(turn.call() * Basis(Vector3.FORWARD, rng.randf_range(-0.1, 0.1)).scaled(Vector3.ONE * rng.randf_range(1.0, 1.5)), at - Vector3(0, 0.05, 0)))
					elif roll < 0.037 and hq > sl + 0.3:
						put.call(COAST + ["boat-row-small.glb", "boat-row-large.glb"][rng.randi() % 2], Transform3D(turn.call() * Basis(Vector3.FORWARD, rng.randf_range(0.08, 0.2)).scaled(Vector3.ONE * 1.5), at + Vector3(0, 0.05, 0)))
					elif roll < 0.0375 and hq < sl + 0.1:
						put.call(COAST + "ship-wreck.glb", Transform3D(turn.call() * Basis(Vector3.FORWARD, rng.randf_range(0.35, 0.55)).scaled(Vector3.ONE * 1.3), at - Vector3(0, 2.8, 0)))
					elif roll < 0.07:
						var sc := rng.randf_range(0.3, 0.6)
						put.call(COAST + SAND_ROCKS[rng.randi() % SAND_ROCKS.size()], Transform3D(turn.call().scaled(Vector3(sc, sc * 0.7, sc)), at - Vector3(0, 0.35, 0)))
					elif roll < 0.11:
						put.call(COAST + PEBBLES[rng.randi() % PEBBLES.size()], Transform3D(turn.call().scaled(Vector3.ONE * rng.randf_range(1.2, 2.2)), at + Vector3(0, 0.01, 0)))
			else:
				if hq > sl - 3.0 and hq < sl - 0.5:
					if roll < 0.035:
						var sc := rng.randf_range(3.5, 7.5)
						put.call(COAST + SEA_STACKS[rng.randi() % SEA_STACKS.size()], Transform3D(turn.call().scaled(Vector3(sc, sc * rng.randf_range(0.9, 1.5), sc)), at - Vector3(0, 0.4, 0)))
				elif hq > sl - 0.9 and hq < sl + 0.3:
					if roll < 0.14:
						var sc := rng.randf_range(0.45, 1.1)
						put.call(COAST + SHORE_BOULDERS[rng.randi() % SHORE_BOULDERS.size()], Transform3D(turn.call().scaled(Vector3(sc, sc * rng.randf_range(0.7, 1.1), sc)), at - Vector3(0, 0.5, 0)))
				elif hq > sl + 0.3 and hq < sl + 1.0:
					if roll < 0.12 + 0.1 * cold:
						var sc := rng.randf_range(0.4, 1.0)
						put.call(FOREST + SMALL_ROCKS[rng.randi() % SMALL_ROCKS.size()] + ".gltf", Transform3D(turn.call().scaled(Vector3(sc, sc * 0.7, sc)), at - Vector3(0, 0.1, 0)))
					elif roll < 0.3:
						put.call(COAST + PEBBLES[rng.randi() % PEBBLES.size()], Transform3D(turn.call().scaled(Vector3.ONE * rng.randf_range(1.2, 2.2)), at + Vector3(0, 0.01, 0)))
			x += step
		y += step

## A wooden fence along a field's far side and its two ends (the side by the house is left open), skipping any length
## a road or water crosses.
func _fences(cx: int, cy: int, put: Callable) -> void:
	var piece := VILLAGE + "Prop_WoodenFence_Single.gltf"
	var m := _mesh(piece)
	if m == null:
		return
	var span := maxf(m.get_aabb().size.x, 0.5) * VS
	for f in land.fields_of_cell(cx, cy):
		var r: Rect2 = f[0]
		var side: int = f[2]
		if side < 0:
			continue # a world file's field: hedgerows bound it, not fences
		var c := [r.position, Vector2(r.end.x, r.position.y), r.end, Vector2(r.position.x, r.end.y)]
		var edges := {0: [c[0], c[1]], 1: [c[1], c[2]], 2: [c[2], c[3]], 3: [c[3], c[0]]} # N, E, S, W
		var open: int = [2, 0, 1, 3][side] # the edge facing the house: a north field opens south, and so on
		for e in edges:
			if e == open:
				continue
			var a: Vector2 = edges[e][0]
			var b: Vector2 = edges[e][1]
			var dir := (b - a).normalized()
			var n := int(a.distance_to(b) / span)
			for k in n:
				var q := a + dir * (span * (k + 0.5))
				if land.near_road(Rect2(q, Vector2.ZERO).grow(0.8)) or land.wet_at_rect(Rect2(q, Vector2.ZERO).grow(0.8)):
					continue
				put.call(piece, Transform3D(Basis(Vector3.UP, -atan2(dir.y, dir.x)).scaled(Vector3.ONE * VS), Vector3(q.x, height(q.x, q.y) - 0.05, q.y)))

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
	var cold: float = land.cold_at(y)
	match t:
		"tree":
			# Pines take over from the broadleaves going north.
			var pine := cold > 0.3 and float(v % 97) / 97.0 < cold
			path = FOREST + (PINES[v % PINES.size()] if pine else TREES[v % TREES.size()]) + ".gltf"
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
	# Undergrowth round trees out in the country: bushes, and stones in the north.
	if t == "tree" and towns.at(Vector2(x, y)).get("farmstead", false):
		var rng := RandomNumberGenerator.new()
		rng.seed = v * 31 + 5
		for k in v % 3:
			var a := rng.randf() * TAU
			var at := Vector2(x, y) + Vector2(cos(a), sin(a)) * rng.randf_range(1.6, 3.2) * s
			var stony := rng.randf() < 0.25 + 0.6 * cold
			var under: String = FOREST + (SMALL_ROCKS[rng.randi() % SMALL_ROCKS.size()] if stony else BUSHES[rng.randi() % BUSHES.size()]) + ".gltf"
			if not batch.has(under):
				batch[under] = []
			var us := rng.randf_range(0.5, 0.85) if stony else rng.randf_range(0.4, 0.75)
			batch[under].append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * us), Vector3(at.x, height(at.x, at.y) - 0.15, at.y)))
	# Now and then a felled log lies by a tree, and a woodcutter's axe in a stump.
	if t == "tree" and v % 23 == 0:
		var a := float(v % 628) / 100.0
		var at := Vector2(x, y) + Vector2(cos(a), sin(a)) * (2.2 + s)
		var dress := {}
		_dress(dress, ["Wood_Log_A", "Wood_Log_B"][v % 2], 1.4, Vector3(at.x, height(at.x, at.y), at.y), a + 1.2)
		if v % 3 == 0:
			_dress(dress, "t:axe", 1.3, Vector3(at.x + 1.2, height(at.x + 1.2, at.y), at.y), a)
		for pth in dress:
			_batch(batch, pth, dress[pth], 0.0)

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
	if life:
		life.snap()

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
	# A snowy region (the north's) is weather over open country, not a place apart: no wall of mist round it and no
	# fog volume over it; walking in, the snow and its haze are the weather's own.
	if weather_kind({"name": z.get("weather", "")}) == "snow":
		return Node3D.new()
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
	return await map_area(Rect2(0, 0, world.size, world.size), px)

## The map of one square of the world, `px` pixels on a side: the land shaded by its slopes, water, roads, what is
## built, and (close up, where they have streamed in) the fields in their crops and the trees. With `spread`, the
## ground is drawn a few rows a frame (await it), so a close-up made while playing does not stall a frame.
func map_area(area: Rect2, px: int, spread := false) -> Image:
	var img := Image.create(px, px, false, Image.FORMAT_RGB8)
	var step: float = area.size.x / px
	var o0 := area.position
	# Slopes over at least a heights cell, so the shading follows the hills rather than the grid they are kept on.
	var e := maxf(step, world.height_cell() if world.has_method("height_cell") else 2.0)
	for j in px:
		if spread and j % 8 == 7:
			await get_tree().process_frame
		for i in px:
			var x := o0.x + (i + 0.5) * step
			var y := o0.y + (j + 0.5) * step
			var slope := (height(x - e, y - e) - height(x + e, y + e)) / (2.0 * e)
			var lit := 0.9 * (1.0 + clampf(slope * 1.2, -0.25, 0.25))
			var hh := height(x, y)
			if world.has_sea and hh < world.sea_level + 0.1:
				img.set_pixel(i, j, Color("2f6f8f"))
			else:
				# High ground goes to heath and grey rock, so hills read on the map as well as by their shading.
				var up := clampf((hh - 14.0) / 22.0, 0.0, 1.0)
				var c := Color(0.36, 0.54, 0.2).lerp(Color(0.52, 0.5, 0.4), up)
				img.set_pixel(i, j, Color(c.r * lit, c.g * lit, c.b * lit))
	var px_of := func(x: float, y: float) -> Vector2i:
		return Vector2i(clampi(int((x - o0.x) / step), 0, px - 1), clampi(int((y - o0.y) / step), 0, px - 1))
	var near := area.grow(64.0)
	var seg_in := func(a: Vector2, b: Vector2) -> bool:
		return near.intersects(Rect2(a, Vector2.ZERO).expand(b))
	# Close up: fields in their crops, and trees as dark specks.
	if step < 16.0 and land != null:
		var crop := {"pasture": Color(0.42, 0.6, 0.26), "hay": Color(0.6, 0.64, 0.33), "wheat": Color(0.8, 0.68, 0.36),
			"barley": Color(0.74, 0.69, 0.42), "oats": Color(0.66, 0.66, 0.42), "potato": Color(0.48, 0.38, 0.26)}
		for f in land.fields_in(area):
			if f.size() < 5:
				continue
			var q: PackedVector2Array = f[3]
			var c: Color = crop.get(f[4], Color(0.5, 0.6, 0.3))
			var box: Rect2 = f[0]
			var p0: Vector2i = px_of.call(box.position.x, box.position.y)
			var p1: Vector2i = px_of.call(box.end.x, box.end.y)
			for jj in range(p0.y, p1.y + 1):
				for ii in range(p0.x, p1.x + 1):
					if Geometry2D.is_point_in_polygon(Vector2(o0.x + (ii + 0.5) * step, o0.y + (jj + 0.5) * step), q):
						img.set_pixel(ii, jj, c)
		var cx0 := int(floor(area.position.x / world.CELL))
		var cy0 := int(floor(area.position.y / world.CELL))
		for cx in range(cx0, int(floor(area.end.x / world.CELL)) + 1):
			for cy in range(cy0, int(floor(area.end.y / world.CELL)) + 1):
				for o in world.in_cell(cx, cy):
					var t := String(o.get("t", ""))
					if t == "tree" or t == "tree_bare":
						var p: Vector2i = px_of.call(o["p"][0], o["p"][1])
						img.set_pixel(p.x, p.y, Color(0.16, 0.3, 0.13))
	for r in world.rivers:
		var pts: Array = r["pts"]
		for k in pts.size() - 1:
			if not seg_in.call(Vector2(pts[k][0], pts[k][1]), Vector2(pts[k + 1][0], pts[k + 1][1])):
				continue
			var wide := maxi(3, int(2.0 * float(r.get("half_width", 5.0)) / step))
			var segs := maxi(6, int(Vector2(pts[k][0], pts[k][1]).distance_to(Vector2(pts[k + 1][0], pts[k + 1][1])) / step))
			for t in segs + 1:
				var p: Vector2i = px_of.call(lerpf(pts[k][0], pts[k + 1][0], float(t) / segs), lerpf(pts[k][1], pts[k + 1][1], float(t) / segs))
				img.fill_rect(Rect2i(p - Vector2i(wide / 2, wide / 2), Vector2i(wide, wide)), Color("2f7f8f"))
	for r in world.roads:
		var pts: Array = r["pts"]
		var wide := maxi(1, int(2.0 * float(r.get("half_width", 3.0)) / step))
		for k in pts.size() - 1:
			if not seg_in.call(Vector2(pts[k][0], pts[k][1]), Vector2(pts[k + 1][0], pts[k + 1][1])):
				continue
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
			if not near.intersects(Rect2(b[0], b[1], b[2] - b[0], b[3] - b[1])):
				continue
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
		["wildlife, smoke and fireflies are pooled round us", life != null and life._deer.size() > 0 and life._smoke.size() > 0, str(life.counts()) if life else "none"],
		["the ground knows how wet and snowy it is", Engine.has_meta("life_globals"), "wet %.2f snow %.2f" % [life.wet, life.snow] if life else ""],
	]

# ---------------------------------------------------------------- builders and pieces

const CHARS := "res://themes/fantasy/assets/characters/"

const DUNGEON := "res://themes/fantasy/assets/dungeon/"

const VILLAGE := "res://themes/fantasy/assets/village/"

const RES := "res://themes/fantasy/assets/resources/" # KayKit Resource Bits: logs, planks, stone, ore, cloth
const TOOLS := "res://themes/fantasy/assets/tools/"    # KayKit RPG Tools Bits: anvil, grindstone, axes, rope, lanterns
const COAST := "res://themes/fantasy/assets/coast/"    # Kenney Pirate Kit and Nature Kit (CC0): boats, a wreck, shore rocks, sea stacks, dune grass

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

## The pines among TREES, which take over going north.
const PINES := ["Tree_4_A_Color1", "Tree_4_B_Color1", "Tree_4_C_Color1"]

const BARE_TREES := ["Tree_Bare_1_A_Color1", "Tree_Bare_1_B_Color1", "Tree_Bare_1_C_Color1", "Tree_Bare_2_A_Color1", "Tree_Bare_2_B_Color1", "Tree_Bare_2_C_Color1"]

const BUSHES := ["Bush_1_A_Color1", "Bush_1_B_Color1", "Bush_1_C_Color1", "Bush_1_D_Color1", "Bush_1_E_Color1", "Bush_1_F_Color1", "Bush_1_G_Color1", "Bush_2_A_Color1", "Bush_2_B_Color1", "Bush_2_C_Color1", "Bush_2_D_Color1", "Bush_2_E_Color1", "Bush_2_F_Color1", "Bush_3_A_Color1", "Bush_3_B_Color1", "Bush_3_C_Color1", "Bush_4_A_Color1", "Bush_4_B_Color1", "Bush_4_C_Color1", "Bush_4_D_Color1", "Bush_4_E_Color1", "Bush_4_F_Color1"]

const SMALL_ROCKS := ["Rock_1_A_Color1", "Rock_1_B_Color1", "Rock_1_C_Color1", "Rock_1_D_Color1", "Rock_1_E_Color1", "Rock_1_F_Color1", "Rock_1_G_Color1", "Rock_1_H_Color1", "Rock_1_I_Color1", "Rock_1_J_Color1", "Rock_2_A_Color1", "Rock_2_B_Color1", "Rock_2_C_Color1", "Rock_2_D_Color1", "Rock_3_A_Color1", "Rock_3_B_Color1", "Rock_3_C_Color1", "Rock_3_D_Color1", "Rock_3_E_Color1", "Rock_3_F_Color1", "Rock_3_G_Color1", "Rock_3_H_Color1", "Rock_3_I_Color1", "Rock_3_J_Color1", "Rock_3_K_Color1", "Rock_3_L_Color1"]

## The settlement at `p` and its character (towns.gd), for tools and tests.
func settlement_at(p: Vector2) -> Dictionary:
	return towns.at(p)

## A piece with its kit materials tinted to a settlement's palette (towns.gd PALETTES), made once per palette.
func _mesh_pal(path: String, pal: int) -> Mesh:
	var m := _mesh(path)
	if pal == 0 or not m is ArrayMesh:
		return m
	var key := "%s#%d" % [path, pal]
	if _pal_meshes.has(key):
		return _pal_meshes[key]
	var tints: Dictionary = TownsScript.PALETTES[pal]
	var am := (m as ArrayMesh).duplicate() as ArrayMesh
	for i in am.get_surface_count():
		var src := am.surface_get_material(i)
		if src is BaseMaterial3D and tints.has(src.resource_name):
			var mk := "%s#%d" % [src.resource_name, pal]
			if not _pal_mats.has(mk):
				var t := (src as BaseMaterial3D).duplicate() as BaseMaterial3D
				var tint = tints[src.resource_name]
				if tint is Array:
					t.albedo_texture = _washed(t.albedo_texture, float(tint[1]))
					tint = tint[0]
				t.albedo_color = (src as BaseMaterial3D).albedo_color * (tint as Color)
				_pal_mats[mk] = t
			am.surface_set_material(i, _pal_mats[mk])
	_pal_meshes[key] = am
	return am

## A texture with its colour washed out to `sat` (0 grey .. 1 as it was), made once.
var _washed_tex := {}
func _washed(tex: Texture2D, sat: float) -> Texture2D:
	if tex == null or sat >= 1.0:
		return tex
	var k := "%d#%.2f" % [tex.get_rid().get_id(), sat]
	if not _washed_tex.has(k):
		var img := tex.get_image()
		if img.is_compressed():
			img.decompress()
		img.adjust_bcs(1.0, 1.0, sat)
		img.generate_mipmaps()
		_washed_tex[k] = ImageTexture.create_from_image(img)
	return _washed_tex[k]

var _warming: Array = [] # [piece, palette] copies still to make

## Make palette copies of loaded pieces for up to `budget_ms`, so a village's first cell finds them made. The
## palettes of the settlements nearest us go first.
func _warm(center: Vector2, budget_ms: float) -> void:
	if _warming.is_empty():
		return
	if _warm_sorted_at.distance_to(center) > 200.0:
		_warm_sorted_at = center
		var near := {}
		for t in towns.list:
			near[t["pal"]] = minf(near.get(t["pal"], INF), (t["c"] as Vector2).distance_to(center))
		_warming.sort_custom(func(a, b): return near.get(a[1], INF) < near.get(b[1], INF))
	var t0 := Time.get_ticks_usec()
	var i := 0
	while i < _warming.size() and (Time.get_ticks_usec() - t0) < budget_ms * 1000.0:
		var w: Array = _warming[i]
		if meshes.has(w[0]) or ResourceLoader.load_threaded_get_status(w[0]) == ResourceLoader.THREAD_LOAD_LOADED:
			_mesh_pal(w[0], w[1])
			_warming.remove_at(i)
		else:
			i += 1
			if i > 64:
				break # the rest are still loading
var _warm_sorted_at := Vector2.INF

## Start loading every piece the theme may draw on the loader's threads, so the first village that needs one does
## not wait for it to load (the pieces come in during the loading screen).
func _prefetch() -> void:
	for dir in [VILLAGE, RES, TOOLS, DUNGEON, FOREST, COAST]:
		for f in DirAccess.get_files_at(dir):
			var name := f.trim_suffix(".import").trim_suffix(".remap") # an exported game lists the import files
			if (name.ends_with(".gltf") or name.ends_with(".glb")) and not _fetching.has(dir + name):
				_fetching[dir + name] = true
				ResourceLoader.load_threaded_request(dir + name)

var _fetching := {}

## The first mesh in a glTF piece, kept so each piece is loaded once and drawn with MultiMesh.
func _mesh(path: String) -> Mesh:
	if meshes.has(path):
		return meshes[path]
	var m: Mesh = null
	if ResourceLoader.exists(path):
		var res: Resource = ResourceLoader.load_threaded_get(path) if _fetching.has(path) else load(path)
		var inst := (res as PackedScene).instantiate()
		var found := inst.find_children("*", "MeshInstance3D", true, false)
		if found.size() == 1:
			m = (found[0] as MeshInstance3D).mesh
		elif found.size() > 1:
			# A piece in parts (a lantern's frame and glass, a grindstone's wheel and stand): one mesh, each part's
			# surfaces moved to where it sits in the piece.
			var am := ArrayMesh.new()
			for mi: MeshInstance3D in found:
				var xf := mi.transform
				var up := mi.get_parent()
				while up != null and up != inst and up is Node3D:
					xf = (up as Node3D).transform * xf
					up = up.get_parent()
				for i in mi.mesh.get_surface_count():
					var st := SurfaceTool.new()
					st.append_from(mi.mesh, i, xf)
					st.set_material(mi.mesh.surface_get_material(i))
					st.commit(am)
			m = am
		inst.free()
	if m is ArrayMesh and path.begins_with(COAST):
		_coast_colours(m as ArrayMesh)
	meshes[path] = m
	return m

## The Nature Kit is pastel (teal grass, peach rock): its materials, by name, in the colours of the rest of the land.
const COAST_COLOURS := {"stone": Color(0.56, 0.55, 0.53), "dirt": Color(0.47, 0.44, 0.40), "dirtDark": Color(0.36, 0.33, 0.29),
	"grass": Color(0.42, 0.52, 0.27), "water": Color(0.30, 0.45, 0.50)}

func _coast_colours(m: ArrayMesh) -> void:
	for i in m.get_surface_count():
		var mat := m.surface_get_material(i) as StandardMaterial3D
		if mat and COAST_COLOURS.has(mat.resource_name):
			var c := mat.duplicate() as StandardMaterial3D
			c.albedo_color = COAST_COLOURS[mat.resource_name]
			c.roughness = 0.95
			m.surface_set_material(i, c)

func _multi(mesh: Mesh, xforms: Array, tint := Color.WHITE) -> MultiMeshInstance3D:
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = mesh
	mm.instance_count = xforms.size()
	for i in xforms.size():
		mm.set_instance_transform(i, xforms[i])
	var mmi := MultiMeshInstance3D.new()
	mmi.multimesh = mm
	_snowable(mmi)
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
## `town` is the settlement's character (towns.gd): its palette tints every piece, its style sets the odds of each
## choice below, so a village is of a piece while its houses still differ. `grand` builds in stone throughout (a
## manor, a fort, a church tower).
func _build_house(b: Array, base: float, v: int, batch: Dictionary, town: Dictionary = {}, grand := false) -> Node3D:
	if town.is_empty():
		town = towns.at(Vector2((float(b[0]) + float(b[2])) / 2.0, (float(b[1]) + float(b[3])) / 2.0))
	var style: Dictionary = town["style"]
	var pal: int = town["pal"]
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
	var fl: Array = style["floors"]
	var floors := 3 if tower else (2 if grand else int(fl[0]) + hsh % (int(fl[1]) - int(fl[0]) + 1))
	var door_side := rng.randi() % 4
	# The settlement's style sets the odds; the house's own number makes the call, so neighbours differ: a plaster
	# or stone ground floor, a round or flat door in the settlement's kind of frame, and on two-storey houses
	# sometimes a jettied upper floor that overhangs the street.
	var roll := func(salt: int) -> float: return float((hsh / salt) % 100) / 100.0
	var stone: bool = grand or roll.call(7) < float(style["stone"])
	var round_door: bool = roll.call(3) < float(style["round_door"])
	var timber_frame: bool = style["timber_frame"]
	var jetty: bool = not grand and floors == 2 and roll.call(11) < float(style["jetty"])
	var shape := "Round" if round_door else "Flat"
	var door_wall: String = ("Wall_UnevenBrick_Door_%s" % shape) if stone else (["Wall_Plaster_Door_Round", "Wall_Plaster_Door_RoundInset"][(hsh / 13) % 2] if round_door else "Wall_Plaster_Door_Flat")
	var frame := "DoorFrame_%s_%s" % [shape, "WoodDark" if timber_frame else "Brick"]
	var door := "Door_%d_%s" % [[1, 2, 4, 8][hsh % 4], shape]
	# Plain upper walls: timber framed as often as the settlement likes it; shutters mostly open, or mostly shut.
	var plaster := ["Wall_Plaster_Straight", "Wall_Plaster_Straight_L", "Wall_Plaster_Straight_R", "Wall_Plaster_Straight_Base"]
	var woodgrid := float(style["woodgrid"])
	var plain_wall := func(n: int) -> String:
		if grand:
			return "Wall_UnevenBrick_Straight"
		return "Wall_Plaster_WoodGrid" if float(n % 10) / 10.0 < woodgrid else plaster[n % plaster.size()]
	var shutters := ["WindowShutters_Wide_Round_Open", "WindowShutters_Wide_Flat_Open"] if style["shutters_open"] else ["WindowShutters_Wide_Round_Closed", "WindowShutters_Wide_Flat_Closed", "WindowShutters_Wide_Round_Open"]
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
						put.call("Wall_UnevenBrick_Straight" if stone else plain_wall.call(i + si), xf)
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
					if grand:
						put.call(["Wall_UnevenBrick_Window_Wide_Round", "Wall_UnevenBrick_Window_Wide_Flat", "Wall_UnevenBrick_Window_Wide_Flat", "Wall_UnevenBrick_Window_Thin_Round"][kind], xf)
					else:
						put.call(["Wall_Plaster_Window_Wide_Round", "Wall_Plaster_Window_Wide_Flat", "Wall_Plaster_Window_Wide_Flat2", "Wall_Plaster_Window_Thin_Round"][kind], xf)
					var win: String = ["Window_Wide_Round1", "Window_Wide_Flat1", "Window_Wide_Flat1", "Window_Thin_Round1"][kind]
					put.call(win, Transform3D(plain, at + lift))
					if kind < 3:
						put.call(shutters[(i + f + hsh) % shutters.size()], Transform3D(plain, at + lift))
					else:
						put.call(["Window_Thin_Flat1", "WindowShutters_Thin_Round_Open", "WindowShutters_Thin_Round_Closed", "WindowShutters_Thin_Flat_Open", "WindowShutters_Thin_Flat_Closed"][(i + hsh) % 5], Transform3D(plain, at + lift))
				else:
					put.call(plain_wall.call(i * 7 + f + si + hsh), xf)
				# Vines hang from the top of some walls; a balcony on the odd upper one.
				if f == floors - 1 and float((i * 5 + si + hsh) % 20) / 20.0 < float(style["vines"]):
					put.call(["Prop_Vine1", "Prop_Vine2", "Prop_Vine4", "Prop_Vine5", "Prop_Vine6", "Prop_Vine9"][(i + si) % 6], Transform3D(plain, at + lift + Vector3(0, tall, 0) + out * 0.2))
				if f == 1 and style["balconies"] and not grand and (i + si * 2 + hsh) % 9 == 0:
					# The balcony piece is only its railing, a unit out from the wall: a floor of boards from the wall
					# to the rail (half a floor tile, turned to run outwards), and two short brackets under it.
					put.call(["Balcony_Simple_Straight", "Balcony_Cross_Straight"][(i + hsh) % 2], Transform3D(plain, at + lift))
					put.call("Floor_WoodDark_Half1", Transform3D(plain * Basis(Vector3.UP, PI / 2.0), at + lift + Vector3(0, 0.03, 0)))
					for bx in [-0.75, 0.75]:
						put.call("Roof_Support2", Transform3D(plain, at + lift + plain * Vector3(bx, 0, 0)))
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
			var post := "Corner_ExteriorWide_Brick" if (f == 0 or grand) and w > 16.0 else ("Corner_Exterior_Brick" if f == 0 or grand else ("Corner_ExteriorWide_Wood" if w > 16.0 else "Corner_Exterior_Wood"))
			if f == floors - 1 and f > 0 and (hsh / 19) % 2 == 0:
				post = "Corner_Exterior_TopDown"
			put.call(post, Transform3D(Basis.from_scale(Vector3.ONE * VS), c + Vector3(0, tall * f, 0)))
	put.call("Corner_Exterior_TopOnly", Transform3D(Basis.from_scale(Vector3.ONE * VS), Vector3(x0, tall * floors, y0)))
	_house_roof(put, b, tall * floors, tower, hsh, float(style["pitch"]), int(style["chimney"]))
	var dress := {}
	_house_yard(put, rng, door_at, door_out, hsh, town, dress, base)
	_house_porch(put, door_at, door_out, hsh)
	if floors == 2 and not jetty and not grand:
		_house_stairs(put, sides[(door_side + 1) % 4], tall, hsh)
	if not grand:
		_woodpile(sides[(door_side + 2) % 4], hsh, town, dress)
	var suffix := "#%d" % pal if pal > 0 else ""
	for name in parts:
		_batch(batch, VILLAGE + String(name) + ".gltf" + suffix, parts[name], node.position.y)
	for path in dress:
		_batch(batch, path, dress[path], node.position.y)
	return node

## A tiled roof chosen for the footprint's shape and stretched to fit it, gable ends, a chimney and dormers.
func _house_roof(put: Callable, b: Array, top: float, tower: bool, hsh: int, pitch := 1.0, chimney_kind := -1) -> void:
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
	# A hall too wide or long for one roof gets several side by side (a double pile) or end to end, so the tiles
	# stay the size they are on the houses; the chimney goes on the first.
	if span > 13.0 or length > 20.0:
		var across := maxi(1, int(ceil(span / 11.0)))
		var along := maxi(1, int(ceil(length / 16.0)))
		var n := 0
		for i in across:
			for j in along:
				var u0 := float(i) / across
				var u1 := float(i + 1) / across
				var v0 := float(j) / along
				var v1 := float(j + 1) / along
				var sub: Array = [lerpf(b[0], b[2], v0), lerpf(b[1], b[3], u0), lerpf(b[0], b[2], v1), lerpf(b[1], b[3], u1)] if along_x else [lerpf(b[0], b[2], u0), lerpf(b[1], b[3], v0), lerpf(b[0], b[2], u1), lerpf(b[1], b[3], v1)]
				_house_roof(put, sub, top, false, hsh + n * 31, pitch, chimney_kind if n == 0 else -2)
				n += 1
		return
	var best: Array = ROOFS[0]
	for r in ROOFS:
		if absf(float(r[1]) / float(r[0]) - length / span) < absf(float(best[1]) / float(best[0]) - length / span):
			best = r
	var name := "Roof_RoundTiles_%dx%d" % [best[0], best[1]]
	# The kit roof overhangs about one unit each way; stretch it to the box plus that overhang.
	var sx := (span + 1.0) / (float(best[0]) + 2.0)
	var sz := (length + 1.0) / (float(best[1]) + 2.0)
	var turn := Basis(Vector3.UP, PI / 2.0) if along_x else Basis()
	var ry := minf(sx, VS * 1.2) * pitch # the settlement's roofs are steeper or flatter
	put.call(name, Transform3D(turn * Basis.from_scale(Vector3(sx, ry, sz)), centre))
	# Gable ends, the brick triangle matching the roof's width, at both ends.
	var gable := "Roof_Front_Brick%d" % clampi(best[0], 4, 8)
	if best[0] == 4 or best[0] == 6 or best[0] == 8:
		for end in [-1.0, 1.0]:
			var face: Basis = turn * Basis(Vector3.UP, 0.0 if end > 0 else PI)
			var at: Vector3 = centre + turn * Vector3(0, 0, end * length / 2.0)
			put.call(gable, Transform3D(face * Basis.from_scale(Vector3(sx, ry, VS)), at))
	# A beam under each gable, and a ridge log along the top of the bigger roofs.
	for end in [-1.0, 1.0]:
		put.call("Roof_FrontSupports", Transform3D(turn * Basis(Vector3.UP, 0.0 if end > 0 else PI) * Basis.from_scale(Vector3(sx, VS, VS)), centre + turn * Vector3(0, 0, end * length / 2.0)))
	if best[0] == 8:
		put.call("Roof_Log", Transform3D(turn * Basis.from_scale(Vector3(sx, ry, (length + 1.0) / 10.7)), centre))
	# Ridge caps every couple of units along the ridge.
	var ridge_y := 6.0 * ry * (float(best[0]) / 8.0) * 0.95
	for k in int(length / (2.4 * VS)):
		put.call("Roof_Modular_RoundTiles", Transform3D(turn * Basis.from_scale(Vector3.ONE * VS), centre + turn * Vector3(0, ridge_y, -length / 2.0 + 1.2 * VS + k * 2.4 * VS)))
	# A chimney through the ridge and dormers on long roofs.
	if chimney_kind == -2:
		return # one chimney to a hall: the first of its roofs has it
	var chimney: String = ["Prop_Chimney", "Prop_Chimney2"][hsh % 2 if chimney_kind < 0 else chimney_kind]
	var chimney_at := centre + turn * Vector3(span * 0.15, 0, length * 0.25)
	put.call(chimney, Transform3D(Basis.from_scale(Vector3.ONE * VS), chimney_at))
	var cm := _mesh(VILLAGE + chimney + ".gltf")
	_chimney_at = chimney_at + Vector3(0, (cm.get_aabb().end.y if cm else 3.0) * VS + 0.2, 0)
	if length >= 12.0:
		for k in [-0.2, 0.2]:
			var at: Vector3 = centre + turn * Vector3(span * 0.25, 0.4, length * k)
			put.call("Roof_Dormer_RoundTile", Transform3D(turn * Basis.from_scale(Vector3.ONE * VS), at))
			put.call("Window_Roof_Wide" if k < 0 else "Window_Roof_Thin", Transform3D(turn * Basis(Vector3.UP, PI / 2.0) * Basis.from_scale(Vector3.ONE * VS), at + turn * Vector3(1.9 * VS, 0, 0)))

## A small tiled roof on brackets over the door, its own little gable to the front.
func _house_porch(put: Callable, door: Vector3, out: Vector3, hsh: int) -> void:
	if out == Vector3.ZERO:
		return
	# A small wooden canopy over the door on two wall brackets: Roof_Wooden_2x1 is high along its origin edge (put
	# against the wall) and falls away outwards; Roof_Support2 runs out from the wall below it.
	var across := Vector3.UP.cross(out).normalized()
	var face := Basis(across, Vector3.UP, out)
	var k := 0.85 * VS
	var at := door + out * 0.05 + Vector3(0, (2.8 + 0.08 * float(hsh % 3)) * VS, 0) # above the frame (2.59) and below the floor above (3.12)
	put.call("Roof_Wooden_2x1", Transform3D(face * Basis.from_scale(Vector3(k, k, k)), at - Vector3(0, 1.0 * k, 0)))
	for sx in [-0.9, 0.9]: # just outside the frame (0.8 each side)
		put.call("Roof_Support2", Transform3D(face * Basis.from_scale(Vector3.ONE * VS), at + across * (sx * VS) - Vector3(0, 0.12 * VS, 0)))

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

## A square's dressing, by its settlement's trade (towns.gd TRADES): logs to sit on round the fire, stalls with
## goods on their tables in a ring, and the trade's piles at the corners. Out of the server's way: a square is open
## ground and these are cosmetic.
func _square_dress(o: Dictionary, batch: Dictionary) -> void:
	var b: Array = o["b"]
	var c := Vector2((float(b[0]) + float(b[2])) / 2.0, (float(b[1]) + float(b[3])) / 2.0)
	var w := float(b[2]) - float(b[0])
	var d := float(b[3]) - float(b[1])
	var town := towns.at(c)
	var trade: Dictionary = TownsScript.TRADES[town["trade"]]
	var rng := RandomNumberGenerator.new()
	rng.seed = absi(int(o.get("v", 0)))
	var dress := {}
	var on_ground := func(p: Vector2) -> Vector3: return Vector3(p.x, height(p.x, p.y), p.y)
	for k in 3:
		var a := TAU * k / 3.0 + rng.randf() * 0.5
		_dress(dress, "Wood_Log_A", 1.6, on_ground.call(c + Vector2(cos(a), sin(a)) * 3.2), -a)
	var stalls := mini(int(trade["stalls"]), int(minf(w, d) / 5.0))
	var ring := minf(w, d) * 0.36
	var goods: Array = trade["stall"]
	var spin := rng.randf() * TAU
	for k in stalls:
		var a := spin + TAU * k / maxf(stalls, 1)
		var p := c + Vector2(cos(a), sin(a)) * ring
		var face := -a + PI / 2.0 # the table's long side to the fire
		var base: Vector3 = on_ground.call(p)
		_dress(dress, "d:table_medium_tablecloth" if k % 2 == 0 else "d:table_long_tablecloth", 1.2, base, face)
		for g in 2:
			var along := Vector2(cos(a + PI / 2.0), sin(a + PI / 2.0)) * (0.5 if g == 0 else -0.5)
			_dress(dress, String(goods[(k * 2 + g) % goods.size()]), 0.8, base + Vector3(along.x, 1.2, along.y), face + g)
		# A candle or a couple of bottles at the end of the table.
		var end := Vector2(cos(a + PI / 2.0), sin(a + PI / 2.0)) * 1.1
		if k % 2 == 0:
			_dress(dress, "d:candle_triple", 0.9, base + Vector3(end.x, 1.2, end.y), face)
		else:
			_dress(dress, "d:bottle_A_brown", 0.9, base + Vector3(end.x, 1.2, end.y), face)
			_dress(dress, "d:bottle_B_green", 0.9, base + Vector3(end.x * 0.8 + 0.15, 1.2, end.y * 0.8), face + 1.0)
		var behind := c + Vector2(cos(a), sin(a)) * (ring + 2.0)
		_dress(dress, ["d:crates_stacked", "d:barrel_small", "d:box_small", "d:keg"][k % 4], 1.0, on_ground.call(behind), rng.randf() * TAU)
		_dress(dress, "d:stool", 1.1, on_ground.call(c + Vector2(cos(a), sin(a)) * (ring - 1.6)), face)
	var piles: Array = trade["square"]
	var corners := [Vector2(-1, -1), Vector2(1, -1), Vector2(1, 1), Vector2(-1, 1)]
	for k in 4:
		var g: Array = piles[k % piles.size()]
		var p: Vector2 = c + (corners[k] as Vector2) * Vector2(w / 2.0 - 2.5, d / 2.0 - 2.5)
		_dress(dress, String(g[0]), float(g[1]), on_ground.call(p), rng.randf() * TAU)
	for pth in dress:
		_batch(batch, pth, dress[pth], 0.0)

## Diggings at the foot of a crag, on some: broken stone, ore, a pick or a shovel left by the rock.
func _crag_diggings(b: Array, v: int, batch: Dictionary) -> void:
	if absi(v) % 3 != 0:
		return
	var rng := RandomNumberGenerator.new()
	rng.seed = absi(v)
	var dress := {}
	var picks := ["Stone_Chunks_Small", "Iron_Nuggets", "Copper_Nuggets", "Stone_Chunks_Large", "t:pickaxe", "t:shovel", "t:bucket_metal", "Silver_Nuggets"]
	var side := rng.randi() % 4
	for k in 4:
		var f := rng.randf_range(0.2, 0.8)
		var p := [Vector2(lerpf(b[0], b[2], f), float(b[1]) - 3.0), Vector2(lerpf(b[0], b[2], f), float(b[3]) + 3.0),
			Vector2(float(b[0]) - 3.0, lerpf(b[1], b[3], f)), Vector2(float(b[2]) + 3.0, lerpf(b[1], b[3], f))][side] as Vector2
		_dress(dress, String(picks[rng.randi() % picks.size()]), 1.3, Vector3(p.x, height(p.x, p.y), p.y), rng.randf() * TAU)
	for pth in dress:
		_batch(batch, pth, dress[pth], 0.0)

## The yard in front of the door: a paved path, the settlement's kind of fence each side, an arch, a lean-to shed
## or a stall roof, and what its trade leaves lying about (towns.gd TRADES), into `dress` by file. All outside the
## box, so you walk through them (the server only knows the house).
func _house_yard(put: Callable, rng: RandomNumberGenerator, door: Vector3, out: Vector3, hsh: int, town: Dictionary, dress: Dictionary, base: float) -> void:
	if out == Vector3.ZERO:
		return
	var style: Dictionary = town["style"]
	var rot := Basis(Vector3.UP, atan2(out.x, out.z))
	var side := rot * Vector3(1, 0, 0)
	var tile := Basis.from_scale(Vector3.ONE * VS)
	# Everything out here stands on the ground where it is, not at the house's floor (the house sits on the lowest
	# corner of its plot; the yard runs on up or down the slope).
	var gy := func(q: Vector3) -> Vector3: return Vector3(q.x, height(q.x, q.z) - base, q.z)
	# The path: the settlement's paving, each slab laid along the slope, a touch above the ground.
	for k in 3:
		var at: Vector3 = door + out * (VS * (1.0 + 2.0 * k))
		var dx := height(at.x + 1.0, at.z) - height(at.x - 1.0, at.z)
		var dz := height(at.x, at.z + 1.0) - height(at.x, at.z - 1.0)
		var up := Vector3(-dx * 0.5, 1.0, -dz * 0.5).normalized()
		put.call(String(style["paving"]), Transform3D(Basis(Quaternion(Vector3.UP, up)) * tile, gy.call(at) + up * 0.04))
	match String(style["fence"]):
		"wood":
			for s2 in [-1.0, 1.0]:
				for k in 3:
					var at: Vector3 = door + out * (VS * (1.0 + 2.0 * k)) + side * s2 * (VS * 2.6)
					var piece: String = "Prop_WoodenFence_Single" if k == 0 else ["Prop_WoodenFence_Extension1", "Prop_WoodenFence_Extension2"][k % 2]
					put.call(piece, Transform3D(rot * Basis(Vector3.UP, PI / 2.0) * tile, gy.call(at) - Vector3(0, 0.1, 0)))
		"metal":
			put.call("Prop_MetalFence_Ornament", Transform3D(rot * tile, gy.call(door + out * (VS * 6.2) + side * (VS * 1.6)) - Vector3(0, 0.1, 0)))
			put.call("Prop_MetalFence_Simple", Transform3D(rot * tile, gy.call(door + out * (VS * 6.2) - side * (VS * 1.6)) - Vector3(0, 0.1, 0)))
	if hsh % 3 != 0:
		# The arch's feet on the lower side of the slope, so neither floats.
		var arch := door + out * (VS * 6.0)
		var low := minf(gy.call(arch - side * VS).y, gy.call(arch + side * VS).y)
		put.call("Wall_Arch", Transform3D(rot * tile, Vector3(arch.x, low - 0.05, arch.z)))
	# A wooden lean-to beside the path on most houses: one sloping roof of the kit's left and right Roof_Wooden_2x1
	# halves (each trimmed on its outer end only, so they meet without a doubled edge; high along the origin edge,
	# falling ~1.1 over the 1.5 depth) on four timber posts cut to meet it, high side towards the house.
	if hsh % 4 != 3:
		var shed := door + out * (VS * 4.5) - side * (VS * 5.5)
		var high := (2.4 + 0.3 * float(hsh % 3)) * VS          # the eaves above the ground at the back
		var across := Vector3.UP.cross(out).normalized()      # the roof's own x, so its z runs down the slope, outwards
		var roof_b := Basis(across, Vector3.UP, out) * tile
		var back: Vector3 = shed - out * (0.75 * VS)
		# The roof sits level off the highest of its four feet, so every post reaches it.
		var feet := []
		for corner in [[-1.0, 0.1, 0.0], [1.0, 0.1, 0.0], [-1.0, 1.3, 0.83], [1.0, 1.3, 0.83]]:
			feet.append([gy.call(back + across * (float(corner[0]) * 2.05 * VS) + out * (float(corner[1]) * VS)), float(corner[2])])
		var top := -INF
		for f in feet:
			top = maxf(top, (f[0] as Vector3).y + float(f[1]) * VS)
		var eave := top + high
		for sx2 in [-1.0, 1.0]:
			put.call("Roof_Wooden_2x1_L" if sx2 < 0.0 else "Roof_Wooden_2x1_R", Transform3D(roof_b, Vector3(back.x, eave - 1.0 * VS, back.z) + across * (sx2 * VS)))
		for f in feet:
			# Corner_Exterior_Wood is a post 3 tall from its foot; scaled to reach the roof's underside there.
			var foot: Vector3 = f[0]
			var tall: float = eave - float(f[1]) * VS - 0.05 * VS - foot.y + 0.1
			put.call("Corner_Exterior_Wood", Transform3D(Basis(across, Vector3.UP, out) * Basis.from_scale(Vector3(VS, tall / 3.0, VS)), foot - Vector3(0, 0.1, 0)))
		# Under the shed, the trade's goods.
		var goods: Array = TownsScript.TRADES[town["trade"]]["yard"]
		for k in 2:
			var g: Array = goods[(hsh / 3 + k * 2) % goods.size()]
			_dress(dress, String(g[0]), float(g[1]), gy.call(shed + side * (VS * (k * 2.0 - 1.0))), rng.randf() * TAU)
	# The trade's workshop (a smithy's anvil and grindstone, a sawpit, a fish store) on one house in a few; a couple
	# more of the trade's things out in the yard on the rest.
	var trade: Dictionary = TownsScript.TRADES[town["trade"]]
	if (hsh / 5) % int(trade["work_every"]) == 0:
		var work: Array = trade["work"]
		for k in work.size():
			var g: Array = work[k]
			var at: Vector3 = door + out * (VS * (1.6 + 1.5 * (k / 2))) + side * (VS * (3.5 + 1.3 * (k % 2)))
			_dress(dress, String(g[0]), float(g[1]), gy.call(at), rng.randf_range(-0.4, 0.4) + atan2(out.x, out.z))
	else:
		var goods: Array = trade["yard"]
		for k in 2:
			var g: Array = goods[(hsh / 7 + k * 3 + 1) % goods.size()]
			_dress(dress, String(g[0]), float(g[1]), gy.call(door + out * (VS * rng.randf_range(2.0, 4.5)) + side * (VS * rng.randf_range(3.4, 4.8) * (1.0 if k == 0 else -1.0))), rng.randf() * TAU)

## A woodpile against the back wall, on most houses: logs for the fire, or the trade's own stack.
func _woodpile(side: Array, hsh: int, town: Dictionary, dress: Dictionary) -> void:
	if hsh % 3 == 0:
		return
	var start: Vector3 = side[0]
	var dir: Vector3 = side[1]
	var length: float = side[2]
	var out: Vector3 = side[3]
	var turn := atan2(dir.x, dir.z)
	var at := start + dir * (length * (0.25 + float(hsh % 50) / 100.0)) + out * 1.1
	_dress(dress, "Wood_Log_Stack", 1.3, at, turn)
	if town["trade"] == "mining":
		_dress(dress, "Stone_Bricks_Stack_Small", 1.2, at + dir * 2.2, turn)
	elif length > 10.0:
		_dress(dress, "Wood_Log_Stack", 1.3, at + dir * 2.0, turn + PI)

## One loose thing by name (towns.gd: "d:" a Dungeon piece, "v:" a village piece, "t:" a tool, else a Resource
## Bits one), resting on the ground at `at` (relative to the building's base), turned `turn`.
func _dress(dress: Dictionary, name: String, scale: float, at: Vector3, turn: float) -> void:
	var path := _dress_path(name)
	var m := _mesh(path)
	if m == null:
		return
	if not dress.has(path):
		dress[path] = []
	dress[path].append(Transform3D(Basis(Vector3.UP, turn).scaled(Vector3.ONE * scale), at - Vector3(0, m.get_aabb().position.y * scale, 0)))

func furniture_node(piece: String) -> Node3D:
	var m := _mesh(_dress_path(piece))
	if m == null:
		return super.furniture_node(piece)
	var mi := MeshInstance3D.new()
	mi.mesh = m
	mi.scale = Vector3.ONE * 1.2
	mi.position.y = -m.get_aabb().position.y * 1.2
	var n := Node3D.new()
	n.add_child(mi)
	_snowable(mi)
	return n

func _dress_path(name: String) -> String:
	match name.left(2):
		"d:": return DUNGEON + name.substr(2) + ".gltf"
		"v:": return VILLAGE + name.substr(2) + ".gltf"
		"t:": return TOOLS + name.substr(2) + ".gltf"
	return RES + name + ".gltf"

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
	if _in_any(Vector2(shore.x, shore.z) - Vector2(into.x, into.z) * 2.0, _lakes_touching(b)):
		return Node3D.new() # that side is more of the same lake: no shore to build a dock on
	# From the water's edge, a step back on the land, not the box's.
	var edge := _lake_edge(water, Vector2(shore.x, shore.z), Vector2(into.x, into.z), minf(float(b[2]) - float(b[0]), float(b[3]) - float(b[1])) / 2.0)
	if edge.is_finite():
		shore = Vector3(edge.x, 0, edge.y) - into * VS
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
	# What a jetty has on it: rope, a bucket, a crate or a covered pallet, a lantern at the end.
	var cargo := {}
	var deck := shore + into * (VS * 2.0) + Vector3(0, water + 0.45 + 0.1, 0)
	var across := Vector3(into.z, 0, -into.x)
	_dress(cargo, ["t:rope_bundle_A", "t:rope_bundle_B"][hsh % 2], 1.3, deck + across * 1.0, float(hsh % 7))
	_dress(cargo, ["Pallet_Wood_Covered_A", "d:crates_stacked", "d:barrel_small"][hsh % 3], 1.0, deck + into * 2.2 - across * 0.9, float(hsh % 5))
	_dress(cargo, "t:bucket_metal", 1.1, deck + into * 3.2 + across * 1.0, 0.0)
	_dress(cargo, "t:lantern", 1.3, end + into * 0.6 + Vector3(0, 0.1, 0), 0.0)
	for pth in cargo:
		node.add_child(_multi(_mesh(pth), cargo[pth]))
	return node

## Rocks along a box's edge, every `gap` units or so, from the box's corner so every client agrees.
func _rim_rocks(b: Array, ground: float, gap: float, lake_level := INF) -> Node3D:
	var rng := RandomNumberGenerator.new()
	rng.seed = (int(b[0]) * 73856093) ^ (int(b[1]) * 19349663)
	var picks := ["Rock_1_A_Color1", "Rock_1_J_Color1", "Rock_2_A_Color1", "Rock_3_E_Color1"]
	var per := {}
	var sides := [[Vector2(b[0], b[1]), Vector2(b[2], b[1])], [Vector2(b[2], b[1]), Vector2(b[2], b[3])],
		[Vector2(b[2], b[3]), Vector2(b[0], b[3])], [Vector2(b[0], b[3]), Vector2(b[0], b[1])]]
	var joined := _lakes_touching(b)
	for sd in sides:
		var a: Vector2 = sd[0]
		var c: Vector2 = sd[1]
		var n := maxi(1, int(a.distance_to(c) / gap))
		for k in n + 1:
			var p := a.lerp(c, (k + rng.randf() * 0.6) / (n + 1))
			if _in_any(p, joined):
				continue # the edge it shares with the rest of the lake is open water, not shore
			var y := ground - 0.3
			if lake_level < INF:
				# A lake's rocks stand at its water's edge, which wanders inside the box.
				var inward := (Vector2((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0) - (a + c) / 2.0).normalized()
				var w := _lake_edge(lake_level, p, inward, minf(float(b[2]) - float(b[0]), float(b[3]) - float(b[1])) / 2.0)
				if w.is_finite():
					p = w - inward * rng.randf_range(2.0, 4.0)
					y = height(p.x, p.y) - 0.3
			var pick: String = picks[rng.randi() % picks.size()]
			if not per.has(pick):
				per[pick] = []
			var s := rng.randf_range(1.6, 3.2)
			if lake_level < INF: # a pond's rocks are a pond's size
				s *= clampf(minf(float(b[2]) - float(b[0]), float(b[3]) - float(b[1])) / 50.0, 0.4, 1.0)
			per[pick].append(Transform3D(Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3.ONE * s), Vector3(p.x, y, p.y)))
	var node := Node3D.new()
	for pick in per:
		var m := _mesh(FOREST + String(pick) + ".gltf")
		if m:
			node.add_child(_multi(m, per[pick], Color(0.72, 0.69, 0.64)))
	return node

## The other lake boxes a lake box touches (a big lake is laid as strips side by side), slightly grown.
func _lakes_touching(b: Array) -> Array:
	var me := Rect2(float(b[0]), float(b[1]), float(b[2]) - float(b[0]), float(b[3]) - float(b[1]))
	var out := []
	for l in world.lakes:
		var lb: Array = l["b"]
		var r := Rect2(float(lb[0]), float(lb[1]), float(lb[2]) - float(lb[0]), float(lb[3]) - float(lb[1]))
		if r != me and r.grow(1.5).intersects(me):
			out.append(r.grow(1.5))
	return out

static func _in_any(p: Vector2, boxes: Array) -> bool:
	for r in boxes:
		if (r as Rect2).has_point(p):
			return true
	return false

## Where the water begins, going `inward` from a lake box's edge at `from` (worldgen rounds the basin and lets its
## shore wander, so that is some way in from the box): INF when there is no water within `reach`.
func _lake_edge(level: float, from: Vector2, inward: Vector2, reach: float) -> Vector2:
	var d := 0.0
	while d <= reach:
		var p := from + inward * d
		if height(p.x, p.y) < level:
			return p
		d += 1.0
	return Vector2.INF

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
uniform float swell = 0.0; // the open sea: lines of breakers rolling in over the shallows
global uniform float world_rain; // life.gd: how hard rain is falling
global uniform vec2 world_clouds;
global uniform vec2 world_cloud_off;
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
	if (world_rain > 0.02) {
		// Rain: rings spreading from where each drop lands, more of them the harder it falls.
		vec2 rn = vec2(0.0);
		for (int l = 0; l < 3; l++) {
			vec2 rq = q * 1.3 + float(l) * vec2(17.3, 9.1);
			vec2 cell = floor(rq);
			float s = h(cell + float(l) * 3.7);
			float age = fract(TIME * (1.1 + 0.4 * s) + s * 7.0);
			vec2 d = fract(rq) - (0.3 + 0.4 * vec2(h(cell + 1.7), h(cell + 4.3)));
			float r = length(d);
			float ring = (1.0 - smoothstep(0.0, 0.05, abs(r - age * 0.32))) * (1.0 - age) * step(s, world_rain);
			rn += d / (r + 0.001) * ring;
		}
		wn = normalize(wn + vec3(rn.x, 0.0, rn.y) * 0.7 * smoothstep(70.0, 25.0, length(VERTEX))); // gone in the distance, where they would shimmer
	}
	NORMAL = normalize((VIEW_MATRIX * vec4(wn, 0.0)).xyz);
	float depth = texture(depth_tex, SCREEN_UV).r;
	vec4 v = INV_PROJECTION_MATRIX * vec4(SCREEN_UV * 2.0 - 1.0, depth, 1.0);
	v.xyz /= v.w;
	float thick = clamp(VERTEX.z - v.z, 0.0, 20.0);
	float k = smoothstep(0.0, 3.0, thick);
	vec3 c = mix(vec3(0.14, 0.44, 0.47), vec3(0.03, 0.14, 0.24), k);
	// The water's edge laps in and out and wanders, so it never follows the straight edges of the ground's triangles.
	float edge = 0.06 + 0.3 * n(q * 0.22 + vec2(t * 0.15, 0.0)) + 0.16 * (0.5 + 0.5 * sin(TIME * 0.8 + 6.2832 * n(q * 0.03)));
	float foam = (1.0 - smoothstep(edge, edge + 0.5, thick)) * (0.6 + 0.4 * n(q * 2.0 + t * 2.0));
	if (swell > 0.0) {
		// Breakers: crests a little apart over the shallows, each rolling in towards the beach and spending itself
		// at the edge; broken up along their length so they come in as sets, not as rings.
		float ph = thick * 4.0 + TIME * 1.2 + 6.2832 * n(q * 0.02);
		float crest = smoothstep(0.75, 0.97, sin(ph)) * smoothstep(0.2, 0.5, n(q * 0.06 + vec2(TIME * 0.05, 0.0)));
		foam = max(foam, swell * crest * smoothstep(4.0, 1.5, thick) * smoothstep(0.1, 0.5, thick) * (0.7 + 0.3 * n(q * 1.5)));
	}
	if (world_clouds.x > 0.0) {
		vec2 cq = q + world_cloud_off;
		float cl = 0.65 * n(cq / 90.0) + 0.35 * n(cq / 37.0 + 5.3);
		c *= 1.0 - 0.32 * world_clouds.x * smoothstep(world_clouds.y, world_clouds.y + 0.12, cl);
	}
	ALBEDO = mix(c, vec3(0.92, 0.95, 0.95), foam * 0.8);
	ALPHA = clamp(0.55 + k * 0.4 + foam * 0.4, 0.0, 0.96) * smoothstep(edge - 0.06, edge + 0.06, thick);
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
				"banner_shield_green.gltf", "banner_triple_yellow.gltf", "banner_thin_white.gltf", "banner_green.gltf",
				"banner_patternA_white.gltf", "banner_patternC_red.gltf", "banner_shield_red.gltf", "banner_thin_yellow.gltf", "banner_triple_red.gltf"]
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

extends MultiMeshInstance3D
## Grass round the player: one MultiMesh of blades in a square patch, drawn in one call, no shadows.
##
## The blades never move on the CPU. The shader wraps each one's place round the player (a blade keeps its spot in
## the world as we walk, and the ones left behind come in again ahead), stands it on the ground from a small map
## of heights, sways it in the wind and parts it round our feet, and shrinks it away towards the patch's edge.
## The map (height, and whether grass grows there: not on roads, water, building footprints) is redrawn only when
## we have walked MAP_STEP from where it was last drawn; slopes and snow are left bare by the shader itself.

const PATCH := 96.0        # the square of grass round us, in world units
const BLADES := 24576      # in the patch: under 3 per square unit
const MAP_N := 64          # map texels a side
const MAP_SIZE := 128.0    # world units the map covers (the patch and a margin of MAP_STEP to walk into)
const MAP_STEP := 16.0     # redraw the map when we are this far from its centre

var world: RefCounted
var land: RefCounted            # land.gd: no grass grows on the fields
var _mat: ShaderMaterial
var _img: Image
var _tex: ImageTexture
var _map_at := Vector2(INF, INF)
var _task := -1                 # the worker drawing the next map, or -1
var _origin := Vector2.ZERO     # the next map's corner, and what _gather took for it
var _segs: Array = []
var _boxes: Array = []
var _lakes: Array = []
var _data := PackedFloat32Array() # the drawn map, and its corner
var _data_origin := Vector2.ZERO

const SHADER := """shader_type spatial;
render_mode cull_disabled, world_vertex_coords, specular_disabled;
uniform vec2 center;
uniform float patch_size = 64.0;
uniform sampler2D ground : filter_linear, repeat_disable; // r height, g grass grows (0..1)
uniform vec2 map_origin;
uniform float map_size = 96.0;
uniform float wind = 0.3;
uniform float sea_level = -1000.0;
global uniform vec2 world_cold_band; // land.gd: fully cold north of x, none south of y (world z)
global uniform float world_autumn;  // life.gd: the year's turn to gold, 0 .. 1
varying vec3 wp;
varying float tip;
varying float cold;
varying float flower; // 0 a plain blade, else which flower its tip carries (1..5)
float h(vec2 p) { vec3 q = fract(vec3(p.xyx) * 0.1031); q += dot(q, q.yzx + 33.33); return fract((q.x + q.y) * q.z); }
float n(vec2 p) {
	vec2 i = floor(p); vec2 f = fract(p); vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(h(i), h(i + vec2(1, 0)), u.x), mix(h(i + vec2(0, 1)), h(i + vec2(1, 1)), u.x), u.y);
}
float fbm(vec2 p) { float v = 0.0; float a = 0.5; for (int k = 0; k < 4; k++) { v += a * n(p); p *= 2.03; a *= 0.5; } return v; }
vec2 at(vec2 p) { return texture(ground, (p - map_origin) / map_size).rg; }
void vertex() {
	vec3 o = MODEL_MATRIX[3].xyz;            // where the blade sits in the patch
	vec3 off = VERTEX - o;                   // the blade itself, turned and sized
	vec2 rel = mod(o.xz - center + patch_size * 0.5, patch_size) - patch_size * 0.5;
	vec2 p = center + rel;
	vec2 g = at(p);
	float e = 2.0;
	float slope = abs(at(p + vec2(e, 0)).r - at(p - vec2(e, 0)).r) + abs(at(p + vec2(0, e)).r - at(p - vec2(0, e)).r);
	cold = 1.0 - smoothstep(world_cold_band.x, world_cold_band.y, p.y + 220.0 * (fbm(p / 300.0) - 0.5)); // as the ground
	float grow = g.y;
	grow *= 1.0 - smoothstep(1.2, 2.0, slope);              // steep ground is rock
	float snow_line = mix(26.0, 16.0, cold);
	grow *= 1.0 - smoothstep(snow_line, snow_line + 6.0, g.x); // high ground is snow, lower in the north
	grow *= step(sea_level + 0.4, g.x);                     // nothing on the beach or under the sea
	grow *= 1.0 - smoothstep(0.55, 1.0, length(rel) / (patch_size * 0.5)); // thin out to the edge
	grow *= smoothstep(0.15, 0.55, n(p * 0.21) + 0.25);     // clumps, with bare patches between
	tip = clamp(off.y / 0.45, 0.0, 1.0);
	// Some ground is grazed short and some is left long: blade height wanders over tens of units, from a lawn's
	// stubble to knee-high meadow, so no two fields look the same.
	float lush = n(p * 0.018) * 0.65 + n(p * 0.06 + 13.0) * 0.35;
	off.y *= mix(0.3, 1.0, smoothstep(0.3, 0.75, lush)) * mix(1.0, 0.65, cold); // the north's tussock is short
	// Wildflowers: a few blades in the warm meadows carry a flower, thick in some drifts and absent in others.
	float drift = smoothstep(0.5, 0.75, n(p * 0.045 + 7.0));
	flower = step(1.0 - 0.06 * drift * (1.0 - cold), h(floor(p * 9.0))) * (1.0 + floor(h(floor(p * 9.0) + 5.3) * 5.0));
	off.y *= 1.0 + 0.25 * step(0.5, flower);
	// Wind: a gust rolling across the field, more at the tips. Our feet push the blades aside.
	float gust = sin(TIME * 1.7 + dot(p, vec2(0.21, 0.13))) * 0.5 + sin(TIME * 3.1 + p.x * 0.7) * 0.15;
	vec3 bend = vec3(0.3, 0.0, 0.18) * gust * (0.15 + wind) * tip * tip;
	vec2 away = p - center;
	float d = length(away);
	bend.xz += away / max(d, 0.001) * (1.0 - smoothstep(0.4, 1.2, d)) * 0.25 * tip;
	vec3 pos = vec3(p.x, g.x - 0.04, p.y) + (off + bend) * grow;
	wp = pos;
	VERTEX = pos;
}
void fragment() {
	vec2 q = wp.xz;
	float k = smoothstep(0.3, 0.7, n(q / 14.0) * 0.6 + n(q / 5.0) * 0.4);
	// Close to the meadow under it, so it reads as texture rather than as a crop.
	vec3 base = mix(vec3(0.31, 0.49, 0.18), vec3(0.42, 0.58, 0.22), k);
	vec3 top = mix(vec3(0.45, 0.63, 0.24), vec3(0.55, 0.68, 0.28), k);
	vec3 c = mix(base, top, tip) * (0.9 + 0.2 * h(floor(q * 3.0)));
	// The north: grey-green tussock with frosted tips.
	vec3 cb = mix(vec3(0.36, 0.42, 0.30), vec3(0.45, 0.47, 0.36), k);
	vec3 ct = mix(vec3(0.58, 0.58, 0.46), vec3(0.80, 0.84, 0.86), smoothstep(0.55, 0.95, cold) * 0.7);
	c = mix(c, mix(cb, ct, tip) * (0.9 + 0.2 * h(floor(q * 3.0))), cold);
	// Autumn: blades gone over to straw, rust at the tips.
	c = mix(c, mix(vec3(0.58, 0.48, 0.22), vec3(0.66, 0.42, 0.20), tip), world_autumn * (1.0 - cold) * 0.6);
	// A flower at the tip: buttercup, daisy, clover, poppy or cornflower.
	vec3 fc = flower < 1.5 ? vec3(0.96, 0.82, 0.22) : flower < 2.5 ? vec3(0.96, 0.95, 0.90) : flower < 3.5 ? vec3(0.70, 0.45, 0.78) : flower < 4.5 ? vec3(0.88, 0.22, 0.18) : vec3(0.40, 0.55, 0.92);
	c = mix(c, fc, step(0.5, flower) * smoothstep(0.7, 0.95, tip));
	ALBEDO = pow(c, vec3(2.2));
	ROUGHNESS = 1.0;
	NORMAL = normalize((VIEW_MATRIX * vec4(0.0, 1.0, 0.0, 0.0)).xyz); // both sides lit as the ground is: no dark backs
}"""

func setup(p_world: RefCounted) -> void:
	world = p_world
	cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	# The blades are moved in the shader, so the box the engine culls by must hold anywhere they can be.
	custom_aabb = AABB(Vector3(-100000, -1000, -100000), Vector3(200000, 2000, 200000))
	_mat = ShaderMaterial.new()
	var sh := Shader.new()
	sh.code = SHADER
	_mat.shader = sh
	_mat.set_shader_parameter("patch_size", PATCH)
	_mat.set_shader_parameter("map_size", MAP_SIZE)
	_mat.set_shader_parameter("sea_level", float(world.sea_level) if world.has_sea else -1000.0)
	_img = Image.create_empty(MAP_N, MAP_N, false, Image.FORMAT_RGF)
	_tex = ImageTexture.create_from_image(_img)
	_mat.set_shader_parameter("ground", _tex)
	material_override = _mat
	var mm := MultiMesh.new()
	mm.transform_format = MultiMesh.TRANSFORM_3D
	mm.mesh = _blade()
	mm.instance_count = BLADES
	var rng := RandomNumberGenerator.new()
	rng.seed = 7
	for i in BLADES:
		var b := Basis(Vector3.UP, rng.randf() * TAU).scaled(Vector3(rng.randf_range(0.7, 1.3), rng.randf_range(0.6, 1.15), 1.0))
		mm.set_instance_transform(i, Transform3D(b, Vector3(rng.randf() * PATCH, 0.0, rng.randf() * PATCH)))
	multimesh = mm

## One blade: a tapered strip in two pieces with a single tip, 0.45 tall, leaning a little.
func _blade() -> ArrayMesh:
	var w := 0.045
	var v := PackedVector3Array([
		Vector3(-w, 0, 0), Vector3(w, 0, 0),
		Vector3(-w * 0.65, 0.22, 0.02), Vector3(w * 0.65, 0.22, 0.02),
		Vector3(0, 0.45, 0.07)])
	var nm := PackedVector3Array()
	for _i in v.size():
		nm.append(Vector3(0, 0, -1))
	var arr := []
	arr.resize(Mesh.ARRAY_MAX)
	arr[Mesh.ARRAY_VERTEX] = v
	arr[Mesh.ARRAY_NORMAL] = nm
	arr[Mesh.ARRAY_INDEX] = PackedInt32Array([0, 1, 2, 1, 3, 2, 2, 3, 4])
	var m := ArrayMesh.new()
	m.add_surface_from_arrays(Mesh.PRIMITIVE_TRIANGLES, arr)
	return m

## Every frame: where we are and how hard the wind blows (0..1). The map under the grass is redrawn on a worker
## thread when we have walked MAP_STEP from its centre (the margin covers us meanwhile); the first is drawn at once.
func follow(center: Vector2, wind: float) -> void:
	_mat.set_shader_parameter("center", center)
	_mat.set_shader_parameter("wind", wind)
	if _task != -1 and WorkerThreadPool.is_task_completed(_task):
		WorkerThreadPool.wait_for_task_completion(_task)
		_task = -1
		_show_map()
	if _task == -1 and center.distance_to(_map_at) > MAP_STEP:
		var first := _map_at == Vector2.INF
		_map_at = center
		_gather(center)
		if first:
			_draw_map()
			_show_map()
		else:
			_task = WorkerThreadPool.add_task(_draw_map, false, "grass map")

func _exit_tree() -> void:
	if _task != -1:
		WorkerThreadPool.wait_for_task_completion(_task)
		_task = -1

## On the main thread, as the world's cells change under it: the roads, rivers and footprints round `center`, and
## the lakes, for _draw_map.
func _gather(center: Vector2) -> void:
	_origin = center - Vector2(MAP_SIZE, MAP_SIZE) * 0.5
	var origin := _origin
	var area := Rect2(origin, Vector2(MAP_SIZE, MAP_SIZE))
	var c0 := Vector2i(int(floor(origin.x / world.CELL)), int(floor(origin.y / world.CELL)))
	var c1 := Vector2i(int(floor((origin.x + MAP_SIZE) / world.CELL)), int(floor((origin.y + MAP_SIZE) / world.CELL)))
	_segs = []
	_boxes = []
	for cx in range(c0.x, c1.x + 1):
		for cy in range(c0.y, c1.y + 1):
			for s in world.road_segments(cx, cy):
				_segs.append([Vector2(s[0], s[1]), Vector2(s[2], s[3]), float(s[4]) + 0.6])
			for o in world.in_cell(cx, cy):
				if o.has("b"):
					var b: Array = o["b"]
					_boxes.append(Rect2(float(b[0]) - 0.5, float(b[1]) - 0.5, float(b[2]) - float(b[0]) + 1.0, float(b[3]) - float(b[1]) + 1.0))
	for r in world.rivers:
		var pts: Array = r["pts"]
		for k in pts.size() - 1:
			var a := Vector2(pts[k][0], pts[k][1])
			var b := Vector2(pts[k + 1][0], pts[k + 1][1])
			var hw := float(r.get("half_width", 5.0)) + 1.5
			if Rect2(a, Vector2.ZERO).expand(b).grow(hw).intersects(area):
				_segs.append([a, b, hw])
	if land:
		for f in land.fields_in(area):
			_boxes.append(f[0])
	_lakes = []
	for l in world.lakes:
		var lb: Array = l["b"]
		if Rect2(float(lb[0]), float(lb[1]), float(lb[2]) - float(lb[0]), float(lb[3]) - float(lb[1])).intersects(area):
			_lakes.append([Rect2(float(lb[0]), float(lb[1]), float(lb[2]) - float(lb[0]), float(lb[3]) - float(lb[1])), float(l.get("level", 0.0)) + 0.4])

## Heights and where grass grows (roads, rivers, lakes and what is built are bare), from what _gather took. Runs on
## a worker thread: it reads only those and the ground's heights, and writes only _data.
func _draw_map() -> void:
	var origin := _origin
	var cell := MAP_SIZE / MAP_N
	var grow := PackedFloat32Array()
	grow.resize(MAP_N * MAP_N)
	grow.fill(1.0)
	for r in _boxes:
		_clear_box(grow, origin, cell, r)
	for s in _segs:
		var a: Vector2 = s[0]
		var b: Vector2 = s[1]
		var hw: float = s[2]
		var box := Rect2(a, Vector2.ZERO).expand(b).grow(hw)
		var i0 := clampi(int((box.position.x - origin.x) / cell), 0, MAP_N - 1)
		var i1 := clampi(int((box.end.x - origin.x) / cell), 0, MAP_N - 1)
		var j0 := clampi(int((box.position.y - origin.y) / cell), 0, MAP_N - 1)
		var j1 := clampi(int((box.end.y - origin.y) / cell), 0, MAP_N - 1)
		for j in range(j0, j1 + 1):
			for i in range(i0, i1 + 1):
				var p := origin + Vector2(i + 0.5, j + 0.5) * cell
				if Geometry2D.get_closest_point_to_segment(p, a, b).distance_to(p) < hw:
					grow[j * MAP_N + i] = 0.0
	var data := PackedFloat32Array()
	data.resize(MAP_N * MAP_N * 2)
	for j in MAP_N:
		for i in MAP_N:
			var p := origin + Vector2(i + 0.5, j + 0.5) * cell
			var hgt: float = world.height(p.x, p.y)
			var g := grow[j * MAP_N + i]
			for l in _lakes:
				if (l[0] as Rect2).has_point(p) and hgt < float(l[1]):
					g = 0.0
			data[(j * MAP_N + i) * 2] = hgt
			data[(j * MAP_N + i) * 2 + 1] = g
	_data = data
	_data_origin = origin

func _show_map() -> void:
	_img.set_data(MAP_N, MAP_N, false, Image.FORMAT_RGF, _data.to_byte_array())
	_tex.update(_img)
	_mat.set_shader_parameter("map_origin", _data_origin)

func _clear_box(grow: PackedFloat32Array, origin: Vector2, cell: float, r: Rect2) -> void:
	var i0 := clampi(int((r.position.x - origin.x) / cell), 0, MAP_N - 1)
	var i1 := clampi(int((r.end.x - origin.x) / cell), 0, MAP_N - 1)
	var j0 := clampi(int((r.position.y - origin.y) / cell), 0, MAP_N - 1)
	var j1 := clampi(int((r.end.y - origin.y) / cell), 0, MAP_N - 1)
	if r.end.x < origin.x or r.end.y < origin.y or r.position.x > origin.x + MAP_SIZE or r.position.y > origin.y + MAP_SIZE:
		return
	for j in range(j0, j1 + 1):
		for i in range(i0, i1 + 1):
			grow[j * MAP_N + i] = 0.0

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
	_clear = {"fog": _env.fog_density, "fog_color": _env.fog_light_color, "sun": _sun.light_energy}
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

## The server's weather ({name, color, intensity}, empty for clear sky).
func apply_weather(w: Dictionary) -> void:
	if w.is_empty():
		_env.fog_density = _clear["fog"]
		_env.fog_light_color = _clear["fog_color"]
		_sun.light_energy = _clear["sun"]
		return
	var k := clampf(float(w.get("intensity", 0.5)), 0.0, 1.0)
	_env.fog_density = _clear["fog"] + 0.015 * k
	_env.fog_light_color = Color(str(w.get("color", "#888888")))
	_sun.light_energy = _clear["sun"] * (1.0 - 0.5 * k)

# ------------------------------------------------------------------------------------------- built blocks

var _block_nodes := {}    # chunk key -> Node3D of that chunk's blocks
var _block_index := {}    # chunk key -> {column key: true}
var _blocks := {}         # column key (cy << 15 | cx) -> {"b": base layer, "k": a character per layer, "o": owners}
var _block_mats := {}     # material -> Material

## Names of the block materials (0..3), for the HUD.
func block_name(kind: int) -> String:
	return ["stone", "wood", "brick", "glass"][clampi(kind, 0, 3)]

## What a block of each material is drawn with.
func block_material(kind: int) -> Material:
	var m := StandardMaterial3D.new()
	m.albedo_color = [Color("8a8f96"), Color("8a5a34"), Color("a6463a"), Color(0.7, 0.9, 1.0, 0.45)][clampi(kind, 0, 3)]
	if kind == 3:
		m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	return m

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
		if not cols.is_empty():
			_block_nodes[ck] = build_blocks(cols)
			add_child(_block_nodes[ck])

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
			var kind := kinds.unicode_at(i) - 48
			if kind < 0 or kind > 3:
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
	return node

## A picture of the whole playable world, px by px, for the map (M).
func map_image(px: int) -> Image:
	var img := Image.create(px, px, false, Image.FORMAT_RGB8)
	img.fill(Color("4a4f57"))
	var step: float = world.size / px
	for o in world.objects:
		if o.get("solid", false):
			var b: Array = o["b"]
			var p0 := Vector2i(int(b[0] / step), int(b[1] / step))
			var p1 := Vector2i(int(b[2] / step), int(b[3] / step))
			img.fill_rect(Rect2i(p0, (p1 - p0).max(Vector2i(1, 1))), Color("9aa0a8"))
	return img

## Extra checks for the scripted smoke test: [[name, passed, detail], ...].
func smoke_checks() -> Array:
	return [["theme drew the world", _chunks.size() > 0 and _terrain.size() > 0, "%d cells, %d ground" % [_chunks.size(), _terrain.size()]]]

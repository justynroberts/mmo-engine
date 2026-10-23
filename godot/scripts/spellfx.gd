extends Node3D
## What spells look like. Casting draws a rune under the caster and a flare at the hand; a bolt flies with a trail
## (lightning arcs instead); where it lands each look has its own burst: fire throws embers and smoke, frost
## shatters into shards, holy light falls in a column, shadow gathers inward, nature scatters leaves, poison leaves a
## cloud, mind rings outward, blood spatters. Areas sweep a ring over the ground, heals rise in a soft column, and
## anyone held, levitating or hasted shows it. Every look is keyed by the action's `fx` (proto::game::FX); nothing
## here decides what a spell does, the server has done that.

## fx -> its colour. An action with no fx is a plain blow and draws nothing here.
const COLORS := {
	"fire": Color("ff7a2a"), "frost": Color("9fdcff"), "shock": Color("cfe6ff"), "holy": Color("ffe39a"),
	"shadow": Color("8a4ce0"), "nature": Color("5fd36b"), "arcane": Color("d27cff"), "poison": Color("b8e23a"),
	"mind": Color("ff8ad8"), "blood": Color("d0213f"),
}
const BOLT_SPEED := 34.0   # units per second
const HELD_COLORS := [Color(0, 0, 0, 0), Color("ff8ad8"), Color("ffe39a"), Color("5fd36b")] # by hold kind: mez, stun, root

var _bolts: Array = []     # [{node, light, trail, from, to (Node3D), t, life, then (Callable)}]
var _fading: Array = []    # [{node, t, life, grow, spin, rise, light}]: things that play out and go
var _marks := {}           # "held:<id>" / "float:<id>" / "haste:<id>" -> node over or under that entity

static var _dot: Texture2D

## A soft round spot, white in the middle and clear at the edge: what sparks and motes are drawn with.
static func _spark_texture() -> Texture2D:
	if _dot == null:
		var g := Gradient.new()
		g.set_color(0, Color(1, 1, 1, 1))
		g.set_color(1, Color(1, 1, 1, 0))
		g.add_point(0.35, Color(1, 1, 1, 0.8))
		var t := GradientTexture2D.new()
		t.gradient = g
		t.fill = GradientTexture2D.FILL_RADIAL
		t.fill_from = Vector2(0.5, 0.5)
		t.fill_to = Vector2(1.0, 0.5)
		t.width = 32
		t.height = 32
		_dot = t
	return _dot

static func color(fx: String) -> Color:
	return COLORS.get(fx, Color("ffd27a"))

# ---------------------------------------------------------------- building blocks

## Unshaded and added onto what is behind (glows), or blended normally (smoke, blood, shadow).
func _mat(c: Color, energy := 1.6, glow := true, textured := false) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.shading_mode = BaseMaterial3D.SHADING_MODE_UNSHADED
	m.transparency = BaseMaterial3D.TRANSPARENCY_ALPHA
	m.blend_mode = BaseMaterial3D.BLEND_MODE_ADD if glow else BaseMaterial3D.BLEND_MODE_MIX
	m.albedo_color = c
	m.cull_mode = BaseMaterial3D.CULL_DISABLED
	if glow:
		m.emission_enabled = true
		m.emission = c
		m.emission_energy_multiplier = energy
	if textured:
		m.albedo_texture = _spark_texture()
		m.billboard_mode = BaseMaterial3D.BILLBOARD_PARTICLES
		m.vertex_color_use_as_albedo = true
	return m

func _mesh(mesh: Mesh, m: Material, at: Vector3) -> MeshInstance3D:
	var mi := MeshInstance3D.new()
	mi.mesh = mesh
	mi.material_override = m
	mi.cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_OFF
	mi.position = at
	add_child(mi)
	return mi

func _sphere(radius: float) -> SphereMesh:
	var s := SphereMesh.new()
	s.radius = radius
	s.height = radius * 2.0
	s.radial_segments = 14
	s.rings = 7
	return s

func _ring(inner: float, outer: float) -> TorusMesh:
	var t := TorusMesh.new()
	t.inner_radius = inner
	t.outer_radius = outer
	t.rings = 48
	t.ring_segments = 6
	return t

## Something that plays out over `life` seconds and goes: grows to `grow` (a radius) if set, spins, rises, fades.
func _fade(node: Node3D, life: float, grow := 0.0, spin := 0.0, rise := 0.0) -> void:
	_fading.append({"node": node, "t": 0.0, "life": life, "grow": grow, "spin": spin, "rise": rise,
		"base": node.scale, "alpha": _alpha_of(node)})

func _alpha_of(node: Node3D) -> float:
	if node is MeshInstance3D and (node as MeshInstance3D).material_override is StandardMaterial3D:
		return ((node as MeshInstance3D).material_override as StandardMaterial3D).albedo_color.a
	return 1.0

func _light(at: Vector3, c: Color, energy: float, reach: float, life: float) -> void:
	var l := OmniLight3D.new()
	l.light_color = c
	l.light_energy = energy
	l.omni_range = reach
	l.position = at
	add_child(l)
	_fading.append({"node": l, "t": 0.0, "life": life, "grow": 0.0, "spin": 0.0, "rise": 0.0, "light": energy})

## A one-shot spray of motes. `o` holds what differs from a quick burst: amount, life, speed, spread (degrees),
## dir, gravity, size, radius (of the sphere they start in), ring (start on a flat ring that wide), glow (false for
## smoke and blood), explode (0 trickles out, 1 all at once), mesh (instead of a soft dot).
func _motes(at: Vector3, c: Color, o: Dictionary) -> CPUParticles3D:
	var p := CPUParticles3D.new()
	p.one_shot = true
	p.emitting = true
	p.amount = int(o.get("amount", 20))
	p.lifetime = float(o.get("life", 0.6))
	p.explosiveness = float(o.get("explode", 0.9))
	p.randomness = 0.5
	p.direction = o.get("dir", Vector3.UP)
	p.spread = float(o.get("spread", 180.0))
	p.initial_velocity_min = float(o.get("speed", 5.0)) * 0.4
	p.initial_velocity_max = float(o.get("speed", 5.0))
	p.gravity = o.get("gravity", Vector3(0, -4.0, 0))
	p.damping_min = float(o.get("damp", 0.0))
	p.damping_max = float(o.get("damp", 0.0))
	var size := float(o.get("size", 0.3))
	p.scale_amount_min = 0.5
	p.scale_amount_max = 1.2
	if o.has("ring"):
		p.emission_shape = CPUParticles3D.EMISSION_SHAPE_RING
		p.emission_ring_axis = Vector3.UP
		p.emission_ring_radius = float(o["ring"])
		p.emission_ring_inner_radius = float(o["ring"]) * 0.6
		p.emission_ring_height = 0.1
	elif o.has("radius"):
		p.emission_shape = CPUParticles3D.EMISSION_SHAPE_SPHERE
		p.emission_sphere_radius = float(o["radius"])
	var glow := bool(o.get("glow", true))
	if o.has("mesh"):
		p.mesh = o["mesh"]
		var m := _mat(c, 1.4, glow)
		m.vertex_color_use_as_albedo = true
		p.material_override = m
		p.particle_flag_rotate_y = true
		p.angle_min = 0.0
		p.angle_max = 360.0
	else:
		var q := QuadMesh.new()
		q.size = Vector2(size, size)
		q.material = _mat(c, 1.3, glow, true)
		p.mesh = q
	var ramp := Gradient.new()
	ramp.set_color(0, Color(1, 1, 1, 1))
	ramp.set_color(1, Color(1, 1, 1, 0))
	p.color_ramp = ramp
	p.position = at
	add_child(p)
	_fading.append({"node": p, "t": 0.0, "life": p.lifetime + 0.4, "grow": 0.0, "spin": 0.0, "rise": 0.0})
	return p

## A jagged bolt of lightning from a to b, made of thin glowing slats; it flickers once and is gone.
func _lightning(a: Vector3, b: Vector3, c: Color) -> void:
	for pass_n in 2:
		var pts: Array[Vector3] = [a]
		var n := 9
		var side := (b - a).cross(Vector3.UP).normalized()
		for i in range(1, n):
			var k := float(i) / n
			var jitter := side * randf_range(-0.7, 0.7) + Vector3(0, randf_range(-0.5, 0.5), 0)
			pts.append(a.lerp(b, k) + jitter * sin(k * PI))
		pts.append(b)
		for i in pts.size() - 1:
			var p0: Vector3 = pts[i]
			var p1: Vector3 = pts[i + 1]
			var len := p0.distance_to(p1)
			var box := BoxMesh.new()
			box.size = Vector3(0.07 if pass_n == 0 else 0.18, 0.07 if pass_n == 0 else 0.18, len)
			var seg := _mesh(box, _mat(Color.WHITE if pass_n == 0 else Color(c.r, c.g, c.b, 0.35), 3.0 if pass_n == 0 else 1.2), (p0 + p1) * 0.5)
			if p0.distance_to(p1) > 0.001:
				seg.look_at_from_position((p0 + p1) * 0.5, p1, Vector3.UP if absf((p1 - p0).normalized().y) < 0.99 else Vector3.RIGHT)
			_fade(seg, 0.22 + pass_n * 0.08)

# ---------------------------------------------------------------- spells

## A spell was used. `caster` and `target` are the entities' nodes (target null when aimed at the caster);
## `info` is the action from /play/info. `landed` is called once it has landed (with where), for a sound.
func cast(caster: Node3D, target: Node3D, info: Dictionary, landed: Callable) -> void:
	var fx := String(info.get("fx", ""))
	if fx == "" or caster == null:
		return
	var c := color(fx)
	var hand := caster.position + Vector3(0, 1.4, 0)
	# A rune turning under the caster's feet, and a flare at the hand.
	var rune := _mesh(_ring(0.75, 0.82), _mat(Color(c.r, c.g, c.b, 0.8), 1.8), caster.position + Vector3(0, 0.08, 0))
	rune.scale = Vector3(1.0, 0.3, 1.0)
	_fade(rune, 0.7, 1.3, 2.5)
	var inner := _mesh(_ring(0.42, 0.46), _mat(Color(c.r, c.g, c.b, 0.6), 1.5), caster.position + Vector3(0, 0.09, 0))
	inner.scale = Vector3(1.0, 0.3, 1.0)
	_fade(inner, 0.7, 0.9, -3.5)
	_motes(hand, c, {"amount": 12, "life": 0.35, "speed": 2.5, "size": 0.25})
	var on: Node3D = target if target != null else caster
	var area := float(info.get("area", 0.0))
	var friendly := String(info.get("hits", "")) == "friends"
	if target != null and fx == "shock":
		_lightning(hand, target.position + Vector3(0, 1.1, 0), c)
		_land(on, fx, c, area, friendly, landed)
	elif bool(info.get("bolt", false)) and target != null:
		var core := _mesh(_sphere(0.16), _mat(Color.WHITE.lerp(c, 0.4), 3.0), hand)
		var halo := _mesh(_sphere(0.38), _mat(Color(c.r, c.g, c.b, 0.35), 1.5), Vector3.ZERO)
		halo.reparent(core, false)
		var l := OmniLight3D.new()
		l.light_color = c
		l.light_energy = 1.6
		l.omni_range = 5.0
		core.add_child(l)
		var trail := CPUParticles3D.new()
		trail.amount = 40
		trail.lifetime = 0.45
		trail.local_coords = false
		trail.direction = Vector3.UP
		trail.spread = 180.0
		trail.initial_velocity_min = 0.2
		trail.initial_velocity_max = 0.8
		trail.gravity = Vector3(0, 1.0, 0) if fx in ["fire", "holy", "arcane"] else Vector3(0, -2.0, 0)
		trail.scale_amount_min = 0.5
		trail.scale_amount_max = 1.0
		var q := QuadMesh.new()
		q.size = Vector2(0.32, 0.32)
		q.material = _mat(c, 1.3, fx != "blood" and fx != "shadow", true)
		trail.mesh = q
		var ramp := Gradient.new()
		ramp.set_color(0, Color(1, 1, 1, 0.9))
		ramp.set_color(1, Color(1, 1, 1, 0))
		trail.color_ramp = ramp
		core.add_child(trail)
		var life := maxf(0.15, hand.distance_to(target.position + Vector3(0, 1.1, 0)) / BOLT_SPEED)
		_bolts.append({"node": core, "trail": trail, "from": hand, "to": target, "t": 0.0, "life": life,
			"then": func(): _land(on, fx, c, area, friendly, landed)})
	else:
		_land(on, fx, c, area, friendly, landed)

func _land(on: Node3D, fx: String, c: Color, area: float, friendly: bool, landed: Callable) -> void:
	if not is_instance_valid(on):
		return
	var feet := on.position + Vector3(0, 0.1, 0)
	var at := on.position + Vector3(0, 1.0, 0)
	if friendly:
		# A soft column of light, motes rising round it, a ring on the ground.
		var col := CylinderMesh.new()
		col.top_radius = 0.55
		col.bottom_radius = 0.75
		col.height = 4.0
		col.radial_segments = 20
		col.cap_top = false
		col.cap_bottom = false
		var beam := _mesh(col, _mat(Color(c.r, c.g, c.b, 0.28), 1.4), on.position + Vector3(0, 2.0, 0))
		_fade(beam, 1.1, 0.0, 0.0, 0.0)
		_motes(feet, c, {"amount": 30, "life": 1.3, "speed": 2.6, "spread": 15.0, "ring": 0.7, "gravity": Vector3(0, 1.2, 0), "explode": 0.2, "size": 0.28})
		var r := _mesh(_ring(0.9, 1.0), _mat(Color(c.r, c.g, c.b, 0.8), 1.6), feet)
		r.scale = Vector3(0.6, 0.3, 0.6)
		_fade(r, 0.9, 1.4)
		_light(at, c, 1.2, 6.0, 0.8)
	else:
		_impact(fx, at, feet, c)
	if area > 0.0:
		var ring := _mesh(_ring(0.93, 1.0), _mat(Color(c.r, c.g, c.b, 0.9), 2.2), on.position + Vector3(0, 0.15, 0))
		ring.scale = Vector3(0.5, 0.4, 0.5)
		_fade(ring, 0.6, area)
		var disc := CylinderMesh.new()
		disc.top_radius = 1.0
		disc.bottom_radius = 1.0
		disc.height = 0.02
		disc.radial_segments = 40
		var flash := _mesh(disc, _mat(Color(c.r, c.g, c.b, 0.22), 1.0), on.position + Vector3(0, 0.12, 0))
		flash.scale = Vector3(0.5, 1.0, 0.5)
		_fade(flash, 0.5, area)
		_motes(on.position + Vector3(0, 0.3, 0), c, {"amount": int(clampf(area * 6.0, 16, 60)), "life": 0.7, "speed": 1.5, "ring": area * 0.8, "gravity": Vector3(0, 2.0, 0), "explode": 0.8})
	landed.call(on.position)

## Where a harmful spell lands: each look its own way.
func _impact(fx: String, at: Vector3, feet: Vector3, c: Color) -> void:
	_light(at, c, 2.6, 8.0, 0.5)
	match fx:
		"fire":
			_motes(at, c, {"amount": 34, "speed": 7.0, "life": 0.5, "size": 0.4})
			_motes(at, Color("ffc04a"), {"amount": 24, "speed": 2.0, "life": 1.4, "spread": 40.0, "gravity": Vector3(0, 2.2, 0), "explode": 0.3, "size": 0.16, "radius": 0.4})
			_motes(at, Color(0.12, 0.1, 0.1, 0.55), {"amount": 10, "speed": 1.2, "life": 1.8, "spread": 30.0, "gravity": Vector3(0, 1.0, 0), "glow": false, "size": 1.2, "explode": 0.5})
		"frost":
			var shard := BoxMesh.new()
			shard.size = Vector3(0.05, 0.05, 0.35)
			_motes(at, c, {"amount": 26, "speed": 8.0, "life": 0.6, "mesh": shard, "gravity": Vector3(0, -9.0, 0)})
			_motes(feet, Color(0.85, 0.95, 1.0, 0.6), {"amount": 16, "speed": 1.2, "life": 1.2, "ring": 0.8, "gravity": Vector3(0, 0.3, 0), "explode": 0.6, "size": 0.7})
		"shock":
			_motes(at, Color.WHITE, {"amount": 30, "speed": 12.0, "life": 0.25, "size": 0.18})
			_motes(at, c, {"amount": 14, "speed": 4.0, "life": 0.5, "size": 0.35})
		"holy":
			var col := CylinderMesh.new()
			col.top_radius = 0.35
			col.bottom_radius = 0.6
			col.height = 7.0
			col.cap_top = false
			col.cap_bottom = false
			var beam := _mesh(col, _mat(Color(c.r, c.g, c.b, 0.5), 2.2), feet + Vector3(0, 3.5, 0))
			_fade(beam, 0.6)
			_motes(feet + Vector3(0, 5.0, 0), c, {"amount": 24, "speed": 3.0, "life": 0.9, "dir": Vector3.DOWN, "spread": 12.0, "gravity": Vector3(0, -6.0, 0), "explode": 0.4, "size": 0.25})
		"shadow":
			_motes(at, Color(0.08, 0.02, 0.12, 0.8), {"amount": 26, "speed": 3.5, "life": 0.8, "glow": false, "size": 0.6, "radius": 1.4, "gravity": Vector3.ZERO, "damp": 6.0})
			_motes(at, c, {"amount": 18, "speed": 2.0, "life": 1.0, "radius": 1.0, "gravity": Vector3(0, 0.6, 0), "size": 0.25})
		"nature":
			var leaf := QuadMesh.new()
			leaf.size = Vector2(0.18, 0.1)
			_motes(at, c, {"amount": 26, "speed": 5.0, "life": 1.3, "mesh": leaf, "gravity": Vector3(0, -2.0, 0), "glow": false})
			_motes(at, Color("c8ff8a"), {"amount": 14, "speed": 3.0, "life": 0.6, "size": 0.22})
		"arcane":
			for i in 3:
				var r := _mesh(_ring(0.9, 0.97), _mat(Color(c.r, c.g, c.b, 0.8), 2.0), at)
				r.rotation = Vector3(randf() * PI, randf() * PI, 0)
				r.scale = Vector3(0.2, 0.2, 0.2)
				_fade(r, 0.5 + i * 0.1, 1.6 + i * 0.4)
			_motes(at, Color.WHITE.lerp(c, 0.5), {"amount": 30, "speed": 6.0, "life": 0.6, "size": 0.2})
		"poison":
			_motes(feet + Vector3(0, 0.6, 0), Color(c.r * 0.6, c.g * 0.8, c.b * 0.3, 0.45), {"amount": 14, "speed": 0.8, "life": 2.4, "radius": 1.2, "gravity": Vector3(0, 0.35, 0), "glow": false, "size": 1.5, "explode": 0.4})
			_motes(at, c, {"amount": 18, "speed": 2.5, "life": 1.0, "gravity": Vector3(0, 1.0, 0), "size": 0.2, "explode": 0.3})
		"mind":
			for i in 3:
				var r := _mesh(_ring(0.9, 0.96), _mat(Color(c.r, c.g, c.b, 0.85), 2.0), feet + Vector3(0, 1.8 + i * 0.35, 0))
				r.scale = Vector3(0.3, 0.3, 0.3)
				_fade(r, 0.7 + i * 0.15, 1.2 - i * 0.2, 4.0, 1.0)
			_motes(at, c, {"amount": 16, "speed": 2.0, "life": 0.8, "size": 0.25})
		"blood":
			_motes(at, Color(0.55, 0.02, 0.06, 0.95), {"amount": 30, "speed": 5.5, "life": 0.8, "gravity": Vector3(0, -14.0, 0), "glow": false, "size": 0.16, "spread": 70.0})
			var disc := CylinderMesh.new()
			disc.top_radius = 0.7
			disc.bottom_radius = 0.7
			disc.height = 0.01
			var splat := _mesh(disc, _mat(Color(0.35, 0.0, 0.03, 0.7), 1.0, false), feet + Vector3(0, -0.06, 0))
			splat.scale = Vector3(0.3, 1.0, 0.3)
			_fade(splat, 3.0, 1.0)
		_:
			_motes(at, c, {"amount": 26, "speed": 6.0})

# ---------------------------------------------------------------- states

## Who is held (entity id -> hold kind 1 mez, 2 stun, 3 root), levitating and hasted now, with their nodes: a
## mark on each, gone when it ends.
func show_states(held: Dictionary, floating: Dictionary, hasted: Dictionary, nodes: Dictionary) -> void:
	var want := {}
	for id in held:
		want["held%d:%d" % [int(held[id]), id]] = id
	for id in floating:
		want["float:%d" % id] = id
	for id in hasted:
		want["haste:%d" % id] = id
	for k in _marks.keys():
		if not want.has(k) or not nodes.has(want[k]):
			(_marks[k] as Node3D).queue_free()
			_marks.erase(k)
	for k in want:
		var id: int = want[k]
		if not nodes.has(id):
			continue
		if not _marks.has(k):
			_marks[k] = _make_mark(String(k).get_slice(":", 0))
		var n: Node3D = nodes[id]
		var mark: Node3D = _marks[k]
		mark.position = n.position

## The old name: only who is held.
func show_held(held: Dictionary, nodes: Dictionary) -> void:
	show_states(held, {}, {}, nodes)

func _make_mark(kind: String) -> Node3D:
	var root := Node3D.new()
	add_child(root)
	match kind:
		"held1", "held2":
			# Mez: slow pink motes circling over the head. Stun: little gold stars spinning fast.
			var c: Color = HELD_COLORS[1 if kind == "held1" else 2]
			var spin := Node3D.new()
			spin.name = "spin"
			spin.position = Vector3(0, 2.4, 0)
			spin.set_meta("rate", 2.0 if kind == "held1" else 7.0)
			root.add_child(spin)
			for i in (3 if kind == "held1" else 5):
				var o := MeshInstance3D.new()
				o.mesh = _sphere(0.08 if kind == "held1" else 0.06)
				o.material_override = _mat(c, 2.2)
				o.position = Vector3(cos(i * TAU / 3.0), 0.0, sin(i * TAU / 3.0)) * 0.45 if kind == "held1" else Vector3(cos(i * TAU / 5.0), 0.0, sin(i * TAU / 5.0)) * 0.35
				spin.add_child(o)
		"held3":
			# Rooted: green thorns round the feet.
			for i in 7:
				var cone := CylinderMesh.new()
				cone.top_radius = 0.0
				cone.bottom_radius = 0.07
				cone.height = 0.6
				var t := MeshInstance3D.new()
				t.mesh = cone
				t.material_override = _mat(HELD_COLORS[3], 1.2)
				var a := i * TAU / 7.0
				t.position = Vector3(cos(a), 0.25, sin(a)) * Vector3(0.55, 1.0, 0.55)
				t.rotation = Vector3(sin(a) * 0.4, 0, -cos(a) * 0.4)
				root.add_child(t)
		"float":
			# Levitating: a soft glowing disc below, motes drifting down from the feet.
			var disc := CylinderMesh.new()
			disc.top_radius = 0.8
			disc.bottom_radius = 0.8
			disc.height = 0.02
			disc.radial_segments = 32
			var d := MeshInstance3D.new()
			d.mesh = disc
			d.material_override = _mat(Color(COLORS["arcane"].r, COLORS["arcane"].g, COLORS["arcane"].b, 0.25), 1.2)
			d.position = Vector3(0, 0.08, 0)
			root.add_child(d)
			root.add_child(_stream(COLORS["arcane"], Vector3(0, 1.0, 0), Vector3(0, -1.5, 0), 0.5))
		"haste":
			# Hasted: green wisps streaming off the heels.
			root.add_child(_stream(COLORS["nature"], Vector3(0, 0.3, 0), Vector3(0, 0.8, 0), 0.35))
	return root

## Motes that keep coming while a state lasts (attached to its mark).
func _stream(c: Color, at: Vector3, gravity: Vector3, size: float) -> CPUParticles3D:
	var p := CPUParticles3D.new()
	p.amount = 18
	p.lifetime = 0.8
	p.local_coords = false
	p.emission_shape = CPUParticles3D.EMISSION_SHAPE_SPHERE
	p.emission_sphere_radius = 0.4
	p.direction = Vector3.UP
	p.spread = 180.0
	p.initial_velocity_min = 0.1
	p.initial_velocity_max = 0.5
	p.gravity = gravity
	var q := QuadMesh.new()
	q.size = Vector2(size, size) * 0.6
	q.material = _mat(c, 1.3, true, true)
	p.mesh = q
	var ramp := Gradient.new()
	ramp.set_color(0, Color(1, 1, 1, 0.9))
	ramp.set_color(1, Color(1, 1, 1, 0))
	p.color_ramp = ramp
	p.position = at
	return p

## Draw one of everything once, at `at`, so the shaders are compiled before anyone sees a spell (the first one
## would otherwise stall the frame). Called under the loading screen.
func prewarm(at: Vector3) -> void:
	var me := Node3D.new()
	me.position = at
	add_child(me)
	for fx in COLORS:
		_impact(fx, at, at, COLORS[fx])
	cast(me, me, {"fx": "fire", "bolt": true, "area": 4.0}, func(_p): pass)
	cast(me, me, {"fx": "shock", "area": 0.0}, func(_p): pass)
	_land(me, "holy", COLORS["holy"], 4.0, true, func(_p): pass)
	show_states({-1: 1, -2: 2, -3: 3}, {-4: true}, {-5: true}, {-1: me, -2: me, -3: me, -4: me, -5: me})
	await get_tree().create_timer(1.0).timeout
	show_states({}, {}, {}, {})
	me.queue_free()

func _process(delta: float) -> void:
	for k in _marks:
		var spin := (_marks[k] as Node3D).get_node_or_null("spin")
		if spin:
			(spin as Node3D).rotate_y(delta * float(spin.get_meta("rate", 2.0)))
	var i := 0
	while i < _bolts.size():
		var b: Dictionary = _bolts[i]
		b["t"] += delta
		var to: Node3D = b["to"]
		var k := clampf(b["t"] / b["life"], 0.0, 1.0)
		var node: Node3D = b["node"]
		if is_instance_valid(to):
			var end := to.position + Vector3(0, 1.1, 0)
			node.position = (b["from"] as Vector3).lerp(end, k) + Vector3(0, sin(k * PI) * 0.6, 0)
		if k >= 1.0 or not is_instance_valid(to):
			# The trail stays a moment where it was, so it fades rather than vanishing.
			var trail: CPUParticles3D = b["trail"]
			trail.emitting = false
			trail.reparent(self)
			_fading.append({"node": trail, "t": 0.0, "life": trail.lifetime + 0.1, "grow": 0.0, "spin": 0.0, "rise": 0.0})
			node.queue_free()
			_bolts.remove_at(i)
			if is_instance_valid(to):
				(b["then"] as Callable).call()
			continue
		i += 1
	i = 0
	while i < _fading.size():
		var f: Dictionary = _fading[i]
		f["t"] += delta
		var k := clampf(f["t"] / f["life"], 0.0, 1.0)
		var node: Node3D = f["node"]
		if not is_instance_valid(node):
			_fading.remove_at(i)
			continue
		if f.get("grow", 0.0) > 0.0:
			var base: Vector3 = f["base"]
			var r := lerpf(1.0, float(f["grow"]) / maxf(base.x, 0.01), 1.0 - pow(1.0 - k, 3.0))
			node.scale = Vector3(base.x * r, base.y, base.z * r)
		if f.get("spin", 0.0) != 0.0:
			node.rotate_y(delta * float(f["spin"]))
		if f.get("rise", 0.0) != 0.0:
			node.position.y += delta * float(f["rise"])
		if node is MeshInstance3D and (node as MeshInstance3D).material_override is StandardMaterial3D:
			var m := (node as MeshInstance3D).material_override as StandardMaterial3D
			m.albedo_color.a = float(f.get("alpha", 1.0)) * (1.0 - k)
		if f.has("light"):
			(node as OmniLight3D).light_energy = float(f["light"]) * (1.0 - k)
		if k >= 1.0:
			node.queue_free()
			_fading.remove_at(i)
			continue
		i += 1

extends Node3D
## A beast the server moves (a dog at its master's heel, a cow that strayed): low-poly boxes, legs that swing with
## its speed, a head that drops when it stands. It answers what main.gd asks of a character (set_speed, set_dead,
## hurt, emote), so it moves and is struck like one.

var _legs: Array[Node3D] = []
var _head: Node3D
var _tail: Node3D
var _speed := 0.0
var _phase := 0.0
var _dead := false
var _hurt := 0.0
var _wag := 0.0

static func make(kind: String) -> Node3D:
	var b: Node3D = (load("res://scripts/beast.gd") as GDScript).new()
	b.call("_build", kind)
	return b

func _mat(c: Color) -> StandardMaterial3D:
	var m := StandardMaterial3D.new()
	m.albedo_color = c
	m.roughness = 0.95
	return m

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

## Four legs under the body, each hung from its hip so it can swing.
func _four(w: float, l: float, hip: float, thick: float, mat: Material) -> void:
	for c in [Vector2(-w, -l), Vector2(w, -l), Vector2(-w, l), Vector2(w, l)]:
		var pivot := Node3D.new()
		pivot.position = Vector3(c.x, hip, c.y)
		add_child(pivot)
		_box(pivot, Vector3(thick, hip, thick), Vector3(0, -hip / 2.0, 0), mat)
		_legs.append(pivot)

func _build(kind: String) -> void:
	var seed := absi(hash(str(get_instance_id())))
	_head = Node3D.new()
	_tail = Node3D.new()
	if kind == "cow":
		var coats := [Color("6b3f22"), Color("2a2522"), Color("c9b9a0")]
		var coat := _mat(coats[seed % coats.size()])
		_box(self, Vector3(0.62, 0.62, 1.35), Vector3(0, 1.0, 0), coat)
		_box(self, Vector3(0.64, 0.4, 0.4), Vector3(0, 1.08, 0.2), _mat(Color("ece6da")))
		_head.position = Vector3(0, 1.05, -0.62)
		add_child(_head)
		_box(_head, Vector3(0.38, 0.4, 0.5), Vector3(0, 0.05, -0.2), coat)
		_box(_head, Vector3(0.3, 0.22, 0.14), Vector3(0, -0.08, -0.48), _mat(Color("caa08a")))
		for s in [-1.0, 1.0]:
			_box(_head, Vector3(0.05, 0.05, 0.22), Vector3(s * 0.22, 0.24, -0.16), _mat(Color("e8dcc0")), Vector3(0, s * 0.9, 0))
		_tail.position = Vector3(0, 1.2, 0.68)
		add_child(_tail)
		_box(_tail, Vector3(0.06, 0.6, 0.06), Vector3(0, -0.3, 0), coat)
		_four(0.22, 0.5, 0.7, 0.13, coat)
	else:
		# A rough-coated dog: a wolfhound's grey, a collie's black and white, a terrier's tan.
		var coats := [Color("8b8478"), Color("2b2622"), Color("a8794a"), Color("d8cfc0")]
		var coat := _mat(coats[seed % coats.size()])
		var dark := _mat(Color("1d1916"))
		_box(self, Vector3(0.3, 0.32, 0.8), Vector3(0, 0.55, 0), coat)
		_head.position = Vector3(0, 0.68, -0.42)
		add_child(_head)
		_box(_head, Vector3(0.24, 0.24, 0.28), Vector3(0, 0.08, -0.08), coat)
		_box(_head, Vector3(0.13, 0.12, 0.18), Vector3(0, 0.0, -0.28), coat)
		_box(_head, Vector3(0.06, 0.05, 0.05), Vector3(0, 0.04, -0.38), dark)
		for s in [-1.0, 1.0]:
			_box(_head, Vector3(0.06, 0.12, 0.04), Vector3(s * 0.09, 0.24, -0.04), coat, Vector3(0, 0, s * 0.3))
		_tail.position = Vector3(0, 0.66, 0.4)
		add_child(_tail)
		_box(_tail, Vector3(0.06, 0.06, 0.32), Vector3(0, 0.06, 0.14), coat, Vector3(0.5, 0, 0))
		_four(0.1, 0.3, 0.42, 0.08, coat)

func set_speed(v: float) -> void:
	_speed = v

func set_dead(d: bool) -> void:
	if d == _dead:
		return
	_dead = d
	rotation.z = PI / 2.0 if d else 0.0
	position.y += 0.25 if d else 0.0

func hurt() -> void:
	_hurt = 0.3

func emote(_anim: String) -> void:
	_wag = 2.0 # a dog's answer to anything: a wag

func _process(delta: float) -> void:
	if _dead:
		return
	var stride := clampf(_speed / 4.0, 0.0, 1.5)
	_phase += delta * (3.0 + stride * 9.0) * minf(stride * 3.0, 1.0)
	for k in _legs.size():
		var sw := sin(_phase + (PI if k == 1 or k == 2 else 0.0)) * 0.6 * minf(stride, 1.0)
		_legs[k].rotation.x = sw
	# Head down to sniff or graze when it stands; up when it goes.
	_head.rotation.x = lerpf(_head.rotation.x, -0.5 if stride < 0.1 else 0.0, minf(1.0, delta * 3.0))
	_wag = maxf(0.0, _wag - delta)
	var wag := 1.0 if _wag > 0.0 or stride > 0.2 else 0.25
	_tail.rotation.y = sin(Time.get_ticks_msec() * 0.018) * 0.6 * wag
	if _hurt > 0.0:
		_hurt -= delta
		scale = Vector3.ONE * (1.0 + 0.08 * sin(_hurt * 40.0))
	else:
		scale = Vector3.ONE

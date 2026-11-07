extends Node3D
## A beast drawn from Quaternius's animated animals (themes/fantasy/assets/animals, CC0): a deer, a stag, a wolf,
## a fox, a dog, a cow or a horse, scaled to its kind and playing its clips by how fast it goes: idle (now and then
## grazing or looking about), walk, gallop, a start when struck, a bite or butt when it attacks, and its death. It
## answers what main.gd asks of a character (set_speed, set_dead, hurt, attack, emote), like beast.gd's boxes, which
## stay the fallback when the model is missing.

const DIR := "res://themes/fantasy/assets/animals/"
## kind -> [file, length nose to tail in world units, coat tint (white = as made), walk speed of the clip, gallop's]
const KINDS := {
	"deer": ["deer.glb", 2.3, Color.WHITE, 1.6, 6.0],
	"stag": ["stag.glb", 2.6, Color.WHITE, 1.6, 6.0],
	"wolf": ["wolf.glb", 2.2, Color.WHITE, 1.5, 6.5],
	"wolf_white": ["wolf.glb", 2.3, Color(1.6, 1.62, 1.66), 1.5, 6.5],
	"fox": ["fox.glb", 1.4, Color.WHITE, 1.2, 5.0],
	"dog": ["husky.glb", 1.6, Color.WHITE, 1.4, 5.5],
	"cow": ["cow.glb", 2.7, Color.WHITE, 1.2, 4.5],
	"horse": ["horse.glb", 2.9, Color.WHITE, 1.8, 7.0],
}

var _anim: AnimationPlayer
var _speed := 0.0
var _dead := false
var _busy := 0.0          # a one-off clip (struck, attack) playing: hold the loop until it is done
var _idle_left := 0.0
var _idle_clip := "Idle"
var _walk_at := 1.5
var _gallop_at := 6.0
var _rng := RandomNumberGenerator.new()

static var _scenes := {}

static func can_draw(kind: String) -> bool:
	return KINDS.has(kind) and ResourceLoader.exists(DIR + String(KINDS[kind][0]))

static func make(kind: String) -> Node3D:
	var a: Node3D = (load("res://scripts/animal.gd") as GDScript).new()
	a.call("_build", kind)
	return a

func _build(kind: String) -> void:
	var k: Array = KINDS[kind]
	var path := DIR + String(k[0])
	if not _scenes.has(path):
		_scenes[path] = load(path)
	var body: Node3D = (_scenes[path] as PackedScene).instantiate()
	add_child(body)
	_walk_at = float(k[3])
	_gallop_at = float(k[4])
	_rng.seed = absi(hash(str(get_instance_id())))
	# To its kind's size: the longest way across its meshes, as it stands.
	var box := _bounds(body, Transform3D.IDENTITY)
	var long := maxf(box.size.x, box.size.z)
	if long > 0.01:
		var s := float(k[1]) / long * (1.0 + _rng.randf_range(-0.07, 0.07))
		body.scale = Vector3.ONE * s
		body.position.y = -box.position.y * s
	var tint: Color = k[2]
	if tint != Color.WHITE:
		for m in body.find_children("*", "MeshInstance3D", true, false):
			var mi := m as MeshInstance3D
			for i in mi.mesh.get_surface_count():
				var src := mi.mesh.surface_get_material(i) as BaseMaterial3D
				if src:
					var mat := src.duplicate() as BaseMaterial3D
					mat.albedo_color = Color(minf(1.0, src.albedo_color.r * tint.r), minf(1.0, src.albedo_color.g * tint.g), minf(1.0, src.albedo_color.b * tint.b))
					mi.set_surface_override_material(i, mat)
	for m in body.find_children("*", "MeshInstance3D", true, false):
		(m as MeshInstance3D).cast_shadow = GeometryInstance3D.SHADOW_CASTING_SETTING_ON
	var found := body.find_children("*", "AnimationPlayer", true, false)
	if not found.is_empty():
		_anim = found[0]
		for n in ["Idle", "Idle_2", "Idle_Headlow", "Idle_2_HeadLow", "Eating", "Walk", "Gallop"]:
			var clip := _clip(n)
			if clip != "":
				_anim.get_animation(clip).loop_mode = Animation.LOOP_LINEAR
		_anim.animation_finished.connect(func(_n): _busy = 0.0)
		_play("Idle", 0.0)
		# Not all in step: a herd starts its idles at different points.
		if _anim.current_animation != "":
			_anim.seek(_rng.randf() * _anim.current_animation_length, true)

## The bounds of every mesh under `n`, in this node's space.
static func _bounds(n: Node, xf: Transform3D) -> AABB:
	var out := AABB()
	var first := true
	var here := xf * ((n as Node3D).transform if n is Node3D else Transform3D.IDENTITY)
	if n is MeshInstance3D and (n as MeshInstance3D).mesh:
		out = here * (n as MeshInstance3D).mesh.get_aabb()
		first = false
	for c in n.get_children():
		var b := _bounds(c, here)
		if b.size == Vector3.ZERO:
			continue
		out = b if first else out.merge(b)
		first = false
	return out

## A clip by its short name: the packs name them "Walk" and "AnimalArmature|Walk"; take the short one.
func _clip(n: String) -> String:
	if _anim == null:
		return ""
	if _anim.has_animation(n):
		return n
	if _anim.has_animation("AnimalArmature|" + n):
		return "AnimalArmature|" + n
	return ""

func _play(n: String, blend := 0.25, speed := 1.0) -> void:
	var clip := _clip(n)
	if clip == "":
		return
	if _anim.current_animation != clip:
		_anim.play(clip, blend, speed)
	else:
		_anim.speed_scale = speed

func set_speed(v: float) -> void:
	_speed = v

func set_dead(d: bool) -> void:
	if d == _dead:
		return
	_dead = d
	if _anim == null:
		rotation.z = PI / 2.0 if d else 0.0
		return
	if d:
		var clip := _clip("Death")
		if clip != "":
			_anim.get_animation(clip).loop_mode = Animation.LOOP_NONE
			_anim.play(clip, 0.15)
	else:
		_play("Idle", 0.3)

func hurt() -> void:
	if _dead or _anim == null:
		return
	var n := "Idle_HitReact_Left" if _rng.randf() < 0.5 else "Idle_HitReact_Right"
	if _clip(n) != "" and _speed < 1.0:
		_anim.play(_clip(n), 0.1)
		_busy = 0.6

func attack() -> void:
	if _dead or _anim == null:
		return
	for n in ["Attack", "Attack_Headbutt", "Attack_Kick"]:
		if _clip(n) != "":
			_anim.play(_clip(n), 0.1)
			_busy = 0.9
			return

func emote(_anim_name: String) -> void:
	if _dead or _anim == null:
		return
	if _clip("Jump_ToIdle") != "" or _clip("Jump_toIdle") != "":
		_anim.play(_clip("Jump_ToIdle") if _clip("Jump_ToIdle") != "" else _clip("Jump_toIdle"), 0.1)
		_busy = 1.0

func _process(delta: float) -> void:
	if _dead or _anim == null:
		return
	if _busy > 0.0:
		_busy -= delta
		return
	if _speed > (_walk_at + _gallop_at) * 0.5:
		_play("Gallop", 0.25, clampf(_speed / _gallop_at, 0.6, 1.8))
	elif _speed > 0.35:
		_play("Walk", 0.25, clampf(_speed / _walk_at, 0.5, 2.2))
	else:
		# Standing: mostly still, now and then head down to graze or a look about.
		_idle_left -= delta
		if _idle_left <= 0.0:
			var r := _rng.randf()
			_idle_clip = "Eating" if r < 0.3 else ("Idle_2" if r < 0.5 else "Idle")
			if _clip(_idle_clip) == "":
				_idle_clip = "Idle"
			_idle_left = _rng.randf_range(4.0, 10.0)
		_play(_idle_clip, 0.4)

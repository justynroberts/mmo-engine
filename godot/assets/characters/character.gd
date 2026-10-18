extends Node3D
## A KayKit character driven by what the server tells us: idle, walk, run, attack, hit, death.
## main.gd calls set_speed() every frame and attack(), hurt(), set_dead() as things happen. The model's own
## animations are used, plus those of `extra_animations` (glTF files on the same rig, like KayKit's Rig_Medium packs).

@export var model: PackedScene
@export var model_scale := 1.0
@export var extra_animations: Array[PackedScene] = []
## An attack borrowed from another rig with the same bone names (the free Skeletons pack has no melee attack).
## Only its rotation tracks are kept, so differences in bone length do not stretch the model.
@export var borrowed_from: PackedScene
@export var borrowed_name := ""
@export var idle := "Idle"
@export var walk := "Walking_A"
@export var run := "Running_A"
@export var attack_name := "1H_Melee_Attack_Chop"
@export var hit := "Hit_A"
@export var death := "Death_A"
@export var stand_up := ""
@export var jump_name := "Jump_Full_Short"
## Ground speed, in units a second, that each cycle was authored for, so feet slide as little as possible.
@export var walk_speed := 2.5
@export var run_speed := 6.5
@export var hide_meshes: PackedStringArray = []
@export var attachments: Array[PackedScene] = []
@export var attachment_bones: PackedStringArray = []

var anim: AnimationPlayer
var speed := 0.0
var dead := false
var busy := 0.0 # seconds left of a one-shot (attack, hit, stand up)
var state := ""

static var _libs := {} # scene path -> AnimationLibrary

func _ready() -> void:
	var m := model.instantiate()
	m.scale = Vector3.ONE * model_scale
	add_child(m)
	for n in hide_meshes:
		var h := m.find_child(n, true, false)
		if h:
			h.visible = false
	var skel := m.find_child("Skeleton3D", true, false) as Skeleton3D
	if skel:
		for i in mini(attachments.size(), attachment_bones.size()):
			var att := BoneAttachment3D.new()
			att.bone_name = attachment_bones[i]
			skel.add_child(att)
			att.add_child(attachments[i].instantiate())
	anim = m.find_child("AnimationPlayer", true, false) as AnimationPlayer
	if anim == null:
		anim = AnimationPlayer.new()
		m.add_child(anim)
	# Every mob shares one merged library, built the first time this scene is used.
	var key := scene_file_path
	if not _libs.has(key):
		var lib: AnimationLibrary = anim.get_animation_library("") if anim.has_animation_library("") else AnimationLibrary.new()
		for s in extra_animations:
			_merge(lib, s)
		if borrowed_from and skel:
			_borrow(lib, m.get_path_to(skel))
		for n in [idle, walk, run]:
			if lib.has_animation(n):
				lib.get_animation(n).loop_mode = Animation.LOOP_LINEAR
		_libs[key] = lib
	if anim.has_animation_library(""):
		anim.remove_animation_library("")
	anim.add_animation_library("", _libs[key])
	_play(idle)

func _merge(lib: AnimationLibrary, s: PackedScene) -> void:
	var inst := s.instantiate()
	var ap := inst.find_child("AnimationPlayer", true, false) as AnimationPlayer
	if ap:
		for l in ap.get_animation_library_list():
			var src := ap.get_animation_library(l)
			for a in src.get_animation_list():
				if not lib.has_animation(a):
					lib.add_animation(a, src.get_animation(a))
	inst.free()

func _borrow(lib: AnimationLibrary, skel_path: NodePath) -> void:
	var inst := borrowed_from.instantiate()
	var ap := inst.find_child("AnimationPlayer", true, false) as AnimationPlayer
	if ap and ap.has_animation(borrowed_name):
		var a := ap.get_animation(borrowed_name).duplicate() as Animation
		for t in range(a.get_track_count() - 1, -1, -1):
			var p := a.track_get_path(t)
			if a.track_get_type(t) != Animation.TYPE_ROTATION_3D or p.get_subname_count() == 0:
				a.remove_track(t)
			else:
				a.track_set_path(t, NodePath("%s:%s" % [skel_path, p.get_subname(0)]))
		lib.add_animation(attack_name, a)
	inst.free()

func _play(n: String, blend := 0.15, scale := 1.0) -> void:
	if anim == null or not anim.has_animation(n):
		return
	anim.speed_scale = scale
	if anim.current_animation == n:
		return
	anim.play(n, blend)

func _one_shot(n: String, scale := 1.0) -> void:
	if anim == null or not anim.has_animation(n):
		return
	anim.speed_scale = scale
	anim.play(n, 0.08)
	anim.seek(0.0, true)
	busy = anim.get_animation(n).length / scale
	state = n

func set_speed(s: float) -> void:
	speed = s

func attack() -> void:
	if not dead:
		_one_shot(attack_name, 1.4)

## Take-off: the arc itself comes from the server (main.gd lifts the node); this is the legs.
func jump(seconds: float) -> void:
	if not dead and anim and anim.has_animation(jump_name):
		_one_shot(jump_name, anim.get_animation(jump_name).length / maxf(seconds, 0.1))

func hurt() -> void:
	if not dead and busy <= 0.0:
		_one_shot(hit, 1.3)

func set_dead(d: bool) -> void:
	if d == dead:
		return
	dead = d
	if d:
		busy = 0.0
		_play(death, 0.1)
		state = death
	elif stand_up != "":
		_one_shot(stand_up, 1.6)
	else:
		_play(idle)

func _process(delta: float) -> void:
	if dead:
		return
	if busy > 0.0:
		busy -= delta
		return
	# A little hysteresis so a character on the walk/run border does not flicker between the two.
	var next := idle
	if speed > (5.0 if state == run else 7.0):
		next = run
	elif speed > (0.6 if state != idle else 1.0):
		next = walk
	state = next
	if next == run:
		_play(run, 0.15, clampf(speed / run_speed, 0.8, 2.0))
	elif next == walk:
		_play(walk, 0.15, clampf(speed / walk_speed, 0.8, 1.8))
	else:
		_play(idle, 0.2)

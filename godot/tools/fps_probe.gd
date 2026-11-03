extends SceneTree
## A bare scene (a camera, a light, a box) that prints its frame rate for a few seconds: what this machine and window
## manage with next to nothing to draw, to tell a slow game from a slow setup.
##   godot --path godot --script res://tools/fps_probe.gd

var _t := 0.0
var _frames := 0
var _report := 0.0

func _init() -> void:
	var cam := Camera3D.new()
	cam.position = Vector3(0, 1, 4)
	root.add_child(cam)
	var box := MeshInstance3D.new()
	box.mesh = BoxMesh.new()
	root.add_child(box)
	var sun := DirectionalLight3D.new()
	sun.rotation = Vector3(-0.8, 0.5, 0)
	root.add_child(sun)

func _process(delta: float) -> bool:
	_t += delta
	_frames += 1
	_report += delta
	if _report >= 1.0:
		print("PROBE fps %d" % _frames)
		_frames = 0
		_report = 0.0
	return _t > 6.0

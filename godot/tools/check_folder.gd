extends SceneTree
## Prints what the join screen would say of a folder the game ran from: "OK", or why it cannot update itself there.
##   godot --headless --path godot --script res://tools/check_folder.gd -- DIR

const Join := preload("res://scripts/join.gd")

func _init() -> void:
	var u := OS.get_cmdline_user_args()
	var why: String = Join.folder_trouble(u[0] if u.size() > 0 else OS.get_executable_path().get_base_dir())
	print("FOLDER ", why if why != "" else "OK")
	quit(0 if why == "" else 1)

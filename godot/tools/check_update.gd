extends SceneTree
## Prints what the join screen's updater would offer: the newest client for this platform from the server given
## (macOS) or the GitHub releases of the repo given, without the join screen. 
##   godot --headless --path godot --script res://tools/check_update.gd -- [--server HOST] [--repo owner/name] [--platform mac|windows]

const Join := preload("res://scripts/join.gd")

func _init() -> void:
	var args := {}
	var u := OS.get_cmdline_user_args()
	for i in range(0, u.size() - 1, 2):
		args[u[i].trim_prefix("--")] = u[i + 1]
	await process_frame
	var j = Join.new()
	root.add_child(j)
	await process_frame
	if args.has("server"):
		j.server_in.text = String(args["server"])
	else:
		j.server_in.text = ""
	if args.has("repo"):
		j.repo_override = String(args["repo"])
	if args.has("platform"):
		j.platform_override = String(args["platform"])
	var m: Dictionary = await j._newest()
	print("NEWEST ", JSON.stringify(m))
	quit(0 if not m.is_empty() else 1)

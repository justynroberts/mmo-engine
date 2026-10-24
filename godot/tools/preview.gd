extends SceneTree
## The fantasy theme's look at given points of a world file, with no server: for comparing what the theme draws.
##   godot --path godot --script res://tools/preview.gd -- --world FILE [--game game.json] --at X,Y[;X,Y...]
##       --out PREFIX [--zoom 34] [--time 0.3] [--pitch 0.85] [--yaw 0.6] [--size 1280x720] [--blocks DIR]
## --blocks draws what is built, from a server's <world>.blocks.d region files.
## Saves PREFIX_0.png, PREFIX_1.png ... one per point, and prints the settlement each point is in.
var a := {}

func _initialize() -> void:
	var u := OS.get_cmdline_user_args()
	var i := 0
	while i < u.size():
		var k := u[i].trim_prefix("--")
		if i + 1 < u.size() and not u[i + 1].begins_with("--"):
			a[k] = u[i + 1]
			i += 2
		else:
			a[k] = true
			i += 1
	_run.call_deferred()

func _run() -> void:
	var sz := String(a.get("size", "1280x720")).split("x")
	root.size = Vector2i(int(sz[0]), int(sz[1]))
	var world = load("res://scripts/world_data.gd").from_file(FileAccess.get_file_as_string(String(a["world"])))
	if world == null:
		push_error("could not read the world file")
		quit(1)
		return
	var info := {"entity_radius": 0.5, "locations": []}
	if a.has("game"):
		var g = JSON.parse_string(FileAccess.get_file_as_string(String(a["game"])))
		if g is Dictionary:
			for p in g.get("places", []):
				info["locations"].append({"title": p.get("name", ""), "x": p.get("x", 0.0), "y": p.get("y", 0.0), "r": p.get("r", 0.0)})
	var theme = load("res://themes/fantasy/theme.gd").new()
	root.add_child(theme)
	theme.setup(info, world)
	theme.time_override = float(a.get("time", 0.3))
	if a.has("blocks"):
		var all := {}
		for f in DirAccess.get_files_at(String(a["blocks"])):
			var r = JSON.parse_string(FileAccess.get_file_as_string(String(a["blocks"]).path_join(f)))
			if r is Dictionary:
				for c in r.get("cols", []):
					all[int(c[1]) << 15 | int(c[0])] = {"b": int(c[2]), "k": String(c[3])}
		theme.set_blocks(all, all.keys())
	var cam := Camera3D.new()
	cam.fov = 50.0
	root.add_child(cam)
	cam.current = true
	var zoom := float(a.get("zoom", 34.0))
	var pitch := float(a.get("pitch", 0.85))
	var yaw := float(a.get("yaw", 0.6))
	var points := String(a["at"]).split(";")
	for n in points.size():
		var c := Vector2(float(points[n].get_slice(",", 0)), float(points[n].get_slice(",", 1)))
		theme.update(c)
		var ground: float = theme.height(c.x, c.y)
		var target := Vector3(c.x, ground + 1.0, c.y)
		cam.position = target + Vector3(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch)) * zoom
		cam.look_at(target)
		# Build everything near before the picture: frame() builds a few cells a frame.
		for f in 600:
			theme.set_characters(target, [])
			theme.frame(c, zoom)
			await process_frame
			if theme._queue.is_empty() and f > 30:
				break
		for f in 20:
			theme.frame(c, zoom)
			await process_frame
		var out := "%s_%d.png" % [a["out"], n]
		root.get_texture().get_image().save_png(out)
		var town = theme.settlement_at(c) if theme.has_method("settlement_at") else {}
		print("PREVIEW %s at %s: %s" % [out, c, town.get("describe", "") if town is Dictionary else ""])
	quit()

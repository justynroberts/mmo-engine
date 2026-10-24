extends SceneTree
## How long one chunk of built blocks takes to mesh, rounded and plain, and how many triangles it makes:
##   godot --headless --path godot --script res://tools/bench_blocks.gd
func _init() -> void:
	var T = load("res://scripts/theme.gd")
	for case in [["town: walls 4 high", 4, 0.35], ["dense: solid 8 high", 8, 1.0], ["sparse towers", 6, 0.08]]:
		var th = T.new()
		th.info = {"build": {"block": 2.0}}
		var all := {}
		var cols := []
		var rng := RandomNumberGenerator.new()
		rng.seed = 7
		for cy in 64:
			for cx in 64:
				if rng.randf() < case[2]:
					var k := ""
					for z in case[1]:
						k += str(rng.randi() % 9)
					all[cy << 15 | cx] = {"b": 0, "k": k, "o": []}
					cols.append([cx, cy, 0, k])
		th._blocks = all
		th._warm_block_shapes()
		var look := {}
		for k in all:
			look[k] = th._look_of(0, all[k]["k"])
		var t0 := Time.get_ticks_usec()
		var flat = th._mesh_chunk({"cols": cols, "look": look, "bs": 2.0, "round": false})
		var flat_ms := (Time.get_ticks_usec() - t0) / 1000.0
		var flat_tris := 0
		for m in flat["meshes"].values():
			flat_tris += (m as Mesh).get_faces().size() / 3
		print("BENCH %s plain: %.0f ms, %d triangles" % [case[0], flat_ms, flat_tris])
		var t := Time.get_ticks_usec()
		var n = th.build_blocks(cols)
		var ms := (Time.get_ticks_usec() - t) / 1000.0
		var blocks := 0
		for c in cols:
			blocks += String(c[3]).length()
		var tris := 0
		for mi in n.get_children():
			if mi is MeshInstance3D:
				tris += (mi as MeshInstance3D).mesh.get_faces().size() / 3
		print("BENCH %s: %d blocks, %.0f ms, %d triangles" % [case[0], blocks, ms, tris])
		n.free()
		th.free()
	quit()

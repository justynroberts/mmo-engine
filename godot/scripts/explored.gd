extends RefCounted
## Fog of war: what this character has seen of the world, a grid of GRID x GRID cells over it, each 0 (never seen)
## to 255 (walked through), cleared in a soft circle round you as you go. The world map and the minimap lay `tex`
## over their picture: dark parchment where you have not been, clear where you have. Kept per server and
## character in user://explored, so the map fills in over a character's life.

const GRID := 1024
const SIGHT := 150.0           # world units cleared round you: less than the minimap's reach, so its rim shows the fog
const FOG := Color(0.07, 0.065, 0.055)
const FOG_ALPHA := 0.86        # how much of the picture the fog hides

var world := 1024.0
var img: Image                 # LA8: grey and alpha, the alpha is the fog
var tex: ImageTexture
var _path := ""
var _dirty := false
var _tex_due := false
var _save_in := 20.0
var _last := Vector2(-1e9, -1e9)
var _tex_in := 0.0

func setup(world_size: float, key: String) -> void:
	world = world_size
	_path = "user://explored/%s.png" % key.validate_filename()
	DirAccess.make_dir_recursive_absolute(ProjectSettings.globalize_path("user://explored"))
	var saved: Image = Image.load_from_file(ProjectSettings.globalize_path(_path)) if FileAccess.file_exists(_path) else null
	if saved != null and saved.get_width() == saved.get_height() and saved.get_width() >= 64:
		saved.convert(Image.FORMAT_LA8)
		if saved.get_width() != GRID:
			saved.resize(GRID, GRID, Image.INTERPOLATE_BILINEAR)
		img = saved
	else:
		img = Image.create(GRID, GRID, false, Image.FORMAT_LA8)
		img.fill(Color(FOG.r, FOG.g, FOG.b, FOG_ALPHA))
	tex = ImageTexture.create_from_image(img)

## How much of `p` has been seen, 0 to 1.
func seen(p: Vector2) -> float:
	if img == null:
		return 1.0
	var x := clampi(int(p.x / world * GRID), 0, GRID - 1)
	var y := clampi(int(p.y / world * GRID), 0, GRID - 1)
	return 1.0 - img.get_pixel(x, y).a / FOG_ALPHA

## Clear the fog round `p` (cheap to call every frame: it only works when we have moved a little).
func visit(p: Vector2) -> void:
	if img == null or p == Vector2.ZERO or p.distance_to(_last) < world / GRID * 0.5:
		return
	_last = p
	var cell := world / GRID
	var reach := int(ceil(SIGHT / cell))
	var cx := int(p.x / cell)
	var cy := int(p.y / cell)
	for y in range(maxi(0, cy - reach), mini(GRID, cy + reach + 1)):
		for x in range(maxi(0, cx - reach), mini(GRID, cx + reach + 1)):
			var d := Vector2((x + 0.5) * cell, (y + 0.5) * cell).distance_to(p)
			if d > SIGHT:
				continue
			# Clear in the middle, thinning to the edge of sight.
			var a := FOG_ALPHA * smoothstep(SIGHT * 0.5, SIGHT, d)
			var was := img.get_pixel(x, y).a
			if a < was - 0.01:
				img.set_pixel(x, y, Color(FOG.r, FOG.g, FOG.b, a))
				_dirty = true
				_tex_due = true

## Once a frame: hand the GPU what changed (at most a few times a second) and save now and then.
func tick(delta: float) -> void:
	_tex_in -= delta
	if _tex_due and tex != null and _tex_in <= 0.0:
		_tex_due = false
		_tex_in = 0.2
		tex.update(img)
	_save_in -= delta
	if _save_in <= 0.0:
		_save_in = 20.0
		save()

func save() -> void:
	if _dirty and img != null and _path != "":
		_dirty = false
		img.save_png(ProjectSettings.globalize_path(_path))

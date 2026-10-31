extends RefCounted
## Blueprints: small buildings laid out block by block, for build mode. Each is a list of cells (dx, dy across the
## ground from where it is placed, dz up from the ground under each column, or from the highest ground under it for a
## roof) and the material of each. main.gd shows the cells still missing as a ghost and builds them one at a time
## as you stand in reach; the server checks every block as it would one placed by hand (land, cost, support).

const STONE := 0
const TIMBER := 1
const PLASTER := 3
const ROOF := 6
const SOIL := 14

## [name, what it is for, cells: [[dx, dy, dz, material, roof], ...]]
static func all() -> Array:
	return [
		["Shelter", "a roof and three walls: night raiders pass you by", _shelter()],
		["Cottage", "four walls with a door and a tiled roof", _cottage()],
		["Field", "twelve beds of tilled soil to plant seeds beside", _field()],
		["Watchtower", "a stone tower with a timber floor on top", _tower()],
	]

static func _shelter() -> Array:
	var out := []
	for dz in 2:
		for d in [-1, 0, 1]:
			out.append([d, -1, dz, TIMBER, false]) # the back
		out.append([-1, 0, dz, TIMBER, false])
		out.append([1, 0, dz, TIMBER, false])
		out.append([-1, 1, dz, TIMBER, false])
		out.append([1, 1, dz, TIMBER, false])
	for dx in [-1, 0, 1]:
		for dy in [-1, 0, 1]:
			out.append([dx, dy, 2, ROOF, true])
	return out

static func _cottage() -> Array:
	var out := []
	for dz in 2:
		for dx in range(-2, 3):
			for dy in range(-1, 3):
				var edge := dx == -2 or dx == 2 or dy == -1 or dy == 2
				var door := dx == 0 and dy == 2
				if edge and not door:
					var corner := (dx == -2 or dx == 2) and (dy == -1 or dy == 2)
					out.append([dx, dy, dz, TIMBER if corner else PLASTER, false])
	for dx in range(-2, 3):
		for dy in range(-1, 3):
			out.append([dx, dy, 2, ROOF, true])
	return out

static func _field() -> Array:
	var out := []
	for dx in range(-2, 2):
		for dy in range(-1, 2):
			out.append([dx, dy, 0, SOIL, false])
	return out

static func _tower() -> Array:
	var out := []
	for dz in 4:
		for dx in [-1, 0, 1]:
			for dy in [-1, 0, 1]:
				var inside: bool = dx == 0 and dy == 0
				var door: bool = dx == 0 and dy == 1 and dz < 2
				if not inside and not door:
					out.append([dx, dy, dz, STONE, false])
	for dx in [-1, 0, 1]:
		for dy in [-1, 0, 1]:
			out.append([dx, dy, 4, TIMBER, true])
	return out

## A blueprint's cells turned a quarter `turns` times about where it is placed.
static func turned(cells: Array, turns: int) -> Array:
	var out := []
	for c in cells:
		var x: int = c[0]
		var y: int = c[1]
		for i in posmod(turns, 4):
			var t := x
			x = -y
			y = t
		out.append([x, y, c[2], c[3], c[4]])
	return out

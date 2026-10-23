extends Node3D
## The air between the weathers: mist lying in the hollows at dawn, thinner at dusk and by night, a haze that comes
## and goes over the minutes, and a few soft wisps drifting over the ground on the wind when it is misty. Nothing
## here is the server's weather; it only colours the clear spells between, and gives way as real weather comes in.
##
## It runs off the wall clock, so everyone playing at the same time sees much the same morning.

const WISPS := 5
const WISP_RANGE := 70.0     # wisps live within this of us, and a new one is laid upwind when one drifts out
var mist := 0.0              # 0 .. 1, how misty it is now (eased)
var _ground := 0.0           # the height the mist lies at, following us gently over the hills
var _wisps: Array[FogVolume] = []
var _mat: FogMaterial

func _ready() -> void:
	_mat = FogMaterial.new()
	_mat.density = 0.0
	_mat.albedo = Color("e6edf0")
	_mat.height_falloff = 0.35 # thick at the bottom, fading upward
	_mat.edge_fade = 0.6
	for i in WISPS:
		var v := FogVolume.new()
		v.shape = RenderingServer.FOG_VOLUME_SHAPE_ELLIPSOID
		v.size = Vector3(randf_range(26.0, 44.0), 5.0, randf_range(20.0, 34.0))
		v.material = _mat
		v.visible = false
		add_child(v)
		_wisps.append(v)

## Slow value noise in time: smooth, between 0 and 1, changing over `period` seconds.
static func _drift(t: float, period: float, seed: float) -> float:
	var x := t / period + seed
	var i := floorf(x)
	var f := x - i
	var a := fposmod(sin(i * 12.9898 + seed * 78.233) * 43758.5453, 1.0)
	var b := fposmod(sin((i + 1.0) * 12.9898 + seed * 78.233) * 43758.5453, 1.0)
	return lerpf(a, b, f * f * (3.0 - 2.0 * f))

## Every frame, after the weather has set the fog: add the air's own. `sun_y` is the sun's height (-1..1),
## `morning` whether it is still climbing, `day` and `dusk` how light and how low it is, `k` how much weather there
## is (0 clear), `wind` 0..1.
func frame(e: Environment, dt: float, center: Vector2, ground: float, sun_y: float, morning: bool, day: float, dusk: float, k: float, wind: float) -> void:
	var t := Time.get_unix_time_from_system()
	# Dawn is the misty hour (the last of the dark and the first hour of sun); the evening and the night carry a
	# little; midday hardly any.
	var dawn := (1.0 - smoothstep(0.08, 0.4, sun_y)) * smoothstep(-0.25, -0.05, sun_y) if morning else 0.0
	var hour := maxf(dawn, maxf(dusk * 0.45, (1.0 - day) * 0.3))
	# Some mornings are clear and some are thick: the clock decides, over a quarter of an hour or so.
	var mood := smoothstep(0.25, 0.85, _drift(t, 900.0, 3.1))
	var want := clampf(hour * (0.35 + 0.65 * mood) + 0.08 * _drift(t, 240.0, 7.7), 0.0, 1.0) * (1.0 - k)
	mist = lerpf(mist, want, 1.0 - exp(-dt / 8.0))
	_ground = lerpf(_ground, ground, 1.0 - exp(-dt / 3.0))
	# Mist pooled low: fog that thickens below a few units over the ground we stand on.
	e.fog_height = _ground + 2.5
	e.fog_height_density = 0.04 * mist
	# A haze that breathes: the distance a little softer or clearer from one few minutes to the next.
	e.fog_density *= 0.9 + 0.2 * _drift(t, 180.0, 1.9) + 0.25 * mist
	e.volumetric_fog_density += 0.002 * mist
	# Wisps, drifting downwind, at the height of the ground under each.
	_mat.density = 0.02 * mist
	var dir := Vector2(1.0, 0.35).normalized()
	var speed := 0.6 + 2.5 * wind
	for v in _wisps:
		v.visible = mist > 0.04
		if not v.visible:
			continue
		var p := Vector2(v.position.x, v.position.z) + dir * speed * dt
		if p.distance_to(center) > WISP_RANGE or v.position == Vector3.ZERO:
			# A new one comes in upwind (or anywhere round us, the first time).
			var a := randf_range(-1.2, 1.2) + (atan2(-dir.y, -dir.x) if v.position != Vector3.ZERO else randf() * TAU)
			p = center + Vector2(cos(a), sin(a)) * randf_range(20.0, WISP_RANGE * 0.9)
		v.position = Vector3(p.x, 0.0, p.y)
		v.position.y = get_parent().height(p.x, p.y) + 1.2

# Attach to a Node2D. Needs the MmoClient node from the extension (bin/ next to mmo.gdextension).
extends Node2D

const SCALE := 2.0              # world units to pixels
@export var host := "127.0.0.1"
@export var port := 7777
@export var web := "http://127.0.0.1:8080"   # the server's --web address, for /play/info

var net: MmoClient
var sprites := {}               # entity id -> Node2D

func _ready() -> void:
	net = MmoClient.new()
	add_child(net)
	net.chat_heard.connect(func(from, _ch, text): print("#%d: %s" % [from, text]))
	var http := HTTPRequest.new()
	add_child(http)
	http.request_completed.connect(_on_info)
	http.request(web + "/play/info")

func _on_info(_result, code, _headers, body: PackedByteArray) -> void:
	if code != 200:
		push_error("could not fetch /play/info")
		return
	var cfg := ConfigFile.new()
	cfg.load("user://identity.cfg")
	var id: String = cfg.get_value("account", "identity", "")
	if id == "":
		id = MmoClient.new_identity()           # keep it: it is the player's account
		cfg.set_value("account", "identity", id)
		cfg.save("user://identity.cfg")
	var err := net.connect_to(host, port, body.get_string_from_utf8(), id)
	if err != "":
		push_error(err)

func _process(_dt: float) -> void:
	net.set_move(Input.get_vector("ui_left", "ui_right", "ui_up", "ui_down").round())
	var seen := {}
	for e in net.entities():
		seen[e.id] = true
		if not sprites.has(e.id):
			var s := Polygon2D.new()
			s.polygon = PackedVector2Array([Vector2(-5, -5), Vector2(5, -5), Vector2(5, 5), Vector2(-5, 5)])
			s.color = Color.WHITE if e.id == net.get_self_id() else Color(0.2, 1.0, 0.4)
			add_child(s)
			sprites[e.id] = s
		# Snapshots come about 20 times a second: glide to each new position.
		var s: Node2D = sprites[e.id]
		s.position = s.position.lerp(e.position * SCALE, 0.35)
	for id in sprites.keys():
		if not seen.has(id):
			sprites[id].queue_free()
			sprites.erase(id)

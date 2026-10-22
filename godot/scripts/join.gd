extends CanvasLayer
## The first screen of a packaged client: which server, and what to be called. Started without --host, the client asks
## here instead of assuming this machine. What was typed is kept in user://join.cfg for next time; the server field
## starts from res://default_server.txt (written by scripts/package_godot.sh) the first time.

signal chosen(server: String, name: String)

const HUD := preload("res://scripts/hud.gd")
const SAVED := "user://join.cfg"

var server_in := LineEdit.new()
var name_in := LineEdit.new()
var note := Label.new()

func _ready() -> void:
	layer = 10 # over the HUD
	HUD.use_fonts()
	var cfg := ConfigFile.new()
	cfg.load(SAVED)
	var fallback := ""
	if FileAccess.file_exists("res://default_server.txt"):
		fallback = FileAccess.get_file_as_string("res://default_server.txt").strip_edges()
	server_in.text = String(cfg.get_value("join", "server", fallback))
	name_in.text = String(cfg.get_value("join", "name", ""))

	var back := ColorRect.new()
	back.color = Color(0.03, 0.035, 0.04)
	back.set_anchors_preset(Control.PRESET_FULL_RECT)
	add_child(back)
	var col := VBoxContainer.new()
	col.set_anchors_preset(Control.PRESET_CENTER_LEFT)
	col.position = Vector2(96, -150)
	col.custom_minimum_size = Vector2(360, 0)
	col.add_theme_constant_override("separation", 8)
	back.add_child(col)

	var title := Label.new()
	title.text = "Join a world"
	title.add_theme_font_override("font", HUD.TITLE)
	title.add_theme_font_size_override("font_size", 44)
	title.add_theme_color_override("font_color", HUD.ACCENT)
	col.add_child(title)
	col.add_child(_label("Server address (host, or host:port of its web gateway)"))
	server_in.placeholder_text = "192.168.1.20"
	col.add_child(_field(server_in))
	col.add_child(_label("Your name"))
	name_in.placeholder_text = "Who you will be known as"
	name_in.max_length = 24
	col.add_child(_field(name_in))
	var go := Button.new()
	go.text = "Enter the world"
	go.custom_minimum_size = Vector2(0, 36)
	go.pressed.connect(_go)
	col.add_child(go)
	note.add_theme_color_override("font_color", HUD.ACCENT)
	note.autowrap_mode = TextServer.AUTOWRAP_WORD
	col.add_child(note)
	server_in.text_submitted.connect(func(_t): name_in.grab_focus())
	name_in.text_submitted.connect(func(_t): _go())
	(name_in if server_in.text != "" else server_in).grab_focus.call_deferred()

func _label(t: String) -> Label:
	var l := Label.new()
	l.text = t
	l.add_theme_color_override("font_color", HUD.MUTED)
	return l

func _field(e: LineEdit) -> LineEdit:
	e.custom_minimum_size = Vector2(0, 34)
	e.add_theme_font_size_override("font_size", 18)
	return e

func _go() -> void:
	var server := server_in.text.strip_edges().trim_prefix("http://").trim_suffix("/")
	var who := name_in.text.strip_edges()
	if server == "":
		note.text = "Which server? Ask whoever runs it for its address."
		return
	if who == "":
		note.text = "Pick a name."
		return
	var cfg := ConfigFile.new()
	cfg.set_value("join", "server", server)
	cfg.set_value("join", "name", who)
	cfg.save(SAVED)
	chosen.emit(server, who)
	queue_free()

## Shown again when the server could not be reached, with why.
func fail(msg: String) -> void:
	note.text = msg

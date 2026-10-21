//! Godot 4 extension: an `MmoClient` node wrapping the mmo-client library.
//!
//! Add the node to a scene, call `connect_to` with the server's `/play/info` JSON, and read
//! `entities()` each frame. The node polls the network itself in `_process`.

use godot::classes::{INode, Node};
use godot::prelude::*;
use mmo_client as m;
use std::ffi::{CStr, CString};

struct MmoExt;

#[gdextension]
unsafe impl ExtensionLibrary for MmoExt {}

#[derive(GodotClass)]
#[class(base = Node)]
struct MmoClient {
    base: Base<Node>,
    c: *mut m::Client,
}

impl MmoClient {
    fn item_name(&self, item: u8) -> GString {
        let p = unsafe { m::mmo_item_name(self.c, item) };
        if p.is_null() { GString::new() } else { GString::from(unsafe { CStr::from_ptr(p) }.to_string_lossy().as_ref()) }
    }

    fn item_color(&self, item: u8) -> GString {
        let p = unsafe { m::mmo_item_color(self.c, item) };
        if p.is_null() { GString::from("#ffffff") } else { GString::from(unsafe { CStr::from_ptr(p) }.to_string_lossy().as_ref()) }
    }
}

#[godot_api]
impl INode for MmoClient {
    fn init(base: Base<Node>) -> Self {
        Self { base, c: std::ptr::null_mut() }
    }

    fn process(&mut self, _delta: f64) {
        if self.c.is_null() {
            return;
        }
        let before = unsafe { m::mmo_status(self.c) };
        let now = unsafe { m::mmo_poll(self.c) };
        if now != before {
            self.base_mut().emit_signal("status_changed", &[Variant::from(now as i64)]);
        }
        loop {
            let mut line = m::MmoChat { from: 0, channel: 0, text: [0; 256] };
            if unsafe { m::mmo_next_chat(self.c, &mut line) } == 0 {
                break;
            }
            let end = line.text.iter().position(|&b| b == 0).unwrap_or(0);
            let text = String::from_utf8_lossy(&line.text[..end]).to_string();
            self.base_mut().emit_signal("chat_heard", &[Variant::from(line.from as i64), Variant::from(line.channel as i64), Variant::from(GString::from(&text))]);
        }
    }
}

impl Drop for MmoClient {
    fn drop(&mut self) {
        unsafe { m::mmo_free(self.c) };
    }
}

#[godot_api]
impl MmoClient {
    /// 0 connecting, 1 playing, 2 rejected, 3 closed, 4 moved to another zone (`moved`).
    #[signal]
    fn status_changed(status: i64);
    /// A chat line: speaker entity id, channel (0 say, 1 shout), text.
    #[signal]
    fn chat_heard(from: i64, channel: i64, text: GString);

    /// Start connecting. `info_json` is the body of GET /play/info; `identity` is 64 hex characters or "" for a guest.
    /// Returns "" on success or an error message.
    #[func]
    fn connect_to(&mut self, host: GString, port: i64, info_json: GString, identity: GString) -> GString {
        unsafe { m::mmo_free(self.c) };
        self.c = std::ptr::null_mut();
        let (Ok(h), Ok(i)) = (CString::new(host.to_string()), CString::new(info_json.to_string())) else {
            return GString::from("strings must not contain NUL");
        };
        let id = identity.to_string();
        let id = (!id.is_empty()).then(|| CString::new(id).unwrap_or_default());
        let p = unsafe { m::mmo_connect(h.as_ptr(), port.clamp(0, 65535) as u16, i.as_ptr(), id.as_ref().map_or(std::ptr::null(), |s| s.as_ptr())) };
        if p.is_null() {
            return GString::from(&unsafe { CStr::from_ptr(m::mmo_last_error()) }.to_string_lossy().to_string());
        }
        self.c = p;
        GString::new()
    }

    #[func]
    fn disconnect_from_server(&mut self) {
        unsafe { m::mmo_free(self.c) };
        self.c = std::ptr::null_mut();
    }

    #[func]
    fn get_status(&self) -> i64 {
        unsafe { m::mmo_status(self.c) as i64 }
    }

    /// Why the connection was rejected or closed.
    #[func]
    fn get_status_text(&self) -> GString {
        let mut buf = [0 as std::ffi::c_char; 160];
        unsafe { m::mmo_status_text(self.c, buf.as_mut_ptr(), buf.len()) };
        GString::from(&unsafe { CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().to_string())
    }

    /// Follow a zone handoff by reconnecting (true, the default after `connect_to`), or stop with status 4 so the
    /// game can load the new zone's map and connect there itself (zone lines). Call it after `connect_to`.
    #[func]
    fn set_follow_redirects(&mut self, on: bool) {
        unsafe { m::mmo_follow_redirects(self.c, on as i32) };
    }

    /// After status 4: where we were handed, {zone (index into /play/info "zones"), host, port}; empty before.
    #[func]
    fn moved(&self) -> VarDictionary {
        let mut d = VarDictionary::new();
        let (mut zone, mut ip, mut port) = (0u8, [0u8; 4], 0u16);
        if unsafe { m::mmo_moved(self.c, &mut zone, ip.as_mut_ptr(), &mut port) } == 1 {
            d.set("zone", zone as i64);
            d.set("host", &GString::from(&format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])));
            d.set("port", port as i64);
        }
        d
    }

    /// Movement intent, each axis -1, 0 or 1. The server decides the speed.
    #[func]
    fn set_move(&mut self, direction: Vector2) {
        unsafe { m::mmo_set_move(self.c, direction.x.round() as i32, direction.y.round() as i32) };
    }

    /// A built block column as /play/blocks reports it (bit k of `mask` = a block in layer `base + k`; 0
    /// clears it), so prediction climbs, falls and walks under as the server does.
    #[func]
    fn set_column(&mut self, cx: i64, cy: i64, base: i64, mask: i64) {
        if cx >= 0 && cy >= 0 {
            unsafe { m::mmo_set_column(self.c, cx as u32, cy as u32, base as i32, mask as u32) };
        }
    }

    /// For a world that streams its walls: the /play/tiles path to GET for the player at `at`, or "" when nothing
    /// is missing. Pass the reply to `put_tiles`, or call `tiles_failed`.
    #[func]
    fn tiles_wanted(&mut self, at: Vector2) -> GString {
        let mut buf = vec![0 as std::ffi::c_char; 2048];
        let n = unsafe { m::mmo_tiles_wanted(self.c, at.x, at.y, buf.as_mut_ptr(), buf.len()) };
        if n <= 0 {
            return GString::new();
        }
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().as_ref().into()
    }

    /// A /play/tiles reply: true once its walls are in prediction, false when the walls changed on the server
    /// (fetch /play/info again and pass it to `set_walls`).
    #[func]
    fn put_tiles(&mut self, bytes: PackedByteArray) -> bool {
        unsafe { m::mmo_put_tiles(self.c, bytes.as_slice().as_ptr(), bytes.len()) == 1 }
    }

    #[func]
    fn tiles_failed(&mut self) {
        unsafe { m::mmo_tiles_failed(self.c) };
    }

    /// True once every wall near the player is in.
    #[func]
    fn walls_ready(&self) -> bool {
        unsafe { m::mmo_walls_ready(self.c) == 1 }
    }

    /// Adopt the walls of a fresh /play/info document; true if they changed.
    #[func]
    fn set_walls(&mut self, info_json: GString) -> bool {
        let Ok(i) = CString::new(info_json.to_string()) else { return false };
        unsafe { m::mmo_set_walls(self.c, i.as_ptr()) == 1 }
    }

    /// Your light on or off (sneaking, when the game has a `lit` attribute).
    #[func]
    fn set_light(&mut self, on: bool) {
        unsafe { m::mmo_set_light(self.c, on as i32) };
    }

    /// The owner tag /play/blocks gives the blocks you build; empty for a guest.
    #[func]
    fn account_tag(&self) -> GString {
        let mut buf = [0 as std::ffi::c_char; 32];
        unsafe { m::mmo_account_tag(self.c, buf.as_mut_ptr(), buf.len()) };
        unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().as_ref().into()
    }

    /// An action with its extra argument (a build's or break's `layer & 0xff | material << 8`).
    #[func]
    fn act_arg(&mut self, action: i64, target: i64, arg: i64) {
        unsafe { m::mmo_act_arg(self.c, action as u8, target as u32, arg as i32) };
    }

    /// Use an action (ids are in /play/info) on a target entity id (0 for none).
    #[func]
    fn act(&mut self, action: i64, target: i64) {
        unsafe { m::mmo_act(self.c, action.clamp(0, 255) as u8, target.max(0) as u32) };
    }

    /// Say (shout = false, heard nearby) or shout (heard by everyone).
    #[func]
    fn say(&mut self, text: GString, shout: bool) {
        if let Ok(t) = CString::new(text.to_string()) {
            unsafe { m::mmo_say(self.c, shout as u8, t.as_ptr()) };
        }
    }

    /// Ask to be known by a name; the server answers in chat (from id 0).
    #[func]
    fn set_display_name(&mut self, name: GString) {
        if let Ok(t) = CString::new(name.to_string()) {
            unsafe { m::mmo_set_name(self.c, t.as_ptr()) };
        }
    }

    #[func]
    fn get_self_id(&self) -> i64 {
        unsafe { m::mmo_self_id(self.c) as i64 }
    }

    #[func]
    fn get_rtt_ms(&self) -> f64 {
        unsafe { m::mmo_rtt_ms(self.c) as f64 }
    }

    /// Everything in view as of the newest snapshot: dictionaries with `id`, `position` (Vector2, world
    /// units, x east and y south) and one key per replicated attribute (for example `health`).
    #[func]
    fn entities(&self) -> Array<VarDictionary> {
        let mut out = Array::new();
        if self.c.is_null() {
            return out;
        }
        let names: Vec<String> = (0..unsafe { m::mmo_attr_count(self.c) })
            .map(|i| unsafe { CStr::from_ptr(m::mmo_attr_name(self.c, i)) }.to_string_lossy().to_string())
            .collect();
        for i in 0..unsafe { m::mmo_entity_count(self.c) } {
            let mut e = m::MmoEntity::default();
            if unsafe { m::mmo_entity(self.c, i, &mut e) } == 0 {
                continue;
            }
            let mut d = VarDictionary::new();
            d.set("id", e.id as i64);
            d.set("position", Vector2::new(e.x, e.y));
            d.set("layer", e.z as i64);
            for (k, n) in names.iter().enumerate() {
                d.set(n.as_str(), e.attrs[k] as f64);
            }
            out.push(&d);
        }
        out
    }

    /// The bag: one dictionary per slot with `slot`, `item` (name, empty string if the slot is empty), `color`
    /// (`#rrggbb`) and `count`. Use a slot with `use_slot`, drop it with `drop_slot`.
    #[func]
    fn bag(&self) -> Array<VarDictionary> {
        let mut out = Array::new();
        if self.c.is_null() {
            return out;
        }
        for i in 0..unsafe { m::mmo_bag_len(self.c) } {
            let (mut item, mut count) = (0u8, 0u8);
            if unsafe { m::mmo_bag_slot(self.c, i, &mut item, &mut count) } == 0 {
                continue;
            }
            let mut d = VarDictionary::new();
            d.set("slot", i as i64);
            d.set("item", &self.item_name(item));
            d.set("color", &self.item_color(item));
            d.set("count", count as i64);
            out.push(&d);
        }
        out
    }

    /// Things on the ground near you: `id`, `position`, `item` (name), `color`, `count`.
    #[func]
    fn ground(&self) -> Array<VarDictionary> {
        let mut out = Array::new();
        if self.c.is_null() {
            return out;
        }
        for i in 0..unsafe { m::mmo_ground_count(self.c) } {
            let mut g = m::MmoGround::default();
            if unsafe { m::mmo_ground(self.c, i, &mut g) } == 0 {
                continue;
            }
            let mut d = VarDictionary::new();
            d.set("id", g.id as i64);
            d.set("position", Vector2::new(g.x, g.y));
            d.set("item", &self.item_name(g.item));
            d.set("color", &self.item_color(g.item));
            d.set("count", g.count as i64);
            out.push(&d);
        }
        out
    }

    /// Use the item in a bag slot (0 to 7).
    #[func]
    fn use_slot(&mut self, slot: i64) {
        self.act(200 + slot.clamp(0, 15), 0);
    }

    /// Drop the stack in a bag slot onto the ground.
    #[func]
    fn drop_slot(&mut self, slot: i64) {
        self.act(216 + slot.clamp(0, 15), 0);
    }

    /// Pick up the nearest thing within reach (3 units).
    #[func]
    fn pickup(&mut self) {
        self.act(250, 0);
    }

    /// A fresh random identity (64 hex characters). Store it: it is the player's account.
    #[func]
    fn new_identity() -> GString {
        let mut b = [0 as std::ffi::c_char; 65];
        unsafe { m::mmo_new_identity(b.as_mut_ptr()) };
        GString::from(&unsafe { CStr::from_ptr(b.as_ptr()) }.to_string_lossy().to_string())
    }

    /// The identity for an account name and passphrase, so the player can log in from any device.
    #[func]
    fn identity_from_passphrase(account: GString, passphrase: GString) -> GString {
        let (Ok(a), Ok(p)) = (CString::new(account.to_string()), CString::new(passphrase.to_string())) else { return GString::new() };
        let mut b = [0 as std::ffi::c_char; 65];
        if unsafe { m::mmo_identity_from_passphrase(a.as_ptr(), p.as_ptr(), b.as_mut_ptr()) } != 0 {
            return GString::new();
        }
        GString::from(&unsafe { CStr::from_ptr(b.as_ptr()) }.to_string_lossy().to_string())
    }
}

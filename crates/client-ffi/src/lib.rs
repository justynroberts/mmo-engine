//! A C ABI for game clients (Unreal, Godot, Unity, anything that can call C).
//!
//! The library owns the UDP socket and the whole protocol: handshake, encryption,
//! snapshots, chat, input timing, reconnects and zone handoffs. The engine calls
//! `mmo_poll` once per frame, sets movement, and reads entities back.
//!
//! Everything is non-blocking and single-threaded: call every function for one
//! client from one thread. Strings are UTF-8 and NUL-terminated.

use std::ffi::{c_char, CStr};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

mod interp;

use proto::chat::{self, ChatClient};
use proto::tiles::TileStream;
use proto::world::World;
use proto::Predictor;
use proto::secure::{self, ClientHs, Session};
pub use proto::snapshot::MAX_REPL;
use proto::wire::{self, kind, Input, ServerMsg};
use proto::{decode_snapshot, AttrDef, ClientView, Defs, OnZero};

pub const MMO_CONNECTING: i32 = 0;
pub const MMO_PLAYING: i32 = 1;
pub const MMO_REJECTED: i32 = 2;
pub const MMO_CLOSED: i32 = 3;
/// The server handed us to another zone and we were asked not to follow by ourselves (`mmo_follow_redirects`):
/// `mmo_moved` says where to go.
pub const MMO_MOVED: i32 = 4;

/// One entity as the client last saw it. Positions are world units.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MmoEntity {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// The block layer its feet are in: `MMO_GROUND` (-128) on the bare ground, otherwise it stands at
    /// `z * build.block` world units up (the top of the block below it).
    pub z: i32,
    /// Replicated attributes, in the order of `mmo_attr_name`. Unused slots are 0.
    pub attrs: [f32; MAX_REPL],
}

/// One chat line. `text` is NUL-terminated UTF-8.
#[repr(C)]
pub struct MmoChat {
    pub from: u32,
    /// 0 = say (nearby), 1 = shout (everyone).
    pub channel: u8,
    pub text: [u8; 256],
}

/// Something lying on the ground in reach. Positions are world units.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct MmoGround {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    /// 1-based index into the item list (`mmo_item_name`); 0 would be empty.
    pub item: u8,
    pub count: u8,
}

pub struct Client {
    bag: Vec<(u8, u8)>,
    ground: Vec<wire::GroundEntry>,
    item_names: Vec<std::ffi::CString>,
    item_colors: Vec<std::ffi::CString>,
    sock: UdpSocket,
    server: SocketAddr,
    defs: Defs,
    hz: u32,
    hash: u64,
    pinned: [u8; 32],
    identity: Option<[u8; 32]>,
    hs: ClientHs,
    session: Option<Session>,
    status: i32,
    entity: u32,
    view: ClientView,
    chat: ChatClient,
    heard: std::collections::VecDeque<chat::ChatMsg>,
    seq: u32,
    dir: (i8, i8),
    action: u8,
    target: u32,
    arg: u16,
    /// Whether our light is on (None until the host says), and whether the server still has to hear it.
    light: Option<bool>,
    light_dirty: bool,
    sent: Vec<(u32, Instant)>,
    rtt_ms: f32,
    last_rx: Instant,
    last_hello: Instant,
    last_input: Instant,
    reject: u8,
    attr_names: Vec<std::ffi::CString>,
    ents: Vec<MmoEntity>,
    err: String,
    world: World,
    /// The walls by tile, when the world streams them (`/play/info` walls.inline false).
    tiles: Option<TileStream>,
    pred: Option<Predictor>,
    hist: interp::History,
    t0: Instant,
    last_poll: Instant,
    /// Where our own entity is drawn: the prediction, smoothed.
    vis: Option<(f32, f32)>,
    /// Follow a zone handoff by reconnecting here (strips of one world), or stop and say where (`MMO_MOVED`) so
    /// the host can load the new zone's map first (zone lines).
    follow: bool,
    moved: Option<wire::Redirect>,
}

fn new_hs(c: &Client) -> ClientHs {
    match c.identity {
        Some(id) => ClientHs::with_identity(Some(c.pinned), c.hash, id),
        None => ClientHs::new(Some(c.pinned), c.hash),
    }
}

/// Defs as far as decoding needs them: names, ranges, widths and which are replicated.
fn defs_from_info(v: &serde_json::Value) -> Option<Defs> {
    let attrs: Vec<AttrDef> = v["attrs"]
        .as_array()?
        .iter()
        .map(|a| {
            Some(AttrDef {
                name: a["name"].as_str()?.to_string(),
                min: a["min"].as_f64()? as f32,
                max: a["max"].as_f64()? as f32,
                default: 0.0,
                bits: a["bits"].as_u64()? as u8,
                replicated: a["replicated"].as_bool()?,
                regen_per_sec: 0.0,
                on_zero: OnZero::None,
            })
        })
        .collect::<Option<_>>()?;
    let replicated: Vec<usize> = v["replicated"].as_array()?.iter().map(|i| i.as_u64().map(|i| i as usize)).collect::<Option<_>>()?;
    if replicated.len() > MAX_REPL || replicated.iter().any(|&i| i >= attrs.len()) {
        return None;
    }
    Some(Defs {
        attrs,
        actions: Vec::new(),
        replicated,
        move_speed: v["move_speed"].as_f64()? as f32,
        entity_radius: v["entity_radius"].as_f64()? as f32,
        interest_radius: v["interest_radius"].as_f64()? as f32,
        max_visible: 0,
    })
}

thread_local! {
    static LAST_ERROR: std::cell::RefCell<std::ffi::CString> = std::cell::RefCell::new(Default::default());
}
fn fail(msg: &str) -> *mut Client {
    LAST_ERROR.with(|e| *e.borrow_mut() = std::ffi::CString::new(msg.replace('\0', " ")).unwrap_or_default());
    std::ptr::null_mut()
}

unsafe fn cstr<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        None
    } else {
        CStr::from_ptr(p).to_str().ok()
    }
}

fn identity_hex(id: &[u8; 32], out: *mut c_char) {
    let s = secure::to_hex(id);
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), out as *mut u8, 64);
        *out.add(64) = 0;
    }
}

/// Why the last call that returned NULL failed. Valid until the next failing call on this thread.
#[no_mangle]
pub extern "C" fn mmo_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// A fresh random identity as 64 hex characters (`out` must hold 65 bytes). Keep it secret: it is the account.
#[no_mangle]
pub unsafe extern "C" fn mmo_new_identity(out: *mut c_char) {
    identity_hex(&secure::new_identity(), out);
}

/// The identity for an account name and passphrase (Argon2id, same as the other clients). `out` holds 65 bytes.
#[no_mangle]
pub unsafe extern "C" fn mmo_identity_from_passphrase(account: *const c_char, passphrase: *const c_char, out: *mut c_char) -> i32 {
    let (Some(a), Some(p)) = (cstr(account), cstr(passphrase)) else { return -1 };
    identity_hex(&secure::identity_from_passphrase(a, p), out);
    0
}

/// The printable recovery code for an identity, written NUL-terminated into `out` (needs `cap` >= 80). Returns the length or -1.
#[no_mangle]
pub unsafe extern "C" fn mmo_recovery_code(identity: *const c_char, out: *mut c_char, cap: usize) -> i32 {
    let Some(id) = cstr(identity).and_then(secure::from_hex32) else { return -1 };
    let code = secure::recovery_code(&id);
    if code.len() + 1 > cap {
        return -1;
    }
    std::ptr::copy_nonoverlapping(code.as_ptr(), out as *mut u8, code.len());
    *out.add(code.len()) = 0;
    code.len() as i32
}

/// The walls `/play/info` carries inline, or, for a world that streams them, none yet and a stream to fetch them by.
fn walls_of(v: &serde_json::Value, size: f32) -> (Vec<proto::Aabb>, Option<TileStream>) {
    if v["walls"]["inline"] == false {
        let ver = v["walls"]["ver"].as_str().and_then(|h| u64::from_str_radix(h, 16).ok()).unwrap_or(0);
        return (Vec::new(), Some(TileStream::new(size, ver)));
    }
    let boxes = v["boxes"]
        .as_array()
        .map(|a| a.iter().filter_map(|b| Some(proto::Aabb { x0: b[0].as_f64()? as f32, y0: b[1].as_f64()? as f32, x1: b[2].as_f64()? as f32, y1: b[3].as_f64()? as f32 })).collect())
        .unwrap_or_default();
    (boxes, None)
}

/// Start connecting. `info_json` is the server's `/play/info` document (fetch it over HTTP from the
/// web gateway); it carries the pinned server key, the content hash and the attribute schema.
/// `identity` is 64 hex characters, or NULL to play as a guest (no saved character).
/// Returns NULL on bad arguments; see `mmo_last_error`. Free with `mmo_free`.
#[no_mangle]
pub unsafe extern "C" fn mmo_connect(host: *const c_char, port: u16, info_json: *const c_char, identity: *const c_char) -> *mut Client {
    let (Some(host), Some(info)) = (cstr(host), cstr(info_json)) else { return fail("host and info_json must be UTF-8 strings") };
    let v: serde_json::Value = match serde_json::from_str(info) {
        Ok(v) => v,
        Err(e) => return fail(&format!("info_json: {e}")),
    };
    if v["proto"].as_u64() != Some(wire::PROTO_VERSION as u64) {
        return fail(&format!("server speaks protocol {}, this library speaks {}", v["proto"], wire::PROTO_VERSION));
    }
    let Some(pinned) = v["server_key"].as_str().and_then(secure::from_hex32) else { return fail("info_json: bad server_key") };
    let Some(hash) = v["content_hash"].as_str().and_then(|h| u64::from_str_radix(h, 16).ok()) else { return fail("info_json: bad content_hash") };
    let Some(defs) = defs_from_info(&v) else { return fail("info_json: bad attribute schema") };
    let size = v["world_size"].as_f64().unwrap_or(0.0) as u32;
    if size == 0 {
        return fail("info_json: no world_size");
    }
    let (boxes, tiles) = walls_of(&v, size as f32);
    let world = World::from_boxes(size as f32, v["seed"].as_u64().unwrap_or(0), boxes);
    let hz = v["hz"].as_u64().unwrap_or(20).clamp(1, 120) as u32;
    let id = match cstr(identity) {
        None => None,
        Some(h) => match secure::from_hex32(h) {
            Some(i) => Some(i),
            None => return fail("identity must be 64 hex characters"),
        },
    };
    let Some(server) = (host, port).to_socket_addrs().ok().and_then(|mut a| a.find(|a| a.is_ipv4())) else { return fail("cannot resolve host") };
    let sock = match UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect(server).and_then(|_| s.set_nonblocking(true)).map(|_| s)) {
        Ok(s) => s,
        Err(e) => return fail(&format!("socket: {e}")),
    };
    let attr_names = defs.replicated.iter().map(|&i| std::ffi::CString::new(defs.attrs[i].name.replace('\0', "")).unwrap_or_default()).collect();
    let now = Instant::now();
    let items = v["items"].as_array().cloned().unwrap_or_default();
    let item_names = items.iter().map(|i| std::ffi::CString::new(i["name"].as_str().unwrap_or("").replace('\0', "")).unwrap_or_default()).collect();
    let item_colors = items.iter().map(|i| std::ffi::CString::new(i["color"].as_str().unwrap_or("#ffffff").replace('\0', "")).unwrap_or_default()).collect();
    let mut c = Client {
        bag: Vec::new(),
        ground: Vec::new(),
        item_names,
        item_colors,
        sock,
        server,
        defs,
        hz,
        hash,
        pinned,
        identity: id,
        hs: ClientHs::new(None, 0),
        session: None,
        status: MMO_CONNECTING,
        entity: 0,
        view: ClientView::new(),
        chat: ChatClient::default(),
        heard: Default::default(),
        seq: 0,
        dir: (0, 0),
        action: 0,
        target: 0,
        arg: 0,
        light: None,
        light_dirty: false,
        sent: Vec::new(),
        rtt_ms: 0.0,
        last_rx: now,
        last_hello: now - Duration::from_secs(1),
        last_input: now,
        reject: 0,
        attr_names,
        ents: Vec::new(),
        err: String::new(),
        world,
        tiles,
        pred: None,
        hist: Default::default(),
        t0: now,
        last_poll: now,
        vis: None,
        follow: true,
        moved: None,
    };
    c.hs = new_hs(&c);
    Box::into_raw(Box::new(c))
}

#[no_mangle]
pub unsafe extern "C" fn mmo_free(c: *mut Client) {
    if !c.is_null() {
        drop(Box::from_raw(c));
    }
}

impl Client {
    fn restart(&mut self, server: SocketAddr) {
        if server != self.server {
            if let Ok(s) = UdpSocket::bind("0.0.0.0:0").and_then(|s| s.connect(server).and_then(|_| s.set_nonblocking(true)).map(|_| s)) {
                self.sock = s;
                self.server = server;
            }
        }
        self.hs = new_hs(self);
        self.session = None;
        self.status = MMO_CONNECTING;
        self.view = ClientView::new();
        self.pred = None;
        self.vis = None;
        self.bag.clear();
        self.ground.clear();
        self.hist.clear();
        self.sent.clear();
        self.last_rx = Instant::now();
        self.last_hello = Instant::now() - Duration::from_secs(1);
    }

    fn on_packet(&mut self, p: &[u8], scratch: &mut Vec<u8>) {
        self.last_rx = Instant::now();
        match p.first().copied() {
            Some(kind::CHALLENGE) => {
                if self.hs.on_challenge(p).is_err() {
                    self.status = MMO_REJECTED;
                    self.reject = 100;
                    self.err = "the server's key is not the one in info_json".into();
                }
            }
            Some(kind::ACCEPT) if self.session.is_none() => {
                if let Some((s, w)) = self.hs.on_accept(p) {
                    if let Some(ServerMsg::Welcome(w)) = wire::decode_server(&w) {
                        self.session = Some(s);
                        self.entity = w.entity;
                        self.status = MMO_PLAYING;
                        self.last_input = Instant::now() - Duration::from_secs(1);
                    }
                }
            }
            Some(kind::REJECT) if self.session.is_none() => {
                self.status = MMO_REJECTED;
                self.reject = p.get(1).copied().unwrap_or(0);
                self.err = match self.reject {
                    wire::reject::BAD_VERSION => "protocol version mismatch",
                    wire::reject::CONTENT_MISMATCH => "the server's game content differs from info_json",
                    wire::reject::SERVER_FULL => "server full",
                    _ => "refused",
                }
                .into();
            }
            Some(kind::DATA) => {
                let Some(s) = self.session.as_mut() else { return };
                let Some(inner) = s.open(p, scratch) else { return };
                if let Some(r) = wire::decode_redirect(inner) {
                    // The zone we were in handed us over: same identity, new server.
                    if self.follow {
                        let to = SocketAddr::from((r.ip, r.port));
                        self.restart(to);
                    } else {
                        self.status = MMO_MOVED;
                        self.session = None;
                        self.moved = Some(r);
                    }
                } else if inner.first() == Some(&kind::INVENTORY) {
                    if let Some(b) = wire::decode_inventory(inner) {
                        self.bag = b;
                    }
                } else if inner.first() == Some(&kind::GROUND) {
                    if let Some(g) = wire::decode_ground(inner) {
                        self.ground = g;
                    }
                } else if inner.first() == Some(&chat::kind::BATCH) {
                    if let Some((ack, msgs)) = chat::decode_batch(inner) {
                        let fresh = self.chat.on_batch(ack, msgs);
                        self.heard.extend(fresh);
                    }
                } else if let Ok(info) = decode_snapshot(inner, &self.defs, &mut self.view) {
                    if !info.stale {
                        let at = self.t0.elapsed().as_secs_f64();
                        let ents = self.view.entries().iter().map(|e| self.ent_of(e)).collect();
                        self.hist.push(interp::Snap { tick: info.tick, at, ents });
                        if let Some(me) = self.view.get(self.entity).copied() {
                            let (mx, my) = (proto::world::dpos(me.x), proto::world::dpos(me.y));
                            match self.pred.as_mut() {
                                Some(p) => {
                                    p.reconcile(&self.world, mx, my, me.z, info.last_input_seq);
                                }
                                None => {
                                    let mut p = Predictor::new(mx, my, self.defs.move_speed, self.hz, self.defs.entity_radius);
                                    p.z = me.z;
                                    self.pred = Some(p);
                                }
                            }
                        }
                        if let Some(i) = self.sent.iter().position(|(s, _)| *s == info.last_input_seq) {
                            self.rtt_ms = self.sent[i].1.elapsed().as_secs_f32() * 1000.0;
                            self.sent.drain(..=i);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn send_input(&mut self) {
        let Some(s) = self.session.as_mut() else { return };
        let mut plain = Vec::new();
        let mut pkt = Vec::new();
        if self.chat.outgoing(&mut plain) && s.seal(&plain, &mut pkt) {
            let _ = self.sock.send(&pkt);
        }
        self.seq = self.seq.wrapping_add(1);
        self.sent.push((self.seq, Instant::now()));
        if self.sent.len() > 128 {
            self.sent.remove(0);
        }
        if let Some(p) = self.pred.as_mut() {
            p.apply_input(&self.world, self.seq, self.dir.0, self.dir.1);
        }
        // The light rides an input with nothing else to say, so it never takes an attack's or a build's place; it
        // repeats now and then because the server resets it when a character respawns.
        if self.action == 0 {
            if let Some(on) = self.light.filter(|_| self.light_dirty || self.seq % 60 == 0) {
                (self.action, self.target, self.arg) = (proto::game::ACT_LIGHT, 0, on as u16);
                self.light_dirty = false;
            }
        }
        wire::encode_input(
            &mut plain,
            &Input { seq: self.seq, ack_tick: self.view.last_tick, dir_x: self.dir.0, dir_y: self.dir.1, action: self.action, target: self.target, arg: self.arg, chat_ack: self.chat.ack() },
        );
        if s.seal(&plain, &mut pkt) {
            let _ = self.sock.send(&pkt);
        }
        self.action = 0; // an action is one request, not a held state
        self.target = 0;
        self.arg = 0;
    }

    fn ent_of(&self, e: &proto::Entry) -> interp::Ent {
        let mut attrs = [0.0; MAX_REPL];
        for (k, &a) in self.defs.replicated.iter().enumerate() {
            attrs[k] = self.defs.dequantize(a, e.attrs[k]);
        }
        interp::Ent { id: e.id, x: proto::world::dpos(e.x), y: proto::world::dpos(e.y), z: e.z, attrs }
    }

    /// Everyone else interpolated between snapshots (drawn two ticks in the past), and
    /// ourselves predicted, so our own movement answers the keyboard at once.
    fn refresh_entities(&mut self, dt: f32) {
        let now = self.t0.elapsed().as_secs_f64();
        self.ents.clear();
        let mine = self.entity;
        if let Some(p) = &self.pred {
            let target = (p.x, p.y);
            let v = self.vis.get_or_insert(target);
            if (v.0 - target.0).hypot(v.1 - target.1) > 24.0 {
                *v = target;
            } else {
                // One predicted step is a whole tick of movement: spread it over the frames in between.
                let k = 1.0 - (-dt * 25.0).exp();
                v.0 += (target.0 - v.0) * k;
                v.1 += (target.1 - v.1) * k;
            }
        }
        for e in self.hist.sample(now, self.hz as f64, 2.0) {
            let mut m = MmoEntity { id: e.id, x: e.x, y: e.y, z: e.z as i32, attrs: e.attrs };
            if e.id == mine {
                if let Some((x, y)) = self.vis {
                    (m.x, m.y) = (x, y);
                }
                if let Some(p) = &self.pred {
                    m.z = p.z as i32;
                }
            }
            self.ents.push(m);
        }
    }
}

/// Run the client for one frame: read packets, retry the handshake, send input at the server's tick
/// rate, notice a dead server and reconnect. Cheap; call it every frame. Returns the status
/// (`MMO_CONNECTING`, `MMO_PLAYING`, `MMO_REJECTED`, `MMO_CLOSED`).
#[no_mangle]
pub unsafe extern "C" fn mmo_poll(c: *mut Client) -> i32 {
    let Some(c) = c.as_mut() else { return MMO_CLOSED };
    if c.status == MMO_REJECTED || c.status == MMO_CLOSED || c.status == MMO_MOVED {
        return c.status;
    }
    let mut buf = [0u8; 2048];
    let mut scratch = Vec::new();
    while let Ok(n) = c.sock.recv(&mut buf) {
        c.on_packet(&buf[..n], &mut scratch);
        if c.status == MMO_REJECTED || c.status == MMO_MOVED {
            return c.status;
        }
    }
    let now = Instant::now();
    if c.session.is_none() {
        if now.duration_since(c.last_hello) >= Duration::from_millis(250) {
            c.last_hello = now;
            let _ = match c.hs.connect_packet() {
                Some(p) => c.sock.send(p),
                None => c.sock.send(&c.hs.hello()),
            };
        }
        if now.duration_since(c.last_rx) > Duration::from_secs(10) {
            c.status = MMO_CLOSED;
            c.err = "no answer from the server".into();
        }
    } else {
        if now.duration_since(c.last_input) >= Duration::from_secs_f64(1.0 / c.hz as f64) {
            c.last_input = now;
            c.send_input();
        }
        if now.duration_since(c.last_rx) > Duration::from_secs(3) {
            let s = c.server;
            c.restart(s);
        }
    }
    let dt = now.duration_since(c.last_poll).as_secs_f32().min(0.25);
    c.last_poll = now;
    c.refresh_entities(dt);
    c.status
}

#[no_mangle]
pub unsafe extern "C" fn mmo_status(c: *const Client) -> i32 {
    c.as_ref().map_or(MMO_CLOSED, |c| c.status)
}

/// Human-readable reason when the status is `MMO_REJECTED` or `MMO_CLOSED`; empty otherwise.
#[no_mangle]
pub unsafe extern "C" fn mmo_status_text(c: *const Client, out: *mut c_char, cap: usize) {
    let Some(c) = c.as_ref() else { return };
    if cap == 0 {
        return;
    }
    let n = c.err.len().min(cap - 1);
    std::ptr::copy_nonoverlapping(c.err.as_ptr(), out as *mut u8, n);
    *out.add(n) = 0;
}

/// Movement intent, each axis -1, 0 or 1 (the server decides the speed).
#[no_mangle]
pub unsafe extern "C" fn mmo_set_move(c: *mut Client, dx: i32, dy: i32) {
    if let Some(c) = c.as_mut() {
        c.dir = ((dx.signum() * 127) as i8, (dy.signum() * 127) as i8);
    }
}

/// Tell prediction about a built block column, as `/play/blocks` reports them, so walking into, onto, under
/// and off one goes here as it does on the server. `cx`, `cy` are block cells (`/play/info` `build.block`
/// units); `base` is the column's first layer and bit k of `mask` is a block in layer `base + k` (0 clears it).
#[no_mangle]
pub unsafe extern "C" fn mmo_set_column(c: *mut Client, cx: u32, cy: u32, base: i32, mask: u32) {
    if let Some(c) = c.as_mut() {
        c.world.set_column(cx, cy, proto::world::Column { base: base.clamp(-128, 127) as i8, mask: mask as u16 });
    }
}

/// For a world that streams its walls: the `/play/tiles` request to make for the player at (x, y), written to `out`
/// as a NUL-terminated path with query. Returns its length, 0 when nothing is missing (or the walls came inline),
/// or -1 if `cap` is too small. Fetch it and hand the reply to `mmo_put_tiles`; if the fetch fails, call
/// `mmo_tiles_failed`. Call it as the player moves; it asks for each tile once.
#[no_mangle]
pub unsafe extern "C" fn mmo_tiles_wanted(c: *mut Client, x: f32, y: f32, out: *mut c_char, cap: usize) -> i32 {
    let Some(t) = c.as_mut().and_then(|c| c.tiles.as_mut()) else { return 0 };
    let want = t.wanted(x, y);
    if want.is_empty() {
        return 0;
    }
    let list: Vec<String> = want.iter().map(|(x, y)| format!("{x},{y}")).collect();
    let path = format!("/play/tiles?v={:016x}&c={}", t.ver, list.join(";"));
    if path.len() + 1 > cap {
        t.retry();
        return -1;
    }
    std::ptr::copy_nonoverlapping(path.as_ptr(), out as *mut u8, path.len());
    *out.add(path.len()) = 0;
    path.len() as i32
}

/// A `/play/tiles` reply. Returns 1 once its walls are in prediction, 0 if it is for walls that have since changed
/// (or the server answered 409): fetch `/play/info` again and give it to `mmo_set_walls`.
#[no_mangle]
pub unsafe extern "C" fn mmo_put_tiles(c: *mut Client, bytes: *const u8, len: usize) -> i32 {
    let Some(c) = c.as_mut() else { return 0 };
    let Some(t) = c.tiles.as_mut() else { return 0 };
    if bytes.is_null() || !t.put(std::slice::from_raw_parts(bytes, len)) {
        return 0;
    }
    c.world.set_boxes(t.boxes().into_iter().map(|(_, b, _)| b).collect());
    1
}

/// A `/play/tiles` fetch failed: the tiles it asked for are asked for again next time.
#[no_mangle]
pub unsafe extern "C" fn mmo_tiles_failed(c: *mut Client) {
    if let Some(t) = c.as_mut().and_then(|c| c.tiles.as_mut()) {
        t.retry();
    }
}

/// True once every wall near the player is in (always, for walls that came inline): before then prediction may
/// walk through a wall the server will not.
#[no_mangle]
pub unsafe extern "C" fn mmo_walls_ready(c: *const Client) -> i32 {
    c.as_ref().is_some_and(|c| c.tiles.as_ref().is_none_or(TileStream::ready)) as i32
}

/// Adopt the walls of a fresh `/play/info` (the console edited them, or `mmo_put_tiles` said they changed). Returns
/// 1 if they changed.
#[no_mangle]
pub unsafe extern "C" fn mmo_set_walls(c: *mut Client, info_json: *const c_char) -> i32 {
    let (Some(c), Some(info)) = (c.as_mut(), cstr(info_json)) else { return 0 };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(info) else { return 0 };
    let (boxes, tiles) = walls_of(&v, c.world.size);
    let same = match (&c.tiles, &tiles) {
        (Some(a), Some(b)) => a.ver == b.ver,
        (None, None) => c.world.boxes == boxes,
        _ => false,
    };
    if same {
        return 0;
    }
    c.tiles = tiles;
    c.world.set_boxes(boxes);
    1
}

/// The tag `/play/blocks` gives as the owner of the blocks you build (16 hex digits, NUL-terminated; `out`
/// holds 17 bytes). Empty for a guest, whose blocks nobody owns.
#[no_mangle]
pub unsafe extern "C" fn mmo_account_tag(c: *const Client, out: *mut c_char, cap: usize) {
    let Some(c) = c.as_ref() else { return };
    if cap == 0 {
        return;
    }
    let tag = c.identity.map(|id| secure::account_tag(&secure::identity_public(&id))).unwrap_or_default();
    let n = tag.len().min(cap - 1);
    std::ptr::copy_nonoverlapping(tag.as_ptr(), out as *mut u8, n);
    *out.add(n) = 0;
}

/// Request an action (the ids are in `/play/info` under `actions`) against a target entity id (0 = none). One request per call.
#[no_mangle]
pub unsafe extern "C" fn mmo_act(c: *mut Client, action: u8, target: u32) {
    mmo_act_arg(c, action, target, 0);
}

/// Your light (a torch, a lantern) on or off. Only means something when the game has a `lit` attribute: then other
/// players' clients hide your name in the dark while it is out, and bots see you less far at night (sneaking). The
/// library tells the server in an input that carries no other action, so it never displaces one.
#[no_mangle]
pub unsafe extern "C" fn mmo_set_light(c: *mut Client, on: i32) {
    if let Some(c) = c.as_mut() {
        if c.light != Some(on != 0) {
            c.light = Some(on != 0);
            c.light_dirty = true;
        }
    }
}

/// An action with its extra argument (0..65535): for build and break, `layer & 0xff | material << 8`, the layer
/// -128..127 and the material 0..build.kinds-1.
#[no_mangle]
pub unsafe extern "C" fn mmo_act_arg(c: *mut Client, action: u8, target: u32, arg: i32) {
    if let Some(c) = c.as_mut() {
        c.action = action;
        c.target = target;
        c.arg = arg as u16;
    }
}

/// Say (`channel` 0, heard nearby) or shout (1, heard by all). Queued and resent until the server has it.
#[no_mangle]
pub unsafe extern "C" fn mmo_say(c: *mut Client, channel: u8, text: *const c_char) {
    if let (Some(c), Some(t)) = (c.as_mut(), cstr(text)) {
        if !t.trim().is_empty() {
            c.chat.say(channel.min(proto::chat::channel::SHOUT), t.trim());
        }
    }
}

/// Whether to follow a zone handoff by reconnecting (1, the default: right for strips of one world, which share the
/// map) or to stop with `MMO_MOVED` (0: zone lines, where the host loads the new zone's map and connects again).
#[no_mangle]
pub unsafe extern "C" fn mmo_follow_redirects(c: *mut Client, on: i32) {
    if let Some(c) = c.as_mut() {
        c.follow = on != 0;
    }
}

/// After `MMO_MOVED`: the zone we were handed to, by index (its web address is `/play/info` zones[index]), and its
/// game address. Returns 1 and fills them in, or 0 if we have not moved.
#[no_mangle]
pub unsafe extern "C" fn mmo_moved(c: *const Client, zone: *mut u8, ip: *mut u8, port: *mut u16) -> i32 {
    let Some(r) = c.as_ref().and_then(|c| c.moved.as_ref()) else { return 0 };
    *zone = r.zone;
    std::ptr::copy_nonoverlapping(r.ip.as_ptr(), ip, 4);
    *port = r.port;
    1
}

/// Ask to be known by `name` (`proto::names::clean` says what is allowed). The server answers with a chat line
/// from id 0 ("You are now known as ...", or why not); everyone's names are at the web gateway's `/play/names`.
#[no_mangle]
pub unsafe extern "C" fn mmo_set_name(c: *mut Client, name: *const c_char) {
    if let (Some(c), Some(t)) = (c.as_mut(), cstr(name)) {
        if !t.trim().is_empty() {
            c.chat.say(proto::chat::channel::NAME, t.trim());
        }
    }
}

/// Pop the next chat line heard. Returns 1 and fills `out`, or 0 when there is none.
#[no_mangle]
pub unsafe extern "C" fn mmo_next_chat(c: *mut Client, out: *mut MmoChat) -> i32 {
    let Some(c) = c.as_mut() else { return 0 };
    let Some(m) = c.heard.pop_front() else { return 0 };
    let o = &mut *out;
    o.from = m.from;
    o.channel = m.channel;
    let mut n = m.text.len().min(255);
    while !m.text.is_char_boundary(n) {
        n -= 1;
    }
    o.text = [0; 256];
    o.text[..n].copy_from_slice(&m.text.as_bytes()[..n]);
    1
}

/// The entity id of the player this client controls (changes after a zone handoff).
#[no_mangle]
pub unsafe extern "C" fn mmo_self_id(c: *const Client) -> u32 {
    c.as_ref().map_or(0, |c| c.entity)
}

/// Entities in the newest snapshot, including your own. Valid until the next `mmo_poll`.
#[no_mangle]
pub unsafe extern "C" fn mmo_entity_count(c: *const Client) -> usize {
    c.as_ref().map_or(0, |c| c.ents.len())
}

/// Copy entity `i` (0 <= i < `mmo_entity_count`) into `out`. Returns 1, or 0 if out of range.
#[no_mangle]
pub unsafe extern "C" fn mmo_entity(c: *const Client, i: usize, out: *mut MmoEntity) -> i32 {
    match c.as_ref().and_then(|c| c.ents.get(i)) {
        Some(e) => {
            *out = *e;
            1
        }
        None => 0,
    }
}

/// How many replicated attributes an entity carries, and their names (in `MmoEntity::attrs` order).
#[no_mangle]
pub unsafe extern "C" fn mmo_attr_count(c: *const Client) -> usize {
    c.as_ref().map_or(0, |c| c.attr_names.len())
}

/// The name of replicated attribute `i`, valid while the client lives. NULL if out of range.
#[no_mangle]
pub unsafe extern "C" fn mmo_attr_name(c: *const Client, i: usize) -> *const c_char {
    c.as_ref().and_then(|c| c.attr_names.get(i)).map_or(std::ptr::null(), |s| s.as_ptr())
}

/// Round trip time to the server in milliseconds (0 until measured).
#[no_mangle]
pub unsafe extern "C" fn mmo_rtt_ms(c: *const Client) -> f32 {
    c.as_ref().map_or(0.0, |c| c.rtt_ms)
}

/// The library's protocol version, to check against the server before connecting.
#[no_mangle]
pub extern "C" fn mmo_protocol_version() -> u16 {
    wire::PROTO_VERSION
}

/// Slots in the bag (always the server's bag size once the first inventory arrives, 0 before).
#[no_mangle]
pub unsafe extern "C" fn mmo_bag_len(c: *const Client) -> usize {
    c.as_ref().map_or(0, |c| c.bag.len())
}

/// Slot `i`: the 1-based item index (0 = empty) and how many. Use an item with `mmo_act(c, 200 + slot, 0)`,
/// drop it with `216 + slot`, pick up the nearest thing within 3 units with 250. `mmo_act(c, 251, 0)` jumps.
#[no_mangle]
pub unsafe extern "C" fn mmo_bag_slot(c: *const Client, i: usize, item: *mut u8, count: *mut u8) -> i32 {
    let Some(c) = c.as_ref() else { return 0 };
    let Some(&(it, n)) = c.bag.get(i) else { return 0 };
    if !item.is_null() {
        *item = it;
    }
    if !count.is_null() {
        *count = n;
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn mmo_ground_count(c: *const Client) -> usize {
    c.as_ref().map_or(0, |c| c.ground.len())
}

#[no_mangle]
pub unsafe extern "C" fn mmo_ground(c: *const Client, i: usize, out: *mut MmoGround) -> i32 {
    let (Some(c), Some(out)) = (c.as_ref(), out.as_mut()) else { return 0 };
    let Some(g) = c.ground.get(i) else { return 0 };
    // GROUND carries the plain item index; this API (like bag slots) is 1-based so 0 can mean "nothing".
    *out = MmoGround { id: g.id, x: proto::world::dpos(g.x), y: proto::world::dpos(g.y), item: g.item.saturating_add(1), count: g.count };
    1
}

/// Number of item kinds in the game, from `info_json`.
#[no_mangle]
pub unsafe extern "C" fn mmo_item_count(c: *const Client) -> usize {
    c.as_ref().map_or(0, |c| c.item_names.len())
}

/// Name of an item by its 1-based index (what bag slots and ground items carry), or NULL.
#[no_mangle]
pub unsafe extern "C" fn mmo_item_name(c: *const Client, item: u8) -> *const c_char {
    c.as_ref().and_then(|c| c.item_names.get((item as usize).wrapping_sub(1))).map_or(std::ptr::null(), |s| s.as_ptr())
}

/// The item's colour as a `#rrggbb` string, or NULL.
#[no_mangle]
pub unsafe extern "C" fn mmo_item_color(c: *const Client, item: u8) -> *const c_char {
    c.as_ref().and_then(|c| c.item_colors.get((item as usize).wrapping_sub(1))).map_or(std::ptr::null(), |s| s.as_ptr())
}

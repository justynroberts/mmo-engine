//! End-to-end tests: start the real server binary and talk to it over real
//! UDP and HTTP, the way a game client and the test UI would.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use proto::game::GameFile;
use proto::secure::{from_hex32, ClientHs, Session};
use proto::wire::{self, kind, reject, Input, ServerMsg};
use proto::{decode_snapshot, ClientView, Defs};

struct Srv {
    child: Child,
    udp: SocketAddr,
    http: SocketAddr,
    dir: PathBuf,
}

impl Drop for Srv {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn free_udp() -> u16 {
    UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}
fn free_tcp() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn start(extra: &[&str]) -> Srv {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-{}-{}", std::process::id(), free_udp()));
    std::fs::create_dir_all(&dir).unwrap();
    start_in(dir, extra)
}

/// Start a server working in `dir` (so a restart can reuse its key and player database).
fn start_in(dir: PathBuf, extra: &[&str]) -> Srv {
    let (u, h) = (free_udp(), free_tcp());
    let child = Command::new(env!("CARGO_BIN_EXE_server"))
        .current_dir(&dir)
        .args(["--port", &u.to_string(), "--admin", &format!("127.0.0.1:{h}")])
        .args(["--world-size", "512", "--quiet", "--timeout", "2"])
        .args(extra)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn server");
    let srv = Srv { child, udp: format!("127.0.0.1:{u}").parse().unwrap(), http: format!("127.0.0.1:{h}").parse().unwrap(), dir };
    let t = Instant::now();
    // Wait until the admin port accepts connections and the sim thread answers.
    loop {
        if TcpStream::connect(srv.http).is_ok() && http(&srv, "GET", "/api/info", "").0 == 200 {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "server did not come up");
        std::thread::sleep(Duration::from_millis(50));
    }
    srv
}

/// Minimal HTTP client. Returns (status, body).
fn http_raw(srv: &Srv, req: &str) -> (u16, String) {
    let mut s = TcpStream::connect(srv.http).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    let _ = s.read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    let body = text.split_once("\r\n\r\n").map(|x| x.1.to_string()).unwrap_or_default();
    (status, body)
}

fn http(srv: &Srv, method: &str, path: &str, body: &str) -> (u16, String) {
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost:{}\r\nX-Admin: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        srv.http.port(),
        body.len()
    );
    http_raw(srv, &req)
}

fn json(srv: &Srv, method: &str, path: &str, body: &str) -> (u16, serde_json::Value) {
    let (s, b) = http(srv, method, path, body);
    (s, serde_json::from_str(&b).unwrap_or(serde_json::Value::Null))
}

fn defs() -> Defs {
    GameFile::default_game().build().unwrap().defs
}

fn server_key(srv: &Srv) -> [u8; 32] {
    let (_, v) = json(srv, "GET", "/api/info", "");
    from_hex32(v["server_key"].as_str().unwrap()).unwrap()
}

struct Client {
    sock: UdpSocket,
    sess: Session,
    entity: u32,
    view: ClientView,
    seq: u32,
    defs: Defs,
    chat: proto::chat::ChatClient,
    heard: Vec<proto::chat::ChatMsg>,
    redirect: Option<wire::Redirect>,
    bag: Vec<(u8, u8)>,
    ground: Vec<wire::GroundEntry>,
    casts: Vec<wire::Cast>,
}

enum Outcome {
    Connected(Box<Client>),
    Rejected(u8),
    Silent,
}

fn connect_with(srv: &Srv, pinned: Option<[u8; 32]>, hash: u64) -> Outcome {
    connect_full(srv, pinned, hash, None)
}

fn connect_full(srv: &Srv, pinned: Option<[u8; 32]>, hash: u64, identity: Option<[u8; 32]>) -> Outcome {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.connect(srv.udp).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(150))).unwrap();
    let mut hs = match identity {
        Some(id) => ClientHs::with_identity(pinned, hash, id),
        None => ClientHs::new(pinned, hash),
    };
    let mut buf = [0u8; 2048];
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(4) {
        let _ = match hs.connect_packet() {
            Some(c) => sock.send(c),
            None => sock.send(&hs.hello()),
        };
        while let Ok(n) = sock.recv(&mut buf) {
            match buf[0] {
                kind::CHALLENGE => {
                    let _ = hs.on_challenge(&buf[..n]);
                }
                kind::REJECT => return Outcome::Rejected(buf[1]),
                kind::ACCEPT => {
                    if let Some((sess, w)) = hs.on_accept(&buf[..n]) {
                        let Some(ServerMsg::Welcome(w)) = wire::decode_server(&w) else { panic!("bad welcome") };
                        return Outcome::Connected(Box::new(Client {
                            sock,
                            sess,
                            entity: w.entity,
                            view: ClientView::new(),
                            seq: 0,
                            defs: defs(),
                            chat: Default::default(),
                            heard: Vec::new(),
                            redirect: None,
                            bag: Vec::new(),
                            ground: Vec::new(),
                            casts: Vec::new(),
                        }));
                    }
                }
                _ => {}
            }
        }
    }
    Outcome::Silent
}

fn connect_as(srv: &Srv, identity: [u8; 32]) -> Client {
    match connect_full(srv, Some(server_key(srv)), defs().content_hash(), Some(identity)) {
        Outcome::Connected(c) => *c,
        Outcome::Rejected(r) => panic!("rejected: {r}"),
        Outcome::Silent => panic!("no answer"),
    }
}

fn connect(srv: &Srv) -> Client {
    match connect_with(srv, Some(server_key(srv)), defs().content_hash()) {
        Outcome::Connected(c) => *c,
        Outcome::Rejected(r) => panic!("rejected: {r}"),
        Outcome::Silent => panic!("no answer"),
    }
}

impl Client {
    fn send_input(&mut self, dx: i8, dy: i8) -> Vec<u8> {
        self.seq += 1;
        let mut plain = Vec::new();
        wire::encode_input(&mut plain, &Input { seq: self.seq, ack_tick: self.view.last_tick, dir_x: dx, dir_y: dy, action: 0, target: 0, arg: 0, chat_ack: self.chat.ack() });
        let mut pkt = Vec::new();
        assert!(self.sess.seal(&plain, &mut pkt));
        self.sock.send(&pkt).unwrap();
        pkt
    }

    /// Receive and apply snapshots for up to `ms`. Returns how many applied.
    fn pump(&mut self, ms: u64) -> usize {
        let mut n_ok = 0;
        let (mut buf, mut plain) = ([0u8; 2048], Vec::new());
        self.sock.set_read_timeout(Some(Duration::from_millis(60))).unwrap();
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            if let Ok(n) = self.sock.recv(&mut buf) {
                if let Some(inner) = self.sess.open(&buf[..n], &mut plain) {
                    if let Some(r) = wire::decode_redirect(inner) {
                        self.redirect = Some(r);
                        continue;
                    }
                    if let Some(b) = wire::decode_inventory(inner) {
                        self.bag = b;
                        continue;
                    }
                    if let Some(g) = wire::decode_ground(inner) {
                        self.ground = g;
                        continue;
                    }
                    if let Some(k) = wire::decode_casts(inner) {
                        self.casts.extend(k);
                        continue;
                    }
                    if inner.first() == Some(&proto::chat::kind::BATCH) {
                        let (ack, msgs) = proto::chat::decode_batch(inner).expect("valid batch");
                        let fresh = self.chat.on_batch(ack, msgs);
                        self.heard.extend(fresh);
                        continue;
                    }
                    if decode_snapshot(inner, &self.defs, &mut self.view).is_ok() {
                        n_ok += 1;
                    }
                }
            }
        }
        n_ok
    }

    /// Queue a line and resend it (as a client would) until the server acknowledges it.
    fn say(&mut self, channel: u8, text: &str) {
        self.chat.say(channel, text);
    }

    /// One client tick: resend any unacked chat, send an input (which carries the chat ack).
    fn tick(&mut self) {
        let mut b = Vec::new();
        if self.chat.outgoing(&mut b) {
            let mut pkt = Vec::new();
            assert!(self.sess.seal(&b, &mut pkt));
            self.sock.send(&pkt).unwrap();
        }
        self.send_input(0, 0);
        self.pump(50);
    }

    fn pos(&self) -> (f32, f32) {
        let e = self.view.get(self.entity).expect("own entity in snapshot");
        (proto::world::dpos(e.x), proto::world::dpos(e.y))
    }
}

fn state(srv: &Srv) -> serde_json::Value {
    json(srv, "GET", "/api/state", "").1
}

// ---------------------------------------------------------------------------

#[test]
fn encrypted_handshake_then_snapshots_flow_and_player_moves() {
    let srv = start(&[]);
    let mut c = connect(&srv);
    // Drive east for a second and watch our own entity move in decrypted snapshots.
    let _ = c.pump(100);
    let (x0, y0) = c.pos();
    for _ in 0..25 {
        c.send_input(127, 0);
        c.pump(40);
    }
    let (x1, y1) = c.pos();
    assert!(x1 > x0 + 3.0 || y1 != y0, "did not move: ({x0},{y0}) -> ({x1},{y1})");
    let s = state(&srv);
    assert_eq!(s["clients"], 1);
    assert_eq!(s["net"]["auth_fail"], 0);
    assert!(s["net"]["inputs"].as_u64().unwrap() >= 20);
}

#[test]
fn traffic_on_the_wire_is_ciphertext() {
    let srv = start(&[]);
    let mut c = connect(&srv);
    let mut pkt = Vec::new();
    let mut plain = Vec::new();
    wire::encode_input(&mut plain, &Input { seq: 7, ack_tick: 0, dir_x: 99, dir_y: 99, action: 0, target: 0, arg: 0, chat_ack: 0 });
    c.sess.seal(&plain, &mut pkt);
    assert_eq!(pkt.len(), plain.len() + proto::secure::DATA_OVERHEAD);
    assert_ne!(&pkt[proto::secure::DATA_HEADER..proto::secure::DATA_HEADER + 2], &plain[..2]);
}

#[test]
fn forged_corrupted_and_replayed_packets_are_dropped_and_counted() {
    let srv = start(&[]);
    let mut c = connect(&srv);
    c.pump(100);
    let good = c.send_input(0, 0);
    c.pump(100);
    let before = state(&srv)["net"]["auth_fail"].as_u64().unwrap();

    let attacker = UdpSocket::bind("127.0.0.1:0").unwrap();
    attacker.connect(srv.udp).unwrap();
    // 1. Replay of a real packet, from the real source and from another one.
    c.sock.send(&good).unwrap();
    attacker.send(&good).unwrap();
    // 2. Bit flips anywhere in a fresh packet.
    let fresh = c.send_input(0, 0);
    for i in 0..fresh.len() {
        let mut bad = fresh.clone();
        bad[i] ^= 0x40;
        c.sock.send(&bad).unwrap();
    }
    // 3. Right session id, random contents; random junk; wrong kind.
    let mut junk = vec![kind::DATA];
    junk.extend_from_slice(&c.sess.sid.to_le_bytes());
    junk.extend((0..40).map(|i| (i * 37) as u8));
    attacker.send(&junk).unwrap();
    attacker.send(&[0xEE; 30]).unwrap();
    attacker.send(&[]).unwrap();
    std::thread::sleep(Duration::from_millis(250));

    let s = state(&srv);
    let after = s["net"]["auth_fail"].as_u64().unwrap();
    // Flips in the kind byte or session id make the packet unparseable or unroutable (counted as
    // bad); every other flip, both replays and the wrong-source junk must fail authentication.
    assert!(after - before >= fresh.len() as u64 - 5 + 3, "auth failures counted: {}", after - before);
    // The legitimate session is unharmed and the entity count did not change.
    c.send_input(127, 0);
    assert!(c.pump(300) > 0, "legitimate client stopped receiving");
    assert_eq!(s["clients"], 1);
}

#[test]
fn hello_flood_allocates_nothing_and_never_amplifies() {
    let srv = start(&[]);
    let before = state(&srv)["live"].as_u64().unwrap();
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.connect(srv.udp).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    let mut buf = [0u8; 2048];
    let t0 = Instant::now();
    for i in 0..500 {
        let hello = ClientHs::new(None, 1).hello();
        sock.send(&hello).unwrap();
        let n = sock.recv(&mut buf).unwrap_or_else(|e| panic!("no challenge for hello {i} at {:?}: {e}", t0.elapsed()));
        assert!(n < hello.len(), "amplification {} -> {n}", hello.len());
    }
    // A short HELLO gets no answer at all.
    sock.send(&ClientHs::new(None, 1).hello()[..40]).unwrap();
    assert!(sock.recv(&mut buf).is_err());
    let s = state(&srv);
    assert_eq!(s["clients"], 0, "a HELLO must not create a client");
    assert_eq!(s["live"].as_u64().unwrap(), before, "a HELLO must not spawn an entity");
    assert!(s["net"]["hellos"].as_u64().unwrap() >= 500);
}

#[test]
fn connect_from_the_wrong_address_is_ignored() {
    let srv = start(&[]);
    // Complete the first half on socket A.
    let a = UdpSocket::bind("127.0.0.1:0").unwrap();
    a.connect(srv.udp).unwrap();
    a.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let mut hs = ClientHs::new(None, defs().content_hash());
    a.send(&hs.hello()).unwrap();
    let mut buf = [0u8; 2048];
    let n = a.recv(&mut buf).unwrap();
    hs.on_challenge(&buf[..n]).unwrap();
    let connect = hs.connect_packet().unwrap().to_vec();
    // An attacker on socket B replays A's CONNECT: wrong source, so wrong cookie.
    let b = UdpSocket::bind("127.0.0.1:0").unwrap();
    b.connect(srv.udp).unwrap();
    b.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
    b.send(&connect).unwrap();
    assert!(b.recv(&mut buf).is_err(), "server answered a spoofed CONNECT");
    std::thread::sleep(Duration::from_millis(100));
    let s = state(&srv);
    assert_eq!(s["clients"], 0);
    assert!(s["net"]["bad_cookie"].as_u64().unwrap() >= 1);
    // The real owner of the address still gets through.
    a.send(&connect).unwrap();
    let n = a.recv(&mut buf).unwrap();
    assert_eq!(buf[0], kind::ACCEPT);
    assert!(hs.on_accept(&buf[..n]).is_some());
}

#[test]
fn retransmitted_connect_gets_the_same_accept_and_one_entity() {
    let srv = start(&[]);
    let a = UdpSocket::bind("127.0.0.1:0").unwrap();
    a.connect(srv.udp).unwrap();
    a.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let mut hs = ClientHs::new(None, defs().content_hash());
    a.send(&hs.hello()).unwrap();
    let mut buf = [0u8; 2048];
    let n = a.recv(&mut buf).unwrap();
    hs.on_challenge(&buf[..n]).unwrap();
    let cp = hs.connect_packet().unwrap().to_vec();
    a.send(&cp).unwrap();
    let n1 = a.recv(&mut buf).unwrap();
    let first = buf[..n1].to_vec();
    a.send(&cp).unwrap();
    // Snapshots may arrive in between; wait for the second ACCEPT.
    let second = loop {
        let n = a.recv(&mut buf).expect("second ACCEPT");
        if buf[0] == kind::ACCEPT {
            break buf[..n].to_vec();
        }
    };
    assert_eq!(first, second, "retransmit must be answered identically");
    assert_eq!(state(&srv)["clients"], 1);
}

#[test]
fn wrong_content_hash_wrong_pinned_key_and_full_server_are_refused() {
    let srv = start(&["--max-clients", "1"]);
    let key = server_key(&srv);
    assert!(matches!(connect_with(&srv, Some(key), 0xBAD), Outcome::Rejected(r) if r == reject::CONTENT_MISMATCH));
    // An impostor key pin: the client refuses before sending anything secret.
    assert!(matches!(connect_with(&srv, Some([9u8; 32]), defs().content_hash()), Outcome::Silent));
    let _first = connect(&srv);
    assert!(matches!(connect_with(&srv, Some(key), defs().content_hash()), Outcome::Rejected(r) if r == reject::SERVER_FULL));
    assert_eq!(state(&srv)["clients"], 1);
}

#[test]
fn silent_clients_time_out_and_their_entity_is_removed() {
    let srv = start(&[]);
    let _c = connect(&srv);
    let base = state(&srv)["live"].as_u64().unwrap();
    std::thread::sleep(Duration::from_millis(3500));
    let s = state(&srv);
    assert_eq!(s["clients"], 0);
    assert_eq!(s["live"].as_u64().unwrap(), base - 1);
    assert!(s["net"]["timeouts"].as_u64().unwrap() >= 1);
}

#[test]
fn many_clients_each_get_only_their_own_session() {
    // Connecting 20 clients one after another takes longer than the default 2 s idle timeout.
    let srv = start(&["--timeout", "30"]);
    let mut cs: Vec<Client> = (0..20).map(|_| connect(&srv)).collect();
    for _ in 0..4 {
        for c in cs.iter_mut() {
            c.send_input(0, 0);
        }
        for c in cs.iter_mut() {
            c.pump(60);
        }
    }
    for c in cs.iter_mut() {
        assert!(c.view.get(c.entity).is_some(), "client never received its own entity");
    }
    // A packet sealed for client 0 is garbage to client 1.
    let pkt = {
        let mut p = Vec::new();
        let mut plain = Vec::new();
        wire::encode_input(&mut plain, &Input { seq: 1, ack_tick: 0, dir_x: 0, dir_y: 0, action: 0, target: 0, arg: 0, chat_ack: 0 });
        cs[0].sess.seal(&plain, &mut p);
        p
    };
    let mut scratch = Vec::new();
    assert!(cs[1].sess.open(&pkt, &mut scratch).is_none());
    assert_eq!(state(&srv)["clients"], 20);
}

// ------------------------------------------------------------------ admin

#[test]
fn admin_api_rejects_cross_site_and_rebinding_and_traversal() {
    let srv = start(&[]);
    let p = srv.http.port();
    // A write without the custom header (what a cross-site form post looks like).
    let (s, _) = http_raw(&srv, &format!("POST /api/kill HTTP/1.1\r\nHost: localhost:{p}\r\nContent-Length: 2\r\n\r\n{{}}"));
    assert_eq!(s, 403);
    // DNS rebinding: a foreign Host header.
    let (s, _) = http_raw(&srv, "GET /api/state HTTP/1.1\r\nHost: evil.example\r\n\r\n");
    assert_eq!(s, 403);
    // Path traversal and hidden files never reach the filesystem.
    for path in ["/../../etc/passwd", "/%2e%2e/secret", "/.env", "//etc/passwd", "/a/../b"] {
        let (s, body) = http_raw(&srv, &format!("GET {path} HTTP/1.1\r\nHost: localhost:{p}\r\n\r\n"));
        assert!(s == 404 || s == 200 && !body.contains("root:"), "{path} -> {s}");
    }
    // Oversized body is refused.
    let big = "x".repeat(300_000);
    let (s, _) = http(&srv, "PUT", "/api/game", &big);
    assert_ne!(s, 200);
    // Unknown endpoint.
    assert_eq!(http(&srv, "GET", "/api/nope", "").0, 404);
}

#[test]
fn admin_refuses_to_bind_to_a_public_address() {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-bind-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_server"))
        .current_dir(&dir)
        .args(["--port", &free_udp().to_string(), "--admin", "0.0.0.0:7999", "--duration", "1"])
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("loopback"));
}

#[test]
fn game_file_validation_errors_are_reported_and_change_nothing() {
    let srv = start(&[]);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    let hash0 = state(&srv)["hash"].clone();
    game["attrs"][0]["min"] = 500.into();
    let (s, v) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(s, 400);
    assert!(v["error"].as_str().unwrap().contains("min must be below max"), "{v}");
    let (s, v) = json(&srv, "PUT", "/api/game", "{not json");
    assert_eq!(s, 400);
    assert!(v["error"].as_str().unwrap().starts_with("json:"));
    assert_eq!(state(&srv)["hash"], hash0);
}

#[test]
fn editing_bots_keeps_clients_but_editing_attributes_resets_them() {
    let srv = start(&[]);
    let mut c = connect(&srv);
    c.pump(100);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");

    // Bot-only edit: population changes, connection survives, hash unchanged.
    game["archetypes"][0]["count"] = 30.into();
    let (s, v) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, v["reset"].clone()), (200, false.into()), "{v}");
    std::thread::sleep(Duration::from_millis(150));
    let st = state(&srv);
    assert_eq!(st["clients"], 1);
    let drifters = st["e"].as_array().unwrap().iter().filter(|e| e[3] == 0).count();
    assert_eq!(drifters, 30);
    c.send_input(0, 0);
    assert!(c.pump(300) > 0, "client should still be connected");

    // Attribute edit: new wire schema, so the world restarts and clients are dropped.
    game["attrs"][0]["max"] = 250.into();
    game["attrs"][0]["default"] = 250.into();
    let (s, v) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, v["reset"].clone()), (200, true.into()), "{v}");
    assert_eq!(v["dropped_clients"], 1);
    assert_eq!(state(&srv)["clients"], 0);
    // The old client (old content hash) is now refused; a client with the new definitions gets in.
    assert!(matches!(connect_with(&srv, Some(server_key(&srv)), defs().content_hash()), Outcome::Rejected(r) if r == reject::CONTENT_MISMATCH));
    // The edit was written back to game.json.
    let saved = std::fs::read_to_string(srv.dir.join("game.json")).unwrap();
    assert!(saved.contains("250"));
}

#[test]
fn spawn_drive_and_kill_bots_through_the_admin_api() {
    let srv = start(&[]);
    let (_, game) = json(&srv, "GET", "/api/game", "");
    let _ = game;
    // Start from a clean slate.
    json(&srv, "POST", "/api/kill", r#"{"all_bots":true}"#);
    let (s, v) = json(&srv, "POST", "/api/spawn", r#"{"archetype":"drifter","count":3}"#);
    assert_eq!((s, v["spawned"].clone()), (200, 3.into()));
    assert_eq!(json(&srv, "POST", "/api/spawn", r#"{"archetype":"nope"}"#).0, 400);
    let st = state(&srv);
    let ents = st["e"].as_array().unwrap();
    assert_eq!(ents.len(), 3);
    let id = ents[0][0].as_u64().unwrap();
    let (x0, y0) = (ents[0][1].as_f64().unwrap(), ents[0][2].as_f64().unwrap());
    // Possess it and walk for a second.
    for _ in 0..12 {
        let (s, _) = json(&srv, "POST", "/api/drive", &format!(r#"{{"id":{id},"dx":1,"dy":1}}"#));
        assert_eq!(s, 200);
        std::thread::sleep(Duration::from_millis(100));
    }
    let st = state(&srv);
    let me = st["e"].as_array().unwrap().iter().find(|e| e[0].as_u64() == Some(id)).unwrap().clone();
    assert_eq!(me[4], 2, "possessed entities are marked driven");
    let moved = ((me[1].as_f64().unwrap() - x0).powi(2) + (me[2].as_f64().unwrap() - y0).powi(2)).sqrt();
    assert!(moved > 2.0, "moved only {moved}");
    json(&srv, "POST", "/api/release", &format!(r#"{{"id":{id}}}"#));
    // A player entity cannot be driven or killed from the admin side.
    let c = connect(&srv);
    assert_eq!(json(&srv, "POST", "/api/drive", &format!(r#"{{"id":{},"dx":1}}"#, c.entity)).0, 409);
    assert_eq!(json(&srv, "POST", "/api/kill", &format!(r#"{{"id":{}}}"#, c.entity)).0, 409);
    // Kill one, then the rest.
    assert_eq!(json(&srv, "POST", "/api/kill", &format!(r#"{{"id":{id}}}"#)).1["killed"], 1);
    assert_eq!(json(&srv, "POST", "/api/kill", r#"{"all_bots":true}"#).1["killed"], 2);
}

#[test]
fn spawned_bots_survive_game_edits() {
    let srv = start(&[]);
    let bots = |srv: &_| state(srv)["e"].as_array().unwrap().iter().filter(|e| e[3].as_i64().unwrap_or(-1) >= 0).count();
    let before = bots(&srv);
    assert_eq!(json(&srv, "POST", "/api/spawn", r#"{"archetype":"drifter","count":25}"#).1["spawned"], 25);
    assert_eq!(bots(&srv), before + 25);
    // A bot-only edit (a colour) applies live and keeps every spawned bot.
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["archetypes"][0]["color"] = "#123456".into();
    let (s, v) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, v["reset"].clone()), (200, false.into()));
    assert_eq!(bots(&srv), before + 25);
    // A definitions edit restarts the world, and the bots still carry over (matched by name).
    game["max_visible"] = 60.into();
    let (s, v) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, v["reset"].clone()), (200, true.into()));
    assert_eq!(bots(&srv), before + 25);
}

#[test]
fn bot_archetypes_behave_as_defined() {
    // One drone (chases and zaps) next to one civilian (flees): the drone should close in and hurt it.
    let srv = start(&[]);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    for a in game["archetypes"].as_array_mut().unwrap() {
        a["count"] = 0.into();
    }
    let (s, _) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(s, 200);
    json(&srv, "POST", "/api/spawn", r#"{"archetype":"drone","count":1,"x":100,"y":100}"#);
    json(&srv, "POST", "/api/spawn", r#"{"archetype":"civilian","count":1,"x":110,"y":100}"#);
    std::thread::sleep(Duration::from_millis(2500));
    let st = state(&srv);
    assert!(st["game"]["zaps"].as_u64().unwrap() > 0, "drone never attacked: {}", st["game"]);
    let civ_health = st["e"].as_array().unwrap().iter().filter(|e| e[3] == 1).map(|e| e[5].as_f64().unwrap()).next();
    // Either it is hurt, or it already died and respawned (that is counted).
    assert!(civ_health.map_or(false, |h| h < 100.0) || st["game"]["respawns"].as_u64().unwrap() > 0, "{st}");
}

#[test]
fn shouts_reach_everyone_in_order_and_text_is_sanitised() {
    let srv = start(&[]);
    let mut a = connect(&srv);
    let mut b = connect(&srv);
    a.say(proto::chat::channel::SHOUT, "  hello\u{7}   world ");
    for _ in 0..20 {
        a.tick();
        b.tick();
    }
    let texts: Vec<_> = b.heard.iter().map(|m| m.text.as_str()).collect();
    assert_eq!(texts, ["hello world"], "heard by the other player exactly once, cleaned up");
    assert_eq!(b.heard[0].from, a.entity);
    assert_eq!(a.heard.len(), 1, "the speaker hears themselves too");
    assert_eq!(a.chat.unsent(), 0, "server acknowledged the line");
    let s = state(&srv);
    assert_eq!(s["clients"], 2);
}

#[test]
fn chat_is_rate_limited_and_garbage_chat_packets_are_counted() {
    let srv = start(&[]);
    let mut a = connect(&srv);
    let mut b = connect(&srv);
    // Ten lines queued at once: a burst of three plus about a line a second of refill passes, the rest are dropped (all acknowledged).
    for i in 0..10 {
        a.say(proto::chat::channel::SAY, &format!("spam {i}"));
    }
    for _ in 0..60 {
        a.tick();
        b.tick();
    }
    assert!(a.heard.len() <= 6, "burst limit not applied: {}", a.heard.len());
    assert!(a.heard.len() >= 3, "burst of three should pass: {}", a.heard.len());
    assert_eq!(a.chat.unsent(), 0, "dropped lines are still acknowledged so the client moves on");
    // Malformed chat inside a valid encrypted packet is counted, not crashed on.
    let mut plain = vec![proto::chat::kind::SEND, 1, 0, 0, 0, 9, 3, b'a'];
    let mut pkt = Vec::new();
    assert!(a.sess.seal(&plain, &mut pkt));
    a.sock.send(&pkt).unwrap();
    plain = vec![proto::chat::kind::SEND, 9, 0, 0, 0, 0, 2, 0xFF, 0xFE];
    assert!(a.sess.seal(&plain, &mut pkt));
    a.sock.send(&pkt).unwrap();
    a.tick();
    assert_eq!(state(&srv)["clients"], 2, "still serving after malformed chat");
}

#[test]
fn admin_can_make_a_bot_speak_and_read_the_chat_log() {
    let srv = start(&[]);
    let mut p = connect(&srv);
    json(&srv, "POST", "/api/spawn", r#"{"archetype":"drifter","count":1}"#);
    let st = state(&srv);
    let bot = st["e"].as_array().unwrap().iter().find(|e| e[4] == 0).map(|e| e[0].as_u64().unwrap()).expect("a bot exists");
    let (code, _) = json(&srv, "POST", "/api/say", &format!(r#"{{"id":{bot},"shout":true,"text":"beep   boop"}}"#));
    assert_eq!(code, 200);
    for _ in 0..10 {
        p.tick();
    }
    assert_eq!(p.heard.iter().map(|m| m.text.as_str()).collect::<Vec<_>>(), ["beep boop"]);
    assert_eq!(p.heard[0].from as u64, bot);
    let (_, log) = json(&srv, "GET", "/api/chat", "");
    assert_eq!(log["lines"][0][3], "beep boop");
    assert_eq!(json(&srv, "POST", "/api/say", r#"{"id":1,"text":"   "}"#).0, 400);
}

#[test]
fn a_bot_rule_can_make_bots_talk_and_new_places_apply_live() {
    let srv = start(&[]);
    let mut p = connect(&srv);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["places"].as_array_mut().unwrap().push(serde_json::json!({"name":"dock","x":50.0,"y":60.0,"r":10.0}));
    game["archetypes"].as_array_mut().unwrap().push(serde_json::json!({
        "name":"crier","count":2,"home":"dock",
        "rules":[{"when":{"kind":"chance","pct":100.0},"then":{"kind":"say","text":"oyez   oyez","shout":true}},
                 {"when":{"kind":"always"},"then":{"kind":"goto","place":"dock"}}]
    }));
    let (code, body) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(code, 200, "{body}");
    assert_eq!(body["reset"], false, "bot-only edits keep players connected");
    for _ in 0..40 {
        p.tick();
    }
    assert!(p.heard.iter().any(|m| m.text == "oyez oyez"), "heard: {:?}", p.heard);
    let st = state(&srv);
    let near_dock = st["e"].as_array().unwrap().iter().filter(|e| e[3] == 7 && (e[1].as_f64().unwrap() - 50.0).hypot(e[2].as_f64().unwrap() - 60.0) < 12.0).count();
    assert_eq!(near_dock, 2, "both criers spawned at the dock");
    // A rule naming a place that does not exist is refused and changes nothing.
    game["archetypes"][7]["home"] = "atlantis".into();
    let (code, body) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(code, 400);
    assert!(body["error"].as_str().unwrap().contains("unknown place"), "{body}");
}

fn walk(c: &mut Client, ticks: usize) {
    for _ in 0..ticks {
        c.send_input(127, 0);
        c.pump(40);
    }
}

/// Rows in the server's player database (it lives in the server's working directory).
fn saved_rows(srv: &Srv) -> i64 {
    let conn = rusqlite::Connection::open_with_flags(srv.dir.join("data/world.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    conn.query_row("SELECT COUNT(*) FROM players", [], |r| r.get(0)).unwrap()
}

fn wait_for_clients(srv: &Srv, n: u64) {
    let t = Instant::now();
    while state(srv)["clients"].as_u64() != Some(n) {
        assert!(t.elapsed() < Duration::from_secs(8), "clients never reached {n}: {}", state(srv)["clients"]);
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_returning_player_is_put_back_where_they_were() {
    let srv = start(&[]);
    let id = proto::secure::new_identity();
    let mut a = connect_as(&srv, id);
    walk(&mut a, 25);
    a.send_input(0, 0); // stop, so the position we note is the one that gets saved
    a.pump(300);
    let (x, y) = a.pos();
    drop(a); // goes silent; the server times it out and saves it
    wait_for_clients(&srv, 0);
    let mut b = connect_as(&srv, id);
    b.send_input(0, 0); // the first authenticated packet is what proves the identity
    b.pump(300);
    let (bx, by) = b.pos();
    assert!((bx - x).hypot(by - y) < 2.0, "was at ({x:.1},{y:.1}), came back at ({bx:.1},{by:.1})");
    assert_eq!(state(&srv)["net"]["restored"], 1);
    assert_eq!(state(&srv)["store"]["loaded"], 1);
    // Someone else, or a guest, is not that character.
    let mut g = connect(&srv);
    g.send_input(0, 0);
    g.pump(200);
    let (gx, gy) = g.pos();
    assert!((gx - x).hypot(gy - y) > 1.0 || (gx, gy) != (x, y), "a guest should spawn at random");
    assert_eq!(saved_rows(&srv), 1, "guests are never written down");
}

#[test]
fn characters_survive_a_server_restart() {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-restart-{}-{}", std::process::id(), free_udp()));
    std::fs::create_dir_all(&dir).unwrap();
    let id = proto::secure::new_identity();
    let (x, y);
    {
        let srv = start_in(dir.clone(), &[]);
        let mut a = connect_as(&srv, id);
        walk(&mut a, 25);
        a.send_input(0, 0);
        a.pump(300);
        (x, y) = a.pos();
        // SIGTERM, not SIGKILL: the server saves everyone online on the way out.
        let status = Command::new("kill").arg("-TERM").arg(srv.child.id().to_string()).status().unwrap();
        assert!(status.success());
        let mut srv = srv;
        let t = Instant::now();
        while srv.child.try_wait().unwrap().is_none() {
            assert!(t.elapsed() < Duration::from_secs(5), "server did not exit on SIGTERM");
            std::thread::sleep(Duration::from_millis(50));
        }
        // Keep the directory for the second run.
        std::mem::forget(std::mem::replace(&mut srv.dir, PathBuf::from("/nonexistent-keep")));
    }
    let srv = start_in(dir.clone(), &[]);
    let mut b = connect_as(&srv, id);
    b.send_input(0, 0);
    b.pump(300);
    let (bx, by) = b.pos();
    assert!((bx - x).hypot(by - y) < 2.0, "was at ({x:.1},{y:.1}), after restart ({bx:.1},{by:.1})");
    drop(srv);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_same_identity_connecting_again_replaces_the_old_session_once_it_proves_itself() {
    let srv = start(&["--timeout", "30"]);
    let id = proto::secure::new_identity();
    let mut a = connect_as(&srv, id);
    walk(&mut a, 15);
    a.send_input(0, 0);
    a.pump(300);
    let (x, y) = a.pos();
    let mut b = connect_as(&srv, id);
    // Connected but not yet proven: the first session is still there.
    assert_eq!(state(&srv)["clients"], 2);
    b.send_input(0, 0);
    b.pump(300);
    assert_eq!(state(&srv)["clients"], 1, "the proven newcomer replaced the old session");
    assert_eq!(state(&srv)["net"]["takeovers"], 1);
    let (bx, by) = b.pos();
    assert!((bx - x).hypot(by - y) < 3.0, "newcomer continues from where the old session was");
    a.pump(150); // drain what was already in flight
    assert_eq!(a.pump(300), 0, "the replaced session gets nothing more");
}

#[test]
fn an_identity_that_never_sends_a_packet_is_never_saved() {
    let srv = start(&[]);
    let id = proto::secure::new_identity();
    let a = connect_as(&srv, id); // handshake done, but no authenticated DATA ever sent
    wait_for_clients(&srv, 0);
    drop(a);
    assert_eq!(saved_rows(&srv), 0, "an unproven identity must not write a record");
    assert_eq!(state(&srv)["net"]["restored"], 0);
}

#[test]
fn handshake_floods_from_one_address_are_rate_limited_but_established_players_are_not() {
    let srv = start(&["--hello-rate", "5", "--limit-loopback"]);
    let mut p = connect(&srv);
    p.send_input(0, 0);
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.connect(srv.udp).unwrap();
    let hello = ClientHs::new(None, 0).hello();
    for _ in 0..200 {
        s.send(&hello).unwrap();
    }
    std::thread::sleep(Duration::from_millis(400));
    let st = state(&srv);
    assert!(st["net"]["rate_limited"].as_u64().unwrap() >= 150, "{}", st["net"]);
    assert!(st["net"]["hellos"].as_u64().unwrap() <= 60, "{}", st["net"]);
    // Gameplay traffic is not subject to it.
    for _ in 0..50 {
        p.send_input(127, 0);
    }
    assert!(p.pump(500) > 0, "an established player keeps getting snapshots");
}

#[test]
fn one_address_cannot_mint_unlimited_identities() {
    let srv = start(&["--limit-loopback", "--hello-rate", "0", "--new-identities-per-hour", "3"]);
    let mut kept = Vec::new();
    for _ in 0..3 {
        let mut c = connect_as(&srv, proto::secure::new_identity());
        c.send_input(0, 0);
        c.pump(100);
        kept.push(c);
    }
    // The fourth new identity from this address is cut off as soon as it proves itself.
    let mut c4 = connect_as(&srv, proto::secure::new_identity());
    c4.send_input(0, 0);
    c4.pump(300);
    let st = state(&srv);
    assert_eq!(st["net"]["identity_limited"], 1, "{}", st["net"]);
    assert_eq!(st["clients"], 3);
    assert_eq!(c4.pump(200), 0, "the refused session gets nothing");
    // A returning identity is not "new", so it is still welcome, and so are guests.
    let mut guest = connect(&srv);
    guest.send_input(0, 0);
    assert!(guest.pump(300) > 0);
}

#[test]
fn logging_in_with_the_same_account_and_passphrase_from_a_fresh_client_returns_the_character() {
    let srv = start(&[]);
    let id = proto::secure::identity_from_passphrase("Justyn", "a long passphrase nobody would guess");
    let mut a = connect_as(&srv, id);
    walk(&mut a, 25);
    a.send_input(0, 0);
    a.pump(300);
    let (x, y) = a.pos();
    drop(a);
    wait_for_clients(&srv, 0);
    // A different "device": nothing but the account name and passphrase carried over.
    let again = proto::secure::identity_from_passphrase("justyn", "a long passphrase nobody would guess");
    let mut b = connect_as(&srv, again);
    b.send_input(0, 0);
    b.pump(300);
    let (bx, by) = b.pos();
    assert!((bx - x).hypot(by - y) < 2.0, "was at ({x:.1},{y:.1}), logged back in at ({bx:.1},{by:.1})");
    // Lost the passphrase but kept the recovery code: same character again.
    let code = proto::secure::recovery_code(&id);
    drop(b);
    wait_for_clients(&srv, 0);
    let mut c = connect_as(&srv, proto::secure::parse_recovery_code(&code).unwrap());
    c.send_input(0, 0);
    c.pump(300);
    let (cx, cy) = c.pos();
    assert!((cx - x).hypot(cy - y) < 2.0);
    // A wrong passphrase is simply a different, empty character.
    let mut w = connect_as(&srv, proto::secure::identity_from_passphrase("justyn", "wrong"));
    w.send_input(0, 0);
    w.pump(300);
    let (wx, wy) = w.pos();
    assert!((wx - x).hypot(wy - y) > 0.5);
}

/// Stop a server with SIGTERM (it saves on the way out) and keep its directory for a restart.
fn term(mut srv: Srv) -> PathBuf {
    let status = Command::new("kill").arg("-TERM").arg(srv.child.id().to_string()).status().unwrap();
    assert!(status.success());
    let t = Instant::now();
    while srv.child.try_wait().unwrap().is_none() {
        assert!(t.elapsed() < Duration::from_secs(5), "server did not exit on SIGTERM");
        std::thread::sleep(Duration::from_millis(50));
    }
    let dir = srv.dir.clone();
    std::mem::forget(std::mem::replace(&mut srv.dir, PathBuf::from("/nonexistent-keep")));
    dir
}

fn bots_of(srv: &Srv, arch: i64) -> usize {
    state(srv)["e"].as_array().unwrap().iter().filter(|e| e[3] == arch).count()
}

#[test]
fn the_bot_population_survives_a_restart_and_fresh_world_ignores_it() {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-world-{}-{}", std::process::id(), free_udp()));
    std::fs::create_dir_all(&dir).unwrap();
    let hounds = 5; // archetype 5 in the default game, 3 by default
    let srv = start_in(dir.clone(), &[]);
    assert_eq!(bots_of(&srv, 5), 3);
    let (code, body) = json(&srv, "POST", "/api/spawn", r#"{"archetype":"hound","count":4}"#);
    assert_eq!(code, 200, "{body}");
    assert_eq!(bots_of(&srv, 5), 3 + 4);
    let dir = term(srv);
    let srv = start_in(dir.clone(), &[]);
    assert_eq!(bots_of(&srv, 5), 3 + 4, "bots added by hand come back after a restart (of {hounds} kinds)");
    let dir = term(srv);
    let srv = start_in(dir.clone(), &["--fresh-world"]);
    assert_eq!(bots_of(&srv, 5), 3, "--fresh-world starts from the archetype counts");
    drop(srv);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_world_restart_from_an_attribute_edit_still_saves_the_characters_in_it() {
    let srv = start(&[]);
    let id = proto::secure::new_identity();
    let mut a = connect_as(&srv, id);
    walk(&mut a, 25);
    a.send_input(0, 0);
    a.pump(300);
    let (x, y) = a.pos();
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["attrs"][0]["max"] = 250.into();
    game["attrs"][0]["default"] = 250.into();
    let (s, v) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, v["reset"].clone()), (200, true.into()), "{v}");
    // The character was written down from the old world, not from the blank new one.
    let conn = rusqlite::Connection::open_with_flags(srv.dir.join("data/world.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let t = Instant::now();
    let rec: serde_json::Value = loop {
        if let Ok(text) = conn.query_row("SELECT data FROM players", [], |r| r.get::<_, String>(0)) {
            break serde_json::from_str(&text).unwrap();
        }
        assert!(t.elapsed() < Duration::from_secs(5), "nothing was saved");
        std::thread::sleep(Duration::from_millis(50));
    };
    let (sx, sy) = (rec["x"].as_f64().unwrap(), rec["y"].as_f64().unwrap());
    assert!((sx - x as f64).hypot(sy - y as f64) < 2.0, "saved ({sx:.1},{sy:.1}), was at ({x:.1},{y:.1})");
}

// ---------------------------------------------------------------------------
// Web gateway

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

fn start_web(extra: &[&str]) -> (Srv, SocketAddr) {
    let w: SocketAddr = format!("127.0.0.1:{}", free_tcp()).parse().unwrap();
    let w_s = w.to_string();
    let mut args = vec!["--web", w_s.as_str()];
    args.extend_from_slice(extra);
    let srv = start(&args);
    let t = Instant::now();
    while std::net::TcpStream::connect(w).is_err() {
        assert!(t.elapsed() < Duration::from_secs(5), "web gateway never came up");
        std::thread::sleep(Duration::from_millis(50));
    }
    (srv, w)
}

fn ws_open(web: SocketAddr, headers: &[(&str, &str)]) -> Result<Ws, tungstenite::Error> {
    use tungstenite::client::IntoClientRequest;
    let mut req = format!("ws://{web}/ws").into_client_request().unwrap();
    for (k, v) in headers {
        req.headers_mut().insert(tungstenite::http::HeaderName::from_bytes(k.as_bytes()).unwrap(), v.parse().unwrap());
    }
    let (ws, _) = tungstenite::connect(req)?;
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = ws.get_ref() {
        s.set_read_timeout(Some(Duration::from_millis(200))).unwrap();
    }
    Ok(ws)
}

/// The browser's part of a session, played from Rust over the WebSocket: the datagrams are the same.
struct WebClient {
    ws: Ws,
    sess: Session,
    entity: u32,
    view: ClientView,
    seq: u32,
}

fn ws_recv(ws: &mut Ws) -> Option<Vec<u8>> {
    match ws.read() {
        Ok(tungstenite::Message::Binary(b)) => Some(b),
        _ => None,
    }
}

fn web_connect(srv: &Srv, web: SocketAddr, headers: &[(&str, &str)], identity: Option<[u8; 32]>) -> Option<WebClient> {
    let mut ws = ws_open(web, headers).ok()?;
    let mut hs = match identity {
        Some(id) => ClientHs::with_identity(Some(server_key(srv)), defs().content_hash(), id),
        None => ClientHs::new(Some(server_key(srv)), defs().content_hash()),
    };
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(4) {
        let out = match hs.connect_packet() {
            Some(c) => c.to_vec(),
            None => hs.hello(),
        };
        ws.send(tungstenite::Message::Binary(out)).ok()?;
        if let Some(b) = ws_recv(&mut ws) {
            match b.first().copied() {
                Some(wire::kind::CHALLENGE) => {
                    hs.on_challenge(&b).ok()?;
                }
                Some(wire::kind::ACCEPT) => {
                    let (sess, w) = hs.on_accept(&b)?;
                    let Some(wire::ServerMsg::Welcome(w)) = wire::decode_server(&w) else { return None };
                    return Some(WebClient { ws, sess, entity: w.entity, view: ClientView::new(), seq: 0 });
                }
                _ => {}
            }
        }
    }
    None
}

impl WebClient {
    fn input(&mut self, dx: i8, dy: i8) {
        self.seq += 1;
        let mut plain = Vec::new();
        wire::encode_input(&mut plain, &Input { seq: self.seq, ack_tick: self.view.last_tick, dir_x: dx, dir_y: dy, action: 0, target: 0, arg: 0, chat_ack: 0 });
        let mut pkt = Vec::new();
        assert!(self.sess.seal(&plain, &mut pkt));
        self.ws.send(tungstenite::Message::Binary(pkt)).unwrap();
    }

    fn pump(&mut self, ms: u64) -> usize {
        let (mut ok, mut plain, d) = (0, Vec::new(), defs());
        let t = Instant::now();
        while t.elapsed() < Duration::from_millis(ms) {
            if let Some(b) = ws_recv(&mut self.ws) {
                if let Some(inner) = self.sess.open(&b, &mut plain) {
                    if inner.first() == Some(&wire::kind::SNAPSHOT) && decode_snapshot(inner, &d, &mut self.view).is_ok() {
                        ok += 1;
                    }
                }
            }
        }
        ok
    }

    fn pos(&self) -> (f32, f32) {
        let e = self.view.get(self.entity).expect("own entity in snapshot");
        (proto::world::dpos(e.x), proto::world::dpos(e.y))
    }
}

#[test]
fn a_browser_can_play_through_the_web_gateway() {
    let (srv, web) = start_web(&[]);
    // The public info page carries what a browser needs, and nothing from the admin API.
    let (code, body) = {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(web).unwrap();
        s.write_all(b"GET /play/info HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let code: u16 = out[9..12].parse().unwrap();
        (code, out.split("\r\n\r\n").nth(1).unwrap().to_string())
    };
    assert_eq!(code, 200);
    let info: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["server_key"].as_str().unwrap(), proto::secure::to_hex(&server_key(&srv)));
    assert_eq!(info["content_hash"].as_str().unwrap(), format!("{:016x}", defs().content_hash()));
    assert!(!info["boxes"].as_array().unwrap().is_empty());
    assert!(info.get("attrs").is_some());
    // The admin API is not reachable on the public port.
    {
        use std::io::{Read, Write};
        let mut s = std::net::TcpStream::connect(web).unwrap();
        s.write_all(b"GET /api/state HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        assert!(out.starts_with("HTTP/1.1 404"), "{}", &out[..40.min(out.len())]);
    }

    // A full encrypted session over the WebSocket: handshake, inputs, snapshots, movement.
    let id = proto::secure::identity_from_passphrase("webby", "a long passphrase for a browser test");
    let mut c = web_connect(&srv, web, &[], Some(id)).expect("handshake over the gateway");
    c.input(0, 0);
    assert!(c.pump(300) > 0, "no snapshots came back");
    let (x0, y0) = c.pos();
    for _ in 0..25 {
        c.input(127, 0);
        c.pump(50);
    }
    let (x1, y1) = c.pos();
    assert!((x1 - x0).hypot(y1 - y0) > 5.0, "player did not move: ({x0},{y0}) to ({x1},{y1})");
    assert_eq!(state(&srv)["web"]["open"], 1);
    drop(c);
    // The identity was recognised and saved like any other player.
    wait_for_clients(&srv, 0);
    assert_eq!(saved_rows(&srv), 1);
}

#[test]
fn the_gateway_refuses_cross_site_pages_and_too_many_connections_per_address() {
    let (_srv, web) = start_web(&["--web-per-ip", "3"]);
    let host = web.to_string();
    // A page from another origin cannot open a game connection from a visitor's browser.
    assert!(ws_open(web, &[("Origin", "https://evil.example")]).is_err());
    // The page's own origin is fine.
    let _ok = ws_open(web, &[("Origin", &format!("http://{host}"))]).expect("same origin");
    let _b = ws_open(web, &[]).expect("second");
    let _c = ws_open(web, &[]).expect("third");
    assert!(ws_open(web, &[]).is_err(), "the fourth connection from one address is refused");
}

/// A player handed over from another zone arrives with that zone's page still open, so the zones' own pages
/// (`--zone-web`) may connect; any other site still may not.
#[test]
fn the_gateway_lets_the_other_zones_pages_connect() {
    let (_srv, web) = start_web(&["--zone-web", "http://zone-a.example:8080,https://zone-b.example/"]);
    let _a = ws_open(web, &[("Origin", "http://zone-a.example:8080")]).expect("a zone's page");
    let _b = ws_open(web, &[("Origin", "https://zone-b.example")]).expect("another zone's page");
    assert!(ws_open(web, &[("Origin", "https://evil.example")]).is_err());
}

#[test]
fn web_players_are_limited_by_their_real_address_not_the_gateways() {
    // Behind a proxy the real address arrives in X-Forwarded-For. Two new identities per
    // address per hour: a third from the same address is cut off, another address is not.
    let (srv, web) = start_web(&["--web-trust-proxy", "--limit-loopback", "--new-identities-per-hour", "2", "--hello-rate", "0", "--web-per-ip", "50"]);
    let mut keep = Vec::new();
    for _ in 0..3 {
        let mut c = web_connect(&srv, web, &[("X-Forwarded-For", "203.0.113.7")], Some(proto::secure::new_identity())).expect("connect");
        c.input(0, 0);
        c.pump(150);
        keep.push(c);
    }
    std::thread::sleep(Duration::from_millis(200));
    let st = state(&srv);
    assert_eq!(st["net"]["identity_limited"], 1, "{}", st["net"]);
    assert_eq!(st["clients"], 2);
    let mut other = web_connect(&srv, web, &[("X-Forwarded-For", "203.0.113.8")], Some(proto::secure::new_identity())).expect("connect");
    other.input(0, 0);
    assert!(other.pump(300) > 0, "a different real address is not affected");
}

// ---------------------------------------------------------------- scale out

const ZONE_SECRET: &str = "test-zone-secret-0123456789";

struct Zones {
    z: Vec<Srv>,
    listen: Vec<u16>,
}

/// Two zone processes sharing one server key, each with its own database.
fn start_zones() -> Zones {
    start_zones_with(false)
}

/// `open` = no walls at all, so movement in a test is never blocked.
fn start_zones_with(open: bool) -> Zones {
    let games = if open {
        let mut g = GameFile::default_game();
        g.walls = Some(vec![]);
        vec![Some(g.clone()), Some(g)]
    } else {
        vec![None, None]
    };
    start_zones_cfg(games, &[])
}

/// Zones with a game file each (None = the default), and more flags for every one of them.
fn start_zones_cfg(games: Vec<Option<GameFile>>, extra: &[&str]) -> Zones {
    let n = games.len();
    let udp: Vec<u16> = (0..n).map(|_| free_udp()).collect();
    let listen: Vec<u16> = (0..n).map(|_| free_tcp()).collect();
    let peers = listen.iter().map(|p| format!("127.0.0.1:{p}")).collect::<Vec<_>>().join(",");
    let public = udp.iter().map(|p| format!("127.0.0.1:{p}")).collect::<Vec<_>>().join(",");
    let mut z = Vec::new();
    let key = proto::secure::new_identity(); // any 32 random bytes will do as a server secret
    let key_hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    for i in 0..n {
        let dir = std::env::temp_dir().join(format!("mmo-e2e-zone{i}-{}-{}", std::process::id(), free_udp()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("server.key"), &key_hex).unwrap();
        if let Some(g) = &games[i] {
            std::fs::write(dir.join("game.json"), g.to_json()).unwrap();
        }
        let h = free_tcp();
        let child = Command::new(env!("CARGO_BIN_EXE_server"))
            .current_dir(&dir)
            .env("MMO_ZONE_SECRET", ZONE_SECRET)
            .args(["--port", &udp[i].to_string(), "--admin", &format!("127.0.0.1:{h}")])
            .args(["--world-size", "512", "--quiet", "--timeout", "2"])
            .args(["--zone", &format!("{i}/{n}"), "--zone-listen", &format!("127.0.0.1:{}", listen[i]), "--zone-peers", &peers, "--zone-public", &public])
            .args(extra)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn zone");
        let srv = Srv { child, udp: format!("127.0.0.1:{}", udp[i]).parse().unwrap(), http: format!("127.0.0.1:{h}").parse().unwrap(), dir };
        let t = Instant::now();
        while !(TcpStream::connect(srv.http).is_ok() && http(&srv, "GET", "/api/info", "").0 == 200) {
            assert!(t.elapsed() < Duration::from_secs(10), "zone {i} did not come up");
            std::thread::sleep(Duration::from_millis(50));
        }
        z.push(srv);
    }
    Zones { z, listen }
}

/// Two zone-line maps, "town" and "crypt", on open ground. Town has a door to the crypt and a well that drops you
/// elsewhere in town; the crypt has stairs back up, and you arrive standing on them.
fn start_zone_lines() -> Zones {
    let place = |name: &str, x: f32, y: f32, r: f32, exit: Option<(&str, f32, f32)>| proto::game::Place {
        name: name.into(),
        x,
        y,
        r,
        title: String::new(),
        exit: exit.map(|(zone, x, y)| proto::game::Exit { zone: zone.into(), x, y }),
        sound: String::new(),
        music: String::new(),
        safe: false,
    };
    let mut town = GameFile::default_game();
    town.walls = Some(vec![]);
    town.places.push(place("crypt door", 100.0, 100.0, 6.0, Some(("crypt", 300.0, 300.0))));
    town.places.push(place("well", 100.0, 200.0, 4.0, Some(("", 400.0, 400.0))));
    let mut crypt = GameFile::default_game();
    crypt.walls = Some(vec![]);
    crypt.places.push(place("stairs up", 300.0, 300.0, 6.0, Some(("town", 120.0, 100.0))));
    start_zones_cfg(vec![Some(town), Some(crypt)], &["--zone-lines", "--zone-names", "town,crypt"])
}

/// Steer straight at (x, y) until `done` says stop (or the time runs out).
fn walk_to(c: &mut Client, x: f32, y: f32, mut done: impl FnMut(&Client) -> bool) {
    let t = Instant::now();
    while !done(c) {
        // A random spawn can be across the world: 1,400 units at 20 a second is over a minute.
        assert!(t.elapsed() < Duration::from_secs(90), "never got there: at {:?}, heading for ({x}, {y})", c.pos());
        let (px, py) = c.pos();
        let (dx, dy) = (x - px, y - py);
        let d = dx.hypot(dy).max(0.001);
        c.send_input((dx / d * 127.0) as i8, (dy / d * 127.0) as i8);
        c.pump(40);
    }
}

#[test]
fn zone_lines_take_a_player_through_an_exit_to_another_map_and_signing_in_finds_them_there() {
    let zs = start_zone_lines();
    assert_eq!((state(&zs.z[0])["zone"]["lines"].clone(), state(&zs.z[1])["zone"]["names"][1].clone()), (true.into(), "crypt".into()));
    let id = proto::secure::new_identity();
    let mut a = connect_as(&zs.z[0], id);
    a.send_input(0, 0);
    a.pump(200);
    walk_to(&mut a, 100.0, 100.0, |c| c.redirect.is_some());
    let r = a.redirect.unwrap();
    assert_eq!((r.zone, r.port), (1, zs.z[1].udp.port()));

    // In the crypt, where the door said, standing on the stairs up: arriving in an exit does not take you through.
    let mut b = connect_as(&zs.z[1], id);
    b.send_input(0, 0);
    b.pump(1500);
    let (bx, by) = b.pos();
    assert!((bx - 300.0).hypot(by - 300.0) < 1.0, "arrived at ({bx:.1}, {by:.1})");
    assert!(b.redirect.is_none(), "sent straight back up the stairs");
    assert_eq!(state(&zs.z[1])["net"]["restored"], 1);
    wait_for_clients(&zs.z[0], 0);
    drop(b);
    wait_for_clients(&zs.z[1], 0);

    // Signing in to the town again: it only has a note that they went to the crypt, so it sends them there...
    let mut c = connect_as(&zs.z[0], id);
    c.send_input(0, 0);
    let t = Instant::now();
    while c.redirect.is_none() {
        assert!(t.elapsed() < Duration::from_secs(5), "the town kept a player who lives in the crypt");
        c.send_input(0, 0);
        c.pump(50);
    }
    assert_eq!(c.redirect.unwrap().zone, 1);
    assert_eq!(state(&zs.z[0])["net"]["sent_on"], 1);
    // ...where the crypt has them as it saved them.
    let mut d = connect_as(&zs.z[1], id);
    d.send_input(0, 0);
    d.pump(300);
    let (dx, dy) = d.pos();
    assert!((dx - 300.0).hypot(dy - 300.0) < 1.0, "the crypt lost them: ({dx:.1}, {dy:.1})");

    // And back up the stairs (step off them first, then on): the town has them again, at the top.
    walk_to(&mut d, 300.0, 320.0, |c| (c.pos().1 - 300.0) > 10.0);
    walk_to(&mut d, 300.0, 300.0, |c| c.redirect.is_some());
    assert_eq!(d.redirect.unwrap().zone, 0);
    let mut e = connect_as(&zs.z[0], id);
    e.send_input(0, 0);
    e.pump(300);
    let (ex, ey) = e.pos();
    assert!((ex - 120.0).hypot(ey - 100.0) < 1.0, "came up at ({ex:.1}, {ey:.1})");
}

#[test]
fn an_exit_on_the_same_map_moves_anyone_and_a_guest_cannot_leave_the_map() {
    let zs = start_zone_lines();
    let mut g = connect(&zs.z[0]);
    g.send_input(0, 0);
    g.pump(200);
    walk_to(&mut g, 100.0, 100.0, |c| c.heard.iter().any(|m| m.from == 0 && m.text.contains("signed-in")));
    assert!(g.redirect.is_none());
    walk_to(&mut g, 100.0, 200.0, |c| (c.pos().0 - 400.0).hypot(c.pos().1 - 400.0) < 1.0);
}

fn bot_xs(srv: &Srv) -> Vec<f64> {
    state(srv)["e"].as_array().unwrap().iter().filter(|r| r[4] == 0).map(|r| r[1].as_f64().unwrap()).collect()
}

#[test]
fn zone_bots_stay_in_their_strip() {
    let zs = start_zones();
    std::thread::sleep(Duration::from_millis(1500));
    let (a, b) = (bot_xs(&zs.z[0]), bot_xs(&zs.z[1]));
    assert!(a.len() >= 30 && b.len() >= 30, "each zone runs its own bots: {} and {}", a.len(), b.len());
    assert!(a.iter().all(|&x| x <= 256.0), "zone 0 bot outside its strip");
    assert!(b.iter().all(|&x| x >= 256.0), "zone 1 bot outside its strip");
    assert_eq!(state(&zs.z[0])["zone"]["index"], 0);
    assert_eq!(state(&zs.z[1])["zone"]["hi"], 512.0);
}

#[test]
fn a_player_who_crosses_the_border_is_handed_to_the_next_zone_and_keeps_their_character() {
    let zs = start_zones();
    let id = proto::secure::new_identity();
    let mut a = connect_as(&zs.z[0], id);
    a.send_input(0, 0);
    a.pump(200);
    // Walk east, sidestepping walls, until the old zone tells us to go.
    let t = Instant::now();
    let mut last = a.pos();
    let mut detour = 0i8;
    let mut n = 0;
    while a.redirect.is_none() {
        assert!(t.elapsed() < Duration::from_secs(40), "never redirected, stuck at {:?}", a.pos());
        if n % 40 == 39 {
            let p = a.pos();
            detour = if (p.0 - last.0).abs() < 2.0 { if detour >= 0 { -127 } else { 127 } } else { 0 };
            last = p;
        }
        n += 1;
        a.send_input(127, detour);
        a.pump(40);
    }
    let r = a.redirect.unwrap();
    assert_eq!(r.zone, 1);
    assert_eq!(r.port, zs.z[1].udp.port());
    a.send_input(0, 0);
    a.pump(200);
    let (x, y) = a.pos();
    assert!(x > 256.0, "the handoff starts past the border, x = {x}");

    // Reconnect to the new zone with the same identity: same character, same place.
    let mut b = connect_as(&zs.z[1], id);
    b.send_input(0, 0);
    b.pump(400);
    let (bx, by) = b.pos();
    assert!((bx - x).hypot(by - y) < 4.0, "was at ({x:.1},{y:.1}) came back at ({bx:.1},{by:.1})");
    let s1 = state(&zs.z[1]);
    assert_eq!(s1["zone"]["in"], 1);
    assert_eq!(s1["net"]["restored"], 1);
    let s0 = state(&zs.z[0]);
    assert_eq!(s0["zone"]["out"], 1);

    // The old zone lets go after its redirect window and never saves over the new owner.
    let t = Instant::now();
    while state(&zs.z[0])["clients"].as_u64() != Some(0) {
        assert!(t.elapsed() < Duration::from_secs(8), "old zone kept the player");
        a.pump(100);
    }
    // What it keeps is only a note of where the character went, so signing in there sends them on.
    assert_eq!(saved_zones(&zs.z[0]), vec![Some(1)], "the zone they left keeps a note, not the character");
}

/// The zone each saved player is marked as having moved to (None for a character that lives here).
fn saved_zones(srv: &Srv) -> Vec<Option<u64>> {
    let conn = rusqlite::Connection::open_with_flags(srv.dir.join("data/world.db"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut q = conn.prepare("SELECT data FROM players").unwrap();
    let rows = q.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|d| serde_json::from_str::<serde_json::Value>(&d.unwrap()).unwrap()["zone"].as_u64()).collect()
}

#[test]
fn guests_are_stopped_at_the_border() {
    let zs = start_zones();
    let mut g = connect(&zs.z[0]);
    g.send_input(0, 0);
    g.pump(200);
    for _ in 0..140 {
        g.send_input(127, 0);
        g.pump(30);
    }
    // Stop and let prediction settle on what the server says: a step it ran ahead past the border is taken back.
    g.send_input(0, 0);
    g.pump(200);
    assert!(g.pos().0 <= 256.0, "a guest cannot be handed over, so it must stay in its zone: at {:?}", g.pos());
    assert!(g.redirect.is_none());
}

fn zone_frame(secret: &str, from: u8, seq: u64, id: &[u8; 32]) -> Vec<u8> {
    let idhex: String = id.iter().map(|b| format!("{b:02x}")).collect();
    let body = serde_json::to_vec(&serde_json::json!({"from": from, "seq": seq, "id": idhex, "rec": {"x": 100.0, "y": 100.0, "attrs": {}}})).unwrap();
    let key = blake3::derive_key("mmo-engine zone link v1", secret.as_bytes());
    let mac = blake3::keyed_hash(&key, &body);
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&body);
    out.extend_from_slice(mac.as_bytes());
    out
}

#[test]
fn the_handoff_link_refuses_forged_and_replayed_frames() {
    let zs = start_zones();
    let addr: SocketAddr = format!("127.0.0.1:{}", zs.listen[0]).parse().unwrap();
    let id = proto::secure::new_identity();
    let ack = |s: &mut TcpStream| {
        let mut b = [9u8; 1];
        s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        s.read_exact(&mut b).map(|_| b[0]).unwrap_or(255)
    };
    // Wrong secret: refused, and the connection is closed.
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(b"H").unwrap();
    s.write_all(&zone_frame("some-other-secret-value", 1, 10, &id)).unwrap();
    assert_eq!(ack(&mut s), 0);
    // Right secret: taken.
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(b"H").unwrap();
    s.write_all(&zone_frame(ZONE_SECRET, 1, 1_000_000_000_000_000_000, &id)).unwrap();
    assert_eq!(ack(&mut s), 1);
    // The same frame again (a replay) and an older one: refused.
    let mut s2 = TcpStream::connect(addr).unwrap();
    s2.write_all(b"H").unwrap();
    s2.write_all(&zone_frame(ZONE_SECRET, 1, 1_000_000_000_000_000_000, &id)).unwrap();
    assert_eq!(ack(&mut s2), 0);
    let mut s3 = TcpStream::connect(addr).unwrap();
    s3.write_all(b"H").unwrap();
    s3.write_all(&zone_frame(ZONE_SECRET, 1, 5, &id)).unwrap();
    assert_eq!(ack(&mut s3), 0);
    // A frame claiming to come from ourselves: refused.
    let mut s4 = TcpStream::connect(addr).unwrap();
    s4.write_all(b"H").unwrap();
    s4.write_all(&zone_frame(ZONE_SECRET, 0, 2_000_000_000_000_000_000, &id)).unwrap();
    assert_eq!(ack(&mut s4), 0);
    let z = state(&zs.z[0])["zone"].clone();
    assert_eq!(z["in"], 1);
    assert_eq!(z["rejected"], 4);
}

// ---------------------------------------------------------------- C ABI client library

fn play_info(web: SocketAddr) -> String {
    let mut s = std::net::TcpStream::connect(web).unwrap();
    s.write_all(b"GET /play/info HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    out.split("\r\n\r\n").nth(1).unwrap().to_string()
}

mod ffi {
    use mmo_client::*;
    use std::ffi::CString;
    pub struct C(pub *mut Client);
    impl Drop for C {
        fn drop(&mut self) {
            unsafe { mmo_free(self.0) }
        }
    }
    pub fn connect(port: u16, info: &str, identity: Option<&str>) -> C {
        let (h, i) = (CString::new("127.0.0.1").unwrap(), CString::new(info).unwrap());
        let id = identity.map(|s| CString::new(s).unwrap());
        let p = unsafe { mmo_connect(h.as_ptr(), port, i.as_ptr(), id.as_ref().map_or(std::ptr::null(), |s| s.as_ptr())) };
        assert!(!p.is_null(), "{:?}", unsafe { std::ffi::CStr::from_ptr(mmo_last_error()) });
        C(p)
    }
    pub fn me(c: &C) -> Option<MmoEntity> {
        let id = unsafe { mmo_self_id(c.0) };
        (0..unsafe { mmo_entity_count(c.0) }).find_map(|i| {
            let mut e = MmoEntity::default();
            (unsafe { mmo_entity(c.0, i, &mut e) } == 1 && e.id == id).then_some(e)
        })
    }
}

/// Poll the library like an engine would, once per frame, until `done` or the time is up.
fn frames(c: &ffi::C, secs: u64, mut done: impl FnMut(&ffi::C) -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        unsafe { mmo_client::mmo_poll(c.0) };
        if done(c) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
    false
}

/// A world too big to send its walls inline: the library joins with none, asks for the tiles around the player,
/// takes the replies, and says when a reply is for walls that have since changed.
#[test]
fn the_c_client_library_streams_the_walls_by_tile() {
    let (srv, web) = start_web(&["--world-size", "8000"]);
    let info = play_info(web);
    assert!(info.contains(r#""inline":false"#), "{info}");
    let c = ffi::connect(srv.udp.port(), &info, None);
    assert!(frames(&c, 5, |c| ffi::me(c).is_some()), "never in the world");
    assert_eq!(unsafe { mmo_client::mmo_walls_ready(c.0) }, 0, "no tiles yet");
    let me = ffi::me(&c).unwrap();
    let mut buf = [0 as std::ffi::c_char; 2048];
    let n = unsafe { mmo_client::mmo_tiles_wanted(c.0, me.x, me.y, buf.as_mut_ptr(), buf.len()) };
    assert!(n > 0);
    let path = unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_str().unwrap().to_string();
    assert!(path.starts_with("/play/tiles?v="), "{path}");
    assert_eq!(unsafe { mmo_client::mmo_tiles_wanted(c.0, me.x, me.y, buf.as_mut_ptr(), buf.len()) }, 0, "asked once");
    let (st, _, body) = web_get_bytes(web, &path, false);
    assert_eq!(st, 200);
    assert_eq!(unsafe { mmo_client::mmo_put_tiles(c.0, body.as_ptr(), body.len()) }, 1);
    assert_eq!(unsafe { mmo_client::mmo_walls_ready(c.0) }, 1);
    // Too small a buffer is refused without losing the request.
    let mut tiny = [0 as std::ffi::c_char; 8];
    let far = if me.x > 4000.0 { me.x - 3000.0 } else { me.x + 3000.0 };
    assert_eq!(unsafe { mmo_client::mmo_tiles_wanted(c.0, far, me.y, tiny.as_mut_ptr(), tiny.len()) }, -1);
    assert!(unsafe { mmo_client::mmo_tiles_wanted(c.0, far, me.y, buf.as_mut_ptr(), buf.len()) } > 0);
    // The console edits the walls: the server answers the old version with 409, and a fresh /play/info puts the
    // library right.
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["walls"] = serde_json::json!([[10.0, 10.0, 30.0, 30.0]]);
    assert_eq!(http(&srv, "PUT", "/api/game", &game.to_string()).0, 200);
    assert_eq!(web_get_bytes(web, &path, false).0, 409);
    let fresh = std::ffi::CString::new(play_info(web)).unwrap();
    assert_eq!(unsafe { mmo_client::mmo_set_walls(c.0, fresh.as_ptr()) }, 1);
    assert_eq!(unsafe { mmo_client::mmo_set_walls(c.0, fresh.as_ptr()) }, 0, "already has them");
    assert_eq!(unsafe { mmo_client::mmo_walls_ready(c.0) }, 1, "one wall comes inline");
}

#[test]
fn the_c_client_library_plays_moves_hears_chat_and_refuses_a_wrong_key() {
    let (srv, web) = start_web(&[]);
    let info = play_info(web);
    let mut id = [0 as std::ffi::c_char; 65];
    unsafe { mmo_client::mmo_new_identity(id.as_mut_ptr()) };
    let id = unsafe { std::ffi::CStr::from_ptr(id.as_ptr()) }.to_str().unwrap().to_string();

    let c = ffi::connect(srv.udp.port(), &info, Some(&id));
    assert!(frames(&c, 5, |c| unsafe { mmo_client::mmo_status(c.0) } == mmo_client::MMO_PLAYING && ffi::me(c).is_some()), "never in the world");
    assert!(unsafe { mmo_client::mmo_attr_count(c.0) } >= 1);
    let name = unsafe { std::ffi::CStr::from_ptr(mmo_client::mmo_attr_name(c.0, 0)) }.to_str().unwrap().to_string();
    assert_eq!(name, "health");
    let start = ffi::me(&c).unwrap();
    assert!(start.attrs[0] > 0.0);

    // Movement: the server decides the speed, the library just sends intent.
    let sx = start.x;
    let mut best = 0.0f32;
    unsafe { mmo_client::mmo_set_move(c.0, 1, 1) };
    frames(&c, 4, |c| {
        let e = ffi::me(c).unwrap();
        best = best.max((e.x - sx).abs() + (e.y - start.y).abs());
        best > 8.0
    });
    assert!(best > 8.0, "did not move ({best})");
    unsafe { mmo_client::mmo_set_move(c.0, 0, 0) };

    // Chat out and back: a shout is heard by the speaker too.
    let line = std::ffi::CString::new("hello from C").unwrap();
    unsafe { mmo_client::mmo_say(c.0, 1, line.as_ptr()) };
    let mut heard = String::new();
    frames(&c, 4, |c| {
        let mut m = mmo_client::MmoChat { from: 0, channel: 0, text: [0; 256] };
        if unsafe { mmo_client::mmo_next_chat(c.0, &mut m) } == 1 {
            heard = String::from_utf8_lossy(&m.text[..m.text.iter().position(|&b| b == 0).unwrap()]).to_string();
        }
        !heard.is_empty()
    });
    assert_eq!(heard, "hello from C");
    assert!(unsafe { mmo_client::mmo_rtt_ms(c.0) } >= 0.0);

    // A pinned key that is not the server's is refused, not trusted.
    let mut bad: serde_json::Value = serde_json::from_str(&info).unwrap();
    bad["server_key"] = "ab".repeat(32).into();
    let b = ffi::connect(srv.udp.port(), &bad.to_string(), None);
    assert!(frames(&b, 4, |b| unsafe { mmo_client::mmo_status(b.0) } == mmo_client::MMO_REJECTED));
    // Bad input is an error, not a crash.
    let junk = std::ffi::CString::new("{").unwrap();
    let host = std::ffi::CString::new("127.0.0.1").unwrap();
    assert!(unsafe { mmo_client::mmo_connect(host.as_ptr(), 1, junk.as_ptr(), std::ptr::null()) }.is_null());
}

#[test]
fn the_c_client_library_follows_a_zone_handoff_by_itself() {
    let zs = start_zones();
    // Zones have no web gateway here, so build the info the gateway would serve from the admin API.
    let (_, ai) = json(&zs.z[0], "GET", "/api/info", "");
    let (_, g) = json(&zs.z[0], "GET", "/api/game", "");
    let (_, w) = json(&zs.z[0], "GET", "/api/world", "");
    let attrs: Vec<_> = g["attrs"].as_array().unwrap().iter().map(|a| serde_json::json!({"name": a["name"], "min": a["min"], "max": a["max"], "bits": a["bits"], "replicated": a["replicated"]})).collect();
    let replicated: Vec<usize> = attrs.iter().enumerate().filter(|(_, a)| a["replicated"] == true).map(|(i, _)| i).collect();
    let info = serde_json::json!({
        "server_key": ai["server_key"], "content_hash": ai["content_hash"], "proto": ai["proto"], "hz": 20,
        "move_speed": g["move_speed"], "entity_radius": g["entity_radius"], "interest_radius": g["interest_radius"],
        "attrs": attrs, "replicated": replicated,
        "world_size": w["size"], "seed": 1, "boxes": w["boxes"],
    })
    .to_string();
    let mut id = [0 as std::ffi::c_char; 65];
    unsafe { mmo_client::mmo_new_identity(id.as_mut_ptr()) };
    let id = unsafe { std::ffi::CStr::from_ptr(id.as_ptr()) }.to_str().unwrap().to_string();
    let c = ffi::connect(zs.z[0].udp.port(), &info, Some(&id));
    assert!(frames(&c, 5, |c| ffi::me(c).is_some()));
    let first = unsafe { mmo_client::mmo_self_id(c.0) };
    // Walk east, sidestepping walls, until we are playing in zone 1 (x past the border and a new session).
    let mut last = ffi::me(&c).unwrap();
    let mut detour = 0;
    let mut n = 0;
    unsafe { mmo_client::mmo_set_move(c.0, 1, 0) };
    let ok = frames(&c, 45, |c| {
        n += 1;
        if n % 60 == 0 {
            let e = ffi::me(c).unwrap_or(last);
            detour = if (e.x - last.x).abs() < 2.0 { if detour >= 0 { -1 } else { 1 } } else { 0 };
            last = e;
            unsafe { mmo_client::mmo_set_move(c.0, 1, detour) };
        }
        state(&zs.z[1])["zone"]["in"] == 1 && state(&zs.z[1])["clients"].as_u64() == Some(1) && ffi::me(c).is_some_and(|e| e.x > 256.0)
    });
    assert!(ok, "never arrived in zone 1");
    assert_eq!(unsafe { mmo_client::mmo_status(c.0) }, mmo_client::MMO_PLAYING);
    let _ = first; // the entity id is allowed to change across zones
    assert_eq!(state(&zs.z[1])["net"]["restored"], 1);
}

#[test]
fn the_c_client_draws_smooth_motion_between_snapshots() {
    let (srv, web) = start_web(&[]);
    let c = ffi::connect(srv.udp.port(), &play_info(web), None);
    assert!(frames(&c, 5, |c| ffi::me(c).is_some()));
    unsafe { mmo_client::mmo_set_move(c.0, 1, 0) };
    // Let the character get going, then watch 60 fps frames: a client that only moved on
    // snapshots (20 per second) would change position on one frame in three.
    frames(&c, 1, |_| false);
    let mut last = ffi::me(&c).unwrap();
    let (mut n, mut still, mut biggest) = (0, 0, 0.0f32);
    let t = Instant::now();
    while t.elapsed() < Duration::from_millis(1500) {
        unsafe { mmo_client::mmo_poll(c.0) };
        let e = ffi::me(&c).unwrap();
        let d = (e.x - last.x).hypot(e.y - last.y);
        if e.x >= 480.0 {
            break; // walked into the east wall
        }
        n += 1;
        still += (d < 1e-4) as i32;
        biggest = biggest.max(d);
        last = e;
        std::thread::sleep(Duration::from_millis(16));
    }
    assert!(n > 40, "only {n} frames");
    assert!(still * 5 < n, "{still} of {n} frames did not move: motion is stepping");
    assert!(biggest < 2.0, "a frame jumped {biggest} units");
}

// ---------------------------------------------------------------- map editor

#[test]
fn walls_edited_live_apply_without_dropping_players_and_free_anyone_trapped() {
    let srv = start(&[]);
    let mut p = connect(&srv);
    p.send_input(0, 0);
    p.pump(300);
    let (px, py) = p.pos();
    // Wall the player in: a slab covering their position.
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["walls"] = serde_json::json!([[(px - 8.0).max(0.0), (py - 8.0).max(0.0), (px + 8.0).min(511.0), (py + 8.0).min(511.0)]]);
    let (code, res) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(code, 200, "{res}");
    assert_eq!(res["reset"], false, "walls are not a schema change");
    assert_eq!(res["dropped_clients"], 0);
    assert!(res["moved_entities"].as_u64().unwrap() >= 1, "{res}");
    let (_, world) = json(&srv, "GET", "/api/world", "");
    assert_eq!(world["boxes"].as_array().unwrap().len(), 1);
    // Still connected, and no longer inside the wall.
    p.send_input(0, 0);
    p.pump(400);
    let (nx, ny) = p.pos();
    let inside = nx > px - 8.0 && nx < px + 8.0 && ny > py - 8.0 && ny < py + 8.0;
    assert!(!inside, "still inside the new wall at ({nx:.1},{ny:.1})");
    assert_eq!(state(&srv)["clients"], 1);
    // The play page's info now carries the same walls.
    // Bad walls are refused and change nothing.
    game["walls"] = serde_json::json!([[50.0, 50.0, 40.0, 60.0]]);
    let (code, res) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(code, 400, "{res}");
    assert_eq!(json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len(), 1);
    // Removing the key goes back to the generated world.
    game.as_object_mut().unwrap().remove("walls");
    assert_eq!(json(&srv, "PUT", "/api/game", &game.to_string()).0, 200);
    assert!(json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len() > 1);
}

#[test]
fn hand_placed_walls_survive_a_restart() {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-walls-{}-{}", std::process::id(), free_udp()));
    std::fs::create_dir_all(&dir).unwrap();
    let srv = start_in(dir.clone(), &[]);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["walls"] = serde_json::json!([[100.0, 100.0, 140.0, 120.0], [200.0, 50.0, 210.0, 300.0]]);
    assert_eq!(json(&srv, "PUT", "/api/game", &game.to_string()).0, 200);
    let dir = term(srv);
    let srv = start_in(dir, &[]);
    let (_, w) = json(&srv, "GET", "/api/world", "");
    assert_eq!(w["boxes"], serde_json::json!([[100.0, 100.0, 140.0, 120.0], [200.0, 50.0, 210.0, 300.0]]));
}

fn act(c: &mut Client, action: u8, target: u32) {
    act_arg(c, action, target, 0);
}

/// An action with the input's extra argument (a build's or break's layer and material).
fn act_arg(c: &mut Client, action: u8, target: u32, arg: u16) {
    c.seq += 1;
    let mut plain = Vec::new();
    wire::encode_input(&mut plain, &Input { seq: c.seq, ack_tick: c.view.last_tick, dir_x: 0, dir_y: 0, action, target, arg, chat_ack: c.chat.ack() });
    let mut pkt = Vec::new();
    assert!(c.sess.seal(&plain, &mut pkt));
    c.sock.send(&pkt).unwrap();
}

/// The entity in `c`'s view nearest to a point, other than `c` itself, and how far away it is.
fn nearest_other(c: &Client, x: f32, y: f32) -> Option<(u32, f32)> {
    c.view
        .entries()
        .iter()
        .filter(|e| e.id != c.entity)
        .map(|e| (e.id, (proto::world::dpos(e.x) - x).hypot(proto::world::dpos(e.y) - y)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

#[test]
fn players_see_across_a_zone_border_and_cannot_hit_across_it() {
    let zs = start_zones_with(true);
    // No bots: nothing but the zap under test may hurt a player, whatever the dice do.
    for z in &zs.z {
        let (_, mut game) = json(z, "GET", "/api/game", "");
        for a in game["archetypes"].as_array_mut().unwrap() {
            a["count"] = 0.into();
        }
        assert_eq!(http(z, "PUT", "/api/game", &game.to_string()).0, 200);
    }
    // Guests are stopped at their own zone's border, which puts them right beside each other.
    let mut a = connect(&zs.z[0]);
    let mut b = connect(&zs.z[1]);
    a.send_input(0, 0);
    b.send_input(0, 0);
    a.pump(200);
    b.pump(200);
    let t = Instant::now();
    loop {
        assert!(t.elapsed() < Duration::from_secs(40), "never lined up: {:?} {:?}", a.pos(), b.pos());
        let (pa, pb) = (a.pos(), b.pos());
        if pa.0 > 236.0 && pb.0 < 276.0 && (pa.1 - pb.1).abs() < 12.0 {
            break;
        }
        let vy = |from: f32, to: f32| if (to - from).abs() < 6.0 { 0 } else if to > from { 127 } else { -127 };
        a.send_input(127, vy(pa.1, pb.1));
        b.send_input(-127, vy(pb.1, pa.1));
        a.pump(40);
        b.pump(40);
    }
    a.send_input(0, 0);
    b.send_input(0, 0);
    a.pump(500);
    b.pump(500);
    let (pa, pb) = (a.pos(), b.pos());
    let (seen_by_a, da) = nearest_other(&a, pb.0, pb.1).expect("a sees nothing");
    let (seen_by_b, db) = nearest_other(&b, pa.0, pa.1).expect("b sees nothing");
    assert!(da < 3.0, "a should see b's ghost where b is: {pb:?}, nearest is {da} away");
    assert!(db < 3.0, "b should see a's ghost where a is: {pa:?}, nearest is {db} away");
    assert_ne!(seen_by_a, b.entity, "what a sees is a mirror with its own id");
    let s0 = state(&zs.z[0]);
    assert!(s0["zone"]["ghosts"].as_u64().unwrap() >= 1, "{}", s0["zone"]);
    assert!(s0["e"].as_array().unwrap().iter().any(|r| r[4] == 3), "the admin map marks ghosts as kind 3");
    assert_eq!(s0["live"].as_u64().unwrap() as usize, s0["e"].as_array().unwrap().iter().filter(|r| r[4] != 3).count(), "ghosts are not counted as live entities");

    // A zap at the mirror does nothing to the real player on the other side.
    let health = |c: &Client| c.view.get(c.entity).unwrap().attrs[0];
    let before = health(&b);
    for _ in 0..30 {
        act(&mut a, proto::defs::ACTION_ZAP, seen_by_a);
        a.pump(40);
        b.pump(10);
    }
    b.pump(300);
    assert_eq!(health(&b), before, "damage crossed the border");
    assert!(state(&zs.z[0])["game"]["rejected_actions"].as_u64().unwrap() >= 1);
    // Ghosts are never written to the database.
    assert_eq!(saved_rows(&zs.z[0]), 0);
}

#[test]
fn ghosts_disappear_when_the_neighbour_goes_away() {
    let mut zs = start_zones_with(true);
    let mut a = connect(&zs.z[0]);
    let mut b = connect(&zs.z[1]);
    a.send_input(0, 0);
    b.send_input(0, 0);
    // Zone 1's bots near the border are mirrored into zone 0 without anyone doing anything.
    let t = Instant::now();
    while state(&zs.z[0])["zone"]["ghosts"].as_u64() == Some(0) {
        assert!(t.elapsed() < Duration::from_secs(10), "no ghosts ever arrived");
        a.pump(100);
    }
    let _ = &mut b;
    zs.z[1].child.kill().unwrap();
    let t = Instant::now();
    while state(&zs.z[0])["zone"]["ghosts"].as_u64() != Some(0) {
        assert!(t.elapsed() < Duration::from_secs(6), "ghosts of a dead zone stayed: {}", state(&zs.z[0])["zone"]);
        a.pump(100);
    }
    assert!(state(&zs.z[0])["e"].as_array().unwrap().iter().all(|r| r[4] != 3));
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn a_json_file_dropped_in_the_bots_directory_adds_a_bot_live_and_a_broken_one_changes_nothing() {
    let srv = start(&[]);
    assert!(srv.dir.join("bots.d/_example.json").exists(), "the template is created on first start");
    let _player = connect(&srv);
    wait_for_clients(&srv, 1);
    let before = json(&srv, "GET", "/api/game", "").1["archetypes"].as_array().unwrap().len();

    std::fs::write(
        srv.dir.join("bots.d/ghoul.json"),
        r##"{"name":"ghoul","color":"#33ff99","count":3,"behaviour":"chase","speed_mult":0.7,"attack":"zap"}"##,
    )
    .unwrap();
    wait_until("the new bot type to appear", || {
        json(&srv, "GET", "/api/game", "").1["archetypes"].as_array().unwrap().iter().any(|a| a["name"] == "ghoul")
    });
    let g = json(&srv, "GET", "/api/game", "").1;
    let idx = g["archetypes"].as_array().unwrap().iter().position(|a| a["name"] == "ghoul").unwrap();
    assert_eq!(g["archetypes"].as_array().unwrap().len(), before + 1);
    wait_until("three ghouls in the world", || bots_of(&srv, idx as i64) == 3);
    let st = state(&srv);
    assert_eq!(st["clients"], 1, "the player stayed connected");
    assert_eq!(st["bot_dir"]["bots"][0], "ghoul");
    assert!(st["bot_dir"]["error"].is_null(), "{}", st["bot_dir"]);

    // Edit it: the count changes live.
    std::fs::write(
        srv.dir.join("bots.d/ghoul.json"),
        r##"{"name":"ghoul","color":"#33ff99","count":5,"behaviour":"chase","speed_mult":0.7,"attack":"zap"}"##,
    )
    .unwrap();
    wait_until("five ghouls", || bots_of(&srv, idx as i64) == 5);

    // A typo is reported and nothing else moves.
    std::fs::write(srv.dir.join("bots.d/typo.json"), r##"{"name":"oops","colour":"#fff"}"##).unwrap();
    wait_until("the error to be reported", || !state(&srv)["bot_dir"]["error"].is_null());
    let e = state(&srv)["bot_dir"]["error"].as_str().unwrap().to_string();
    assert!(e.contains("typo.json") && e.contains("colour"), "{e}");
    let g2 = json(&srv, "GET", "/api/game", "").1;
    assert!(!g2["archetypes"].as_array().unwrap().iter().any(|a| a["name"] == "oops"));
    assert_eq!(bots_of(&srv, idx as i64), 5);

    // The bot is in the game file now, so it survives without the directory.
    let saved = std::fs::read_to_string(srv.dir.join("game.json")).unwrap();
    assert!(saved.contains("ghoul"));
}

#[test]
fn weather_and_events_are_built_fired_and_reported_live_without_dropping_players() {
    let srv = start(&[]);
    let _player = connect(&srv);
    wait_for_clients(&srv, 1);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    let arch = game["archetypes"][0]["name"].as_str().unwrap().to_string();
    for a in game["archetypes"].as_array_mut().unwrap() {
        a["count"] = 0.into();
    }
    game["weather"] = serde_json::json!([{"name":"fog","color":"#8899aa","intensity":0.9,"sight_mult":0.4,"drain":[]}]);
    game["events"] = serde_json::json!([{"name":"arrival","every_secs":0,"steps":[
        {"kind":"announce","text":"Strangers are arriving"},
        {"kind":"weather","name":"fog","duration_secs":60},
        {"kind":"spawn","archetype":arch,"count":7,"place":"plaza"}
    ]}]);
    let (s, body) = http(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(s, 200, "{body}");
    assert_eq!(state(&srv)["clients"], 1, "adding weather and events does not drop anyone");

    // A mistake is refused with a reason and changes nothing.
    let mut bad = game.clone();
    bad["events"][0]["steps"][2]["place"] = "atlantis".into();
    let (s, body) = http(&srv, "PUT", "/api/game", &bad.to_string());
    assert_eq!(s, 400);
    assert!(body.contains("atlantis"), "{body}");

    // By hand.
    assert_eq!(http(&srv, "POST", "/api/weather", r#"{"name":"hail"}"#).0, 400);
    assert_eq!(http(&srv, "POST", "/api/events/fire", r#"{"name":"nope"}"#).0, 404);
    assert_eq!(http(&srv, "POST", "/api/weather", r#"{"name":"fog","duration_secs":30}"#).0, 200);
    wait_until("fog", || state(&srv)["director"]["weather"]["name"] == "fog");
    assert!(state(&srv)["director"]["weather"]["left_secs"].as_u64().unwrap() <= 30);
    assert_eq!(http(&srv, "POST", "/api/weather", r#"{"name":null}"#).0, 200);
    wait_until("clear sky", || state(&srv)["director"]["weather"].is_null());

    // An event: announcement reaches the chat log from the server, weather starts, bots appear.
    assert_eq!(http(&srv, "POST", "/api/events/fire", r#"{"name":"arrival"}"#).0, 200);
    wait_until("seven arrivals", || bots_of(&srv, 0) == 7);
    let chat = json(&srv, "GET", "/api/chat", "").1;
    assert!(chat["lines"].as_array().unwrap().iter().any(|l| l[1] == 0 && l[3] == "Strangers are arriving"), "{chat}");
    let st = state(&srv);
    assert_eq!(st["director"]["weather"]["name"], "fog");
    assert_eq!(st["director"]["events"][0]["fired"], 1);
    assert!(st["director"]["log"].as_array().unwrap().iter().any(|l| l[1].as_str().unwrap().contains("arrival")));
    assert_eq!(st["clients"], 1);

    // It is in the saved game file.
    let saved = std::fs::read_to_string(srv.dir.join("game.json")).unwrap();
    assert!(saved.contains("arrival") && saved.contains("fog"));
}

#[test]
fn items_are_given_used_dropped_picked_up_and_kept_across_a_reconnect() {
    let srv = start(&["--db", "players.db"]);
    let id = proto::secure::new_identity();
    let mut c = connect_as(&srv, id);
    wait_for_clients(&srv, 1);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    for a in game["archetypes"].as_array_mut().unwrap() {
        a["count"] = 0.into();
    }
    let (s, body) = http(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(s, 200, "{body}");
    let me = c.entity;
    c.pump(300);
    assert!(c.bag.iter().all(|&(t, _)| t == 0), "starts empty: {:?}", c.bag);

    // The play page is told what items exist.
    let info = {
        let (s, b) = http(&srv, "GET", "/api/info", "");
        assert_eq!(s, 200);
        serde_json::from_str::<serde_json::Value>(&b).unwrap()
    };
    assert!(info["server_key"].is_string());
    let names: Vec<&str> = game["items"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"medkit"), "{names:?}");

    // Hurt, then given two medkits: the bag shows them and using one heals and uses it up.
    let (s, _) = http(&srv, "POST", "/api/give", &format!(r#"{{"id":{me},"item":"medkit","count":2}}"#));
    assert_eq!(s, 200);
    let (s, _) = http(&srv, "POST", "/api/give", &format!(r#"{{"id":{me},"item":"nonsense"}}"#));
    assert_eq!(s, 404);
    c.pump(500);
    let medkit = (names.iter().position(|n| *n == "medkit").unwrap() + 1) as u8;
    assert_eq!(c.bag[0], (medkit, 2), "{:?}", c.bag);
    act(&mut c, wire_use(0), 0);
    c.pump(500);
    assert_eq!(c.bag[0], (medkit, 1), "using one used one up");
    act(&mut c, wire_use(3), 0); // an empty slot is refused quietly
    c.pump(300);
    assert_eq!(c.bag[0], (medkit, 1));

    // Drop it: it is on the ground near us, then we pick it back up.
    act(&mut c, proto::game::ACT_DROP, 0);
    c.pump(800);
    assert_eq!(c.bag[0].0, 0, "dropped");
    assert_eq!(c.ground.len(), 1, "{:?}", c.ground);
    assert_eq!(c.ground[0].item, medkit - 1);
    std::thread::sleep(Duration::from_millis(300));
    act(&mut c, proto::game::ACT_PICKUP, 0);
    c.pump(800);
    assert_eq!(c.bag[0], (medkit, 1), "picked up again");
    assert!(c.ground.is_empty(), "{:?}", c.ground);

    // Items scattered by the admin appear on the ground, but not near a player who is far from them.
    let (s, _) = http(&srv, "POST", "/api/ground", r#"{"item":"scrap","count":5,"x":5,"y":5}"#);
    assert_eq!(s, 200);
    assert_eq!(state(&srv)["game"]["ground_items"], 5);

    // The bag is saved with the character and comes back by name.
    drop(c);
    wait_for_clients(&srv, 0);
    let mut c2 = connect_as(&srv, id);
    c2.send_input(0, 0); // the first packet is what proves who it is
    c2.pump(800);
    assert_eq!(c2.bag[0], (medkit, 1), "{:?}", c2.bag);
}

fn wire_use(slot: u8) -> u8 {
    proto::game::ACT_USE + slot
}

#[test]
fn a_world_far_bigger_than_sixteen_bit_positions_could_describe_works_end_to_end() {
    let srv = start(&["--world-size", "20000"]);
    let (_, info) = json(&srv, "GET", "/api/info", "");
    assert_eq!(info["world_size"].as_f64(), Some(20000.0));
    // Guests spawn anywhere in the world; of eight, some land beyond 4,096 on at least one axis.
    let mut clients: Vec<Client> = (0..8).map(|_| connect(&srv)).collect();
    // Keep every one alive (the test server drops the silent after 2 s) while snapshots arrive.
    for _ in 0..8 {
        for c in clients.iter_mut() {
            c.send_input(0, 0);
            c.pump(60);
        }
    }
    wait_for_clients(&srv, 8);
    let st = state(&srv);
    let mut far = 0;
    for c in &clients {
        let (x, y) = c.pos();
        assert!(x > 0.0 && y > 0.0 && x < 20000.0 && y < 20000.0, "{x},{y}");
        // What the client decoded is where the server says the entity is, to the wire's resolution.
        let row = st["e"].as_array().unwrap().iter().find(|r| r[0].as_u64() == Some(c.entity as u64)).expect("server row");
        assert!((row[1].as_f64().unwrap() - x as f64).abs() < 0.1 && (row[2].as_f64().unwrap() - y as f64).abs() < 0.1, "client {x},{y} server {row:?}");
        if x > 4096.0 || y > 4096.0 {
            far += 1;
        }
    }
    assert!(far > 0, "no player landed beyond 4,096 in a 20,000 unit world");
    // Items lie far out too, and are told to someone standing there.
    let (x, y) = clients[0].pos();
    let (s, _) = http(&srv, "POST", "/api/ground", &format!(r#"{{"item":"scrap","count":1,"x":{x},"y":{y}}}"#));
    assert_eq!(s, 200);
    clients[0].pump(1200);
    assert_eq!(clients[0].ground.len(), 1, "{:?}", clients[0].ground);
    let g = clients[0].ground[0];
    assert!((proto::world::dpos(g.x) - x).abs() < 3.0 && (proto::world::dpos(g.y) - y).abs() < 3.0, "{g:?} vs {x},{y}");
}

fn web_get(web: SocketAddr, path: &str) -> (u16, String) {
    let mut s = std::net::TcpStream::connect(web).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, out.split("\r\n\r\n").nth(1).unwrap_or("").to_string())
}

/// A GET's status, head and body as bytes, optionally saying it takes gzip.
fn web_get_bytes(web: SocketAddr, path: &str, gzip: bool) -> (u16, String, Vec<u8>) {
    let mut s = std::net::TcpStream::connect(web).unwrap();
    let enc = if gzip { "Accept-Encoding: gzip, br\r\n" } else { "" };
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n{enc}Connection: close\r\n\r\n").as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    let end = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&out[..end]).into_owned();
    let status = head.split(' ').nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
    (status, head, out[end + 4..].to_vec())
}

#[test]
fn a_big_world_streams_its_walls_by_area_instead_of_sending_them_all() {
    let (srv, web) = start_web(&["--world-size", "8000"]);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    let walls = &info["walls"];
    assert_eq!(walls["inline"], false);
    let count = walls["count"].as_u64().unwrap();
    assert!(count > 600, "{count}");
    assert_eq!(info["boxes"].as_array().unwrap().len(), 0, "walls must not ride along with the join info");
    let ver = walls["ver"].as_str().unwrap().to_string();

    // Three tiles, binary: each box carries its id and appears in each tile it touches. A reply names its version,
    // so it may be cached for good.
    let (st, head, body) = web_get_bytes(web, &format!("/play/tiles?v={ver}&c=0,0;1,0;2,3"), false);
    assert_eq!(st, 200);
    assert!(head.contains("immutable"), "{head}");
    let (v, tiles) = proto::tiles::decode(&body).expect("a tiles reply");
    assert_eq!(format!("{v:016x}"), ver);
    assert_eq!(tiles.len(), 3);
    for ((cx, cy), list, _) in &tiles {
        let (cx, cy) = (*cx as f32, *cy as f32);
        for t in list {
            let b = t.b;
            assert!(b.x1 >= cx * 256.0 && b.x0 < (cx + 1.0) * 256.0 && b.y1 >= cy * 256.0 && b.y0 < (cy + 1.0) * 256.0, "({cx}, {cy}): {b:?}");
        }
    }
    // Gzipped for a client that takes it, and the same once unpacked.
    let all: Vec<String> = (0..32).flat_map(|y| (0..32).map(move |x| format!("{x},{y}"))).collect();
    let row = format!("/play/tiles?v={ver}&c={}", all[..64].join(";"));
    let (_, head, gz) = web_get_bytes(web, &row, true);
    assert!(head.contains("Content-Encoding: gzip"), "{head}");
    let mut plain = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&gz[..]), &mut plain).unwrap();
    assert_eq!(plain, web_get_bytes(web, &row, false).2);
    // Every tile, 64 at a time, holds every wall.
    let mut ids = std::collections::HashSet::new();
    for part in all.chunks(64) {
        let (_, _, body) = web_get_bytes(web, &format!("/play/tiles?v={ver}&c={}", part.join(";")), false);
        ids.extend(proto::tiles::decode(&body).unwrap().1.into_iter().flat_map(|(_, l, _)| l.into_iter().map(|t| t.id)));
    }
    assert_eq!(ids.len() as u64, count);
    // The console's view agrees with what the browsers get.
    let (_, g) = json(&srv, "GET", "/api/world", "");
    assert_eq!(g["boxes"].as_array().unwrap().len() as u64, count);
    assert_eq!(g["class"].as_array().unwrap().len() as u64, count);

    // Asking for the whole world a tile at a time is refused, and so is an old version.
    let many: Vec<String> = (0..65).map(|i| format!("{i},0")).collect();
    assert_eq!(web_get(web, &format!("/play/tiles?c={}", many.join(";"))).0, 400);
    assert_eq!(web_get(web, "/play/tiles?v=0123456789abcdef&c=0,0").0, 409);

    // Editing the walls changes the version, so open pages know to start over.
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["walls"] = serde_json::json!([[10.0, 10.0, 30.0, 30.0]]);
    let (s, b) = http(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(s, 200, "{b}");
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    assert_ne!(info["walls"]["ver"].as_str().unwrap(), ver);
    assert_eq!(info["walls"]["inline"], true, "one wall is small enough to send inline");
    assert_eq!(info["boxes"].as_array().unwrap().len(), 1);
}

#[test]
fn a_world_file_supplies_the_walls_is_served_as_is_and_reloads_live() {
    let dir = std::env::temp_dir().join(format!("e2e-world-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("world.json");
    let file = |extra: &str| {
        format!(
            r#"{{"format":1,"size":1024,"heights":{{"anything":true}},"objects":[
                {{"t":"house","b":[100,100,140,120],"solid":true,"v":7}},
                {{"t":"tree","p":[300,300],"s":1.5}}{extra}]}}"#
        )
    };
    std::fs::write(&path, file("")).unwrap();
    let p = path.to_string_lossy().to_string();
    let (srv, web) = start_web(&["--world", &p, "--world-size", "4096"]);
    // The file decides the size and the walls: only its one solid object.
    let (_, world) = json(&srv, "GET", "/api/world", "");
    assert_eq!(world["size"], 1024.0);
    assert_eq!(world["boxes"].as_array().unwrap().len(), 1);
    // Clients are told about it and get it byte for byte.
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    let ver = info["world"]["ver"].as_str().unwrap().to_string();
    let (s, body) = web_get(web, "/play/world");
    assert_eq!((s, body.as_str()), (200, file("").as_str()));
    // Or in parts: the head has the footprints, the tree comes with its tile.
    assert_eq!(info["world"]["parts"], true);
    let head: serde_json::Value = serde_json::from_str(&web_get(web, "/play/world/head").1).unwrap();
    assert_eq!((head["objects"].as_array().unwrap().len(), head["size"].clone()), (1, 1024.into()));
    let (s, tiles) = web_get(web, &format!("/play/world/tiles?v={ver}&c=1,1;0,0"));
    let tiles: serde_json::Value = serde_json::from_str(&tiles).unwrap();
    assert_eq!((s, tiles["tiles"]["1,1"][0]["t"].clone(), tiles["tiles"]["0,0"].clone()), (200, "tree".into(), serde_json::json!([])));
    assert_eq!(web_get(web, "/play/world/tiles?v=0000000000000001&c=0,0").0, 409);
    // Regenerate (here: add a wall) and reload: applied live, new version.
    std::fs::write(&path, file(r#",{"t":"crag","b":[500,500,540,540],"solid":true}"#)).unwrap();
    let (s, r) = json(&srv, "POST", "/api/world/reload", "");
    assert_eq!((s, r["walls"].clone()), (200, 2.into()));
    assert_eq!(json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len(), 2);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    assert_ne!(info["world"]["ver"].as_str().unwrap(), ver);
    // A game file edit does not bring the generated walls back.
    let (_, game) = json(&srv, "GET", "/api/game", "");
    assert_eq!(json(&srv, "PUT", "/api/game", &game.to_string()).0, 200);
    assert_eq!(json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len(), 2);
    // A file of another size is refused (that needs a restart), and so is a broken one.
    std::fs::write(&path, file("").replace("1024", "2048")).unwrap();
    let (s, r) = json(&srv, "POST", "/api/world/reload", "");
    assert_eq!((s, r["restart"].clone()), (200, true.into()), "{r}");
    assert_eq!(json(&srv, "GET", "/api/world", "").1["size"], 1024.0, "the size waits for a restart");
    std::fs::write(&path, "nope").unwrap();
    assert_eq!(json(&srv, "POST", "/api/world/reload", "").0, 400);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Roads in a world file are faster underfoot: the server moves a player on one `road_speed` times as far, and tells
/// clients the road cells (with the tiles) and the speed (in the join info) so prediction agrees.
#[test]
fn a_player_on_a_road_covers_more_ground_than_one_on_grass() {
    let dir = std::env::temp_dir().join(format!("e2e-road-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("world.json");
    std::fs::write(&path, r#"{"format":1,"size":1024,"objects":[],"roads":[{"half_width":6,"pts":[[40,300],[1000,300]]}]}"#).unwrap();
    let p = path.to_string_lossy().to_string();
    let (srv, web) = start_web(&["--world", &p]);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    assert_eq!((info["road_speed"].as_f64(), info["walls"]["inline"].clone()), (Some(1.5), false.into()), "roads travel with tiles");
    let ver = info["walls"]["ver"].as_str().unwrap().to_string();
    let (_, _, body) = web_get_bytes(web, &format!("/play/tiles?v={ver}&c=0,1;3,3"), false);
    let (_, tiles) = proto::tiles::decode(&body).unwrap();
    assert!(!tiles[0].2.is_empty() && tiles[1].2.is_empty(), "the road's tile has road cells, a far one none");
    let mut c = connect(&srv);
    c.send_input(0, 0);
    c.pump(200);
    let mut run = |c: &mut Client, y: f32| {
        walk_to(c, 100.0, y, |c| (c.pos().0 - 100.0).abs() < 1.0 && (c.pos().1 - y).abs() < 1.0);
        for _ in 0..6 {
            c.send_input(0, 0);
            c.pump(40);
        }
        let x0 = c.pos().0;
        walk(c, 30);
        for _ in 0..6 {
            c.send_input(0, 0);
            c.pump(40);
        }
        c.pos().0 - x0
    };
    let road = run(&mut c, 300.0);
    let grass = run(&mut c, 500.0);
    let k = road / grass;
    assert!((1.3..1.7).contains(&k), "road {road}, grass {grass}: {k} times as far");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The World tab: a recipe is generated into a preview (the world is untouched), applied live, and kept for
/// the next start, which picks up world.json without being told.
#[test]
fn the_console_generates_a_world_previews_it_and_applies_it_live() {
    let worldgen = Path::new(env!("CARGO_BIN_EXE_server")).with_file_name("worldgen");
    assert!(worldgen.is_file(), "build the generator first: cargo build --release -p worldgen");
    let wg = worldgen.to_string_lossy().to_string();
    let (mut srv, web) = start_web(&["--world-size", "1024", "--worldgen", &wg]);
    let walls_before = json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len();
    assert_eq!(json(&srv, "GET", "/api/worldfile", "").0, 404);

    let recipe = r#"{"size":1024,"seed":3,"city":false,"edits":[{"add":{"t":"castle","b":[400,400,460,450]}}]}"#;
    let (s, preview) = http(&srv, "POST", "/api/world/generate", recipe);
    assert_eq!(s, 200, "{preview}");
    let preview: serde_json::Value = serde_json::from_str(&preview).unwrap();
    assert_eq!(preview["recipe"]["seed"], 3);
    assert!(preview["objects"].as_array().unwrap().iter().any(|o| o["t"] == "castle" && o["b"][0] == 400.0));
    // A preview changes nothing yet.
    assert_eq!(json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len(), walls_before);
    // A bad recipe says why.
    let (s, r) = json(&srv, "POST", "/api/world/generate", r#"{"edits":[{"add":{"t":"spaceship","b":[0,0,9,9]}}]}"#);
    assert_eq!(s, 400);
    assert!(r["error"].as_str().unwrap().contains("spaceship"), "{r}");

    // Apply: live, served to clients, and saved as world.json. (The bad recipe did not spoil the preview.)
    let (s, r) = json(&srv, "POST", "/api/world/apply", "");
    assert_eq!(s, 200, "{r}");
    let solid = preview["objects"].as_array().unwrap().iter().filter(|o| o["solid"] == true).count();
    assert_eq!(json(&srv, "GET", "/api/world", "").1["boxes"].as_array().unwrap().len(), solid);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    assert!(info["world"]["ver"].is_string());
    assert_eq!(web_get(web, "/play/world").0, 200);
    assert!(srv.dir.join("world.json").is_file());
    // Nothing left to apply.
    assert_eq!(json(&srv, "POST", "/api/world/apply", "").0, 400);

    // Restarted in the same place, the server comes back on that world.
    let dir = std::mem::replace(&mut srv.dir, PathBuf::from("/nonexistent-keep"));
    drop(srv);
    let srv = start_in(dir, &["--world-size", "4096"]);
    let w = json(&srv, "GET", "/api/world", "").1;
    assert_eq!((w["size"].clone(), w["boxes"].as_array().unwrap().len()), (1024.0.into(), solid));
}

/// Building over the real protocol: a client builds blocks next to itself in layers and breaks one, the web feed
/// reports each change (and only the changes) with the builder's account as owner, and blocks are kept across a
/// restart.
#[test]
fn players_build_and_break_blocks_which_are_fed_to_clients_and_kept() {
    use proto::game::{build_arg, build_target, ACT_BREAK, ACT_BUILD};
    let (mut srv, web) = start_web(&[]);
    let identity = proto::secure::new_identity();
    let tag = proto::secure::account_tag(&proto::secure::identity_public(&identity));
    let mut c = connect_as(&srv, identity);
    c.pump(400);
    let me = c.view.entries().iter().find(|e| e.id == c.entity).map(|e| (proto::world::dpos(e.x), proto::world::dpos(e.y))).expect("in the world");
    let feed = |since: u64| -> serde_json::Value { serde_json::from_str(&web_get(web, &format!("/play/blocks?since={since}")).1).unwrap() };
    let total = |srv: &Srv| json(srv, "GET", "/api/blocks", "").1["total"].as_u64().unwrap();
    let start = feed(0);
    assert_eq!((start["full"].clone(), start["cols"].as_array().unwrap().len()), (true.into(), 0));

    // A few units away in some direction there is open ground (a wall may be in the way of one or two). This
    // world has no heights, so every column starts in layer 0.
    let mut cell = None;
    for (dx, dy) in [(4.0, 0.0), (-4.0, 0.0), (0.0, 4.0), (0.0, -4.0), (3.0, 3.0), (-3.0, -3.0)] {
        let (cx, cy) = (((me.0 + dx) / 2.0).floor() as u32, ((me.1 + dy) / 2.0).floor() as u32);
        act_arg(&mut c, ACT_BUILD, build_target(cx, cy), build_arg(0, 1));
        c.pump(250);
        if total(&srv) == 1 {
            cell = Some((cx, cy));
            break;
        }
    }
    let (cx, cy) = cell.expect("could not build anywhere near");
    act_arg(&mut c, ACT_BUILD, build_target(cx, cy), build_arg(1, 11));
    c.pump(250);
    let f = feed(start["ver"].as_u64().unwrap());
    assert_eq!(f["full"], false);
    assert_eq!(f["owners"], serde_json::json!([tag]));
    assert_eq!(f["cols"], serde_json::json!([[cx, cy, 0, "1b", [1, 1]]]));
    // The same by region (proto::blockfeed), binary: the region it is in, with the builder's tag; asked again
    // with the version just given, nothing; a neighbouring empty region never comes back at all.
    let (rx, ry) = proto::blockfeed::region_of(cx, cy);
    let ask = format!("/play/blocks/r?c={rx},{ry},0;{},{ry},0", rx + 1);
    let (st, _, body) = web_get_bytes(web, &ask, false);
    assert_eq!(st, 200);
    let regions = proto::blockfeed::decode_reply(&body).expect("a region reply");
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].cols, vec![(proto::world::block_key(cx, cy), 0, vec![(0, 1, tag.clone()), (1, 11, tag.clone())])]);
    let again = proto::blockfeed::decode_reply(&web_get_bytes(web, &format!("/play/blocks/r?c={rx},{ry},{}", regions[0].ver), false).2).unwrap();
    assert!(again.is_empty(), "nothing changed since that version");
    assert_eq!(web_get_bytes(web, "/play/blocks/r?c=1,x", false).0, 400);
    // Refused: out of reach, and a block with nothing under it or beside it.
    act_arg(&mut c, ACT_BUILD, build_target(cx + 40, cy), build_arg(0, 0));
    c.pump(250);
    act_arg(&mut c, ACT_BUILD, build_target(cx, cy), build_arg(3, 0));
    c.pump(250);
    assert_eq!(total(&srv), 2);
    let state = json(&srv, "GET", "/api/state?limit=1", "").1;
    assert_eq!(state["game"]["builds"], 2);
    assert_eq!(state["blocks"]["total"], 2);
    assert!(play_info(web).contains(r#""act_build":252"#));

    // Break the bottom one: the top one stays where it is, and the feed reports the column as it now is.
    let v = f["ver"].as_u64().unwrap();
    act_arg(&mut c, ACT_BREAK, build_target(cx, cy), build_arg(0, 0));
    c.pump(250);
    assert_eq!(feed(v)["cols"], serde_json::json!([[cx, cy, 0, ".b", [0, 1]]]));
    assert_eq!(feed(feed(0)["ver"].as_u64().unwrap())["cols"], serde_json::json!([]), "nothing new");

    // Saved within a few seconds, and back after a restart, still owned.
    std::thread::sleep(Duration::from_millis(5600));
    let dir = std::mem::replace(&mut srv.dir, PathBuf::from("/nonexistent-keep"));
    drop(c);
    drop(srv);
    // Kept by region: one small file for the one region built in.
    let (rx, ry) = proto::blockfeed::region_of(cx, cy);
    assert!(dir.join("world.blocks.d").join(format!("{rx}_{ry}.json")).is_file());
    let srv = start_in(dir, &[]);
    assert_eq!(total(&srv), 1);
    let b = json(&srv, "GET", "/api/blocks", "").1;
    assert_eq!((b["owners"].clone(), b["cols"][0][3].clone(), b["cols"][0][4].clone()), (serde_json::json!([tag]), ".b".into(), serde_json::json!([0, 1])));
    // The admin can place whole columns, counted from the ground (not on a wall or a player), and clear them all
    // (the restored block too).
    let (s, r) = json(&srv, "POST", "/api/blocks", r#"{"cols":[[3,3,"0123"],[4,3,"2"],[3,3,"1.1"]]}"#);
    assert_eq!((s, r["set"].clone(), r["total"].clone()), (200, 3.into(), 4.into()), "{r}");
    assert_eq!(json(&srv, "POST", "/api/blocks", r#"{"cols":[[4,3,""]]}"#).1["total"], 3);
    assert_eq!(json(&srv, "POST", "/api/blocks", "nope").0, 400);
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", "").1["removed"], 3);
    assert_eq!(total(&srv), 0);
}

/// The day's schedule rides along in /play/info (with a sample moment for clients to check their sums against);
/// the console can make it any time of day now, which is kept, and a new schedule applies live.
#[test]
fn the_daylight_schedule_is_published_moved_and_kept() {
    let (srv, web) = start_web(&[]);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    let d = &info["daylight"];
    assert_eq!((d["mode"].clone(), d["day_minutes"].clone()), ("cycle".into(), 24.0.into()), "the default cycle: {d}");
    let sample = d["sample"].as_array().unwrap();
    // The schedule as the game file has it is the published one less the sample (a cycle names its night share).
    let mut v = d.clone();
    v.as_object_mut().unwrap().remove("sample");
    let schedule: proto::daylight::Daylight = serde_json::from_value(v).unwrap();
    assert!((schedule.phase(sample[0].as_f64().unwrap()) - sample[1].as_f64().unwrap()).abs() < 1e-9);

    // Midnight now: published, and saved in the game file.
    let (s, r) = json(&srv, "POST", "/api/daylight", r#"{"phase":0.8}"#);
    assert_eq!(s, 200, "{r}");
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    let phase = info["daylight"]["sample"][1].as_f64().unwrap();
    assert!((phase - 0.8).abs() < 0.001, "phase {phase}");
    assert!(std::fs::read_to_string(srv.dir.join("game.json")).unwrap().contains("\"daylight\""));

    // A clock schedule applies live (no restart, nobody dropped); its time cannot be moved, and nonsense is refused.
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["daylight"] = serde_json::json!({"mode": "clock", "sunrise": "06:00", "sunset": "18:00", "utc_offset_minutes": 0});
    let (s, r) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, r["reset"].clone()), (200, false.into()), "{r}");
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    assert_eq!((info["daylight"]["mode"].clone(), info["daylight"]["night_share"].clone()), ("clock".into(), 0.5.into()));
    assert_eq!(json(&srv, "POST", "/api/daylight", r#"{"phase":0.2}"#).0, 409);
    game["daylight"] = serde_json::json!({"mode": "cycle", "day_minutes": 0, "night_share": 0.3});
    assert_eq!(json(&srv, "PUT", "/api/game", &game.to_string()).0, 400);
    // The lamp and the fire are block materials like any other.
    let (s, r) = json(&srv, "POST", "/api/blocks", r#"{"cols":[[3,3,"cd"]]}"#);
    assert_eq!(s, 200, "{r}");
    assert_eq!(json(&srv, "GET", "/api/blocks", "").1["cols"][0][3], "cd");
}

/// A weather zone (a place and a weather) is told to clients while it is on, and switches live.
#[test]
fn weather_zones_are_published_and_switch_live() {
    let (srv, web) = start_web(&[]);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    game["weather"] = serde_json::json!([{"name": "red mist", "color": "#aa1111", "intensity": 0.8, "sight_mult": 0.5, "drain": []},
        {"name": "healing light", "color": "#ffd86b", "intensity": 0.4, "drain": [{"attr": "health", "per_sec": 5.0}]}]);
    let places = game["places"].as_array_mut().unwrap();
    places.push(serde_json::json!({"name": "ruin", "x": 200.0, "y": 200.0, "r": 30.0}));
    places.push(serde_json::json!({"name": "spring", "x": 100.0, "y": 300.0, "r": 4.0}));
    game["zones"] = serde_json::json!([{"name": "haunt", "place": "ruin", "weather": "red mist", "height": 40.0},
        {"name": "well", "place": "spring", "weather": "healing light", "height": 6.0}]);
    let (s, r) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, r["reset"].clone()), (200, false.into()), "{r}");
    let zones = |web| {
        let t = Instant::now();
        loop {
            let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
            let z = info["weather_zones"].clone();
            if z.as_array().is_some_and(|a| !a.is_empty()) || t.elapsed() > Duration::from_secs(2) {
                return z;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let z = zones(web);
    assert_eq!(z.as_array().map(|a| a.len()), Some(2), "{z}");
    assert_eq!((z[0]["name"].clone(), z[0]["color"].clone(), z[0]["r"].clone(), z[0]["height"].clone()), ("haunt".into(), "#aa1111".into(), 30.0.into(), 40.0.into()));
    let (s, _) = json(&srv, "POST", "/api/zone", r#"{"name":"haunt","on":false}"#);
    assert_eq!(s, 200);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    assert_eq!(info["weather_zones"].as_array().unwrap().len(), 1, "only the well is left on");
    assert_eq!(json(&srv, "POST", "/api/zone", r#"{"name":"nowhere","on":true}"#).0, 404);
    let st = json(&srv, "GET", "/api/state?limit=0", "").1;
    assert_eq!(st["director"]["zones"], serde_json::json!([{"name": "haunt", "on": false}, {"name": "well", "on": true}]));
}

/// Places with a title are locations: /play/info lists them for clients to name as players walk in and out, a title
/// set in the console applies live, and a bad one is refused.
#[test]
fn titled_places_are_published_as_locations_live() {
    let (srv, web) = start_web(&[]);
    let titles = |web| -> Vec<String> {
        let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
        info["locations"].as_array().unwrap().iter().map(|l| l["title"].as_str().unwrap().to_string()).collect()
    };
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    let before = titles(web);
    let places = game["places"].as_array_mut().unwrap();
    places.push(serde_json::json!({"name": "ashby", "x": 200.0, "y": 200.0, "r": 30.0, "title": "Ashby Crossing"}));
    places.push(serde_json::json!({"name": "spawn-b", "x": 100.0, "y": 300.0, "r": 4.0}));
    let (s, r) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!((s, r["reset"].clone()), (200, false.into()), "{r}");
    let after = titles(web);
    assert_eq!(after.len(), before.len() + 1, "only the titled place is a location: {after:?}");
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    let ashby = info["locations"].as_array().unwrap().iter().find(|l| l["title"] == "Ashby Crossing").cloned().unwrap();
    assert_eq!((ashby["x"].clone(), ashby["y"].clone(), ashby["r"].clone()), (200.0.into(), 200.0.into(), 30.0.into()));
    assert!(std::fs::read_to_string(srv.dir.join("game.json")).unwrap().contains("Ashby Crossing"), "kept in the game file");
    // A sound zone (no title) is published too, with its sound and music; an unknown sound is refused.
    let last = game["places"].as_array().unwrap().len() - 1;
    game["places"][last]["sound"] = "crypt".into();
    game["places"][last]["music"] = "eerie".into();
    assert_eq!(json(&srv, "PUT", "/api/game", &game.to_string()).0, 200);
    let info: serde_json::Value = serde_json::from_str(&play_info(web)).unwrap();
    let zone = info["locations"].as_array().unwrap().iter().find(|l| l["sound"] == "crypt").cloned().unwrap();
    assert_eq!((zone["title"].clone(), zone["music"].clone()), ("".into(), "eerie".into()));
    game["places"][last]["sound"] = "bagpipes".into();
    let (s, r) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert!(s == 400 && r["error"].as_str().unwrap().contains("sound is one of"), "{r}");
    game["places"][last]["sound"] = "crypt".into();
    game["places"][last]["title"] = " padded".into();
    assert_eq!(json(&srv, "PUT", "/api/game", &game.to_string()).0, 400);
}

/// A local event: a player walking into the plaza starts it (`when` players), it puts a banner across the screens
/// of those there and nobody else's, and rewards them with items in their bags.
#[test]
fn players_gathering_start_a_local_event_that_tells_and_rewards_only_those_there() {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-ev-{}-{}", std::process::id(), free_udp()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut g = GameFile::default_game();
    g.walls = Some(vec![]);
    g.events.push(proto::game::EventDef {
        name: "defend".into(),
        place: Some("plaza".into()),
        when: Some(proto::game::When::Players { count: 1 }),
        steps: vec![
            proto::game::Step::Announce { text: "Hold the plaza!".into(), banner: true, nearby: true },
            proto::game::Step::Reward { item: "medkit".into(), count: 2, everyone: false },
        ],
        ..Default::default()
    });
    std::fs::write(dir.join("game.json"), g.to_json()).unwrap();
    let srv = start_in(dir, &[]);
    let (px, py) = (200.0, 200.0); // the default plaza, radius 20
    // Someone far off, well outside the plaza and its surroundings.
    let mut far = connect_as(&srv, proto::secure::new_identity());
    far.send_input(0, 0);
    far.pump(200);
    walk_to(&mut far, 470.0, 470.0, |c| (c.pos().0 - 470.0).hypot(c.pos().1 - 470.0) < 4.0);
    far.send_input(0, 0);
    far.pump(300);
    let far_bag = far.bag.clone(); // (whatever they picked up on the way)
    // Someone who goes to the plaza.
    let mut near = connect_as(&srv, proto::secure::new_identity());
    near.send_input(0, 0);
    near.pump(200);
    walk_to(&mut near, px, py, |c| c.heard.iter().any(|m| m.from == 0 && m.channel == proto::chat::channel::BANNER));
    near.send_input(0, 0);
    let t = Instant::now();
    while !near.bag.iter().any(|&(item, n)| item == 1 && n >= 2) {
        assert!(t.elapsed() < Duration::from_secs(5), "no reward in the bag: {:?}", near.bag);
        near.send_input(0, 0);
        near.pump(50);
    }
    far.pump(300);
    assert!(!far.heard.iter().any(|m| m.text.contains("Hold the plaza")), "the far player heard a nearby message");
    assert_eq!(far.bag, far_bag, "and was not rewarded");
    let st = state(&srv);
    assert_eq!(st["director"]["events"][0]["last"], "succeeded", "{}", st["director"]);
    // It does not start again straight away (cooldown), though the player is still there.
    near.pump(1500);
    assert_eq!(state(&srv)["director"]["events"][0]["fired"], 1);
}

/// Clear down: built blocks all at once, by account or in an area; ground items all or in an area.
#[test]
fn clear_down_takes_blocks_and_ground_items_by_owner_or_area() {
    let srv = start(&[]);
    let total = |srv: &Srv| json(srv, "GET", "/api/blocks", "").1["total"].as_u64().unwrap();
    // Two columns near (7, 7) and one far away, nobody's.
    assert_eq!(json(&srv, "POST", "/api/blocks", r#"{"cols":[[3,3,"00"],[4,3,"1"],[100,100,"2"]]}"#).0, 200);
    assert_eq!(total(&srv), 4);
    let (s, r) = json(&srv, "POST", "/api/blocks/clear", r#"{"x":7,"y":7,"r":6}"#);
    assert_eq!((s, r["removed"].clone()), (200, 3.into()), "only the near ones: {r}");
    assert_eq!(total(&srv), 1);
    // Someone's account: nothing of theirs here, so nothing goes; "" is nobody's, which is what is left.
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", r#"{"owner":"00000000000000aa"}"#).1["removed"], 0);
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", r#"{"owner":""}"#).1["removed"], 1);
    // Everything, as before (an empty body).
    assert_eq!(json(&srv, "POST", "/api/blocks", r#"{"cols":[[3,3,"0"]]}"#).0, 200);
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", "").1["removed"], 1);
    // Bad filters are refused rather than read as "everything".
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", r#"{"place":"nowhere"}"#).0, 404);
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", r#"{"x":1}"#).0, 404);
    assert_eq!(json(&srv, "POST", "/api/blocks/clear", r#"{"radius":1}"#).0, 400);
    // Ground items: some at the plaza, some elsewhere.
    let (_, game) = json(&srv, "GET", "/api/game", "");
    let item = game["items"][0]["name"].as_str().unwrap().to_string();
    let plaza = &game["places"][0];
    assert_eq!(json(&srv, "POST", "/api/ground", &format!(r#"{{"item":"{item}","count":3,"place":{}}}"#, plaza["name"])).0, 200);
    assert_eq!(json(&srv, "POST", "/api/ground", &format!(r#"{{"item":"{item}","count":2,"x":500,"y":20}}"#)).0, 200);
    let (s, r) = json(&srv, "POST", "/api/ground/clear", &format!(r#"{{"place":{}}}"#, plaza["name"]));
    assert_eq!((s, r["removed"].clone()), (200, 3.into()), "{r}");
    assert_eq!(json(&srv, "POST", "/api/ground/clear", "").1["removed"], 2);
}

// ---------------------------------------------------------------------------
// Names

/// Tick both clients until `c` hears a line from the server (id 0) and return it.
fn server_says(c: &mut Client, others: &mut [&mut Client]) -> String {
    let before = c.heard.len();
    for _ in 0..40 {
        c.tick();
        for o in others.iter_mut() {
            o.tick();
        }
        if let Some(m) = c.heard[before..].iter().find(|m| m.from == 0) {
            return m.text.clone();
        }
    }
    panic!("the server never answered");
}

fn names_of(web: SocketAddr, ids: &[u32]) -> serde_json::Value {
    let (code, body) = web_get(web, &format!("/play/names?ids={}", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")));
    assert_eq!(code, 200, "{body}");
    serde_json::from_str(&body).unwrap()
}

#[test]
fn players_claim_names_that_stay_theirs_and_everyone_can_look_them_up() {
    let dir = std::env::temp_dir().join(format!("mmo-e2e-names-{}-{}", std::process::id(), free_udp()));
    std::fs::create_dir_all(&dir).unwrap();
    let up = |dir: &PathBuf| {
        let w: SocketAddr = format!("127.0.0.1:{}", free_tcp()).parse().unwrap();
        let srv = start_in(dir.clone(), &["--web", &w.to_string()]);
        let t = Instant::now();
        while std::net::TcpStream::connect(w).is_err() {
            assert!(t.elapsed() < Duration::from_secs(5), "web gateway never came up");
            std::thread::sleep(Duration::from_millis(50));
        }
        (srv, w)
    };
    let (ana_id, bob_id) = (proto::secure::new_identity(), proto::secure::new_identity());
    {
        let (srv, web) = up(&dir);
        let mut ana = connect_as(&srv, ana_id);
        let mut bob = connect_as(&srv, bob_id);
        let mut guest = connect(&srv);
        ana.say(proto::chat::channel::NAME, "  Ana   Lee ");
        assert_eq!(server_says(&mut ana, &mut [&mut bob, &mut guest]), "You are now known as Ana Lee.");
        assert!(bob.heard.iter().all(|m| m.text != "Ana Lee"), "a name claim is never routed to anyone");

        // Everyone's name, from the web gateway: a claimed one, a given one, a guest, a bot by its type.
        let s = state(&srv);
        let bot = s["e"].as_array().unwrap().iter().find(|e| e[4] == 0 && e[3].as_i64().unwrap_or(-1) >= 0).map(|e| e[0].as_u64().unwrap() as u32);
        let mut ids = vec![ana.entity, bob.entity, guest.entity];
        ids.extend(bot);
        let n = names_of(web, &ids);
        assert_eq!(n[ana.entity.to_string()], "Ana Lee");
        let bob_given = n[bob.entity.to_string()].as_str().unwrap().to_string();
        assert!(proto::names::clean(&bob_given).is_ok(), "an unclaimed account still has a name: {bob_given}");
        assert_eq!(n[guest.entity.to_string()], "Guest");
        if let Some(bot) = bot {
            assert!(n[bot.to_string()].as_str().unwrap().chars().next().unwrap().is_uppercase(), "{n}");
        }
        assert!(names_of(web, &[ana.entity ^ (1 << 24)]).as_object().unwrap().is_empty(), "a stale id (old generation) has no name");
        assert_eq!(web_get(web, &format!("/play/names?ids={}", vec!["1"; 300].join(","))).0, 400);

        // Taken (case and marks do not count), not allowed, and guests cannot.
        bob.say(proto::chat::channel::NAME, "ana-lee");
        assert!(server_says(&mut bob, &mut [&mut ana]).contains("is taken"));
        bob.say(proto::chat::channel::NAME, "Admin");
        assert!(server_says(&mut bob, &mut [&mut ana]).contains("reserved"));
        guest.say(proto::chat::channel::NAME, "Somebody");
        assert!(server_says(&mut guest, &mut [&mut ana]).contains("Guests cannot"));

        // The console sees names too, and chat lines carry the speaker's.
        let s = state(&srv);
        assert_eq!(s["names"][ana.entity.to_string()], "Ana Lee");
        ana.say(proto::chat::channel::SAY, "hello");
        for _ in 0..10 {
            ana.tick();
        }
        let (_, chat) = json(&srv, "GET", "/api/chat", "");
        assert!(chat["lines"].as_array().unwrap().iter().any(|l| l[3] == "hello" && l[4] == "Ana Lee"), "{chat}");

        let status = Command::new("kill").arg("-TERM").arg(srv.child.id().to_string()).status().unwrap();
        assert!(status.success());
        let mut srv = srv;
        let t = Instant::now();
        while srv.child.try_wait().unwrap().is_none() {
            assert!(t.elapsed() < Duration::from_secs(5), "server did not exit on SIGTERM");
            std::thread::sleep(Duration::from_millis(50));
        }
        std::mem::forget(std::mem::replace(&mut srv.dir, PathBuf::from("/nonexistent-keep")));
    }
    // After a restart the name is still Ana's: taken while she is away, and hers again when she is back.
    let (srv, web) = up(&dir);
    let mut bob = connect_as(&srv, bob_id);
    bob.say(proto::chat::channel::NAME, "Ana Lee");
    assert!(server_says(&mut bob, &mut []).contains("is taken"), "a saved name stays taken while its player is offline");
    let mut ana = connect_as(&srv, ana_id);
    ana.tick();
    assert_eq!(names_of(web, &[ana.entity])[ana.entity.to_string()], "Ana Lee");
    // And she can change it, which frees the old one.
    ana.say(proto::chat::channel::NAME, "Ana Grey");
    assert_eq!(server_says(&mut ana, &mut [&mut bob]), "You are now known as Ana Grey.");
    bob.say(proto::chat::channel::NAME, "Ana Lee");
    assert_eq!(server_says(&mut bob, &mut [&mut ana]), "You are now known as Ana Lee.");
    drop(srv);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_spell_edit_applies_live_and_its_casts_reach_the_client_a_few_times_over() {
    let srv = start(&[]);
    let (_, mut game) = json(&srv, "GET", "/api/game", "");
    let zap = &mut game["actions"][0];
    zap["aim"] = serde_json::json!("self");
    zap["area"] = serde_json::json!(10.0);
    zap["fx"] = serde_json::json!("fire");
    let (code, out) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(code, 200, "{out}");
    assert_eq!(out["reset"], false, "spell fields are not in the content hash");
    let (_, info) = json(&srv, "GET", "/api/info", "");
    assert_eq!(info["actions"][0]["fx"], "fire");
    assert_eq!(info["actions"][0]["area"], 10.0);
    let mut c = connect(&srv);
    c.send_input(0, 0);
    c.pump(300);
    act(&mut c, proto::defs::ACTION_ZAP, 0);
    c.pump(400);
    let mine: Vec<_> = c.casts.iter().filter(|k| k.caster == c.entity).collect();
    assert!(!mine.is_empty(), "no cast came back");
    assert!(mine.iter().all(|k| k.action == proto::defs::ACTION_ZAP && k.target == 0), "{mine:?}");
    assert!(mine.len() >= 2 && mine.iter().all(|k| k.seq == mine[0].seq), "one cast, repeated: {mine:?}");
}

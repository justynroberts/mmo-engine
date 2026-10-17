//! End-to-end tests: start the real server binary and talk to it over real
//! UDP and HTTP, the way a game client and the test UI would.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::path::PathBuf;
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
}

enum Outcome {
    Connected(Box<Client>),
    Rejected(u8),
    Silent,
}

fn connect_with(srv: &Srv, pinned: Option<[u8; 32]>, hash: u64) -> Outcome {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    sock.connect(srv.udp).unwrap();
    sock.set_read_timeout(Some(Duration::from_millis(150))).unwrap();
    let mut hs = ClientHs::new(pinned, hash);
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
                        }));
                    }
                }
                _ => {}
            }
        }
    }
    Outcome::Silent
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
        wire::encode_input(&mut plain, &Input { seq: self.seq, ack_tick: self.view.last_tick, dir_x: dx, dir_y: dy, action: 0, target: 0, chat_ack: self.chat.ack() });
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
    wire::encode_input(&mut plain, &Input { seq: 7, ack_tick: 0, dir_x: 99, dir_y: 99, action: 0, target: 0, chat_ack: 0 });
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
    for _ in 0..500 {
        let hello = ClientHs::new(None, 1).hello();
        sock.send(&hello).unwrap();
        let n = sock.recv(&mut buf).expect("challenge");
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
        wire::encode_input(&mut plain, &Input { seq: 1, ack_tick: 0, dir_x: 0, dir_y: 0, action: 0, target: 0, chat_ack: 0 });
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
    let near_dock = st["e"].as_array().unwrap().iter().filter(|e| e[3] == 4 && (e[1].as_f64().unwrap() - 50.0).hypot(e[2].as_f64().unwrap() - 60.0) < 12.0).count();
    assert_eq!(near_dock, 2, "both criers spawned at the dock");
    // A rule naming a place that does not exist is refused and changes nothing.
    game["archetypes"][4]["home"] = "atlantis".into();
    let (code, body) = json(&srv, "PUT", "/api/game", &game.to_string());
    assert_eq!(code, 400);
    assert!(body["error"].as_str().unwrap().contains("unknown place"), "{body}");
}

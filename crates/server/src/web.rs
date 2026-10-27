//! Public web gateway: lets a browser play.
//!
//! Browsers cannot send raw UDP, so this listens for WebSocket connections and
//! relays them to the game's UDP port: each binary WebSocket message is exactly
//! one datagram, in both directions. The relay never looks inside a packet.
//! The browser runs the same encrypted handshake as every other client, so the
//! gateway cannot read or forge anything, and the player's passphrase (and so
//! their identity key) never leaves the browser.
//!
//! The same port serves the play page and `/play/info`, the public facts a
//! client needs (server key, definitions, world geometry), with the walls by tile
//! at `/play/tiles` and the world file at `/play/world`. Nothing here exposes
//! the admin API, which stays on loopback.
//!
//! Every relayed connection reaches the game server from 127.0.0.1, so by
//! itself every web player would share one address for the per-address rate
//! limits. The gateway therefore records which local UDP port belongs to which
//! real address, and the server looks that up (`Gateway::real_ip`) before
//! applying a limit. Behind a TLS-terminating proxy set `--web-trust-proxy` so
//! the real address is read from `X-Forwarded-For`; without a proxy leave it
//! off, because then the header is attacker-controlled.

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use tungstenite::protocol::WebSocketConfig;
use tungstenite::Message;

/// Largest datagram a game client sends or receives.
const MAX_DATAGRAM: usize = 1300;
/// A connection that says nothing for this long is closed.
const IDLE: Duration = Duration::from_secs(30);
/// Messages per second one connection may relay to the game (inputs run at 20/s).
const MSG_RATE: f64 = 120.0;
const MSG_BURST: f64 = 240.0;

#[derive(Default)]
pub struct GwStats {
    pub open: AtomicU64,
    pub total: AtomicU64,
    pub refused: AtomicU64,
    pub up_bytes: AtomicU64,
    pub down_bytes: AtomicU64,
}

pub struct Gateway {
    /// Local UDP port of a relayed connection -> the real address behind it.
    ports: Mutex<HashMap<u16, [u8; 4]>>,
    per_ip: Mutex<HashMap<[u8; 4], u32>>,
    info: RwLock<String>,
    /// The walls by tile and the world file (`/play/tiles`, `/play/world`).
    statics: Arc<crate::statics::Statics>,
    /// Built blocks, served at /play/blocks.
    blocks: RwLock<crate::blocks::Feed>,
    /// Every entity's name by slot, with the id it was for (`/play/names`).
    names: RwLock<Vec<(u32, Arc<str>)>>,
    pub stats: GwStats,
}

/// Most ids one `/play/names` request may ask about.
pub const NAMES_MAX: usize = 256;

/// More walls than this and `/play/info` stops carrying them inline: clients fetch them by tile from `/play/tiles`.
pub const WALLS_INLINE_MAX: usize = 600;

pub struct WebCfg {
    pub addr: SocketAddr,
    pub ui_dir: Option<PathBuf>,
    pub udp_port: u16,
    pub max_conns: usize,
    pub per_ip: u32,
    pub trust_proxy: bool,
    /// Serve only the data a client fetches (`/play/info`, tiles, the world, names, blocks): no play page and no
    /// WebSocket relay, for a server played from the native client only (`--no-browser`).
    pub no_browser: bool,
    /// The other zones' web addresses (`--zone-web`): a page one of them served may connect here, since a player
    /// handed over from that zone arrives with that page still open.
    pub zone_hosts: Vec<String>,
}

impl Gateway {
    pub fn new(statics: Arc<crate::statics::Statics>) -> Arc<Gateway> {
        Arc::new(Gateway { ports: Mutex::default(), per_ip: Mutex::default(), info: RwLock::new("{}".into()), statics, blocks: RwLock::new(crate::blocks::Feed::new()), names: RwLock::default(), stats: GwStats::default() })
    }

    /// Columns players changed this tick, as they now are.
    pub fn blocks_changed(&self, changes: Vec<(u32, Option<crate::blocks::Col>)>, owners: &[String]) {
        if let Ok(mut f) = self.blocks.write() {
            f.apply(changes, owners);
        }
    }

    /// Take the names that changed this tick (all of them when bot types changed). Called by the sim thread after
    /// the tick; the lock is held only to copy a handful of pointers.
    pub fn names_changed(&self, sim: &crate::sim::Sim) {
        let slots: Vec<usize> = if sim.names_all { (0..sim.ents.high).collect() } else { sim.ents.renamed.iter().map(|&s| s as usize).collect() };
        let fresh: Vec<(usize, u32, Arc<str>)> =
            slots.into_iter().filter(|&s| sim.ents.alive[s]).map(|s| (s, sim.ents.id_of(s), sim.feed_name(s))).collect();
        if let Ok(mut t) = self.names.write() {
            if t.len() < sim.ents.cap() {
                t.resize(sim.ents.cap(), (u32::MAX, Arc::from("")));
            }
            for (s, id, n) in fresh {
                t[s] = (id, n);
            }
        }
    }

    /// `{"<id>": "<name>", ...}` for those of `ids` that are alive. An id whose slot has moved on is left out.
    pub fn names_json(&self, ids: &[u32]) -> String {
        let t = self.names.read();
        let mut out = serde_json::Map::new();
        if let Ok(t) = t {
            for &id in ids {
                if let Some((have, n)) = t.get((id & crate::sim::SLOT_MASK) as usize) {
                    if *have == id {
                        out.insert(id.to_string(), serde_json::Value::String(n.to_string()));
                    }
                }
            }
        }
        serde_json::Value::Object(out).to_string()
    }

    pub fn set_info(&self, json: String) {
        if let Ok(mut w) = self.info.write() {
            *w = json;
        }
    }

    /// The address a packet really came from. Only packets from this machine can be relayed
    /// ones, and only those are looked up, so the common path costs nothing.
    pub fn real_ip(&self, addr: &SocketAddrV4) -> [u8; 4] {
        if addr.ip().is_loopback() {
            if let Some(ip) = self.ports.lock().ok().and_then(|m| m.get(&addr.port()).copied()) {
                return ip;
            }
        }
        addr.ip().octets()
    }
}

/// IPv4 as is; IPv6 folded into four bytes (collisions are possible but rare, and only
/// ever make two addresses share a limit).
fn key_of(ip: IpAddr) -> [u8; 4] {
    match ip {
        IpAddr::V4(v) => v.octets(),
        IpAddr::V6(v) => match v.to_ipv4_mapped() {
            Some(m) => m.octets(),
            None => {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                v.octets().hash(&mut h);
                let b = h.finish().to_le_bytes();
                [0xFD, b[0], b[1], b[2]]
            }
        },
    }
}

pub fn start(cfg: WebCfg, gw: Arc<Gateway>) -> std::io::Result<SocketAddr> {
    let listener = TcpListener::bind(cfg.addr)?;
    let local = listener.local_addr()?;
    let cfg = Arc::new(cfg);
    std::thread::Builder::new().name("web-accept".into()).spawn(move || {
        for conn in listener.incoming().flatten() {
            let (cfg, gw) = (cfg.clone(), gw.clone());
            // A thread per connection keeps this simple; `max_conns` bounds how many exist.
            if gw.stats.open.load(Ordering::Relaxed) as usize >= cfg.max_conns.saturating_add(64) {
                gw.stats.refused.fetch_add(1, Ordering::Relaxed);
                continue; // dropped: even answering would cost a thread
            }
            gw.stats.open.fetch_add(1, Ordering::Relaxed);
            let _ = std::thread::Builder::new().name("web-conn".into()).spawn(move || {
                let _ = serve(conn, &cfg, &gw);
                gw.stats.open.fetch_sub(1, Ordering::Relaxed);
            });
        }
    })?;
    Ok(local)
}

struct Head {
    path: String,
    websocket: bool,
    host: Option<String>,
    origin: Option<String>,
    forwarded: Option<String>,
    query: String,
    gzip: bool,
}

/// Read the request head without consuming it, so a WebSocket upgrade can be handed to
/// tungstenite untouched.
fn peek_head(conn: &TcpStream) -> Option<(Head, usize)> {
    let mut buf = [0u8; 8192];
    let t = Instant::now();
    loop {
        let n = conn.peek(&mut buf).ok()?;
        if n == 0 || t.elapsed() > Duration::from_secs(5) {
            return None;
        }
        if let Some(end) = buf[..n].windows(4).position(|w| w == b"\r\n\r\n") {
            let text = std::str::from_utf8(&buf[..end]).ok()?;
            let mut lines = text.split("\r\n");
            let mut first = lines.next()?.split(' ');
            if first.next()? != "GET" {
                return None;
            }
            let target = first.next()?;
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            let (path, query) = (path.to_string(), query.to_string());
            let mut h = Head { path, websocket: false, host: None, origin: None, forwarded: None, query, gzip: false };
            for l in lines {
                let Some((k, v)) = l.split_once(':') else { continue };
                let v = v.trim();
                match k.trim().to_ascii_lowercase().as_str() {
                    "upgrade" => h.websocket = v.eq_ignore_ascii_case("websocket"),
                    "host" => h.host = Some(v.to_string()),
                    "origin" => h.origin = Some(v.to_string()),
                    "x-forwarded-for" => h.forwarded = Some(v.to_string()),
                    "accept-encoding" => h.gzip = v.split(',').any(|e| e.trim().split(';').next() == Some("gzip")),
                    _ => {}
                }
            }
            return Some((h, end + 4));
        }
        if n == buf.len() {
            return None; // head too large
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn respond(conn: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    respond_with(conn, status, ctype, body, "Cache-Control: no-store\r\n")
}

/// A reply with `extra` header lines (each ending in CRLF) in place of the default no-store. Any page may read it:
/// everything here is public and no request carries credentials, and with zone lines the play page of one zone
/// loads the next zone's info, walls and blocks from that zone's gateway.
fn respond_with(conn: &mut TcpStream, status: u16, ctype: &str, body: &[u8], extra: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        _ => "Error",
    };
    write!(
        conn,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n{extra}Access-Control-Allow-Origin: *\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    conn.write_all(body)
}

/// Where the client updates live, relative to the working directory (`/play/client`).
const CLIENT_DIR: &str = "client";

/// Send a file without reading it all into memory first (a packaged app is a couple of hundred megabytes).
fn stream_file(conn: &mut TcpStream, path: &Path, ctype: &str) -> std::io::Result<()> {
    let Ok(mut f) = std::fs::File::open(path) else { return respond(conn, 404, "text/plain", b"not found") };
    let len = f.metadata()?.len();
    conn.set_write_timeout(Some(Duration::from_secs(30)))?;
    write!(
        conn,
        "HTTP/1.1 200 OK\r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n"
    )?;
    std::io::copy(&mut f, conn)?;
    Ok(())
}

/// Gzip `body` for a client that takes it (Vary says the reply depends on that).
fn maybe_gzip(gzip: bool, body: Vec<u8>, extra: &str) -> (Vec<u8>, String) {
    if gzip && body.len() > 256 {
        (crate::worldfile::gzip(&body, 1), format!("{extra}Content-Encoding: gzip\r\nVary: Accept-Encoding\r\n"))
    } else {
        (body, format!("{extra}Vary: Accept-Encoding\r\n"))
    }
}

fn serve(mut conn: TcpStream, cfg: &WebCfg, gw: &Gateway) -> std::io::Result<()> {
    conn.set_read_timeout(Some(Duration::from_secs(5)))?;
    conn.set_write_timeout(Some(Duration::from_secs(5)))?;
    let peer = conn.peer_addr()?;
    let Some((head, head_len)) = peek_head(&conn) else { return Ok(()) };
    let real = match (&head.forwarded, cfg.trust_proxy) {
        // The proxy appends the address it saw, so the last entry is the one to trust.
        (Some(f), true) => f.rsplit(',').next().and_then(|s| s.trim().parse::<IpAddr>().ok()).map_or(key_of(peer.ip()), key_of),
        _ => key_of(peer.ip()),
    };
    if head.websocket && head.path == "/ws" {
        if cfg.no_browser {
            return respond(&mut conn, 404, "text/plain", b"not found");
        }
        return relay(conn, cfg, gw, &head, real);
    }
    // Plain HTTP: consume the head, then answer.
    let mut sink = vec![0u8; head_len];
    conn.read_exact(&mut sink)?;
    match head.path.as_str() {
        "/play/info" => {
            let body = gw.info.read().map(|s| s.clone()).unwrap_or_default();
            respond(&mut conn, 200, "application/json", body.as_bytes())
        }
        "/play/world" => match gw.statics.file() {
            None => respond(&mut conn, 404, "text/plain", b"this world has no world file"),
            Some(w) if head.gzip => respond_with(&mut conn, 200, "application/json", &w.gz(), "Cache-Control: no-store\r\nContent-Encoding: gzip\r\nVary: Accept-Encoding\r\n"),
            Some(w) => respond_with(&mut conn, 200, "application/json", &w.bytes, "Cache-Control: no-store\r\nVary: Accept-Encoding\r\n"),
        },
        // The world file in parts: the head (every footprint object, the heights and the water), then the point
        // objects (trees, rocks) by tile around the player. `v` is the file's version from /play/info, as for walls.
        "/play/world/head" => match gw.statics.file() {
            None => respond(&mut conn, 404, "text/plain", b"this world has no world file"),
            Some(w) if head.gzip => respond_with(&mut conn, 200, "application/json", &w.head_gz, "Cache-Control: no-store\r\nContent-Encoding: gzip\r\nVary: Accept-Encoding\r\n"),
            Some(w) => respond_with(&mut conn, 200, "application/json", &w.head, "Cache-Control: no-store\r\nVary: Accept-Encoding\r\n"),
        },
        "/play/world/tiles" => {
            let q = |k: &str| head.query.split('&').find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
            let want = q("c").and_then(crate::worldfile::parse_tiles);
            match (gw.statics.file(), q("v").and_then(|v| u64::from_str_radix(v, 16).ok()), want) {
                (None, _, _) => respond(&mut conn, 404, "text/plain", b"this world has no world file"),
                (_, _, None) => respond(&mut conn, 400, "text/plain", b"c must be cx,cy;cx,cy"),
                (Some(w), Some(v), _) if v != w.ver => respond(&mut conn, 409, "text/plain", b"the world has changed: read /play/info again"),
                (Some(w), v, Some(want)) => match w.points_json(&want) {
                    None => respond(&mut conn, 400, "text/plain", b"too many tiles"),
                    Some(body) => {
                        let cache = if v.is_some() { "Cache-Control: public, max-age=31536000, immutable\r\n" } else { "Cache-Control: no-store\r\n" };
                        let (body, extra) = maybe_gzip(head.gzip, body, cache);
                        respond_with(&mut conn, 200, "application/json", &body, &extra)
                    }
                },
            }
        }
        // The newest client for players to update to: `client/mac.json` (version, size, sha256, written by
        // scripts/package_godot.sh) in the working directory, and the app itself, streamed from `client/mac.zip`.
        "/play/client" => match std::fs::read(Path::new(CLIENT_DIR).join("mac.json")) {
            Ok(body) => respond(&mut conn, 200, "application/json", &body),
            Err(_) => respond(&mut conn, 404, "text/plain", b"no client to update to"),
        },
        "/play/client/mac.zip" => stream_file(&mut conn, &Path::new(CLIENT_DIR).join("mac.zip"), "application/zip"),
        "/play/names" => {
            // ?ids=1,2,3: the names of those entities (players, bots by their type).
            let ids: Vec<u32> = head.query.split('&').find_map(|kv| kv.strip_prefix("ids=")).unwrap_or("").split(',').filter_map(|v| v.parse().ok()).collect();
            if ids.len() > NAMES_MAX {
                return respond(&mut conn, 400, "text/plain", format!("at most {NAMES_MAX} ids").as_bytes());
            }
            respond(&mut conn, 200, "application/json", gw.names_json(&ids).as_bytes())
        }
        "/play/blocks" => {
            let since = head.query.split('&').find_map(|kv| kv.strip_prefix("since=")).and_then(|v| v.parse().ok()).unwrap_or(0);
            let body = gw.blocks.read().map(|f| f.since(since)).unwrap_or_default();
            respond(&mut conn, 200, "application/json", body.as_bytes())
        }
        // The blocks near a client, by region: `c=rx,ry,ver;...` (the version of each it holds, 0 for none), and
        // only the regions that differ come back (`proto::blockfeed`). Binary, gzipped when the client takes it.
        "/play/blocks/r" => {
            let c = head.query.split('&').find_map(|kv| kv.strip_prefix("c=")).unwrap_or("");
            match proto::blockfeed::parse_ask(c) {
                None => respond(&mut conn, 400, "text/plain", format!("c must be rx,ry,ver;... (at most {})", proto::blockfeed::MAX_ASK).as_bytes()),
                Some(ask) => {
                    let body = gw.blocks.read().map(|f| f.regions_reply(&ask)).unwrap_or_default();
                    let (body, extra) = maybe_gzip(head.gzip, body, "Cache-Control: no-store\r\n");
                    respond_with(&mut conn, 200, "application/octet-stream", &body, &extra)
                }
            }
        }
        "/play/tiles" => {
            // `v` is the walls' version from /play/info: a reply is for that version only, so it can be cached for
            // good, and a client asking for an older one is told to read /play/info again.
            let q = |k: &str| head.query.split('&').find_map(|kv| kv.strip_prefix(k).and_then(|v| v.strip_prefix('=')));
            let tiles = gw.statics.tiles();
            let want = q("c").and_then(proto::tiles::parse_list);
            match (q("v").and_then(|v| u64::from_str_radix(v, 16).ok()), want) {
                (_, None) => respond(&mut conn, 400, "text/plain", b"c must be cx,cy;cx,cy"),
                (Some(v), _) if v != tiles.ver => respond(&mut conn, 409, "text/plain", b"the walls have changed: read /play/info again"),
                (v, Some(want)) => match tiles.encode(&want) {
                    None => respond(&mut conn, 400, "text/plain", b"too many tiles"),
                    Some(body) => {
                        let cache = if v.is_some() { "Cache-Control: public, max-age=31536000, immutable\r\n" } else { "Cache-Control: no-store\r\n" };
                        let (body, extra) = maybe_gzip(head.gzip, body, cache);
                        respond_with(&mut conn, 200, "application/octet-stream", &body, &extra)
                    }
                },
            }
        }
        _ if cfg.no_browser => respond(&mut conn, 404, "text/plain", b"not found"),
        p => static_file(&mut conn, cfg.ui_dir.as_deref(), p),
    }
}

fn static_file(conn: &mut TcpStream, ui: Option<&Path>, path: &str) -> std::io::Result<()> {
    let Some(root) = ui else { return respond(conn, 404, "text/plain", b"no UI directory, build the UI or pass --ui-dir") };
    let rel = path.trim_start_matches('/');
    // The web port serves the play page only, not the admin console that shares the build.
    let rel = if rel.is_empty() { "play.html" } else { rel };
    let safe = rel.split('/').all(|p| !p.is_empty() && !p.starts_with('.') && !p.contains('\\') && !p.contains('\0'));
    if !safe || rel == "index.html" {
        return respond(conn, 404, "text/plain", b"not found");
    }
    let file = root.join(rel);
    match std::fs::read(&file) {
        Ok(bytes) => {
            let ctype = match file.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                Some("svg") => "image/svg+xml",
                Some("json") => "application/json",
                Some("woff2") => "font/woff2",
                Some("woff") => "font/woff",
                Some("png") => "image/png",
                Some("ico") => "image/x-icon",
                _ => "application/octet-stream",
            };
            respond(conn, 200, ctype, &bytes)
        }
        Err(_) => respond(conn, 404, "text/plain", b"not found"),
    }
}

/// Decrements the per-address count when a relayed connection ends, however it ends.
struct Slot<'a> {
    gw: &'a Gateway,
    ip: [u8; 4],
    port: u16,
}

impl Drop for Slot<'_> {
    fn drop(&mut self) {
        if let Ok(mut m) = self.gw.ports.lock() {
            m.remove(&self.port);
        }
        if let Ok(mut m) = self.gw.per_ip.lock() {
            if let Some(n) = m.get_mut(&self.ip) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    m.remove(&self.ip);
                }
            }
        }
    }
}

fn relay(mut conn: TcpStream, cfg: &WebCfg, gw: &Gateway, head: &Head, real: [u8; 4]) -> std::io::Result<()> {
    // A page on another site must not be able to open a game connection from a visitor's browser. Our own pages
    // and the other zones' may.
    if let (Some(o), Some(h)) = (&head.origin, &head.host) {
        let o_host = o.split("://").nth(1).unwrap_or(o);
        if !o_host.eq_ignore_ascii_case(h) && !cfg.zone_hosts.iter().any(|z| z.eq_ignore_ascii_case(o_host)) {
            gw.stats.refused.fetch_add(1, Ordering::Relaxed);
            return respond(&mut conn, 403, "text/plain", b"cross-site connections are not allowed");
        }
    }
    {
        let mut m = gw.per_ip.lock().map_err(|_| ErrorKind::Other)?;
        let total: u32 = m.values().sum();
        let n = m.entry(real).or_insert(0);
        if *n >= cfg.per_ip || total as usize >= cfg.max_conns {
            if *n == 0 {
                m.remove(&real);
            }
            gw.stats.refused.fetch_add(1, Ordering::Relaxed);
            return respond(&mut conn, 429, "text/plain", b"too many connections");
        }
        *n += 1;
    }
    let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
    udp.connect((Ipv4Addr::LOCALHOST, cfg.udp_port))?;
    udp.set_nonblocking(true)?;
    let port = udp.local_addr()?.port();
    if let Ok(mut m) = gw.ports.lock() {
        m.insert(port, real);
    }
    let _slot = Slot { gw, ip: real, port };
    gw.stats.total.fetch_add(1, Ordering::Relaxed);

    let ws_cfg = WebSocketConfig { max_message_size: Some(4096), max_frame_size: Some(4096), ..Default::default() };
    let mut ws = match tungstenite::accept_with_config(conn, Some(ws_cfg)) {
        Ok(ws) => ws,
        Err(_) => return Ok(()),
    };
    ws.get_ref().set_nonblocking(true)?;
    ws.get_ref().set_nodelay(true)?;

    let (tfd, ufd) = (ws.get_ref().as_raw_fd(), udp.as_raw_fd());
    let mut buf = [0u8; 2048];
    let mut tokens = MSG_BURST;
    let mut last = Instant::now();
    let mut heard = Instant::now();
    loop {
        let mut fds = [libc::pollfd { fd: tfd, events: libc::POLLIN, revents: 0 }, libc::pollfd { fd: ufd, events: libc::POLLIN, revents: 0 }];
        // SAFETY: two valid pollfd structs that outlive the call.
        unsafe { libc::poll(fds.as_mut_ptr(), 2, 50) };

        // Game server to browser.
        loop {
            match udp.recv(&mut buf) {
                Ok(n) if n <= MAX_DATAGRAM => {
                    gw.stats.down_bytes.fetch_add(n as u64, Ordering::Relaxed);
                    match ws.write(Message::Binary(buf[..n].to_vec())) {
                        Ok(()) => {}
                        Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                        Err(_) => return Ok(()),
                    }
                }
                Ok(_) => {}
                Err(_) => break, // nothing more waiting
            }
        }

        // Browser to game server.
        loop {
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    heard = Instant::now();
                    let now = Instant::now();
                    tokens = (tokens + now.duration_since(last).as_secs_f64() * MSG_RATE).min(MSG_BURST);
                    last = now;
                    if tokens < 1.0 || b.len() > MAX_DATAGRAM || b.is_empty() {
                        continue; // over the rate or malformed: dropped, not relayed
                    }
                    tokens -= 1.0;
                    gw.stats.up_bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
                    let _ = udp.send(&b);
                }
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => heard = Instant::now(), // pings and the like count as life
                Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => break,
                Err(_) => return Ok(()),
            }
        }
        match ws.flush() {
            Ok(()) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
            Err(_) => return Ok(()),
        }
        if heard.elapsed() > IDLE {
            return Ok(());
        }
    }
}

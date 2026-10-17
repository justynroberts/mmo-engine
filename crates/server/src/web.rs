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
//! client needs (server key, definitions, world geometry). Nothing here exposes
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
    pub stats: GwStats,
}

pub struct WebCfg {
    pub addr: SocketAddr,
    pub ui_dir: Option<PathBuf>,
    pub udp_port: u16,
    pub max_conns: usize,
    pub per_ip: u32,
    pub trust_proxy: bool,
}

impl Gateway {
    pub fn new() -> Arc<Gateway> {
        Arc::new(Gateway { ports: Mutex::default(), per_ip: Mutex::default(), info: RwLock::new("{}".into()), stats: GwStats::default() })
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
            let path = first.next()?.split('?').next()?.to_string();
            let mut h = Head { path, websocket: false, host: None, origin: None, forwarded: None };
            for l in lines {
                let Some((k, v)) = l.split_once(':') else { continue };
                let v = v.trim();
                match k.trim().to_ascii_lowercase().as_str() {
                    "upgrade" => h.websocket = v.eq_ignore_ascii_case("websocket"),
                    "host" => h.host = Some(v.to_string()),
                    "origin" => h.origin = Some(v.to_string()),
                    "x-forwarded-for" => h.forwarded = Some(v.to_string()),
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
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Error",
    };
    write!(
        conn,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    conn.write_all(body)
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
    // A page on another site must not be able to open a game connection from a visitor's browser.
    if let (Some(o), Some(h)) = (&head.origin, &head.host) {
        let o_host = o.split("://").nth(1).unwrap_or(o);
        if !o_host.eq_ignore_ascii_case(h) {
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

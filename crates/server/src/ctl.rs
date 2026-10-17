//! Admin/control plane for development: a tiny HTTP server on localhost that
//! the test UI talks to. It runs on its own thread and never touches the
//! simulation directly: requests become commands that the sim thread services
//! once per tick, so the tick loop stays the only owner of the world.
//!
//! This is a dev tool. It binds to loopback only, checks the Host header (DNS
//! rebinding) and requires a custom header on writes (cross-site requests).
//! It has no login and must not be exposed to a network.

use std::fmt::Write as _;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use proto::game::GameFile;
use proto::secure::to_hex;
use proto::world::World;
use serde::Deserialize;
use serde_json::json;

use crate::net::{Clients, Handshake, NetStats};
use crate::sim::{PendingAction, Sim, DRIVEN, NONE, NO_ARCH};

const MAX_BODY: usize = 256 * 1024;

pub enum Cmd {
    Info,
    World,
    GetGame,
    PutGame(String),
    State { limit: usize },
    Spawn(SpawnReq),
    Kill(KillReq),
    Drive(DriveReq),
    Release { id: u32 },
    ChatLog,
    Say(SayReq),
}

#[derive(Deserialize)]
pub struct SayReq {
    pub id: u32,
    #[serde(default)]
    pub shout: bool,
    pub text: String,
}

#[derive(Deserialize)]
pub struct SpawnReq {
    pub archetype: String,
    #[serde(default = "one")]
    pub count: u32,
    pub x: Option<f32>,
    pub y: Option<f32>,
}
fn one() -> u32 {
    1
}

#[derive(Deserialize)]
pub struct KillReq {
    pub id: Option<u32>,
    #[serde(default)]
    pub all_bots: bool,
}

#[derive(Deserialize)]
pub struct DriveReq {
    pub id: u32,
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
    /// Action name to perform on the nearest entity in range.
    pub action: Option<String>,
}

pub struct Request {
    pub cmd: Cmd,
    pub reply: Sender<(u16, String)>,
}

/// Numbers the main loop refreshes once a second for the UI.
#[derive(Default, Clone)]
pub struct LiveStats {
    pub tick_avg_ms: f64,
    pub tick_p99_ms: f64,
    pub recv_ms: f64,
    pub sim_ms: f64,
    pub snap_ms: f64,
    pub send_ms: f64,
    pub in_pps: f64,
    pub out_pps: f64,
    pub mbit: f64,
    pub snap_lanes: usize,
}

/// Fixed facts about this run.
pub struct Params {
    pub cap: usize,
    pub world_size: u16,
    pub seed: u64,
    pub hz: u32,
    pub port: u16,
    pub plain_npcs: usize,
    pub game_path: Option<PathBuf>,
}

/// Everything the sim thread lets a command touch.
pub struct Ctl<'a> {
    pub sim: &'a mut Sim,
    pub clients: &'a mut Clients,
    pub hs: &'a mut Handshake,
    pub game: &'a mut GameFile,
    pub net: &'a NetStats,
    pub live: &'a LiveStats,
    pub params: &'a Params,
}

pub fn start(addr: SocketAddr, ui_dir: Option<PathBuf>) -> std::io::Result<Receiver<Request>> {
    if !addr.ip().is_loopback() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "the admin interface has no authentication, so it only binds to loopback",
        ));
    }
    let listener = TcpListener::bind(addr)?;
    let (tx, rx) = channel::<Request>();
    std::thread::Builder::new()
        .name("admin-http".into())
        .spawn(move || {
            for conn in listener.incoming().flatten() {
                let tx = tx.clone();
                let ui = ui_dir.clone();
                std::thread::spawn(move || {
                    let _ = serve(conn, &tx, ui.as_deref());
                });
            }
        })?;
    Ok(rx)
}

// -------------------------------------------------------------------- http

struct Http {
    method: String,
    path: String,
    query: String,
    host: String,
    admin_header: bool,
    body: Vec<u8>,
}

fn read_request(conn: &mut TcpStream) -> Option<Http> {
    conn.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let mut buf = Vec::with_capacity(2048);
    let mut tmp = [0u8; 4096];
    let head_end = loop {
        if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break p;
        }
        if buf.len() > 16 * 1024 {
            return None;
        }
        let n = conn.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&tmp[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).ok()?.to_string();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let method = first.next()?.to_string();
    let target = first.next()?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let (mut host, mut admin_header, mut clen) = (String::new(), false, 0usize);
    for l in lines {
        if let Some((k, v)) = l.split_once(':') {
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            match k.as_str() {
                "host" => host = v.to_ascii_lowercase(),
                "x-admin" => admin_header = v == "1",
                "content-length" => clen = v.parse().ok()?,
                _ => {}
            }
        }
    }
    if clen > MAX_BODY {
        return None;
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < clen {
        let n = conn.read(&mut tmp).ok()?;
        if n == 0 {
            return None;
        }
        body.extend_from_slice(&tmp[..n]);
    }
    body.truncate(clen);
    Some(Http { method, path: path.to_string(), query: query.to_string(), host, admin_header, body })
}

fn respond(conn: &mut TcpStream, status: u16, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(
        conn,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    conn.write_all(body)
}

fn host_ok(host: &str) -> bool {
    let name = host.rsplit_once(':').map_or(host, |(h, p)| if p.chars().all(|c| c.is_ascii_digit()) { h } else { host });
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
}

fn serve(mut conn: TcpStream, tx: &Sender<Request>, ui: Option<&Path>) -> std::io::Result<()> {
    let Some(req) = read_request(&mut conn) else {
        return respond(&mut conn, 400, "text/plain", b"bad request");
    };
    if !host_ok(&req.host) {
        return respond(&mut conn, 403, "text/plain", b"bad host");
    }
    let is_write = req.method != "GET";
    if is_write && !req.admin_header {
        return respond(&mut conn, 403, "text/plain", b"missing X-Admin header");
    }
    let body_str = || String::from_utf8_lossy(&req.body).into_owned();
    let cmd = match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/api/info") => Some(Cmd::Info),
        ("GET", "/api/world") => Some(Cmd::World),
        ("GET", "/api/game") => Some(Cmd::GetGame),
        ("PUT", "/api/game") => Some(Cmd::PutGame(body_str())),
        ("GET", "/api/state") => {
            let limit = req
                .query
                .split('&')
                .find_map(|kv| kv.strip_prefix("limit="))
                .and_then(|v| v.parse().ok())
                .unwrap_or(4000usize)
                .min(20000);
            Some(Cmd::State { limit })
        }
        ("POST", "/api/spawn") => serde_json::from_slice(&req.body).ok().map(Cmd::Spawn),
        ("POST", "/api/kill") => serde_json::from_slice(&req.body).ok().map(Cmd::Kill),
        ("POST", "/api/drive") => serde_json::from_slice(&req.body).ok().map(Cmd::Drive),
        ("POST", "/api/release") => {
            #[derive(Deserialize)]
            struct R {
                id: u32,
            }
            serde_json::from_slice::<R>(&req.body).ok().map(|r| Cmd::Release { id: r.id })
        }
        ("GET", "/api/chat") => Some(Cmd::ChatLog),
        ("POST", "/api/say") => serde_json::from_slice(&req.body).ok().map(Cmd::Say),
        _ => None,
    };
    if let Some(cmd) = cmd {
        let (rtx, rrx) = channel();
        if tx.send(Request { cmd, reply: rtx }).is_err() {
            return respond(&mut conn, 503, "application/json", br#"{"error":"server is shutting down"}"#);
        }
        return match rrx.recv_timeout(Duration::from_secs(3)) {
            Ok((status, body)) => respond(&mut conn, status, "application/json", body.as_bytes()),
            Err(_) => respond(&mut conn, 503, "application/json", br#"{"error":"sim thread did not answer in time"}"#),
        };
    }
    if req.path.starts_with("/api/") {
        let known = matches!(
            req.path.as_str(),
            "/api/info" | "/api/world" | "/api/game" | "/api/state" | "/api/spawn" | "/api/kill" | "/api/drive" | "/api/release" | "/api/chat" | "/api/say"
        );
        return if known {
            respond(&mut conn, 400, "application/json", br#"{"error":"bad request body or method"}"#)
        } else {
            respond(&mut conn, 404, "application/json", br#"{"error":"no such endpoint"}"#)
        };
    }
    if req.method != "GET" {
        return respond(&mut conn, 405, "text/plain", b"method not allowed");
    }
    serve_static(&mut conn, ui, &req.path)
}

fn serve_static(conn: &mut TcpStream, ui: Option<&Path>, path: &str) -> std::io::Result<()> {
    let Some(root) = ui else {
        return respond(
            conn,
            200,
            "text/html; charset=utf-8",
            b"<!doctype html><title>mmo-engine</title><p>No UI directory found. Build it with <code>cd ui &amp;&amp; npm install &amp;&amp; npm run build</code>, or pass <code>--ui-dir</code>.",
        );
    };
    let rel = path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    // Plain relative names only: no traversal, no hidden files, no backslashes.
    let safe = rel.split('/').all(|p| !p.is_empty() && !p.starts_with('.') && !p.contains('\\') && !p.contains('\0'));
    if !safe {
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

// ----------------------------------------------------------------- commands

/// Run every waiting command (bounded per tick so a flood cannot stall the sim).
pub fn service(rx: &Receiver<Request>, ctl: &mut Ctl) {
    for _ in 0..8 {
        let Ok(req) = rx.try_recv() else { return };
        let out = handle(req.cmd, ctl);
        let _ = req.reply.send(out);
    }
}

fn err(status: u16, msg: impl AsRef<str>) -> (u16, String) {
    (status, json!({ "ok": false, "error": msg.as_ref() }).to_string())
}

fn handle(cmd: Cmd, c: &mut Ctl) -> (u16, String) {
    match cmd {
        Cmd::Info => (
            200,
            json!({
                "server_key": to_hex(&c.hs.keys.public),
                "content_hash": format!("{:016x}", c.hs.defs_hash),
                "hz": c.sim.tick_hz,
                "udp_port": c.params.port,
                "world_size": c.sim.world.size,
                "seed": c.sim.world.seed,
                "proto": proto::wire::PROTO_VERSION,
                "capacity": c.params.cap,
                "snapshot_lanes": c.live.snap_lanes,
                "actions": c.sim.defs.actions.iter().map(|a| json!({"id": a.id, "name": a.name, "range": a.range})).collect::<Vec<_>>(),
            })
            .to_string(),
        ),
        Cmd::World => {
            let boxes: Vec<[f32; 4]> = c.sim.world.boxes.iter().map(|b| [b.x0, b.y0, b.x1, b.y1]).collect();
            (200, json!({ "size": c.sim.world.size, "boxes": boxes }).to_string())
        }
        Cmd::GetGame => (200, c.game.to_json()),
        Cmd::PutGame(text) => put_game(&text, c),
        Cmd::State { limit } => (200, state_json(c, limit)),
        Cmd::Spawn(r) => {
            let Some(a) = c.sim.archs.iter().position(|a| a.name == r.archetype) else {
                return err(400, format!("no archetype named '{}'", r.archetype));
            };
            let at = r.x.zip(r.y);
            let mut n = 0;
            for _ in 0..r.count.min(5000) {
                if c.sim.spawn_as(NONE, a as u8, at).is_none() {
                    break;
                }
                n += 1;
            }
            (200, json!({ "ok": true, "spawned": n }).to_string())
        }
        Cmd::Kill(r) => {
            let mut n = 0;
            if r.all_bots {
                for s in 0..c.sim.ents.high {
                    if c.sim.ents.alive[s] && c.sim.ents.owner[s] >= DRIVEN {
                        c.sim.ents.despawn(s);
                        n += 1;
                    }
                }
            } else if let Some(id) = r.id {
                let Some(s) = c.sim.ents.resolve(id) else { return err(404, "no such entity") };
                if c.sim.ents.owner[s] < DRIVEN {
                    return err(409, "that entity belongs to a connected client; it leaves when the client does");
                }
                c.sim.ents.despawn(s);
                n = 1;
            } else {
                return err(400, "give an id or all_bots");
            }
            (200, json!({ "ok": true, "killed": n }).to_string())
        }
        Cmd::Drive(r) => {
            let Some(s) = c.sim.ents.resolve(r.id) else { return err(404, "no such entity") };
            if c.sim.ents.owner[s] < DRIVEN {
                return err(409, "that entity belongs to a connected client");
            }
            c.sim.ents.owner[s] = DRIVEN;
            let q = |v: f32| (v.clamp(-1.0, 1.0) * 127.0).round() as i8;
            c.sim.ents.dir_x[s] = q(r.dx);
            c.sim.ents.dir_y[s] = q(r.dy);
            let mut used = None;
            if let Some(name) = r.action {
                let Some(def) = c.sim.defs.actions.iter().find(|a| a.name == name) else {
                    return err(400, format!("no action named '{name}'"));
                };
                let (x, y, range) = (c.sim.ents.x[s], c.sim.ents.y[s], def.range);
                let id = def.id;
                let mut best: Option<(f32, usize)> = None;
                let e = &c.sim.ents;
                c.sim.grid.for_neighbours(x, y, |t| {
                    let t = t as usize;
                    if t != s {
                        let d2 = (e.x[t] - x).powi(2) + (e.y[t] - y).powi(2);
                        if d2 <= range * range && best.map_or(true, |b| d2 < b.0) {
                            best = Some((d2, t));
                        }
                    }
                });
                if let Some((_, t)) = best {
                    let target = c.sim.ents.id_of(t);
                    c.sim.admin_actions.push(PendingAction { slot: s, action: id, target });
                    used = Some(target);
                }
            }
            (200, json!({ "ok": true, "target": used }).to_string())
        }
        Cmd::ChatLog => {
            let rows: Vec<_> = c.clients.chat_log.iter().map(|(t, from, ch, text)| json!([t, from, ch, text])).collect();
            (200, json!({ "tick": c.sim.tick, "lines": rows }).to_string())
        }
        Cmd::Say(r) => {
            let Some(s) = c.sim.ents.resolve(r.id) else { return err(404, "no such entity") };
            let Some(text) = proto::chat::sanitize(&r.text) else { return err(400, "empty message") };
            let ch = if r.shout { proto::chat::channel::SHOUT } else { proto::chat::channel::SAY };
            crate::net::say_from(c.sim, c.clients, s, ch, &text);
            (200, json!({ "ok": true, "text": text }).to_string())
        }
        Cmd::Release { id } => {
            let Some(s) = c.sim.ents.resolve(id) else { return err(404, "no such entity") };
            if c.sim.ents.owner[s] == DRIVEN {
                c.sim.ents.owner[s] = NONE;
                c.sim.ents.dir_x[s] = 0;
                c.sim.ents.dir_y[s] = 0;
            }
            (200, json!({ "ok": true }).to_string())
        }
    }
}

fn put_game(text: &str, c: &mut Ctl) -> (u16, String) {
    let file = match GameFile::from_json(text) {
        Ok(f) => f,
        Err(e) => return err(400, e),
    };
    let game = match file.build() {
        Ok(g) => g,
        Err(e) => return err(400, e),
    };
    let hash = game.defs.content_hash();
    let mut dropped = 0;
    let reset = hash != c.hs.defs_hash;
    if reset {
        // Different wire schema: every connected client would misparse. Start a
        // fresh world under the new definitions; clients see a content mismatch
        // on reconnect until they load the same game file.
        dropped = c.clients.count;
        let world = World::generate(c.params.world_size, c.params.seed);
        *c.sim = Sim::new(game, world, c.params.cap, c.params.hz, c.params.seed);
        *c.clients = Clients::default();
        c.hs.defs_hash = hash;
        for _ in 0..c.params.plain_npcs {
            c.sim.spawn(NONE);
        }
    } else {
        c.sim.archs = game.archs;
        c.sim.places = game.places;
        c.sim.routes = game.routes;
    }
    c.sim.populate_bots();
    *c.game = file;
    let saved = match &c.params.game_path {
        Some(p) => {
            let tmp = p.with_extension("json.tmp");
            match std::fs::write(&tmp, c.game.to_json()).and_then(|_| std::fs::rename(&tmp, p)) {
                Ok(()) => json!(true),
                Err(e) => json!(format!("not saved: {e}")),
            }
        }
        None => json!(false),
    };
    (
        200,
        json!({ "ok": true, "reset": reset, "dropped_clients": dropped, "content_hash": format!("{hash:016x}"), "saved": saved })
            .to_string(),
    )
}

fn state_json(c: &Ctl, limit: usize) -> String {
    let s = &*c.sim;
    let mut o = String::with_capacity(64 * 1024);
    let l = c.live;
    let n = c.net;
    let _ = write!(
        o,
        r#"{{"tick":{},"hz":{},"live":{},"clients":{},"hash":"{:016x}","#,
        s.tick, s.tick_hz, s.ents.live, c.clients.count, c.hs.defs_hash
    );
    let _ = write!(
        o,
        r#""perf":{{"tick_avg_ms":{:.2},"tick_p99_ms":{:.2},"recv_ms":{:.2},"sim_ms":{:.2},"snap_ms":{:.2},"send_ms":{:.2},"in_pps":{:.0},"out_pps":{:.0},"mbit":{:.2},"lanes":{}}},"#,
        l.tick_avg_ms, l.tick_p99_ms, l.recv_ms, l.sim_ms, l.snap_ms, l.send_ms, l.in_pps, l.out_pps, l.mbit, l.snap_lanes
    );
    let _ = write!(
        o,
        r#""net":{{"hellos":{},"connects":{},"inputs":{},"bad":{},"bad_cookie":{},"auth_fail":{},"rejected":{},"timeouts":{}}},"#,
        n.hellos, n.connects, n.inputs, n.bad, n.bad_cookie, n.auth_fail, n.rejected, n.timeouts
    );
    let _ = write!(
        o,
        r#""game":{{"zaps":{},"respawns":{},"rejected_actions":{}}},"attrs":["#,
        s.stats.zaps, s.stats.respawns, s.stats.rejected_actions
    );
    for (i, a) in s.defs.attrs.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{:?}", a.name);
    }
    o.push_str(r#"],"e":["#);
    let mut first = true;
    let mut shown = 0usize;
    for slot in 0..s.ents.high {
        if !s.ents.alive[slot] {
            continue;
        }
        if shown >= limit {
            break;
        }
        shown += 1;
        if !first {
            o.push(',');
        }
        first = false;
        let kind = match s.ents.owner[slot] {
            NONE => 0,
            DRIVEN => 2,
            _ => 1,
        };
        let arch = if s.ents.arch[slot] == NO_ARCH { -1 } else { s.ents.arch[slot] as i32 };
        let _ = write!(o, "[{},{:.1},{:.1},{},{}", s.ents.id_of(slot), s.ents.x[slot], s.ents.y[slot], arch, kind);
        for a in 0..s.defs.attrs.len() {
            let _ = write!(o, ",{:.1}", s.ents.attrs[a][slot]);
        }
        o.push(']');
    }
    let _ = write!(o, r#"],"shown":{shown}}}"#);
    o
}

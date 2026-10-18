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
use proto::World;
use proto::secure::to_hex;
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
    SetWeather(WeatherReq),
    FireEvent(FireReq),
    Give(GiveReq),
    PutItems(PutItemsReq),
    Inventory { id: u32 },
    God(GodReq),
    WorldFile,
    WorldReload,
}

#[derive(Deserialize)]
pub struct GodReq {
    pub id: u32,
    #[serde(default = "yes")]
    pub on: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
pub struct GiveReq {
    pub id: u32,
    pub item: String,
    #[serde(default = "one")]
    pub count: u32,
}

#[derive(Deserialize)]
pub struct PutItemsReq {
    pub item: String,
    /// How many piles of one item each.
    #[serde(default = "one")]
    pub count: u32,
    pub place: Option<String>,
    pub x: Option<f32>,
    pub y: Option<f32>,
}

fn one() -> u32 {
    1
}

#[derive(Deserialize)]
pub struct WeatherReq {
    /// A weather from the game file, or null to clear.
    pub name: Option<String>,
    /// How long it lasts; 0 means until changed.
    #[serde(default)]
    pub duration_secs: u32,
}

#[derive(Deserialize)]
pub struct FireReq {
    pub name: String,
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
    /// With x and y: scatter the bots within this distance of the point instead of stacking them on it.
    #[serde(default)]
    pub radius: Option<f32>,
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
    pub world_size: u32,
    pub seed: u64,
    pub hz: u32,
    pub port: u16,
    pub plain_npcs: usize,
    pub game_path: Option<PathBuf>,
    pub max_clients: usize,
}

/// What a browser needs to join: the server's key, the definitions' hash and shape, and the world.
/// Everything here is public by nature (a UDP client learns the same from the handshake).
pub fn play_info(sim: &Sim, hs: &Handshake) -> String {
    let all: Vec<[f32; 4]> = sim.world.boxes.iter().map(|b| [b.x0, b.y0, b.x1, b.y1]).collect();
    let ver = hs.gateway.as_ref().map_or(0, |g| g.set_walls(&all));
    // Small worlds ship their walls inline; big ones are fetched by area from /play/walls.
    let inline = all.len() <= crate::web::WALLS_INLINE_MAX;
    let boxes = if inline { all.clone() } else { Vec::new() };
    json!({
        "server_key": to_hex(&hs.keys.public),
        // A string: a u64 does not fit a JavaScript number.
        "content_hash": format!("{:016x}", hs.defs_hash),
        "proto": proto::wire::PROTO_VERSION,
        "hz": sim.tick_hz,
        "world_size": sim.world.size,
        "seed": sim.world.seed,
        "move_speed": sim.defs.move_speed,
        "entity_radius": sim.defs.entity_radius,
        "interest_radius": sim.defs.interest_radius,
        "max_visible": sim.defs.max_visible,
        "attrs": sim.defs.attrs.iter().map(|a| json!({"name": a.name, "min": a.min, "max": a.max, "bits": a.bits, "replicated": a.replicated})).collect::<Vec<_>>(),
        "replicated": sim.defs.replicated,
        "actions": sim.defs.actions.iter().map(|a| json!({"id": a.id, "name": a.name, "range": a.range})).collect::<Vec<_>>(),
        "boxes": boxes,
        "walls": { "ver": format!("{ver:016x}"), "chunk": crate::web::WALL_CHUNK, "count": all.len(), "inline": inline },
        // A world file to fetch from /play/world: terrain, water and objects for a client theme to draw.
        "world": hs.world.as_ref().map(|w| json!({ "ver": format!("{:016x}", w.ver), "bytes": w.bytes.len() })),
        "items": sim.items.iter().map(|i| json!({"name": i.name, "color": i.color, "stack_max": i.stack_max, "usable": !i.uses.is_empty()})).collect::<Vec<_>>(),
        "weather": sim.weather.as_ref().map(|w| json!({"name": w.name, "color": w.color, "intensity": w.intensity})),
        "zone": hs.zone.as_ref().map(|z| z.index),
        "zones": hs.zone.as_ref().map(|z| (0..z.count).map(|i| hs.zone_web.get(i as usize).cloned()).collect::<Vec<_>>()),
    })
    .to_string()
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
    pub bot_dir: Option<&'a crate::botdir::BotDirStatus>,
    pub director: &'a mut crate::director::Director,
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
        ("GET", "/api/worldfile") => Some(Cmd::WorldFile),
        ("POST", "/api/world/reload") => Some(Cmd::WorldReload),
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
        ("POST", "/api/weather") => serde_json::from_slice(&req.body).ok().map(Cmd::SetWeather),
        ("POST", "/api/give") => serde_json::from_slice(&req.body).ok().map(Cmd::Give),
        ("POST", "/api/ground") => serde_json::from_slice(&req.body).ok().map(Cmd::PutItems),
        ("GET", "/api/inventory") => req.query.split('&').find_map(|kv| kv.strip_prefix("id=")).and_then(|v| v.parse().ok()).map(|id| Cmd::Inventory { id }),
        ("POST", "/api/events/fire") => serde_json::from_slice(&req.body).ok().map(Cmd::FireEvent),
        ("POST", "/api/god") => serde_json::from_slice(&req.body).ok().map(Cmd::God),
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
            "/api/info" | "/api/world" | "/api/game" | "/api/state" | "/api/spawn" | "/api/kill" | "/api/drive" | "/api/release" | "/api/chat" | "/api/say" | "/api/give" | "/api/ground" | "/api/inventory" | "/api/god" | "/api/worldfile" | "/api/world/reload"
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
        Cmd::WorldFile => match &c.hs.world {
            Some(w) => (200, String::from_utf8_lossy(&w.bytes).into_owned()),
            None => err(404, "this world has no world file (start the server with --world)"),
        },
        Cmd::WorldReload => {
            // Read the world file again (after running worldgen) and apply it live, as a wall edit is.
            let Some(path) = c.hs.world.as_ref().map(|w| w.path.clone()) else {
                return err(404, "this world has no world file (start the server with --world)");
            };
            let w = match crate::worldfile::load(&path) {
                Ok(w) => w,
                Err(e) => return err(400, e),
            };
            if w.size != c.sim.world.size {
                return err(400, format!("the new file is {} units across and this world is {}; restart the server to change size", w.size, c.sim.world.size));
            }
            let moved = c.sim.replace_world(World::from_boxes(w.size, c.params.seed, w.boxes.clone()));
            let w = std::sync::Arc::new(w);
            c.hs.world = Some(w.clone());
            if let Some(g) = &c.hs.gateway {
                g.set_world(w.bytes.clone());
                g.set_info(play_info(c.sim, c.hs));
            }
            (200, json!({ "ok": true, "ver": format!("{:016x}", w.ver), "walls": w.boxes.len(), "moved_entities": moved }).to_string())
        }
        Cmd::PutGame(text) => apply_game(&text, c),
        Cmd::State { limit } => (200, state_json(c, limit)),
        Cmd::Spawn(r) => {
            let Some(a) = c.sim.archs.iter().position(|a| a.name == r.archetype) else {
                return err(400, format!("no archetype named '{}'", r.archetype));
            };
            let centre = r.x.zip(r.y);
            let spread = r.radius.filter(|v| v.is_finite()).map_or(0.0, |v| v.clamp(0.0, 512.0));
            let mut n = 0;
            for _ in 0..r.count.min(5000) {
                let at = centre.map(|(x, y)| {
                    if spread <= 0.0 {
                        return (x, y);
                    }
                    let (ang, d) = (c.sim.rng.f32() * std::f32::consts::TAU, spread * c.sim.rng.f32().sqrt());
                    (x + ang.cos() * d, y + ang.sin() * d)
                });
                if c.sim.spawn_as(NONE, a as u8, at).is_none() {
                    break;
                }
                n += 1;
            }
            let mut out = json!({ "ok": true, "spawned": n });
            if n < r.count.min(5000) {
                out["note"] = json!(format!("only {n} fitted: there are {} bots, and the rest of the entity table is kept for players", c.sim.ents.bots));
            }
            (200, out.to_string())
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
        Cmd::SetWeather(r) => match c.director.set_weather(c.sim, r.name.as_deref(), r.duration_secs.min(86_400)) {
            Ok(()) => {
                if let Some(g) = &c.hs.gateway {
                    g.set_info(play_info(c.sim, c.hs));
                }
                (200, json!({ "ok": true, "weather": r.name }).to_string())
            }
            Err(e) => err(400, e),
        },
        Cmd::FireEvent(r) => match c.director.fire(&r.name) {
            Ok(()) => (200, json!({ "ok": true, "queued": r.name }).to_string()),
            Err(e) => err(404, e),
        },
        Cmd::Give(r) => {
            let Some(s) = c.sim.ents.resolve(r.id) else { return err(404, "no such entity") };
            let Some(i) = c.sim.items.iter().position(|x| x.name == r.item) else { return err(404, "no such item") };
            if r.count == 0 || r.count > 9999 {
                return err(400, "count must be 1..9999");
            }
            let left = c.sim.give(s, i, r.count);
            if left > 0 {
                // What does not fit in the bag lands at their feet.
                let (x, y) = (c.sim.ents.x[s], c.sim.ents.y[s]);
                let stack = c.sim.items[i].stack_max as u32;
                for n in (0..left.div_ceil(stack)).map(|k| (left - k * stack).min(stack)) {
                    c.sim.put_ground(x, y, i, n as u8);
                }
            }
            (200, json!({ "ok": true, "given": r.count - left, "dropped": left }).to_string())
        }
        Cmd::PutItems(r) => {
            let Some(i) = c.sim.items.iter().position(|x| x.name == r.item) else { return err(404, "no such item") };
            if r.count == 0 || r.count > 500 {
                return err(400, "count must be 1..500");
            }
            let area = match (&r.place, r.x, r.y) {
                (Some(p), _, _) => match c.sim.places.iter().find(|x| &x.name == p) {
                    Some(p) => Some((p.x, p.y, p.r)),
                    None => return err(404, "no such place"),
                },
                (None, Some(x), Some(y)) => Some((x, y, 1.0)),
                _ => None,
            };
            c.sim.scatter_ground(i, r.count, area);
            (200, json!({ "ok": true, "ground": c.sim.ground.len() }).to_string())
        }
        Cmd::Inventory { id } => {
            let Some(s) = c.sim.ents.resolve(id) else { return err(404, "no such entity") };
            (200, json!({ "id": id, "bag": c.sim.inv_names(s).into_iter().map(|(n, k)| json!({"item": n, "count": k})).collect::<Vec<_>>() }).to_string())
        }
        Cmd::God(r) => {
            let Some(s) = c.sim.ents.resolve(r.id) else { return err(404, "no such entity") };
            c.sim.ents.god[s] = r.on;
            (200, json!({ "ok": true, "id": r.id, "god": r.on }).to_string())
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

pub fn apply_game(text: &str, c: &mut Ctl) -> (u16, String) {
    let file = match GameFile::from_json(text) {
        Ok(f) => f,
        Err(e) => return err(400, e),
    };
    let game = match file.build() {
        Ok(g) => g,
        Err(e) => return err(400, e),
    };
    // With a world file, the walls are the file's; the game file's own walls do not apply.
    let world = match &c.hs.world {
        Some(w) => World::from_boxes(w.size, c.params.seed, w.boxes.clone()),
        None => match file.world(c.params.world_size, c.params.seed) {
            Ok(w) => w,
            Err(e) => return err(400, e),
        },
    };
    let hash = game.defs.content_hash();
    // The bots carry over any edit, matched by archetype and attribute name like a save and restore: spawned
    // bots are not thrown away, removed archetypes are dropped and every archetype is topped up to its count.
    // An archetype whose count the edit changes starts again at its new count, so lowering a count removes bots.
    let mut bots = c.sim.bot_records();
    let old_count = |name: &str| c.sim.archs.iter().find(|a| a.name == name).map(|a| a.count);
    let recount: Vec<String> = game.archs.iter().filter(|a| old_count(&a.name) != Some(a.count)).map(|a| a.name.clone()).collect();
    bots.bots.retain(|b| !recount.contains(&b.arch));
    let mut dropped = 0;
    let mut moved = 0;
    let reset = hash != c.hs.defs_hash;
    if reset {
        // Different wire schema: every connected client would misparse. Start a
        // fresh world under the new definitions; clients see a content mismatch
        // on reconnect until they load the same game file.
        dropped = c.clients.count;
        // Characters survive a world restart: save them from the world they are in, before it is replaced.
        crate::net::save_all(c.sim, c.clients, c.hs.store.as_deref());
        *c.sim = Sim::new(game, world, c.params.cap, c.params.hz, c.params.seed);
        c.sim.player_reserve = c.params.max_clients;
        let (limiter, id_limiter) = (c.clients.limiter.take(), c.clients.id_limiter.take());
        *c.clients = Clients::default();
        c.clients.limiter = limiter;
        c.clients.id_limiter = id_limiter;
        c.hs.defs_hash = hash;
        for _ in 0..c.params.plain_npcs {
            c.sim.spawn(NONE);
        }
    } else {
        c.sim.archs = game.archs;
        c.sim.set_items(game.items);
        c.sim.places = game.places;
        c.sim.routes = game.routes;
        if world.boxes != c.sim.world.boxes {
            moved = c.sim.replace_world(world);
        }
    }
    c.sim.populate_from(&bots);
    *c.game = file;
    c.director.sync(c.game, c.sim.tick, c.params.hz);
    if let Some(g) = &c.hs.gateway {
        g.set_info(play_info(c.sim, c.hs));
    }
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
        json!({ "ok": true, "reset": reset, "dropped_clients": dropped, "moved_entities": moved, "content_hash": format!("{hash:016x}"), "saved": saved })
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
        s.tick, s.tick_hz, s.ents.live - s.ghost_count(), c.clients.count, c.hs.defs_hash
    );
    let _ = write!(
        o,
        r#""perf":{{"tick_avg_ms":{:.2},"tick_p99_ms":{:.2},"recv_ms":{:.2},"sim_ms":{:.2},"snap_ms":{:.2},"send_ms":{:.2},"in_pps":{:.0},"out_pps":{:.0},"mbit":{:.2},"lanes":{}}},"#,
        l.tick_avg_ms, l.tick_p99_ms, l.recv_ms, l.sim_ms, l.snap_ms, l.send_ms, l.in_pps, l.out_pps, l.mbit, l.snap_lanes
    );
    let _ = write!(
        o,
        r#""net":{{"hellos":{},"connects":{},"inputs":{},"bad":{},"bad_cookie":{},"auth_fail":{},"rejected":{},"timeouts":{},"chat_in":{},"chat_dropped":{},"takeovers":{},"restored":{},"rate_limited":{},"identity_limited":{}}},"#,
        n.hellos, n.connects, n.inputs, n.bad, n.bad_cookie, n.auth_fail, n.rejected, n.timeouts, n.chat_in, n.chat_dropped, n.takeovers, n.restored, n.rate_limited, n.identity_limited
    );
    let (sv, ld, er) = c.hs.store.as_ref().map_or((0, 0, 0), |st| {
        use std::sync::atomic::Ordering::Relaxed;
        (st.stats.saved.load(Relaxed), st.stats.loaded.load(Relaxed), st.stats.errors.load(Relaxed))
    });
    if let Some(g) = &c.hs.gateway {
        use std::sync::atomic::Ordering::Relaxed;
        let _ = write!(o, r#""web":{{"open":{},"total":{},"refused":{},"up_bytes":{},"down_bytes":{}}},"#, g.stats.open.load(Relaxed), g.stats.total.load(Relaxed), g.stats.refused.load(Relaxed), g.stats.up_bytes.load(Relaxed), g.stats.down_bytes.load(Relaxed));
    }
    if let Some(z) = &c.hs.zone {
        use std::sync::atomic::Ordering::Relaxed;
        let _ = write!(o, r#""zone":{{"index":{},"count":{},"lo":{},"hi":{},"out":{},"in":{},"failed":{},"rejected":{},"ghosts":{},"feeds_out":{},"feeds_in":{}}},"#, z.index, z.count, z.lo, z.hi, z.stats.handoffs_out.load(Relaxed), z.stats.handoffs_in.load(Relaxed), z.stats.failed.load(Relaxed), z.stats.rejected.load(Relaxed), z.stats.ghosts.load(Relaxed), z.stats.feeds_out.load(Relaxed), z.stats.feeds_in.load(Relaxed));
    }
    if let Some(b) = c.bot_dir {
        let _ = write!(o, r#""bot_dir":{},"#, json!({"dir": b.dir, "files": b.files, "bots": b.bots, "error": b.error, "applied": b.applied}));
    }
    {
        let d = &*c.director;
        let _ = write!(
            o,
            r#""director":{},"#,
            json!({
                "weather": d.active.as_ref().map(|a| json!({"name": a.name, "left_secs": a.until.map(|u| u.saturating_sub(s.tick) / s.tick_hz.max(1))})),
                "weathers": d.weather_names(),
                "events": d.event_rows(s.tick, s.tick_hz),
                "log": d.log.iter().rev().take(40).map(|(t, x)| json!([t, x])).collect::<Vec<_>>(),
            })
        );
    }
    let _ = write!(o, r#""store":{{"enabled":{},"saved":{},"loaded":{},"errors":{}}},"#, c.hs.store.is_some(), sv, ld, er);
    // The world file's version, so the console can refetch it after a reload.
    if let Some(w) = &c.hs.world {
        let _ = write!(o, r#""world_ver":"{:016x}","#, w.ver);
    }
    let _ = write!(
        o,
        r#""game":{{"zaps":{},"respawns":{},"rejected_actions":{},"jumps":{},"dodges":{},"ground_items":{}}},"attrs":["#,
        s.stats.zaps, s.stats.respawns, s.stats.rejected_actions, s.stats.jumps, s.stats.dodges, s.ground.len()
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
            crate::sim::GHOST => 3,
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

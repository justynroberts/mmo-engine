//! Scale out: the world is cut into vertical strips, one server process per strip.
//!
//! Every zone process runs the same world (same seed, same game file, same content
//! hash) but only *owns* the entities inside its strip: its bots are kept in it, and a
//! player who walks across a border is handed to the neighbour.
//!
//! Handoff, for a player with a long-term identity:
//!
//! 1. The old zone sees the player a few units past the border and sends the
//!    character (position and attributes) to the new zone over TCP. The frame is
//!    authenticated with a shared secret and carries a strictly increasing sequence
//!    number, so a captured frame cannot be replayed to roll a character back.
//! 2. The new zone parks the character, keyed by identity, and acknowledges.
//! 3. Only then does the old zone tell the client where to go (`REDIRECT`, inside the
//!    encrypted session), repeating it for a few seconds in case a datagram is lost.
//! 4. The client does an ordinary handshake with the new zone using the same identity
//!    key. The new zone finds the parked character when the identity is proven and
//!    uses it instead of anything in its own database.
//!
//! Network I/O is on background threads. The tick only drains channels, so a dead or
//! slow neighbour costs a failed handoff (retried a little later), never a stalled tick.
//!
//! Seeing across a border: every tick each zone sends its neighbours the entities within
//! interest range of the shared border. The neighbour mirrors them as inert *ghosts*
//! (see `sim::GHOST`): they take part in interest management, so players near the border
//! see the other side, but they are not simulated, cannot be targeted, are never saved and
//! are replaced by each new frame (and dropped if the neighbour goes quiet).
//!
//! Known limits: guests have no identity to hand over, so they are stopped at the
//! border; actions do not cross a border (a ghost cannot be hit); only adjacent zones
//! see each other, so a strip must be wider than the interest radius.

use std::io::{Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::io::Tx;
use crate::net::{Client, Clients, ZState};
use crate::sim::{Sim, GHOST};
use std::collections::HashMap;
use std::sync::mpsc::{sync_channel, SyncSender};
use crate::store::Record;
use proto::secure::GUEST;
use proto::wire;

/// How far past a border a player must be before the handoff starts. The new zone then
/// treats them as inside, so walking back needs the same distance again: no ping-pong.
pub const HYSTERESIS: f32 = 6.0;
/// Seconds the old zone keeps repeating the redirect before it lets the client go.
const REDIRECT_SECS: u32 = 3;
/// Seconds a parked character waits for its client.
const PARK_SECS: u32 = 30;
const MAX_FRAME: usize = 64 * 1024;

#[derive(Clone)]
pub struct ZoneCfg {
    pub index: u8,
    pub count: u8,
    /// Where this zone listens for handoffs (TCP).
    pub listen: SocketAddr,
    /// Handoff address of every zone, by index (this zone's own entry is unused).
    pub peers: Vec<String>,
    /// Public UDP address of every zone, by index: what clients are redirected to.
    pub public: Vec<SocketAddrV4>,
    pub secret: Vec<u8>,
}

/// Counters the admin state shows. Shared with the I/O threads.
#[derive(Default)]
pub struct ZoneStats {
    pub handoffs_out: AtomicU64,
    pub handoffs_in: AtomicU64,
    pub failed: AtomicU64,
    pub rejected: AtomicU64,
    /// Ghosts currently mirrored from neighbours.
    pub ghosts: AtomicU64,
    pub feeds_out: AtomicU64,
    pub feeds_in: AtomicU64,
}

pub struct ZoneShared {
    pub index: u8,
    pub count: u8,
    pub lo: f32,
    pub hi: f32,
    pub stats: ZoneStats,
}

#[derive(Serialize, Deserialize)]
struct Frame {
    from: u8,
    seq: u64,
    id: String,
    rec: Record,
}

enum Event {
    Acked([u8; 32]),
    Failed([u8; 32]),
}

struct Out {
    id: [u8; 32],
    rec: Record,
}

pub struct Zone {
    pub shared: Arc<ZoneShared>,
    cfg: ZoneCfg,
    size: f32,
    out: Vec<Option<Sender<Out>>>,
    events: Receiver<Event>,
    inbox: Receiver<([u8; 32], Record)>,
    feed_out: Vec<Option<SyncSender<Vec<u8>>>>,
    feed_ctr: Arc<AtomicU64>,
    ghost_in: Receiver<(u8, Vec<GhostEnt>, usize)>,
    /// (source zone, its entity id) -> our ghost slot.
    ghosts: HashMap<(u8, u32), usize>,
    /// Our tick when each neighbour's feed last arrived.
    last_feed: Vec<u32>,
}

fn mac_key(secret: &[u8]) -> [u8; 32] {
    blake3::derive_key("mmo-engine zone link v1", secret)
}

fn write_frame(s: &mut TcpStream, key: &[u8; 32], f: &Frame) -> std::io::Result<()> {
    let body = serde_json::to_vec(f).map_err(std::io::Error::other)?;
    let mac = blake3::keyed_hash(key, &body);
    let mut buf = Vec::with_capacity(4 + body.len() + 32);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    buf.extend_from_slice(mac.as_bytes());
    s.write_all(&buf)
}

/// Write one frame of raw bytes with its MAC.
fn write_body(s: &mut TcpStream, key: &[u8; 32], body: &[u8]) -> std::io::Result<()> {
    let mac = blake3::keyed_hash(key, body);
    let mut buf = Vec::with_capacity(4 + body.len() + 32);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(body);
    buf.extend_from_slice(mac.as_bytes());
    s.write_all(&buf)
}

/// Read one frame and check its MAC. `Ok(None)` means it was well formed but not authentic.
fn read_body(s: &mut TcpStream, key: &[u8; 32]) -> std::io::Result<Option<Vec<u8>>> {
    let mut len = [0u8; 4];
    s.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n == 0 || n > MAX_FRAME {
        return Err(std::io::Error::other("bad frame length"));
    }
    let mut body = vec![0u8; n + 32];
    s.read_exact(&mut body)?;
    let (b, m) = body.split_at(n);
    let mac = blake3::Hash::from_bytes(m.try_into().unwrap());
    // `Hash` equality is constant time.
    if blake3::keyed_hash(key, b) != mac {
        return Ok(None);
    }
    Ok(Some(b.to_vec()))
}

fn read_frame(s: &mut TcpStream, key: &[u8; 32]) -> std::io::Result<Option<Frame>> {
    Ok(read_body(s, key)?.and_then(|b| serde_json::from_slice(&b).ok()))
}

/// What a neighbour tells us about one entity near the border.
#[derive(Clone, Debug, PartialEq)]
struct GhostEnt {
    rid: u32,
    x: f32,
    y: f32,
    arch: u8,
    attrs: Vec<f32>,
}

/// Feed frame: `from u8 | seq u64 | tick u32 | n_attrs u8 | n u16 | n x (rid u32, x f32, y f32, arch u8, n_attrs x f32)`.
fn encode_feed(from: u8, seq: u64, tick: u32, n_attrs: usize, ents: &[GhostEnt]) -> Vec<u8> {
    let mut b = Vec::with_capacity(16 + ents.len() * (13 + 4 * n_attrs));
    b.push(from);
    b.extend_from_slice(&seq.to_le_bytes());
    b.extend_from_slice(&tick.to_le_bytes());
    b.push(n_attrs as u8);
    b.extend_from_slice(&(ents.len() as u16).to_le_bytes());
    for e in ents {
        b.extend_from_slice(&e.rid.to_le_bytes());
        b.extend_from_slice(&e.x.to_le_bytes());
        b.extend_from_slice(&e.y.to_le_bytes());
        b.push(e.arch);
        for k in 0..n_attrs {
            b.extend_from_slice(&e.attrs.get(k).copied().unwrap_or(0.0).to_le_bytes());
        }
    }
    b
}

fn decode_feed(b: &[u8]) -> Option<(u8, u64, Vec<GhostEnt>, usize)> {
    let from = *b.first()?;
    let seq = u64::from_le_bytes(b.get(1..9)?.try_into().ok()?);
    let na = *b.get(13)? as usize;
    let n = u16::from_le_bytes(b.get(14..16)?.try_into().ok()?) as usize;
    let rec = 13 + 4 * na;
    if b.len() != 16 + n * rec {
        return None;
    }
    let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
    let ents = (0..n)
        .map(|i| {
            let o = 16 + i * rec;
            GhostEnt { rid: u32::from_le_bytes(b[o..o + 4].try_into().unwrap()), x: f(o + 4), y: f(o + 8), arch: b[o + 12], attrs: (0..na).map(|k| f(o + 13 + 4 * k)).collect() }
        })
        .collect();
    Some((from, seq, ents, na))
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn hex(b: &[u8; 32]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

impl Zone {
    /// Bind the handoff listener and start the I/O threads.
    pub fn start(cfg: ZoneCfg, world_size: f32) -> std::io::Result<Zone> {
        let cfg_count = cfg.count as usize;
        let width = world_size / cfg.count as f32;
        let shared = Arc::new(ZoneShared {
            index: cfg.index,
            count: cfg.count,
            lo: width * cfg.index as f32,
            hi: if cfg.index + 1 == cfg.count { world_size } else { width * (cfg.index + 1) as f32 },
            stats: ZoneStats::default(),
        });
        let key = mac_key(&cfg.secret);
        let (itx, inbox) = channel();
        let (etx, events) = channel();
        let (gtx, ghost_in) = channel::<(u8, Vec<GhostEnt>, usize)>();
        let feed_seq = Arc::new(Mutex::new(vec![0u64; cfg.count as usize]));

        // Incoming: one thread per peer connection, a handful at most.
        let listener = TcpListener::bind(cfg.listen)?;
        let last_seq = Arc::new(Mutex::new(vec![0u64; cfg.count as usize]));
        let live = Arc::new(AtomicU64::new(0));
        {
            let (shared, index, count) = (shared.clone(), cfg.index, cfg.count);
            std::thread::spawn(move || {
                for conn in listener.incoming().flatten() {
                    if live.load(Ordering::Relaxed) >= 32 {
                        continue;
                    }
                    live.fetch_add(1, Ordering::Relaxed);
                    let (itx, last_seq, shared, live) = (itx.clone(), last_seq.clone(), shared.clone(), live.clone());
                    let (gtx, feed_seq) = (gtx.clone(), feed_seq.clone());
                    std::thread::spawn(move || {
                        let mut s = conn;
                        let _ = s.set_nodelay(true);
                        // A peer that stalls mid-frame must not hold the thread forever.
                        let _ = s.set_read_timeout(Some(Duration::from_secs(120)));
                        // The first byte says what this connection carries: 'H' handoffs, 'F' the border feed.
                        let mut tag = [0u8; 1];
                        if s.read_exact(&mut tag).is_err() {
                            live.fetch_sub(1, Ordering::Relaxed);
                            return;
                        }
                        if tag[0] == b'F' {
                            while let Ok(Some(body)) = read_body(&mut s, &key) {
                                let Some((from, seq, ents, na)) = decode_feed(&body) else { break };
                                let fresh = (from as usize) < count as usize && from != index && {
                                    let mut ls = feed_seq.lock().unwrap();
                                    let ok = seq > ls[from as usize];
                                    if ok {
                                        ls[from as usize] = seq;
                                    }
                                    ok
                                };
                                if !fresh {
                                    shared.stats.rejected.fetch_add(1, Ordering::Relaxed);
                                    break;
                                }
                                shared.stats.feeds_in.fetch_add(1, Ordering::Relaxed);
                                if gtx.send((from, ents, na)).is_err() {
                                    break;
                                }
                            }
                            live.fetch_sub(1, Ordering::Relaxed);
                            return;
                        }
                        if tag[0] != b'H' {
                            live.fetch_sub(1, Ordering::Relaxed);
                            return;
                        }
                        while let Ok(f) = read_frame(&mut s, &key) {
                            let ok = f.as_ref().is_some_and(|f| {
                                let mut ls = last_seq.lock().unwrap();
                                let fresh = (f.from as usize) < count as usize && f.from != index && f.seq > ls[f.from as usize];
                                if fresh {
                                    ls[f.from as usize] = f.seq;
                                }
                                fresh
                            });
                            let id = f.as_ref().and_then(|f| unhex(&f.id));
                            match (ok, f, id) {
                                (true, Some(f), Some(id)) => {
                                    shared.stats.handoffs_in.fetch_add(1, Ordering::Relaxed);
                                    if itx.send((id, f.rec)).is_err() || s.write_all(&[1]).is_err() {
                                        break;
                                    }
                                }
                                _ => {
                                    shared.stats.rejected.fetch_add(1, Ordering::Relaxed);
                                    let _ = s.write_all(&[0]);
                                    // A bad MAC or a replay: do not keep talking to this peer.
                                    break;
                                }
                            }
                        }
                        live.fetch_sub(1, Ordering::Relaxed);
                    });
                }
            });
        }

        // Outgoing: one thread per neighbour that connects on demand.
        let mut out = Vec::new();
        // Sequence numbers must survive restarts, so start from the wall clock.
        let seq0 = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_micros() as u64);
        let seq = Arc::new(AtomicU64::new(seq0));
        for (j, peer) in cfg.peers.iter().enumerate() {
            if j == cfg.index as usize || j >= cfg.count as usize {
                out.push(None);
                continue;
            }
            let (tx, rx) = channel::<Out>();
            let (peer, etx, seq, from) = (peer.clone(), etx.clone(), seq.clone(), cfg.index);
            std::thread::spawn(move || {
                let mut conn: Option<TcpStream> = None;
                for o in rx {
                    let id = o.id;
                    let sent = (|| -> std::io::Result<bool> {
                        if conn.is_none() {
                            let addr = std::net::ToSocketAddrs::to_socket_addrs(&peer)?.next().ok_or_else(|| std::io::Error::other("no address"))?;
                            let s = TcpStream::connect_timeout(&addr, Duration::from_millis(800))?;
                            s.set_nodelay(true)?;
                            s.set_read_timeout(Some(Duration::from_secs(2)))?;
                            s.set_write_timeout(Some(Duration::from_secs(2)))?;
                            let mut s = s;
                            s.write_all(b"H")?;
                            conn = Some(s);
                        }
                        let s = conn.as_mut().unwrap();
                        write_frame(s, &key, &Frame { from, seq: seq.fetch_add(1, Ordering::Relaxed), id: hex(&o.id), rec: o.rec })?;
                        let mut ack = [0u8; 1];
                        s.read_exact(&mut ack)?;
                        Ok(ack[0] == 1)
                    })();
                    match sent {
                        Ok(true) => {
                            let _ = etx.send(Event::Acked(id));
                        }
                        r => {
                            // Drop the connection so the next try starts clean.
                            conn = None;
                            let _ = r;
                            let _ = etx.send(Event::Failed(id));
                        }
                    }
                }
            });
            out.push(Some(tx));
        }

        // The border feed: one thread per neighbour, latest frame wins (a slow link drops frames, it never queues them).
        let mut feed_out = Vec::new();
        let feed_ctr = Arc::new(AtomicU64::new(seq0));
        for (j, peer) in cfg.peers.iter().enumerate() {
            let adjacent = j + 1 == cfg.index as usize || j == cfg.index as usize + 1;
            if !adjacent || j >= cfg.count as usize {
                feed_out.push(None);
                continue;
            }
            let (tx, rx) = sync_channel::<Vec<u8>>(1);
            let peer = peer.clone();
            std::thread::spawn(move || {
                let mut conn: Option<TcpStream> = None;
                for body in rx {
                    if conn.is_none() {
                        conn = (|| -> std::io::Result<TcpStream> {
                            let addr = std::net::ToSocketAddrs::to_socket_addrs(&peer)?.next().ok_or_else(|| std::io::Error::other("no address"))?;
                            let mut s = TcpStream::connect_timeout(&addr, Duration::from_millis(500))?;
                            s.set_nodelay(true)?;
                            s.set_write_timeout(Some(Duration::from_millis(500)))?;
                            s.write_all(b"F")?;
                            Ok(s)
                        })()
                        .ok();
                    }
                    if let Some(s) = conn.as_mut() {
                        if write_body(s, &key, &body).is_err() {
                            conn = None;
                        }
                    }
                }
            });
            feed_out.push(Some(tx));
        }
        Ok(Zone { shared, size: world_size, cfg, out, events, inbox, feed_out, feed_ctr, ghost_in, ghosts: HashMap::new(), last_feed: vec![0; cfg_count] })
    }

    /// Which zone owns an x coordinate.
    pub fn zone_of(&self, x: f32) -> u8 {
        let w = self.size / self.cfg.count as f32;
        ((x / w).floor().max(0.0) as usize).min(self.cfg.count as usize - 1) as u8
    }

    /// Once a tick, before the sim step: apply the neighbours' border feeds to our ghosts.
    pub fn ingest(&mut self, sim: &mut Sim) {
        // Only the newest frame from each neighbour matters.
        let mut newest: HashMap<u8, (Vec<GhostEnt>, usize)> = HashMap::new();
        while let Ok((from, ents, na)) = self.ghost_in.try_recv() {
            newest.insert(from, (ents, na));
        }
        let r = sim.defs.entity_radius;
        let size = sim.world.size;
        for (from, (ents, na)) in newest {
            // The definitions are identical across zones (the handshake hash guarantees it for clients, the
            // shared game file for us); a frame that disagrees is ignored rather than trusted.
            if na != sim.defs.attrs.len() {
                continue;
            }
            self.last_feed[from as usize] = sim.tick;
            let mut seen = std::collections::HashSet::new();
            for e in ents.iter().take(4096) {
                if !(e.x.is_finite() && e.y.is_finite()) {
                    continue;
                }
                let (x, y) = (e.x.clamp(r, size - r), e.y.clamp(r, size - r));
                let key = (from, e.rid);
                let live = self.ghosts.get(&key).copied().filter(|&s| sim.ents.alive[s] && sim.ents.owner[s] == GHOST);
                let slot = match live {
                    Some(s) => s,
                    None => match sim.spawn_ghost(x, y, e.arch) {
                        Some(s) => {
                            self.ghosts.insert(key, s);
                            s
                        }
                        None => continue,
                    },
                };
                seen.insert(e.rid);
                sim.ents.x[slot] = x;
                sim.ents.y[slot] = y;
                sim.ents.arch[slot] = e.arch;
                for (k, v) in e.attrs.iter().enumerate() {
                    if v.is_finite() {
                        sim.ents.attrs[k][slot] = *v;
                    }
                }
            }
            self.drop_ghosts(sim, |k| k.0 == from && !seen.contains(&k.1));
        }
        // A neighbour that has gone quiet leaves nothing behind.
        let dead: Vec<u8> = (0..self.cfg.count).filter(|&z| self.last_feed[z as usize] != 0 && sim.tick.wrapping_sub(self.last_feed[z as usize]) > sim.tick_hz).collect();
        for z in dead {
            self.drop_ghosts(sim, |k| k.0 == z);
            self.last_feed[z as usize] = 0;
        }
        self.shared.stats.ghosts.store(self.ghosts.len() as u64, Ordering::Relaxed);
    }

    fn drop_ghosts(&mut self, sim: &mut Sim, pred: impl Fn(&(u8, u32)) -> bool) {
        let gone: Vec<(u8, u32)> = self.ghosts.keys().filter(|k| pred(k)).copied().collect();
        for k in gone {
            if let Some(slot) = self.ghosts.remove(&k) {
                if sim.ents.alive[slot] && sim.ents.owner[slot] == GHOST {
                    sim.ents.despawn(slot);
                }
            }
        }
    }

    /// Tell each neighbour what is near the border it shares with us.
    fn send_feeds(&mut self, sim: &Sim, clients: &Clients) {
        let reach = sim.defs.interest_radius + 8.0;
        let (lo, hi) = (self.shared.lo, self.shared.hi);
        let na = sim.defs.attrs.len();
        for j in [self.cfg.index.wrapping_sub(1), self.cfg.index + 1] {
            let Some(Some(tx)) = self.feed_out.get(j as usize) else { continue };
            let east = j > self.cfg.index;
            let mut ents = Vec::new();
            for s in 0..sim.ents.high {
                if !sim.ents.alive[s] || sim.ents.owner[s] == GHOST {
                    continue;
                }
                let x = sim.ents.x[s];
                if (east && x < hi - reach) || (!east && x > lo + reach) {
                    continue;
                }
                // A player we have already handed over is the neighbour's now.
                let o = sim.ents.owner[s];
                if o < crate::sim::DRIVEN {
                    if let Some(Some(c)) = clients.slab.get(o as usize) {
                        if matches!(c.zstate, ZState::Moved { .. }) {
                            continue;
                        }
                    }
                }
                ents.push(GhostEnt { rid: sim.ents.id_of(s), x, y: sim.ents.y[s], arch: sim.ents.arch[s], attrs: (0..na).map(|a| sim.ents.attrs[a][s]).collect() });
                if ents.len() >= 2000 {
                    break;
                }
            }
            let body = encode_feed(self.cfg.index, self.feed_ctr.fetch_add(1, Ordering::Relaxed), sim.tick, na, &ents);
            // A full channel means the link is behind: skip this frame, the next one supersedes it.
            if tx.try_send(body).is_ok() {
                self.shared.stats.feeds_out.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Once a tick, after the sim step: take in parked characters, finish and start
    /// handoffs, and keep guests inside the strip.
    pub fn tick(&mut self, sim: &mut Sim, clients: &mut Clients, tx: &mut Tx, scratch: &mut Vec<u8>) {
        let hz = sim.tick_hz;
        while let Ok((id, rec)) = self.inbox.try_recv() {
            clients.incoming.insert(id, (rec, sim.tick));
        }
        clients.incoming.retain(|_, (_, t)| sim.tick.wrapping_sub(*t) < PARK_SECS * hz);

        while let Ok(ev) = self.events.try_recv() {
            match ev {
                Event::Acked(id) => {
                    self.shared.stats.handoffs_out.fetch_add(1, Ordering::Relaxed);
                    if let Some(&sid) = clients.by_id_get(&id) {
                        if let Some(Some(c)) = clients.slab.get_mut(sid as usize) {
                            if let ZState::Handing { to } = c.zstate {
                                c.zstate = ZState::Moved { to, since: sim.tick };
                            }
                        }
                    }
                }
                Event::Failed(id) => {
                    self.shared.stats.failed.fetch_add(1, Ordering::Relaxed);
                    if let Some(&sid) = clients.by_id_get(&id) {
                        if let Some(Some(c)) = clients.slab.get_mut(sid as usize) {
                            // Try again in two seconds.
                            c.zstate = ZState::Retry { at: sim.tick + 2 * hz };
                        }
                    }
                }
            }
        }

        let (lo, hi) = (self.shared.lo, self.shared.hi);
        let r = sim.defs.entity_radius;
        let mut gone = Vec::new();
        for (sid, slot) in clients.slab.iter_mut().enumerate() {
            let Some(c) = slot else { continue };
            let x = sim.ents.x[c.slot];
            match c.zstate {
                ZState::Here | ZState::Retry { .. } => {
                    if let ZState::Retry { at } = c.zstate {
                        if sim.tick < at {
                            continue;
                        }
                    }
                    let owner = self.zone_of(x);
                    let past = if owner < self.cfg.index { lo - x } else { x - hi };
                    if owner != self.cfg.index && past >= HYSTERESIS && c.proven && c.id != GUEST {
                        if let Some(Some(tx_out)) = self.out.get(owner as usize) {
                            let rec = sim.record_of(c.slot);
                            if tx_out.send(Out { id: c.id, rec }).is_ok() {
                                c.zstate = ZState::Handing { to: owner };
                                continue;
                            }
                        }
                    }
                    if c.id == GUEST || !c.proven {
                        // Nothing to hand over: stop at the border instead.
                        let nx = x.clamp(lo + r, (hi - r).max(lo + r));
                        if nx != x {
                            sim.ents.x[c.slot] = nx;
                        }
                    }
                }
                ZState::Handing { .. } => {}
                ZState::Moved { to, since } => {
                    if sim.tick.wrapping_sub(since) >= REDIRECT_SECS * hz {
                        gone.push(sid as u32);
                        continue;
                    }
                    if let Some(a) = self.cfg.public.get(to as usize) {
                        wire::encode_redirect(scratch, &wire::Redirect { zone: to, ip: a.ip().octets(), port: a.port() });
                        send_inner(c, scratch, tx);
                    }
                }
            }
        }
        self.send_feeds(sim, clients);
        // The new zone owns the character now: free the entity without saving over it.
        for sid in gone {
            if let Some(c) = clients.remove(sid) {
                sim.ents.despawn(c.slot);
            }
        }
    }
}

fn send_inner(c: &mut Client, inner: &[u8], tx: &mut Tx) {
    let mut out = Vec::with_capacity(inner.len() + 32);
    if c.session.seal(inner, &mut out) {
        tx.queue(c.addr, &out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_a_bad_mac_is_not_authentic() {
        let key = mac_key(b"s3cret");
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let mut c = TcpStream::connect(addr).unwrap();
        let (mut s, _) = l.accept().unwrap();
        let f = Frame { from: 1, seq: 9, id: hex(&[7; 32]), rec: Record { x: 3.0, y: 4.0, attrs: [("health".to_string(), 50.0)].into(), inv: vec![("medkit".to_string(), 2)] } };
        write_frame(&mut c, &key, &f).unwrap();
        let got = read_frame(&mut s, &key).unwrap().unwrap();
        assert_eq!((got.from, got.seq, unhex(&got.id), got.rec.x), (1, 9, Some([7; 32]), 3.0));
        write_frame(&mut c, &mac_key(b"other"), &f).unwrap();
        assert!(read_frame(&mut s, &key).unwrap().is_none());
    }

    #[test]
    fn feed_frames_round_trip_and_reject_wrong_lengths() {
        let ents = vec![
            GhostEnt { rid: 7, x: 255.5, y: 12.25, arch: 3, attrs: vec![80.0, 5.0] },
            GhostEnt { rid: 0x0100_0009, x: 1.0, y: 2.0, arch: 255, attrs: vec![100.0, 100.0] },
        ];
        let b = encode_feed(1, 99, 1234, 2, &ents);
        let (from, seq, got, na) = decode_feed(&b).unwrap();
        assert_eq!((from, seq, na), (1, 99, 2));
        assert_eq!(got, ents);
        assert!(decode_feed(&b[..b.len() - 1]).is_none());
        let mut long = b.clone();
        long.push(0);
        assert!(decode_feed(&long).is_none());
        assert!(decode_feed(&[]).is_none());
        assert_eq!(decode_feed(&encode_feed(0, 1, 1, 2, &[])).unwrap().2.len(), 0);
    }

    #[test]
    fn hex_round_trips() {
        let b: [u8; 32] = std::array::from_fn(|i| (i * 9) as u8);
        assert_eq!(unhex(&hex(&b)), Some(b));
        assert_eq!(unhex("zz"), None);
    }
}

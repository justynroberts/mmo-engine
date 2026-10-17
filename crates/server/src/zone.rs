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
//! Known limits: guests have no identity to hand over, so they are stopped at the
//! border; a player near a border does not see entities in the other strip.

use std::io::{Read, Write};
use std::net::{SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::io::Tx;
use crate::net::{Client, Clients, ZState};
use crate::sim::Sim;
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

/// Read one frame and check its MAC. `Ok(None)` means it was well formed but not authentic.
fn read_frame(s: &mut TcpStream, key: &[u8; 32]) -> std::io::Result<Option<Frame>> {
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
    Ok(serde_json::from_slice(b).ok())
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
                    std::thread::spawn(move || {
                        let mut s = conn;
                        let _ = s.set_nodelay(true);
                        // A peer that stalls mid-frame must not hold the thread forever.
                        let _ = s.set_read_timeout(Some(Duration::from_secs(120)));
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
        Ok(Zone { shared, size: world_size, cfg, out, events, inbox })
    }

    /// Which zone owns an x coordinate.
    pub fn zone_of(&self, x: f32) -> u8 {
        let w = self.size / self.cfg.count as f32;
        ((x / w).floor().max(0.0) as usize).min(self.cfg.count as usize - 1) as u8
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
        let f = Frame { from: 1, seq: 9, id: hex(&[7; 32]), rec: Record { x: 3.0, y: 4.0, attrs: [("health".to_string(), 50.0)].into() } };
        write_frame(&mut c, &key, &f).unwrap();
        let got = read_frame(&mut s, &key).unwrap().unwrap();
        assert_eq!((got.from, got.seq, unhex(&got.id), got.rec.x), (1, 9, Some([7; 32]), 3.0));
        write_frame(&mut c, &mac_key(b"other"), &f).unwrap();
        assert!(read_frame(&mut s, &key).unwrap().is_none());
    }

    #[test]
    fn hex_round_trips() {
        let b: [u8; 32] = std::array::from_fn(|i| (i * 9) as u8);
        assert_eq!(unhex(&hex(&b)), Some(b));
        assert_eq!(unhex("zz"), None);
    }
}

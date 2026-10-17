//! Connection state, handshake, input handling and snapshot building.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::net::SocketAddrV4;

use proto::secure::{addr_bytes, data_sid, ServerKeys, Session};
use proto::snapshot::{encode_snapshot, MAX_BASELINE_AGE, RING};
use proto::wire::{self, kind, reject, ClientMsg, Welcome, PROTO_VERSION};
use proto::Entry;

use crate::io::Tx;
use crate::sim::{PendingAction, Sim};

/// Identity-ish hasher for pre-mixed u64 keys: avoids SipHash on the hot path.
#[derive(Default)]
pub struct KeyHasher(u64);
impl Hasher for KeyHasher {
    fn write(&mut self, _: &[u8]) {
        unreachable!("only u64 keys are used")
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = v.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}

pub struct Client {
    pub addr: SocketAddrV4,
    pub session: Session,
    pub client_pub: [u8; 32],
    /// The ACCEPT we sent, kept until the client proves it got it (first valid
    /// DATA packet) so a retransmitted CONNECT can be answered identically.
    pub accept: Option<Vec<u8>>,
    pub slot: usize,
    pub last_seen: u32,
    pub in_seq: u32,
    pub ack_tick: u32,
    pub action: u8,
    pub target: u32,
    pub baseline_tick: u32,
    pub baseline: Vec<Entry>,
    pub chat: proto::chat::ChatServer,
    ring: Vec<(u32, Vec<Entry>)>,
}

#[derive(Default)]
pub struct Clients {
    pub slab: Vec<Option<Client>>,
    free: Vec<u32>,
    by_key: HashMap<u64, u32, BuildHasherDefault<KeyHasher>>,
    pub count: usize,
    /// Chat lines accepted this tick, waiting to be routed: (speaker slot, channel, text).
    pub chat_in: Vec<(usize, u8, String)>,
    /// Recent chat for the console: (tick, speaker id, channel, text).
    pub chat_log: std::collections::VecDeque<(u32, u32, u8, String)>,
}

#[inline]
fn key(a: &SocketAddrV4) -> u64 {
    (u32::from_be_bytes(a.ip().octets()) as u64) << 16 | a.port() as u64
}

impl Clients {
    pub fn find(&self, a: &SocketAddrV4) -> Option<u32> {
        self.by_key.get(&key(a)).copied()
    }

    /// Index the next `insert` will return (it doubles as the session id).
    pub fn next_index(&self) -> u32 {
        self.free.last().copied().unwrap_or(self.slab.len() as u32)
    }

    fn insert(&mut self, c: Client) -> u32 {
        let k = key(&c.addr);
        let idx = if let Some(i) = self.free.pop() {
            self.slab[i as usize] = Some(c);
            i
        } else {
            self.slab.push(Some(c));
            (self.slab.len() - 1) as u32
        };
        self.by_key.insert(k, idx);
        self.count += 1;
        idx
    }

    pub fn remove(&mut self, idx: u32) -> Option<Client> {
        let c = self.slab[idx as usize].take()?;
        self.by_key.remove(&key(&c.addr));
        self.free.push(idx);
        self.count -= 1;
        Some(c)
    }
}

#[derive(Default, Clone, Copy)]
pub struct NetStats {
    pub in_pkts: u64,
    pub hellos: u64,
    pub connects: u64,
    pub inputs: u64,
    /// Not parseable at all.
    pub bad: u64,
    /// CONNECT with a missing, wrong, stale or wrong-address cookie.
    pub bad_cookie: u64,
    /// Failed decryption or replayed packets (forgeries, corruption, replays).
    pub auth_fail: u64,
    pub rejected: u64,
    pub timeouts: u64,
    pub chat_in: u64,
    pub chat_dropped: u64,
}

pub struct Handshake {
    pub keys: ServerKeys,
    pub defs_hash: u64,
    pub max_clients: usize,
}

pub fn handle_packet(
    sim: &mut Sim,
    clients: &mut Clients,
    tx: &mut Tx,
    hs: &Handshake,
    stats: &mut NetStats,
    addr: SocketAddrV4,
    data: &[u8],
    scratch: &mut Vec<u8>,
    plain: &mut Vec<u8>,
    now: u32,
) {
    stats.in_pkts += 1;
    match data.first().copied() {
        Some(kind::DATA) => on_data(sim, clients, stats, addr, data, plain),
        Some(kind::HELLO) => {
            stats.hellos += 1;
            if let Some(reply) = hs.keys.on_hello(&addr_bytes(addr.ip().octets(), addr.port()), data, now) {
                tx.queue(addr, &reply);
            } else {
                stats.bad += 1;
            }
        }
        Some(kind::CONNECT) => on_connect(sim, clients, tx, hs, stats, addr, data, scratch, now),
        _ => stats.bad += 1,
    }
}

fn reject_to(tx: &mut Tx, stats: &mut NetStats, addr: SocketAddrV4, why: u8, scratch: &mut Vec<u8>) {
    stats.rejected += 1;
    wire::encode_reject(scratch, why);
    tx.queue(addr, scratch);
}

#[allow(clippy::too_many_arguments)]
fn on_connect(
    sim: &mut Sim,
    clients: &mut Clients,
    tx: &mut Tx,
    hs: &Handshake,
    stats: &mut NetStats,
    addr: SocketAddrV4,
    data: &[u8],
    scratch: &mut Vec<u8>,
    now: u32,
) {
    stats.connects += 1;
    // 1. Cheap: does the sender own the address it claims? Nothing else happens before this.
    let Some(hdr) = hs.keys.check_connect(&addr_bytes(addr.ip().octets(), addr.port()), data, now) else {
        stats.bad_cookie += 1;
        return;
    };
    if let Some(ci) = clients.find(&addr) {
        let c = clients.slab[ci as usize].as_ref().unwrap();
        if c.client_pub == hdr.client_pub {
            // Retransmitted CONNECT: answer with the same ACCEPT, or stay silent once established.
            if let Some(a) = &c.accept {
                tx.queue(addr, a);
            }
            return;
        }
        // Same address, new keys: the client restarted. Replace the old session.
        if let Some(old) = clients.remove(ci) {
            sim.ents.despawn(old.slot);
        }
    }
    if hdr.version != PROTO_VERSION {
        return reject_to(tx, stats, addr, reject::BAD_VERSION, scratch);
    }
    // 2. Capacity before the expensive key agreement.
    if clients.count >= hs.max_clients {
        return reject_to(tx, stats, addr, reject::SERVER_FULL, scratch);
    }
    // 3. Key agreement and decryption.
    let Some(open) = hs.keys.open_connect(&hdr, data) else {
        stats.auth_fail += 1;
        return;
    };
    if open.defs_hash != hs.defs_hash {
        return reject_to(tx, stats, addr, reject::CONTENT_MISMATCH, scratch);
    }
    let Some(slot) = sim.spawn(crate::sim::NONE) else {
        return reject_to(tx, stats, addr, reject::SERVER_FULL, scratch);
    };
    let sid = clients.next_index();
    wire::encode_welcome(
        scratch,
        &Welcome {
            entity: sim.ents.id_of(slot),
            tick: sim.tick,
            tick_hz: sim.tick_hz as u8,
            world_size: sim.world.size as u16,
            world_seed: sim.world.seed,
        },
    );
    let Some((accept, session)) = hs.keys.accept(&open, sid, scratch) else {
        sim.ents.despawn(slot);
        stats.bad += 1;
        return;
    };
    tx.queue(addr, &accept);
    let client = Client {
        addr,
        session,
        client_pub: open.client_pub,
        accept: Some(accept),
        slot,
        last_seen: sim.tick,
        in_seq: 0,
        chat: Default::default(),
        ack_tick: 0,
        action: 0,
        target: 0,
        baseline_tick: 0,
        baseline: Vec::new(),
        ring: (0..RING).map(|_| (0, Vec::new())).collect(),
    };
    let ci = clients.insert(client);
    debug_assert_eq!(ci, sid);
    sim.ents.owner[slot] = ci;
}

fn on_data(sim: &mut Sim, clients: &mut Clients, stats: &mut NetStats, addr: SocketAddrV4, data: &[u8], plain: &mut Vec<u8>) {
    let Some(sid) = data_sid(data) else {
        stats.bad += 1;
        return;
    };
    let Some(Some(c)) = clients.slab.get_mut(sid as usize) else {
        stats.bad += 1;
        return;
    };
    if c.addr != addr {
        stats.auth_fail += 1;
        return;
    }
    let Some(body) = c.session.open(data, plain) else {
        stats.auth_fail += 1;
        return;
    };
    if body.first() == Some(&proto::chat::kind::SEND) {
        let Some((cseq, ch, text)) = proto::chat::decode_send(body) else {
            stats.bad += 1;
            return;
        };
        c.accept = None;
        c.last_seen = sim.tick;
        match c.chat.accept(cseq, ch, &text, sim.tick, sim.tick_hz) {
            proto::chat::Accept::Route { channel, text } => {
                stats.chat_in += 1;
                clients.chat_in.push((c.slot, channel, text));
            }
            proto::chat::Accept::Dropped => stats.chat_dropped += 1,
            proto::chat::Accept::Duplicate => {}
        }
        return;
    }
    let Some(ClientMsg::Input(i)) = wire::decode_client(body) else {
        stats.bad += 1;
        return;
    };
    stats.inputs += 1;
    c.chat.on_client_ack(i.chat_ack);
    c.accept = None;
    c.last_seen = sim.tick;
    // Newest-wins; wrapping compare so seq overflow is harmless.
    if (i.seq.wrapping_sub(c.in_seq) as i32) > 0 {
        c.in_seq = i.seq;
        sim.ents.dir_x[c.slot] = i.dir_x;
        sim.ents.dir_y[c.slot] = i.dir_y;
        if i.action != 0 {
            c.action = i.action;
            c.target = i.target;
        }
    }
    if i.ack_tick > c.ack_tick && i.ack_tick <= sim.tick {
        c.ack_tick = i.ack_tick;
        let (t, v) = &c.ring[i.ack_tick as usize % RING];
        if *t == i.ack_tick {
            // The client confirmed it holds this snapshot: delta against it from now on.
            c.baseline.clear();
            c.baseline.extend_from_slice(v);
            c.baseline_tick = i.ack_tick;
        }
    }
}

/// Radius within which `say` is heard.
pub const SAY_RADIUS: f32 = 64.0;

/// Deliver one line from an entity to everyone who should hear it, and log it.
pub fn say_from(sim: &Sim, clients: &mut Clients, slot: usize, channel: u8, text: &str) {
    let from = sim.ents.id_of(slot);
    let (sx, sy) = (sim.ents.x[slot], sim.ents.y[slot]);
    let r2 = SAY_RADIUS * SAY_RADIUS;
    for c in clients.slab.iter_mut().flatten() {
        let heard = channel == proto::chat::channel::SHOUT || {
            let (dx, dy) = (sim.ents.x[c.slot] - sx, sim.ents.y[c.slot] - sy);
            dx * dx + dy * dy <= r2
        };
        if heard {
            c.chat.deliver(from, channel, text);
        }
    }
    clients.log_chat(sim.tick, from, channel, text);
}

impl Clients {
    pub fn log_chat(&mut self, tick: u32, from: u32, channel: u8, text: &str) {
        if self.chat_log.len() >= 200 {
            self.chat_log.pop_front();
        }
        self.chat_log.push_back((tick, from, channel, text.to_string()));
    }
}

/// Route chat accepted during the input phase and refill rate limits once a second.
pub fn route_chat(sim: &mut Sim, clients: &mut Clients) {
    if sim.tick % sim.tick_hz == 0 {
        for c in clients.slab.iter_mut().flatten() {
            c.chat.refill(1.0);
        }
    }
    let events = std::mem::take(&mut clients.chat_in);
    for (slot, ch, text) in &events {
        say_from(sim, clients, *slot, *ch, text);
    }
    let said = std::mem::take(&mut sim.npc_say);
    for (slot, shout, text) in &said {
        // A bot that died or was removed since it decided to speak stays quiet.
        if sim.ents.alive[*slot] {
            say_from(sim, clients, *slot, if *shout { proto::chat::channel::SHOUT } else { proto::chat::channel::SAY }, text);
        }
    }
    sim.npc_say = said;
    sim.npc_say.clear();
    clients.chat_in = events;
    clients.chat_in.clear();
}

/// Collect queued action requests (at most one per client per tick).
pub fn drain_actions(clients: &mut Clients, out: &mut Vec<PendingAction>) {
    out.clear();
    for c in clients.slab.iter_mut().flatten() {
        if c.action != 0 {
            out.push(PendingAction { slot: c.slot, action: c.action, target: c.target });
            c.action = 0;
        }
    }
}

/// Drop clients we have not heard from. Their entity leaves the world and the
/// slot's generation bumps, so stale ids can never alias a newcomer.
pub fn reap(sim: &mut Sim, clients: &mut Clients, timeout_ticks: u32, stats: &mut NetStats) {
    let mut dead: Vec<u32> = Vec::new();
    for (i, c) in clients.slab.iter().enumerate() {
        if let Some(c) = c {
            if sim.tick.saturating_sub(c.last_seen) > timeout_ticks {
                dead.push(i as u32);
            }
        }
    }
    for i in dead {
        if let Some(c) = clients.remove(i) {
            sim.ents.despawn(c.slot);
            stats.timeouts += 1;
        }
    }
}

#[derive(Default)]
pub struct SnapScratch {
    cand: Vec<(f32, u32)>,
    entries: Vec<Entry>,
    buf: Vec<u8>,
    out: Vec<u8>,
}

#[derive(Default, Clone, Copy)]
pub struct SnapStats {
    pub snapshots: u64,
    pub chat_batches: u64,
    pub full: u64,
    pub entries_visible: u64,
    pub entries_changed: u64,
}

/// One snapshot-building lane: its own scratch, stats and send queue, so
/// lanes never contend. Lane 0 runs on the sim thread; the others on spare cores.
pub struct Worker {
    pub tx: Tx,
    pub sc: SnapScratch,
    pub st: SnapStats,
}

impl Worker {
    pub fn new(fd: i32) -> Worker {
        Worker { tx: Tx::new(fd), sc: SnapScratch::default(), st: SnapStats::default() }
    }
}

/// Build, encrypt and send one snapshot per client. The sim is read-only during
/// this phase, so clients are split into disjoint shards, one per lane.
/// With one lane everything runs on the calling thread.
pub fn build_snapshots(sim: &Sim, clients: &mut Clients, lanes: &mut [Worker], pin_base: Option<usize>) {
    let n = lanes.len().max(1);
    if n == 1 || clients.count < 64 {
        let w = &mut lanes[0];
        build_range(sim, &mut clients.slab, w);
        w.tx.flush();
        return;
    }
    let chunk = ((clients.slab.len() + n - 1) / n).max(1);
    std::thread::scope(|scope| {
        let mut parts = clients.slab.chunks_mut(chunk).zip(lanes.iter_mut());
        let first = parts.next();
        for (i, (shard, w)) in parts.enumerate() {
            scope.spawn(move || {
                if let Some(base) = pin_base {
                    crate::io::pin_to_core(base + 1 + i);
                }
                build_range(sim, shard, w);
                w.tx.flush();
            });
        }
        if let Some((shard, w)) = first {
            build_range(sim, shard, w);
            w.tx.flush();
        }
    });
}

fn build_range(sim: &Sim, shard: &mut [Option<Client>], w: &mut Worker) {
    let Worker { tx, sc, st } = w;
    let r2 = sim.defs.interest_radius * sim.defs.interest_radius;
    let cap = sim.defs.max_visible.saturating_sub(1);
    let tick = sim.tick;
    for c in shard.iter_mut().flatten() {
        let (px, py) = (sim.ents.x[c.slot], sim.ents.y[c.slot]);

        sc.cand.clear();
        let cand = &mut sc.cand;
        let ents = &sim.ents;
        let me = c.slot as u32;
        sim.grid.for_neighbours(px, py, |s| {
            if s != me {
                let dx = ents.x[s as usize] - px;
                let dy = ents.y[s as usize] - py;
                let d2 = dx * dx + dy * dy;
                if d2 <= r2 {
                    cand.push((d2, s));
                }
            }
        });
        if sc.cand.len() > cap {
            sc.cand.select_nth_unstable_by(cap, |a, b| a.0.total_cmp(&b.0));
            sc.cand.truncate(cap);
        }

        sc.entries.clear();
        sc.entries.push(sim.entry_cache[c.slot]);
        for &(_, s) in &sc.cand {
            sc.entries.push(sim.entry_cache[s as usize]);
        }
        sc.entries.sort_unstable_by_key(|e| e.id);

        let (bt, base): (u32, &[Entry]) =
            if c.baseline_tick != 0 && tick.wrapping_sub(c.baseline_tick) < MAX_BASELINE_AGE {
                (c.baseline_tick, &c.baseline)
            } else {
                (0, &[])
            };
        encode_snapshot(&mut sc.buf, &sim.defs, tick, bt, c.in_seq, &sc.entries, base);
        // A session whose counter is exhausted just goes quiet and times out.
        if c.session.seal(&sc.buf, &mut sc.out) {
            tx.queue(c.addr, &sc.out);
        }

        st.snapshots += 1;
        st.entries_visible += sc.entries.len() as u64;
        if bt == 0 {
            st.full += 1;
        }
        st.entries_changed += u16::from_le_bytes([sc.buf[13], sc.buf[14]]) as u64;
        if c.chat.build(&mut sc.buf) && c.session.seal(&sc.buf, &mut sc.out) {
            tx.queue(c.addr, &sc.out);
            st.chat_batches += 1;
        }

        // Remember what we sent so a future ack can become the new baseline.
        let slot = &mut c.ring[tick as usize % RING];
        slot.0 = tick;
        std::mem::swap(&mut slot.1, &mut sc.entries);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::NONE;
    use proto::game::GameFile;
    use proto::secure::ClientHs;
    use proto::world::World;
    use std::os::fd::AsRawFd;

    /// Handshake `n` fake clients in-process (no network) so lanes can be timed in isolation.
    fn fixture(n: usize, world: u16) -> (Sim, Clients, Vec<Session>, std::net::UdpSocket, std::net::UdpSocket) {
        let game = GameFile::default_game().build().unwrap();
        let hash = game.defs.content_hash();
        let mut sim = Sim::new(game, World::generate(world, 1), n + 64, 20, 1);
        let (keys, _) = ServerKeys::generate();
        let sink = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let out = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let dest = match sink.local_addr().unwrap() {
            std::net::SocketAddr::V4(a) => a,
            _ => unreachable!(),
        };
        let mut clients = Clients::default();
        let mut peers = Vec::new();
        for i in 0..n {
            let a = addr_bytes([10, 0, (i >> 8) as u8, i as u8], 4000);
            let mut c = ClientHs::new(None, hash);
            let chal = keys.on_hello(&a, &c.hello(), 100).unwrap();
            c.on_challenge(&chal).unwrap();
            let cp = c.connect_packet().unwrap().to_vec();
            let hdr = keys.check_connect(&a, &cp, 100).unwrap();
            let open = keys.open_connect(&hdr, &cp).unwrap();
            let sid = clients.next_index();
            let (accept, session) = keys.accept(&open, sid, &[0x81; 20]).unwrap();
            let (peer, _) = c.on_accept(&accept).unwrap();
            peers.push(peer);
            let slot = sim.spawn(NONE).unwrap();
            let ci = clients.insert(Client {
                addr: dest,
                session,
                client_pub: [0; 32],
                accept: None,
                slot,
                last_seen: 0,
                in_seq: 0,
                chat: Default::default(),
                ack_tick: 0,
                action: 0,
                target: 0,
                baseline_tick: 0,
                baseline: Vec::new(),
                ring: (0..RING).map(|_| (0, Vec::new())).collect(),
            });
            sim.ents.owner[slot] = ci;
        }
        sim.step(&[]);
        (sim, clients, peers, sink, out)
    }

    #[test]
    fn every_lane_count_sends_every_client_a_decryptable_snapshot() {
        for lanes_n in [1usize, 2, 3] {
            let (sim, mut clients, mut peers, sink, out) = fixture(200, 256);
            sink.set_nonblocking(true).unwrap();
            let mut lanes: Vec<Worker> = (0..lanes_n).map(|_| Worker::new(out.as_raw_fd())).collect();
            build_snapshots(&sim, &mut clients, &mut lanes, None);
            let snaps: u64 = lanes.iter().map(|l| l.st.snapshots).sum();
            assert_eq!(snaps, 200, "{lanes_n} lanes");
            if lanes_n > 1 {
                assert!(lanes.iter().filter(|l| l.st.snapshots > 0).count() > 1, "work was not shared");
            }
            // Every packet that reached the sink must be a valid snapshot for exactly one client.
            let mut buf = [0u8; 2048];
            let (mut ok, mut scratch) = (0, Vec::new());
            while let Ok(n) = sink.recv(&mut buf) {
                let sid = data_sid(&buf[..n]).unwrap() as usize;
                let inner = peers[sid].open(&buf[..n], &mut scratch).expect("authentic");
                assert_eq!(inner[0], kind::SNAPSHOT);
                ok += 1;
            }
            assert!(ok >= 150, "only {ok} of 200 packets arrived ({lanes_n} lanes)");
        }
    }

    #[test]
    fn say_is_heard_only_within_the_radius_and_shout_by_everyone() {
        let (mut sim, mut clients, _peers, _sink, _out) = fixture(3, 512);
        let slots: Vec<usize> = clients.slab.iter().flatten().map(|c| c.slot).collect();
        let place = |sim: &mut Sim, s: usize, x: f32, y: f32| {
            sim.ents.x[s] = x;
            sim.ents.y[s] = y;
        };
        place(&mut sim, slots[0], 100.0, 100.0);
        place(&mut sim, slots[1], 100.0 + SAY_RADIUS - 1.0, 100.0);
        place(&mut sim, slots[2], 100.0 + SAY_RADIUS + 1.0, 100.0);
        say_from(&sim, &mut clients, slots[0], proto::chat::channel::SAY, "psst");
        let pending: Vec<usize> = clients.slab.iter().flatten().map(|c| c.chat.pending()).collect();
        assert_eq!(pending, [1, 1, 0], "speaker and the near listener hear it, the far one does not");
        say_from(&sim, &mut clients, slots[0], proto::chat::channel::SHOUT, "HEY");
        let pending: Vec<usize> = clients.slab.iter().flatten().map(|c| c.chat.pending()).collect();
        assert_eq!(pending, [2, 2, 1]);
        assert_eq!(clients.chat_log.len(), 2);
    }

    /// Run with: cargo test -p server --release lane_scaling -- --ignored --nocapture
    #[test]
    #[ignore]
    fn lane_scaling() {
        let n: usize = std::env::var("LANE_CLIENTS").ok().and_then(|v| v.parse().ok()).unwrap_or(2000);
        let world: u16 = std::env::var("LANE_WORLD").ok().and_then(|v| v.parse().ok()).unwrap_or(512);
        let (sim, mut clients, _peers, _sink, out) = fixture(n, world);
        for lanes_n in [1usize, 2] {
            let mut lanes: Vec<Worker> = (0..lanes_n).map(|_| Worker::new(out.as_raw_fd())).collect();
            for _ in 0..5 {
                build_snapshots(&sim, &mut clients, &mut lanes, None);
            }
            let mut times = Vec::new();
            for _ in 0..40 {
                let t = std::time::Instant::now();
                build_snapshots(&sim, &mut clients, &mut lanes, None);
                times.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            println!(
                "{n} clients, world {world}, {lanes_n} lane(s): snapshot phase p50 {:.2} ms p99 {:.2} ms",
                times[times.len() / 2],
                times[times.len() * 99 / 100]
            );
        }
    }
}

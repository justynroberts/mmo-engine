//! Connection state, handshake, input handling and snapshot building.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::net::SocketAddrV4;

use proto::snapshot::{encode_snapshot, MAX_BASELINE_AGE, RING};
use proto::wire::{self, reject, ClientMsg, Welcome, PROTO_VERSION};
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
    pub slot: usize,
    pub last_seen: u32,
    pub in_seq: u32,
    pub ack_tick: u32,
    pub action: u8,
    pub target: u32,
    pub baseline_tick: u32,
    pub baseline: Vec<Entry>,
    ring: Vec<(u32, Vec<Entry>)>,
}

#[derive(Default)]
pub struct Clients {
    pub slab: Vec<Option<Client>>,
    free: Vec<u32>,
    by_key: HashMap<u64, u32, BuildHasherDefault<KeyHasher>>,
    pub count: usize,
}

#[inline]
fn key(a: &SocketAddrV4) -> u64 {
    (u32::from_be_bytes(a.ip().octets()) as u64) << 16 | a.port() as u64
}

impl Clients {
    pub fn find(&self, a: &SocketAddrV4) -> Option<u32> {
        self.by_key.get(&key(a)).copied()
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

    fn remove(&mut self, idx: u32) -> Option<Client> {
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
    pub inputs: u64,
    pub bad: u64,
    pub rejected: u64,
    pub timeouts: u64,
}

pub struct Handshake {
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
) {
    stats.in_pkts += 1;
    let Some(msg) = wire::decode_client(data) else {
        stats.bad += 1;
        return;
    };
    match msg {
        ClientMsg::Hello { version, defs_hash } => {
            stats.hellos += 1;
            if let Some(ci) = clients.find(&addr) {
                // Retransmitted hello: answer again, idempotently.
                let slot = clients.slab[ci as usize].as_ref().unwrap().slot;
                send_welcome(sim, tx, addr, slot, scratch);
                return;
            }
            let reason = if version != PROTO_VERSION {
                Some(reject::BAD_VERSION)
            } else if defs_hash != hs.defs_hash {
                Some(reject::CONTENT_MISMATCH)
            } else if clients.count >= hs.max_clients {
                Some(reject::SERVER_FULL)
            } else {
                None
            };
            if let Some(r) = reason {
                stats.rejected += 1;
                wire::encode_reject(scratch, r);
                tx.queue(addr, scratch);
                return;
            }
            let Some(slot) = sim.spawn(crate::sim::NONE) else {
                stats.rejected += 1;
                wire::encode_reject(scratch, reject::SERVER_FULL);
                tx.queue(addr, scratch);
                return;
            };
            let client = Client {
                addr,
                slot,
                last_seen: sim.tick,
                in_seq: 0,
                ack_tick: 0,
                action: 0,
                target: 0,
                baseline_tick: 0,
                baseline: Vec::new(),
                ring: (0..RING).map(|_| (0, Vec::new())).collect(),
            };
            let ci = clients.insert(client);
            sim.ents.owner[slot] = ci;
            send_welcome(sim, tx, addr, slot, scratch);
        }
        ClientMsg::Input(i) => {
            let Some(ci) = clients.find(&addr) else {
                stats.bad += 1;
                return;
            };
            stats.inputs += 1;
            let c = clients.slab[ci as usize].as_mut().unwrap();
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
    }
}

fn send_welcome(sim: &Sim, tx: &mut Tx, addr: SocketAddrV4, slot: usize, scratch: &mut Vec<u8>) {
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
    tx.queue(addr, scratch);
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
}

#[derive(Default, Clone, Copy)]
pub struct SnapStats {
    pub snapshots: u64,
    pub full: u64,
    pub entries_visible: u64,
    pub entries_changed: u64,
}

/// Build and queue one snapshot per client: interest-filtered, nearest-first
/// capped, delta-compressed against the last acked snapshot.
pub fn build_snapshots(
    sim: &Sim,
    clients: &mut Clients,
    tx: &mut Tx,
    sc: &mut SnapScratch,
    st: &mut SnapStats,
) {
    let r2 = sim.defs.interest_radius * sim.defs.interest_radius;
    let cap = sim.defs.max_visible.saturating_sub(1);
    let tick = sim.tick;
    for c in clients.slab.iter_mut().flatten() {
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
        tx.queue(c.addr, &sc.buf);

        st.snapshots += 1;
        st.entries_visible += sc.entries.len() as u64;
        if bt == 0 {
            st.full += 1;
        }
        st.entries_changed += u16::from_le_bytes([sc.buf[13], sc.buf[14]]) as u64;

        // Remember what we sent so a future ack can become the new baseline.
        let slot = &mut c.ring[tick as usize % RING];
        slot.0 = tick;
        std::mem::swap(&mut slot.1, &mut sc.entries);
    }
}

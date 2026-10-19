//! Snapshot encoding with delta compression against the last *acknowledged*
//! snapshot, and the matching client-side view.
//!
//! Layout:
//! `kind u8 | tick u32 | baseline_tick u32 | last_input_seq u32 | n_changed u16 | n_removed u16`
//! then `n_changed` entries (`id u32, x u32, y u32, replicated attrs...`), then
//! `n_removed` entity ids. Entry lists are sorted by id, so both encode and
//! decode are linear merges with no hashing and no allocation.
//!
//! The server only deltas against a snapshot the client has acked, so a lost
//! packet can never corrupt the client's state: the worst case is that the next
//! delta is a bit bigger, or a full snapshot if the ack is too old.

use crate::defs::Defs;
use crate::wire::{kind, put_u16, put_u32, put_u8, Reader};

pub const MAX_REPL: usize = 4;
/// Snapshots remembered on each side. Server falls back to a full snapshot
/// before the baseline can age out of the client's ring.
pub const RING: usize = 16;
pub const MAX_BASELINE_AGE: u32 = 12;
pub const HEADER_LEN: usize = 17;
/// A snapshot entry's fixed part: id, x and y (u32 each) and the layer (i8), before its attributes.
pub const ENTRY_LEN: usize = 13;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Entry {
    pub id: u32,
    pub x: u32,
    pub y: u32,
    /// The layer its feet are in (`world::GROUND` on the bare ground): how high it stands on built blocks.
    pub z: i8,
    /// Quantised replicated attributes, in `Defs::replicated` order.
    pub attrs: [u16; MAX_REPL],
}

#[derive(Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    NotSnapshot,
    /// Delta against a tick this client no longer holds. Ignore the packet; the
    /// next ack makes the server send a baseline we do have (or a full one).
    MissingBaseline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotInfo {
    pub tick: u32,
    pub baseline_tick: u32,
    pub last_input_seq: u32,
    pub changed: usize,
    pub removed: usize,
    /// Old or duplicate packet; state was not modified.
    pub stale: bool,
}

fn write_entry(out: &mut Vec<u8>, defs: &Defs, e: &Entry) {
    put_u32(out, e.id);
    put_u32(out, e.x);
    put_u32(out, e.y);
    put_u8(out, e.z as u8);
    for (k, &attr) in defs.replicated.iter().enumerate() {
        if defs.attr_wire_bytes(attr) == 1 {
            put_u8(out, e.attrs[k] as u8);
        } else {
            put_u16(out, e.attrs[k]);
        }
    }
}

fn read_entry(r: &mut Reader, defs: &Defs) -> Option<Entry> {
    let mut e = Entry { id: r.u32()?, x: r.u32()?, y: r.u32()?, z: r.i8()?, attrs: [0; MAX_REPL] };
    for (k, &attr) in defs.replicated.iter().enumerate() {
        e.attrs[k] = if defs.attr_wire_bytes(attr) == 1 { r.u8()? as u16 } else { r.u16()? };
    }
    Some(e)
}

/// Encode `cur` as a delta against `base`. Both must be sorted by entity id.
/// Pass `baseline_tick = 0` and an empty `base` for a full snapshot.
pub fn encode_snapshot(
    out: &mut Vec<u8>,
    defs: &Defs,
    tick: u32,
    baseline_tick: u32,
    last_input_seq: u32,
    cur: &[Entry],
    base: &[Entry],
) {
    debug_assert!(defs.replicated.len() <= MAX_REPL);
    out.clear();
    put_u8(out, kind::SNAPSHOT);
    put_u32(out, tick);
    put_u32(out, baseline_tick);
    put_u32(out, last_input_seq);
    put_u16(out, 0); // n_changed, patched below
    put_u16(out, 0); // n_removed, patched below

    // Changed or new entries.
    let mut n_changed = 0u16;
    let mut i = 0;
    for c in cur {
        while i < base.len() && base[i].id < c.id {
            i += 1;
        }
        if i < base.len() && base[i] == *c {
            continue;
        }
        write_entry(out, defs, c);
        n_changed += 1;
    }

    // Entities that were in the baseline but are gone now.
    let mut n_removed = 0u16;
    let mut j = 0;
    for b in base {
        while j < cur.len() && cur[j].id < b.id {
            j += 1;
        }
        if j >= cur.len() || cur[j].id != b.id {
            put_u32(out, b.id);
            n_removed += 1;
        }
    }

    out[13..15].copy_from_slice(&n_changed.to_le_bytes());
    out[15..17].copy_from_slice(&n_removed.to_le_bytes());
}

/// What a client holds of the world: a ring of recent snapshots by tick.
pub struct ClientView {
    ring: Vec<(u32, Vec<Entry>)>,
    pub last_tick: u32,
    changed: Vec<Entry>,
    removed: Vec<u32>,
    scratch: Vec<Entry>,
}

impl Default for ClientView {
    fn default() -> Self {
        Self::new()
    }
}

impl ClientView {
    pub fn new() -> Self {
        ClientView {
            ring: (0..RING).map(|_| (0, Vec::new())).collect(),
            last_tick: 0,
            changed: Vec::new(),
            removed: Vec::new(),
            scratch: Vec::new(),
        }
    }

    /// Current world state as of the newest applied snapshot, sorted by id.
    pub fn entries(&self) -> &[Entry] {
        if self.last_tick == 0 {
            return &[];
        }
        let (t, v) = &self.ring[self.last_tick as usize % RING];
        if *t == self.last_tick {
            v
        } else {
            &[]
        }
    }

    pub fn get(&self, id: u32) -> Option<&Entry> {
        let e = self.entries();
        e.binary_search_by_key(&id, |x| x.id).ok().map(|i| &e[i])
    }
}

pub fn decode_snapshot(
    buf: &[u8],
    defs: &Defs,
    view: &mut ClientView,
) -> Result<SnapshotInfo, DecodeError> {
    let mut r = Reader::new(buf);
    if r.u8().ok_or(DecodeError::Truncated)? != kind::SNAPSHOT {
        return Err(DecodeError::NotSnapshot);
    }
    let tick = r.u32().ok_or(DecodeError::Truncated)?;
    let baseline_tick = r.u32().ok_or(DecodeError::Truncated)?;
    let last_input_seq = r.u32().ok_or(DecodeError::Truncated)?;
    let n_changed = r.u16().ok_or(DecodeError::Truncated)? as usize;
    let n_removed = r.u16().ok_or(DecodeError::Truncated)? as usize;

    let ClientView { ring, last_tick, changed, removed, scratch } = view;

    changed.clear();
    for _ in 0..n_changed {
        changed.push(read_entry(&mut r, defs).ok_or(DecodeError::Truncated)?);
    }
    removed.clear();
    for _ in 0..n_removed {
        removed.push(r.u32().ok_or(DecodeError::Truncated)?);
    }

    let info = SnapshotInfo {
        tick,
        baseline_tick,
        last_input_seq,
        changed: n_changed,
        removed: n_removed,
        stale: false,
    };

    if *last_tick != 0 && tick <= *last_tick {
        return Ok(SnapshotInfo { stale: true, ..info });
    }

    let base: &[Entry] = if baseline_tick == 0 {
        &[]
    } else {
        let (t, v) = &ring[baseline_tick as usize % RING];
        if *t != baseline_tick {
            return Err(DecodeError::MissingBaseline);
        }
        v
    };

    // Three-way merge: baseline + changed - removed, all sorted by id.
    scratch.clear();
    let (mut i, mut j, mut k) = (0usize, 0usize, 0usize);
    while i < base.len() || j < changed.len() {
        let take_changed = match (base.get(i), changed.get(j)) {
            (Some(b), Some(c)) => {
                if b.id == c.id {
                    i += 1; // changed supersedes baseline
                    true
                } else {
                    c.id < b.id
                }
            }
            (None, Some(_)) => true,
            _ => false,
        };
        if take_changed {
            scratch.push(changed[j]);
            j += 1;
        } else {
            let b = base[i];
            i += 1;
            while k < removed.len() && removed[k] < b.id {
                k += 1;
            }
            if k < removed.len() && removed[k] == b.id {
                continue;
            }
            scratch.push(b);
        }
    }

    let slot = &mut ring[tick as usize % RING];
    slot.0 = tick;
    std::mem::swap(&mut slot.1, scratch);
    *last_tick = tick;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Rng;

    fn random_world(rng: &mut Rng, n: usize, id_space: u32) -> Vec<Entry> {
        let mut ids: Vec<u32> = (0..n).map(|_| rng.below(id_space)).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.into_iter()
            .map(|id| Entry {
                id,
                x: rng.below(65536) as u32 * 61,
                y: rng.below(65536) as u32 * 61,
                z: rng.below(256) as u8 as i8,
                attrs: [rng.below(256) as u16, 0, 0, 0],
            })
            .collect()
    }

    fn mutate(rng: &mut Rng, w: &[Entry], id_space: u32) -> Vec<Entry> {
        let mut out: Vec<Entry> = Vec::new();
        for e in w {
            if rng.below(10) == 0 {
                continue; // leaves
            }
            let mut e = *e;
            if rng.below(3) == 0 {
                e.x = e.x.wrapping_add(rng.below(50) as u32);
            }
            if rng.below(8) == 0 {
                e.attrs[0] = rng.below(256) as u16;
            }
            if rng.below(8) == 0 {
                e.z = rng.below(256) as u8 as i8;
            }
            out.push(e);
        }
        for _ in 0..rng.below(6) {
            // some arrive
            let id = rng.below(id_space);
            if out.binary_search_by_key(&id, |e| e.id).is_err() {
                out.push(Entry { id, x: 1, y: 2, z: crate::world::GROUND, attrs: [9, 0, 0, 0] });
            }
        }
        out.sort_by_key(|e| e.id);
        out.dedup_by_key(|e| e.id); // ids must be unique for a valid snapshot
        out
    }

    #[test]
    fn full_snapshot_roundtrip() {
        let defs = Defs::default_game();
        let mut rng = Rng::new(1);
        let w = random_world(&mut rng, 40, 1000);
        let mut buf = Vec::new();
        encode_snapshot(&mut buf, &defs, 5, 0, 77, &w, &[]);
        let mut view = ClientView::new();
        let info = decode_snapshot(&buf, &defs, &mut view).unwrap();
        assert_eq!(info.tick, 5);
        assert_eq!(info.last_input_seq, 77);
        assert_eq!(view.entries(), &w[..]);
    }

    #[test]
    fn delta_chain_with_loss_stays_exact() {
        // Server only deltas against ticks the client has acked; the network
        // drops a third of snapshots and delays acks. State must stay exact.
        let defs = Defs::default_game();
        let mut rng = Rng::new(99);
        let mut truth: Vec<(u32, Vec<Entry>)> = Vec::new();
        let mut world = random_world(&mut rng, 50, 400);
        let mut view = ClientView::new();
        let mut acked: u32 = 0;
        let mut pending_acks: Vec<(u32, u32)> = Vec::new(); // (deliver_at, tick)
        let mut buf = Vec::new();
        let mut delivered = 0;
        let mut delta_bytes = 0usize;
        let mut full_bytes = 0usize;

        for tick in 1..=600u32 {
            world = mutate(&mut rng, &world, 400);
            truth.push((tick, world.clone()));

            let (bt, base): (u32, &[Entry]) = if acked != 0 && tick - acked < MAX_BASELINE_AGE {
                let b = truth.iter().find(|(t, _)| *t == acked).unwrap();
                (acked, &b.1)
            } else {
                (0, &[])
            };
            encode_snapshot(&mut buf, &defs, tick, bt, tick, &world, base);
            assert!(buf.len() <= crate::wire::MAX_PACKET + 4000, "sanity");

            if rng.below(3) != 0 {
                // delivered
                match decode_snapshot(&buf, &defs, &mut view) {
                    Ok(i) => {
                        assert!(!i.stale);
                        assert_eq!(view.entries(), &world[..], "tick {tick}");
                        delivered += 1;
                        pending_acks.push((tick + 1 + rng.below(4), tick));
                        if bt == 0 {
                            full_bytes += buf.len();
                        } else {
                            delta_bytes += buf.len();
                        }
                    }
                    Err(DecodeError::MissingBaseline) => panic!("server used a baseline the client lacks"),
                    Err(e) => panic!("{e:?}"),
                }
            }
            // Ack delivery (acks themselves may arrive out of order).
            pending_acks.retain(|&(at, t)| {
                if at <= tick {
                    acked = acked.max(t);
                    false
                } else {
                    true
                }
            });
        }
        assert!(delivered > 300);
        assert!(delta_bytes > 0);
        let _ = full_bytes;
    }

    #[test]
    fn deltas_are_smaller_than_full() {
        let defs = Defs::default_game();
        let mut rng = Rng::new(5);
        let a = random_world(&mut rng, 60, 1000);
        let mut b = a.clone();
        b[3].x = b[3].x.wrapping_add(1);
        let (mut full, mut delta) = (Vec::new(), Vec::new());
        encode_snapshot(&mut full, &defs, 2, 0, 0, &b, &[]);
        encode_snapshot(&mut delta, &defs, 2, 1, 0, &b, &a);
        assert!(delta.len() < full.len() / 5, "{} vs {}", delta.len(), full.len());
    }

    #[test]
    fn stale_and_missing_baseline_do_not_corrupt_state() {
        let defs = Defs::default_game();
        let mut rng = Rng::new(8);
        let w1 = random_world(&mut rng, 10, 100);
        let w2 = mutate(&mut rng, &w1, 100);
        let mut view = ClientView::new();
        let mut buf = Vec::new();
        encode_snapshot(&mut buf, &defs, 10, 0, 0, &w2, &[]);
        decode_snapshot(&buf, &defs, &mut view).unwrap();
        // Old packet arrives late.
        encode_snapshot(&mut buf, &defs, 9, 0, 0, &w1, &[]);
        let info = decode_snapshot(&buf, &defs, &mut view).unwrap();
        assert!(info.stale);
        assert_eq!(view.entries(), &w2[..]);
        // Delta against a baseline we never held.
        encode_snapshot(&mut buf, &defs, 12, 11, 0, &w1, &w2);
        assert_eq!(decode_snapshot(&buf, &defs, &mut view), Err(DecodeError::MissingBaseline));
        assert_eq!(view.entries(), &w2[..]);
    }

    #[test]
    fn garbage_never_panics() {
        let defs = Defs::default_game();
        let mut rng = Rng::new(123);
        let mut view = ClientView::new();
        for _ in 0..2000 {
            let n = rng.below(80) as usize;
            let mut b: Vec<u8> = (0..n).map(|_| rng.below(256) as u8).collect();
            if !b.is_empty() && rng.below(2) == 0 {
                b[0] = kind::SNAPSHOT;
            }
            let _ = decode_snapshot(&b, &defs, &mut view);
        }
    }

    #[test]
    fn entry_size_matches_budget() {
        // 64 visible entities fully changed must fit one datagram with room to spare.
        let defs = Defs::default_game();
        let mut rng = Rng::new(2);
        let w = random_world(&mut rng, 200, 100_000);
        let w: Vec<Entry> = w.into_iter().take(defs.max_visible).collect();
        let mut buf = Vec::new();
        encode_snapshot(&mut buf, &defs, 1, 0, 0, &w, &[]);
        assert!(buf.len() < crate::wire::MAX_PACKET, "{}", buf.len());
    }
}

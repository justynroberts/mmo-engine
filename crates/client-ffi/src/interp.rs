//! Snapshot interpolation: show other entities slightly in the past, blended between the
//! two snapshots on either side of that moment, so motion is smooth although the server
//! only sends about 20 snapshots a second.

use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ent {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub attrs: [f32; crate::MAX_REPL],
}

pub struct Snap {
    pub tick: u32,
    /// Seconds (on the client's own clock) when it arrived.
    pub at: f64,
    /// Sorted by id.
    pub ents: Vec<Ent>,
}

/// Further apart than this between two snapshots is a teleport (respawn, handoff): do not slide.
const SNAP_DIST: f32 = 48.0;

#[derive(Default)]
pub struct History {
    snaps: VecDeque<Snap>,
}

impl History {
    pub fn clear(&mut self) {
        self.snaps.clear();
    }

    pub fn push(&mut self, s: Snap) {
        if self.snaps.back().is_some_and(|b| s.tick <= b.tick) {
            return;
        }
        self.snaps.push_back(s);
        while self.snaps.len() > 8 {
            self.snaps.pop_front();
        }
    }

    /// The world as it should be drawn at client time `now`, `delay` ticks behind the newest snapshot's clock.
    pub fn sample(&self, now: f64, hz: f64, delay_ticks: f64) -> Vec<Ent> {
        let Some(newest) = self.snaps.back() else { return Vec::new() };
        // Server tick the newest snapshot corresponds to, advanced by how long ago it arrived.
        let t = newest.tick as f64 + (now - newest.at) * hz - delay_ticks;
        // Hold the last snapshot rather than extrapolating far past it.
        let t = t.min(newest.tick as f64);
        let oldest = self.snaps.front().unwrap();
        if t <= oldest.tick as f64 || self.snaps.len() == 1 {
            return if t >= newest.tick as f64 || self.snaps.len() == 1 { newest.ents.clone() } else { oldest.ents.clone() };
        }
        let i = self.snaps.iter().rposition(|s| (s.tick as f64) <= t).unwrap_or(0);
        let (a, b) = (&self.snaps[i], self.snaps.get(i + 1).unwrap_or(&self.snaps[i]));
        if a.tick == b.tick {
            return b.ents.clone();
        }
        let f = ((t - a.tick as f64) / (b.tick - a.tick) as f64).clamp(0.0, 1.0) as f32;
        b.ents
            .iter()
            .map(|e| match a.ents.binary_search_by_key(&e.id, |x| x.id) {
                Ok(k) => {
                    let p = &a.ents[k];
                    if (e.x - p.x).hypot(e.y - p.y) > SNAP_DIST {
                        *e
                    } else {
                        Ent { x: p.x + (e.x - p.x) * f, y: p.y + (e.y - p.y) * f, ..*e }
                    }
                }
                Err(_) => *e,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ent(id: u32, x: f32) -> Ent {
        Ent { id, x, y: 0.0, attrs: [0.0; crate::MAX_REPL] }
    }
    fn snap(tick: u32, at: f64, ents: Vec<Ent>) -> Snap {
        Snap { tick, at, ents }
    }

    #[test]
    fn blends_between_the_two_snapshots_around_the_render_time() {
        let mut h = History::default();
        h.push(snap(10, 0.0, vec![ent(1, 0.0)]));
        h.push(snap(11, 0.05, vec![ent(1, 10.0)]));
        h.push(snap(12, 0.10, vec![ent(1, 20.0)]));
        // At client time 0.10 with a 1.5 tick delay we are at tick 10.5 of the server's clock.
        let s = h.sample(0.10, 20.0, 1.5);
        assert!((s[0].x - 5.0).abs() < 1e-3, "{:?}", s[0]);
        // A little later it has moved on, never backwards.
        let s2 = h.sample(0.12, 20.0, 1.5);
        assert!(s2[0].x > s[0].x);
    }

    #[test]
    fn motion_between_frames_is_smooth_not_stepped() {
        let mut h = History::default();
        for k in 0..6u32 {
            h.push(snap(100 + k, k as f64 * 0.05, vec![ent(1, k as f32 * 20.0)]));
        }
        let mut last = f32::MIN;
        let mut biggest = 0.0f32;
        // 60 fps frames across the last two snapshot intervals: steps are about 1/3 of a snapshot step.
        for f in 0..12 {
            let now = 0.20 + f as f64 / 60.0;
            if now > 0.25 {
                break;
            }
            let x = h.sample(now, 20.0, 2.0)[0].x;
            if last > f32::MIN {
                assert!(x >= last);
                biggest = biggest.max(x - last);
            }
            last = x;
        }
        assert!(biggest > 0.0 && biggest < 20.0 * 0.5, "step {biggest}");
    }

    #[test]
    fn a_teleport_is_not_slid_across_the_map() {
        let mut h = History::default();
        h.push(snap(1, 0.0, vec![ent(1, 0.0)]));
        h.push(snap(2, 0.05, vec![ent(1, 400.0)]));
        let s = h.sample(0.05, 20.0, 0.5);
        assert!(s[0].x == 400.0, "{:?}", s[0]);
    }

    #[test]
    fn new_entities_appear_and_gone_ones_vanish() {
        let mut h = History::default();
        h.push(snap(1, 0.0, vec![ent(1, 0.0), ent(2, 5.0)]));
        h.push(snap(2, 0.05, vec![ent(1, 1.0), ent(3, 9.0)]));
        let s = h.sample(0.05, 20.0, 0.5);
        let ids: Vec<u32> = s.iter().map(|e| e.id).collect();
        assert_eq!(ids, vec![1, 3]);
    }

    #[test]
    fn holds_the_newest_snapshot_when_the_network_stalls() {
        let mut h = History::default();
        h.push(snap(1, 0.0, vec![ent(1, 0.0)]));
        h.push(snap(2, 0.05, vec![ent(1, 10.0)]));
        let s = h.sample(5.0, 20.0, 2.0);
        assert_eq!(s[0].x, 10.0);
    }

    #[test]
    fn old_or_duplicate_snapshots_are_ignored() {
        let mut h = History::default();
        h.push(snap(5, 0.0, vec![ent(1, 5.0)]));
        h.push(snap(4, 0.01, vec![ent(1, 4.0)]));
        h.push(snap(5, 0.02, vec![ent(1, 9.0)]));
        assert_eq!(h.sample(1.0, 20.0, 0.0)[0].x, 5.0);
    }
}

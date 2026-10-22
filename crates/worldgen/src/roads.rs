//! Roads: avenues from the city's gates to its square, each village's high street, and a network joining them and
//! the forts. The network grows outward from the city: each place in turn is joined to the nearest road already
//! laid, by the cheapest path over the land (gentle slopes, round anything solid, across as little river as it
//! can, and along an existing road where one goes the same way, so roads merge rather than run side by side).
//! Paths are straightened where nothing is in the way and then rounded off.

use super::{Land, Layout, River, RiverIndex, Rng, RIVER_HALF, RIVER_LIP};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

/// Grid the paths are found on, in world units (the engine's road cell).
const STEP: f32 = 4.0;
/// Half the width of the roads between places, of a spur to a fort, and of a city avenue.
pub const MAIN_HALF: f32 = 3.0;
pub const SPUR_HALF: f32 = 2.2;
pub const AVENUE_HALF: f32 = 4.0;
/// Half the width of a city street (the gap between blocks is 10).
pub const STREET_HALF: f32 = 3.5;
/// What a step costs over and above its length: per unit of climb, and per cell of river.
const CLIMB_COST: f32 = 6.0;
const RIVER_COST: f32 = 30.0;
/// A step along a road already laid costs this fraction of one over open land.
const REUSE: f32 = 0.35;
/// Worlds past this are too big to path every village; they get their high streets and avenues only.
const MAX_NETWORK_SIZE: f32 = 16384.0;

pub struct Road {
    pub half: f32,
    pub pts: Vec<(f32, f32)>,
}

/// A place the network reaches: a village's high street (its two ends), or the door of a fort.
pub enum Place {
    Village([(f32, f32); 2]),
    Fort((f32, f32)),
}

/// The cells (`STEP` units) a road covers, by which road.
#[derive(Default)]
pub struct Cells {
    map: HashMap<(i32, i32), usize>,
}

impl Cells {
    fn add(&mut self, r: &Road, id: usize) {
        for w in r.pts.windows(2) {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let n = ((bx - ax).hypot(by - ay) / (STEP / 2.0)).ceil().max(1.0) as i32;
            for k in 0..=n {
                let t = k as f32 / n as f32;
                let (x, y) = (ax + (bx - ax) * t, ay + (by - ay) * t);
                let reach = (r.half / STEP).ceil() as i32;
                let (ci, cj) = cell(x, y);
                for dj in -reach..=reach {
                    for di in -reach..=reach {
                        let (px, py) = centre(ci + di, cj + dj);
                        if (px - x).hypot(py - y) <= r.half + STEP * 0.5 {
                            self.map.entry((ci + di, cj + dj)).or_insert(id);
                        }
                    }
                }
            }
        }
    }

    /// True within `pad` of any road.
    pub fn near(&self, x: f32, y: f32, pad: f32) -> bool {
        let reach = (pad / STEP).ceil() as i32;
        let (ci, cj) = cell(x, y);
        (-reach..=reach).any(|dj| (-reach..=reach).any(|di| self.map.contains_key(&(ci + di, cj + dj)) && {
            let (px, py) = centre(ci + di, cj + dj);
            (px - x).abs() <= pad + STEP / 2.0 && (py - y).abs() <= pad + STEP / 2.0
        }))
    }
}

fn cell(x: f32, y: f32) -> (i32, i32) {
    ((x / STEP).floor() as i32, (y / STEP).floor() as i32)
}
fn centre(i: i32, j: i32) -> (f32, f32) {
    ((i as f32 + 0.5) * STEP, (j as f32 + 0.5) * STEP)
}

#[derive(PartialEq)]
struct Open(f32, (i32, i32));
impl Eq for Open {}
impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.0.total_cmp(&self.0)
    }
}
impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

pub struct Net<'a> {
    pub land: &'a Land,
    pub l: &'a Layout,
    pub rivers: &'a [River],
    pub ri: &'a RiverIndex,
    pub roads: Vec<Road>,
    pub cells: Cells,
    /// Solid things by 64 unit cell, for the "is it in the way" test.
    solid: HashMap<(i32, i32), Vec<usize>>,
}

impl<'a> Net<'a> {
    pub fn new(land: &'a Land, l: &'a Layout, rivers: &'a [River], ri: &'a RiverIndex) -> Net<'a> {
        let mut solid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, s) in l.things.iter().enumerate().filter(|(_, s)| s.solid) {
            for cx in ((s.b[0] - 8.0) / 64.0).floor() as i32..=((s.b[2] + 8.0) / 64.0).floor() as i32 {
                for cy in ((s.b[1] - 8.0) / 64.0).floor() as i32..=((s.b[3] + 8.0) / 64.0).floor() as i32 {
                    solid.entry((cx, cy)).or_default().push(i);
                }
            }
        }
        Net { land, l, rivers, ri, roads: Vec::new(), cells: Cells::default(), solid }
    }

    /// True if a road of half width `half` centred here would run into something solid or off the world.
    fn blocked(&self, x: f32, y: f32, half: f32) -> bool {
        let edge = half + 4.0;
        if x < edge || y < edge || x > self.l.size - edge || y > self.l.size - edge {
            return true;
        }
        if self.l.coast.as_ref().is_some_and(|c| c.d(x, y) < half + 3.0) {
            return true;
        }
        let pad = half * 0.8;
        self.solid.get(&((x / 64.0).floor() as i32, (y / 64.0).floor() as i32)).is_some_and(|v| {
            v.iter().any(|&i| {
                let b = &self.l.things[i].b;
                x > b[0] - pad && x < b[2] + pad && y > b[1] - pad && y < b[3] + pad
            })
        })
    }

    fn wet(&self, x: f32, y: f32) -> bool {
        self.ri.at(self.rivers, x, y).is_some_and(|(d, _)| d < RIVER_HALF + RIVER_LIP + 1.0)
    }

    pub fn lay(&mut self, r: Road) {
        let id = self.roads.len();
        self.cells.add(&r, id);
        self.roads.push(r);
    }

    /// Join `from` to the nearest road (not counting `except`, a village's own street) by the cheapest path, and lay
    /// it. False if there is no road yet, no way through, or it is too far to look.
    pub fn join(&mut self, from: (f32, f32), half: f32, except: Option<usize>) -> bool {
        if self.l.size > MAX_NETWORK_SIZE || self.roads.is_empty() {
            return false;
        }
        // Look no further than a little past the nearest road point, so a path never searches the whole world.
        let mut nearest = f32::MAX;
        for (k, r) in self.roads.iter().enumerate() {
            if Some(k) == except {
                continue;
            }
            for &(x, y) in &r.pts {
                nearest = nearest.min((x - from.0).hypot(y - from.1));
            }
        }
        if nearest == f32::MAX {
            return false;
        }
        let reach = nearest * 1.4 + 120.0;
        let (lo_i, lo_j) = cell(from.0 - reach, from.1 - reach);
        let (hi_i, hi_j) = cell(from.0 + reach, from.1 + reach);
        let start = cell(from.0, from.1);
        let mut best: HashMap<(i32, i32), (f32, (i32, i32))> = HashMap::new();
        let mut open = BinaryHeap::new();
        best.insert(start, (0.0, start));
        open.push(Open(0.0, start));
        let mut end = None;
        while let Some(Open(c, at)) = open.pop() {
            if best.get(&at).is_some_and(|b| c > b.0) {
                continue;
            }
            if at != start && self.cells.map.get(&at).is_some_and(|&k| Some(k) != except) {
                end = Some(at);
                break;
            }
            let (ax, ay) = centre(at.0, at.1);
            let ha = self.land.raw(ax, ay);
            for (di, dj) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let nb = (at.0 + di, at.1 + dj);
                if nb.0 < lo_i || nb.1 < lo_j || nb.0 > hi_i || nb.1 > hi_j {
                    continue;
                }
                let (bx, by) = centre(nb.0, nb.1);
                if self.blocked(bx, by, half) {
                    continue;
                }
                let len = if di != 0 && dj != 0 { STEP * std::f32::consts::SQRT_2 } else { STEP };
                let mut step = len + (self.land.raw(bx, by) - ha).abs() * CLIMB_COST;
                if self.wet(bx, by) {
                    step += RIVER_COST;
                }
                if self.cells.map.contains_key(&nb) {
                    step *= REUSE;
                }
                let nc = c + step;
                if best.get(&nb).is_none_or(|b| nc < b.0) {
                    best.insert(nb, (nc, at));
                    open.push(Open(nc, nb));
                }
            }
        }
        let Some(end) = end else { return false };
        let mut path = vec![end];
        let mut at = end;
        while at != start {
            at = best[&at].1;
            path.push(at);
        }
        path.reverse();
        let mut pts: Vec<(f32, f32)> = path.iter().map(|&(i, j)| centre(i, j)).collect();
        pts[0] = from;
        let pts = self.round(self.straighten(&pts, half));
        self.lay(Road { half, pts });
        true
    }

    /// Drop the corners a straight line can cut without touching anything solid or crossing more river.
    fn straighten(&self, pts: &[(f32, f32)], half: f32) -> Vec<(f32, f32)> {
        let clear = |a: (f32, f32), b: (f32, f32)| {
            let d = (b.0 - a.0).hypot(b.1 - a.1);
            if d > 90.0 {
                return false;
            }
            let n = (d / 2.0).ceil().max(1.0) as i32;
            let wet_a = self.wet(a.0, a.1) || self.wet(b.0, b.1);
            (0..=n).all(|k| {
                let t = k as f32 / n as f32;
                let (x, y) = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                !self.blocked(x, y, half) && (wet_a || !self.wet(x, y))
            })
        };
        let mut out = vec![pts[0]];
        let mut i = 0;
        while i < pts.len() - 1 {
            let mut j = pts.len() - 1;
            while j > i + 1 && !clear(pts[i], pts[j]) {
                j -= 1;
            }
            out.push(pts[j]);
            i = j;
        }
        out
    }

    /// Round the corners (two passes of corner cutting), keeping both ends where they are.
    fn round(&self, mut pts: Vec<(f32, f32)>) -> Vec<(f32, f32)> {
        for _ in 0..2 {
            if pts.len() < 3 {
                break;
            }
            let mut out = vec![pts[0]];
            for w in pts.windows(2) {
                let ((ax, ay), (bx, by)) = (w[0], w[1]);
                out.push((ax * 0.75 + bx * 0.25, ay * 0.75 + by * 0.25));
                out.push((ax * 0.25 + bx * 0.75, ay * 0.25 + by * 0.75));
            }
            out.remove(1);
            out.pop();
            out.push(*pts.last().unwrap());
            pts = out;
        }
        pts
    }
}

/// Lay the network outward from `centre`: each place in turn, nearest first, joined to the roads already laid, so
/// every road leads back to the first. A village's street is laid as it is reached and joined at its nearer end;
/// now and then its far end joins too, so some roads run through villages instead of ending in them.
pub fn network(net: &mut Net, places: &[Place], rng: &mut Rng, centre: (f32, f32)) {
    let at = |p: &Place| match p {
        Place::Village([a, b]) => ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
        Place::Fort(a) => *a,
    };
    let mut order: Vec<usize> = (0..places.len()).collect();
    order.sort_by(|&a, &b| {
        let d = |p: &Place| (at(p).0 - centre.0).hypot(at(p).1 - centre.1);
        d(&places[a]).total_cmp(&d(&places[b]))
    });
    for i in order {
        match &places[i] {
            Place::Fort(door) => {
                net.join(*door, SPUR_HALF, None);
            }
            Place::Village(ends) => {
                let d = |p: (f32, f32)| (p.0 - centre.0).hypot(p.1 - centre.1);
                let (near, far) = if d(ends[0]) <= d(ends[1]) { (ends[0], ends[1]) } else { (ends[1], ends[0]) };
                let id = net.roads.len();
                net.lay(Road { half: MAIN_HALF, pts: vec![near, far] });
                net.join(near, MAIN_HALF, Some(id));
                if rng.f() < 0.5 {
                    net.join(far, MAIN_HALF, Some(id));
                }
            }
        }
    }
}

/// Points along a road every `every` units, with the direction it runs there.
pub fn along(r: &Road, every: f32) -> Vec<(f32, f32, f32)> {
    let mut out = Vec::new();
    let mut carry = 0.0;
    for w in r.pts.windows(2) {
        let ((ax, ay), (bx, by)) = (w[0], w[1]);
        let d = (bx - ax).hypot(by - ay);
        if d < 1e-3 {
            continue;
        }
        let a = (by - ay).atan2(bx - ax);
        let mut t = carry;
        while t < d {
            out.push((ax + (bx - ax) * t / d, ay + (by - ay) * t / d, a));
            t += every;
        }
        carry = t - d;
    }
    out
}

/// Where roads cross rivers: a point on each crossing and the road's direction there.
pub fn fords(net: &Net) -> Vec<(f32, f32, f32)> {
    let mut out: Vec<(f32, f32, f32)> = Vec::new();
    for r in &net.roads {
        let mut wet = false;
        for (x, y, a) in along(r, 2.0) {
            let w = net.ri.at(net.rivers, x, y).is_some_and(|(d, _)| d < RIVER_HALF);
            if w && !wet && !out.iter().any(|f| (f.0 - x).hypot(f.1 - y) < 20.0) {
                out.push((x, y, a));
            }
            wet = w;
        }
    }
    out
}

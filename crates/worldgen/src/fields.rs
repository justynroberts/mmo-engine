//! Farms: farmsteads along the country roads and lanes, and fields round them and round each village, a patchwork
//! squared to the road it hangs off, with hedgerows along the field edges.
//!
//! A farmstead is a farmhouse (and often a barn) set back from a road, with a track to its door. Fields are laid out
//! from a road: strips running back from it, cut across into fields of different widths, every corner shared by the
//! fields that meet there and nudged a little, so the patchwork has no gaps and no two fields are quite square. A
//! field that would cover water, a road, a building or steep ground is left out (rough ground, or a wood). The west
//! is grazing and hay, the east has more tillage. Fields are not solid and the engine knows nothing of them; a theme
//! draws them (`t: "field"`, see WORLD.md).

use super::roads::{self, Road};
use super::{overlaps, Box4, Land, Layout, River, RiverIndex, Rng, RIVER_HALF, RIVER_LIP};
use std::collections::HashMap;

/// One field: its four corners in order round it, what grows there, and which way the furrows (or the mowing) run.
pub struct Field {
    pub q: [(f32, f32); 4],
    pub kind: &'static str,
    pub furrow: f32,
}

/// What is laid: the fields, the farm tracks (roads), and the hedgerow plants along field edges ("bush" or "tree",
/// x, y, scale).
pub struct Farms {
    pub fields: Vec<Field>,
    pub tracks: Vec<Road>,
    pub hedges: Vec<(&'static str, f32, f32, f32)>,
}

const BUCKET: f32 = 64.0;

fn bucket(x: f32, y: f32) -> (i32, i32) {
    ((x / BUCKET).floor() as i32, (y / BUCKET).floor() as i32)
}

/// Boxes by 64 unit bucket, for "is anything here" without looking at everything.
#[derive(Default)]
struct Index {
    map: HashMap<(i32, i32), Vec<usize>>,
    boxes: Vec<Box4>,
}

impl Index {
    fn add(&mut self, b: Box4) {
        let id = self.boxes.len();
        self.boxes.push(b);
        let (i0, j0) = bucket(b[0], b[1]);
        let (i1, j1) = bucket(b[2], b[3]);
        for j in j0..=j1 {
            for i in i0..=i1 {
                self.map.entry((i, j)).or_default().push(id);
            }
        }
    }
    fn hits(&self, b: &Box4, pad: f32) -> bool {
        let (i0, j0) = bucket(b[0] - pad, b[1] - pad);
        let (i1, j1) = bucket(b[2] + pad, b[3] + pad);
        for j in j0..=j1 {
            for i in i0..=i1 {
                if let Some(v) = self.map.get(&(i, j)) {
                    if v.iter().any(|&k| overlaps(&self.boxes[k], b, pad)) {
                        return true;
                    }
                }
            }
        }
        false
    }
}

fn bbox(q: &[(f32, f32)]) -> Box4 {
    q.iter().fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, p| [b[0].min(p.0), b[1].min(p.1), b[2].max(p.0), b[3].max(p.1)])
}

/// True if (x, y) is inside the convex quad `q`.
pub fn inside(q: &[(f32, f32); 4], x: f32, y: f32) -> bool {
    let mut sign = 0.0f32;
    for k in 0..4 {
        let (a, b) = (q[k], q[(k + 1) % 4]);
        let c = (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        if c != 0.0 {
            if sign != 0.0 && c.signum() != sign {
                return false;
            }
            sign = c.signum();
        }
    }
    true
}

pub struct Ground<'a> {
    pub land: &'a Land,
    pub rivers: &'a [River],
    pub ri: &'a RiverIndex,
    pub roads: &'a roads::Cells,
    /// The city's extent, kept clear of farms.
    pub city: Option<Box4>,
}

impl Ground<'_> {
    fn dry(&self, l: &Layout, x: f32, y: f32) -> bool {
        x > 8.0
            && y > 8.0
            && x < l.size - 8.0
            && y < l.size - 8.0
            && l.coast.as_ref().is_none_or(|c| c.d(x, y) > 10.0)
            && self.ri.at(self.rivers, x, y).is_none_or(|(d, _)| d > RIVER_HALF + RIVER_LIP + 4.0)
    }
}

/// Lay out the farms. `streets` are the villages' high streets (their two ends); farmsteads go along the country
/// roads in `roads`. Farmhouses are added to the layout (solid, as any house).
pub fn lay(l: &mut Layout, g: &Ground, roads_laid: &[Road], streets: &[[(f32, f32); 2]], rng: &mut Rng) -> Farms {
    let mut things = Index::default();
    for s in &l.things {
        if s.t != "city" && s.t != "square" {
            things.add(s.b);
        }
    }
    let mut farms = Farms { fields: Vec::new(), tracks: Vec::new(), hedges: Vec::new() };
    let mut taken = Index::default(); // fields laid so far
    let mut seats: Vec<((f32, f32), f32, f32, f32)> = Vec::new(); // (where on the road, its direction, side, set back)

    // Farmsteads: along the country roads, now and then, set back on one side.
    for r in roads_laid.iter().filter(|r| (roads::TRACK_HALF + 0.1..=roads::MAIN_HALF + 0.05).contains(&r.half)) {
        let len: f32 = r.pts.windows(2).map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).sum();
        if len < 200.0 {
            continue; // a high street, or a stub
        }
        let mut next = rng.range(60.0, 200.0);
        let mut walked = 0.0;
        for (x, y, a) in roads::along(r, 10.0) {
            walked += 10.0;
            if walked < next {
                continue;
            }
            next = walked + rng.range(150.0, 320.0);
            if rng.f() > 0.6 || g.city.is_some_and(|c| overlaps(&c, &[x, y, x, y], 250.0)) {
                continue;
            }
            let side = if rng.f() < 0.5 { -1.0 } else { 1.0 };
            let (nx, ny) = (-a.sin() * side, a.cos() * side);
            let back = r.half + rng.range(12.0, 22.0);
            let (hx, hy) = (x + nx * back, y + ny * back);
            let (w, d) = (rng.range(9.0, 13.0), rng.range(7.5, 10.0));
            let (bw, bd) = if a.cos().abs() > a.sin().abs() { (w, d) } else { (d, w) };
            let house = [hx - bw / 2.0, hy - bd / 2.0, hx + bw / 2.0, hy + bd / 2.0];
            if !g.dry(l, hx, hy) || things.hits(&house, 10.0) || taken.hits(&house, 4.0) || g.roads.near(hx, hy, bw.max(bd) / 2.0 + 2.0) {
                continue;
            }
            let v = rng.v();
            l.add("house", house, v, true);
            l.things.last_mut().unwrap().base = g.land.raw(hx, hy);
            things.add(house);
            // A barn beside it, along the road, more often than not.
            if rng.f() < 0.6 {
                let (ux, uy) = (a.cos(), a.sin());
                let off = bw.max(bd) / 2.0 + 6.0 + rng.range(0.0, 4.0);
                let (bx, by) = (hx + ux * off, hy + uy * off);
                let (w2, d2) = (rng.range(8.0, 12.0), rng.range(6.0, 8.0));
                let (b2w, b2d) = if a.cos().abs() > a.sin().abs() { (w2, d2) } else { (d2, w2) };
                let barn = [bx - b2w / 2.0, by - b2d / 2.0, bx + b2w / 2.0, by + b2d / 2.0];
                if g.dry(l, bx, by) && !things.hits(&barn, 3.0) && !g.roads.near(bx, by, b2w.max(b2d) / 2.0 + 1.5) {
                    l.add("house", barn, rng.v(), true);
                    l.things.last_mut().unwrap().base = g.land.raw(bx, by);
                    things.add(barn);
                }
            }
            // The track from the road to the door.
            let door = (hx - nx * (bd.min(bw) / 2.0 + 1.0), hy - ny * (bd.min(bw) / 2.0 + 1.0));
            farms.tracks.push(Road { half: roads::TRACK_HALF, pts: vec![(x + nx * (r.half - 0.5), y + ny * (r.half - 0.5)), door] });
            seats.push(((x, y), a, side, back + bw.max(bd) / 2.0 + 6.0));
        }
    }

    // Fields: behind each farmstead, and on both sides of each village beyond its houses.
    let mut plots: Vec<((f32, f32), f32, f32, f32, f32, f32)> = Vec::new(); // (origin, dir, side, from, depth, half length)
    for &((x, y), a, side, from) in &seats {
        plots.push(((x, y), a, side, from.min(16.0), rng.range(110.0, 220.0), rng.range(70.0, 150.0)));
        // Across the road too, now and then: a farm's land is often both sides of the lane.
        if rng.f() < 0.4 {
            plots.push(((x, y), a, -side, 6.0, rng.range(70.0, 150.0), rng.range(50.0, 110.0)));
        }
    }
    for e in streets {
        let ((ax, ay), (bx, by)) = (e[0], e[1]);
        let (cx, cy) = ((ax + bx) / 2.0, (ay + by) / 2.0);
        if g.city.is_some_and(|c| overlaps(&c, &[cx, cy, cx, cy], 300.0)) {
            continue;
        }
        let a = (by - ay).atan2(bx - ax);
        let half_len = (bx - ax).hypot(by - ay) / 2.0;
        for side in [-1.0, 1.0] {
            plots.push(((cx, cy), a, side, 60.0, rng.range(150.0, 260.0), half_len + rng.range(60.0, 160.0)));
        }
    }
    let west = l.size * 0.45;
    for (o, a, side, from, depth, half_len) in plots {
        let (ux, uy) = (a.cos(), a.sin());
        let (nx, ny) = (-a.sin() * side, a.cos() * side);
        // Cuts across (along the road) and back (away from it), uneven.
        let mut us = vec![-half_len];
        while *us.last().unwrap() < half_len {
            let w = rng.range(28.0, 75.0);
            us.push((us.last().unwrap() + w).min(half_len));
        }
        let mut vs = vec![from];
        while *vs.last().unwrap() < from + depth {
            let d = rng.range(30.0, 85.0);
            vs.push((vs.last().unwrap() + d).min(from + depth));
        }
        let jit = |rng: &mut Rng, s: f32| rng.range(-s, s);
        let mut grid: Vec<Vec<(f32, f32)>> = Vec::new();
        for (j, &v) in vs.iter().enumerate() {
            let mut row = Vec::new();
            for (i, &u) in us.iter().enumerate() {
                // The road side stays straight; corners further back wander a little.
                let (du, dv) = if j == 0 { (0.0, 0.0) } else { (jit(rng, 5.0), jit(rng, 5.0)) };
                let _ = i;
                row.push((o.0 + ux * (u + du) + nx * (v + dv), o.1 + uy * (u + du) + ny * (v + dv)));
            }
            grid.push(row);
        }
        // This plot's fields share their corners, so their boxes overlap each other: they are checked against other
        // plots' fields only, and join `taken` once the plot is done.
        let mut mine: Vec<Box4> = Vec::new();
        for j in 0..vs.len() - 1 {
            for i in 0..us.len() - 1 {
                let q = [grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]];
                let b = bbox(&q);
                if b[2] - b[0] < 12.0 || b[3] - b[1] < 12.0 {
                    continue;
                }
                let mid = ((q[0].0 + q[2].0) / 2.0, (q[0].1 + q[2].1) / 2.0);
                let probe: Vec<(f32, f32)> = q.iter().copied().chain([mid, ((q[0].0 + q[1].0) / 2.0, (q[0].1 + q[1].1) / 2.0), ((q[2].0 + q[3].0) / 2.0, (q[2].1 + q[3].1) / 2.0)]).collect();
                if probe.iter().any(|&(x, y)| !g.dry(l, x, y) || g.roads.near(x, y, 2.5)) {
                    continue;
                }
                let hs: Vec<f32> = probe.iter().map(|&(x, y)| g.land.raw(x, y)).collect();
                let (lo, hi) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), &h| (a.min(h), b.max(h)));
                if hi - lo > 8.0 || things.hits(&b, 3.0) || taken.hits(&b, -1.5) {
                    continue;
                }
                // A field now and then is left as rough ground, so the patchwork is not unbroken.
                if rng.f() < 0.08 {
                    continue;
                }
                let roll = rng.f();
                let kind = if mid.0 < west {
                    if roll < 0.55 { "pasture" } else if roll < 0.78 { "hay" } else if roll < 0.9 { "oats" } else { "potato" }
                } else if roll < 0.24 {
                    "wheat"
                } else if roll < 0.42 {
                    "barley"
                } else if roll < 0.7 {
                    "pasture"
                } else if roll < 0.85 {
                    "hay"
                } else {
                    "potato"
                };
                let furrow = if rng.f() < 0.75 { a } else { a + std::f32::consts::FRAC_PI_2 };
                mine.push(b);
                farms.fields.push(Field { q, kind, furrow });
            }
        }
        for b in mine {
            taken.add(b);
        }
    }

    // Hedgerows: bushes along every field edge (each edge once), a tree now and then, a gap for a gate.
    let mut done: HashMap<((i32, i32), (i32, i32)), ()> = HashMap::new();
    let key = |p: (f32, f32)| ((p.0 * 2.0).round() as i32, (p.1 * 2.0).round() as i32);
    for f in &farms.fields {
        let gate = rng.v() % 4;
        let thin = f.q[0].0 < west; // the west's hedges are sparser (more stone and open ground)
        for k in 0..4 {
            let (a, b) = (f.q[k], f.q[(k + 1) % 4]);
            let e = if key(a) < key(b) { (key(a), key(b)) } else { (key(b), key(a)) };
            if done.insert(e, ()).is_some() {
                continue;
            }
            let len = (b.0 - a.0).hypot(b.1 - a.1);
            let gap_at = if k as u32 == gate { rng.range(0.25, 0.75) * len } else { -100.0 };
            let mut t = rng.range(1.0, 3.0);
            while t < len - 1.0 {
                if (t - gap_at).abs() > 3.0 {
                    let (x, y) = (a.0 + (b.0 - a.0) * t / len, a.1 + (b.1 - a.1) * t / len);
                    if !g.roads.near(x, y, 2.0) && g.dry(l, x, y) {
                        if rng.f() < 0.045 {
                            farms.hedges.push(("tree", x, y, rng.range(1.2, 1.7)));
                        } else if !thin || rng.f() < 0.45 {
                            farms.hedges.push(("bush", x + rng.range(-0.6, 0.6), y + rng.range(-0.6, 0.6), rng.range(1.8, 2.8)));
                        }
                    }
                }
                t += rng.range(3.5, 5.5);
            }
        }
    }
    farms
}

/// Fields by 64 unit bucket, so the scenery can keep its woods out of them.
pub struct FieldIndex<'a> {
    fields: &'a [Field],
    map: HashMap<(i32, i32), Vec<usize>>,
}

impl<'a> FieldIndex<'a> {
    pub fn new(fields: &'a [Field]) -> FieldIndex<'a> {
        let mut map: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (k, f) in fields.iter().enumerate() {
            let b = bbox(&f.q);
            let (i0, j0) = bucket(b[0], b[1]);
            let (i1, j1) = bucket(b[2], b[3]);
            for j in j0..=j1 {
                for i in i0..=i1 {
                    map.entry((i, j)).or_default().push(k);
                }
            }
        }
        FieldIndex { fields, map }
    }
    pub fn at(&self, x: f32, y: f32) -> bool {
        self.map.get(&bucket(x, y)).is_some_and(|v| v.iter().any(|&k| inside(&self.fields[k].q, x, y)))
    }
}

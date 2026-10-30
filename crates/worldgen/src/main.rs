//! worldgen: makes a world file for the mmo-engine server (`server --world FILE`).
//!
//! The file is data, not code: a heightfield with the rivers, lake beds and mountains already cut into it,
//! the water (river courses and lake surfaces), and a list of typed objects. Objects with a footprint
//! (`b`) that are `solid` become the server's walls; everything else is scenery. The server knows nothing
//! about what the objects are, and this tool knows nothing about how they look: a client theme maps each
//! type to models. A different world is a different run of this tool (or a different tool writing the same
//! format); a different look is a different theme. See WORLD.md for the format.
//!
//! Usage: worldgen [--recipe FILE] [--size 2048] [--seed 1] [--out world.json] [--no-city]
//!
//! A recipe (JSON) holds everything a world is made from: the seed and size, how much of each thing to lay out,
//! and hand edits on top of what the seed lays out (see `Params::from_recipe`). The recipe is written into the
//! world file, so a world can be opened, edited and made again. Flags given after `--recipe` override it.

mod coast;
mod fields;
mod roads;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::rc::Rc;

// ------------------------------------------------------------------------------------------------ random and noise

/// SplitMix64: small, fast, and the same everywhere.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    fn v(&mut self) -> u32 {
        self.next() as u32
    }
}

fn hash(x: i64, y: i64, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ seed.wrapping_mul(0x1656_67B1_9E37_79F9);
    h = (h ^ (h >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((h ^ (h >> 32)) >> 40) as f32 / (1u64 << 24) as f32
}

fn noise(x: f32, y: f32, seed: u64) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ux, uy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (i, j) = (ix as i64, iy as i64);
    let (a, b, c, d) = (hash(i, j, seed), hash(i + 1, j, seed), hash(i, j + 1, seed), hash(i + 1, j + 1, seed));
    a + (b - a) * ux + (c - a) * uy + (a - b - c + d) * ux * uy
}

fn fbm(x: f32, y: f32, seed: u64, octaves: u32) -> f32 {
    let (mut v, mut amp, mut x, mut y) = (0.0, 0.5, x, y);
    for k in 0..octaves {
        v += amp * noise(x, y, seed + k as u64 * 101);
        x *= 2.03;
        y *= 2.03;
        amp *= 0.5;
    }
    v
}

fn smooth(a: f32, b: f32, t: f32) -> f32 {
    let k = ((t - a) / (b - a)).clamp(0.0, 1.0);
    k * k * (3.0 - 2.0 * k)
}

// ------------------------------------------------------------------------------------------------ layout

type Box4 = [f32; 4];

#[derive(Clone)]
struct Solid {
    t: &'static str,
    b: Box4,
    v: u32,
    /// Ground height the thing stands on (filled in once the hills exist).
    base: f32,
    solid: bool,
}

fn overlaps(a: &Box4, b: &Box4, pad: f32) -> bool {
    a[0] < b[2] + pad && a[2] + pad > b[0] && a[1] < b[3] + pad && a[3] + pad > b[1]
}

struct Layout {
    size: f32,
    things: Vec<Solid>,
    /// The coastline, if the land has one: nothing is laid out at sea or on the beach.
    coast: Option<Rc<coast::Coast>>,
    /// River courses given by the recipe: nothing is laid out across them.
    courses: Vec<Vec<(f32, f32)>>,
}

impl Layout {
    fn free(&self, b: &Box4, pad: f32) -> bool {
        self.clear(b, pad) && self.dry(b)
    }
    /// Inside the world and clear of everything laid out so far (wet or not).
    fn clear(&self, b: &Box4, pad: f32) -> bool {
        b[0] >= 8.0 && b[1] >= 8.0 && b[2] <= self.size - 8.0 && b[3] <= self.size - 8.0 && !self.things.iter().any(|s| overlaps(&s.b, b, pad))
    }
    /// True if a box is well inland and clear of the recipe's rivers.
    fn dry(&self, b: &Box4) -> bool {
        let corners = [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3]), ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0)];
        if let Some(c) = &self.coast {
            if corners.iter().any(|&(x, y)| c.d(x, y) < 12.0) {
                return false;
            }
        }
        let keep = RIVER_HALF + RIVER_LIP + 8.0;
        !self.courses.iter().any(|r| {
            r.windows(2).any(|w| {
                let ((ax, ay), (bx, by)) = (w[0], w[1]);
                // Distance from the segment to the box, closely enough: the box grown by the keep-out, against the segment.
                let n = ((bx - ax).hypot(by - ay) / 4.0).ceil().max(1.0) as i32;
                (0..=n).any(|k| {
                    let t = k as f32 / n as f32;
                    let (x, y) = (ax + (bx - ax) * t, ay + (by - ay) * t);
                    x > b[0] - keep && x < b[2] + keep && y > b[1] - keep && y < b[3] + keep
                })
            })
        })
    }
    fn add(&mut self, t: &'static str, b: Box4, v: u32, solid: bool) {
        self.things.push(Solid { t, b, v, base: 0.0, solid });
    }
    /// Place a random box of the given type somewhere free; false if no room after many tries.
    fn scatter(&mut self, rng: &mut Rng, t: &'static str, w: (f32, f32), d: (f32, f32), pad: f32) -> bool {
        for _ in 0..200 {
            let (bw, bd) = (rng.range(w.0, w.1), rng.range(d.0, d.1));
            let x = rng.range(10.0, self.size - bw - 10.0);
            let y = rng.range(10.0, self.size - bd - 10.0);
            let b = [x, y, x + bw, y + bd];
            if self.free(&b, pad) {
                let v = rng.v();
                self.add(t, b, v, true);
                return true;
            }
        }
        false
    }
}

/// A walled city: curtain wall with towers and four gates, avenues to a central plaza and castle with corner
/// towers, side streets, terraced houses back to back along every street, and an open market square.
/// Returns the plaza's size and the city's streets (each a straight line), which become paved roads.
fn city(l: &mut Layout, rng: &mut Rng, cx: f32, cy: f32, side: f32) -> (f32, Vec<[(f32, f32); 2]>) {
    const WALL: f32 = 3.0;
    const GATE: f32 = 18.0;
    const TOWER: f32 = 9.0;
    const ROW: f32 = 12.0;
    const STREET: f32 = 10.0;
    const AVENUE: f32 = 16.0;
    const BLOCK: f32 = 46.0;
    let h = side / 2.0;
    let (x0, y0, x1, y1) = (cx - h, cy - h, cx + h, cy + h);
    l.add("city", [x0 - 6.0, y0 - 6.0, x1 + 6.0, y1 + 6.0], rng.v(), false);
    let gx = GATE / 2.0;
    for (ax, ay, bx, by, horiz) in [(x0, y0, x1, y0, true), (x0, y1, x1, y1, true), (x0, y0, x0, y1, false), (x1, y0, x1, y1, false)] {
        let v = rng.v();
        if horiz {
            let m = (ax + bx) / 2.0;
            l.add("wall", [ax, ay - WALL / 2.0, m - gx, ay + WALL / 2.0], v, true);
            l.add("wall", [m + gx, ay - WALL / 2.0, bx, ay + WALL / 2.0], v ^ 1, true);
        } else {
            let m = (ay + by) / 2.0;
            l.add("wall", [ax - WALL / 2.0, ay, ax + WALL / 2.0, m - gx], v, true);
            l.add("wall", [ax - WALL / 2.0, m + gx, ax + WALL / 2.0, by], v ^ 1, true);
        }
    }
    let t = TOWER / 2.0;
    let mut spots = vec![(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
    for s in [-1.0, 1.0] {
        spots.extend([(cx + s * (gx + t), y0), (cx + s * (gx + t), y1), (x0, cy + s * (gx + t)), (x1, cy + s * (gx + t))]);
    }
    let n = ((h - GATE) / 70.0).max(1.0) as i32;
    for k in 1..=n {
        let f = k as f32 / (n + 1) as f32;
        for s in [-1.0, 1.0] {
            let o = s * (gx + f * (h - gx));
            spots.extend([(cx + o, y0), (cx + o, y1), (x0, cy + o), (x1, cy + o)]);
        }
    }
    for (sx, sy) in spots {
        let v = rng.v();
        l.add("tower", [sx - t, sy - t, sx + t, sy + t], v, true);
    }
    let plaza = (side * 0.24).max(70.0);
    let v = rng.v();
    l.add("castle", [cx - 18.0, cy - 14.0, cx + 18.0, cy + 14.0], v, true);
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            let v = rng.v();
            l.add("tower", [cx + sx * 22.0 - t, cy + sy * 18.0 - t, cx + sx * 22.0 + t, cy + sy * 18.0 + t], v, true);
        }
    }
    l.add("square", [cx - plaza / 2.0, cy - plaza / 2.0, cx + plaza / 2.0, cy + plaza / 2.0], rng.v(), false);
    let inner = h - WALL / 2.0 - 12.0;
    let mut market = false;
    // A street runs round inside the wall, and between every row and column of blocks.
    let ring = h - WALL / 2.0 - 6.0;
    let mut streets = vec![
        [(cx - ring, cy - ring), (cx + ring, cy - ring)],
        [(cx + ring, cy - ring), (cx + ring, cy + ring)],
        [(cx + ring, cy + ring), (cx - ring, cy + ring)],
        [(cx - ring, cy + ring), (cx - ring, cy - ring)],
    ];
    for qx in [-1.0f32, 1.0] {
        for qy in [-1.0f32, 1.0] {
            let (lo_x, hi_x) = if qx < 0.0 { (cx - inner, cx - AVENUE / 2.0) } else { (cx + AVENUE / 2.0, cx + inner) };
            let (lo_y, hi_y) = if qy < 0.0 { (cy - inner, cy - AVENUE / 2.0) } else { (cy + AVENUE / 2.0, cy + inner) };
            let mut x = lo_x + BLOCK + STREET / 2.0;
            while x + STREET / 2.0 + BLOCK <= hi_x {
                streets.push([(x, lo_y - 6.0), (x, hi_y)]);
                x += BLOCK + STREET;
            }
            let mut y = lo_y + 2.0 * ROW + 1.0 + STREET / 2.0;
            while y + STREET / 2.0 + 2.0 * ROW + 1.0 <= hi_y {
                streets.push([(lo_x - 6.0, y), (hi_x, y)]);
                y += 2.0 * ROW + 1.0 + STREET;
            }
            let mut y = lo_y;
            while y + 2.0 * ROW + 1.0 <= hi_y {
                let mut x = lo_x;
                while x + BLOCK <= hi_x {
                    let blk = [x, y, x + BLOCK, y + 2.0 * ROW + 1.0];
                    let near_plaza = ((blk[0] + blk[2]) / 2.0 - cx).abs() < plaza / 2.0 + BLOCK / 2.0 && ((blk[1] + blk[3]) / 2.0 - cy).abs() < plaza / 2.0 + ROW;
                    if !near_plaza {
                        if !market && qx > 0.0 && qy > 0.0 {
                            market = true;
                            l.add("square", blk, rng.v(), false); // the market square
                        } else {
                            // Each row is a terrace: cut along its length into houses that share walls.
                            for (ry0, ry1) in [(blk[1], blk[1] + ROW), (blk[1] + ROW + 1.0, blk[3])] {
                                let parts = (BLOCK / 16.0).ceil() as i32;
                                for k in 0..parts {
                                    let a = blk[0] + BLOCK * k as f32 / parts as f32;
                                    let b = blk[0] + BLOCK * (k + 1) as f32 / parts as f32;
                                    let v = rng.v();
                                    l.add("house", [a, ry0, b, ry1], v, true);
                                }
                            }
                        }
                    }
                    x += BLOCK + STREET;
                }
                y += 2.0 * ROW + 1.0 + STREET;
            }
        }
    }
    (plaza, streets)
}

/// A village along a high street: houses facing the street on both sides with gaps between, a green where the
/// street widens in the middle, a church tower by the green, and a lane of cottages behind. Returns the street's two
/// ends, where the roads come in.
fn village(l: &mut Layout, rng: &mut Rng) -> Option<[(f32, f32); 2]> {
    for _ in 0..60 {
        let (cx, cy) = (rng.range(140.0, l.size - 140.0), rng.range(140.0, l.size - 140.0));
        let half = rng.range(45.0, 80.0);
        if let Some(ends) = village_at(l, rng, cx, cy, half, false) {
            return Some(ends);
        }
    }
    None
}

/// A village (or, `town`, a market town: a longer street and a manor by the green) with its green at (cx, cy), if
/// there is room; the street ends, as `village` returns them.
fn village_at(l: &mut Layout, rng: &mut Rng, cx: f32, cy: f32, half: f32, town: bool) -> Option<[(f32, f32); 2]> {
    const STREET: f32 = 5.0;
    {
        // Room for it among what is already laid out; each house then finds its own dry ground, so a village can
        // come down to the shore or a river bank.
        let area = [cx - half - 20.0, cy - half - 20.0, cx + half + 20.0, cy + half + 20.0];
        let green = [cx - 20.0, cy - 20.0, cx + 20.0, cy + 20.0];
        if !l.clear(&area, 30.0) || !l.dry(&green) {
            return None;
        }
        // Lay it out as if the street ran east to west, then turn it a quarter for a north-south one.
        let across = rng.f() < 0.5;
        let put = |l: &mut Layout, t: &'static str, x0: f32, y0: f32, x1: f32, y1: f32, v: u32, solid: bool| {
            let (a, b, c, d) = if across { (cx + y0, cy + x0, cx + y1, cy + x1) } else { (cx + x0, cy + y0, cx + x1, cy + y1) };
            l.add(t, [a.min(c), b.min(d), a.max(c), b.max(d)], v, solid);
        };
        let fits = |l: &Layout, x0: f32, y0: f32, x1: f32, y1: f32, pad: f32| {
            let (a, b, c, d) = if across { (cx + y0, cy + x0, cx + y1, cy + x1) } else { (cx + x0, cy + y0, cx + x1, cy + y1) };
            l.free(&[a.min(c), b.min(d), a.max(c), b.max(d)], pad)
        };
        let green = rng.range(12.0, 16.0);
        put(l, "square", -green, -green, green, green, rng.v(), false);
        // A church tower on the green's north side, set back from the street.
        if rng.f() < 0.7 {
            let x = rng.range(-green + 6.0, green - 6.0);
            if fits(l, x - 4.5, -green - 14.0, x + 4.5, -green - 5.0, 3.0) {
                put(l, "tower", x - 4.5, -green - 14.0, x + 4.5, -green - 5.0, rng.v(), true);
            }
        }
        for side in [-1.0f32, 1.0] {
            // Frontages: houses shoulder to shoulder with now and then a gap, a few set back.
            let mut x = -half;
            while x < half {
                let w = rng.range(9.0, 15.0);
                let d = rng.range(8.0, 12.0);
                let back = if rng.f() < 0.25 { rng.range(2.0, 5.0) } else { 0.0 };
                let (y0, y1) = if side < 0.0 { (-STREET - back - d, -STREET - back) } else { (STREET + back, STREET + back + d) };
                let on_green = x + w > -green - 2.0 && x < green + 2.0;
                if !on_green && rng.f() < 0.85 && fits(l, x, y0, x + w, y1, 1.0) {
                    put(l, "house", x, y0, x + w, y1, rng.v(), true);
                }
                x += w + if rng.f() < 0.3 { rng.range(6.0, 12.0) } else { rng.range(1.5, 3.0) };
            }
            // A back lane of cottages, sparser.
            let mut x = -half * 0.8;
            while x < half * 0.8 {
                let w = rng.range(8.0, 12.0);
                let d = rng.range(7.0, 10.0);
                let off = STREET + 12.0 + 14.0;
                let (y0, y1) = if side < 0.0 { (-off - d, -off) } else { (off, off + d) };
                if rng.f() < 0.5 && fits(l, x, y0, x + w, y1, 4.0) {
                    put(l, "house", x, y0, x + w, y1, rng.v(), true);
                }
                x += w + rng.range(8.0, 18.0);
            }
        }
        // A town's manor house stands behind the green on the south side.
        if town {
            let (w, d) = (rng.range(20.0, 28.0), rng.range(16.0, 22.0));
            if fits(l, -w / 2.0, green + 6.0, w / 2.0, green + 6.0 + d, 3.0) {
                put(l, "keep", -w / 2.0, green + 6.0, w / 2.0, green + 6.0 + d, rng.v(), true);
            }
        }
        let end = half + 12.0;
        Some(if across { [(cx, cy - end), (cx, cy + end)] } else { [(cx - end, cy), (cx + end, cy)] })
    }
}

/// Lay out a settlement near a point the recipe gives, trying a little way round it if the spot itself is taken.
/// Where it went: the green's middle (or the fort's), and for a town or village its street's ends.
fn settle(l: &mut Layout, rng: &mut Rng, s: &Settlement) -> Option<((f32, f32), Option<[(f32, f32); 2]>)> {
    // Spiral out from the spot, a step at a time; a town or village that does not fit shrinks before it moves far.
    for k in 0..90 {
        let (r, a) = (k as f32 * 4.0, k as f32 * 2.39996);
        let (x, y) = (s.at.0 + r * a.cos(), s.at.1 + r * a.sin());
        let shrink = 1.0 - (k as f32 / 90.0) * 0.45;
        match s.kind {
            "town" | "village" => {
                let town = s.kind == "town";
                let half = if town { rng.range(90.0, 120.0) } else { rng.range(45.0, 75.0) } * shrink;
                if let Some(e) = village_at(l, rng, x, y, half, town) {
                    return Some(((x, y), Some(e)));
                }
            }
            _ => {
                let (w, d) = (rng.range(22.0, 34.0), rng.range(18.0, 28.0));
                let b = [x - w / 2.0, y - d / 2.0, x + w / 2.0, y + d / 2.0];
                if l.free(&b, 20.0) {
                    let v = rng.v();
                    l.add("keep", b, v, true);
                    return Some(((x, y), None));
                }
            }
        }
    }
    None
}

// ------------------------------------------------------------------------------------------------ land

const RIVER_HALF: f32 = 5.0;
const RIVER_LIP: f32 = 1.5;
const RIVER_DEPTH: f32 = 1.6;
const FLAT_PAD: f32 = 10.0;

struct Land {
    size: f32,
    seed: u64,
    /// How tall the hills are (1 = as designed, 0 = flat).
    hills: f32,
    coast: Option<Rc<coast::Coast>>,
}

/// The sea's surface, for land with a coast, and the height of the shore just above it.
const SEA_LEVEL: f32 = 0.0;
const SHORE: f32 = 1.2;
/// How far out from the shore the sea floor is shallow enough to wade.
const SHALLOWS: f32 = 10.0;

impl Land {
    /// Hills, rising into mountains past the playable edge; or, with a coast, hills that rise from a beach inland and a
    /// sea floor that falls away outside it.
    fn raw(&self, x: f32, y: f32) -> f32 {
        let mut h = ((fbm(x / 170.0, y / 170.0, self.seed + 31, 3) - 0.5) * 30.0 + (noise(x / 35.0, y / 35.0, self.seed + 7) - 0.5) * 1.8) * self.hills;
        if let Some(c) = &self.coast {
            let d = c.d(x, y);
            return if d >= 0.0 {
                SHORE + smooth(0.0, 140.0, d) * (h + 16.0 * self.hills).max(0.0) * 0.85
            } else {
                (SHORE + d * 0.45).max(-12.0 + 2.0 * noise(x / 40.0, y / 40.0, self.seed + 11))
            };
        }
        let out = (-x).max(x - self.size).max(-y).max(y - self.size);
        if out > 0.0 {
            h += smooth(0.0, 110.0, out) * (55.0 + 30.0 * (fbm(x / 60.0, y / 60.0, self.seed + 3, 4) * 2.0 - 1.0));
        }
        h
    }
}

struct River {
    pts: Vec<(f32, f32, f32)>, // x, y, level
}

/// River courses: from an edge across the land, meandering, pulled towards the far side, bending round
/// anything solid, and never running uphill (the level only falls, so a river cuts a gorge through a rise).
fn rivers(land: &Land, l: &Layout, rng: &mut Rng, count: Option<usize>) -> Vec<River> {
    let size = land.size;
    let count = count.unwrap_or((2 + (size / 2000.0) as usize).min(6)).min(12);
    let blocked = |x: f32, y: f32| l.things.iter().any(|s| s.solid && x > s.b[0] - 18.0 && x < s.b[2] + 18.0 && y > s.b[1] - 18.0 && y < s.b[3] + 18.0);
    let ground = |x: f32, y: f32, hd: f32| {
        let (sx, sy) = (-hd.sin() * (RIVER_HALF + RIVER_LIP + 1.0), hd.cos() * (RIVER_HALF + RIVER_LIP + 1.0));
        land.raw(x, y).min(land.raw(x + sx, y + sy)).min(land.raw(x - sx, y - sy))
    };
    let mut out = Vec::new();
    // Each river has its own random stream, so one bending round something new leaves the others where they were.
    let base = rng.next();
    for k in 0..count {
        let rng = &mut Rng(base ^ (k as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let side = (rng.f() * 4.0) as usize % 4;
        let along = (0.15 + 0.7 * rng.f()) * size;
        let (mut x, mut y) = [(along, 0.0), (size, along), (along, size), (0.0, along)][side];
        let aim = [std::f32::consts::FRAC_PI_2, std::f32::consts::PI, -std::f32::consts::FRAC_PI_2, 0.0][side];
        let mut hd = aim + rng.range(-0.4, 0.4);
        let mut pts = vec![(x, y, ground(x, y, hd))];
        for n in 0..4000 {
            hd += rng.range(-0.175, 0.175);
            let mut off = (aim - hd + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            if off < -std::f32::consts::PI {
                off += std::f32::consts::TAU;
            }
            hd += off * 0.06;
            let mut nx = x + hd.cos() * 10.0;
            let mut ny = y + hd.sin() * 10.0;
            if blocked(nx, ny) {
                let course = hd;
                for t in 0..8 {
                    hd = course + 0.4 * (t / 2 + 1) as f32 * if t % 2 == 0 { 1.0 } else { -1.0 };
                    nx = x + hd.cos() * 10.0;
                    ny = y + hd.sin() * 10.0;
                    if !blocked(nx, ny) {
                        break;
                    }
                }
            }
            let outside = nx < 0.0 || ny < 0.0 || nx > size || ny > size;
            if outside && n < 30 {
                hd = aim;
                continue;
            }
            let lvl = pts.last().unwrap().2.min(ground(nx, ny, hd));
            pts.push((nx, ny, lvl));
            if outside {
                break;
            }
            x = nx;
            y = ny;
        }
        out.push(River { pts });
    }
    out
}

/// Rivers along the recipe's courses: each line followed in 10 unit steps with a gentle meander of its own, the
/// level falling (never rising) from source to mouth as the ground does.
fn courses(land: &Land, list: &[Vec<(f32, f32)>], seed: u64) -> Vec<River> {
    let mut out = Vec::new();
    for (k, c) in list.iter().enumerate() {
        let mut pts: Vec<(f32, f32, f32)> = Vec::new();
        let mut along = 0.0f32;
        for w in c.windows(2) {
            let ((ax, ay), (bx, by)) = (w[0], w[1]);
            let len = (bx - ax).hypot(by - ay);
            if len < 1e-3 {
                continue;
            }
            let (nx, ny) = (-(by - ay) / len, (bx - ax) / len);
            let n = (len / 10.0).ceil() as i32;
            for i in 0..n {
                let t = i as f32 / n as f32;
                // The meander fades out at the course's own points, so the river still passes through each of them.
                let wiggle = (fbm(along / 90.0, k as f32 * 7.3, seed + 61, 2) - 0.5) * 2.0 * (len * 0.12).min(14.0) * (std::f32::consts::PI * t).sin();
                let (x, y) = (ax + (bx - ax) * t + nx * wiggle, ay + (by - ay) * t + ny * wiggle);
                let ground = land.raw(x, y);
                let lvl = pts.last().map_or(ground, |p| p.2.min(ground));
                pts.push((x, y, lvl));
                along += len / n as f32;
            }
        }
        if let Some(&(x, y)) = c.last() {
            let lvl = pts.last().map_or(land.raw(x, y), |p| p.2.min(land.raw(x, y)));
            pts.push((x, y, lvl));
        }
        if pts.len() >= 2 {
            out.push(River { pts });
        }
    }
    out
}

/// Spatial index of river segments by 64 unit cell.
struct RiverIndex {
    cells: HashMap<(i32, i32), Vec<(usize, usize)>>,
}

impl RiverIndex {
    fn new(rivers: &[River]) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<(usize, usize)>> = HashMap::new();
        let pad = RIVER_HALF + RIVER_LIP + 40.0;
        for (ri, r) in rivers.iter().enumerate() {
            for k in 0..r.pts.len().saturating_sub(1) {
                let (a, b) = (r.pts[k], r.pts[k + 1]);
                for cx in ((a.0.min(b.0) - pad) / 64.0).floor() as i32..=((a.0.max(b.0) + pad) / 64.0).floor() as i32 {
                    for cy in ((a.1.min(b.1) - pad) / 64.0).floor() as i32..=((a.1.max(b.1) + pad) / 64.0).floor() as i32 {
                        cells.entry((cx, cy)).or_default().push((ri, k));
                    }
                }
            }
        }
        RiverIndex { cells }
    }
    /// Distance to the nearest river and its level there.
    fn at(&self, rivers: &[River], x: f32, y: f32) -> Option<(f32, f32)> {
        let mut best: Option<(f32, f32)> = None;
        for &(ri, k) in self.cells.get(&((x / 64.0).floor() as i32, (y / 64.0).floor() as i32))? {
            let (a, b) = (rivers[ri].pts[k], rivers[ri].pts[k + 1]);
            let (abx, aby) = (b.0 - a.0, b.1 - a.1);
            let t = (((x - a.0) * abx + (y - a.1) * aby) / (abx * abx + aby * aby).max(1e-4)).clamp(0.0, 1.0);
            let d = ((a.0 + abx * t - x).powi(2) + (a.1 + aby * t - y).powi(2)).sqrt();
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, a.2 + (b.2 - a.2) * t));
            }
        }
        best
    }
}

fn water_level(level: f32) -> f32 {
    level - RIVER_DEPTH + 1.0
}

/// The ground everywhere: hills, rivers cut into them (a flat bed, a short lip just above the water, then
/// valley sides, steep in hills), lake beds with ragged shores, crags raised into mountains, and the ground
/// levelled round buildings and across a city.
fn height(land: &Land, l: &Layout, near: &[usize], rivers: &[River], ri: &RiverIndex, road: &roads::Cells, x: f32, y: f32) -> f32 {
    let mut h = land.raw(x, y);
    if let Some((d, lvl)) = ri.at(rivers, x, y) {
        // Where a road crosses, a ford: the bed comes up to just under the water, so the road wades across.
        let ford = d <= RIVER_HALF + RIVER_LIP + 2.0 && road.near(x, y, 2.0);
        let bed = lvl - RIVER_DEPTH;
        let top = water_level(lvl) + 0.5;
        h = if d <= RIVER_HALF {
            h.min(bed + 0.3 * d / RIVER_HALF)
        } else if d <= RIVER_HALF + RIVER_LIP {
            h.min(bed + 0.3 + (top - bed - 0.3) * (d - RIVER_HALF) / RIVER_LIP)
        } else {
            let bank = 6.0f32.max((h - top) * 1.3);
            h.min(top + (h - top) * smooth(RIVER_HALF + RIVER_LIP, RIVER_HALF + RIVER_LIP + bank, d))
        };
        if ford {
            h = h.max(water_level(lvl) - 0.3);
        }
    }
    let (mut best, mut target) = (0.0f32, h);
    for &i in near {
        let s = &l.things[i];
        let b = &s.b;
        let dx = (b[0] - x).max(x - b[2]).max(0.0);
        let dy = (b[1] - y).max(y - b[3]).max(0.0);
        let inside = dx == 0.0 && dy == 0.0;
        match s.t {
            "lake" if inside => {
                let (cx, cy) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
                let (hx, hy) = ((b[2] - b[0]) / 2.0, (b[3] - b[1]) / 2.0);
                let r = 8.0f32.min(hx.min(hy) * 0.6);
                let (qx, qy) = ((x - cx).abs() - (hx - r), (y - cy).abs() - (hy - r));
                let ins = r - (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() - qx.max(qy).min(0.0);
                let shore = 1.5 + 3.0 * noise(x / 3.0, y / 3.0, land.seed + 77);
                let depth = ins - shore;
                return if depth <= 0.0 { s.base - 0.15 * (ins / shore).clamp(0.0, 1.0) } else { s.base - 0.6 - (depth * 0.9).min(6.0) };
            }
            "crag" if inside => {
                let ins = (x - b[0]).min(b[2] - x).min(y - b[1]).min(b[3] - y);
                let r = (b[2] - b[0]).min(b[3] - b[1]) / 2.0;
                let peak = ((b[2] - b[0]).min(b[3] - b[1]) * 0.7).clamp(8.0, 28.0);
                let rough = 0.75 + 0.5 * fbm(x / 6.0, y / 6.0, land.seed + 91, 3);
                return s.base + peak * (0.35 * smooth(0.0, 3.0, ins) + 0.65 * smooth(0.0, r, ins).powf(0.8)) * rough;
            }
            "square" | "dock" => continue,
            _ => {}
        }
        let pad = if s.t == "city" { 14.0 } else { FLAT_PAD };
        let w = 1.0 - smooth(0.0, pad, (dx * dx + dy * dy).sqrt());
        if w > best {
            best = w;
            target = s.base;
        }
    }
    h + (target - h) * best
}

// ------------------------------------------------------------------------------------------------ output

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Half the side of a scenery object's solid footprint at scale `s`, for the kinds that are solid: about a trunk
/// for a tree, most of the stone for a rock.
fn scenery_half(t: &str, s: f32) -> Option<f32> {
    match t {
        "tree" | "tree_bare" => Some(0.3 * s),
        "rock" => Some(0.35 * s),
        "boulder" | "spire" => Some(0.6 * s),
        _ => None,
    }
}

fn r1(v: f32) -> f64 {
    (v as f64 * 10.0).round() / 10.0
}
fn r2(v: f32) -> f64 {
    (v as f64 * 100.0).round() / 100.0
}

pub struct Params {
    pub size: f32,
    pub seed: u64,
    pub city: bool,
    /// How many of each thing to lay out, as a multiple of the usual amount for the size (1 = usual, 0 = none).
    pub villages: f32,
    pub forts: f32,
    pub crags: f32,
    pub lakes: f32,
    /// River count; None for the usual number for the size.
    pub rivers: Option<usize>,
    /// How wooded the land is (1 = usual, 0 = open meadow) and how tall the hills are (1 = usual, 0 = flat).
    pub forest: f32,
    pub hills: f32,
    /// Roads between the places, high streets and avenues.
    pub roads: bool,
    /// The land's outlines, if it has a coast (sea outside them); none is land to the edge, with mountains past it.
    pub coast: Vec<Vec<(f32, f32)>>,
    /// Rivers along these courses, source to mouth, instead of ones the seed lays out.
    pub courses: Vec<Vec<(f32, f32)>>,
    /// Places the recipe puts where it wants them, before the seed lays out the rest.
    pub settlements: Vec<Settlement>,
    /// Hand edits on top of what the seed lays out, applied in order.
    pub edits: Vec<Edit>,
}

/// A city, town, village or fort at a chosen point (`side`: a city's width).
#[derive(Clone, Debug, PartialEq)]
pub struct Settlement {
    pub kind: &'static str,
    pub at: (f32, f32),
    pub side: Option<f32>,
    /// What it is called, carried through to the world file's `settled` list.
    pub name: Option<String>,
}

const SETTLEMENT_KINDS: [&str; 4] = ["city", "town", "village", "fort"];

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    /// Take away what is at this point: the smallest footprint containing it (a whole city, with everything in
    /// it, if the point is on a city's open ground).
    Remove(f32, f32),
    /// Put a footprint here, clearing whatever it overlaps.
    Add(&'static str, Box4),
}

/// Footprint types an edit may add, and whether each is solid.
const ADDABLE: [(&str, bool); 9] =
    [("house", true), ("keep", true), ("tower", true), ("castle", true), ("wall", true), ("crag", true), ("lake", true), ("square", false), ("city", false)];

impl Default for Params {
    fn default() -> Self {
        Params { size: 2048.0, seed: 1, city: true, villages: 1.0, forts: 1.0, crags: 1.0, lakes: 1.0, rivers: None, forest: 1.0, hills: 1.0, roads: true, coast: Vec::new(), courses: Vec::new(), settlements: Vec::new(), edits: Vec::new() }
    }
}

impl Params {
    /// Read a recipe: `{"size", "seed", "city", "villages", "forts", "crags", "lakes", "rivers", "forest", "hills",
    /// "edits": [{"remove": [x, y]} | {"add": {"t": type, "b": [x0, y0, x1, y1]}}]}`. Anything left out is usual.
    pub fn from_recipe(v: &Value) -> Result<Params, String> {
        let mut p = Params::default();
        let num = |k: &str, lo: f64, hi: f64| -> Result<Option<f64>, String> {
            match v.get(k) {
                None | Some(Value::Null) => Ok(None),
                Some(n) => n.as_f64().filter(|n| (lo..=hi).contains(n)).map(Some).ok_or(format!("{k} must be a number from {lo} to {hi}")),
            }
        };
        if let Some(n) = num("size", 64.0, 65536.0)? {
            p.size = n as f32;
        }
        if let Some(n) = num("seed", 0.0, 9.0e15)? {
            p.seed = n as u64;
        }
        if let Some(b) = v.get("city") {
            p.city = b.as_bool().ok_or("city must be true or false")?;
        }
        if let Some(b) = v.get("roads") {
            p.roads = b.as_bool().ok_or("roads must be true or false")?;
        }
        for (k, f) in [("villages", &mut p.villages), ("forts", &mut p.forts), ("crags", &mut p.crags), ("lakes", &mut p.lakes), ("forest", &mut p.forest), ("hills", &mut p.hills)] {
            if let Some(n) = num(k, 0.0, 5.0)? {
                *f = n as f32;
            }
        }
        p.rivers = num("rivers", 0.0, 12.0)?.map(|n| n as usize);
        let line = |v: &Value, what: String, min: usize| -> Result<Vec<(f32, f32)>, String> {
            let pts: Option<Vec<(f32, f32)>> = v.as_array().map(|a| a.iter().map(|q| Some((q[0].as_f64()? as f32, q[1].as_f64()? as f32))).collect()).flatten();
            pts.filter(|p| p.len() >= min && p.len() <= 5000 && p.iter().all(|q| q.0.is_finite() && q.1.is_finite())).ok_or(format!("{what}: a list of at least {min} [x, y] points"))
        };
        for (i, o) in v.get("coast").and_then(|c| c.as_array()).map_or(&[][..], |a| a.as_slice()).iter().enumerate() {
            p.coast.push(line(o, format!("coast {i}"), 3)?);
        }
        for (i, r) in v.get("river_courses").and_then(|c| c.as_array()).map_or(&[][..], |a| a.as_slice()).iter().enumerate() {
            p.courses.push(line(r, format!("river course {i}"), 2)?);
        }
        for (i, s) in v.get("settlements").and_then(|c| c.as_array()).map_or(&[][..], |a| a.as_slice()).iter().enumerate() {
            let t = s.get("t").and_then(|t| t.as_str()).unwrap_or("");
            let kind = SETTLEMENT_KINDS.iter().find(|k| **k == t).copied().ok_or(format!("settlement {i}: t is one of city, town, village, fort"))?;
            let at = line(s.get("at").map(|a| Value::Array(vec![a.clone()])).as_ref().unwrap_or(&Value::Null), format!("settlement {i}: at"), 1)?[0];
            let side = s.get("side").and_then(|v| v.as_f64()).map(|v| (v as f32).clamp(160.0, 800.0));
            let name = s.get("name").and_then(|n| n.as_str()).map(|n| n.chars().take(60).collect());
            p.settlements.push(Settlement { kind, at, side, name });
        }
        for (i, e) in v.get("edits").and_then(|e| e.as_array()).map_or(&[][..], |a| a.as_slice()).iter().enumerate() {
            let pt = |a: &Value, n: usize| -> Option<Vec<f32>> {
                let a = a.as_array()?;
                let v: Vec<f32> = a.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                (v.len() == n && a.len() == n).then_some(v)
            };
            if let Some(r) = e.get("remove") {
                let q = pt(r, 2).ok_or(format!("edit {i}: remove needs [x, y]"))?;
                p.edits.push(Edit::Remove(q[0], q[1]));
            } else if let Some(a) = e.get("add") {
                let t = a.get("t").and_then(|t| t.as_str()).unwrap_or("");
                let t = ADDABLE.iter().find(|(n, _)| *n == t).map(|(n, _)| *n).ok_or(format!("edit {i}: cannot add a '{t}'"))?;
                let b = a.get("b").and_then(|b| pt(b, 4)).ok_or(format!("edit {i}: add needs b = [x0, y0, x1, y1]"))?;
                let b = [b[0].min(b[2]), b[1].min(b[3]), b[0].max(b[2]), b[1].max(b[3])];
                if b[2] - b[0] < 1.0 || b[3] - b[1] < 1.0 {
                    return Err(format!("edit {i}: footprint too small"));
                }
                p.edits.push(Edit::Add(t, b));
            } else {
                return Err(format!("edit {i}: expected remove or add"));
            }
        }
        Ok(p)
    }

    /// The recipe as written into the world file.
    pub fn recipe(&self) -> Value {
        let edits: Vec<Value> = self
            .edits
            .iter()
            .map(|e| match e {
                Edit::Remove(x, y) => json!({ "remove": [r1(*x), r1(*y)] }),
                Edit::Add(t, b) => json!({ "add": { "t": t, "b": [r1(b[0]), r1(b[1]), r1(b[2]), r1(b[3])] } }),
            })
            .collect();
        json!({
            "size": self.size, "seed": self.seed, "city": self.city, "villages": self.villages, "forts": self.forts, "crags": self.crags,
            "lakes": self.lakes, "rivers": self.rivers, "forest": self.forest, "hills": self.hills, "roads": self.roads, "edits": edits,
            "coast": self.coast.iter().map(|o| o.iter().map(|&(x, y)| json!([r1(x), r1(y)])).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "river_courses": self.courses.iter().map(|o| o.iter().map(|&(x, y)| json!([r1(x), r1(y)])).collect::<Vec<_>>()).collect::<Vec<_>>(),
            "settlements": self.settlements.iter().map(|s| {
                let mut o = json!({ "t": s.kind, "at": [r1(s.at.0), r1(s.at.1)] });
                if let Some(side) = s.side { o["side"] = json!(side); }
                if let Some(n) = &s.name { o["name"] = json!(n); }
                o
            }).collect::<Vec<_>>(),
        })
    }
}

impl Layout {
    fn apply(&mut self, e: &Edit, rng: &mut Rng) {
        match *e {
            Edit::Remove(x, y) => {
                let inside = |b: &Box4| x >= b[0] && x <= b[2] && y >= b[1] && y <= b[3];
                let area = |b: &Box4| (b[2] - b[0]) * (b[3] - b[1]);
                let Some(i) = (0..self.things.len()).filter(|&i| inside(&self.things[i].b)).min_by(|&a, &b| area(&self.things[a].b).total_cmp(&area(&self.things[b].b))) else {
                    return;
                };
                let gone = self.things.swap_remove(i);
                if gone.t == "city" {
                    self.things.retain(|s| !overlaps(&gone.b, &s.b, -0.01));
                }
            }
            Edit::Add(t, b) => {
                let solid = ADDABLE.iter().any(|(n, s)| *n == t && *s);
                let b = [b[0].clamp(0.0, self.size), b[1].clamp(0.0, self.size), b[2].clamp(0.0, self.size), b[3].clamp(0.0, self.size)];
                // A new open area keeps what stands in it; anything else clears the ground it covers.
                if solid {
                    self.things.retain(|s| !s.solid || !overlaps(&s.b, &b, -0.01));
                    self.things.retain(|s| s.solid || s.t == "city" || !overlaps(&s.b, &b, -0.01));
                }
                let v = rng.v();
                self.add(t, b, v, solid);
            }
        }
    }
}

/// The whole world as the JSON value written to the file.
/// A generated world, ready to write: everything but the objects as a value, and each object as its own JSON text
/// (a few dozen bytes, where a JSON value is ten times that), so a 65,536 unit world of six million objects fits in
/// the memory of a small server.
pub struct Generated {
    head: Value,
    objects: Vec<String>,
    pub solid: usize,
}

impl Generated {
    pub fn objects(&self) -> usize {
        self.objects.len()
    }

    /// Write the world file, exactly as `serde_json` would write `to_value()`; returns the bytes written.
    pub fn write(&self, out: &mut impl std::io::Write) -> std::io::Result<usize> {
        // Keys come out sorted, so the objects go where the empty list is in the head.
        let head = self.head.to_string();
        let at = head.find(r#""objects":[]"#).expect("the head has an empty object list");
        let mut n = 0;
        let mut put = |b: &[u8]| -> std::io::Result<()> {
            n += b.len();
            out.write_all(b)
        };
        put(head[..at].as_bytes())?;
        put(br#""objects":["#)?;
        for (i, o) in self.objects.iter().enumerate() {
            if i > 0 {
                put(b",")?;
            }
            put(o.as_bytes())?;
        }
        put(b"]")?;
        put(head[at + r#""objects":[]"#.len()..].as_bytes())?;
        Ok(n)
    }

    pub fn to_value(&self) -> Value {
        let mut v = self.head.clone();
        v["objects"] = Value::Array(self.objects.iter().map(|o| serde_json::from_str(o).expect("written by us")).collect());
        v
    }
}

pub fn generate(p: &Params) -> Value {
    build(p).to_value()
}

pub fn build(p: &Params) -> Generated {
    let size = p.size;
    let mut rng = Rng(p.seed ^ 0x5EED_0F_3A7);
    let coast = (!p.coast.is_empty()).then(|| Rc::new(coast::Coast::new(size, p.coast.clone())));
    let land = Land { size, seed: p.seed, hills: p.hills, coast: coast.clone() };
    let mut l = Layout { size, things: Vec::new(), coast: coast.clone(), courses: p.courses.clone() };
    let mut city_at = None;
    let given = p.settlements.iter().find(|s| s.kind == "city");
    if (p.city || given.is_some()) && size >= 1024.0 {
        let mut side = (size / 6.0).clamp(240.0, 600.0);
        let (mut cx, mut cy) = (rng.range(0.3, 0.7) * size, rng.range(0.3, 0.7) * size);
        if let Some(s) = given {
            (cx, cy) = s.at;
            side = s.side.unwrap_or(side);
        }
        let (plaza, streets) = city(&mut l, &mut rng, cx, cy, side);
        city_at = Some((cx, cy, side / 2.0, plaza, streets));
    }
    let mut streets = Vec::new();
    // The recipe's own places go in first, where it wants them; then the seed lays out the rest round them.
    let mut srng = Rng(p.seed ^ 0x5E77);
    let mut settled: Vec<Value> = Vec::new();
    // How much of a place each high street is (`roads::Place`): towns 1, named villages 2, the rest 3; and where
    // the named forts stand, so their keeps rank with the named villages.
    let mut street_rank: Vec<u8> = Vec::new();
    let mut named_forts: Vec<(f32, f32)> = Vec::new();
    for s in &p.settlements {
        let at = if s.kind == "city" {
            city_at.as_ref().map(|c| (c.0, c.1))
        } else {
            settle(&mut l, &mut srng, s).map(|(at, ends)| {
                if let Some(e) = ends {
                    streets.push(e);
                    street_rank.push(if s.kind == "town" { 1 } else { 2 });
                }
                if s.kind == "fort" {
                    named_forts.push(at);
                }
                at
            })
        };
        let mut o = json!({ "t": s.kind, "asked": [r1(s.at.0), r1(s.at.1)], "at": at.map(|a| json!([r1(a.0), r1(a.1)])) });
        if let Some(n) = &s.name {
            o["name"] = json!(n);
        }
        settled.push(o);
    }
    let per = size * size / 1_000_000.0;
    // How many of a thing: the usual number for the size times the recipe's dial (at least one when it is on).
    let n = |usual: f32, dial: f32| if dial <= 0.0 { 0 } else { ((per * usual * dial).round() as usize).max(1) };
    for _ in 0..n(2.5, p.villages) {
        if let Some(ends) = village(&mut l, &mut rng) {
            streets.push(ends);
        }
    }
    for _ in 0..n(2.0, p.forts) {
        l.scatter(&mut rng, "keep", (18.0, 36.0), (16.0, 32.0), 24.0);
    }
    for _ in 0..n(7.0, p.crags) {
        l.scatter(&mut rng, "crag", (14.0, 45.0), (14.0, 45.0), 12.0);
    }
    for _ in 0..n(3.5, p.lakes) {
        l.scatter(&mut rng, "lake", (18.0, 48.0), (16.0, 40.0), 16.0);
    }
    // Hand edits go in before the land is shaped round the layout, so what they add gets level ground, a clear
    // patch and rivers that bend round it, as anything the seed laid out does. Their own random stream keeps the
    // rest of the world the same when an edit is added.
    let mut erng = Rng(p.seed ^ 0xED17);
    for e in &p.edits {
        l.apply(e, &mut erng);
    }
    // Everything stands on the ground its middle is on; a city's buildings all stand on the city's level.
    let city_base = l.things.iter().find(|s| s.t == "city").map(|s| land.raw((s.b[0] + s.b[2]) / 2.0, (s.b[1] + s.b[3]) / 2.0));
    let city_box = l.things.iter().find(|s| s.t == "city").map(|s| s.b);
    for s in &mut l.things {
        s.base = match (city_box, city_base) {
            (Some(cb), Some(h)) if overlaps(&cb, &s.b, 0.0) => h,
            _ => land.raw((s.b[0] + s.b[2]) / 2.0, (s.b[1] + s.b[3]) / 2.0),
        };
    }
    let rv = if p.courses.is_empty() { rivers(&land, &l, &mut rng, p.rivers) } else { courses(&land, &p.courses, p.seed) };
    let ri = RiverIndex::new(&rv);
    // Roads, once the rivers are known (they cross them as little as they can), and then the houses that line the
    // roads out of the city's gates. Their own random stream leaves everything else as it was.
    let (mut road_list, mut road_cells, fords) = if p.roads {
        let mut net = roads::Net::new(&land, &l, &rv, &ri);
        let mut places = Vec::new();
        if let Some((cx, cy, h, plaza, streets)) = &city_at {
            for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (-1.0, 0.0), (1.0, 0.0)] {
                let out = (cx + dx * (h + 18.0), cy + dy * (h + 18.0));
                let inn = (cx + dx * plaza / 2.0, cy + dy * plaza / 2.0);
                net.lay(roads::Road { half: roads::AVENUE_HALF, pts: vec![out, inn] });
            }
            for s in streets {
                net.lay(roads::Road { half: roads::STREET_HALF, pts: s.to_vec() });
            }
        }
        let big = size > roads::MAX_NETWORK_SIZE;
        if big {
            if let Some((cx, cy, h, _, _)) = &city_at {
                let g = |dx: f32, dy: f32| (cx + dx * (h + 18.0), cy + dy * (h + 18.0));
                places.push(roads::Place::City([g(0.0, -1.0), g(0.0, 1.0), g(-1.0, 0.0), g(1.0, 0.0)]));
            }
        }
        places.extend(streets.iter().enumerate().map(|(i, &e)| roads::Place::Village(e, street_rank.get(i).copied().unwrap_or(3))));
        // A fort's track comes to its door, on the side facing the middle of the world.
        for s in l.things.iter().filter(|s| s.t == "keep") {
            let (kx, ky) = ((s.b[0] + s.b[2]) / 2.0, (s.b[1] + s.b[3]) / 2.0);
            let (mx, my) = city_at.as_ref().map_or((size / 2.0, size / 2.0), |c| (c.0, c.1));
            let (dx, dy) = (mx - kx, my - ky);
            let at = if dx.abs() > dy.abs() { (if dx > 0.0 { s.b[2] + 8.0 } else { s.b[0] - 8.0 }, ky) } else { (kx, if dy > 0.0 { s.b[3] + 8.0 } else { s.b[1] - 8.0 }) };
            let named = named_forts.iter().any(|f| (f.0 - kx).hypot(f.1 - ky) < 60.0);
            places.push(roads::Place::Fort(at, if named { 2 } else { 4 }));
        }
        let mut rrng = Rng(p.seed ^ 0x40AD);
        let centre = city_at.as_ref().map_or((size / 2.0, size / 2.0), |c| (c.0, c.1));
        if big {
            roads::network_big(&mut net, &places, &mut rrng);
        } else {
            roads::network(&mut net, &places, &mut rrng, centre);
        }
        let fords = roads::fords(&net);
        let roads::Net { roads: list, cells, .. } = net;
        (list, cells, fords)
    } else {
        (Vec::new(), roads::Cells::default(), Vec::new())
    };
    if let Some(&(cx, cy, h, _, _)) = city_at.as_ref() {
        let mut srng = Rng(p.seed ^ 0x5B5B);
        let city_box = [cx - h - 10.0, cy - h - 10.0, cx + h + 10.0, cy + h + 10.0];
        for r in road_list.iter().filter(|r| r.half >= roads::MAIN_HALF) {
            for (x, y, a) in roads::along(r, 16.0) {
                let gate = (x - cx).abs().max((y - cy).abs()) - h;
                if !(10.0..160.0).contains(&gate) || srng.f() > 0.75 - gate / 400.0 {
                    continue;
                }
                // A house either side, its front a little back from the road, squared to whichever way it runs more.
                for side in [-1.0f32, 1.0] {
                    let (w, d) = (srng.range(9.0, 14.0), srng.range(8.0, 11.0));
                    let off = r.half + 5.0 + d / 2.0;
                    let (nx, ny) = (-a.sin() * side, a.cos() * side);
                    let (hx, hy) = (x + nx * off, y + ny * off);
                    let (bw, bd) = if a.cos().abs() > a.sin().abs() { (w, d) } else { (d, w) };
                    let b = [hx - bw / 2.0, hy - bd / 2.0, hx + bw / 2.0, hy + bd / 2.0];
                    let wet = |x: f32, y: f32| ri.at(&rv, x, y).is_some_and(|(d, _)| d < RIVER_HALF + RIVER_LIP + 4.0);
                    if overlaps(&b, &city_box, 0.0) || !l.free(&b, 2.0) || wet(hx, hy) {
                        continue;
                    }
                    if [(b[0], b[1]), (b[2], b[1]), (b[0], b[3]), (b[2], b[3]), (hx, hy)].iter().any(|&(px, py)| road_cells.near(px, py, 0.0)) {
                        continue;
                    }
                    let v = srng.v();
                    l.add("house", b, v, true);
                    l.things.last_mut().unwrap().base = land.raw(hx, hy);
                }
            }
        }
    }
    // A big world's farms: farmsteads along the lanes, fields round them and the villages, hedgerows between.
    let farms = if size > roads::MAX_NETWORK_SIZE && p.roads {
        let city = city_at.as_ref().map(|&(cx, cy, h, _, _)| [cx - h, cy - h, cx + h, cy + h]);
        let g = fields::Ground { land: &land, rivers: &rv, ri: &ri, roads: &road_cells, city };
        let mut frng = Rng(p.seed ^ 0xFA4E);
        fields::lay(&mut l, &g, &road_list, &streets, &mut frng)
    } else {
        fields::Farms { fields: Vec::new(), tracks: Vec::new(), hedges: Vec::new() }
    };
    for t in farms.tracks {
        road_cells.add(&t, road_list.len());
        road_list.push(t);
    }
    let in_field = fields::FieldIndex::new(&farms.fields);
    // Things by 64 unit cell (padded), so each height sample checks only those nearby.
    let mut cells: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, s) in l.things.iter().enumerate() {
        let pad = 16.0;
        for cx in ((s.b[0] - pad) / 64.0).floor() as i32..=((s.b[2] + pad) / 64.0).floor() as i32 {
            for cy in ((s.b[1] - pad) / 64.0).floor() as i32..=((s.b[3] + pad) / 64.0).floor() as i32 {
                cells.entry((cx, cy)).or_default().push(i);
            }
        }
    }
    let empty = Vec::new();
    let h_at = |x: f32, y: f32| height(&land, &l, cells.get(&((x / 64.0).floor() as i32, (y / 64.0).floor() as i32)).unwrap_or(&empty), &rv, &ri, &road_cells, x, y);

    // Heightfield over the world plus a margin past each edge, where the mountains are.
    let margin = 256.0f32.min(size / 4.0).max(64.0);
    let cell = (4.0f32).max((size + 2.0 * margin) / 1024.0);
    let n = ((size + 2.0 * margin) / cell).ceil() as usize + 1;
    let mut hs = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            hs.push(h_at(-margin + i as f32 * cell, -margin + j as f32 * cell));
        }
    }
    let (lo, hi) = hs.iter().fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    let span = (hi - lo).max(1e-3);
    let mut bytes = Vec::with_capacity(hs.len() * 2);
    for v in &hs {
        bytes.extend_from_slice(&(((v - lo) / span * 65535.0).round() as u16).to_le_bytes());
    }

    // Scenery: forests where a slow noise says so, meadows elsewhere, kept off water, buildings and streets;
    // a belt of forest just past the edge and rocks on the mountains beyond.
    let mut objects: Vec<String> = Vec::new();
    let mut solid = 0;
    for s in &l.things {
        let mut o = json!({ "t": s.t, "b": [r2(s.b[0]), r2(s.b[1]), r2(s.b[2]), r2(s.b[3])], "v": s.v, "y": r2(s.base) });
        if s.solid {
            o["solid"] = true.into();
            solid += 1;
        }
        objects.push(o.to_string());
    }
    let near_thing = |x: f32, y: f32, pad: f32| {
        cells.get(&((x / 64.0).floor() as i32, (y / 64.0).floor() as i32)).is_some_and(|v| {
            v.iter().any(|&i| {
                let s = &l.things[i];
                let p = if s.t == "city" { 0.0 } else { pad };
                x > s.b[0] - p && x < s.b[2] + p && y > s.b[1] - p && y < s.b[3] + p
            })
        })
    };
    let step = 16.0;
    let span_c = ((size + 2.0 * margin) / step) as i32;
    for j in 0..span_c {
        for i in 0..span_c {
            // Each patch has its own random stream, so an edit only changes the scenery where it is.
            let rng = &mut Rng(p.seed ^ (i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (j as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F) ^ 0x5CE7);
            let (x, y) = (-margin + (i as f32 + rng.f()) * step, -margin + (j as f32 + rng.f()) * step);
            let out = (-x).max(x - size).max(-y).max(y - size);
            let forest = smooth(0.45, 0.7, fbm(x / 260.0, y / 260.0, p.seed + 50, 3)) * p.forest;
            let wood = p.forest.min(1.0);
            // With a coast, the world's edge is open sea: nothing grows at sea or on the beach.
            if land.coast.as_ref().is_some_and(|c| c.d(x, y) < 3.0) {
                continue;
            }
            // Fields are kept clear: their hedgerows are laid with them.
            if in_field.at(x, y) {
                continue;
            }
            let (t, s): (&str, f32) = if out > 70.0 {
                if rng.f() < 0.12 { ("spire", rng.range(2.5, 6.0)) } else { continue }
            } else if out > 25.0 {
                if rng.f() < 0.2 { ("boulder", rng.range(2.5, 6.0)) } else { continue }
            } else if out > -12.0 && rng.f() < 0.85 * wood {
                ("tree", rng.range(1.3, 1.9))
            } else {
                let roll = rng.f();
                if roll < 0.05 * wood + 0.6 * forest {
                    (if rng.f() < 0.07 { "tree_bare" } else { "tree" }, rng.range(1.3, 1.9))
                } else if roll < 0.2 + 0.6 * forest {
                    ("bush", rng.range(2.2, 4.0))
                } else if roll < 0.27 + 0.6 * forest {
                    ("rock", rng.range(1.2, 2.2))
                } else {
                    continue;
                }
            };
            let tree = t.starts_with("tree");
            if out <= 0.0 && near_thing(x, y, if tree { 7.0 } else { 2.5 }) {
                continue;
            }
            if ri.at(&rv, x, y).is_some_and(|(d, _)| d < RIVER_HALF + RIVER_LIP + 2.0) {
                continue;
            }
            if out <= 0.0 && road_cells.near(x, y, if tree { 4.0 } else { 2.0 }) {
                continue;
            }
            let mut o = json!({ "t": t, "p": [r1(x), r1(y)], "s": r2(s), "r": r2(rng.range(0.0, std::f32::consts::TAU)), "v": rng.v() });
            // Trunks and stones are solid where they stand (bushes are walked through): a square footprint the
            // size of the trunk or stone, if it is inside the world, where the engine makes it a wall. `scenery`
            // tells clients this file draws it, so they add no block of their own.
            if let Some(h) = scenery_half(t, s) {
                let (x0, y0, x1, y1) = (r2(x - h), r2(y - h), r2(x + h), r2(y + h));
                if x0 >= 0.0 && y0 >= 0.0 && x1 <= size as f64 && y1 <= size as f64 {
                    o["solid"] = true.into();
                    o["scenery"] = true.into();
                    o["b"] = json!([x0, y0, x1, y1]);
                    solid += 1;
                }
            }
            objects.push(o.to_string());
        }
    }
    for f in &farms.fields {
        let q: Vec<f64> = f.q.iter().flat_map(|&(x, y)| [r1(x), r1(y)]).collect();
        let (cx, cy) = (f.q.iter().map(|p| p.0).sum::<f32>() / 4.0, f.q.iter().map(|p| p.1).sum::<f32>() / 4.0);
        objects.push(json!({ "t": "field", "p": [r1(cx), r1(cy)], "q": q, "k": f.kind, "r": r2(f.furrow), "v": (cx.to_bits() ^ cy.to_bits().rotate_left(7)) }).to_string());
    }
    let mut hrng = Rng(p.seed ^ 0x4ED6);
    for &(t, x, y, s) in &farms.hedges {
        let mut o = json!({ "t": t, "p": [r1(x), r1(y)], "s": r2(s), "r": r2(hrng.range(0.0, std::f32::consts::TAU)), "v": hrng.v() });
        if let Some(h) = scenery_half(t, s) {
            let (x0, y0, x1, y1) = (r2(x - h), r2(y - h), r2(x + h), r2(y + h));
            o["solid"] = true.into();
            o["scenery"] = true.into();
            o["b"] = json!([x0, y0, x1, y1]);
            solid += 1;
        }
        objects.push(o.to_string());
    }
    for &(x, y, a) in &fords {
        objects.push(json!({ "t": "ford", "p": [r1(x), r1(y)], "r": r2(a), "v": (x.to_bits() ^ y.to_bits()) }).to_string());
    }
    // The open sea, past the shallows, is solid (nobody walks the sea floor); it is drawn as water, so it is scenery.
    if let Some(c) = &coast {
        for b in coast::sea_boxes(c, size, SHALLOWS) {
            objects.push(json!({ "t": "sea", "b": [r2(b[0]), r2(b[1]), r2(b[2]), r2(b[3])], "solid": true, "scenery": true, "v": 0, "y": SEA_LEVEL }).to_string());
            solid += 1;
        }
    }
    let roads: Vec<Value> = road_list
        .iter()
        .map(|r| json!({ "half_width": r.half, "pts": r.pts.iter().map(|&(x, y)| json!([r1(x), r1(y)])).collect::<Vec<_>>() }))
        .collect();
    let lakes: Vec<Value> = l.things.iter().filter(|s| s.t == "lake").map(|s| json!({ "b": [r2(s.b[0]), r2(s.b[1]), r2(s.b[2]), r2(s.b[3])], "level": r2(s.base - 0.45) })).collect();
    let rivers: Vec<Value> = rv
        .iter()
        .map(|r| json!({ "half_width": RIVER_HALF, "pts": r.pts.iter().map(|&(x, y, lvl)| json!([r1(x), r1(y), r2(water_level(lvl))])).collect::<Vec<_>>() }))
        .collect();
    let head = json!({
        "format": 1,
        "generator": format!("worldgen {} (fantasy)", env!("CARGO_PKG_VERSION")),
        "size": size,
        "seed": p.seed,
        "recipe": p.recipe(),
        "heights": { "x0": -margin, "y0": -margin, "cell": cell, "w": n, "h": n, "lo": lo, "hi": hi, "u16le": base64(&bytes) },
        "water": if coast.is_some() { json!({ "rivers": rivers, "lakes": lakes, "sea": { "level": SEA_LEVEL } }) } else { json!({ "rivers": rivers, "lakes": lakes }) },
        "roads": roads,
        "settled": settled,
        "objects": [],
    });
    let mut head = head;
    if !farms.fields.is_empty() {
        // Fields are in the objects (`t: "field"`): a theme draws those rather than making up its own round farms.
        head["farms"] = true.into();
    }
    Generated { head, objects, solid }
}

fn main() {
    let mut p = Params::default();
    let mut out = "world.json".to_string();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let val = || args.get(i + 1).cloned().unwrap_or_else(|| usage());
        match args[i].as_str() {
            "--recipe" => {
                let path = val();
                let text = if path == "-" {
                    let mut t = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut t).map(|_| t)
                } else {
                    std::fs::read_to_string(&path)
                };
                let v: Value = text.ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| fail(&format!("{path}: not a JSON recipe")));
                // A whole world file works as a recipe too: it carries the one it was made from.
                let r = if v.get("format").is_some() { v.get("recipe").cloned().unwrap_or(json!({})) } else { v };
                p = Params::from_recipe(&r).unwrap_or_else(|e| fail(&e));
            }
            "--size" => p.size = val().parse().unwrap_or_else(|_| usage()),
            "--seed" => p.seed = val().parse().unwrap_or_else(|_| usage()),
            "--out" => out = val(),
            "--no-city" => {
                p.city = false;
                i += 1;
                continue;
            }
            _ => usage(),
        }
        i += 2;
    }
    if !(64.0..=65536.0).contains(&p.size) {
        usage();
    }
    let w = build(&p);
    // Written beside the target and renamed into place, so a reader never sees half a file.
    let tmp = format!("{out}.tmp");
    let written = std::fs::File::create(&tmp).and_then(|f| {
        let mut f = std::io::BufWriter::with_capacity(1 << 20, f);
        let n = w.write(&mut f)?;
        std::io::Write::flush(&mut f)?;
        Ok(n)
    });
    let n = written.and_then(|n| std::fs::rename(&tmp, &out).map(|_| n)).unwrap_or_else(|e| {
        let _ = std::fs::remove_file(&tmp);
        eprintln!("{out}: {e}");
        std::process::exit(1)
    });
    let water = &w.head["water"];
    println!("wrote {out}: {} KB, {} objects ({} solid), {} rivers, {} lakes", n / 1024, w.objects(), w.solid, water["rivers"].as_array().map_or(0, |a| a.len()), water["lakes"].as_array().map_or(0, |a| a.len()));
}

fn usage() -> ! {
    eprintln!("usage: worldgen [--recipe FILE|-] [--size 2048] [--seed 1] [--out world.json] [--no-city]");
    std::process::exit(2)
}

fn fail(msg: &str) -> ! {
    eprintln!("worldgen: {msg}");
    std::process::exit(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(size: f32, seed: u64) -> Value {
        generate(&Params { size, seed, ..Params::default() })
    }

    fn count(w: &Value, t: &str) -> usize {
        w["objects"].as_array().unwrap().iter().filter(|o| o["t"] == t).count()
    }

    #[test]
    fn a_recipe_round_trips_through_the_world_file() {
        let r = json!({ "size": 1024, "seed": 9, "city": false, "villages": 2, "forest": 0.5, "rivers": 3,
            "edits": [{ "remove": [10, 10] }, { "add": { "t": "tower", "b": [500, 500, 490, 512] } }] });
        let p = Params::from_recipe(&r).unwrap();
        assert_eq!(p.edits[1], Edit::Add("tower", [490.0, 500.0, 500.0, 512.0]), "corners are put in order");
        let w = generate(&p);
        let again = Params::from_recipe(&w["recipe"]).unwrap();
        assert_eq!(generate(&again).to_string(), w.to_string());
        assert_eq!(w["water"]["rivers"].as_array().unwrap().len(), 3);
        assert_eq!(count(&w, "city"), 0);
        for bad in [json!({ "size": 10 }), json!({ "forest": -1 }), json!({ "edits": [{ "add": { "t": "spaceship", "b": [0, 0, 9, 9] } }] }), json!({ "edits": [{ "move": 1 }] })] {
            assert!(Params::from_recipe(&bad).is_err(), "{bad}");
        }
    }

    fn roads_of(w: &Value) -> Vec<(f32, Vec<(f32, f32)>)> {
        w["roads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (r["half_width"].as_f64().unwrap() as f32, r["pts"].as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32)).collect()))
            .collect()
    }

    /// Points along a road's centre line, every unit or so.
    fn samples(pts: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        for w in pts.windows(2) {
            let n = ((w[1].0 - w[0].0).hypot(w[1].1 - w[0].1)).ceil().max(1.0) as i32;
            out.extend((0..=n).map(|k| (w[0].0 + (w[1].0 - w[0].0) * k as f32 / n as f32, w[0].1 + (w[1].1 - w[0].1) * k as f32 / n as f32)));
        }
        out
    }

    #[test]
    fn roads_join_every_village_and_fort_to_the_city_and_go_round_what_is_solid() {
        for seed in [1, 1066] {
            let w = world(2048.0, seed);
            let roads = roads_of(&w);
            assert!(roads.len() > 20, "seed {seed}: {} roads", roads.len());
            // Roads that share a 4 unit cell are joined; every road should be joined to every other.
            let mut owner: HashMap<(i32, i32), usize> = HashMap::new();
            let mut parent: Vec<usize> = (0..roads.len()).collect();
            fn find(p: &mut Vec<usize>, i: usize) -> usize {
                if p[i] != i {
                    let r = find(p, p[i]);
                    p[i] = r;
                }
                p[i]
            }
            for (i, (half, pts)) in roads.iter().enumerate() {
                for (x, y) in samples(pts) {
                    let reach = (half / 4.0).ceil() as i32 + 1;
                    for dj in -reach..=reach {
                        for di in -reach..=reach {
                            let c = ((x / 4.0).floor() as i32 + di, (y / 4.0).floor() as i32 + dj);
                            if let Some(&j) = owner.get(&c) {
                                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                                parent[a] = b;
                            } else if di.abs() <= 1 && dj.abs() <= 1 {
                                owner.insert(c, i);
                            }
                        }
                    }
                }
            }
            let root = find(&mut parent, 0);
            let apart = (0..roads.len()).filter(|&i| find(&mut parent, i) != root).count();
            assert!(apart <= roads.len() / 10, "seed {seed}: {apart} of {} roads are not joined to the rest", roads.len());
            let solids: Vec<[f32; 4]> = w["objects"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|o| o["solid"] == true)
                .map(|o| {
                    let b = o["b"].as_array().unwrap();
                    [0, 1, 2, 3].map(|k| b[k].as_f64().unwrap() as f32)
                })
                .collect();
            for (_, pts) in &roads {
                for (x, y) in samples(pts) {
                    assert!(!solids.iter().any(|b| x > b[0] + 0.5 && x < b[2] - 0.5 && y > b[1] + 0.5 && y < b[3] - 0.5), "seed {seed}: a road runs through something solid at ({x}, {y})");
                }
            }
        }
    }

    #[test]
    fn a_big_world_has_tiered_roads_farms_and_fields() {
        // Past the fine-grid limit: highways between the city and the towns, lanes and tracks, and farms.
        let p = Params::from_recipe(&json!({
            "size": 17000, "seed": 3, "villages": 0.3, "forts": 0.2, "crags": 0.2, "lakes": 0.2, "forest": 0.6,
            "settlements": [
                { "t": "city", "at": [8500, 8500], "name": "Capital" },
                { "t": "town", "at": [3500, 4000], "name": "West" },
                { "t": "town", "at": [13500, 12500], "name": "East" },
                { "t": "village", "at": [8000, 14000], "name": "South" }
            ]
        }))
        .unwrap();
        let w = generate(&p);
        let roads = roads_of(&w);
        let wide = |lo: f32, hi: f32| roads.iter().filter(|(h, _)| (lo..=hi).contains(h)).count();
        assert!(wide(3.4, 3.6) >= 2, "highways between the city and the towns");
        assert!(wide(1.7, 1.9) > 0 && wide(1.2, 1.4) > 0, "lanes and farm tracks");
        assert_eq!(w["farms"], true);
        let fields: Vec<&Value> = w["objects"].as_array().unwrap().iter().filter(|o| o["t"] == "field").collect();
        assert!(fields.len() > 100, "{} fields", fields.len());
        // A field is never laid over a road: no road runs through its middle.
        for f in fields.iter().step_by(7) {
            let (x, y) = (f["p"][0].as_f64().unwrap() as f32, f["p"][1].as_f64().unwrap() as f32);
            for (half, pts) in &roads {
                assert!(!samples(pts).iter().any(|s| (s.0 - x).hypot(s.1 - y) < *half), "a road through the field at ({x}, {y})");
            }
            assert_eq!(f["q"].as_array().unwrap().len(), 8);
        }
    }

    #[test]
    fn villages_line_a_street_and_roads_can_be_left_out() {
        let w = world(2048.0, 5);
        assert!(count(&w, "house") > 150, "{} houses", count(&w, "house"));
        let trees: Vec<(f32, f32)> = w["objects"].as_array().unwrap().iter().filter(|o| o["t"] == "tree").map(|o| (o["p"][0].as_f64().unwrap() as f32, o["p"][1].as_f64().unwrap() as f32)).collect();
        for (half, pts) in roads_of(&w) {
            for (x, y) in samples(&pts).into_iter().step_by(4) {
                assert!(!trees.iter().any(|t| (t.0 - x).hypot(t.1 - y) < half), "a tree stands on the road at ({x}, {y})");
            }
        }
        let p = Params::from_recipe(&json!({ "size": 2048, "seed": 5, "roads": false })).unwrap();
        let bare = generate(&p);
        assert!(roads_of(&bare).is_empty() && count(&bare, "ford") == 0);
        assert_eq!(bare["recipe"]["roads"], false);
    }

    #[test]
    fn a_coast_makes_an_island_and_the_recipe_puts_its_places_and_rivers_where_it_says() {
        let island = json!([[[300, 300], [1700, 300], [1700, 1700], [300, 1700]]]);
        let r = json!({ "size": 2048, "seed": 4, "villages": 0, "forts": 0, "coast": island,
            "river_courses": [[[1000, 600], [1000, 1000], [1000, 1800]]],
            "settlements": [{ "t": "city", "at": [700, 700], "side": 240, "name": "Áth Cliath" }, { "t": "town", "at": [1400, 1400], "name": "Corcaigh" },
                { "t": "village", "at": [600, 1400] }, { "t": "fort", "at": [1400, 600], "name": "Teamhair" }] });
        let p = Params::from_recipe(&r).unwrap();
        let w = generate(&p);
        assert_eq!(generate(&Params::from_recipe(&w["recipe"]).unwrap()).to_string(), w.to_string(), "the recipe round trips");
        assert_eq!(w["water"]["sea"]["level"], 0.0);
        // Every place went in, near where it was asked for, and says so by name.
        let settled = w["settled"].as_array().unwrap();
        assert_eq!(settled.len(), 4);
        for s in settled {
            let (a, b) = (&s["asked"], &s["at"]);
            let d = (a[0].as_f64().unwrap() - b[0].as_f64().unwrap()).hypot(a[1].as_f64().unwrap() - b[1].as_f64().unwrap());
            assert!(d < 120.0, "{s}");
        }
        assert_eq!(settled[0]["name"], "Áth Cliath");
        assert_eq!(count(&w, "city"), 1);
        // The sea is solid scenery, all of it well out from the shore; nothing else stands at sea.
        let objs = w["objects"].as_array().unwrap();
        let sea: Vec<&Value> = objs.iter().filter(|o| o["t"] == "sea").collect();
        assert!(!sea.is_empty() && sea.iter().all(|o| o["solid"] == true && o["scenery"] == true));
        for o in objs.iter().filter(|o| o["t"] == "house" || o["t"] == "tree") {
            let (x, y) = if o["t"] == "tree" { (o["p"][0].as_f64().unwrap(), o["p"][1].as_f64().unwrap()) } else { ((o["b"][0].as_f64().unwrap() + o["b"][2].as_f64().unwrap()) / 2.0, (o["b"][1].as_f64().unwrap() + o["b"][3].as_f64().unwrap()) / 2.0) };
            assert!((300.0..=1700.0).contains(&x) && (300.0..=1700.0).contains(&y), "a {} at sea at ({x}, {y})", o["t"]);
        }
        // The river follows its course, and no house stands in it.
        let river = &w["water"]["rivers"][0]["pts"];
        assert!(river.as_array().unwrap().iter().all(|p| (p[0].as_f64().unwrap() - 1000.0).abs() < 40.0));
        for o in objs.iter().filter(|o| o["t"] == "house") {
            let b = o["b"].as_array().unwrap();
            assert!(!(b[0].as_f64().unwrap() < 1005.0 && b[2].as_f64().unwrap() > 995.0 && b[1].as_f64().unwrap() > 650.0 && b[3].as_f64().unwrap() < 1650.0), "a house in the river: {o}");
        }
        assert!(Params::from_recipe(&json!({ "settlements": [{ "t": "castle", "at": [1, 1] }] })).is_err());
        assert!(Params::from_recipe(&json!({ "coast": [[[1, 1], [2, 2]]] })).is_err(), "a coast needs three points");
    }

    #[test]
    fn dials_change_how_much_of_each_thing_there_is() {
        // Without the city, whose streets of houses would swamp the villages' count.
        let base = generate(&Params { size: 2048.0, seed: 4, city: false, ..Params::default() });
        let more = generate(&Params { size: 2048.0, seed: 4, city: false, villages: 3.0, crags: 0.0, forest: 0.0, ..Params::default() });
        assert!(count(&more, "house") > count(&base, "house") * 2, "{} vs {}", count(&more, "house"), count(&base, "house"));
        assert_eq!(count(&more, "crag"), 0);
        assert!(count(&more, "tree") < count(&base, "tree") / 3, "{} vs {}", count(&more, "tree"), count(&base, "tree"));
    }

    #[test]
    fn edits_add_and_remove_footprints_and_the_land_is_shaped_round_them() {
        let base = world(2048.0, 6);
        let house = base["objects"].as_array().unwrap().iter().find(|o| o["t"] == "house").unwrap().clone();
        let b: Vec<f32> = house["b"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        let (mx, my) = ((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0);
        let p = Params {
            size: 2048.0,
            seed: 6,
            edits: vec![Edit::Remove(mx, my), Edit::Add("castle", [100.0, 100.0, 160.0, 150.0])],
            ..Params::default()
        };
        let w = generate(&p);
        let objs = w["objects"].as_array().unwrap();
        assert!(!objs.iter().any(|o| o["b"] == house["b"]), "the house is gone");
        let castle = objs.iter().find(|o| o["t"] == "castle" && o["b"][0] == 100.0).expect("the castle is there");
        assert_eq!(castle["solid"], true);
        // Nothing solid overlaps it and no tree stands on it.
        for o in objs {
            if let Some(q) = o["p"].as_array() {
                let (x, y) = (q[0].as_f64().unwrap(), q[1].as_f64().unwrap());
                assert!(!(x > 100.0 && x < 160.0 && y > 100.0 && y < 150.0), "a {} at {x},{y}", o["t"]);
            } else if o["solid"] == true && o["t"] != "castle" {
                let ob = o["b"].as_array().unwrap();
                let ov = ob[0].as_f64().unwrap() < 160.0 && ob[2].as_f64().unwrap() > 100.0 && ob[1].as_f64().unwrap() < 150.0 && ob[3].as_f64().unwrap() > 100.0;
                assert!(!ov, "{} overlaps the castle", o["t"]);
            }
        }
        assert_eq!(count(&w, "house"), count(&base, "house") - 1);
        // Edits are local: rivers that do not pass the castle keep their course, and trees far from it stay put.
        let tree_far = |w: &Value| w["objects"].as_array().unwrap().iter().filter(|o| o["t"] == "tree" && o["p"][0].as_f64().unwrap() > 1200.0 && o["p"][1].as_f64().unwrap() > 1200.0).count();
        assert_eq!(tree_far(&w), tree_far(&base));
        let same = (0..w["water"]["rivers"].as_array().unwrap().len()).filter(|&k| w["water"]["rivers"][k] == base["water"]["rivers"][k]).count();
        assert!(same >= 1, "every river moved");
    }

    #[test]
    fn same_seed_same_world_different_seed_different_world() {
        assert_eq!(world(1024.0, 7).to_string(), world(1024.0, 7).to_string());
        assert_ne!(world(1024.0, 7).to_string(), world(1024.0, 8).to_string());
    }

    #[test]
    fn solids_are_inside_the_world_and_do_not_overlap() {
        let w = world(2048.0, 3);
        // Fortifications are meant to meet: wall runs overlap at corners and towers straddle them, so they are
        // left out here.
        let solids: Vec<[f64; 4]> = w["objects"].as_array().unwrap().iter().filter(|o| o["solid"] == true && o["t"] != "tower" && o["t"] != "wall").map(|o| {
            let b = o["b"].as_array().unwrap();
            [b[0].as_f64().unwrap(), b[1].as_f64().unwrap(), b[2].as_f64().unwrap(), b[3].as_f64().unwrap()]
        }).collect();
        assert!(solids.len() > 100, "{} solids", solids.len());
        for (i, a) in solids.iter().enumerate() {
            assert!(a[0] >= 0.0 && a[1] >= 0.0 && a[2] <= 2048.0 && a[3] <= 2048.0 && a[0] < a[2] && a[1] < a[3], "{a:?}");
            for b in &solids[i + 1..] {
                // Terraced houses and the castle's towers share edges, which is not an overlap.
                let ov = a[0] < b[2] - 0.01 && a[2] > b[0] + 0.01 && a[1] < b[3] - 0.01 && a[3] > b[1] + 0.01;
                assert!(!ov, "{a:?} overlaps {b:?}");
            }
        }
    }

    #[test]
    fn has_a_city_villages_rivers_and_scenery() {
        let w = world(2048.0, 1);
        let count = |t: &str| w["objects"].as_array().unwrap().iter().filter(|o| o["t"] == t).count();
        assert_eq!(count("city"), 1);
        assert!(count("house") > 100, "{} houses", count("house"));
        assert!(count("tree") > 1000, "{} trees", count("tree"));
        assert!(w["water"]["rivers"].as_array().unwrap().len() >= 2);
        let hs = &w["heights"];
        let n = hs["w"].as_u64().unwrap() * hs["h"].as_u64().unwrap();
        assert_eq!(hs["u16le"].as_str().unwrap().len() as u64, (n * 2).div_ceil(3) * 4);
    }

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn the_streamed_file_is_the_value_written_out() {
        let g = build(&Params { size: 1024.0, seed: 3, ..Params::default() });
        let mut out = Vec::new();
        let n = g.write(&mut out).unwrap();
        assert_eq!((String::from_utf8(out).unwrap(), n), (g.to_value().to_string(), g.to_value().to_string().len()));
        assert_eq!(g.solid, g.to_value()["objects"].as_array().unwrap().iter().filter(|o| o["solid"] == true).count());
    }

    #[test]
    fn trees_and_rocks_are_solid_where_they_stand_and_bushes_are_not() {
        let w = generate(&Params { size: 1024.0, ..Params::default() });
        let objs = w["objects"].as_array().unwrap();
        let solid = |t: &str| objs.iter().filter(|o| o["t"] == t && o["solid"] == true && o["scenery"] == true).count();
        assert!(solid("tree") > 200, "{} solid trees", solid("tree"));
        assert!(solid("rock") > 0 && solid("bush") == 0);
        // Each footprint is inside the world and centred on the object.
        for o in objs.iter().filter(|o| o["t"] == "tree" && o["solid"] == true) {
            let (b, p) = (o["b"].as_array().unwrap(), o["p"].as_array().unwrap());
            let f = |v: &Value| v.as_f64().unwrap();
            assert!(f(&b[0]) >= 0.0 && f(&b[2]) <= 1024.0 && f(&b[0]) < f(&p[0]) && f(&p[0]) < f(&b[2]));
        }
    }
}

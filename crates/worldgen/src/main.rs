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

use serde_json::{json, Value};
use std::collections::HashMap;

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
}

impl Layout {
    fn free(&self, b: &Box4, pad: f32) -> bool {
        b[0] >= 8.0 && b[1] >= 8.0 && b[2] <= self.size - 8.0 && b[3] <= self.size - 8.0 && !self.things.iter().any(|s| overlaps(&s.b, b, pad))
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
fn city(l: &mut Layout, rng: &mut Rng, cx: f32, cy: f32, side: f32) {
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
    for qx in [-1.0f32, 1.0] {
        for qy in [-1.0f32, 1.0] {
            let (lo_x, hi_x) = if qx < 0.0 { (cx - inner, cx - AVENUE / 2.0) } else { (cx + AVENUE / 2.0, cx + inner) };
            let (lo_y, hi_y) = if qy < 0.0 { (cy - inner, cy - AVENUE / 2.0) } else { (cy + AVENUE / 2.0, cy + inner) };
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
}

/// A village: a green with a ring of houses around it, lanes between.
fn village(l: &mut Layout, rng: &mut Rng) -> bool {
    for _ in 0..60 {
        let (cx, cy) = (rng.range(120.0, l.size - 120.0), rng.range(120.0, l.size - 120.0));
        let area = [cx - 75.0, cy - 75.0, cx + 75.0, cy + 75.0];
        if !l.free(&area, 30.0) {
            continue;
        }
        l.add("square", [cx - 14.0, cy - 14.0, cx + 14.0, cy + 14.0], rng.v(), false);
        let n = 5 + (rng.v() % 5) as usize;
        let mut placed = 0;
        for k in 0..n * 6 {
            if placed >= n {
                break;
            }
            let a = (k as f32 / n as f32) * std::f32::consts::TAU + rng.range(-0.3, 0.3);
            let r = rng.range(28.0, 58.0);
            let (w, d) = (rng.range(10.0, 18.0), rng.range(8.0, 13.0));
            let (w, d) = if a.cos().abs() > a.sin().abs() { (d, w) } else { (w, d) };
            let (x, y) = (cx + a.cos() * r - w / 2.0, cy + a.sin() * r - d / 2.0);
            let b = [x, y, x + w, y + d];
            if l.free(&b, 7.0) {
                let v = rng.v();
                l.add("house", b, v, true);
                placed += 1;
            }
        }
        if rng.f() < 0.5 {
            let t = 4.5;
            let (x, y) = (cx + rng.range(-8.0, 8.0), cy - 60.0);
            let b = [x - t, y - t, x + t, y + t];
            if l.free(&b, 6.0) {
                let v = rng.v();
                l.add("tower", b, v, true);
            }
        }
        return true;
    }
    false
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
}

impl Land {
    /// Hills, rising into mountains past the playable edge.
    fn raw(&self, x: f32, y: f32) -> f32 {
        let mut h = ((fbm(x / 170.0, y / 170.0, self.seed + 31, 3) - 0.5) * 30.0 + (noise(x / 35.0, y / 35.0, self.seed + 7) - 0.5) * 1.8) * self.hills;
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
fn height(land: &Land, l: &Layout, near: &[usize], rivers: &[River], ri: &RiverIndex, x: f32, y: f32) -> f32 {
    let mut h = land.raw(x, y);
    if let Some((d, lvl)) = ri.at(rivers, x, y) {
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
    /// Hand edits on top of what the seed lays out, applied in order.
    pub edits: Vec<Edit>,
}

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
        Params { size: 2048.0, seed: 1, city: true, villages: 1.0, forts: 1.0, crags: 1.0, lakes: 1.0, rivers: None, forest: 1.0, hills: 1.0, edits: Vec::new() }
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
        for (k, f) in [("villages", &mut p.villages), ("forts", &mut p.forts), ("crags", &mut p.crags), ("lakes", &mut p.lakes), ("forest", &mut p.forest), ("hills", &mut p.hills)] {
            if let Some(n) = num(k, 0.0, 5.0)? {
                *f = n as f32;
            }
        }
        p.rivers = num("rivers", 0.0, 12.0)?.map(|n| n as usize);
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
            "lakes": self.lakes, "rivers": self.rivers, "forest": self.forest, "hills": self.hills, "edits": edits,
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
    let land = Land { size, seed: p.seed, hills: p.hills };
    let mut l = Layout { size, things: Vec::new() };
    if p.city && size >= 1024.0 {
        let side = (size / 6.0).clamp(240.0, 600.0);
        let (cx, cy) = (rng.range(0.3, 0.7) * size, rng.range(0.3, 0.7) * size);
        city(&mut l, &mut rng, cx, cy, side);
    }
    let per = size * size / 1_000_000.0;
    // How many of a thing: the usual number for the size times the recipe's dial (at least one when it is on).
    let n = |usual: f32, dial: f32| if dial <= 0.0 { 0 } else { ((per * usual * dial).round() as usize).max(1) };
    for _ in 0..n(2.5, p.villages) {
        village(&mut l, &mut rng);
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
    let rv = rivers(&land, &l, &mut rng, p.rivers);
    let ri = RiverIndex::new(&rv);
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
    let h_at = |x: f32, y: f32| height(&land, &l, cells.get(&((x / 64.0).floor() as i32, (y / 64.0).floor() as i32)).unwrap_or(&empty), &rv, &ri, x, y);

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
        "water": { "rivers": rivers, "lakes": lakes },
        "objects": [],
    });
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

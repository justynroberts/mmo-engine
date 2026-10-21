//! Bot navigation: a walkability grid over the static walls, and flow fields toward the places and route points
//! bots walk to, so they find their way round buildings instead of pushing into them.
//!
//! The grid is 2 unit cells, built a 256 unit tile (128 x 128 cells, one bit each, 2 KB) at a time when first
//! needed and kept until the world changes. A cell is walkable when a body of the entity radius stands clear of
//! every wall at its centre. Built blocks are left out: they change every tick, and a bot that meets one climbs
//! it or slides along it as before.
//!
//! A field is a Dijkstra distance (2 straight, 3 diagonal, never cutting a blocked corner) over a window 128
//! cells each way of its target. At most `per_tick` fields are built a tick and at most `MAX_FIELDS` are kept,
//! the least recently used going first. A bot outside a field's window, or whose field is not built yet, steers
//! straight as before.

use proto::world::World;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

/// Edge of a grid cell, in world units.
pub const CELL: f32 = 2.0;
/// Edge of a tile, in cells.
const TILE: i32 = 128;
const WORDS: usize = (TILE * TILE / 64) as usize;
/// Cells each way of a field's target.
const WIN: i32 = 128;
const SPAN: i32 = 2 * WIN + 1;
pub const MAX_FIELDS: usize = 64;
/// Tiles kept before the lot is dropped (8 MB); fields hold their own copy, so nothing else is lost.
const MAX_TILES: usize = 4096;
/// Fields built per tick unless changed.
pub const FIELDS_PER_TICK: u32 = 1;
/// Unreachable (or a wall).
const FAR: u16 = u16::MAX;
/// Steps of the descent looked along for the farthest cell in sight.
const LOOK: usize = 6;
/// Within this distance (about three cells) of the target a bot walks straight at it.
const NEAR: u16 = 6;

/// Multiply-and-rotate hash for packed cell keys: SipHash is wasted on them.
#[derive(Default)]
pub struct KeyHash(u64);

impl Hasher for KeyHash {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, b: &[u8]) {
        for &x in b {
            self.write_u64(x as u64);
        }
    }
    fn write_u64(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

type Map<V> = HashMap<u64, V, BuildHasherDefault<KeyHash>>;

fn key(x: i32, y: i32) -> u64 {
    ((x as u32 as u64) << 32) | y as u32 as u64
}

/// The cell a world position is in.
pub fn cell_of(x: f32, y: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (y / CELL).floor() as i32)
}

struct Tile {
    bits: [u64; WORDS],
}

struct Field {
    ox: i32,
    oy: i32,
    dist: Vec<u16>,
    used: u64,
}

pub struct Nav {
    tiles: Map<Box<Tile>>,
    fields: Map<Field>,
    /// What the grid was built from: the walls (by address and count), the world size and the body radius.
    stamp: (usize, usize, u32, u32),
    tick: u32,
    built: u32,
    clock: u64,
    /// New fields allowed per tick.
    pub per_tick: u32,
    free: Vec<bool>,
    queue: [Vec<u32>; 4],
}

impl Default for Nav {
    fn default() -> Self {
        Nav {
            tiles: Map::default(),
            fields: Map::default(),
            stamp: (0, 0, 0, 0),
            tick: u32::MAX,
            built: 0,
            clock: 0,
            per_tick: FIELDS_PER_TICK,
            free: Vec::new(),
            queue: Default::default(),
        }
    }
}

const N8: [(i32, i32, u16); 8] = [(1, 0, 2), (-1, 0, 2), (0, 1, 2), (0, -1, 2), (1, 1, 3), (1, -1, 3), (-1, 1, 3), (-1, -1, 3)];

impl Nav {
    /// Forget every tile and field (the walls changed).
    pub fn clear(&mut self) {
        self.tiles.clear();
        self.fields.clear();
    }

    #[cfg(test)]
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    #[cfg(test)]
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Clear everything if the world or the radius is not the one the grid was built for (also catches a world
    /// swapped in without `Sim::replace_world`), and start a new tick's budget.
    fn sync(&mut self, world: &World, r: f32, tick: u32) {
        let stamp = (world.boxes.as_ptr() as usize, world.boxes.len(), world.size.to_bits(), r.to_bits());
        if stamp != self.stamp {
            self.stamp = stamp;
            self.clear();
        }
        if tick != self.tick {
            self.tick = tick;
            self.built = 0;
        }
    }

    /// Mark the cells of `[i0, i0 + k)` squared that are walkable. Each test is one box query covering every
    /// body centred in the block, so open ground costs one query and only blocks touching a wall split up.
    fn fill(t: &mut Tile, world: &World, r: f32, tx0: i32, ty0: i32, i0: i32, j0: i32, k: i32) {
        let (x, y) = ((i0 * 2 + k) as f32 * CELL * 0.5, (j0 * 2 + k) as f32 * CELL * 0.5);
        if !world.blocked_by_walls(x, y, (k - 1) as f32 * CELL * 0.5 + r) {
            for j in j0..j0 + k {
                for i in i0..i0 + k {
                    let n = ((j - ty0) * TILE + (i - tx0)) as usize;
                    t.bits[n / 64] |= 1 << (n % 64);
                }
            }
        } else if k > 1 {
            let h = k / 2;
            for (di, dj) in [(0, 0), (h, 0), (0, h), (h, h)] {
                Self::fill(t, world, r, tx0, ty0, i0 + di, j0 + dj, h);
            }
        }
    }

    fn tile(&mut self, world: &World, r: f32, tx: i32, ty: i32) -> &Tile {
        if self.tiles.len() >= MAX_TILES && !self.tiles.contains_key(&key(tx, ty)) {
            self.tiles.clear();
        }
        self.tiles.entry(key(tx, ty)).or_insert_with(|| {
            let mut t = Box::new(Tile { bits: [0; WORDS] });
            Self::fill(&mut t, world, r, tx * TILE, ty * TILE, tx * TILE, ty * TILE, TILE);
            t
        })
    }

    /// Build the field toward cell `(cx, cy)`, recycling the evicted field's buffer.
    fn build(&mut self, world: &World, r: f32, cx: i32, cy: i32) -> Field {
        let (ox, oy) = (cx - WIN, cy - WIN);
        let n = (SPAN * SPAN) as usize;
        let cells = (world.size / CELL).ceil() as i32;
        let mut free = std::mem::take(&mut self.free);
        free.clear();
        free.resize(n, false);
        // Copy the window's walkability out of the tiles it overlaps (cells outside the world stay blocked).
        let (lo_x, hi_x) = (ox.max(0), (ox + SPAN).min(cells));
        let (lo_y, hi_y) = (oy.max(0), (oy + SPAN).min(cells));
        if lo_x < hi_x && lo_y < hi_y {
            for ty in lo_y.div_euclid(TILE)..=(hi_y - 1).div_euclid(TILE) {
                for tx in lo_x.div_euclid(TILE)..=(hi_x - 1).div_euclid(TILE) {
                    let t = self.tile(world, r, tx, ty);
                    for j in lo_y.max(ty * TILE)..hi_y.min((ty + 1) * TILE) {
                        for i in lo_x.max(tx * TILE)..hi_x.min((tx + 1) * TILE) {
                            let b = ((j - ty * TILE) * TILE + (i - tx * TILE)) as usize;
                            free[((j - oy) * SPAN + (i - ox)) as usize] = t.bits[b / 64] >> (b % 64) & 1 == 1;
                        }
                    }
                }
            }
        }
        let mut dist = if self.fields.len() >= MAX_FIELDS {
            let (&k, _) = self.fields.iter().min_by_key(|(_, f)| f.used).unwrap();
            self.fields.remove(&k).unwrap().dist
        } else {
            Vec::new()
        };
        dist.clear();
        dist.resize(n, FAR);
        // Seed the target, or if it is in a wall the free cells of the nearest ring around it that has any.
        let mut q = std::mem::take(&mut self.queue);
        let at = |i: i32, j: i32| (j * SPAN + i) as usize;
        let mut pending = 0usize;
        for ring in 0..=WIN {
            for j in WIN - ring..=WIN + ring {
                for i in WIN - ring..=WIN + ring {
                    if (i - WIN).abs().max((j - WIN).abs()) == ring && free[at(i, j)] {
                        dist[at(i, j)] = 0;
                        q[0].push(at(i, j) as u32);
                        pending += 1;
                    }
                }
            }
            if pending > 0 {
                break;
            }
        }
        // Dial's algorithm: edges cost 2 or 3, so four buckets in a ring hold every distance still to come.
        let mut d: u32 = 0;
        while pending > 0 {
            let b = (d % 4) as usize;
            let mut list = std::mem::take(&mut q[b]);
            for &c in &list {
                pending -= 1;
                let c = c as usize;
                if dist[c] as u32 != d {
                    continue;
                }
                let (i, j) = ((c as i32) % SPAN, (c as i32) / SPAN);
                for (di, dj, cost) in N8 {
                    let (ni, nj) = (i + di, j + dj);
                    if ni < 0 || nj < 0 || ni >= SPAN || nj >= SPAN || !free[at(ni, nj)] {
                        continue;
                    }
                    if di != 0 && dj != 0 && !(free[at(i + di, j)] && free[at(i, j + dj)]) {
                        continue;
                    }
                    let nd = d + cost as u32;
                    let m = at(ni, nj);
                    if nd < FAR as u32 && nd < dist[m] as u32 {
                        dist[m] = nd as u16;
                        q[(nd % 4) as usize].push(m as u32);
                        pending += 1;
                    }
                }
            }
            list.clear();
            q[b] = list;
            d += 1;
        }
        self.queue = q;
        self.free = free;
        Field { ox, oy, dist, used: 0 }
    }

    /// Which way a bot at `(x, y)` should go to reach the static point `(tx, ty)`, or None to steer straight:
    /// outside the field's window, near the target, unreachable, or the field not built yet (built now when
    /// `build` is set and the tick's budget allows).
    #[allow(clippy::too_many_arguments)]
    pub fn steer(&mut self, world: &World, r: f32, tick: u32, x: f32, y: f32, tx: f32, ty: f32, build: bool) -> Option<(i8, i8)> {
        self.sync(world, r, tick);
        let (cx, cy) = cell_of(tx, ty);
        let k = key(cx, cy);
        let (bx, by) = cell_of(x, y);
        if (bx - cx).abs() > WIN || (by - cy).abs() > WIN {
            return None;
        }
        if !self.fields.contains_key(&k) {
            if !build || self.built >= self.per_tick {
                return None;
            }
            self.built += 1;
            let f = self.build(world, r, cx, cy);
            self.fields.insert(k, f);
        }
        self.clock += 1;
        let f = self.fields.get_mut(&k)?;
        f.used = self.clock;
        let d = |i: i32, j: i32| -> u16 {
            let (li, lj) = (i - f.ox, j - f.oy);
            if li < 0 || lj < 0 || li >= SPAN || lj >= SPAN { FAR } else { f.dist[(lj * SPAN + li) as usize] }
        };
        let here = d(bx, by);
        if here <= NEAR {
            return None;
        }
        // Walk down the field a few steps, then aim at the farthest of them in plain sight.
        let (mut ci, mut cj, mut cd) = (bx, by, here);
        let mut aim = None;
        for step in 0..LOOK {
            let mut best: Option<(i32, i32, u16)> = None;
            for (di, dj, _) in N8 {
                let nd = d(ci + di, cj + dj);
                if nd == FAR || (di != 0 && dj != 0 && (d(ci + di, cj) == FAR || d(ci, cj + dj) == FAR)) {
                    continue;
                }
                if nd < cd && best.is_none_or(|b| nd < b.2) {
                    best = Some((ci + di, cj + dj, nd));
                }
            }
            let Some((ni, nj, nd)) = best else { break };
            (ci, cj, cd) = (ni, nj, nd);
            let (ax, ay) = ((ci as f32 + 0.5) * CELL, (cj as f32 + 0.5) * CELL);
            if step == 0 || Self::clear_line(&d, x, y, ax, ay) {
                aim = Some((ax, ay));
            }
            if cd == 0 {
                break;
            }
        }
        let (ax, ay) = aim?;
        let (dx, dy) = (ax - x, ay - y);
        let l = (dx * dx + dy * dy).sqrt();
        if l == 0.0 {
            return None;
        }
        let q = |v: f32| (v / l * 127.0).round().clamp(-127.0, 127.0) as i8;
        Some((q(dx), q(dy)))
    }

    /// True if every cell along the line, sampled each half cell, is reachable.
    fn clear_line(d: &impl Fn(i32, i32) -> u16, x: f32, y: f32, ax: f32, ay: f32) -> bool {
        let n = (((ax - x).abs().max((ay - y).abs())) / (CELL * 0.5)).ceil().max(1.0) as i32;
        (1..=n).all(|s| {
            let t = s as f32 / n as f32;
            let (i, j) = cell_of(x + (ax - x) * t, y + (ay - y) * t);
            d(i, j) != FAR
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::world::Aabb;

    fn u_world() -> World {
        // A cup open to the south (low y) around (250, 175), its base between it and (250, 260).
        World::from_boxes(
            512.0,
            1,
            vec![
                Aabb { x0: 200.0, y0: 200.0, x1: 300.0, y1: 204.0 },
                Aabb { x0: 200.0, y0: 140.0, x1: 204.0, y1: 204.0 },
                Aabb { x0: 296.0, y0: 140.0, x1: 300.0, y1: 204.0 },
            ],
        )
    }

    #[test]
    fn tiles_match_a_query_per_cell() {
        let w = u_world();
        let mut n = Nav::default();
        n.sync(&w, 0.5, 0);
        for (tx, ty) in [(0, 0), (0, 1), (1, 1), (1, 0)] {
            let t = n.tile(&w, 0.5, tx, ty);
            let bits = t.bits;
            for j in 0..TILE {
                for i in 0..TILE {
                    let (gi, gj) = (tx * TILE + i, ty * TILE + j);
                    let want = !w.blocked_by_walls((gi as f32 + 0.5) * CELL, (gj as f32 + 0.5) * CELL, 0.5);
                    let b = (j * TILE + i) as usize;
                    assert_eq!(bits[b / 64] >> (b % 64) & 1 == 1, want, "cell {gi},{gj}");
                }
            }
        }
    }

    #[test]
    fn budget_limits_new_fields_per_tick_and_the_cache_is_bounded() {
        let w = World::from_boxes(4096.0, 1, vec![]);
        let mut n = Nav { per_tick: 2, ..Default::default() };
        let goals: Vec<(f32, f32)> = (0..5).map(|i| (100.0 + i as f32 * 40.0, 100.0)).collect();
        for &(gx, gy) in &goals {
            n.steer(&w, 0.5, 1, gx, gy + 60.0, gx, gy, true);
        }
        assert_eq!(n.field_count(), 2);
        // A bot whose field is not built gets None and steers straight; an existing field costs no budget.
        assert!(n.steer(&w, 0.5, 1, 260.0, 160.0, 260.0, 100.0, true).is_none());
        assert!(n.steer(&w, 0.5, 1, 100.0, 160.0, 100.0, 100.0, true).is_some());
        for &(gx, gy) in &goals {
            n.steer(&w, 0.5, 2, gx, gy + 60.0, gx, gy, true);
        }
        assert_eq!(n.field_count(), 4);
        for t in 0..80u32 {
            n.steer(&w, 0.5, 3 + t, 300.0, 300.0, 300.0 + (t % 70) as f32 * 2.0, 280.0, true);
        }
        assert_eq!(n.field_count(), MAX_FIELDS);
        assert!(n.steer(&w, 0.5, 200, 2000.0, 2000.0, 100.0, 100.0, true).is_none(), "outside the window");
    }

    #[test]
    fn a_target_inside_a_wall_seeds_from_the_nearest_free_cells() {
        // Covers cells 120..=130 each way, centred on the target's cell 125: the ring of cells round it is seeded.
        let w = World::from_boxes(512.0, 1, vec![Aabb { x0: 240.5, y0: 240.5, x1: 261.5, y1: 261.5 }]);
        let mut n = Nav::default();
        assert_eq!(n.steer(&w, 0.5, 1, 251.0, 151.0, 251.0, 251.0, true), Some((0, 127)));
    }

    #[test]
    fn a_new_world_drops_the_grid() {
        let w = u_world();
        let mut n = Nav::default();
        n.steer(&w, 0.5, 1, 250.0, 175.0, 250.0, 260.0, true).unwrap();
        assert!(n.field_count() == 1 && n.tile_count() > 0);
        let w2 = World::from_boxes(512.0, 1, vec![]);
        // Straight up, now nothing is in the way.
        assert_eq!(n.steer(&w2, 0.5, 2, 251.0, 175.0, 251.0, 260.0, false), None);
        assert_eq!((n.field_count(), n.tile_count()), (0, 0));
        assert_eq!(n.steer(&w2, 0.5, 2, 251.0, 175.0, 251.0, 260.0, true), Some((0, 127)));
    }
}

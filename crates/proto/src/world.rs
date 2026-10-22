//! Static world geometry and the movement step shared by server and client.
//!
//! In the full plan this is baked per chunk from the world editor. The spike
//! generates axis-aligned boxes from a seed so both sides can rebuild the same
//! collision world from just `(size, seed)`. Keeping `step` here, in the crate
//! both sides compile, is what stops client prediction drifting from the server.

use crate::rng::Rng;

/// Wire position resolution: 1/16 unit in a u32, so the wire itself could carry 268 million units.
pub const POS_SCALE: f32 = 16.0;
/// The largest world edge the server will run. Well inside the wire's range, and small enough that f32 positions
/// keep a resolution finer than the wire's (a 1/16 unit step needs the edge under 2^19).
pub const MAX_WORLD_SIZE: u32 = 65_536;
/// Generated walls stop getting more numerous past this, so a huge world is not a huge file for every browser.
pub const MAX_GENERATED_WALLS: usize = 4000;

#[inline]
pub fn qpos(v: f32) -> u32 {
    (v * POS_SCALE).round().clamp(0.0, MAX_WORLD_SIZE as f32 * POS_SCALE) as u32
}

#[inline]
pub fn dpos(q: u32) -> f32 {
    q as f32 / POS_SCALE
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

/// Edge of a building block. Blocks sit on one grid for the whole world, in all three directions: block layer
/// `z` fills heights `z * BLOCK ..= (z + 1) * BLOCK`, so blocks line up however the ground below them runs.
pub const BLOCK: f32 = 2.0;
/// Layers a column spans, upwards from its base (the layer its ground is in).
pub const COLUMN_LAYERS: i16 = 16;
/// The layer of something standing on the bare ground, away from any built column. Anywhere else, a layer is
/// where its feet are: one block up from the ground for something standing on a ground-level block.
pub const GROUND: i8 = i8::MIN;
/// Layers a walker steps up without help. A step up has to leave it room (one layer is a body's height); a
/// taller face is a wall. It drops off any height.
pub const CLIMB: i16 = 1;
/// Block columns per axis the cell key has room for (the largest world is 32768 blocks across).
pub const BLOCK_GRID: u32 = 1 << 15;

/// Edge of a road cell. Roads are a set of these cells, so whether a point is on a road is one lookup, and the
/// same lookup in every client (`ui/src/play/sim.ts`).
pub const ROAD_CELL: f32 = 4.0;
/// How much faster than on open ground something moves on a road, unless the game file says otherwise.
pub const ROAD_SPEED: f32 = 1.5;

/// A road cell's key: its column and row, `ROAD_CELL` units each (a 65,536 unit world is 16,384 across).
#[inline]
pub fn road_key(cx: u32, cy: u32) -> u32 {
    cx | cy << 16
}

/// The base layer of a column whose ground's lowest corner is at height `lowest`: blocks start just below the
/// ground, so a column never floats.
pub fn base_layer(lowest: f32) -> i8 {
    ((lowest / BLOCK).floor() as i32).clamp(-120, (i8::MAX as i16 - COLUMN_LAYERS) as i32) as i8
}

/// A built column: its base layer and which of the `COLUMN_LAYERS` above it hold a block (bit k = layer base + k).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub base: i8,
    pub mask: u16,
}

impl Column {
    #[inline]
    pub fn solid(&self, z: i16) -> bool {
        let k = z - self.base as i16;
        (0..COLUMN_LAYERS).contains(&k) && self.mask & (1 << k) != 0
    }
}

/// A block column's key: its cell, packed.
#[inline]
pub fn block_key(cx: u32, cy: u32) -> u32 {
    cy << 15 | cx
}

#[inline]
pub fn block_cell(key: u32) -> (u32, u32) {
    (key & (BLOCK_GRID - 1), key >> 15)
}

/// The columns under a body: up to 16 without allocating (a body under 3 units across), any number past that.
#[derive(Default)]
struct Under {
    near: [Option<Column>; 16],
    n: usize,
    more: Vec<Column>,
}

impl Under {
    fn push(&mut self, c: Column) {
        if self.n < self.near.len() {
            self.near[self.n] = Some(c);
            self.n += 1;
        } else {
            self.more.push(c);
        }
    }

    fn each(&self) -> impl Iterator<Item = &Column> {
        self.near[..self.n].iter().flatten().chain(&self.more)
    }

    fn floor(&self) -> Option<i16> {
        self.each().map(|c| c.base as i16).max()
    }

    fn solid(&self, z: i16) -> bool {
        self.each().any(|c| c.solid(z))
    }

    /// See `World::settle`.
    fn settle(&self, z: i8) -> i8 {
        let Some(floor) = self.floor() else { return GROUND };
        let mut z = if z == GROUND { floor } else { (z as i16).max(floor) };
        while self.solid(z) && z < i8::MAX as i16 {
            z += 1;
        }
        while z > floor && !self.solid(z - 1) {
            z -= 1;
        }
        z as i8
    }
}

pub struct World {
    pub size: f32,
    pub seed: u64,
    pub boxes: Vec<Aabb>,
    cell: f32,
    gw: usize,
    buckets: Vec<Vec<u32>>,
    /// The block columns players have built (by `block_key`), never with an empty mask.
    blocks: std::collections::HashMap<u32, Column>,
    /// The road cells (by `road_key`), and how much faster movement is on them (the game file's `road_speed`).
    roads: std::collections::HashSet<u32>,
    pub road_speed: f32,
}

impl World {
    pub fn generate(size: u32, seed: u64) -> World {
        assert!((64..=MAX_WORLD_SIZE).contains(&size), "world size must be 64..={MAX_WORLD_SIZE}");
        let sf = size as f32;
        let mut rng = Rng::new(seed ^ 0xA11CE);
        let n = (((sf * sf) / 40_000.0) as usize).min(MAX_GENERATED_WALLS);
        let mut boxes = Vec::with_capacity(n);
        for _ in 0..n {
            let w = rng.range(8.0, 48.0);
            let h = rng.range(8.0, 48.0);
            let x0 = rng.range(0.0, sf - w);
            let y0 = rng.range(0.0, sf - h);
            boxes.push(Aabb { x0, y0, x1: x0 + w, y1: y0 + h });
        }
        Self::from_boxes(sf, seed, boxes)
    }

    pub fn from_boxes(size: f32, seed: u64, boxes: Vec<Aabb>) -> World {
        // 16 unit cells keep a forest's trunks (each a box a unit or so across) to a handful per lookup; a huge world
        // gets coarser ones so the bucket table stays small (at most about a million buckets).
        let cell = (size / 1024.0).max(16.0);
        let gw = (size / cell).ceil() as usize + 1;
        let mut w = World { size, seed, boxes: Vec::new(), cell, gw, buckets: vec![Vec::new(); gw * gw], blocks: Default::default(), roads: Default::default(), road_speed: ROAD_SPEED };
        w.set_boxes(boxes);
        w
    }

    /// Replace the walls, keeping the built blocks. Costs what the old and new walls touch, not the size of the
    /// world, so a client streaming tiles can call it whenever its window moves.
    pub fn set_boxes(&mut self, boxes: Vec<Aabb>) {
        let old = std::mem::replace(&mut self.boxes, boxes);
        for b in &old {
            self.cells(b, |k, buckets| buckets[k].clear());
        }
        for i in 0..self.boxes.len() {
            let b = self.boxes[i];
            self.cells(&b, |k, buckets| buckets[k].push(i as u32));
        }
    }

    fn cells(&mut self, b: &Aabb, mut f: impl FnMut(usize, &mut Vec<Vec<u32>>)) {
        let (cell, gw) = (self.cell, self.gw);
        let (cx0, cx1) = ((b.x0.max(0.0) / cell) as usize, ((b.x1.max(0.0) / cell) as usize).min(gw - 1));
        let (cy0, cy1) = ((b.y0.max(0.0) / cell) as usize, ((b.y1.max(0.0) / cell) as usize).min(gw - 1));
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                f(cy * gw + cx, &mut self.buckets);
            }
        }
    }

    /// Set a block column (an empty mask clears it). The server does this as players build; clients mirror it
    /// for prediction.
    pub fn set_column(&mut self, cx: u32, cy: u32, col: Column) {
        if cx >= BLOCK_GRID || cy >= BLOCK_GRID {
            return;
        }
        if col.mask != 0 {
            self.blocks.insert(block_key(cx, cy), col);
        } else {
            self.blocks.remove(&block_key(cx, cy));
        }
    }

    /// Replace the road cells (keys from `road_key`).
    pub fn set_roads(&mut self, cells: impl IntoIterator<Item = u32>) {
        self.roads = cells.into_iter().collect();
    }

    pub fn road_count(&self) -> usize {
        self.roads.len()
    }

    /// True if the point is on a road cell.
    #[inline]
    pub fn on_road(&self, x: f32, y: f32) -> bool {
        // ROAD_CELL is a power of two, so the division is exact and the browser's floor finds the same cell.
        !self.roads.is_empty() && x >= 0.0 && y >= 0.0 && self.roads.contains(&road_key((x / ROAD_CELL) as u32, (y / ROAD_CELL) as u32))
    }

    pub fn column(&self, cx: u32, cy: u32) -> Option<Column> {
        self.blocks.get(&block_key(cx, cy)).copied()
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// The built columns a circle (by its bounding box) of radius `r` at `(x, y)` overlaps, looked up once so a
    /// step can ask several things of them.
    fn under(&self, x: f32, y: f32, r: f32) -> Under {
        let mut u = Under::default();
        if self.blocks.is_empty() {
            return u;
        }
        let last = (BLOCK_GRID - 1) as f32;
        let (cx0, cx1) = (((x - r) / BLOCK).floor().max(0.0) as u32, ((x + r) / BLOCK).floor().min(last) as u32);
        let (cy0, cy1) = (((y - r) / BLOCK).floor().max(0.0) as u32, ((y + r) / BLOCK).floor().min(last) as u32);
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                if let Some(c) = self.blocks.get(&block_key(cx, cy)) {
                    let (bx, by) = (cx as f32 * BLOCK, cy as f32 * BLOCK);
                    if x + r > bx && x - r < bx + BLOCK && y + r > by && y - r < by + BLOCK {
                        u.push(*c);
                    }
                }
            }
        }
        u
    }

    /// The highest base among the columns under a circle: the ground there, as far as blocks are concerned.
    /// None away from every column, where only the bare ground counts.
    pub fn floor(&self, x: f32, y: f32, r: f32) -> Option<i16> {
        self.under(x, y, r).floor()
    }

    /// True if any column under a circle has a block in layer `z`.
    pub fn solid_at(&self, x: f32, y: f32, r: f32, z: i16) -> bool {
        self.under(x, y, r).solid(z)
    }

    /// Where something with its feet in layer `z` ends up standing at `(x, y)` without moving sideways: pushed up
    /// out of any block it is inside, then dropped onto whatever is below it.
    pub fn settle(&self, x: f32, y: f32, r: f32, z: i8) -> i8 {
        self.under(x, y, r).settle(z)
    }

    /// Where something with its feet in layer `z` stands if it moves to `(x, y)`, or None if it cannot: a wall,
    /// the world's edge, or a block face taller than it climbs.
    pub fn enter(&self, x: f32, y: f32, r: f32, z: i8) -> Option<i8> {
        if self.blocked_by_walls(x, y, r) {
            return None;
        }
        let u = self.under(x, y, r);
        let Some(floor) = u.floor() else { return Some(GROUND) };
        let z = if z == GROUND { floor } else { (z as i16).max(floor) };
        if u.solid(z) {
            return (1..=CLIMB).find(|&up| !u.solid(z + up)).map(|up| u.settle((z + up) as i8));
        }
        Some(u.settle(z as i8))
    }

    /// True if a circle (approximated by its bounding box) of radius `r` at `(x, y)` leaves the world, overlaps
    /// any static box or overlaps any built column. For finding free spots; walking uses `enter`.
    pub fn blocked(&self, x: f32, y: f32, r: f32) -> bool {
        self.floor(x, y, r).is_some() || self.blocked_by_walls(x, y, r)
    }

    /// Like `blocked` but built blocks do not count (something may be standing on them).
    pub fn blocked_by_walls(&self, x: f32, y: f32, r: f32) -> bool {
        if x - r < 0.0 || y - r < 0.0 || x + r >= self.size || y + r >= self.size {
            return true;
        }
        let cx0 = ((x - r) / self.cell) as usize;
        let cx1 = (((x + r) / self.cell) as usize).min(self.gw - 1);
        let cy0 = ((y - r) / self.cell) as usize;
        let cy1 = (((y + r) / self.cell) as usize).min(self.gw - 1);
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                for &bi in &self.buckets[cy * self.gw + cx] {
                    let b = &self.boxes[bi as usize];
                    if x + r > b.x0 && x - r < b.x1 && y + r > b.y0 && y - r < b.y1 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Axis-separated move so entities slide along walls instead of sticking, climbing and dropping as they go.
    /// `z` is the layer the feet are in (`GROUND` on the bare ground).
    #[inline]
    pub fn step(&self, x: f32, y: f32, z: i8, dx: f32, dy: f32, r: f32) -> (f32, f32, i8) {
        let (x1, z1) = match self.enter(x + dx, y, r, z) {
            Some(z1) => (x + dx, z1),
            None => (x, z),
        };
        let (y1, z2) = match self.enter(x1, y + dy, r, z1) {
            Some(z2) => (y + dy, z2),
            None => (y, z1),
        };
        (x1, y1, z2)
    }

    /// Find a free spot. Falls back to the centre if the world is pathological.
    pub fn random_spawn(&self, rng: &mut Rng, r: f32) -> (f32, f32) {
        for _ in 0..128 {
            let x = rng.range(r + 1.0, self.size - r - 1.0);
            let y = rng.range(r + 1.0, self.size - r - 1.0);
            if !self.blocked(x, y, r) {
                return (x, y);
            }
        }
        (self.size * 0.5, self.size * 0.5)
    }
}

/// Wire direction (-127..=127 per axis) to a movement vector with magnitude
/// at most 1. The client can only ever ask for *direction*; speed is the
/// server's business.
#[inline]
pub fn move_vector(dir_x: i8, dir_y: i8) -> (f32, f32) {
    let fx = (dir_x as f32 / 127.0).clamp(-1.0, 1.0);
    let fy = (dir_y as f32 / 127.0).clamp(-1.0, 1.0);
    let len2 = fx * fx + fy * fy;
    if len2 > 1.0 {
        let inv = 1.0 / len2.sqrt();
        (fx * inv, fy * inv)
    } else {
        (fx, fy)
    }
}

/// One movement tick, identical on server and (for prediction) client. Standing still still settles: a block
/// broken underfoot drops you. Starting the tick on a road makes it `world.road_speed` times as long.
#[inline]
#[allow(clippy::too_many_arguments)]
pub fn integrate(
    world: &World,
    x: f32,
    y: f32,
    z: i8,
    dir_x: i8,
    dir_y: i8,
    speed: f32,
    dt: f32,
    r: f32,
) -> (f32, f32, i8) {
    let (mx, my) = move_vector(dir_x, dir_y);
    if mx == 0.0 && my == 0.0 {
        return (x, y, if z == GROUND { z } else { world.settle(x, y, r, z) });
    }
    let speed = if world.on_road(x, y) { speed * world.road_speed } else { speed };
    world.step(x, y, z, mx * speed * dt, my * speed * dt, r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swapping_the_walls_keeps_the_blocks_and_forgets_the_old_walls() {
        let mut w = wall_world();
        w.set_column(10, 10, Column { base: 0, mask: 1 });
        w.set_boxes(vec![Aabb { x0: 10.0, y0: 10.0, x1: 12.0, y1: 12.0 }]);
        assert!(!w.blocked_by_walls(105.0, 100.0, 0.5), "the old wall is gone");
        assert!(w.blocked_by_walls(11.0, 11.0, 0.5));
        assert_eq!(w.column(10, 10), Some(Column { base: 0, mask: 1 }));
        w.set_boxes(Vec::new());
        assert!(!w.blocked_by_walls(11.0, 11.0, 0.5));
    }

    fn wall_world() -> World {
        World::from_boxes(
            256.0,
            1,
            vec![Aabb { x0: 100.0, y0: 0.0, x1: 110.0, y1: 200.0 }],
        )
    }

    #[test]
    fn cannot_walk_through_walls_or_out_of_bounds() {
        let w = wall_world();
        let (mut x, mut y) = (95.0, 50.0);
        for _ in 0..200 {
            let r = integrate(&w, x, y, GROUND, 127, 0, 20.0, 0.05, 0.5);
            x = r.0;
            y = r.1;
        }
        assert!(x < 100.0, "tunnelled through wall: x={x}");
        let (mut x, mut y) = (5.0, 5.0);
        for _ in 0..400 {
            let r = integrate(&w, x, y, GROUND, -127, -127, 20.0, 0.05, 0.5);
            x = r.0;
            y = r.1;
        }
        assert!(x >= 0.5 && y >= 0.5);
    }

    #[test]
    fn slides_along_walls() {
        let w = wall_world();
        let (mut x, mut y) = (99.0, 50.0);
        for _ in 0..40 {
            let r = integrate(&w, x, y, GROUND, 127, 127, 20.0, 0.05, 0.5);
            x = r.0;
            y = r.1;
        }
        assert!(y > 60.0, "should slide down the wall, y={y}");
        assert!(x < 100.0);
    }

    #[test]
    fn speed_is_server_controlled_and_diagonals_not_faster() {
        let w = World::from_boxes(512.0, 1, vec![]);
        let (x, y, _) = integrate(&w, 100.0, 100.0, GROUND, 127, 0, 20.0, 1.0, 0.5);
        assert!((x - 120.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3);
        let (x, y, _) = integrate(&w, 100.0, 100.0, GROUND, 127, 127, 20.0, 1.0, 0.5);
        let d = ((x - 100.0).powi(2) + (y - 100.0).powi(2)).sqrt();
        assert!((d - 20.0).abs() < 1e-3, "diagonal speed {d}");
    }

    /// The same cases are in ui/src/play/sim.test.ts.
    #[test]
    fn a_road_is_faster_from_the_cell_a_tick_starts_in() {
        let mut w = World::from_boxes(512.0, 1, vec![]);
        w.set_roads([road_key(25, 25), road_key(26, 25)]); // x 100..108, y 100..104
        assert!(w.on_road(100.0, 100.0) && w.on_road(107.9, 103.9) && !w.on_road(108.0, 100.0) && !w.on_road(99.9, 101.0));
        let (x, _, _) = integrate(&w, 101.0, 102.0, GROUND, 127, 0, 20.0, 0.05, 0.5);
        assert_eq!(x, 101.0 + 20.0 * ROAD_SPEED * 0.05);
        let (x, _, _) = integrate(&w, 99.0, 102.0, GROUND, 127, 0, 20.0, 0.05, 0.5);
        assert_eq!(x, 100.0, "off the road at the start of the tick: the usual speed");
        w.road_speed = 1.0;
        let (x, _, _) = integrate(&w, 101.0, 102.0, GROUND, 127, 0, 20.0, 0.05, 0.5);
        assert_eq!(x, 102.0);
        assert!(!w.on_road(-1.0, 100.0), "nothing below zero is a road");
    }

    #[test]
    fn same_seed_same_world() {
        let a = World::generate(1024, 42);
        let b = World::generate(1024, 42);
        assert_eq!(a.boxes, b.boxes);
        let c = World::generate(1024, 43);
        assert_ne!(a.boxes, c.boxes);
    }

    #[test]
    fn spawns_are_free() {
        let w = World::generate(1024, 7);
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let (x, y) = w.random_spawn(&mut rng, 0.5);
            assert!(!w.blocked(x, y, 0.5));
        }
    }

    /// A column on flat ground (base 0) with blocks in these layers.
    fn col(layers: &[i16]) -> Column {
        Column { base: 0, mask: layers.iter().fold(0, |m, &z| m | 1 << z) }
    }

    /// Walk east along row 25 (y 50..52) from x 95 for `ticks`, returning where you end up and the highest layer
    /// your feet reached.
    fn walk_east(w: &World, ticks: usize) -> (f32, i8, i8) {
        let (mut x, mut z, mut top) = (95.0, GROUND, GROUND);
        for _ in 0..ticks {
            (x, _, z) = integrate(w, x, 51.0, z, 127, 0, 20.0, 0.05, 0.5);
            top = top.max(z);
        }
        (x, z, top)
    }

    #[test]
    fn built_blocks_stop_movement_like_walls_and_clearing_them_frees_the_way() {
        let mut w = World::from_boxes(256.0, 1, vec![]);
        // A wall two blocks high across x = 100..102: too tall to step onto.
        for cy in 0..128 {
            w.set_column(50, cy, col(&[0, 1]));
        }
        assert!(w.column(50, 3).unwrap().solid(1) && w.column(51, 3).is_none());
        assert!(walk_east(&w, 200).0 < 100.0, "walked through blocks");
        // Exactly touching a block's edge is not blocked, as with boxes.
        assert!(w.enter(99.5, 51.0, 0.5, GROUND).is_some() && w.enter(99.51, 51.0, 0.5, GROUND).is_none());
        w.set_column(50, 25, col(&[]));
        assert!(walk_east(&w, 200).0 > 102.0, "the gap should let you through");
        w.set_column(BLOCK_GRID, 0, col(&[0])); // out of range: ignored
        assert_eq!(w.block_count(), 127);
        assert_eq!(block_cell(block_key(32767, 12345)), (32767, 12345));
    }

    #[test]
    fn walkers_climb_a_staircase_one_block_at_a_time_and_drop_off_the_far_side() {
        let mut w = World::from_boxes(256.0, 1, vec![]);
        // Steps 1, 2, 3 high along row 25, then a 3-high landing, then nothing.
        for (cx, h) in [(50, 1), (51, 2), (52, 3), (53, 3)] {
            w.set_column(cx, 25, col(&(0..h).collect::<Vec<_>>()));
        }
        let (x, z, top) = walk_east(&w, 200);
        assert_eq!(top, 3, "climbed to the top");
        assert!(x > 108.0 && z == GROUND, "walked off the far end and dropped to the ground: {x} {z}");
        // From the ground a 2-high column is a wall; from the 1-high step next to it, it is a step.
        assert_eq!(w.enter(102.0, 51.0, 0.5, GROUND), None);
        assert_eq!(w.enter(102.0, 51.0, 0.5, 1), Some(2));
        assert!(w.blocked(101.0, 51.0, 0.5) && !w.blocked_by_walls(101.0, 51.0, 0.5));
    }

    #[test]
    fn a_bridge_is_walked_under_from_the_ground_and_over_from_above() {
        let mut w = World::from_boxes(256.0, 1, vec![]);
        // Towers 3 high at x 100..102 and 106..108, a bridge in layer 2 between them (x 102..106), nothing under it.
        w.set_column(50, 25, col(&[0, 1, 2]));
        w.set_column(53, 25, col(&[0, 1, 2]));
        w.set_column(51, 25, col(&[2]));
        w.set_column(52, 25, col(&[2]));
        // Under it: the gap below is two layers, room to stand.
        assert_eq!(w.enter(104.0, 51.0, 0.5, GROUND), Some(0));
        // On it: from the top of a tower (feet in layer 3) you walk across at layer 3, no drop.
        assert_eq!(w.enter(104.0, 51.0, 0.5, 3), Some(3));
        let (mut x, mut z) = (101.0, 3);
        for _ in 0..6 {
            (x, _, z) = integrate(&w, x, 51.0, z, 127, 0, 20.0, 0.05, 0.5);
            assert_eq!(z, 3, "kept to the top of the bridge at x {x}");
        }
        // A block broken out from under you drops you on the next tick, standing still.
        w.set_column(51, 25, col(&[]));
        w.set_column(52, 25, col(&[]));
        assert_eq!(integrate(&w, 104.0, 51.0, 3, 0, 0, 20.0, 0.05, 0.5).2, GROUND);
    }

    #[test]
    fn a_window_is_a_gap_in_a_wall_and_a_low_doorway_lets_you_through() {
        let mut w = World::from_boxes(256.0, 1, vec![]);
        // A wall at x 100..102: blocks in layers 1 and 2 over a gap at layer 0, a doorway one block high.
        for cy in 0..128 {
            w.set_column(50, cy, col(&[1, 2, 3]));
        }
        assert!(walk_east(&w, 200).0 > 102.0, "walked through the doorway");
        // Fill the doorway's floor: now the face is solid at 0 and 1, too tall to climb.
        for cy in 0..128 {
            w.set_column(50, cy, col(&[0, 1, 2, 3]));
        }
        assert!(walk_east(&w, 200).0 < 100.0);
        // A window: a gap at layer 2 over a 1-high sill. You step onto the sill and into the window.
        w.set_column(50, 25, col(&[0, 3]));
        assert_eq!(w.enter(99.6, 51.0, 0.5, GROUND), Some(1));
    }

    #[test]
    fn columns_on_uneven_ground_share_one_grid_and_are_levelled_by_layer() {
        let mut w = World::from_boxes(256.0, 1, vec![]);
        // Ground lower at x 100 (base 2) than at x 102 (base 3); both columns reach layer 4.
        assert_eq!((base_layer(5.9), base_layer(6.0), base_layer(-0.1)), (2, 3, -1));
        w.set_column(50, 25, Column { base: 2, mask: 0b111 });
        w.set_column(51, 25, Column { base: 3, mask: 0b11 });
        assert!(w.column(50, 25).unwrap().solid(4) && w.column(51, 25).unwrap().solid(4));
        // Standing on the low column's top (feet in 5), you walk straight across onto the other at the same level.
        assert_eq!(w.enter(103.0, 51.0, 0.5, 5), Some(5));
        // Coming off the bare ground into the high column, you meet its base block and step up onto it.
        assert_eq!(w.enter(103.0, 51.0, 0.5, GROUND), None, "two layers above its base: a wall");
    }

    #[test]
    fn position_quantisation_roundtrip() {
        for v in [0.0f32, 0.0625, 1.0, 1234.5, 4095.0] {
            assert!((dpos(qpos(v)) - v).abs() <= 1.0 / 32.0 + 1e-4);
        }
    }
}

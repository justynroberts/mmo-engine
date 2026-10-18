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

/// Edge of a building block: players build on a grid of these, and a column with any block in it is solid.
pub const BLOCK: f32 = 2.0;
/// Block columns per axis the cell key has room for (the largest world is 32768 blocks across).
pub const BLOCK_GRID: u32 = 1 << 15;

/// A block column's key: its cell, packed.
#[inline]
pub fn block_key(cx: u32, cy: u32) -> u32 {
    cy << 15 | cx
}

#[inline]
pub fn block_cell(key: u32) -> (u32, u32) {
    (key & (BLOCK_GRID - 1), key >> 15)
}

pub struct World {
    pub size: f32,
    pub seed: u64,
    pub boxes: Vec<Aabb>,
    cell: f32,
    gw: usize,
    buckets: Vec<Vec<u32>>,
    /// Block columns that players have built (by `block_key`). They block movement like the boxes do.
    blocks: std::collections::HashSet<u32>,
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
        // Finer cells than 64 units buy nothing; a huge world gets coarser ones so the bucket table stays small.
        let cell = (size / 1024.0).max(64.0);
        let gw = (size / cell).ceil() as usize + 1;
        let mut buckets = vec![Vec::new(); gw * gw];
        for (i, b) in boxes.iter().enumerate() {
            let cx0 = (b.x0 / cell) as usize;
            let cx1 = (b.x1 / cell) as usize;
            let cy0 = (b.y0 / cell) as usize;
            let cy1 = (b.y1 / cell) as usize;
            for cy in cy0..=cy1.min(gw - 1) {
                for cx in cx0..=cx1.min(gw - 1) {
                    buckets[cy * gw + cx].push(i as u32);
                }
            }
        }
        World { size, seed, boxes, cell, gw, buckets, blocks: Default::default() }
    }

    /// Make a block column solid or clear. The server does this as players build; clients mirror it for prediction.
    pub fn set_block(&mut self, cx: u32, cy: u32, solid: bool) {
        if cx >= BLOCK_GRID || cy >= BLOCK_GRID {
            return;
        }
        if solid {
            self.blocks.insert(block_key(cx, cy));
        } else {
            self.blocks.remove(&block_key(cx, cy));
        }
    }

    pub fn has_block(&self, cx: u32, cy: u32) -> bool {
        self.blocks.contains(&block_key(cx, cy))
    }

    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// True if a circle (by its bounding box) of radius `r` at `(x, y)` overlaps a built block.
    fn blocked_by_blocks(&self, x: f32, y: f32, r: f32) -> bool {
        if self.blocks.is_empty() {
            return false;
        }
        let (cx0, cx1) = (((x - r) / BLOCK).floor().max(0.0) as u32, ((x + r) / BLOCK).floor() as u32);
        let (cy0, cy1) = (((y - r) / BLOCK).floor().max(0.0) as u32, ((y + r) / BLOCK).floor() as u32);
        for cy in cy0..=cy1 {
            for cx in cx0..=cx1 {
                if self.has_block(cx, cy) {
                    let (bx, by) = (cx as f32 * BLOCK, cy as f32 * BLOCK);
                    if x + r > bx && x - r < bx + BLOCK && y + r > by && y - r < by + BLOCK {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// True if a circle (approximated by its bounding box) of radius `r` at
    /// `(x, y)` leaves the world or overlaps any static box.
    pub fn blocked(&self, x: f32, y: f32, r: f32) -> bool {
        if x - r < 0.0 || y - r < 0.0 || x + r >= self.size || y + r >= self.size {
            return true;
        }
        if self.blocked_by_blocks(x, y, r) {
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

    /// Axis-separated move so entities slide along walls instead of sticking.
    #[inline]
    pub fn step(&self, x: f32, y: f32, dx: f32, dy: f32, r: f32) -> (f32, f32) {
        let nx = x + dx;
        let x1 = if self.blocked(nx, y, r) { x } else { nx };
        let ny = y + dy;
        let y1 = if self.blocked(x1, ny, r) { y } else { ny };
        (x1, y1)
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

/// One movement tick, identical on server and (for prediction) client.
#[inline]
pub fn integrate(
    world: &World,
    x: f32,
    y: f32,
    dir_x: i8,
    dir_y: i8,
    speed: f32,
    dt: f32,
    r: f32,
) -> (f32, f32) {
    let (mx, my) = move_vector(dir_x, dir_y);
    if mx == 0.0 && my == 0.0 {
        return (x, y);
    }
    world.step(x, y, mx * speed * dt, my * speed * dt, r)
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let r = integrate(&w, x, y, 127, 0, 20.0, 0.05, 0.5);
            x = r.0;
            y = r.1;
        }
        assert!(x < 100.0, "tunnelled through wall: x={x}");
        let (mut x, mut y) = (5.0, 5.0);
        for _ in 0..400 {
            let r = integrate(&w, x, y, -127, -127, 20.0, 0.05, 0.5);
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
            let r = integrate(&w, x, y, 127, 127, 20.0, 0.05, 0.5);
            x = r.0;
            y = r.1;
        }
        assert!(y > 60.0, "should slide down the wall, y={y}");
        assert!(x < 100.0);
    }

    #[test]
    fn speed_is_server_controlled_and_diagonals_not_faster() {
        let w = World::from_boxes(512.0, 1, vec![]);
        let (x, y) = integrate(&w, 100.0, 100.0, 127, 0, 20.0, 1.0, 0.5);
        assert!((x - 120.0).abs() < 1e-3 && (y - 100.0).abs() < 1e-3);
        let (x, y) = integrate(&w, 100.0, 100.0, 127, 127, 20.0, 1.0, 0.5);
        let d = ((x - 100.0).powi(2) + (y - 100.0).powi(2)).sqrt();
        assert!((d - 20.0).abs() < 1e-3, "diagonal speed {d}");
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

    #[test]
    fn built_blocks_stop_movement_like_walls_and_clearing_them_frees_the_way() {
        let mut w = World::from_boxes(256.0, 1, vec![]);
        // A line of blocks across x = 100..102.
        for cy in 0..128 {
            w.set_block(50, cy, true);
        }
        assert!(w.has_block(50, 3) && !w.has_block(51, 3));
        // Along the middle of row 25 (y 50..52), where the gap will be.
        let walk = |w: &World| {
            let (mut x, mut y) = (95.0, 51.0);
            for _ in 0..200 {
                (x, y) = integrate(w, x, y, 127, 0, 20.0, 0.05, 0.5);
            }
            x
        };
        assert!(walk(&w) < 100.0, "walked through blocks");
        // Exactly touching a block's edge is not blocked, as with boxes.
        assert!(!w.blocked(99.5, 50.0, 0.5) && w.blocked(99.51, 50.0, 0.5));
        w.set_block(50, 25, false);
        assert!(walk(&w) > 102.0, "the gap should let you through");
        w.set_block(BLOCK_GRID, 0, true); // out of range: ignored
        assert_eq!(w.block_count(), 127);
        assert_eq!(block_cell(block_key(32767, 12345)), (32767, 12345));
    }

    #[test]
    fn position_quantisation_roundtrip() {
        for v in [0.0f32, 0.0625, 1.0, 1234.5, 4095.0] {
            assert!((dpos(qpos(v)) - v).abs() <= 1.0 / 32.0 + 1e-4);
        }
    }
}

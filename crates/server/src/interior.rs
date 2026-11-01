//! Dungeon interiors: a dungeon with an `interior` (`proto::realm::InteriorFile`) has rooms and corridors laid out
//! inside the interior's place, in a row from west to east: a small first room with the way out by its west wall,
//! rooms between of differing sizes and heights, and the boss's large room last, each joined to the next by a
//! corridor. The layout is worked out from the place and the seed alone, so it is the same every run; its walls are
//! added to the world's when the server starts (as scenery, so clients draw them from the layout in `/play/info`,
//! not as blocks), after cutting its footprint out of whatever was solid there (an interior is best put out at
//! sea, which a world file makes solid: its own walls close it in, and the solid sea round the cut seals the rest).
//! A moved interior takes a restart to move its walls.

use proto::game::Place;
use proto::world::Aabb;

/// Wall thickness, corridor width and length, world units.
pub const WALL: f32 = 2.0;
pub const CORRIDOR: f32 = 8.0;
const GAP: f32 = 14.0;

#[derive(Clone, Debug, PartialEq)]
pub struct Layout {
    /// Floors: the first room is where the party comes in, the last the boss's.
    pub rooms: Vec<Aabb>,
    pub corridors: Vec<Aabb>,
    pub walls: Vec<Aabb>,
    /// Where the party is put on the way in, and where standing takes you back out.
    pub entry: (f32, f32),
    pub exit: (f32, f32),
    /// Everything, walls included.
    pub bounds: Aabb,
}

impl Layout {
    pub fn inside(&self, x: f32, y: f32) -> bool {
        x > self.bounds.x0 && x < self.bounds.x1 && y > self.bounds.y0 && y < self.bounds.y1
    }

    /// The middle of the boss's room.
    pub fn boss_at(&self) -> (f32, f32) {
        let b = self.rooms[self.rooms.len() - 1];
        ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0)
    }

    /// A spot for the `k`th lesser foe: dealt round the rooms between the first and the boss's (into the boss's too
    /// when there are only two), a few units in from the walls.
    pub fn foe_at(&self, k: usize) -> (f32, f32) {
        let pool: &[Aabb] = if self.rooms.len() > 2 { &self.rooms[1..self.rooms.len() - 1] } else { &self.rooms[1..] };
        let room = pool[k % pool.len()];
        let i = (k / pool.len()) as f32;
        let (w, h) = (room.x1 - room.x0 - 12.0, room.y1 - room.y0 - 12.0);
        (room.x0 + 6.0 + w * (0.37 + i * 0.618).fract(), room.y0 + 6.0 + h * (0.5 + i * 0.381).fract())
    }
}

/// A small, fixed random stream from the seed (the layout must never change with the server's own random draws).
struct Stream(u64);

impl Stream {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn between(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

/// The layout of `rooms` rooms centred on `place`.
pub fn layout(place: &Place, rooms: u8, seed: u32) -> Layout {
    let n = rooms.clamp(2, 8) as usize;
    let mut rng = Stream(0x9e37_79b9_7f4a_7c15 ^ (seed as u64).wrapping_mul(0x1000_0000_01b3) ^ place.name.bytes().fold(0u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)) | 1);
    // Sizes first, so the row can be centred on the place.
    let size: Vec<(f32, f32, f32)> = (0..n)
        .map(|i| {
            if i == 0 {
                (36.0, 36.0, 0.0)
            } else if i == n - 1 {
                (64.0, 64.0, 0.0)
            } else {
                (rng.between(40.0, 56.0).round(), rng.between(40.0, 56.0).round(), rng.between(-10.0, 10.0).round())
            }
        })
        .collect();
    let width: f32 = size.iter().map(|s| s.0).sum::<f32>() + GAP * (n - 1) as f32;
    let mut x = (place.x - width / 2.0).round();
    let mut floors = Vec::new();
    for &(w, h, dy) in &size {
        let cy = place.y + dy;
        floors.push(Aabb { x0: x, y0: (cy - h / 2.0).round(), x1: x + w, y1: (cy + h / 2.0).round() });
        x += w + GAP;
    }
    let mut corridors = Vec::new();
    let mut walls = Vec::new();
    // The door in each room's west and east walls: (y of its middle) or none.
    let mut west: Vec<Option<f32>> = vec![None; n];
    let mut east: Vec<Option<f32>> = vec![None; n];
    for i in 0..n - 1 {
        let (a, b) = (floors[i], floors[i + 1]);
        let lo = a.y0.max(b.y0) + CORRIDOR / 2.0 + 1.0;
        let hi = a.y1.min(b.y1) - CORRIDOR / 2.0 - 1.0;
        let c = ((lo + hi) / 2.0).round();
        corridors.push(Aabb { x0: a.x1, y0: c - CORRIDOR / 2.0, x1: b.x0, y1: c + CORRIDOR / 2.0 });
        walls.push(Aabb { x0: a.x1 + WALL, y0: c - CORRIDOR / 2.0 - WALL, x1: b.x0 - WALL, y1: c - CORRIDOR / 2.0 });
        walls.push(Aabb { x0: a.x1 + WALL, y0: c + CORRIDOR / 2.0, x1: b.x0 - WALL, y1: c + CORRIDOR / 2.0 + WALL });
        east[i] = Some(c);
        west[i + 1] = Some(c);
    }
    for (i, f) in floors.iter().enumerate() {
        walls.push(Aabb { x0: f.x0 - WALL, y0: f.y0 - WALL, x1: f.x1 + WALL, y1: f.y0 });
        walls.push(Aabb { x0: f.x0 - WALL, y0: f.y1, x1: f.x1 + WALL, y1: f.y1 + WALL });
        for (x0, door) in [(f.x0 - WALL, west[i]), (f.x1, east[i])] {
            match door {
                None => walls.push(Aabb { x0, y0: f.y0, x1: x0 + WALL, y1: f.y1 }),
                Some(c) => {
                    walls.push(Aabb { x0, y0: f.y0, x1: x0 + WALL, y1: c - CORRIDOR / 2.0 });
                    walls.push(Aabb { x0, y0: c + CORRIDOR / 2.0, x1: x0 + WALL, y1: f.y1 });
                }
            }
        }
    }
    let first = floors[0];
    let mid = (first.y0 + first.y1) / 2.0;
    let bounds = walls.iter().fold(Aabb { x0: f32::MAX, y0: f32::MAX, x1: f32::MIN, y1: f32::MIN }, |b, w| Aabb { x0: b.x0.min(w.x0), y0: b.y0.min(w.y0), x1: b.x1.max(w.x1), y1: b.y1.max(w.y1) });
    Layout { rooms: floors, corridors, walls, entry: ((first.x0 + first.x1) / 2.0 + 4.0, mid), exit: (first.x0 + 5.0, mid), bounds }
}

/// The layout of dungeon `d` of a game, if it has an interior.
pub fn of(places: &[Place], d: &proto::realm::DungeonRt) -> Option<Layout> {
    let i = d.interior.as_ref()?;
    Some(layout(places.get(i.place)?, i.rooms, i.seed))
}

/// Every interior's layout.
pub fn all(places: &[Place], dungeons: &[proto::realm::DungeonRt]) -> Vec<Layout> {
    dungeons.iter().filter_map(|d| of(places, d)).collect()
}

/// A world's walls (with their tile classes) with these interiors in them: each one's footprint cut out of every box
/// it overlaps (a box less a rectangle is up to four boxes), then its walls added as scenery.
pub fn apply(boxes: &[Aabb], classes: &[u8], layouts: &[Layout]) -> (Vec<Aabb>, Vec<u8>) {
    let mut out: Vec<(Aabb, u8)> = boxes.iter().copied().zip(classes.iter().copied().chain(std::iter::repeat(proto::tiles::class::WALL))).collect();
    for l in layouts {
        let h = l.bounds;
        let mut next = Vec::with_capacity(out.len());
        for (b, c) in out {
            if b.x1 <= h.x0 || b.x0 >= h.x1 || b.y1 <= h.y0 || b.y0 >= h.y1 {
                next.push((b, c));
                continue;
            }
            let pieces = [
                Aabb { x0: b.x0, y0: b.y0, x1: b.x1, y1: h.y0 },
                Aabb { x0: b.x0, y0: h.y1, x1: b.x1, y1: b.y1 },
                Aabb { x0: b.x0, y0: h.y0.max(b.y0), x1: h.x0, y1: h.y1.min(b.y1) },
                Aabb { x0: h.x1, y0: h.y0.max(b.y0), x1: b.x1, y1: h.y1.min(b.y1) },
            ];
            next.extend(pieces.into_iter().filter(|p| p.x1 > p.x0 && p.y1 > p.y0).map(|p| (p, c)));
        }
        out = next;
        out.extend(l.walls.iter().map(|&w| (w, proto::tiles::class::SCENERY)));
    }
    out.into_iter().unzip()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place() -> Place {
        Place { name: "deep".into(), x: 300.0, y: 300.0, r: 200.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false, hold: None, dock: false }
    }

    #[test]
    fn rooms_run_west_to_east_joined_by_open_corridors_closed_in_by_walls() {
        let l = layout(&place(), 5, 7);
        assert_eq!(l, layout(&place(), 5, 7), "the same every time");
        assert_eq!((l.rooms.len(), l.corridors.len()), (5, 4));
        let solid = |x: f32, y: f32| l.walls.iter().any(|w| x > w.x0 && x < w.x1 && y > w.y0 && y < w.y1);
        // Every floor is open, and each corridor leads from one room into the next.
        for f in l.rooms.iter().chain(&l.corridors) {
            assert!(!solid((f.x0 + f.x1) / 2.0, (f.y0 + f.y1) / 2.0), "{f:?}");
        }
        for (k, c) in l.corridors.iter().enumerate() {
            let y = (c.y0 + c.y1) / 2.0;
            for x in (c.x0 as i32 - 3)..=(c.x1 as i32 + 3) {
                assert!(!solid(x as f32, y), "corridor {k} blocked at {x}");
            }
        }
        // Closed in: walking out of any room in any direction but a door meets a wall before the bounds.
        let (bx, by) = l.boss_at();
        for (dx, dy) in [(0.0, -1.0), (0.0, 1.0), (1.0, 0.0)] {
            let hit = (1..200).any(|t| solid(bx + dx * t as f32, by + dy * t as f32));
            assert!(hit, "the boss room is open to the {dx},{dy}");
        }
        assert!(l.inside(l.entry.0, l.entry.1) && l.inside(l.exit.0, l.exit.1) && !solid(l.exit.0, l.exit.1));
        assert!(l.bounds.x1 - l.bounds.x0 < 2.0 * place().r, "fits its place");
        // Put out at sea (solid), its footprint is cut out of the sea and its walls go in.
        let sea = [Aabb { x0: 0.0, y0: 0.0, x1: 1024.0, y1: 1024.0 }];
        let (cut, classes) = apply(&sea, &[proto::tiles::class::SCENERY], &[l.clone()]);
        let solid_now = |x: f32, y: f32| cut.iter().any(|w| x > w.x0 && x < w.x1 && y > w.y0 && y < w.y1);
        assert!(!solid_now(l.entry.0, l.entry.1) && !solid_now(bx, by), "the rooms are open");
        assert!(solid_now(l.bounds.x0 - 1.0, by) && solid_now(bx, l.bounds.y1 + 1.0), "the sea round them is not");
        assert_eq!(cut.len(), classes.len());
        for k in 0..6 {
            let (x, y) = l.foe_at(k);
            assert!(l.rooms[1..].iter().any(|r| x > r.x0 && x < r.x1 && y > r.y0 && y < r.y1), "foe {k} at {x},{y} is in a room");
        }
    }
}

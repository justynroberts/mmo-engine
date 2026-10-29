//! Where townsfolk live. A bot type has a home place and maybe more (`Archetype::homes`), shared out over its bots
//! one each in turn, so one type can people every town. Inside its home a bot has a house of its own, one of the
//! world file's houses (`Terrain::houses`): it goes in at that door at night (`Act::Indoors`), so it comes out of it
//! in the morning, and in the day it calls now and then at a neighbour's door (`Sim::wander_dir`).
//!
//! A bot type with a trade (`Archetype::work`) keeps a working day: by day it is at its work, found in the world file
//! round its home (a stall on a square, the shore of the nearest water, the fields beyond the town, or the roads for
//! the watch), with now and then an errand; of an evening many are out on the square; at night they are indoors.

use crate::sim::{Sim, NONE, NO_ARCH};
use proto::world::Aabb;

/// How far outside a house's wall its door step is.
const STEP: f32 = 1.5;

impl Sim {
    /// The home place of the bot in `slot` (an index into `places`): its type's homes, shared out by slot.
    pub(crate) fn home_of(&self, s: usize) -> Option<usize> {
        if self.ents.owner[s] != NONE {
            return None;
        }
        self.home_over[s].or_else(|| self.home_for(self.ents.arch[s], s))
    }

    /// The home a bot of type `arch` gets in `slot`.
    pub(crate) fn home_for(&self, arch: u8, slot: usize) -> Option<usize> {
        if arch == NO_ARCH {
            return None;
        }
        let homes = &self.archs.get(arch as usize)?.homes;
        (!homes.is_empty()).then(|| homes[slot % homes.len()])
    }

    /// The houses whose middle is inside place `h`.
    pub(crate) fn houses_in(&self, h: usize) -> impl Iterator<Item = &Aabb> + '_ {
        let p = &self.places[h];
        let (px, py, r2) = (p.x, p.y, p.r * p.r);
        self.terrain.houses.iter().filter(move |b| ((b.x0 + b.x1) / 2.0 - px).powi(2) + ((b.y0 + b.y1) / 2.0 - py).powi(2) <= r2)
    }

    /// A house's door step: the middle of one of its sides, just outside, on a road if one side gives onto one,
    /// else the side facing the middle of `toward`, and never inside another wall.
    pub(crate) fn step_of(&self, b: &Aabb, toward: (f32, f32)) -> Option<(f32, f32)> {
        let (mx, my) = ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0);
        let sides = [(mx, b.y0 - STEP), (mx, b.y1 + STEP), (b.x0 - STEP, my), (b.x1 + STEP, my)];
        let r = self.defs.entity_radius;
        sides
            .into_iter()
            .filter(|&(x, y)| !self.world.blocked_by_walls(x, y, r))
            .min_by(|a, b| {
                let score = |&(x, y): &(f32, f32)| (!self.world.on_road(x, y) as u8, (x - toward.0).powi(2) + (y - toward.1).powi(2));
                let (sa, sb) = (score(a), score(b));
                sa.0.cmp(&sb.0).then(sa.1.total_cmp(&sb.1))
            })
    }

    /// The bot in `slot`'s own front door, if it has a home with houses in it: the same house for as long as it
    /// holds the slot (two bots may share one, as a family does).
    pub(crate) fn door_of(&mut self, s: usize) -> Option<(f32, f32)> {
        if let Some(d) = self.door[s] {
            return d;
        }
        let found = self.home_of(s).and_then(|h| {
            let mid = (self.places[h].x, self.places[h].y);
            let n = self.houses_in(h).count();
            // Spread over the houses, not in file order: neighbouring slots get houses apart from each other.
            let pick = (s as u64).wrapping_mul(2_654_435_761) as usize % n.max(1);
            let b = *self.houses_in(h).nth(pick)?;
            Some((self.step_of(&b, mid)?, ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0)))
        });
        self.door[s] = Some(found.map(|f| f.0));
        self.house[s] = found.map(|f| f.1);
        found.map(|f| f.0)
    }

    /// Which places are towns (a house inside), and which bot types are armed: see `Sim::towns`.
    pub(crate) fn find_towns(&mut self) {
        self.town_index = (0..self.places.len()).filter(|&h| self.houses_in(h).next().is_some()).collect();
        self.towns = self.town_index.iter().map(|&h| (self.places[h].x, self.places[h].y, self.places[h].r * self.places[h].r)).collect();
        self.armed = self.archs.iter().map(|a| a.attack.is_some()).collect();
    }

    /// The squares whose middle is inside place `h`.
    fn squares_in(&self, h: usize) -> impl Iterator<Item = &Aabb> + '_ {
        let p = &self.places[h];
        let (px, py, r2) = (p.x, p.y, p.r * p.r);
        self.terrain.squares.iter().filter(move |b| ((b.x0 + b.x1) / 2.0 - px).powi(2) + ((b.y0 + b.y1) / 2.0 - py).powi(2) <= r2)
    }

    /// Whether a bot could stand at (x, y): inside the world and clear of walls and water.
    fn standable(&self, x: f32, y: f32) -> bool {
        let r = self.defs.entity_radius;
        x > r && y > r && x < self.world.size - r && y < self.world.size - r && !self.world.blocked_by_walls(x, y, r)
    }

    /// Where the bot in `slot` works by day (its type's `work`), the same spot for as long as it holds the slot;
    /// None for one with no trade, or no such place by its home. The watch has no one spot (`beat_spot`).
    pub(crate) fn work_of(&mut self, s: usize) -> Option<(f32, f32)> {
        if let Some(w) = self.work[s] {
            return w;
        }
        let w = self.find_work(s);
        self.work[s] = Some(w);
        w
    }

    fn find_work(&mut self, s: usize) -> Option<(f32, f32)> {
        use proto::game::Work;
        let h = self.home_of(s)?;
        let kind = self.archs.get(self.ents.arch[s] as usize)?.work?;
        let (px, py, r) = (self.places[h].x, self.places[h].y, self.places[h].r);
        // Spread over the spots by slot, as houses are, so neighbours work apart.
        let hash = (s as u64).wrapping_mul(2_654_435_761) >> 7;
        let frac = |k: u64| ((hash >> (k * 8)) & 0xff) as f32 / 255.0;
        match kind {
            Work::Streets | Work::Play => None,
            // A stall along one side of a square, a little in from its edge.
            Work::Square => {
                let n = self.squares_in(h).count();
                if n == 0 {
                    return None;
                }
                let b = *self.squares_in(h).nth(hash as usize % n)?;
                (0..8u64).find_map(|k| {
                    let (t, side) = (0.15 + 0.7 * frac(k % 3), (hash >> 3).wrapping_add(k) % 4);
                    let (x, y) = match side {
                        0 => (b.x0 + (b.x1 - b.x0) * t, b.y0 + 2.5),
                        1 => (b.x0 + (b.x1 - b.x0) * t, b.y1 - 2.5),
                        2 => (b.x0 + 2.5, b.y0 + (b.y1 - b.y0) * t),
                        _ => (b.x1 - 2.5, b.y0 + (b.y1 - b.y0) * t),
                    };
                    self.standable(x, y).then_some((x, y))
                })
            }
            // The shore of the water nearest the town, a step out of it, spaced along it.
            Work::Water => {
                let near = |b: &Aabb| (px.clamp(b.x0, b.x1), py.clamp(b.y0, b.y1));
                let (sx, sy) = self
                    .terrain
                    .water
                    .iter()
                    .map(near)
                    .min_by(|a, b| ((a.0 - px).powi(2) + (a.1 - py).powi(2)).total_cmp(&((b.0 - px).powi(2) + (b.1 - py).powi(2))))?;
                let d = (sx - px).hypot(sy - py);
                if d > r + 150.0 || d < 0.01 {
                    return None;
                }
                let (ux, uy) = ((px - sx) / d, (py - sy) / d);
                let along = (frac(0) - 0.5) * 2.0 * r.clamp(10.0, 40.0);
                // On the shore itself: the first dry ground out of the water, not a spot in it.
                (0..40).map(|k| 2.0 + k as f32 * 1.5).find_map(|out| {
                    let (x, y) = (sx + ux * out - uy * along, sy + uy * out + ux * along);
                    (self.standable(x, y) && self.terrain.dry(x, y)).then_some((x, y))
                })
            }
            // Out in the fields round the town, off the road.
            Work::Fields => (0..12u64).find_map(|k| {
                let ang = (frac(0) + k as f32 / 12.0) * std::f32::consts::TAU;
                let d = r + 8.0 + 30.0 * frac(1);
                let (x, y) = (px + ang.cos() * d, py + ang.sin() * d);
                (self.standable(x, y) && !self.world.on_road(x, y)).then_some((x, y))
            }),
        }
    }

    /// A spot on the watch's beat: somewhere along a road in its home, else anywhere in it.
    pub(crate) fn beat_spot(&mut self, s: usize) -> Option<(f32, f32)> {
        let h = self.home_of(s)?;
        let (px, py, r) = (self.places[h].x, self.places[h].y, self.places[h].r);
        let mut any = None;
        for _ in 0..12 {
            let (ang, d) = (self.rng.f32() * std::f32::consts::TAU, r * self.rng.f32().sqrt());
            let (x, y) = (px + ang.cos() * d, py + ang.sin() * d);
            if !self.standable(x, y) {
                continue;
            }
            if self.world.on_road(x, y) {
                return Some((x, y));
            }
            any.get_or_insert((x, y));
        }
        any
    }

    /// The middle of town `h`'s first square, else of the town: where its gatherings are, and its inn is beside.
    pub(crate) fn square_middle(&self, h: usize) -> (f32, f32) {
        self.squares_in(h).next().map_or((self.places[h].x, self.places[h].y), |b| ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0))
    }

    /// A spot on the shore of the water nearest (x, y), a step out of it, if any is within `reach`: where a beast
    /// comes to drink.
    pub(crate) fn water_near(&self, x: f32, y: f32, reach: f32) -> Option<(f32, f32)> {
        let (sx, sy) = self
            .terrain
            .water
            .iter()
            .map(|b| (x.clamp(b.x0, b.x1), y.clamp(b.y0, b.y1)))
            .min_by(|a, b| ((a.0 - x).powi(2) + (a.1 - y).powi(2)).total_cmp(&((b.0 - x).powi(2) + (b.1 - y).powi(2))))?;
        let d = (sx - x).hypot(sy - y);
        if d > reach || d < 0.01 {
            return None;
        }
        let (ux, uy) = ((x - sx) / d, (y - sy) / d);
        (0..20).map(|k| 2.0 + k as f32 * 1.5).find_map(|out| {
            let (px, py) = (sx + ux * out, sy + uy * out);
            (self.standable(px, py) && self.terrain.dry(px, py)).then_some((px, py))
        })
    }

    /// Somewhere on one of the squares of the bot's home, for an evening there.
    pub(crate) fn square_spot(&mut self, s: usize) -> Option<(f32, f32)> {
        let h = self.home_of(s)?;
        let n = self.squares_in(h).count();
        if n == 0 {
            return None;
        }
        let k = self.rng.below(n as u32) as usize;
        let b = *self.squares_in(h).nth(k)?;
        let (fx, fy) = (0.2 + 0.6 * self.rng.f32(), 0.2 + 0.6 * self.rng.f32());
        let (x, y) = (b.x0 + (b.x1 - b.x0) * fx, b.y0 + (b.y1 - b.y0) * fy);
        self.standable(x, y).then_some((x, y))
    }

    /// Some other house's door in the bot's home, to call at.
    pub(crate) fn neighbour_door(&mut self, s: usize) -> Option<(f32, f32)> {
        let h = self.home_of(s)?;
        let n = self.houses_in(h).count();
        if n < 2 {
            return None;
        }
        let k = self.rng.below(n as u32) as usize;
        let b = *self.houses_in(h).nth(k)?;
        self.step_of(&b, (self.places[h].x, self.places[h].y))
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{Sim, NONE};
    use proto::game::{Act, Behaviour, Cond, DayPart, GameFile, Place, Rule};
    use proto::world::{Aabb, World};
    use std::sync::Arc;

    #[test]
    fn townsfolk_with_a_trade_go_to_their_work_by_day_and_the_square_at_dusk() {
        use proto::game::Work;
        let mut g = GameFile::default_game();
        g.places.push(Place { name: "town".into(), x: 500.0, y: 500.0, r: 40.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false });
        let base = g.archetypes[0].clone();
        for (name, work) in [("trader", Work::Square), ("fisher", Work::Water), ("farmer", Work::Fields)] {
            let mut a = base.clone();
            (a.name, a.behaviour, a.attack, a.count, a.home, a.rules, a.work) = (name.into(), Behaviour::Wander, None, 0, Some("town".into()), vec![], Some(work));
            g.archetypes.push(a);
        }
        let n = g.archetypes.len() as u8;
        let lake = Aabb { x0: 600.0, y0: 400.0, x1: 700.0, y1: 600.0 };
        let square = Aabb { x0: 480.0, y0: 480.0, x1: 520.0, y1: 520.0 };
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![lake]), 64, 20, 1);
        (s.terrain.squares, s.terrain.water) = (Arc::new(vec![square]), Arc::new(vec![lake]));
        let [trader, fisher, farmer] = [n - 3, n - 2, n - 1].map(|k| s.spawn_as(NONE, k, None).unwrap());
        let (tx, ty) = s.work_of(trader).expect("a stall");
        assert!(tx > square.x0 && tx < square.x1 && ty > square.y0 && ty < square.y1, "on the square: {tx},{ty}");
        let (fx, fy) = s.work_of(fisher).expect("the shore");
        assert!(fx < lake.x0 && fx > lake.x0 - 50.0 && (fy - 500.0).abs() <= 40.0, "by the lake: {fx},{fy}");
        let (gx, gy) = s.work_of(farmer).expect("a field");
        let d = (gx - 500.0).hypot(gy - 500.0);
        assert!(d > 40.0 && d < 80.0, "just out of town: {d}");
        assert_eq!(s.work_of(farmer), Some((gx, gy)), "the same field every day");
        // By day they walk to work and stay about it.
        let near = |s: &Sim, b: usize, (x, y): (f32, f32), r: f32| (s.ents.x[b] - x).hypot(s.ents.y[b] - y) <= r;
        let mut at_work = [false; 3];
        for _ in 0..20 * 90 {
            s.daypart = DayPart::Day;
            s.step(&[]);
            for (k, (b, w)) in [(trader, (tx, ty)), (fisher, (fx, fy)), (farmer, (gx, gy))].into_iter().enumerate() {
                at_work[k] |= near(&s, b, w, 5.0);
            }
        }
        assert_eq!(at_work, [true; 3], "each got to its work");
        // Of an evening, the square.
        let mut on_square = false;
        for _ in 0..20 * 120 {
            s.daypart = DayPart::Dusk;
            s.step(&[]);
            on_square |= [fisher, farmer].iter().any(|&b| s.ents.x[b] > square.x0 && s.ents.x[b] < square.x1 && s.ents.y[b] > square.y0 && s.ents.y[b] < square.y1);
        }
        assert!(on_square, "out on the square at dusk");
    }

    #[test]
    fn townsfolk_are_shared_over_their_homes_and_go_in_at_their_own_door() {
        let mut g = GameFile::default_game();
        for (name, x) in [("north", 200.0), ("south", 700.0)] {
            g.places.push(Place { name: name.into(), x, y: 500.0, r: 60.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false });
        }
        let mut a = g.archetypes[0].clone();
        (a.name, a.behaviour, a.attack, a.count, a.home, a.homes) = ("folk".into(), Behaviour::Wander, None, 0, Some("north".into()), vec!["south".into()]);
        a.rules = vec![Rule { when: Cond::During { parts: vec![DayPart::Dawn, DayPart::Day, DayPart::Dusk, DayPart::Night] }, then: Act::Indoors }];
        g.archetypes.push(a);
        let n = g.archetypes.len() as u8 - 1;
        let house = Aabb { x0: 690.0, y0: 480.0, x1: 710.0, y1: 490.0 };
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![house]), 64, 20, 1);
        s.terrain.houses = Arc::new(vec![house]);
        let one = s.spawn_as(NONE, n, None).unwrap();
        let two = s.spawn_as(NONE, n, None).unwrap();
        let homes: Vec<_> = [one, two].iter().map(|&b| s.places[s.home_of(b).unwrap()].name.clone()).collect();
        assert!(homes.contains(&"north".to_string()) && homes.contains(&"south".to_string()), "one each: {homes:?}");
        let south = if homes[0] == "south" { one } else { two };
        let north = if south == one { two } else { one };
        assert_eq!(s.door_of(north), None, "no house in the north");
        let (dx, dy) = s.door_of(south).expect("the house in the south");
        assert!((dx - 700.0).abs() < 0.1 && (dy - 491.5).abs() < 0.1, "the side facing the middle of the place: {dx},{dy}");
        for _ in 0..20 * 40 {
            s.step(&[]);
            if s.ents.hidden[south] {
                break;
            }
        }
        assert!(s.ents.hidden[south], "in at its door: at {},{} for {dx},{dy}", s.ents.x[south], s.ents.y[south]);
        assert!((s.ents.x[south] - dx).abs() < 3.0 && (s.ents.y[south] - dy).abs() < 3.0, "at the door: {},{}", s.ents.x[south], s.ents.y[south]);
    }
}

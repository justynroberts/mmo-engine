//! Where townsfolk live. A bot type has a home place and maybe more (`Archetype::homes`), shared out over its bots
//! one each in turn, so one type can people every town. Inside its home a bot has a house of its own, one of the
//! world file's houses (`Terrain::houses`): it goes in at that door at night (`Act::Indoors`), so it comes out of it
//! in the morning, and in the day it calls now and then at a neighbour's door (`Sim::wander_dir`).

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
        self.home_for(self.ents.arch[s], s)
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
    fn houses_in(&self, h: usize) -> impl Iterator<Item = &Aabb> + '_ {
        let p = &self.places[h];
        let (px, py, r2) = (p.x, p.y, p.r * p.r);
        self.terrain.houses.iter().filter(move |b| ((b.x0 + b.x1) / 2.0 - px).powi(2) + ((b.y0 + b.y1) / 2.0 - py).powi(2) <= r2)
    }

    /// A house's door step: the middle of one of its sides, just outside, on a road if one side gives onto one,
    /// else the side facing the middle of `toward`, and never inside another wall.
    fn step_of(&self, b: &Aabb, toward: (f32, f32)) -> Option<(f32, f32)> {
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
        let d = self.home_of(s).and_then(|h| {
            let mid = (self.places[h].x, self.places[h].y);
            let n = self.houses_in(h).count();
            // Spread over the houses, not in file order: neighbouring slots get houses apart from each other.
            let pick = (s as u64).wrapping_mul(2_654_435_761) as usize % n.max(1);
            let b = *self.houses_in(h).nth(pick)?;
            self.step_of(&b, mid)
        });
        self.door[s] = Some(d);
        d
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

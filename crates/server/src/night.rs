//! The dark, and shelter from it. At night bots that fight see further (`Rules::night_sight`), and a player out in
//! the open (not in a `safe` place, not in a shelter) is now and then set on by bots of the types marked `ambush`
//! (`Rules::night_ambush_pct`), which come out of the dark already after them and are gone again at daybreak.
//!
//! A shelter is what a player builds: a roof of blocks over their head and walls on at least three sides (a door
//! in the fourth). Nothing sees a player in one, so nothing hunting them finds them there, day or night.

use crate::sim::{Sim, GHOST, NONE};

/// How far out from a player a wall still counts as theirs, in blocks, and how high over their head the roof.
const WALL_REACH: i64 = 5;
const ROOF_REACH: i16 = 5;

impl Sim {
    /// How much further a bot of type `a` sees now: `night_sight` at night for one that fights, else 1.
    pub(crate) fn dark_eyes(&self, a: usize) -> f32 {
        if self.night && self.archs.get(a).is_some_and(|r| r.attack.is_some()) {
            self.rules.night_sight
        } else {
            1.0
        }
    }

    /// Whether the player in `s` stands in a shelter: a block over their head and walls on three sides or more.
    pub(crate) fn shelter_at(&self, s: usize) -> bool {
        use proto::world::BLOCK;
        if self.world.block_count() == 0 {
            return false;
        }
        let (cx, cy) = ((self.ents.x[s] / BLOCK).floor() as i64, (self.ents.y[s] / BLOCK).floor() as i64);
        if cx < 0 || cy < 0 {
            return false;
        }
        let base = self.world.column(cx as u32, cy as u32).map_or_else(|| self.terrain.base(cx as u32, cy as u32), |c| c.base);
        let feet = self.feet(s, base);
        let solid = |x: i64, y: i64, z: i16| x >= 0 && y >= 0 && self.world.column(x as u32, y as u32).is_some_and(|c| c.solid(z));
        if !(feet + 1..=feet + ROOF_REACH).any(|z| solid(cx, cy, z)) {
            return false;
        }
        let walls = [(1, 0), (-1, 0), (0, 1), (0, -1)]
            .into_iter()
            .filter(|&(dx, dy)| (1..=WALL_REACH).any(|k| solid(cx + dx * k, cy + dy * k, feet) || solid(cx + dx * k, cy + dy * k, feet + 1)))
            .count();
        walls >= 3
    }

    /// Once a second: who is sheltered, the bots after them lose them, and at night those out in the open may be
    /// set on; at daybreak the night's hunters go (those still in a fight once it is over).
    pub(crate) fn system_night(&mut self) {
        for p in 0..self.ents.high {
            if !self.ents.alive[p] || self.ents.owner[p] >= GHOST {
                continue;
            }
            let was = self.sheltered[p];
            let now = !self.ents.fallen[p] && self.shelter_at(p);
            self.sheltered[p] = now;
            if !now {
                continue;
            }
            if !was && self.tick >= self.shelter_said[p] {
                self.shelter_said[p] = self.tick + 60 * self.tick_hz;
                self.tells.push((p, "You are under your own roof: nothing out there can find you here.".into()));
            }
            for b in self.pursuers[p].clone() {
                let Some(b) = self.ents.resolve(b) else { continue };
                if let Some(a) = self.archs.get(self.ents.arch[b] as usize) {
                    self.tells.push((p, format!("The {} loses your trail.", a.shown())));
                }
                self.release(b);
            }
        }
        if !self.night {
            if !self.ambushers.is_empty() {
                for id in std::mem::take(&mut self.ambushers) {
                    match self.ents.resolve(id) {
                        Some(b) if self.foe[b].is_some() => self.ambushers.push(id),
                        Some(b) => self.ents.despawn(b),
                        None => {}
                    }
                }
            }
            return;
        }
        self.ambushers.retain(|&id| self.ents.resolve(id).is_some());
        let pct = self.rules.night_ambush_pct * self.season_night();
        let kinds: Vec<u8> = self.archs.iter().enumerate().filter(|(_, a)| a.ambush).map(|(i, _)| i as u8).collect();
        if pct <= 0.0 || kinds.is_empty() {
            return;
        }
        let pack = self.rules.night_pack.max(1) as u32;
        let size = self.world.size;
        let r = self.defs.entity_radius;
        for p in 0..self.ents.high {
            if !self.ents.alive[p] || self.ents.owner[p] >= GHOST || self.ents.fallen[p] || self.ents.hidden[p] || self.ents.god[p] {
                continue;
            }
            if self.sheltered[p] || !self.pursuers[p].is_empty() || self.rs.phase[p] != 0 || self.in_safe(p) {
                continue;
            }
            // `pct` a minute, so a sixtieth of it each second.
            if self.rng.f32() * 100.0 >= pct / 60.0 || self.ambushers.len() >= 8 * self.ents.live.max(1) as usize {
                continue;
            }
            let kind = kinds[self.rng.below(kinds.len() as u32) as usize];
            let n = 1 + self.rng.below(pack);
            // Out at the edge of what it sees in the dark, so it comes at them out of it.
            let d = (self.archs[kind as usize].sight * self.rules.night_sight * 0.7).clamp(18.0, 50.0);
            let (px, py) = (self.ents.x[p], self.ents.y[p]);
            let ang = self.rng.f32() * std::f32::consts::TAU;
            let mut came = 0;
            for k in 0..n {
                let a = ang + (k as f32 - n as f32 / 2.0) * 0.35;
                let (x, y) = (px + a.cos() * d, py + a.sin() * d);
                if x < r || y < r || x > size - r || y > size - r || self.world.blocked_by_walls(x, y, r) {
                    continue;
                }
                let Some(b) = self.spawn_as(NONE, kind, Some((x, y))) else { break };
                self.ents.fleeting[b] = true; // beaten is beaten: it does not come back
                self.ambushers.push(self.ents.id_of(b));
                self.engage(b, p);
                came += 1;
            }
            if came > 0 {
                self.tells.push((p, "Something is moving out there in the dark.".into()));
                let who = self.name_of(p).to_string();
                self.tell_news("news_ambush", who, String::new(), px, py);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::{Behaviour, GameFile};
    use proto::world::World;

    fn night_world(pct: f32) -> (Sim, usize) {
        let mut g = GameFile::default_game();
        (g.rules.night_pack, g.rules.night_sight) = (2, 1.5);
        let mut wolf = g.archetypes[0].clone();
        (wolf.name, wolf.behaviour, wolf.attack, wolf.count, wolf.home, wolf.rules, wolf.ambush) =
            ("night wolf".into(), Behaviour::Chase, Some(g.actions[0].name.clone()), 0, None, vec![], true);
        g.archetypes.push(wolf);
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        s.rules.night_ambush_pct = pct; // past what a game file may say, so the test need not roll for it
        let me = s.spawn(0).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (500.0, 500.0);
        (s, me)
    }

    /// Build a hut round the player in `s`: a roof over them and walls on every side but the south, a door.
    fn hut(s: &mut Sim, me: usize) {
        let (cx, cy) = ((s.ents.x[me] / 2.0).floor() as u32, (s.ents.y[me] / 2.0).floor() as u32);
        let base = s.terrain.base(cx, cy);
        let mut cols = Vec::new();
        let wall = proto::world::Column { base, mask: 0b11 };
        for (x, y) in [(cx + 2, cy), (cx - 2, cy), (cx, cy - 2)] {
            cols.push((x, y, wall));
        }
        cols.push((cx, cy, proto::world::Column { base, mask: 0b100 }));
        for (x, y, c) in cols {
            s.world.set_column(x, y, c);
        }
    }

    #[test]
    fn at_night_hunters_come_for_a_player_in_the_open_and_go_at_daybreak() {
        let (mut s, me) = night_world(6000.0); // a hundred a second: every time
        s.night = true;
        s.system_night();
        assert!(!s.ambushers.is_empty(), "set on");
        assert!(!s.pursuers[me].is_empty(), "and already after them");
        assert!(s.tells.iter().any(|(p, t)| *p == me && t.contains("in the dark")));
        let b = s.ents.resolve(s.ambushers[0]).unwrap();
        assert!(s.ents.fleeting[b], "an ambusher does not come back once beaten");
        assert_eq!(s.dark_eyes(s.ents.arch[b] as usize), 1.5, "it sees further in the dark");
        let n = s.ambushers.len();
        s.system_night();
        assert_eq!(s.ambushers.len(), n, "not again while they are in a fight");
        s.night = false;
        for &id in &s.ambushers.clone() {
            let b = s.ents.resolve(id).unwrap();
            s.release(b);
        }
        s.system_night();
        assert!(s.ambushers.is_empty() && s.ents.resolve(s.ents.id_of(b)).is_none(), "gone at daybreak");
    }

    #[test]
    fn a_shelter_a_player_built_hides_them_and_breaks_off_the_chase() {
        let (mut s, me) = night_world(6000.0);
        s.night = true;
        hut(&mut s, me);
        s.system_night();
        assert!(s.sheltered[me], "a roof and three walls");
        assert!(s.ambushers.is_empty(), "nothing comes for a player under their own roof");
        assert!(s.tells.iter().any(|(p, t)| *p == me && t.contains("your own roof")));
        // Out of it, set on; back in, lost.
        s.ents.x[me] += 30.0;
        s.system_night();
        assert!(!s.sheltered[me] && !s.pursuers[me].is_empty(), "out in the open, found");
        s.ents.x[me] -= 30.0;
        s.system_night();
        assert!(s.sheltered[me] && s.pursuers[me].is_empty(), "back in, the chase is broken off");
        assert!(s.tells.iter().any(|(p, t)| *p == me && t.contains("loses your trail")));
        // Two walls and a roof are not a shelter.
        let (cx, cy) = ((s.ents.x[me] / 2.0).floor() as u32, (s.ents.y[me] / 2.0).floor() as u32);
        s.world.set_column(cx, cy - 2, proto::world::Column { base: 0, mask: 0 });
        assert!(!s.shelter_at(me));
    }
}

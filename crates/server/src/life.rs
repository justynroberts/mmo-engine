//! Life experience (`LevelsFile::life`): a player earns a little for everything they do, walking, fighting,
//! building, trading, talking, not only for beating bots. Deeds add fractions to a bank; once a second the ground
//! covered is added (at most a run's worth, so a teleport or a respawn counts for nothing), and every ten seconds
//! the whole points in the bank are paid as one award, within the minute's cap.
//!
//! Against macros (a mouse jiggler, an auto-walk in circles, a key repeating one action): walking counts only
//! while it reaches ground the player has not trodden lately (`LifeXp::trail`, in `TRAIL_CELL` cells), and a deed
//! the same as one of the last few (`LifeXp::repeat`) pays nothing; neither wakes them for the minute's pay, so a
//! loop earns nothing once it is learnt. What was let go is counted in `Sim::life_stale`.

use crate::sim::{Sim, DRIVEN};

/// Ground counted in one second at most (a fast mount on a road), so a jump across the map is not a walk.
const MAX_STRIDE: f32 = 60.0;

/// The trail's cell, in world units.
const TRAIL_CELL: f32 = 32.0;

/// Ground in a new cell keeps paying for this long, so the seconds spent crossing it count.
const FRESH_SECS: u32 = 10;

/// A deed's key, from what makes it the same deed (the action, the cell, the item).
pub fn key(parts: &[u64]) -> u64 {
    parts.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &p| (h ^ p).wrapping_mul(0x1000_0000_01b3))
}

/// What kind of deed, for its rate in `LifeXp`.
#[derive(Clone, Copy)]
pub enum Deed {
    Act,
    Build,
    Deed,
    Plant,
    Harvest,
    /// A line said: pays nothing, but a new one shows the player is there.
    Say,
}

impl Sim {
    /// A player did something worth a little experience.
    pub(crate) fn lived(&mut self, s: usize, what: Deed) {
        if self.levels.is_none() || self.ents.owner[s] >= DRIVEN {
            return;
        }
        self.life_awake[s] = self.tick;
        self.life_bank[s] += self.life_rate(what);
    }

    /// A deed that counts only if it is not one of the player's last few (`LifeXp::repeat`). Returns whether it
    /// counted.
    pub(crate) fn lived_once(&mut self, s: usize, what: Deed, key: u64) -> bool {
        let Some(n) = self.levels.as_ref().map(|l| l.life.repeat as usize) else { return false };
        if self.ents.owner[s] >= DRIVEN {
            return false;
        }
        if n > 0 {
            let done = &mut self.life_done[s];
            if done.contains(&key) {
                self.life_stale[s] += 1;
                return false;
            }
            if done.len() >= n {
                done.pop_front();
            }
            done.push_back(key);
        }
        self.lived(s, what);
        true
    }

    fn life_rate(&self, what: Deed) -> f32 {
        let Some(l) = self.levels.as_ref() else { return 0.0 };
        match what {
            Deed::Act => l.life.act,
            Deed::Build => l.life.build,
            Deed::Deed => l.life.deed,
            Deed::Plant => l.life.plant,
            Deed::Harvest => l.life.harvest,
            Deed::Say => 0.0,
        }
    }

    /// Once a second: the ground each player covered goes in the bank; every ten seconds the bank is paid.
    pub fn system_life(&mut self) {
        let Some(l) = self.levels.as_ref() else { return };
        let (walk, per_min, each_min, company, trail) = (l.life.walk, l.life.per_min, l.life.minute, l.life.company, l.life.trail as usize);
        let hz = self.tick_hz.max(1);
        let pay = self.tick % (10 * hz) == 0;
        let minute = self.tick / (60 * hz);
        // Once a minute: time in the world pays too, for whoever is awake, and more in company.
        let on_the_minute = self.tick > 0 && self.tick % (60 * hz) == 0 && (each_min > 0.0 || company > 0.0);
        let awake: Vec<usize> = if on_the_minute {
            (0..self.ents.high).filter(|&s| self.ents.alive[s] && self.ents.owner[s] < DRIVEN && !self.ents.fallen[s] && self.tick.saturating_sub(self.life_awake[s]) < 300 * hz).collect()
        } else {
            Vec::new()
        };
        let reach = self.rules.say_reach * self.rules.say_reach;
        for &s in &awake {
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            let near = awake.iter().any(|&o| o != s && (self.ents.x[o] - x).powi(2) + (self.ents.y[o] - y).powi(2) <= reach);
            self.life_bank[s] += each_min + if near { company } else { 0.0 };
        }
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= DRIVEN {
                continue;
            }
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            let (px, py) = self.life_at[s];
            self.life_at[s] = (x, y);
            if px.is_finite() {
                let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                let fresh = trail == 0 || self.new_ground(s, x, y, trail);
                if fresh {
                    self.life_fresh[s] = self.tick;
                }
                if d > 0.5 && self.tick.saturating_sub(self.life_fresh[s]) < FRESH_SECS * hz {
                    if d <= MAX_STRIDE {
                        self.life_bank[s] += d / 100.0 * walk;
                    }
                    self.life_awake[s] = self.tick;
                } else if d > 0.5 {
                    self.life_stale[s] += 1; // a second spent going over old ground
                }
            }
            if !pay || self.life_bank[s] < 1.0 {
                continue;
            }
            if self.life_paid[s].0 != minute {
                self.life_paid[s] = (minute, 0);
            }
            let room = per_min.saturating_sub(self.life_paid[s].1);
            let n = (self.life_bank[s].floor() as u32).min(room);
            // What the cap would not take is let go, so a long busy spell does not pay out for an hour after.
            self.life_bank[s] = if n == room { 0.0 } else { self.life_bank[s].fract() };
            if n > 0 {
                self.life_paid[s].1 += n;
                self.award(s, n, None);
            }
        }
    }
}

impl Sim {
    /// Whether (x, y) is in a cell not among the player's last `trail`; if so it joins them.
    fn new_ground(&mut self, s: usize, x: f32, y: f32, trail: usize) -> bool {
        let cell = ((x / TRAIL_CELL) as u32 & 0xffff) << 16 | ((y / TRAIL_CELL) as u32 & 0xffff);
        let t = &mut self.life_trail[s];
        if t.contains(&cell) {
            return false;
        }
        while t.len() >= trail {
            t.pop_front();
        }
        t.push_back(cell);
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim};
    use proto::game::{GameFile, LevelsFile};
    use proto::world::World;

    fn sim() -> (Sim, usize) {
        let mut g = GameFile::default_game();
        g.levels = Some(LevelsFile { cap: 10, base: 1000.0, exponent: 1.0, might: 0.0, toughness: 0.0, discover_xp: 0, life: Default::default(), rewards: vec![] });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (100.0, 500.0);
        (s, me)
    }

    #[test]
    fn walking_and_doing_things_earn_a_little_within_the_cap() {
        let (mut s, me) = sim();
        // Walk east 20 units a second for 20 seconds: 380 units counted (the first second has nothing to measure
        // from), 5.7 experience at 1.5 a hundred, 5 of it paid.
        for t in 0..20 * 20 {
            s.ents.x[me] = 100.0 + t as f32;
            s.ents.dir_x[me] = 0;
            s.step(&[]);
        }
        assert_eq!(s.ents.xp[me], 5, "bank {}", s.life_bank[me]);
        // A teleport is not a walk.
        let before = s.ents.xp[me];
        s.ents.x[me] = 900.0;
        for _ in 0..10 * 20 {
            s.ents.x[me] = 900.0;
            s.step(&[]);
        }
        assert_eq!(s.ents.xp[me], before);
        // Blows: half a point each, paid in the next ten seconds.
        let other = s.spawn(1).unwrap();
        (s.ents.x[other], s.ents.y[other]) = (904.0, 500.0);
        let oid = s.ents.id_of(other);
        for _ in 0..6 {
            s.ents.attrs[proto::defs::ATTR_ENERGY as usize][me] = 100.0; // energy for the zap
            s.step(&[PendingAction { slot: me, action: proto::defs::ACTION_ZAP, target: oid, arg: 0 }]);
            for _ in 0..20 {
                s.step(&[]);
            }
        }
        for _ in 0..10 * 20 {
            s.step(&[]);
        }
        assert!(s.ents.xp[me] >= before + 2, "{} -> {}", before, s.ents.xp[me]);
    }

    #[test]
    fn time_in_the_world_pays_more_in_company_but_not_asleep() {
        let (mut s, me) = sim();
        let minute = |s: &mut Sim| {
            for _ in 0..60 * 20 {
                s.step(&[]);
            }
        };
        minute(&mut s);
        assert_eq!(s.ents.xp[me], 1, "a minute in the world, alone");
        let friend = s.spawn(0).unwrap();
        (s.ents.x[friend], s.ents.y[friend]) = (110.0, 500.0);
        minute(&mut s);
        assert_eq!((s.ents.xp[me], s.ents.xp[friend]), (3, 2), "in company: a minute's worth and as much again");
        for _ in 0..6 {
            minute(&mut s);
        }
        let dozing = s.ents.xp[me];
        minute(&mut s);
        assert_eq!(s.ents.xp[me], dozing, "five minutes without stirring and time stops paying");
        s.ents.x[me] += 40.0; // into ground not walked yet: a step in the same spot would not do
        minute(&mut s);
        assert!(s.ents.xp[me] > dozing, "awake again");
    }

    #[test]
    fn pacing_a_loop_or_repeating_one_deed_stops_paying_once_it_is_learnt() {
        let (mut s, me) = sim();
        // A macro walking back and forth over the same 60 units, 20 units a second.
        let pace = |s: &mut Sim, secs: u32| {
            for t in 0..secs * 20 {
                let k = (t / 20) % 6;
                s.ents.x[me] = 100.0 + if k < 3 { k * 20 } else { (6 - k) * 20 } as f32;
                s.step(&[]);
            }
        };
        // The first minutes still pay for time (they were awake when the loop began, and stay so five minutes).
        pace(&mut s, 6 * 60);
        let learnt = s.ents.xp[me];
        pace(&mut s, 10 * 60);
        assert_eq!(s.ents.xp[me], learnt, "going over old ground pays nothing, nor keeps them awake for time");
        assert!(s.life_stale[me] > 300, "and is counted: {}", s.life_stale[me]);
        // Off down the road and it pays again.
        for t in 0..60 * 20 {
            s.ents.x[me] = 200.0 + t as f32;
            s.step(&[]);
        }
        assert!(s.ents.xp[me] > learnt, "new ground pays");

        // The same deed over and over (build and break one block, drop and pick up one thing) pays once.
        use crate::life::{key, Deed};
        let block = key(&[1, 77, 0]);
        assert!(s.lived_once(me, Deed::Deed, block));
        for _ in 0..20 {
            assert!(!s.lived_once(me, Deed::Deed, block));
        }
        // Two alternating deeds are still a loop.
        let (a, b) = (key(&[4, 3]), key(&[2, 9]));
        assert!(s.lived_once(me, Deed::Deed, a) && s.lived_once(me, Deed::Deed, b));
        assert!(!s.lived_once(me, Deed::Deed, a) && !s.lived_once(me, Deed::Deed, b));
        // Different deeds all count.
        assert!((0..20).all(|i| s.lived_once(me, Deed::Build, key(&[1, 100 + i, 0]))));
    }

    #[test]
    fn no_more_than_the_cap_a_minute() {
        let (mut s, me) = sim();
        s.life_bank[me] = 500.0;
        for _ in 0..10 * 20 {
            s.step(&[]);
        }
        assert_eq!(s.ents.xp[me], 30);
        assert_eq!(s.life_bank[me], 0.0, "the rest is let go");
    }
}

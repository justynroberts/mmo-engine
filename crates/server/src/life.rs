//! Life experience (`LevelsFile::life`): a player earns a little for everything they do, walking, fighting,
//! building, trading, talking, not only for beating bots. Deeds add fractions to a bank; once a second the ground
//! covered is added (at most a run's worth, so a teleport or a respawn counts for nothing), and every ten seconds
//! the whole points in the bank are paid as one award, within the minute's cap.

use crate::sim::{Sim, DRIVEN};

/// Ground counted in one second at most (a fast mount on a road), so a jump across the map is not a walk.
const MAX_STRIDE: f32 = 60.0;

/// What kind of deed, for its rate in `LifeXp`.
#[derive(Clone, Copy)]
pub enum Deed {
    Act,
    Build,
    Deed,
}

impl Sim {
    /// A player did something worth a little experience.
    pub(crate) fn lived(&mut self, s: usize, what: Deed) {
        let Some(l) = self.levels.as_ref() else { return };
        if self.ents.owner[s] >= DRIVEN {
            return;
        }
        self.life_bank[s] += match what {
            Deed::Act => l.life.act,
            Deed::Build => l.life.build,
            Deed::Deed => l.life.deed,
        };
    }

    /// Once a second: the ground each player covered goes in the bank; every ten seconds the bank is paid.
    pub fn system_life(&mut self) {
        let Some(l) = self.levels.as_ref() else { return };
        let (walk, per_min) = (l.life.walk, l.life.per_min);
        let pay = self.tick % (10 * self.tick_hz.max(1)) == 0;
        let minute = self.tick / (60 * self.tick_hz.max(1));
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= DRIVEN {
                continue;
            }
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            let (px, py) = self.life_at[s];
            self.life_at[s] = (x, y);
            if px.is_finite() {
                let d = ((x - px).powi(2) + (y - py).powi(2)).sqrt();
                if d <= MAX_STRIDE {
                    self.life_bank[s] += d / 100.0 * walk;
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

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim};
    use proto::game::{GameFile, LevelsFile};
    use proto::world::World;

    fn sim() -> (Sim, usize) {
        let mut g = GameFile::default_game();
        g.levels = Some(LevelsFile { cap: 10, base: 1000.0, exponent: 1.0, might: 0.0, toughness: 0.0, discover_xp: 0, life: Default::default() });
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

//! Hailing (H, `ACT_HAIL`): a player calls to a bot that does not fight, or keeps a shop or gives quests, and it
//! stands still for them, `rules.hail_secs`, or until they go more than twice `rules.hail_reach` away. Talking
//! to one holds it the same way, so a wandering trader does not walk off mid-sale.

use crate::sim::{Sim, NONE};

impl Sim {
    /// Whether the bot in `t` stops when hailed: a plain bot (not possessed) whose type does not fight, or one
    /// that does but keeps a shop or gives quests (an armed trader, a warrior with work for you).
    fn hailable(&self, t: usize) -> bool {
        self.ents.alive[t]
            && self.ents.owner[t] == NONE
            && !self.ents.hidden[t]
            && !self.ents.fallen[t]
            && self.archs.get(self.ents.arch[t] as usize).is_some_and(|a| a.attack.is_none() || !a.shop.is_empty() || self.quests.iter().any(|q| q.giver == a.name))
    }

    /// A player hails the bot `target` (an entity id), or with 0 the nearest one in reach that stops. False if
    /// there is none.
    pub fn hail(&mut self, s: usize, target: u32) -> bool {
        let reach2 = self.rules.hail_reach * self.rules.hail_reach;
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let d2 = |t: usize| (self.ents.x[t] - x).powi(2) + (self.ents.y[t] - y).powi(2);
        let t = if target != 0 {
            self.ents.resolve(target).filter(|&t| self.hailable(t) && d2(t) <= reach2)
        } else {
            (0..self.ents.high).filter(|&t| t != s && self.hailable(t) && d2(t) <= reach2).min_by(|&a, &b| d2(a).total_cmp(&d2(b)))
        };
        let Some(t) = t else {
            self.tells.push((s, "Nobody near enough stops for a hail.".into()));
            return false;
        };
        if let Some(why) = self.refuses(t, s) {
            self.tells.push((s, why));
            return true;
        }
        self.hold_for(t, s);
        let who = self.called(t);
        let mut c = who.chars();
        let who: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
        self.tells.push((s, format!("{who} stops for you.")));
        true
    }

    /// Keep the bot in `t` standing for the player in `s` (`rules.hail_secs` from now).
    pub(crate) fn hold_for(&mut self, t: usize, s: usize) {
        let secs = self.rules.hail_secs;
        self.hold_for_secs(t, s, secs);
    }

    /// Keep the bot in `t` standing for the player in `s` for `secs`, unless it is held longer already.
    pub(crate) fn hold_for_secs(&mut self, t: usize, s: usize, secs: f32) {
        if !self.hailable(t) || secs <= 0.0 {
            return;
        }
        let until = self.tick + proto::rules::Rules::ticks(secs, self.tick_hz);
        if self.hailed[t].1 < until {
            self.hailed[t] = (self.ents.id_of(s), until);
        }
    }

    /// Each tick, after the bots decide: those held stand still, until the time is up or whoever hailed them
    /// has gone.
    pub(crate) fn system_hail(&mut self) {
        let far2 = (2.0 * self.rules.hail_reach).powi(2);
        for t in 0..self.ents.high {
            let (by, until) = self.hailed[t];
            if until == 0 {
                continue;
            }
            let near = self.ents.resolve(by).is_some_and(|p| {
                !self.ents.fallen[p] && (self.ents.x[p] - self.ents.x[t]).powi(2) + (self.ents.y[p] - self.ents.y[t]).powi(2) <= far2
            });
            if self.tick >= until || !near || !self.hailable(t) {
                self.hailed[t] = (0, 0);
                continue;
            }
            self.ents.dir_x[t] = 0;
            self.ents.dir_y[t] = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim};
    use proto::game::{Behaviour, GameFile, ACT_HAIL};
    use proto::world::World;

    #[test]
    fn a_hailed_trader_stands_still_until_you_walk_off_but_a_wolf_does_not() {
        let mut g = GameFile::default_game();
        g.rules.hail_secs = 60.0;
        let mut a = g.archetypes[0].clone();
        (a.name, a.title, a.behaviour, a.attack, a.count, a.speed_mult, a.home, a.rules) =
            ("ceannai".into(), "Trader".into(), Behaviour::Wander, None, 0, 1.0, None, vec![]);
        g.archetypes.push(a);
        let mut wolf = g.archetypes[0].clone();
        (wolf.name, wolf.behaviour, wolf.attack, wolf.count, wolf.home, wolf.rules) = ("wolf".into(), Behaviour::Wander, Some(g.actions[0].name.clone()), 0, None, vec![]);
        g.archetypes.push(wolf);
        let n = g.archetypes.len();
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (500.0, 500.0);
        let trader = s.spawn_as(crate::sim::NONE, n as u8 - 2, Some((510.0, 500.0))).unwrap();
        let wolf = s.spawn_as(crate::sim::NONE, n as u8 - 1, Some((490.0, 500.0))).unwrap();
        assert_eq!(&*s.name_of(trader), "Trader", "the title is what it is called");

        s.step(&[PendingAction { slot: me, action: ACT_HAIL, target: s.ents.id_of(wolf), arg: 0 }]);
        assert_eq!(s.hailed[wolf].1, 0, "one that fights does not stop");
        s.archs[n - 1].shop = vec![(0, 1)];
        s.step(&[PendingAction { slot: me, action: ACT_HAIL, target: s.ents.id_of(wolf), arg: 0 }]);
        assert!(s.hailed[wolf].1 > 0, "unless it keeps a shop");
        s.hailed[wolf] = (0, 0);
        s.ents.despawn(wolf); // before it fells us
        s.step(&[PendingAction { slot: me, action: ACT_HAIL, target: 0, arg: 0 }]);
        assert!(s.hailed[trader].1 > 0, "the nearest that will stop does");
        let at = (s.ents.x[trader], s.ents.y[trader]);
        for _ in 0..20 * 20 {
            s.step(&[]);
        }
        assert_eq!((s.ents.x[trader], s.ents.y[trader]), at, "it stands for twenty seconds");
        s.ents.x[me] = 900.0; // gone
        for _ in 0..20 * 20 {
            s.step(&[]);
        }
        assert_eq!(s.hailed[trader].1, 0, "and walks on once they leave");
    }
}

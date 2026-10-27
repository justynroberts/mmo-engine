//! Town life between townsfolk. Now and then two of them quarrel and it comes to blows (`Rules::brawl_pct`), till
//! one gives in or the watch breaks it up. A cutpurse (`Archetype::thief`) works the crowd: standing beside someone
//! it may lift their purse (`Rules::lift_pct`), coin from a player's bag; the cry goes up, it runs, and the watch
//! (bot types that work the `streets`) goes after it. A cutpurse that falls drops what it took.
//!
//! Townsfolk are bots with a home that carry no weapon; the watch are those whose `work` is the streets.

use crate::sim::{dir_to, PendingAction, Sim, DRIVEN, NONE};
use proto::defs::ATTR_HEALTH;
use proto::game::Work;

/// How close two must be to come to blows, or a cutpurse to its mark, and how near the watch must be to step in.
const QUARREL_REACH: f32 = 6.0;
const LIFT_REACH: f32 = 2.5;
const WATCH_SEES: f32 = 25.0;
const WATCH_CHASES: f32 = 60.0;
/// Below this share of its health one in a brawl gives in.
const GIVES_IN: f32 = 0.45;

const INSULTS: &[&str] = &["Say that again, I dare you!", "You cheated me at the market!", "That's my seat, you lout!", "You've a face like a slapped haddock.", "Who are you calling a liar?"];
const GIVE_IN: &[&str] = &["Enough! Enough, I yield!", "All right, all right!", "I'll pay, I'll pay!"];
const WON: &[&str] = &["And that's the penny you owed me.", "Let that be a lesson to you.", "Next time, mind your tongue."];
const BREAK_UP: &[&str] = &["Break it up, you two!", "Enough of that! Off home with you.", "Any more of this and it's the stocks."];
const CRY: &[&str] = &["Thief! Stop, thief!", "My purse! Someone stop him!", "Cutpurse! Watch, watch!"];
const NOTICED: &[&str] = &["Hands off my purse!", "I see you there, you rogue!"];
const STOP: &[&str] = &["Stop in the name of the watch!", "Halt, thief!", "After him!"];

fn pick(sim: &mut Sim, lines: &[&str]) -> String {
    lines[sim.rng.below(lines.len() as u32) as usize].to_string()
}

impl Sim {
    /// A bot with a home and no weapon, out and about: one of the townsfolk.
    fn townsperson(&self, s: usize) -> bool {
        self.ents.alive[s]
            && self.ents.owner[s] == NONE
            && !self.ents.hidden[s]
            && !self.ents.fallen[s]
            && self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.attack.is_none() && !a.thief && !a.homes.is_empty())
    }

    fn is_thief(&self, s: usize) -> bool {
        self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.thief)
    }

    fn is_watch(&self, s: usize) -> bool {
        self.ents.alive[s] && self.ents.owner[s] == NONE && !self.ents.fallen[s] && self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.work == Some(Work::Streets) && a.attack.is_some())
    }

    fn d2(&self, a: usize, b: usize) -> f32 {
        (self.ents.x[a] - self.ents.x[b]).powi(2) + (self.ents.y[a] - self.ents.y[b]).powi(2)
    }

    /// What a brawl is fought with: the blow the first armed bot type uses.
    fn fists(&self) -> Option<u8> {
        self.archs.iter().find_map(|a| a.attack)
    }

    /// A spawn takes nothing of the last one in the slot.
    pub(crate) fn town_reset(&mut self, s: usize) {
        (self.brawl[s], self.purse[s], self.wanted[s], self.slip[s], self.collar[s], self.lift_at[s], self.calm[s]) = (None, 0, 0, None, None, 0, 0);
    }

    /// The bot in `s` is busy with town life this AI tick (in a brawl, running with a purse, after a thief): its
    /// steering is set here. False leaves it to its rules and behaviour.
    pub(crate) fn town_steer(&mut self, s: usize) -> bool {
        let tick = self.tick;
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        if let Some((other, until)) = self.brawl[s] {
            match self.ents.resolve(other).filter(|&o| tick < until && self.brawl[o].is_some()) {
                Some(o) => {
                    (self.ents.dir_x[s], self.ents.dir_y[s]) = dir_to(self.ents.x[o] - x, self.ents.y[o] - y, 1.2);
                    if let Some(act) = self.fists() {
                        let range = self.defs.action(act).map_or(2.0, |d| d.range.min(2.5));
                        if self.d2(s, o) <= range * range && self.rng.f32() < 0.5 {
                            self.npc_actions.push(PendingAction { slot: s, action: act, target: other, arg: 0 });
                        }
                    }
                    return true;
                }
                None => self.brawl[s] = None,
            }
        }
        if let Some((fx, fy, until)) = self.slip[s] {
            if tick < until {
                // Away from whoever is after it: the nearest of the watch on its heels, else where it struck.
                let (fx, fy) = (0..self.ents.high)
                    .filter(|&w| self.collar[w].is_some_and(|t| self.ents.resolve(t) == Some(s)))
                    .min_by(|&a, &b| self.d2(a, s).total_cmp(&self.d2(b, s)))
                    .map_or((fx, fy), |w| (self.ents.x[w], self.ents.y[w]));
                (self.ents.dir_x[s], self.ents.dir_y[s]) = dir_to(x - fx, y - fy, 0.0);
                return true;
            }
            self.slip[s] = None;
        }
        if let Some(t) = self.collar[s] {
            match self.ents.resolve(t).filter(|&t| tick < self.wanted[t] && !self.ents.fallen[t]) {
                Some(t) if self.d2(s, t) <= WATCH_CHASES * WATCH_CHASES * 1.5 => {
                    (self.ents.dir_x[s], self.ents.dir_y[s]) = dir_to(self.ents.x[t] - x, self.ents.y[t] - y, 1.0);
                    if let Some(act) = self.archs.get(self.ents.arch[s] as usize).and_then(|a| a.attack) {
                        let range = self.defs.action(act).map_or(2.0, |d| d.range);
                        if self.d2(s, t) <= range * range {
                            self.npc_actions.push(PendingAction { slot: s, action: act, target: self.ents.id_of(t), arg: 0 });
                        }
                    }
                    return true;
                }
                _ => self.collar[s] = None,
            }
        }
        false
    }

    /// Once a second: quarrels start and end, cutpurses work the crowd, and the watch answers the cry.
    pub(crate) fn system_town(&mut self) {
        self.town_brawls();
        self.town_thieves();
        self.town_watch();
    }

    fn town_brawls(&mut self) {
        let tick = self.tick;
        let hz = self.tick_hz.max(1);
        // Those in a brawl: one that has had enough gives in, and the watch close by breaks it up.
        for s in 0..self.ents.high {
            let Some((other, _)) = self.brawl[s] else { continue };
            let Some(o) = self.ents.resolve(other) else {
                self.brawl[s] = None;
                continue;
            };
            let max = self.defs.attrs[ATTR_HEALTH as usize].max;
            let beaten = self.ents.attrs[ATTR_HEALTH as usize][s] < max * GIVES_IN;
            let watch = (0..self.ents.high).find(|&w| self.is_watch(w) && self.d2(w, s) <= WATCH_SEES * WATCH_SEES);
            if !beaten && watch.is_none() {
                continue;
            }
            if let Some(w) = watch {
                let line = pick(self, BREAK_UP);
                self.npc_chat.push((w, line));
            } else {
                let (a, b) = (pick(self, GIVE_IN), pick(self, WON));
                self.npc_chat.push((s, a));
                self.npc_chat.push((o, b));
            }
            for b in [s, o] {
                self.brawl[b] = None;
                self.calm[b] = tick + 120 * hz;
                self.wander_wait[b] = tick + 3 * hz;
                self.wander_to[b] = (f32::NAN, f32::NAN);
            }
        }
        // In each town, now and then a quarrel between two out in the street.
        let pct = self.rules.brawl_pct;
        if pct <= 0.0 || self.fists().is_none() {
            return;
        }
        for k in 0..self.towns.len() {
            if self.rng.f32() * 100.0 >= pct / 60.0 {
                continue;
            }
            let (tx, ty, r2) = self.towns[k];
            let here: Vec<usize> = (0..self.ents.high)
                .filter(|&s| self.townsperson(s) && self.brawl[s].is_none() && self.foe[s].is_none() && tick >= self.calm[s])
                .filter(|&s| (self.ents.x[s] - tx).powi(2) + (self.ents.y[s] - ty).powi(2) <= r2)
                .collect();
            if here.len() < 2 {
                continue;
            }
            let a = here[self.rng.below(here.len() as u32) as usize];
            let Some(&b) = here.iter().filter(|&&b| b != a && self.d2(a, b) <= QUARREL_REACH * QUARREL_REACH).min_by(|&&p, &&q| self.d2(a, p).total_cmp(&self.d2(a, q))) else { continue };
            // Nobody squares up under the nose of the watch.
            if (0..self.ents.high).any(|w| self.is_watch(w) && self.d2(w, a) <= WATCH_SEES * WATCH_SEES) {
                continue;
            }
            let until = tick + (10 + self.rng.below(7)) * hz;
            self.brawl[a] = Some((self.ents.id_of(b), until));
            self.brawl[b] = Some((self.ents.id_of(a), until));
            let (la, lb) = (pick(self, INSULTS), pick(self, INSULTS));
            self.npc_chat.push((a, la));
            self.npc_chat.push((b, lb));
        }
    }

    fn town_thieves(&mut self) {
        let pct = self.rules.lift_pct;
        if pct <= 0.0 {
            return;
        }
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] != NONE || self.ents.fallen[s] || self.ents.hidden[s] || !self.is_thief(s) {
                continue;
            }
            if tick < self.lift_at[s] || self.slip[s].is_some() {
                continue;
            }
            // Its mark: the nearest player with coin, or townsperson, close enough to touch.
            let mark = (0..self.ents.high)
                .filter(|&t| t != s && self.d2(s, t) <= LIFT_REACH * LIFT_REACH)
                .filter(|&t| {
                    let player = self.ents.alive[t] && self.ents.owner[t] < DRIVEN && !self.ents.god[t] && !self.ents.fallen[t] && self.money(t) > 0;
                    player || self.townsperson(t)
                })
                .min_by(|&a, &b| self.d2(s, a).total_cmp(&self.d2(s, b)));
            let Some(t) = mark else { continue };
            if self.rng.f32() * 100.0 >= pct {
                continue;
            }
            self.lift_at[s] = tick + 20 * hz;
            let player = self.ents.owner[t] < DRIVEN;
            // Not every hand is quick enough: a fumble is caught half the time, and the cry goes up all the same.
            let fumbled = self.rng.f32() < 0.25;
            if fumbled && self.rng.f32() < 0.5 {
                if player {
                    self.tells.push((t, format!("You catch {}'s hand at your purse!", self.called(s))));
                } else {
                    let line = pick(self, NOTICED);
                    self.npc_chat.push((t, line));
                }
            } else if !fumbled {
                let n = if player {
                    let Some(c) = self.currency else { continue };
                    let n = 1 + self.rng.below(self.money(t).min(5));
                    if !crate::market::take(&mut self.inv[t], c as u8 + 1, n) {
                        continue;
                    }
                    self.inv_ver[t] = self.inv_ver[t].wrapping_add(1);
                    self.tells.push((t, format!("Your purse is {n} {} lighter: {} is off with it! Catch them and you'll have it back.", self.items[c].shown(), self.called(s))));
                    n
                } else {
                    let line = pick(self, CRY);
                    self.npc_chat.push((t, line));
                    1 + self.rng.below(3)
                };
                self.purse[s] = self.purse[s].saturating_add(n);
            } else {
                continue; // a fumble nobody noticed
            }
            self.wanted[s] = tick + 60 * hz;
            self.slip[s] = Some((self.ents.x[t], self.ents.y[t], tick + 8 * hz));
        }
    }

    fn town_watch(&mut self) {
        let tick = self.tick;
        for t in 0..self.ents.high {
            if !self.ents.alive[t] || tick >= self.wanted[t] || self.ents.fallen[t] {
                continue;
            }
            let id = self.ents.id_of(t);
            if (0..self.ents.high).any(|w| self.collar[w] == Some(id)) {
                continue;
            }
            let near = (0..self.ents.high)
                .filter(|&w| self.is_watch(w) && self.collar[w].is_none() && self.d2(w, t) <= WATCH_CHASES * WATCH_CHASES)
                .min_by(|&a, &b| self.d2(a, t).total_cmp(&self.d2(b, t)));
            if let Some(w) = near {
                self.collar[w] = Some(id);
                let line = pick(self, STOP);
                self.npc_chat.push((w, line));
            }
        }
    }

    /// A cutpurse fell: what it took is on the ground where it lies.
    pub(crate) fn drop_purse(&mut self, s: usize) {
        let n = std::mem::take(&mut self.purse[s]);
        self.wanted[s] = 0;
        self.slip[s] = None;
        if let (Some(c), true) = (self.currency, n > 0) {
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            self.put_ground(x, y, c, n.min(255) as u8);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{Sim, NONE};
    use proto::defs::ATTR_HEALTH;
    use proto::game::{Behaviour, GameFile, Place, Work};
    use proto::world::{Aabb, World};
    use std::sync::Arc;

    /// A town with houses, townsfolk, one of the watch and a cutpurse; the game's first action is the blow.
    fn town(brawl: f32, lift: f32) -> (Sim, [u8; 3]) {
        let mut g = GameFile::default_game();
        (g.rules.brawl_pct, g.rules.lift_pct) = (brawl.min(100.0), lift);
        g.places.push(Place { name: "town".into(), x: 500.0, y: 500.0, r: 60.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: true });
        let base = g.archetypes[0].clone();
        let blow = Some(g.actions[0].name.clone());
        let mut folk = base.clone();
        (folk.name, folk.behaviour, folk.attack, folk.count, folk.home, folk.rules) = ("folk".into(), Behaviour::Idle, None, 0, Some("town".into()), vec![]);
        let mut watch = folk.clone();
        (watch.name, watch.attack, watch.work) = ("watch".into(), blow.clone(), Some(Work::Streets));
        let mut thief = folk.clone();
        (thief.name, thief.thief) = ("cutpurse".into(), true);
        g.archetypes.extend([folk, watch, thief]);
        let n = g.archetypes.len() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        s.terrain.houses = Arc::new(vec![Aabb { x0: 520.0, y0: 520.0, x1: 530.0, y1: 530.0 }]);
        s.find_towns();
        s.rules.brawl_pct = brawl; // past what a file may say, so a test need not wait on luck
        (s, [n - 3, n - 2, n - 1])
    }

    #[test]
    fn two_townsfolk_come_to_blows_till_one_gives_in_and_the_watch_breaks_up_the_next() {
        let (mut s, [folk, watch, _]) = town(6000.0, 0.0);
        let a = s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        let b = s.spawn_as(NONE, folk, Some((503.0, 500.0))).unwrap();
        s.system_town();
        assert!(s.brawl[a].is_some() && s.brawl[b].is_some(), "a quarrel");
        let mut hurt = false;
        for _ in 0..20 * 20 {
            s.step(&[]);
            hurt |= s.ents.attrs[ATTR_HEALTH as usize][a] < 100.0 || s.ents.attrs[ATTR_HEALTH as usize][b] < 100.0;
            if s.brawl[a].is_none() {
                break;
            }
        }
        assert!(hurt, "blows were struck");
        assert!(s.brawl[a].is_none() && s.brawl[b].is_none(), "and it ended");
        assert!(!s.ents.fallen[a] && !s.ents.fallen[b], "nobody killed over it");
        // Two minutes on they are at it again, and the watch coming by breaks it up at once.
        for (slot, x) in [(a, 500.0), (b, 503.0)] {
            s.ents.attrs[ATTR_HEALTH as usize][slot] = 100.0;
            (s.ents.x[slot], s.ents.y[slot]) = (x, 500.0);
        }
        s.system_town();
        assert!(s.brawl[a].is_none(), "not straight away");
        s.tick += 121 * s.tick_hz;
        s.system_town();
        assert!(s.brawl[a].is_some(), "another quarrel");
        let w = s.spawn_as(NONE, watch, Some((506.0, 500.0))).unwrap();
        s.system_town();
        assert!(s.brawl[a].is_none(), "the watch stepped in");
        assert!(s.npc_chat.iter().any(|(by, line)| *by == w && line.contains('!')), "and said so");
    }

    #[test]
    fn a_cutpurse_lifts_coin_runs_from_the_watch_and_drops_it_when_caught() {
        let (mut s, [_, watch, thief]) = town(0.0, 100.0);
        let coin = s.currency.unwrap_or(0);
        s.currency = Some(coin);
        let me = s.spawn(0).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (500.0, 500.0);
        s.inv[me][0] = (coin as u8 + 1, 20);
        let t = s.spawn_as(NONE, thief, Some((501.5, 500.0))).unwrap();
        let w = s.spawn_as(NONE, watch, Some((540.0, 500.0))).unwrap();
        // A fumble is a matter of luck: give it a few tries.
        for _ in 0..40 {
            s.lift_at[t] = 0;
            s.slip[t] = None;
            (s.ents.x[t], s.ents.y[t]) = (501.5, 500.0);
            s.system_town();
            if s.purse[t] > 0 {
                break;
            }
        }
        let took = s.purse[t];
        assert!(took > 0 && s.money(me) == 20 - took, "lifted {took}, left {}", s.money(me));
        assert!(s.tells.iter().any(|(p, l)| *p == me && l.contains("lighter")), "and the player knows it");
        assert!(s.collar[w].is_some(), "the watch is after it");
        // It runs, the watch chases, and when it falls the purse is on the ground.
        let start = (s.ents.x[t] - 500.0).hypot(s.ents.y[t] - 500.0);
        for _ in 0..20 * 3 {
            s.step(&[]);
        }
        assert!((s.ents.x[t] - 500.0).hypot(s.ents.y[t] - 500.0) > start + 5.0, "away from its mark");
        s.ents.attrs[ATTR_HEALTH as usize][t] = 0.0;
        s.step(&[]);
        assert_eq!(s.purse[t], 0);
        assert!(!s.ground.is_empty(), "the purse lies where it fell");
    }
}

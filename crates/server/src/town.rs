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


/// A line of `occasion` (`GameFile::talk`) with no holes to fill but `{place}`, for bot `s` to say.
fn pick(sim: &mut Sim, s: usize, occasion: &str) -> Option<(usize, String)> {
    let place = sim.town_near(sim.ents.x[s], sim.ents.y[s]);
    sim.say_line(occasion, &[("place", &place)]).map(|l| (s, l))
}

impl Sim {
    /// A bot with a home and no weapon, out and about: one of the townsfolk (not a dog, nor a stray).
    pub(crate) fn townsperson(&self, s: usize) -> bool {
        self.ents.alive[s]
            && self.ents.owner[s] == NONE
            && !self.ents.hidden[s]
            && !self.ents.fallen[s]
            && self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.attack.is_none() && !a.thief && !a.pet && !a.stray && !a.homes.is_empty())
    }

    pub(crate) fn is_thief(&self, s: usize) -> bool {
        self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.thief)
    }

    pub(crate) fn is_thief_type(&self, s: usize) -> bool {
        self.is_thief(s)
    }

    pub(crate) fn is_watch(&self, s: usize) -> bool {
        self.ents.alive[s] && self.ents.owner[s] == NONE && !self.ents.fallen[s] && self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.work == Some(Work::Streets) && a.attack.is_some())
    }

    pub(crate) fn d2(&self, a: usize, b: usize) -> f32 {
        (self.ents.x[a] - self.ents.x[b]).powi(2) + (self.ents.y[a] - self.ents.y[b]).powi(2)
    }

    /// What a brawl is fought with: the blow the first armed bot type uses.
    fn fists(&self) -> Option<u8> {
        self.archs.iter().find_map(|a| a.attack)
    }

    /// A spawn takes nothing of the last one in the slot.
    pub(crate) fn town_reset(&mut self, s: usize) {
        (self.brawl[s], self.purse[s], self.wanted[s], self.slip[s], self.collar[s], self.lift_at[s], self.calm[s]) = (None, 0, 0, None, None, 0, 0);
        (self.talk_at[s], self.heard[s]) = (0, Vec::new());
        self.met[s].clear();
        self.folk[s] = Default::default();
        (self.plea[s], self.plea_at[s], self.lit_sent[s]) = (None, 0, 0);
        self.repute[s].clear();
        self.spoils[s].clear();
        self.skill_lv[s].clear();
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
        if self.plague_steer(s) || self.folk_steer(s) {
            return true;
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
                self.crowd_round(s, o);
                continue;
            }
            if let Some(w) = watch {
                if let Some(l) = pick(self, w, "break_up") {
                self.npc_chat.push(l);
            }
            } else {
                if let Some(l) = pick(self, s, "give_in") {
                self.npc_chat.push(l);
            }
                if let Some(l) = pick(self, o, "won") {
                self.npc_chat.push(l);
            }
                let (who, other) = (self.news_name(o), self.news_name(s));
                self.tell_news("news_brawl", who, other, self.ents.x[s], self.ents.y[s]);
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
                .filter(|&s| self.adult(s) && self.brawl[s].is_none() && self.foe[s].is_none() && tick >= self.calm[s])
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
            self.start_brawl(a, b);
        }
    }

    /// Two townsfolk come to blows, with words first.
    pub(crate) fn start_brawl(&mut self, a: usize, b: usize) {
        let until = self.tick + (10 + self.rng.below(7)) * self.tick_hz.max(1);
        self.brawl[a] = Some((self.ents.id_of(b), until));
        self.brawl[b] = Some((self.ents.id_of(a), until));
        if let Some(l) = pick(self, a, "insult") {
            self.npc_chat.push(l);
        }
        if let Some(l) = pick(self, b, "insult") {
            self.npc_chat.push(l);
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
                    if let Some(l) = pick(self, t, "noticed") {
                self.npc_chat.push(l);
            }
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
                    if let Some(l) = pick(self, t, "cry") {
                self.npc_chat.push(l);
            }
                    1 + self.rng.below(3)
                };
                let who = self.news_name(t);
                self.tell_news("news_theft", who, String::new(), self.ents.x[t], self.ents.y[t]);
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
                if let Some(l) = pick(self, w, "stop") {
                self.npc_chat.push(l);
            }
            }
        }
    }

    /// Townsfolk near a brawl stop to watch it, and now and then one calls out.
    fn crowd_round(&mut self, a: usize, b: usize) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        let (mx, my) = ((self.ents.x[a] + self.ents.x[b]) / 2.0, (self.ents.y[a] + self.ents.y[b]) / 2.0);
        let near: Vec<usize> = (0..self.ents.high)
            .filter(|&t| t != a && t != b && self.townsperson(t) && self.brawl[t].is_none())
            .filter(|&t| (self.ents.x[t] - mx).powi(2) + (self.ents.y[t] - my).powi(2) <= 12.0 * 12.0)
            .collect();
        for t in near {
            self.wander_wait[t] = self.wander_wait[t].max(tick + 2 * hz);
            self.wander_to[t] = (f32::NAN, f32::NAN);
            if tick >= self.ents.say_at[t] && self.rng.f32() < 0.12 {
                self.ents.say_at[t] = tick + 8 * hz;
                if let Some(l) = pick(self, t, "cheer") {
                self.npc_chat.push(l);
            }
            }
        }
    }

    /// A cutpurse fell: what it took is on the ground where it lies, and whoever brought it down (`by`, a player, or
    /// else one of the watch after it) has the thanks of the town and its talk.
    pub(crate) fn drop_purse(&mut self, s: usize, by: Option<usize>) {
        let n = std::mem::take(&mut self.purse[s]);
        let was_wanted = self.tick < self.wanted[s] || n > 0;
        self.wanted[s] = 0;
        self.slip[s] = None;
        if self.is_thief(s) && was_wanted {
            let id = self.ents.id_of(s);
            let watch = (0..self.ents.high).find(|&w| self.collar[w] == Some(id));
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            match (by, watch) {
                (Some(p), _) => {
                    let (name, place) = (self.name_of(p).to_string(), self.town_near(x, y));
                    if let Some(w) = watch.or_else(|| (0..self.ents.high).filter(|&w| self.is_watch(w)).min_by(|&a, &b| self.d2(a, s).total_cmp(&self.d2(b, s))).filter(|&w| self.d2(w, s) <= WATCH_CHASES * WATCH_CHASES)) {
                        if let Some(line) = self.say_line("thanks", &[("name", &name), ("place", &place)]) {
                            self.npc_chat.push((w, line));
                        }
                    }
                    self.tell_news("news_caught", name, String::new(), x, y);
                    if let Some(h) = self.town_at(x, y).or_else(|| self.town_of_bot(s)) {
                        self.change_repute(p, h, 15);
                    }
                }
                (None, Some(w)) => {
                    let who = self.news_name(w);
                    self.tell_news("news_caught", who, String::new(), x, y);
                }
                _ => {}
            }
        }
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
        assert!(s.npc_chat.iter().any(|(by, line)| *by == w && s.talk.lines("break_up").contains(line)), "and said so");
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

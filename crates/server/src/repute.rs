//! How each town thinks of a player, by town (`Sim::repute`, saved by place name in `store::Record::towns`).
//! Catching a cutpurse, answering a plea for help or a quest for one of its folk, and beating what attacks it raise
//! it; striking its folk lowers it (and sets the watch on you a while). Word carries: a third of every change
//! reaches the towns round about. A town's friends pay less and are greeted warmly, its hero is cheered in the
//! street; one it distrusts pays more, and one it shuns is turned away from its shops and followed by the watch.
//! Bad standing heals a point a minute.
//!
//! Pleas: now and then (`Rules::plea_pct`) a townsperson with trouble close by asks a player near them for help: a
//! beast at the fields (drive it off: it must fall), a cutpurse on the run (catch them), or a beast of theirs that
//! has strayed (`Archetype::stray`: one is put out past the fields, and it follows whoever comes for it). Done in
//! time, they are paid in coin and the town thinks better of them.

use crate::sim::{Sim, DRIVEN, NONE};
use proto::game::Behaviour;

pub const REPUTE_MAX: i32 = 100;
/// How far word of a deed carries to other towns, at a third of its weight.
const WORD_CARRIES: f32 = 1500.0;
/// How long a plea stands, and how near the trouble must be to the one who asks.
const PLEA_SECS: u32 = 240;
const TROUBLE_REACH: f32 = 45.0;
const ASK_REACH: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Shunned,
    Cold,
    Plain,
    Friend,
    Hero,
}

impl Tier {
    pub fn of(v: i32) -> Tier {
        match v {
            i32::MIN..=-50 => Tier::Shunned,
            -49..=-20 => Tier::Cold,
            -19..=19 => Tier::Plain,
            20..=59 => Tier::Friend,
            _ => Tier::Hero,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Tier::Shunned => "shunned",
            Tier::Cold => "distrusted",
            Tier::Plain => "known",
            Tier::Friend => "a friend",
            Tier::Hero => "a hero",
        }
    }

    /// What its shops charge, as a share of the price.
    pub fn price(self) -> f32 {
        match self {
            Tier::Shunned | Tier::Cold => 1.25,
            Tier::Plain => 1.0,
            Tier::Friend => 0.9,
            Tier::Hero => 0.8,
        }
    }
}

/// What a player was asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PleaKind {
    /// Bring down this beast or cutpurse (an entity id).
    Beat(u32),
    /// Lead this stray (an entity id) back into the town.
    Fetch(u32),
    /// Cure this one of the sickness (an entity id): a healing draught used beside them (`plague.rs`).
    Cure(u32),
}

#[derive(Clone, Debug)]
pub struct Plea {
    pub kind: PleaKind,
    /// Who asked (an entity id), their name, and the town.
    pub from: u32,
    pub from_name: String,
    pub town: usize,
    pub until: u32,
}

impl Sim {
    /// The town at (x, y): a town place it is inside (the nearest middle, where they overlap).
    pub(crate) fn town_at(&self, x: f32, y: f32) -> Option<usize> {
        self.town_index
            .iter()
            .copied()
            .filter(|&h| (self.places[h].x - x).powi(2) + (self.places[h].y - y).powi(2) <= self.places[h].r * self.places[h].r)
            .min_by(|&a, &b| {
                let d = |h: usize| (self.places[h].x - x).powi(2) + (self.places[h].y - y).powi(2);
                d(a).total_cmp(&d(b))
            })
    }

    /// The town a bot belongs to: its home, if that is a town, else the one it stands in.
    pub(crate) fn town_of_bot(&self, s: usize) -> Option<usize> {
        self.home_of(s).filter(|h| self.town_index.contains(h)).or_else(|| self.town_at(self.ents.x[s], self.ents.y[s]))
    }

    pub fn repute(&self, p: usize, h: usize) -> i32 {
        let name = &self.places[h].name;
        self.repute[p].iter().find(|e| &e.0 == name).map_or(0, |e| e.1)
    }

    pub fn tier(&self, p: usize, h: usize) -> Tier {
        Tier::of(self.repute(p, h))
    }

    fn set_repute(&mut self, p: usize, h: usize, v: i32) {
        let name = self.places[h].name.clone();
        let v = v.clamp(-REPUTE_MAX, REPUTE_MAX);
        let list = &mut self.repute[p];
        match list.iter_mut().find(|e| e.0 == name) {
            Some(e) => e.1 = v,
            None => list.push((name, v)),
        }
        list.retain(|e| e.1 != 0);
    }

    /// Town `h` thinks `delta` better (or worse) of player `p`, and a third of it reaches the towns round about.
    pub(crate) fn change_repute(&mut self, p: usize, h: usize, delta: i32) {
        if delta == 0 || self.ents.owner[p] >= DRIVEN || h >= self.places.len() {
            return;
        }
        let (hx, hy) = (self.places[h].x, self.places[h].y);
        let towns: Vec<usize> = self.town_index.clone();
        for t in towns {
            let d = if t == h {
                delta
            } else if (self.places[t].x - hx).hypot(self.places[t].y - hy) <= WORD_CARRIES {
                delta / 3
            } else {
                0
            };
            if d == 0 {
                continue;
            }
            let (was, now) = (self.repute(p, t), self.repute(p, t) + d);
            self.set_repute(p, t, now);
            let (before, after) = (Tier::of(was), Tier::of(now.clamp(-REPUTE_MAX, REPUTE_MAX)));
            if before != after && t == h {
                let place = self.place_called(t);
                let name = self.name_of(p).to_string();
                self.tells.push((p, format!("Folk in {place} think you {} now.", after.word())));
                match after {
                    a if a >= Tier::Friend && after > before => self.tell_news("news_hero", name, String::new(), hx, hy),
                    Tier::Shunned | Tier::Cold if after < before => self.tell_news("news_villain", name, String::new(), hx, hy),
                    _ => {}
                }
            }
        }
    }

    /// Player `p` struck one of the townsfolk `t`: the town thinks worse of them, and the watch is after them a while.
    pub(crate) fn struck_townsfolk(&mut self, p: usize, t: usize, killed: bool) {
        if self.ents.owner[p] >= DRIVEN || self.ents.god[p] || self.ents.owner[t] != NONE || !(self.townsperson(t) || self.is_watch(t)) {
            return;
        }
        let Some(h) = self.town_of_bot(t) else { return };
        self.change_repute(p, h, if killed { -25 } else { -6 });
        self.wanted[p] = self.wanted[p].max(self.tick + 30 * self.tick_hz.max(1));
    }

    /// The `/rep` lines for towns: each the player is known in.
    pub fn repute_line(&self, p: usize) -> String {
        let mut known: Vec<String> = self
            .repute[p]
            .iter()
            .filter_map(|(n, v)| {
                let h = self.places.iter().position(|pl| &pl.name == n)?;
                Some(format!("{} {v} ({})", self.place_called(h), Tier::of(*v).word()))
            })
            .collect();
        if known.is_empty() {
            return "No town knows you yet.".into();
        }
        known.sort();
        format!("Towns: {}", known.join(", "))
    }

    /// Once a minute: bad standing heals a point.
    pub(crate) fn repute_forgive(&mut self) {
        for list in self.repute.iter_mut() {
            for e in list.iter_mut() {
                if e.1 < 0 {
                    e.1 += 1;
                }
            }
            list.retain(|e| e.1 != 0);
        }
    }

    /// A shop's price for player `p` from merchant `t`: the town's mood and, for food, the season. None: they will
    /// not serve them.
    pub(crate) fn price_for(&self, p: usize, t: usize, item: usize, price: u32) -> Option<u32> {
        let tier = self.town_of_bot(t).map_or(Tier::Plain, |h| self.tier(p, h));
        if tier == Tier::Shunned {
            return None;
        }
        let f = tier.price() * self.season_price(item);
        Some(((price as f32 * f).round() as u32).max(if price > 0 { 1 } else { 0 }))
    }

    /// Once a second: the watch keeps an eye on those its town shuns, pleas are made, and those done are paid.
    pub(crate) fn system_repute(&mut self) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        // The watch lets go of anyone it need not follow any more.
        for w in 0..self.ents.high {
            let Some(p) = self.ents.resolve(self.folk[w].shadow) else {
                self.folk[w].shadow = 0;
                continue;
            };
            let still = self.town_at(self.ents.x[p], self.ents.y[p]).is_some_and(|h| self.tier(p, h) == Tier::Shunned) && self.d2(w, p) <= 60.0 * 60.0;
            if !still || !self.is_watch(w) {
                self.folk[w].shadow = 0;
            } else if tick >= self.ents.say_at[w] && self.rng.f32() < 0.05 {
                self.ents.say_at[w] = tick + 10 * hz;
                if let Some(l) = self.pick_line(w, "watch_warn", &[]) {
                    self.npc_chat.push(l);
                }
            }
        }
        for p in 0..self.ents.high {
            if !self.ents.alive[p] || self.ents.owner[p] >= DRIVEN || self.ents.fallen[p] {
                continue;
            }
            let id = self.ents.id_of(p);
            if self.town_at(self.ents.x[p], self.ents.y[p]).is_some_and(|h| self.tier(p, h) == Tier::Shunned) {
                if !(0..self.ents.high).any(|w| self.folk[w].shadow == id) {
                    let w = (0..self.ents.high)
                        .filter(|&w| self.is_watch(w) && self.folk[w].shadow == 0 && self.collar[w].is_none() && self.d2(w, p) <= 50.0 * 50.0)
                        .min_by(|&a, &b| self.d2(a, p).total_cmp(&self.d2(b, p)));
                    if let Some(w) = w {
                        self.folk[w].shadow = id;
                    }
                }
            }
            self.plea_tick(p);
        }
    }

    /// A player's plea, once a second: done, given up, or (now and then) a new one.
    fn plea_tick(&mut self, p: usize) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        if let Some(pl) = self.plea[p].clone() {
            match pl.kind {
                PleaKind::Beat(t) => {
                    if self.ents.resolve(t).is_none_or(|t| self.ents.fallen[t] || !self.ents.alive[t]) && tick < pl.until {
                        // It fell without them: whoever brought it down, the town is rid of it (paid only if it was them,
                        // in `plea_kill`).
                        self.plea[p] = None;
                    }
                }
                PleaKind::Cure(t) => {
                    // Over it some other way (a doctor, its own time) or gone: no longer asked.
                    if self.ents.resolve(t).is_none_or(|t| !self.is_sick(t) || !self.ents.alive[t]) {
                        self.plea[p] = None;
                    }
                }
                PleaKind::Fetch(t) => match self.ents.resolve(t) {
                    Some(t) => {
                        // It follows whoever comes for it, once they are beside it.
                        if self.folk[t].lead == 0 && self.d2(t, p) <= 5.0 * 5.0 {
                            self.folk[t].lead = self.ents.id_of(p);
                            self.tells.push((p, format!("The {} follows you. Lead it back into {}.", self.name_of(t).to_lowercase(), self.place_called(pl.town))));
                        }
                        if self.town_at(self.ents.x[t], self.ents.y[t]) == Some(pl.town) && self.folk[t].lead != 0 {
                            self.ents.despawn(t);
                            self.plea_done(p);
                            return;
                        }
                    }
                    None => self.plea[p] = None,
                },
            }
            if tick >= pl.until {
                self.plea[p] = None;
                self.tells.push((p, format!("{} has given up waiting for your help.", pl.from_name)));
                if let PleaKind::Fetch(t) = pl.kind {
                    if let Some(t) = self.ents.resolve(t) {
                        self.ents.despawn(t);
                    }
                }
            }
            return;
        }
        if tick < self.plea_at[p] || self.rules.plea_pct <= 0.0 || self.rng.f32() * 100.0 >= self.rules.plea_pct / 60.0 {
            return;
        }
        self.plea_at[p] = tick + 90 * hz;
        let Some(t) = (0..self.ents.high)
            .filter(|&t| self.adult(t) && self.given[t].is_some() && self.town_of_bot(t).is_some() && self.d2(t, p) <= ASK_REACH * ASK_REACH)
            // The sick ask first (for a cure), then the nearest.
            .min_by(|&a, &b| self.is_sick(b).cmp(&self.is_sick(a)).then(self.d2(a, p).total_cmp(&self.d2(b, p))))
        else {
            return;
        };
        let Some(h) = self.town_of_bot(t) else { return };
        if self.tier(p, h) <= Tier::Cold {
            return; // nobody asks a favour of one they do not trust
        }
        // What trouble there is near them: a beast, a cutpurse on the run; else perhaps a stray of theirs.
        let trouble = (0..self.ents.high)
            .filter(|&b| self.ents.alive[b] && self.ents.owner[b] == NONE && !self.ents.fallen[b] && !self.ents.hidden[b] && self.d2(b, t) <= TROUBLE_REACH * TROUBLE_REACH)
            .filter(|&b| (self.tick < self.wanted[b] && self.is_thief(b)) || self.archs.get(self.ents.arch[b] as usize).is_some_and(|a| a.behaviour == Behaviour::Chase && a.attack.is_some()))
            .min_by(|&a, &b| self.d2(a, t).total_cmp(&self.d2(b, t)));
        let from_name = self.given[t].as_deref().unwrap_or("").to_string();
        let name = self.name_of(p).to_string();
        let (kind, line) = if self.is_sick(t) {
            (PleaKind::Cure(self.ents.id_of(t)), self.pick_line(t, "plea_cure", &[("name", &name)]))
        } else if let Some(b) = trouble {
            let what = self.name_of(b).to_lowercase();
            let occasion = if self.is_thief(b) { "plea_thief" } else { "plea_beast" };
            (PleaKind::Beat(self.ents.id_of(b)), self.pick_line(t, occasion, &[("other", &what), ("name", &name)]))
        } else {
            let Some(stray) = self.archs.iter().position(|a| a.stray) else { return };
            if self.rng.f32() > 0.5 {
                return;
            }
            let (px, py, r) = (self.places[h].x, self.places[h].y, self.places[h].r);
            let at = (0..12).find_map(|_| {
                let ang = self.rng.f32() * std::f32::consts::TAU;
                let d = r + 30.0 + 40.0 * self.rng.f32();
                let (x, y) = (px + ang.cos() * d, py + ang.sin() * d);
                (x > 2.0 && y > 2.0 && x < self.world.size - 2.0 && y < self.world.size - 2.0 && !self.world.blocked_by_walls(x, y, self.defs.entity_radius)).then_some((x, y))
            });
            let Some(at) = at else { return };
            let Some(b) = self.spawn_as(NONE, stray as u8, Some(at)) else { return };
            self.ents.fleeting[b] = true;
            let what = self.name_of(b).to_lowercase();
            (PleaKind::Fetch(self.ents.id_of(b)), self.pick_line(t, "plea_stray", &[("other", &what), ("name", &name)]))
        };
        let Some((_, said)) = line else { return };
        self.npc_chat.push((t, said.clone()));
        self.hold_for_secs(t, p, 6.0);
        let task = match kind {
            PleaKind::Beat(_) => "Bring it down",
            PleaKind::Fetch(_) => "Find it and lead it home",
            PleaKind::Cure(_) => "Use a healing draught beside them",
        };
        self.tells.push((p, format!("{from_name}: \"{said}\" ({task}, and come back to {} for your thanks.)", self.place_called(h))));
        self.plea[p] = Some(Plea { kind, from: self.ents.id_of(t), from_name, town: h, until: tick + PLEA_SECS * hz });
    }

    /// A bot fell: a player who was asked to bring it down, and did (`killers`), is thanked.
    pub(crate) fn plea_kill(&mut self, s: usize, killers: &[usize]) {
        let id = self.ents.id_of(s);
        for &p in killers {
            if self.plea[p].as_ref().is_some_and(|pl| pl.kind == PleaKind::Beat(id)) {
                self.plea_done(p);
            }
        }
    }

    pub(crate) fn plea_done(&mut self, p: usize) {
        let Some(pl) = self.plea[p].take() else { return };
        let name = self.name_of(p).to_string();
        let coin = 5 + self.rng.below(11);
        if let Some(c) = self.currency {
            self.give_or_drop(p, c, coin);
        }
        let thanks = self.say_line("plea_thanks", &[("name", &name)]).unwrap_or_else(|| format!("Thank you, {name}."));
        match self.ents.resolve(pl.from).filter(|&t| self.ents.alive[t] && !self.ents.fallen[t]) {
            Some(t) => self.npc_chat.push((t, thanks.clone())),
            None => {}
        }
        let paid = self.currency.map_or(String::new(), |c| format!(" (+{coin} {})", self.items[c].shown()));
        self.tells.push((p, format!("{}: \"{thanks}\"{paid}", pl.from_name)));
        self.change_repute(p, pl.town, 8);
    }
}

#[cfg(test)]
mod tests {
    use crate::repute::{PleaKind, Tier};
    use crate::sim::{Sim, NONE};
    use proto::game::{Behaviour, GameFile, Place, Work};
    use proto::world::{Aabb, World};
    use std::sync::Arc;

    fn town() -> (Sim, [u8; 4]) {
        let mut g = GameFile::default_game();
        g.rules.plea_pct = 100.0;
        for (n, x) in [("kells", 500.0), ("trim", 900.0)] {
            g.places.push(Place { name: n.into(), x, y: 500.0, r: 60.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false, hold: None });
        }
        let base = g.archetypes[0].clone();
        let blow = Some(g.actions[0].name.clone());
        let mut folk = base.clone();
        (folk.name, folk.behaviour, folk.attack, folk.count, folk.home, folk.rules, folk.names) = ("folk".into(), Behaviour::Idle, None, 0, Some("kells".into()), vec![], vec!["Aoife".into(), "Brian".into()]);
        let mut watch = folk.clone();
        (watch.name, watch.attack, watch.work) = ("watch".into(), blow.clone(), Some(Work::Streets));
        let mut wolf = base.clone();
        (wolf.name, wolf.behaviour, wolf.attack, wolf.count, wolf.home, wolf.rules) = ("wolf".into(), Behaviour::Chase, blow, 0, None, vec![]);
        let mut cow = base.clone();
        (cow.name, cow.behaviour, cow.attack, cow.count, cow.home, cow.rules, cow.stray) = ("cow".into(), Behaviour::Idle, None, 0, None, vec![], true);
        g.archetypes.extend([folk, watch, wolf, cow]);
        let n = g.archetypes.len() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(2048.0, 1, vec![]), 64, 20, 1);
        s.terrain.houses = Arc::new(vec![Aabb { x0: 520.0, y0: 520.0, x1: 530.0, y1: 530.0 }, Aabb { x0: 900.0, y0: 520.0, x1: 910.0, y1: 530.0 }]);
        s.find_towns();
        (s, [n - 4, n - 3, n - 2, n - 1])
    }

    #[test]
    fn striking_townsfolk_turns_a_town_against_you_word_carries_and_its_shops_refuse_you() {
        let (mut s, [folk, ..]) = town();
        let t = s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        let p = s.spawn(0).unwrap();
        let (kells, trim) = (s.places.iter().position(|x| x.name == "kells").unwrap(), s.places.iter().position(|x| x.name == "trim").unwrap());
        for _ in 0..9 {
            s.struck_townsfolk(p, t, false);
        }
        assert_eq!(s.repute(p, kells), -54);
        assert_eq!(s.tier(p, kells), Tier::Shunned);
        assert_eq!(s.repute(p, trim), -18, "word carries, at a third");
        assert!(s.tick < s.wanted[p], "the watch is after them");
        assert!(s.tells.iter().any(|(q, l)| *q == p && l.contains("shunned")), "and they are told");
        assert_eq!(s.price_for(p, t, 0, 10), None, "not served");
        // Saved by name and back again.
        let rec = s.record_of(p);
        assert!(rec.towns.contains(&("kells".to_string(), -54)));
        // A hero pays less.
        s.repute[p].clear();
        s.change_repute(p, kells, 70);
        assert_eq!(s.price_for(p, t, 0, 10), Some(8));
    }

    #[test]
    fn a_townsperson_asks_for_help_with_a_wolf_and_pays_when_it_falls() {
        let (mut s, [folk, _, wolf, _]) = town();
        let coin = s.currency.unwrap_or(0);
        s.currency = Some(coin);
        let t = s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        let w = s.spawn_as(NONE, wolf, Some((520.0, 480.0))).unwrap();
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (505.0, 500.0);
        for _ in 0..2000 {
            s.plea_at[p] = 0;
            s.system_repute();
            if s.plea[p].is_some() {
                break;
            }
        }
        let pl = s.plea[p].clone().expect("a plea");
        assert_eq!(pl.kind, PleaKind::Beat(s.ents.id_of(w)));
        assert!(s.npc_chat.iter().any(|(by, l)| *by == t && l.contains("wolf")), "asked out loud");
        let before = s.money(p);
        s.plea_kill(w, &[p]);
        assert!(s.plea[p].is_none() && s.money(p) > before, "paid");
        assert!(s.repute(p, s.home_of(t).unwrap()) > 0, "and better thought of");
    }

    #[test]
    fn a_stray_follows_whoever_comes_for_it_and_home_is_thanks() {
        let (mut s, [folk, ..]) = town();
        let coin = s.currency.unwrap_or(0);
        s.currency = Some(coin);
        s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (505.0, 500.0);
        for _ in 0..4000 {
            s.plea_at[p] = 0;
            s.system_repute();
            if s.plea[p].is_some() {
                break;
            }
        }
        let Some(PleaKind::Fetch(id)) = s.plea[p].as_ref().map(|pl| pl.kind.clone()) else { panic!("asked to fetch a stray") };
        let cow = s.ents.resolve(id).unwrap();
        assert!((s.ents.x[cow] - 500.0).hypot(s.ents.y[cow] - 500.0) > 60.0, "out past the town");
        (s.ents.x[p], s.ents.y[p]) = (s.ents.x[cow] + 2.0, s.ents.y[cow]);
        s.system_repute();
        assert_eq!(s.folk[cow].lead, s.ents.id_of(p), "it follows");
        let before = s.money(p);
        (s.ents.x[cow], s.ents.y[cow]) = (510.0, 500.0);
        s.system_repute();
        assert!(s.plea[p].is_none() && s.money(p) > before, "home, and thanks");
    }
}

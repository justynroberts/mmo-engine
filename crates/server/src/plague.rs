//! The sickness (`Step::Plague`): it comes to a town, a few of its folk fall ill, and it goes from the sick to those
//! who stand close to them. Each case runs its course and ends in recovery or, for townsfolk, now and then death (a
//! death like any other: talked of, mourned, a newcomer in their place, `folk.rs`). A player it weakens but does not
//! kill. A healing draught used beside the sick cures them (`plague_cure`, from `items.rs`), a plague doctor
//! (`Archetype::healer`) goes about curing them, and the sick ask passing players for a cure (`PleaKind::Cure`). The
//! town's shops shut while it lasts (`trade.rs`), the news carries it, and clients are told which towns have it on
//! chat channel 13 (`net.rs` `send_plague`) and see the sick cough (a wordless "cough" emote).

use crate::sim::{dir_to, Sim, DRIVEN, NONE};

/// How close the sick must be to pass it on: the next stall, the same doorway, a step along the street.
const SPREAD_REACH: f32 = 8.0;
/// How close a draught must be used to the sick to cure them, and a doctor stand to tend them.
const CURE_REACH: f32 = 3.0;
/// How long a case lasts, in seconds (townsfolk, players).
const ILL_SECS: (u32, u32) = (150, 300);
/// A player it weakens to no less than this share of their health.
const PLAYER_FLOOR: f32 = 0.2;

/// The sickness in one town.
#[derive(Clone, Debug)]
pub struct Plague {
    pub town: usize,
    pub until: u32,
    /// A minute's chance, in percent, of catching it from each sick one stood beside.
    pub spread: f32,
    /// The chance, in percent, that a townsperson's case ends in death.
    pub deaths: f32,
    pub cases: u32,
    pub dead: u32,
    pub cured: u32,
}

impl Sim {
    /// The plague in town `h`, if it has one.
    pub(crate) fn plague_in(&self, h: usize) -> Option<&Plague> {
        self.plagues.iter().find(|p| p.town == h)
    }

    /// Whether a bot's town has the sickness (its shops shut).
    pub(crate) fn plagued_town_of(&self, s: usize) -> Option<usize> {
        self.town_of_bot(s).filter(|&h| self.plague_in(h).is_some())
    }

    /// A merchant in a town with the sickness: shut, and player `p` told so. False if open.
    pub(crate) fn shut_for_plague(&mut self, p: usize, t: usize) -> bool {
        if self.plagued_town_of(t).is_none() {
            return false;
        }
        let line = self.pick_line(t, "shut", &[]).map_or_else(|| "Shut while the sickness lasts.".to_string(), |l| l.1);
        self.tells.push((p, format!("{}: \"{line}\"", self.called(t))));
        true
    }

    pub(crate) fn is_sick(&self, s: usize) -> bool {
        self.sick[s] > 0
    }

    fn is_healer(&self, s: usize) -> bool {
        self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.healer)
    }

    /// The sickness comes to town `h` for `secs`: `first` of its folk fall ill. False if it is there already or the
    /// town has no one to catch it.
    pub(crate) fn start_plague(&mut self, h: usize, secs: u32, spread: f32, deaths: f32, first: u8) -> bool {
        if self.plague_in(h).is_some() {
            return false;
        }
        let mut folk: Vec<usize> = (0..self.ents.high).filter(|&s| self.townsperson(s) && self.town_of_bot(s) == Some(h)).collect();
        if folk.is_empty() {
            return false;
        }
        let until = self.tick + secs * self.tick_hz.max(1);
        self.plagues.push(Plague { town: h, until, spread, deaths, cases: 0, dead: 0, cured: 0 });
        for _ in 0..(first as usize).min(folk.len()) {
            let k = self.rng.below(folk.len() as u32) as usize;
            let s = folk.swap_remove(k);
            self.fall_ill(s, h);
        }
        let (x, y) = (self.places[h].x, self.places[h].y);
        self.tell_news("news_plague", String::new(), String::new(), x, y);
        self.plague_ver = self.plague_ver.wrapping_add(1);
        true
    }

    fn fall_ill(&mut self, s: usize, h: usize) {
        let hz = self.tick_hz.max(1);
        self.sick[s] = self.tick + (ILL_SECS.0 + self.rng.below(ILL_SECS.1 - ILL_SECS.0)) * hz;
        if let Some(p) = self.plagues.iter_mut().find(|p| p.town == h) {
            p.cases += 1;
        }
        if self.ents.owner[s] < DRIVEN {
            self.tells.push((s, "You feel feverish. You've caught the sickness: rest, or find a healing draught.".into()));
        }
    }

    /// A case over, one way or the other: well again (and not to catch it again while this plague lasts).
    fn get_well(&mut self, s: usize) {
        self.sick[s] = 0;
        self.immune[s] = true;
        if self.ents.owner[s] < DRIVEN {
            self.tells.push((s, "The fever breaks. You're over the sickness.".into()));
        }
    }

    /// A healing draught used by player `p`: it cures the nearest of the sick beside them, else them if they are
    /// sick. True if it cured someone (the draught is spent on that, not on its usual effect).
    pub(crate) fn plague_cure(&mut self, p: usize) -> bool {
        let sick = (0..self.ents.high)
            .filter(|&t| t != p && self.ents.alive[t] && self.is_sick(t) && self.ents.owner[t] == NONE && self.d2(p, t) <= CURE_REACH * CURE_REACH)
            .min_by(|&a, &b| self.d2(a, p).total_cmp(&self.d2(b, p)));
        let Some(t) = sick.or_else(|| self.is_sick(p).then_some(p)) else { return false };
        let h = self.town_of_bot(t).or_else(|| self.town_at(self.ents.x[t], self.ents.y[t]));
        self.get_well(t);
        if let Some(pl) = h.and_then(|h| self.plagues.iter_mut().find(|q| q.town == h)) {
            pl.cured += 1;
        }
        if t != p {
            let name = self.name_of(p).to_string();
            if let Some(l) = self.pick_line(t, "cured", &[("name", &name)]) {
                self.npc_chat.push(l);
            }
            let id = self.ents.id_of(t);
            if self.plea[p].as_ref().is_some_and(|pl| pl.kind == crate::repute::PleaKind::Cure(id)) {
                self.plea_done(p);
            } else if let Some(h) = h {
                self.change_repute(p, h, 4);
            }
        }
        true
    }

    /// Once a second: the sickness spreads, weakens, coughs, kills or passes, and plagues end.
    pub(crate) fn system_plague(&mut self) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        // Plagues that have run their course: every case left is over, and the town is clean.
        let mut ended = Vec::new();
        self.plagues.retain(|p| {
            let done = tick >= p.until;
            if done {
                ended.push(p.town);
            }
            !done
        });
        for h in ended {
            for s in 0..self.ents.high {
                if self.is_sick(s) && self.town_of_bot(s).or_else(|| self.town_at(self.ents.x[s], self.ents.y[s])) == Some(h) {
                    self.get_well(s);
                }
            }
            if self.plagues.is_empty() {
                self.immune.iter_mut().for_each(|i| *i = false);
            }
            let (x, y) = (self.places[h].x, self.places[h].y);
            self.tell_news("news_plague_over", String::new(), String::new(), x, y);
            self.plague_ver = self.plague_ver.wrapping_add(1);
        }
        if self.plagues.is_empty() {
            return;
        }
        let health = proto::defs::ATTR_HEALTH as usize;
        let (lo, hi) = (self.defs.attrs[health].min, self.defs.attrs[health].max);
        // (One brought down by it, waiting on the death block, is past all this.)
        let sick: Vec<usize> = (0..self.ents.high).filter(|&s| self.ents.alive[s] && !self.ents.fallen[s] && self.is_sick(s) && self.ents.attrs[health][s] > lo).collect();
        for s in sick {
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            let h = self.town_of_bot(s).or_else(|| self.town_at(x, y));
            let Some(pl) = h.and_then(|h| self.plague_in(h)).cloned() else {
                // Carried out of the town it is in: it runs its course all the same, spreading no further.
                if tick >= self.sick[s] {
                    self.get_well(s);
                }
                continue;
            };
            let player = self.ents.owner[s] < DRIVEN;
            if tick >= self.sick[s] {
                if !player && self.rng.f32() * 100.0 < pl.deaths {
                    // Dead of it: the death block takes it from here (and the news says what did it).
                    self.ents.attrs[health][s] = lo;
                    self.last_hit[s] = None;
                    if let Some(q) = self.plagues.iter_mut().find(|q| q.town == pl.town) {
                        q.dead += 1;
                    }
                } else {
                    self.get_well(s);
                }
                continue;
            }
            // It weakens a player, slowly, to a floor it will not take them under.
            if player {
                let v = &mut self.ents.attrs[health][s];
                *v = (*v - 0.4).max(lo + (hi - lo) * PLAYER_FLOOR).min(*v);
            } else if !self.ents.hidden[s] && tick >= self.ents.say_at[s] && self.rng.f32() < 0.06 {
                self.ents.say_at[s] = tick + 12 * hz;
                self.npc_emotes.push((s, "cough".into()));
                if self.rng.f32() < 0.5 {
                    if let Some(l) = self.pick_line(s, "cough", &[]) {
                        self.npc_chat.push(l);
                    }
                }
            }
            let chance = pl.spread / 100.0 / 60.0;
            // Through the town: a neighbour, a customer, someone in the same pew (any of its folk).
            if !player && self.rng.f32() < chance {
                let well: Vec<usize> = (0..self.ents.high).filter(|&t| t != s && !self.is_sick(t) && !self.immune[t] && self.townsperson(t) && self.town_of_bot(t) == Some(pl.town)).collect();
                if !well.is_empty() {
                    let t = well[self.rng.below(well.len() as u32) as usize];
                    self.fall_ill(t, pl.town);
                }
            }
            // And to anyone who stands beside them, a player too.
            let near: Vec<usize> = (0..self.ents.high)
                .filter(|&t| t != s && !self.is_sick(t) && !self.immune[t] && self.d2(s, t) <= SPREAD_REACH * SPREAD_REACH)
                .filter(|&t| self.townsperson(t) || (self.ents.alive[t] && self.ents.owner[t] < DRIVEN && !self.ents.god[t] && !self.ents.fallen[t]))
                .collect();
            for t in near {
                if self.rng.f32() < chance {
                    self.fall_ill(t, pl.town);
                }
            }
        }
        // The plague doctors: now and then one cures the sick one it stands beside.
        for d in 0..self.ents.high {
            // About a minute's tending to each: one doctor slows the sickness, it does not stop it.
            if !self.ents.alive[d] || self.ents.fallen[d] || !self.is_healer(d) || self.rng.f32() >= 0.015 {
                continue;
            }
            let near = (0..self.ents.high).find(|&t| self.ents.alive[t] && self.is_sick(t) && self.ents.owner[t] == NONE && self.d2(d, t) <= CURE_REACH * CURE_REACH);
            if let Some(t) = near {
                if let Some(h) = self.town_of_bot(t) {
                    if let Some(q) = self.plagues.iter_mut().find(|q| q.town == h) {
                        q.cured += 1;
                    }
                }
                self.get_well(t);
            }
        }
    }

    /// A plague doctor's way: to the nearest of the sick in a town with the plague. True if it steered.
    pub(crate) fn plague_steer(&mut self, s: usize) -> bool {
        if self.plagues.is_empty() || !self.is_healer(s) {
            return false;
        }
        let Some(t) = (0..self.ents.high)
            .filter(|&t| self.ents.alive[t] && !self.ents.hidden[t] && self.is_sick(t) && self.ents.owner[t] == NONE && self.d2(s, t) <= 120.0 * 120.0)
            .min_by(|&a, &b| self.d2(a, s).total_cmp(&self.d2(b, s)))
        else {
            return false;
        };
        let (dx, dy) = (self.ents.x[t] - self.ents.x[s], self.ents.y[t] - self.ents.y[s]);
        (self.ents.dir_x[s], self.ents.dir_y[s]) = if dx * dx + dy * dy <= 2.0 * 2.0 { (0, 0) } else { dir_to(dx, dy, 1.0) };
        true
    }

    /// The towns with the sickness, as clients are told it (chat channel 13): "x,y,r" of each, ";" between.
    pub(crate) fn plague_line(&self) -> String {
        let towns: Vec<String> = self.plagues.iter().map(|p| {
            let pl = &self.places[p.town];
            format!("{},{},{}", pl.x.round() as i32, pl.y.round() as i32, pl.r.round() as i32)
        }).collect();
        format!("P|{}", towns.join(";"))
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{Sim, NONE};
    use proto::game::{Behaviour, GameFile, Place};
    use proto::world::{Aabb, World};
    use std::sync::Arc;

    fn town() -> (Sim, u8) {
        let mut g = GameFile::default_game();
        g.places.push(Place { name: "town".into(), x: 500.0, y: 500.0, r: 60.0, title: "Kells".into(), exit: None, sound: String::new(), music: String::new(), safe: false, hold: None });
        let mut folk = g.archetypes[0].clone();
        (folk.name, folk.behaviour, folk.attack, folk.count, folk.home, folk.rules) = ("folk".into(), Behaviour::Idle, None, 0, Some("town".into()), vec![]);
        folk.names = vec!["Aoife".into(), "Brian".into(), "Ciara".into(), "Dara".into(), "Eoin".into(), "Fiona".into()];
        g.archetypes.push(folk);
        let n = g.archetypes.len() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        s.terrain.houses = Arc::new(vec![Aabb { x0: 505.0, y0: 505.0, x1: 512.0, y1: 512.0 }, Aabb { x0: 530.0, y0: 470.0, x1: 537.0, y1: 477.0 }]);
        s.find_towns();
        s.daylight = proto::daylight::Daylight::Fixed { phase: 0.3 };
        (s, n - 1)
    }

    fn secs(s: &mut Sim, n: u32) {
        for _ in 0..n {
            s.tick += s.tick_hz;
            s.system_plague();
        }
    }

    #[test]
    fn the_sickness_goes_through_a_town_and_to_those_who_stand_close_and_passes_with_its_time() {
        let (mut s, folk) = town();
        let h = s.places.iter().position(|p| p.name == "town").unwrap();
        let folk: Vec<usize> = (0..4).map(|k| s.spawn_as(NONE, folk, Some((500.0 + k as f32 * 9.0, 500.0))).unwrap()).collect();
        assert!(s.start_plague(h, 600, 100.0, 0.0, 1));
        assert!(!s.start_plague(h, 600, 100.0, 0.0, 1), "once in a town at a time");
        assert_eq!(folk.iter().filter(|&&k| s.is_sick(k)).count(), 1, "one falls ill");
        let first = *folk.iter().find(|&&k| s.is_sick(k)).unwrap();
        // A player who stands beside the sick catches it; one who keeps well away does not.
        let (near, far) = (s.spawn(0).unwrap(), s.spawn(1).unwrap());
        (s.ents.x[near], s.ents.y[near]) = (s.ents.x[first] + 1.0, s.ents.y[first]);
        (s.ents.x[far], s.ents.y[far]) = (900.0, 900.0);
        secs(&mut s, 120);
        assert!(folk.iter().filter(|&&k| s.is_sick(k) || s.immune[k]).count() >= 2, "it went through the town");
        assert!(s.is_sick(near) || s.immune[near], "to the player beside them");
        assert!(!s.is_sick(far) && !s.immune[far], "not to one who kept away");
        assert!(s.news.iter().any(|n| n.occasion == "news_plague"), "and it is news");
        assert!(s.plague_line().starts_with("P|500,500,60"));
        // At its end every case is over and the town is clean.
        secs(&mut s, 600);
        assert!(s.plagues.is_empty() && (0..s.ents.high).all(|k| !s.is_sick(k)));
        assert!(s.news.iter().any(|n| n.occasion == "news_plague_over"));
        assert_eq!(s.plague_line(), "P|");
    }

    #[test]
    fn a_case_can_end_in_death_and_the_news_says_the_sickness_did_it() {
        let (mut s, folk) = town();
        let h = s.places.iter().position(|p| p.name == "town").unwrap();
        let a = s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        assert!(s.start_plague(h, 900, 0.0, 100.0, 1));
        assert!(s.is_sick(a));
        secs(&mut s, 310);
        let health = proto::defs::ATTR_HEALTH as usize;
        assert_eq!(s.ents.attrs[health][a], s.defs.attrs[health].min, "dead of it");
        assert_eq!(s.plagues[0].dead, 1);
    }

    #[test]
    fn a_healing_draught_used_beside_the_sick_cures_them_and_a_player_is_weakened_not_killed() {
        let (mut s, folk) = town();
        let h = s.places.iter().position(|p| p.name == "town").unwrap();
        let a = s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        assert!(s.start_plague(h, 900, 0.0, 0.0, 1));
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (501.0, 500.0);
        assert!(s.plague_cure(p), "cured");
        assert!(!s.is_sick(a) && s.immune[a]);
        assert!(s.npc_chat.iter().any(|(t, _)| *t == a), "and thanks them");
        assert!(!s.plague_cure(p), "nothing to cure");
        // A player who catches it: weakened, to a floor.
        s.fall_ill(p, h);
        let health = proto::defs::ATTR_HEALTH as usize;
        secs(&mut s, 280);
        let (lo, hi) = (s.defs.attrs[health].min, s.defs.attrs[health].max);
        assert!(s.ents.attrs[health][p] >= lo + (hi - lo) * 0.2 - 0.01, "not killed by it: {}", s.ents.attrs[health][p]);
    }
}

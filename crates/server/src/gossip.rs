//! Townsfolk who talk to players: a stranger is welcomed and someone they have met is greeted by name; they
//! remark on how a player looks (hurt, out at night, soaked, carrying a fat purse), and pass on the news: what has
//! happened lately in the world (a theft, a cutpurse caught, a brawl, an ambush, a fall, a player growing in
//! strength), nearest first. The lines are `GameFile::talk` (`proto::talk`).

use crate::sim::{Sim, DRIVEN, NONE};
use proto::game::DayPart;
use proto::talk::fill;

/// How many pieces of news are kept, and how long one is worth telling.
const NEWS_KEPT: usize = 48;
const NEWS_SECS: u32 = 20 * 60;
/// How often a player is spoken to at most, and how near a townsperson must be.
const TALK_EVERY_SECS: (u32, u32) = (18, 40);

/// Something that happened, to be told about.
#[derive(Clone, Debug)]
pub struct News {
    pub id: u32,
    pub at: u32,
    pub occasion: &'static str,
    pub who: String,
    pub other: String,
    /// Where (the nearest town), and where exactly, so the news nearest a listener comes first.
    pub place: String,
    pub x: f32,
    pub y: f32,
}

impl Sim {
    /// A place as players know it: its title, else its name.
    pub(crate) fn place_called(&self, h: usize) -> String {
        let p = &self.places[h];
        if p.title.is_empty() {
            let mut c = p.name.chars();
            c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
        } else {
            p.title.clone()
        }
    }

    /// The town nearest (x, y), as players know it ("the wilds" in a world without towns).
    pub(crate) fn town_near(&self, x: f32, y: f32) -> String {
        let near = (0..self.places.len())
            .filter(|&h| self.town_index.contains(&h))
            .min_by(|&a, &b| {
                let d = |h: usize| (self.places[h].x - x).powi(2) + (self.places[h].y - y).powi(2);
                d(a).total_cmp(&d(b))
            });
        near.map_or_else(|| "the wilds".into(), |h| self.place_called(h))
    }

    /// Someone as the news calls them: a player by name, a townsperson by their own name, else "a <kind>".
    pub(crate) fn news_name(&self, s: usize) -> String {
        if self.ents.owner[s] < DRIVEN {
            return self.name_of(s).to_string();
        }
        match self.given_name(s) {
            Some(g) => format!("{g} the {}", self.name_of(s)),
            None => format!("a {}", self.name_of(s).to_lowercase()),
        }
    }

    /// Something happened at (x, y) that townsfolk will talk about.
    pub(crate) fn tell_news(&mut self, occasion: &'static str, who: String, other: String, x: f32, y: f32) {
        self.news_id = self.news_id.wrapping_add(1);
        let place = self.town_near(x, y);
        if self.news.len() >= NEWS_KEPT {
            self.news.remove(0);
        }
        self.news.push(News { id: self.news_id, at: self.tick, occasion, who, other, place, x, y });
    }

    /// A line of `occasion`, holes filled, or None if it has no lines.
    pub(crate) fn say_line(&mut self, occasion: &str, holes: &[(&str, &str)]) -> Option<String> {
        let lines = self.talk.lines(occasion);
        if lines.is_empty() {
            return None;
        }
        let line = lines[self.rng.below(lines.len() as u32) as usize].clone();
        Some(fill(&line, holes))
    }

    fn greeting(&self) -> &'static str {
        match self.daypart {
            DayPart::Dawn => "Good morning",
            DayPart::Day => "Good day",
            DayPart::Dusk => "Good evening",
            DayPart::Night => "You're out late",
        }
    }

    /// Once a second: now and then the nearest townsperson to each player has a word for them.
    pub(crate) fn system_gossip(&mut self) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        self.news.retain(|n| tick.saturating_sub(n.at) < NEWS_SECS * hz);
        let reach = self.rules.remark_reach;
        for p in 0..self.ents.high {
            if !self.ents.alive[p] || self.ents.owner[p] >= DRIVEN || self.ents.fallen[p] || self.ents.hidden[p] || tick < self.talk_at[p] {
                continue;
            }
            if self.rng.f32() > 0.3 {
                continue;
            }
            // The nearest townsperson free to talk, facing nothing more pressing.
            let (px, py) = (self.ents.x[p], self.ents.y[p]);
            let Some(t) = (0..self.ents.high)
                .filter(|&t| self.ents.alive[t] && self.ents.owner[t] == NONE && !self.ents.hidden[t] && !self.ents.fallen[t] && self.given[t].is_some() && !self.is_pet(t))
                .filter(|&t| self.brawl[t].is_none() && self.slip[t].is_none() && self.foe[t].is_none() && !self.archs.get(self.ents.arch[t] as usize).is_some_and(|a| a.thief))
                .filter(|&t| tick >= self.ents.say_at[t])
                .map(|t| ((self.ents.x[t] - px).powi(2) + (self.ents.y[t] - py).powi(2), t))
                .filter(|&(d2, _)| d2 <= reach * reach)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, t)| t)
            else {
                continue;
            };
            let Some(line) = self.word_for(t, p) else { continue };
            self.talk_at[p] = tick + (TALK_EVERY_SECS.0 + self.rng.below(TALK_EVERY_SECS.1 - TALK_EVERY_SECS.0)) * hz;
            self.ents.say_at[t] = tick + 10 * hz;
            self.npc_chat.push((t, line));
            let secs = self.rules.remark_pause_secs;
            self.hold_for_secs(t, p, secs);
        }
    }

    /// What townsperson `t` says to player `p`: a welcome the first time, then news, a remark or a greeting.
    fn word_for(&mut self, t: usize, p: usize) -> Option<String> {
        let name = self.name_of(p).to_string();
        let place = self.town_near(self.ents.x[t], self.ents.y[t]);
        let greeting = self.greeting();
        let holes = |extra: &[(&'static str, String)]| -> Vec<(&'static str, String)> {
            // The news's own holes first: the first of a key fills it (where it happened, not where it is told).
            let mut v = extra.to_vec();
            v.extend([("name", name.clone()), ("place", place.clone()), ("greeting", greeting.to_string())]);
            v
        };
        let say = |sim: &mut Sim, occasion: &str, v: Vec<(&'static str, String)>| {
            let r: Vec<(&str, &str)> = v.iter().map(|(k, s)| (*k, s.as_str())).collect();
            sim.say_line(occasion, &r)
        };
        // What the town thinks of them comes first: one it shuns is told to clear off.
        let tier = self.town_of_bot(t).map_or(crate::repute::Tier::Plain, |h| self.tier(p, h));
        if tier == crate::repute::Tier::Shunned {
            return say(self, "shun", holes(&[]));
        }
        let tid = self.ents.id_of(t);
        if !self.met[p].contains(&tid) {
            if self.met[p].len() >= 256 {
                self.met[p].clear();
            }
            self.met[p].insert(tid);
            // A newcomer is new here too.
            let occasion = if self.tick < self.folk[t].new_till { "newcomer_hello" } else { "greet_new" };
            return say(self, occasion, holes(&[]));
        }
        // News first, the nearest not yet heard; else how they look; else a plain greeting.
        let (px, py) = (self.ents.x[p], self.ents.y[p]);
        let fresh = self
            .news
            .iter()
            .filter(|n| !self.heard[p].contains(&n.id) && n.who != name)
            .min_by(|a, b| ((a.x - px).powi(2) + (a.y - py).powi(2)).total_cmp(&((b.x - px).powi(2) + (b.y - py).powi(2))))
            .cloned();
        if let Some(n) = fresh.filter(|_| self.rng.f32() < 0.6) {
            if self.heard[p].len() >= 64 {
                self.heard[p].remove(0);
            }
            self.heard[p].push(n.id);
            return say(self, n.occasion, holes(&[("who", n.who.clone()), ("other", n.other.clone()), ("place", n.place.clone())]));
        }
        let health = proto::defs::ATTR_HEALTH as usize;
        let hurt = self.ents.attrs[health][p] < self.defs.attrs[health].max * 0.4;
        let wet = self.weather_name_at(px, py).is_some() && self.rng.f32() < 0.5;
        let remark = if hurt {
            Some("hurt")
        } else if self.daypart == DayPart::Night {
            Some("night")
        } else if wet {
            Some("rain")
        } else if self.money(p) >= 50 && self.rng.f32() < 0.5 {
            Some("rich")
        } else if self.season.is_some() && self.rng.f32() < 0.2 {
            self.season.map(|s| s.name())
        } else {
            None
        };
        let greet = match tier {
            crate::repute::Tier::Hero => "greet_hero",
            crate::repute::Tier::Friend => "greet_friend",
            crate::repute::Tier::Cold => "greet_cold",
            _ => "greet_known",
        };
        say(self, remark.unwrap_or(greet), holes(&[]))
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{Sim, NONE};
    use proto::game::{Behaviour, GameFile, Place};
    use proto::world::{Aabb, World};
    use std::sync::Arc;

    #[test]
    fn a_townsperson_welcomes_a_stranger_then_greets_them_by_name_and_tells_the_news() {
        let mut g = GameFile::default_game();
        g.places.push(Place { name: "kells".into(), x: 500.0, y: 500.0, r: 60.0, title: "Ceanannas".into(), exit: None, sound: String::new(), music: String::new(), safe: true, hold: None });
        let mut folk = g.archetypes[0].clone();
        (folk.name, folk.behaviour, folk.attack, folk.count, folk.home, folk.rules, folk.names) = ("folk".into(), Behaviour::Idle, None, 0, Some("kells".into()), vec![], vec!["Aoife".into()]);
        g.archetypes.push(folk);
        let n = g.archetypes.len() as u8 - 1;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        s.terrain.houses = Arc::new(vec![Aabb { x0: 520.0, y0: 520.0, x1: 530.0, y1: 530.0 }]);
        s.find_towns();
        let t = s.spawn_as(NONE, n, Some((500.0, 500.0))).unwrap();
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (503.0, 500.0);
        s.ents.name[p] = Some("Justyn".into());
        let said = |s: &mut Sim| -> String {
            for _ in 0..200 {
                s.talk_at[p] = 0;
                s.ents.say_at[t] = 0;
                s.npc_chat.clear();
                s.system_gossip();
                if let Some((_, l)) = s.npc_chat.iter().find(|(by, _)| *by == t) {
                    return l.clone();
                }
            }
            panic!("never spoke");
        };
        let first = said(&mut s);
        assert!(s.talk.lines("greet_new").iter().any(|l| proto::talk::fill(l, &[("place", "Ceanannas"), ("greeting", "Good day")]) == first), "a welcome: {first}");
        s.places.push(Place { name: "trim".into(), x: 800.0, y: 800.0, r: 40.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false, hold: None });
        s.terrain.houses = Arc::new(vec![Aabb { x0: 520.0, y0: 520.0, x1: 530.0, y1: 530.0 }, Aabb { x0: 800.0, y0: 800.0, x1: 805.0, y1: 805.0 }]);
        s.find_towns();
        s.tell_news("news_caught", "Brian".into(), String::new(), 790.0, 800.0);
        let mut heard = false;
        for _ in 0..20 {
            let line = said(&mut s);
            heard |= line.contains("Brian") && line.contains("Trim");
            assert!(!line.contains('{'), "every hole filled: {line}");
        }
        assert!(heard, "the news, with the name of the town it happened in");
    }
}

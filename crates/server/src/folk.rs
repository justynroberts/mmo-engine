//! The rest of town life (`town.rs` has quarrels and cutpurses). Of an evening townsfolk go to the inn (the house
//! nearest the town's square, `Rules::inn_pct`) and come out late, singing and weaving, ready to quarrel. Children
//! (bot types that work at `play`) play chase about the square. Dogs (`Archetype::pet`) keep at the heel of one of
//! the townsfolk, wait at their door while they are in, and bark at strangers and run at a cutpurse. A townsperson
//! who is killed is talked of, mourned (a few of their neighbours walk from their door to the square, saying a
//! word for them) and, `Rules::newcomer_secs` on, someone new takes their place under another name. An event's
//! `gather` step brings a town out to its square for a market, a feast or a wedding (`Sim::gather`): lines of
//! that occasion, and dancing.
//!
//! Steering for all of it comes first in `town_steer` (`folk_steer`), so it overrides a bot's own rules.

use crate::sim::{dir_to, Sim, DRIVEN, NONE};
use proto::game::{Behaviour, DayPart, Work};

/// Of a merry spell out of the inn, how long is the walk home (the rest is spent by its door).
const MERRY_HOME_SECS: u32 = 70;

/// Where a townsperson is with the inn this evening.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Inn {
    #[default]
    Out,
    /// On the way there, till the tick it gives up.
    Going(u32),
    /// Inside, till.
    In(u32),
    /// Out again, the worse for drink, on the way home, till.
    Merry(u32),
}

/// One bot's share of town life beyond `town.rs`, reset when its slot is spawned into.
#[derive(Clone, Debug, Default)]
pub struct Folk {
    pub inn: Inn,
    /// The day (`Sim::day`) it last made up its mind about the inn.
    pub inn_day: Option<i64>,
    /// A child at play: the other one, whether this one is "it" (chasing), and till when.
    pub play: Option<(u32, bool, u32)>,
    /// A dog's master (an entity id, 0 for none yet), the cutpurse it is after, and when it may bark again.
    pub master: u32,
    pub hunt: u32,
    pub bark_at: u32,
    /// It comes back as someone new (`Rules::newcomer_secs`), and the tick it is still new till.
    pub newcomer: bool,
    pub new_till: u32,
    /// When it may dance again at a gathering.
    pub dance_at: u32,
    /// A stray (`Archetype::stray`): the player leading it home (an entity id).
    pub lead: u32,
    /// One of the watch keeping an eye on a player the town shuns (`repute.rs`).
    pub shadow: u32,
}

/// A town out on its square (`Step::Gather`).
#[derive(Clone, Debug)]
pub struct Gathering {
    pub place: usize,
    pub until: u32,
    pub occasion: String,
    /// The two a wedding is for (or two of the townsfolk there), for `{who}` and `{other}`.
    pub couple: (String, String),
    pub dance: bool,
}

/// A townsperson being buried: mourners walk from their door to the square.
#[derive(Clone, Debug)]
pub struct Funeral {
    pub home: usize,
    pub who: String,
    pub from: (f32, f32),
    pub to: (f32, f32),
    pub start: u32,
    pub until: u32,
    pub mourners: Vec<u32>,
}

/// How long a funeral waits after the death, and how long it lasts.
const FUNERAL_AFTER_SECS: u32 = 30;
const FUNERAL_SECS: u32 = 150;
const MOURNERS: usize = 6;

impl Sim {
    /// Toward something that moves (a master, the mourner ahead, a player): straight at it when it is close, and
    /// otherwise along a flow field to a point snapped to a coarse grid near it, so a moving target does not cost
    /// a new field every step and those after the same one share theirs.
    fn toward_moving(&mut self, s: usize, tx: f32, ty: f32, stop: f32) -> (i8, i8) {
        let (dx, dy) = (tx - self.ents.x[s], ty - self.ents.y[s]);
        if dx * dx + dy * dy <= 24.0 * 24.0 {
            self.nav_goal[s].2 = -1.0;
            return dir_to(dx, dy, stop);
        }
        let snap = |v: f32| ((v / 8.0).floor() + 0.5) * 8.0;
        self.toward_static(s, snap(tx), snap(ty), stop.max(4.0))
    }

    fn arch_of(&self, s: usize) -> Option<&proto::game::ArchRt> {
        self.archs.get(self.ents.arch[s] as usize)
    }

    pub(crate) fn is_child(&self, s: usize) -> bool {
        self.arch_of(s).is_some_and(|a| a.work == Some(Work::Play))
    }

    pub(crate) fn is_pet(&self, s: usize) -> bool {
        self.arch_of(s).is_some_and(|a| a.pet)
    }

    /// A traveller: a bot that walks a road (a `patrol` rule) and has no house in its town.
    pub(crate) fn is_traveller(&self, s: usize) -> bool {
        self.house[s].is_none() && self.arch_of(s).is_some_and(|a| a.rules.iter().any(|r| matches!(r.then, proto::game::ActRt::Patrol { .. })))
    }

    pub(crate) fn is_stray(&self, s: usize) -> bool {
        self.arch_of(s).is_some_and(|a| a.stray)
    }

    /// A grown townsperson out and about, not of the watch: one who goes to the inn, mourns and quarrels.
    pub(crate) fn adult(&self, s: usize) -> bool {
        self.townsperson(s) && !self.is_child(s) && self.arch_of(s).is_some_and(|a| a.work != Some(Work::Streets))
    }

    /// The inn of town `h`: the door of the house nearest the middle of its square (or of the town), and that
    /// house's middle. Worked out once per world.
    pub(crate) fn inn_of(&mut self, h: usize) -> Option<((f32, f32), (f32, f32))> {
        let ptr = std::sync::Arc::as_ptr(&self.terrain.houses) as usize;
        if self.inns_for != ptr {
            self.inns.clear();
            self.inns_for = ptr;
        }
        if let Some(v) = self.inns.get(&h) {
            return *v;
        }
        let mid = self.square_middle(h);
        let house = self
            .houses_in(h)
            .min_by(|a, b| {
                let d = |b: &proto::world::Aabb| ((b.x0 + b.x1) / 2.0 - mid.0).powi(2) + ((b.y0 + b.y1) / 2.0 - mid.1).powi(2);
                d(a).total_cmp(&d(b))
            })
            .copied();
        let v = house.and_then(|b| self.step_of(&b, mid).map(|door| (door, ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0))));
        self.inns.insert(h, v);
        v
    }

    /// Where town `h` buries its dead: beside the church (the world file's tower nearest the town, if one is in or
    /// just outside it), else its square.
    pub(crate) fn churchyard(&self, h: usize) -> (f32, f32) {
        let (px, py, r) = (self.places[h].x, self.places[h].y, self.places[h].r);
        let tower = self
            .terrain
            .towers
            .iter()
            .map(|b| ((b.x0 + b.x1) / 2.0, (b.y0 + b.y1) / 2.0, b))
            .filter(|t| (t.0 - px).hypot(t.1 - py) <= r + 40.0)
            .min_by(|a, b| (a.0 - px).hypot(a.1 - py).total_cmp(&(b.0 - px).hypot(b.1 - py)));
        match tower {
            // A step out from its south wall, where the graves are.
            Some((x, _, b)) => {
                let at = (x, b.y1 + 4.0);
                if self.world.blocked_by_walls(at.0, at.1, self.defs.entity_radius) { (x, b.y0 - 4.0) } else { at }
            }
            None => self.square_middle(h),
        }
    }

    /// Whether the bot in `s` lives in a town out at a gathering now.
    pub(crate) fn festive(&self, s: usize) -> bool {
        let tick = self.tick;
        self.home_of(s).is_some_and(|h| self.gatherings.iter().any(|g| g.place == h && tick < g.until)) && (self.adult(s) || self.is_child(s))
    }

    /// An event's `gather` step at place `h`: its townsfolk come out to the square for `secs`. Returns the
    /// announcement filled in (`{who}` and `{other}` two of the townsfolk there, `{place}` the town), if any.
    pub(crate) fn gather(&mut self, h: usize, secs: u32, occasion: &str, announce: Option<&str>, dance: bool) -> Option<String> {
        let folk: Vec<usize> = (0..self.ents.high).filter(|&s| self.adult(s) && self.home_of(s) == Some(h) && self.given[s].is_some()).collect();
        let pick = |sim: &mut Sim, not: Option<usize>| -> Option<usize> {
            let c: Vec<usize> = folk.iter().copied().filter(|&s| Some(s) != not).collect();
            (!c.is_empty()).then(|| c[sim.rng.below(c.len() as u32) as usize])
        };
        let a = pick(self, None);
        let b = pick(self, a);
        let name = |sim: &Sim, s: Option<usize>| s.and_then(|s| sim.given[s].as_deref().map(str::to_string)).unwrap_or_else(|| "the couple".into());
        let couple = (name(self, a), name(self, b));
        let until = self.tick + secs * self.tick_hz.max(1);
        self.gatherings.retain(|g| g.place != h);
        self.gatherings.push(Gathering { place: h, until, occasion: occasion.to_string(), couple: couple.clone(), dance });
        for &s in &folk {
            // Off to the square now, whatever they were about.
            self.wander_to[s] = (f32::NAN, f32::NAN);
            self.wander_wait[s] = 0;
        }
        let place = self.place_called(h);
        announce.map(|t| proto::talk::fill(t, &[("who", &couple.0), ("other", &couple.1), ("place", &place)]))
    }

    /// The bot in `s` is busy with the life of the town here: its steering is set. False leaves it to the rest.
    pub(crate) fn folk_steer(&mut self, s: usize) -> bool {
        let tick = self.tick;
        let id = self.ents.id_of(s);
        // A mourner: to the door of the dead, then behind the one ahead of them to the square.
        if let Some(f) = self.funerals.iter().find(|f| tick >= f.start && tick < f.until && f.mourners.contains(&id)).cloned() {
            let k = f.mourners.iter().position(|&m| m == id).unwrap_or(0);
            let walking = tick >= f.start + (f.until - f.start) / 4;
            self.ents.hidden[s] = false;
            let d = if !walking {
                self.toward_static(s, f.from.0, f.from.1, 2.5)
            } else if k == 0 {
                self.toward_static(s, f.to.0, f.to.1, 2.0)
            } else {
                match self.ents.resolve(f.mourners[k - 1]) {
                    Some(ahead) if (self.ents.x[ahead] - f.to.0).hypot(self.ents.y[ahead] - f.to.1) > 3.0 => {
                        let (ax, ay) = (self.ents.x[ahead], self.ents.y[ahead]);
                        self.toward_moving(s, ax, ay, 2.2)
                    }
                    _ => self.toward_static(s, f.to.0, f.to.1, 2.5 + k as f32 * 0.8),
                }
            };
            (self.ents.dir_x[s], self.ents.dir_y[s]) = d;
            return true;
        }
        match self.folk[s].inn {
            Inn::Out => {}
            Inn::Going(until) => {
                let inn = self.home_of(s).and_then(|h| self.inn_of(h));
                match inn.filter(|_| tick < until) {
                    Some(((dx, dy), _)) => {
                        let (x, y) = (self.ents.x[s], self.ents.y[s]);
                        if (dx - x).powi(2) + (dy - y).powi(2) <= 3.0 * 3.0 {
                            // In for a while, then out at closing (on the minute), so they come out together.
                            let (stay, minute) = (90 + self.rng.below(150), 60 * self.tick_hz.max(1));
                            self.folk[s].inn = Inn::In((tick + stay * self.tick_hz.max(1)) / minute * minute + minute);
                            self.ents.hidden[s] = true;
                            (self.ents.dir_x[s], self.ents.dir_y[s]) = (0, 0);
                        } else {
                            self.ents.hidden[s] = false;
                            (self.ents.dir_x[s], self.ents.dir_y[s]) = self.toward_static(s, dx, dy, 1.5);
                        }
                        return true;
                    }
                    None => self.folk[s].inn = Inn::Out,
                }
            }
            Inn::In(until) => {
                if tick < until {
                    self.ents.hidden[s] = true;
                    (self.ents.dir_x[s], self.ents.dir_y[s]) = (0, 0);
                    return true;
                }
                self.ents.hidden[s] = false;
                self.folk[s].inn = Inn::Merry(tick + 90 * self.tick_hz.max(1));
            }
            Inn::Merry(until) => {
                // Out at closing: first a while by the inn's door, singing (and quarrelling), then home.
                if tick + MERRY_HOME_SECS * self.tick_hz.max(1) < until {
                    self.ents.hidden[s] = false;
                    (self.ents.dir_x[s], self.ents.dir_y[s]) = (0, 0);
                    return true;
                }
                let door = self.door_of(s);
                let (x, y) = (self.ents.x[s], self.ents.y[s]);
                match door.filter(|_| tick < until) {
                    Some((dx, dy)) if (dx - x).powi(2) + (dy - y).powi(2) > 3.0 * 3.0 => {
                        let mut d = self.toward_static(s, dx, dy, 1.5);
                        // Weaving: now and then a lurch to one side.
                        if self.rng.f32() < 0.4 {
                            let ang = self.rng.f32() * std::f32::consts::TAU;
                            d = dir_to(ang.cos(), ang.sin(), 0.0);
                        }
                        self.ents.hidden[s] = false;
                        (self.ents.dir_x[s], self.ents.dir_y[s]) = d;
                        return true;
                    }
                    _ => self.folk[s].inn = Inn::Out,
                }
            }
        }
        // A child at play.
        if let Some((other, it, until)) = self.folk[s].play {
            match self.ents.resolve(other).filter(|&o| tick < until && self.folk[o].play.is_some() && !self.ents.hidden[o]) {
                Some(o) => {
                    let (dx, dy) = (self.ents.x[o] - self.ents.x[s], self.ents.y[o] - self.ents.y[s]);
                    let d = if it {
                        if dx * dx + dy * dy <= 1.6 * 1.6 {
                            // Tagged: the other is "it" now.
                            self.folk[s].play = Some((other, false, until));
                            self.folk[o].play = Some((id, true, until));
                            self.wander_wait[o] = tick + self.tick_hz;
                            (0, 0)
                        } else {
                            dir_to(dx, dy, 0.8)
                        }
                    } else {
                        // Away, but not out of town: back towards the middle once well out.
                        let home = self.home_of(s).map(|h| (self.places[h].x, self.places[h].y, self.places[h].r));
                        match home {
                            Some((hx, hy, r)) if (self.ents.x[s] - hx).hypot(self.ents.y[s] - hy) > r * 0.5 => dir_to(hx - self.ents.x[s], hy - self.ents.y[s], 0.0),
                            _ => dir_to(-dx, -dy, 0.0),
                        }
                    };
                    self.ents.hidden[s] = false;
                    (self.ents.dir_x[s], self.ents.dir_y[s]) = d;
                    return true;
                }
                None => {
                    self.folk[s].play = None;
                    self.calm[s] = tick + 20 * self.tick_hz.max(1);
                }
            }
        }
        // A dog: after a thief, else at its master's heel or door.
        if self.is_pet(s) {
            if let Some(t) = self.ents.resolve(self.folk[s].hunt).filter(|&t| tick < self.wanted[t] && !self.ents.fallen[t]) {
                let (dx, dy) = (self.ents.x[t] - self.ents.x[s], self.ents.y[t] - self.ents.y[s]);
                (self.ents.dir_x[s], self.ents.dir_y[s]) = dir_to(dx, dy, 1.5);
                self.ents.hidden[s] = false;
                return true;
            }
            self.folk[s].hunt = 0;
            if let Some(m) = self.ents.resolve(self.folk[s].master).filter(|&m| self.ents.alive[m] && !self.ents.fallen[m]) {
                let door = if self.ents.hidden[m] { self.door_of(m) } else { None };
                self.ents.hidden[s] = false;
                (self.ents.dir_x[s], self.ents.dir_y[s]) = match door {
                    Some((dx, dy)) => self.toward_static(s, dx, dy, 1.5),
                    None => {
                        let (mx, my) = (self.ents.x[m], self.ents.y[m]);
                        self.toward_moving(s, mx, my, 3.0)
                    }
                };
                return true;
            }
        }
        // A stray being led home.
        if self.is_stray(s) {
            if let Some(p) = self.ents.resolve(self.folk[s].lead).filter(|&p| self.ents.alive[p] && !self.ents.fallen[p]) {
                let (px, py) = (self.ents.x[p], self.ents.y[p]);
                (self.ents.dir_x[s], self.ents.dir_y[s]) = self.toward_moving(s, px, py, 2.5);
                return true;
            }
        }
        // One of the watch keeping an eye on someone.
        if let Some(p) = self.ents.resolve(self.folk[s].shadow).filter(|&p| self.ents.alive[p] && !self.ents.fallen[p]) {
            let (px, py) = (self.ents.x[p], self.ents.y[p]);
            (self.ents.dir_x[s], self.ents.dir_y[s]) = self.toward_moving(s, px, py, 5.0);
            return true;
        }
        // Out for a gathering on the square.
        if self.festive(s) {
            self.ents.hidden[s] = false;
            let a = self.ents.arch[s] as usize;
            (self.ents.dir_x[s], self.ents.dir_y[s]) = self.wander_dir(s, a);
            return true;
        }
        false
    }

    /// Once a second: to the inn of an evening and home singing, games of chase, dogs, funerals and gatherings.
    pub(crate) fn system_folk(&mut self) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        let today = self.day.floor() as i64;
        let dusk = self.daypart == DayPart::Dusk;
        let day = matches!(self.daypart, DayPart::Dawn | DayPart::Day);
        // The bell as the day comes and as it goes (the first look at the clock rings nothing).
        if let Some(was) = self.bell_seen.replace(self.daypart) {
            if was != self.daypart && matches!(self.daypart, DayPart::Dawn | DayPart::Dusk) {
                self.ring_bell(self.daypart == DayPart::Dawn);
            }
        }
        let mut taken_masters: std::collections::HashSet<u32> = (0..self.ents.high).filter(|&d| self.ents.alive[d] && self.is_pet(d)).map(|d| self.folk[d].master).collect();
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] != NONE || self.ents.fallen[s] {
                continue;
            }
            // The inn: an evening's decision, once a day.
            if dusk && self.folk[s].inn == Inn::Out && self.folk[s].inn_day != Some(today) && self.adult(s) {
                self.folk[s].inn_day = Some(today);
                if self.rng.f32() * 100.0 < self.rules.inn_pct && self.home_of(s).and_then(|h| self.inn_of(h)).is_some() {
                    self.folk[s].inn = Inn::Going(tick + 120 * hz);
                }
            }
            if let Inn::Merry(_) = self.folk[s].inn {
                if tick >= self.ents.say_at[s] && self.rng.f32() < 0.2 {
                    self.ents.say_at[s] = tick + 6 * hz;
                    if let Some(l) = self.pick_line(s, "sing", &[]) {
                        self.npc_chat.push(l);
                    }
                }
                // Two the worse for drink, close together and out of the watch's sight: it may come to blows.
                if self.brawl[s].is_none() && tick >= self.calm[s] && self.rng.f32() < 0.04 {
                    let other = (0..self.ents.high).find(|&o| o != s && matches!(self.folk[o].inn, Inn::Merry(_)) && self.brawl[o].is_none() && self.d2(s, o) <= 36.0);
                    if let Some(o) = other {
                        self.start_brawl(s, o);
                    }
                }
            }
            // Children: a game of chase with another close by.
            if day && self.is_child(s) && self.folk[s].play.is_none() && tick >= self.calm[s] && !self.ents.hidden[s] && self.rng.f32() < 0.3 {
                let other = (0..self.ents.high)
                    .filter(|&o| o != s && self.ents.alive[o] && self.is_child(o) && self.folk[o].play.is_none() && !self.ents.hidden[o] && self.d2(s, o) <= 14.0 * 14.0)
                    .min_by(|&a, &b| self.d2(s, a).total_cmp(&self.d2(s, b)));
                if let Some(o) = other {
                    let until = tick + (15 + self.rng.below(15)) * hz;
                    self.folk[s].play = Some((self.ents.id_of(o), true, until));
                    self.folk[o].play = Some((self.ents.id_of(s), false, until));
                    if let Some(l) = self.pick_line(s, "play", &[]) {
                        self.npc_chat.push(l);
                    }
                }
            } else if self.folk[s].play.is_some() && tick >= self.ents.say_at[s] && self.rng.f32() < 0.08 {
                self.ents.say_at[s] = tick + 5 * hz;
                if let Some(l) = self.pick_line(s, "play", &[]) {
                    self.npc_chat.push(l);
                }
            }
            if self.is_pet(s) {
                self.dog(s, &mut taken_masters);
            }
        }
        self.system_funerals();
        self.system_gatherings();
    }

    /// A dog finds a master if it has none, and barks at what it should.
    fn dog(&mut self, s: usize, taken: &mut std::collections::HashSet<u32>) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        let home = self.home_of(s);
        if self.ents.resolve(self.folk[s].master).is_none() {
            let m = (0..self.ents.high)
                .filter(|&m| self.adult(m) && self.home_of(m) == home && !taken.contains(&self.ents.id_of(m)))
                .min_by(|&a, &b| self.d2(s, a).total_cmp(&self.d2(s, b)));
            if let Some(m) = m {
                self.folk[s].master = self.ents.id_of(m);
                taken.insert(self.folk[s].master);
            }
        }
        if tick < self.folk[s].bark_at || self.ents.hidden[s] {
            return;
        }
        // A cutpurse on the run: after it.
        let thief = (0..self.ents.high).filter(|&t| self.ents.alive[t] && tick < self.wanted[t] && self.ents.owner[t] == NONE && self.d2(s, t) <= 20.0 * 20.0).min_by(|&a, &b| self.d2(s, a).total_cmp(&self.d2(s, b)));
        let at = if let Some(t) = thief {
            self.folk[s].hunt = self.ents.id_of(t);
            Some(3)
        } else {
            let master = self.folk[s].master;
            // A stranger to its master close by, or a beast in the dark.
            let stranger = (0..self.ents.high).any(|p| self.ents.alive[p] && self.ents.owner[p] < DRIVEN && !self.ents.hidden[p] && self.d2(s, p) <= 7.0 * 7.0 && !self.met[p].contains(&master));
            let beast = self.night
                && (0..self.ents.high).any(|b| self.ents.alive[b] && self.ents.owner[b] == NONE && !self.ents.hidden[b] && self.arch_of(b).is_some_and(|a| a.behaviour == Behaviour::Chase) && self.d2(s, b) <= 30.0 * 30.0);
            if beast {
                Some(8)
            } else if stranger && self.rng.f32() < 0.35 {
                Some(12)
            } else {
                None
            }
        };
        if let Some(secs) = at {
            self.folk[s].bark_at = tick + secs * hz;
            if let Some(l) = self.pick_line(s, "bark", &[]) {
                self.npc_chat.push(l);
            }
        }
    }

    /// A line of `occasion` for bot `s` to say, `{place}` its town and any `holes` besides.
    pub(crate) fn pick_line(&mut self, s: usize, occasion: &str, holes: &[(&str, &str)]) -> Option<(usize, String)> {
        let place = self.town_near(self.ents.x[s], self.ents.y[s]);
        let mut all: Vec<(&str, &str)> = holes.to_vec();
        all.push(("place", &place));
        self.say_line(occasion, &all).map(|l| (s, l))
    }

    /// A bot has been killed (by `by`, what the news calls the killer): a townsperson is talked of, mourned and,
    /// with `Rules::newcomer_secs`, replaced by a newcomer that many seconds on (true: it stays gone till then); a
    /// traveller struck down on the road is talked of too.
    pub(crate) fn folk_death(&mut self, s: usize, by: String) -> bool {
        let Some(given) = self.given[s].clone() else { return false };
        if self.ents.fleeting[s] || self.is_thief_type(s) || self.is_pet(s) || self.is_stray(s) {
            return false;
        }
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let home = self.home_of(s).filter(|h| self.town_index.contains(h) && !self.is_traveller(s));
        let who = self.news_name(s);
        let Some(h) = home else {
            // A traveller: robbers on the road.
            self.tell_news("news_robbed", who, by, x, y);
            return false;
        };
        let (px, py) = (self.places[h].x, self.places[h].y);
        let sickness = std::mem::take(&mut self.sick[s]) > 0;
        self.tell_news(if sickness { "news_plague_died" } else { "news_died" }, who, by, px, py);
        if self.rules.newcomer_secs <= 0.0 {
            return false;
        }
        let from = self.door_of(s).unwrap_or((px, py));
        let to = self.churchyard(h);
        let start = self.tick + FUNERAL_AFTER_SECS * self.tick_hz.max(1);
        self.funerals.push(Funeral { home: h, who: given.to_string(), from, to, start, until: start + FUNERAL_SECS * self.tick_hz.max(1), mourners: Vec::new() });
        self.folk[s].newcomer = true;
        true
    }

    /// A fallen townsperson comes back: as a newcomer, under a new name, if they were buried.
    pub(crate) fn folk_back(&mut self, s: usize) {
        if !std::mem::take(&mut self.folk[s].newcomer) {
            return;
        }
        self.give_name(s);
        self.ents.renamed.push(s as u32);
        self.door[s] = None;
        self.work[s] = None;
        self.folk[s].new_till = self.tick + 900 * self.tick_hz.max(1);
        let who = self.news_name(s);
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        self.tell_news("news_newcomer", who, String::new(), x, y);
    }

    /// The bell, in every town with a church to ring it (four houses or more, as clients ring it), at dawn and at
    /// dusk: of an evening the games of chase end and the children go in; either way a townsperson or two remarks on
    /// it.
    fn ring_bell(&mut self, dawn: bool) {
        let towns: Vec<usize> = self.town_index.iter().copied().filter(|&h| self.houses_in(h).nth(3).is_some()).collect();
        for s in 0..self.ents.high {
            if !dawn && self.is_child(s) {
                self.folk[s].play = None;
            }
        }
        // Those nearest a player say it, so it is heard; in a town with no one about, whoever is out.
        let players: Vec<(f32, f32)> = (0..self.ents.high).filter(|&p| self.ents.alive[p] && self.ents.owner[p] < DRIVEN).map(|p| (self.ents.x[p], self.ents.y[p])).collect();
        let near = |s: &Sim, t: usize| players.iter().map(|&(x, y)| (s.ents.x[t] - x).powi(2) + (s.ents.y[t] - y).powi(2)).fold(f32::INFINITY, f32::min);
        for h in towns {
            let mut folk: Vec<usize> = (0..self.ents.high).filter(|&s| self.adult(s) && self.town_of_bot(s) == Some(h)).collect();
            folk.sort_by(|&a, &b| near(self, a).total_cmp(&near(self, b)));
            for &s in folk.iter().take(2) {
                if let Some(l) = self.pick_line(s, if dawn { "bell_dawn" } else { "bell" }, &[]) {
                    self.npc_chat.push(l);
                }
            }
        }
    }

    fn system_funerals(&mut self) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        self.funerals.retain(|f| tick < f.until);
        for k in 0..self.funerals.len() {
            let f = &self.funerals[k];
            if tick < f.start {
                continue;
            }
            // Not in the dark: a funeral waits for the morning.
            if f.mourners.is_empty() && self.night {
                let len = f.until - f.start;
                self.funerals[k].start = tick + hz;
                self.funerals[k].until = tick + hz + len;
                continue;
            }
            if f.mourners.is_empty() {
                let (home, from) = (f.home, f.from);
                let mut near: Vec<usize> = (0..self.ents.high).filter(|&s| self.adult(s) && self.home_of(s) == Some(home) && self.brawl[s].is_none()).collect();
                near.sort_by(|&a, &b| {
                    let d = |s: usize| (self.ents.x[s] - from.0).powi(2) + (self.ents.y[s] - from.1).powi(2);
                    d(a).total_cmp(&d(b))
                });
                near.truncate(MOURNERS);
                let ids: Vec<u32> = near.iter().map(|&s| self.ents.id_of(s)).collect();
                for &s in &near {
                    self.folk[s].inn = Inn::Out;
                    self.wander_to[s] = (f32::NAN, f32::NAN);
                }
                self.funerals[k].mourners = ids;
                continue;
            }
            // Now and then one of them says a word for the dead.
            if self.rng.f32() < 0.15 {
                let f = self.funerals[k].clone();
                let m = f.mourners[self.rng.below(f.mourners.len() as u32) as usize];
                if let Some(s) = self.ents.resolve(m).filter(|&s| tick >= self.ents.say_at[s]) {
                    self.ents.say_at[s] = tick + 8 * hz;
                    if let Some(l) = self.pick_line(s, "mourn", &[("who", &f.who)]) {
                        self.npc_chat.push(l);
                    }
                }
            }
        }
    }

    fn system_gatherings(&mut self) {
        let (tick, hz) = (self.tick, self.tick_hz.max(1));
        self.gatherings.retain(|g| tick < g.until);
        let dance = self.emotes.iter().find(|e| e.cmd == "dance").map(|e| e.anim.clone());
        for k in 0..self.gatherings.len() {
            let g = self.gatherings[k].clone();
            let (px, py, r) = (self.places[g.place].x, self.places[g.place].y, self.places[g.place].r);
            let there: Vec<usize> = (0..self.ents.high)
                .filter(|&s| self.festive(s) && self.home_of(s) == Some(g.place) && !self.ents.hidden[s] && (self.ents.x[s] - px).powi(2) + (self.ents.y[s] - py).powi(2) <= r * r)
                .collect();
            if there.is_empty() {
                continue;
            }
            if self.rng.f32() < 0.5 {
                let s = there[self.rng.below(there.len() as u32) as usize];
                if tick >= self.ents.say_at[s] {
                    self.ents.say_at[s] = tick + 8 * hz;
                    if let Some(l) = self.pick_line(s, &g.occasion, &[("who", &g.couple.0), ("other", &g.couple.1)]) {
                        self.npc_chat.push(l);
                    }
                }
            }
            if let (true, Some(anim)) = (g.dance, dance.as_ref()) {
                for &s in &there {
                    // Dancing where they stand, not on the way there.
                    if tick < self.wander_wait[s] && tick >= self.folk[s].dance_at && self.rng.f32() < 0.25 {
                        self.folk[s].dance_at = tick + (6 + self.rng.below(6)) * hz;
                        self.npc_emotes.push((s, anim.clone()));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::folk::Inn;
    use crate::sim::{Sim, NONE};
    use proto::game::{Behaviour, DayPart, GameFile, Place, Work};
    use proto::world::{Aabb, World};
    use std::sync::Arc;

    /// A town with a square and three houses: the inn is the one by the square.
    fn town() -> (Sim, [u8; 3]) {
        let mut g = GameFile::default_game();
        g.rules.inn_pct = 100.0;
        g.rules.newcomer_secs = 60.0;
        g.places.push(Place { name: "town".into(), x: 500.0, y: 500.0, r: 60.0, title: "Kells".into(), exit: None, sound: String::new(), music: String::new(), safe: false, hold: None });
        let base = g.archetypes[0].clone();
        let mut folk = base.clone();
        (folk.name, folk.behaviour, folk.attack, folk.count, folk.home, folk.rules) = ("folk".into(), Behaviour::Wander, None, 0, Some("town".into()), vec![]);
        folk.names = vec!["Aoife".into(), "Brian".into(), "Ciara".into(), "Dara".into()];
        folk.surnames = vec!["Byrne".into(), "Walsh".into()];
        let mut child = folk.clone();
        (child.name, child.work, child.names) = ("child".into(), Some(Work::Play), vec!["Una".into(), "Finn".into()]);
        let mut dog = folk.clone();
        (dog.name, dog.pet, dog.names, dog.surnames) = ("dog".into(), true, vec![], vec![]);
        g.archetypes.extend([folk, child, dog]);
        let n = g.archetypes.len() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        s.terrain.houses = Arc::new(vec![
            Aabb { x0: 505.0, y0: 505.0, x1: 512.0, y1: 512.0 },
            Aabb { x0: 530.0, y0: 470.0, x1: 537.0, y1: 477.0 },
            Aabb { x0: 470.0, y0: 530.0, x1: 477.0, y1: 537.0 },
        ]);
        s.terrain.squares = Arc::new(vec![Aabb { x0: 490.0, y0: 490.0, x1: 502.0, y1: 502.0 }]);
        s.find_towns();
        s.daylight = proto::daylight::Daylight::Fixed { phase: 0.3 }; // midday, whatever the clock says
        (s, [n - 3, n - 2, n - 1])
    }

    fn run(s: &mut Sim, secs: u32) {
        for _ in 0..secs * s.tick_hz {
            s.step(&[]);
        }
    }

    #[test]
    fn of_an_evening_townsfolk_go_into_the_inn_and_come_out_singing() {
        let (mut s, [folk, ..]) = town();
        let a = s.spawn_as(NONE, folk, Some((480.0, 480.0))).unwrap();
        let ((dx, dy), (hx, hy)) = s.inn_of(s.home_of(a).unwrap()).expect("an inn");
        assert!((hx - 508.5).abs() < 0.1 && (hy - 508.5).abs() < 0.1, "the house by the square");
        s.daypart = DayPart::Dusk;
        s.daylight = proto::daylight::Daylight::Fixed { phase: 0.68 };
        s.system_folk();
        assert!(matches!(s.folk[a].inn, Inn::Going(_)), "off to the inn");
        let mut inside = false;
        for _ in 0..60 * s.tick_hz {
            s.step(&[]);
            inside |= s.ents.hidden[a];
            if inside {
                break;
            }
        }
        assert!(inside, "in at the inn's door ({dx},{dy}): at {},{}", s.ents.x[a], s.ents.y[a]);
        // Time passes inside; out again, the worse for drink, singing.
        if let Inn::In(_) = s.folk[a].inn {
            s.folk[a].inn = Inn::In(s.tick + 1);
        }
        run(&mut s, 1);
        assert!(matches!(s.folk[a].inn, Inn::Merry(_)) && !s.ents.hidden[a]);
        let mut sang = false;
        for _ in 0..30 {
            s.ents.say_at[a] = 0;
            s.npc_chat.clear();
            s.system_folk();
            sang |= s.npc_chat.iter().any(|(by, l)| *by == a && s.talk.lines("sing").contains(l));
        }
        assert!(sang, "and singing");
    }

    #[test]
    fn children_play_chase_and_a_dog_keeps_to_its_master_and_barks_at_a_stranger() {
        let (mut s, [folk, child, dog]) = town();
        let a = s.spawn_as(NONE, child, Some((495.0, 495.0))).unwrap();
        let b = s.spawn_as(NONE, child, Some((498.0, 495.0))).unwrap();
        for _ in 0..30 {
            s.system_folk();
            if s.folk[a].play.is_some() {
                break;
            }
        }
        let (_, a_it, _) = s.folk[a].play.expect("a game");
        let (_, b_it, _) = s.folk[b].play.expect("both in it");
        assert!(a_it != b_it, "one is it");
        let m = s.spawn_as(NONE, folk, Some((520.0, 520.0))).unwrap();
        let d = s.spawn_as(NONE, dog, Some((540.0, 540.0))).unwrap();
        s.system_folk();
        assert_eq!(s.folk[d].master, s.ents.id_of(m), "a master");
        run(&mut s, 10);
        assert!(s.d2(d, m) < 8.0 * 8.0, "at its master's heel: {}", s.d2(d, m).sqrt());
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (s.ents.x[d] + 2.0, s.ents.y[d]);
        let mut barked = false;
        for _ in 0..40 {
            s.folk[d].bark_at = 0;
            s.npc_chat.clear();
            s.system_folk();
            barked |= s.npc_chat.iter().any(|(by, _)| *by == d);
        }
        assert!(barked, "at a stranger");
    }

    #[test]
    fn the_evening_bell_ends_the_games_and_calls_the_children_home() {
        let (mut s, [folk, child, _]) = town();
        let mut houses = (*s.terrain.houses).clone();
        houses.push(Aabb { x0: 530.0, y0: 530.0, x1: 537.0, y1: 537.0 }); // a church's worth of town
        s.terrain.houses = Arc::new(houses);
        s.find_towns();
        let kids: Vec<usize> = [(495.0, 495.0), (498.0, 495.0)].iter().map(|&at| s.spawn_as(NONE, child, Some(at)).unwrap()).collect();
        let _ = s.spawn_as(NONE, folk, Some((500.0, 500.0))).unwrap();
        while s.folk[kids[0]].play.is_none() {
            s.system_folk();
        }
        s.daypart = DayPart::Dusk;
        s.daylight = proto::daylight::Daylight::Fixed { phase: 0.68 };
        s.system_folk();
        assert!(kids.iter().all(|&k| s.folk[k].play.is_none()), "the games are over");
        let lines = s.talk.lines("bell").to_vec();
        assert!(s.npc_chat.iter().any(|(_, l)| lines.contains(l)), "someone remarks on the bell: {:?}", s.npc_chat);
        let door = s.door_of(kids[0]).expect("a door");
        run(&mut s, 30);
        let k = kids[0];
        assert!(s.ents.hidden[k] || (s.ents.x[k] - door.0).hypot(s.ents.y[k] - door.1) < 4.0, "home by the door {door:?}: at {},{}", s.ents.x[k], s.ents.y[k]);
        s.system_folk();
        assert!(s.npc_chat.iter().filter(|(_, l)| lines.contains(l)).count() <= 2, "it rings once an evening");
    }

    #[test]
    fn a_townsperson_killed_is_mourned_and_a_newcomer_comes_in_their_place() {
        let (mut s, [folk, ..]) = town();
        let dead = s.spawn_as(NONE, folk, Some((520.0, 520.0))).unwrap();
        let mourners: Vec<usize> = (0..4).map(|k| s.spawn_as(NONE, folk, Some((480.0 + k as f32 * 3.0, 500.0))).unwrap()).collect();
        let was = s.given[dead].clone().unwrap();
        s.night = false;
        s.daylight = proto::daylight::Daylight::Fixed { phase: 0.3 };
        assert!(s.folk_death(dead, "a wolf".into()));
        s.ents.fallen[dead] = true;
        assert!(s.news.iter().any(|n| n.occasion == "news_died" && n.who.contains(&*was) && n.other == "a wolf"), "talked of");
        s.tick += 31 * s.tick_hz;
        s.system_folk();
        let f = s.funerals[0].clone();
        assert_eq!(f.mourners.len(), 4, "neighbours come");
        let mut said = false;
        for _ in 0..80 {
            s.npc_chat.clear();
            for &m in &mourners {
                s.ents.say_at[m] = 0;
            }
            s.system_folk();
            said |= s.npc_chat.iter().any(|(_, l)| l.contains(&f.who) || s.talk.lines("mourn").contains(l));
        }
        assert!(said, "a word for the dead");
        s.folk_back(dead);
        let now = s.given[dead].clone().unwrap();
        assert_ne!(now, was, "someone new");
        assert!(s.news.iter().any(|n| n.occasion == "news_newcomer"), "and folk talk of them");
    }

    #[test]
    fn a_gathering_brings_the_town_to_its_square_to_dance() {
        let (mut s, [folk, ..]) = town();
        s.emotes.retain(|e| e.cmd != "dance");
        s.emotes.push(proto::game::Emote { cmd: "dance".into(), text: "dances".into(), at: String::new(), anim: "Cheer".into(), stays: false });
        let all: Vec<usize> = (0..4).map(|k| s.spawn_as(NONE, folk, Some((540.0, 460.0 + k as f32 * 6.0))).unwrap()).collect();
        let h = s.home_of(all[0]).unwrap();
        let said = s.gather(h, 120, "wedding", Some("{who} and {other} are wed in {place}!"), true).unwrap();
        assert!(said.ends_with("are wed in Kells!") && !said.contains('{'), "{said}");
        run(&mut s, 40);
        let on = all.iter().filter(|&&a| (490.0..=502.0).contains(&s.ents.x[a]) && (490.0..=502.0).contains(&s.ents.y[a])).count();
        assert!(on >= 3, "out on the square: {on}");
        let mut danced = false;
        for _ in 0..20 {
            run(&mut s, 1);
            danced |= !s.npc_emotes.is_empty();
            s.npc_emotes.clear();
        }
        assert!(danced, "and dancing");
    }
}

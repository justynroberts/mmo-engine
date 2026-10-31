//! Boss fights: a bot type with a `boss` section (`proto::game::BossFile`) fights by script as well as by its rules.
//!
//! Threat decides who it goes for. Damage done to it adds threat, healing someone in the fight adds half the heal to
//! the healer, and an action with `taunt` puts its user on top. It turns on a new player only when they pass the
//! one it is on by a tenth, so whoever holds it (a tank) can keep it.
//!
//! Its moves are actions used on timers. A move with `warn_secs` marks the ground first: clients are told on chat
//! channel 14 (`proto::chat::channel::BOSS`) as `T|id|x|y|r|secs|fx`, and the blow lands on whoever is still inside
//! when the time is up. `C|id` takes a mark back (the fight ended before it landed). Health thresholds move it
//! through its phases, each with its own moves, a banner and adds; past `enrage_secs` its blows count
//! `enrage_might` times. With nobody left fighting it (fallen, gone, past its leash, in a safe place) it resets:
//! whole again, its adds gone, the fight to start over.

use crate::sim::{Sim, DRIVEN, NONE};
use proto::game::MoveAt;

/// Threat a newcomer must have over the one a boss is on, as a share of theirs, before it turns.
const TURN: f32 = 1.1;
/// How far round a fight, beyond its players, others are told of its marks and banners.
const TELL_REACH: f32 = 60.0;

/// One boss in a fight, keyed by its entity id in `Sim::bosses`.
#[derive(Debug, Clone)]
pub struct Fight {
    pub phase: usize,
    pub started: u32,
    /// When each move of the phase is next due (ticks).
    pub next: Vec<u32>,
    /// (player id, threat)
    pub threat: Vec<(u32, f32)>,
    /// Ids of the adds it brought in.
    pub adds: Vec<u32>,
    /// What its blows count for: 1, or its `enrage_might` once enraged.
    pub might: f32,
}

/// A marked spot on the ground, and the blow that lands on it.
#[derive(Debug, Clone)]
pub struct Mark {
    pub id: u32,
    pub boss: u32,
    pub action: u8,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub at: u32,
}

impl Sim {
    fn boss_rt(&self, s: usize) -> Option<&proto::game::BossRt> {
        self.archs.get(self.ents.arch[s] as usize)?.boss.as_ref()
    }

    /// A bot that is a boss came to blows with a player: its fight begins, if it had not.
    pub(crate) fn boss_engaged(&mut self, bot: usize, player: usize) {
        let id = self.ents.id_of(bot);
        if self.bosses.contains_key(&id) || self.ents.owner[player] >= DRIVEN {
            return;
        }
        if self.boss_rt(bot).is_none() {
            return;
        }
        let fight = Fight { phase: 0, started: self.tick, next: Vec::new(), threat: vec![(self.ents.id_of(player), 1.0)], adds: Vec::new(), might: 1.0 };
        self.bosses.insert(id, fight);
        self.enter_phase(bot, 0);
    }

    /// Threat on a boss from a player: `amount` more, or (taunting) straight to the top.
    pub(crate) fn add_threat(&mut self, boss: usize, player: usize, amount: f32, taunt: bool) {
        if self.ents.owner[player] >= DRIVEN {
            return;
        }
        let pid = self.ents.id_of(player);
        let Some(f) = self.bosses.get_mut(&self.ents.id_of(boss)) else { return };
        let top = f.threat.iter().map(|t| t.1).fold(0.0f32, f32::max);
        let at = match f.threat.iter().position(|t| t.0 == pid) {
            Some(i) => i,
            None => {
                f.threat.push((pid, 0.0));
                f.threat.len() - 1
            }
        };
        f.threat[at].1 += amount.max(0.0);
        if taunt {
            f.threat[at].1 = f.threat[at].1.max(top * TURN + 1.0);
        }
    }

    /// A heal of `amount` on `healed` by `healer`: half of it is threat on every boss `healed` is fighting.
    pub(crate) fn heal_threat(&mut self, healed: usize, healer: usize, amount: f32) {
        if self.ents.owner[healer] >= DRIVEN || amount <= 0.0 {
            return;
        }
        let hid = self.ents.id_of(healed);
        let bosses: Vec<u32> = self.bosses.iter().filter(|(_, f)| f.threat.iter().any(|t| t.0 == hid)).map(|(&k, _)| k).collect();
        for b in bosses {
            if let Some(bs) = self.ents.resolve(b) {
                self.add_threat(bs, healer, amount * 0.5, false);
            }
        }
    }

    /// How much a boss's blows count for (1 for any other entity).
    pub(crate) fn boss_might(&self, s: usize) -> f32 {
        if self.bosses.is_empty() {
            return 1.0;
        }
        self.bosses.get(&self.ents.id_of(s)).map_or(1.0, |f| f.might)
    }

    /// A boss fell: those fighting it are told, its adds go and so do its marks.
    pub(crate) fn boss_fell(&mut self, s: usize) {
        let Some(f) = self.bosses.remove(&self.ents.id_of(s)) else { return };
        if let Some(line) = self.boss_rt(s).map(|b| b.defeat.clone()).filter(|l| !l.is_empty()) {
            self.boss_banner(s, &f, &line);
        }
        self.end_fight(s, f);
    }

    /// Players fighting a boss and anyone else close enough to see it.
    fn fight_audience(&self, s: usize, f: &Fight) -> Vec<usize> {
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let reach = TELL_REACH * TELL_REACH;
        (0..self.ents.high)
            .filter(|&p| self.ents.alive[p] && self.ents.owner[p] < DRIVEN)
            .filter(|&p| {
                let id = self.ents.id_of(p);
                f.threat.iter().any(|t| t.0 == id) || ((self.ents.x[p] - x).powi(2) + (self.ents.y[p] - y).powi(2) <= reach && self.same_phase(p, s))
            })
            .collect()
    }

    fn boss_banner(&mut self, s: usize, f: &Fight, line: &str) {
        for p in self.fight_audience(s, f) {
            self.notes.push((p, 0, proto::chat::channel::BANNER, line.to_string()));
        }
    }

    fn boss_note(&mut self, s: usize, f: &Fight, line: String) {
        for p in self.fight_audience(s, f) {
            self.notes.push((p, 0, proto::chat::channel::BOSS, line.clone()));
        }
    }

    /// The fight is over (won or reset): its adds and marks go.
    fn end_fight(&mut self, s: usize, f: Fight) {
        for a in &f.adds {
            if let Some(a) = self.ents.resolve(*a) {
                self.release(a);
                self.ents.despawn(a);
            }
        }
        let id = self.ents.id_of(s);
        let gone: Vec<u32> = self.marks.iter().filter(|m| m.boss == id).map(|m| m.id).collect();
        self.marks.retain(|m| m.boss != id);
        for m in gone {
            self.boss_note(s, &f, format!("C|{m}"));
        }
    }

    /// Into phase `i`: its banner, its adds, and its moves' clocks started.
    fn enter_phase(&mut self, s: usize, i: usize) {
        let Some(p) = self.boss_rt(s).and_then(|b| b.phases.get(i)).cloned() else { return };
        let id = self.ents.id_of(s);
        let tick = self.tick;
        let hz = self.tick_hz as f32;
        let Some(f) = self.bosses.get_mut(&id) else { return };
        f.phase = i;
        // The first use of each move comes a beat in, not on the instant the phase starts.
        f.next = p.moves.iter().map(|m| tick + ((m.every_secs * hz * 0.5).round() as u32).max(1)).collect();
        let f = f.clone();
        if !p.say.is_empty() {
            self.boss_banner(s, &f, &p.say);
        }
        let top = f.threat.iter().max_by(|a, b| a.1.total_cmp(&b.1)).and_then(|t| self.ents.resolve(t.0));
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let mut added = Vec::new();
        for &(k, n) in &p.adds {
            for j in 0..n {
                let ang = (j as f32 / n as f32 + self.rng.f32() * 0.2) * std::f32::consts::TAU;
                let Some(a) = self.spawn_as(NONE, k as u8, Some((x + ang.cos() * 6.0, y + ang.sin() * 6.0))) else { break };
                self.ents.fleeting[a] = true;
                self.rs.phase[a] = self.rs.phase[s];
                if let Some(t) = top {
                    self.engage(a, t);
                }
                added.push(self.ents.id_of(a));
            }
        }
        if let Some(f) = self.bosses.get_mut(&id) {
            f.adds.extend(added);
        }
    }

    /// Where a move lands: (x, y), or None with nobody to aim it at.
    fn move_spot(&mut self, s: usize, at: MoveAt, players: &[usize], tank: Option<usize>) -> Option<(f32, f32)> {
        let (bx, by) = (self.ents.x[s], self.ents.y[s]);
        let p = match at {
            MoveAt::Boss => return Some((bx, by)),
            MoveAt::Tank => tank?,
            MoveAt::Random => *players.get(self.rng.below(players.len().max(1) as u32) as usize)?,
            MoveAt::Farthest => *players.iter().max_by(|&&a, &&b| {
                let d = |p: usize| (self.ents.x[p] - bx).powi(2) + (self.ents.y[p] - by).powi(2);
                d(a).total_cmp(&d(b))
            })?,
        };
        Some((self.ents.x[p], self.ents.y[p]))
    }

    /// A blow lands on a marked spot (or, unwarned, at once): the action's effects on every player in it.
    fn land(&mut self, s: usize, action: u8, x: f32, y: f32, r: f32) {
        let r2 = r * r;
        let mut struck = Vec::new();
        let e = &self.ents;
        self.grid.for_neighbours(x, y, |t| {
            let t = t as usize;
            if e.alive[t] && !e.fallen[t] && e.owner[t] < DRIVEN && e.air[t] == 0 && (e.x[t] - x).powi(2) + (e.y[t] - y).powi(2) <= r2 {
                struck.push(t);
            }
        });
        struck.retain(|&t| self.same_phase(t, s) && !self.ents.god[t]);
        self.apply_action(s, action, &struck);
        self.cast_seq = self.cast_seq.wrapping_add(1);
        let target = struck.first().map_or(0, |&t| self.ents.id_of(t));
        let cast = proto::wire::Cast { seq: self.cast_seq, caster: self.ents.id_of(s), target, action };
        self.casts.push((self.tick, s, cast));
    }

    /// Every tick: each boss in a fight picks its target by threat, moves through its phases, uses its moves, and
    /// resets when nobody is left fighting it; marks whose time is up land.
    pub(crate) fn system_bosses(&mut self) {
        if self.bosses.is_empty() {
            return;
        }
        let hz = self.tick_hz as f32;
        let ids: Vec<u32> = self.bosses.keys().copied().collect();
        for id in ids {
            let Some(s) = self.ents.resolve(id).filter(|&s| self.ents.alive[s] && !self.ents.fallen[s] && self.boss_rt(s).is_some()) else {
                // Gone without falling (removed, its type edited): the fight is simply over.
                if let Some(f) = self.bosses.remove(&id) {
                    for a in f.adds {
                        if let Some(a) = self.ents.resolve(a) {
                            self.ents.despawn(a);
                        }
                    }
                }
                self.marks.retain(|m| m.boss != id);
                continue;
            };
            let b = self.boss_rt(s).cloned().unwrap_or_default();
            let leash = self.archs[self.ents.arch[s] as usize].leash;
            // Who is still in it: there, standing, near enough, not in a safe place, in its phase.
            let mut f = self.bosses.remove(&id).unwrap_or_else(|| unreachable!());
            let (bx, by) = (self.ents.x[s], self.ents.y[s]);
            f.threat.retain(|t| {
                self.ents.resolve(t.0).is_some_and(|p| {
                    self.ents.alive[p]
                        && !self.ents.fallen[p]
                        && (self.ents.x[p] - bx).powi(2) + (self.ents.y[p] - by).powi(2) <= leash * leash
                        && !self.in_safe(p)
                        && self.same_phase(p, s)
                })
            });
            if f.threat.is_empty() {
                // Reset: whole again, and the fight starts over.
                let full = self.archs[self.ents.arch[s] as usize].start.clone();
                let h = proto::defs::ATTR_HEALTH as usize;
                self.ents.attrs[h][s] = full[h];
                self.ents.hurt_by[s].clear();
                self.release(s);
                self.end_fight(s, f);
                continue;
            }
            let players: Vec<usize> = f.threat.iter().filter_map(|t| self.ents.resolve(t.0)).collect();
            // The target: the top of its threat, once they pass the one it is on by a tenth.
            let top = f.threat.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or_else(|| unreachable!());
            let cur = self.foe[s].and_then(|c| f.threat.iter().find(|t| t.0 == c).copied());
            let tank_id = match cur {
                Some(c) if c.0 == top.0 || top.1 <= c.1 * TURN => c.0,
                _ => top.0,
            };
            if self.foe[s] != Some(tank_id) {
                self.release(s);
                if let Some(t) = self.ents.resolve(tank_id) {
                    self.bosses.insert(id, f.clone()); // engage must see the fight is on
                    self.engage(s, t);
                    self.bosses.remove(&id);
                }
            }
            self.foe_at[s] = self.tick; // a boss does not lose interest while anyone is fighting it
            let tank = self.ents.resolve(tank_id);
            // Its phase, by health left.
            let h = proto::defs::ATTR_HEALTH as usize;
            let full = self.archs[self.ents.arch[s] as usize].start[h].max(self.ents.attrs[h][s]).max(1.0);
            let frac = self.ents.attrs[h][s] / full;
            let mut next_phase = f.phase;
            while next_phase + 1 < b.phases.len() && frac < b.phases[next_phase + 1].below {
                next_phase += 1;
            }
            if b.enrage_secs > 0.0 && f.might == 1.0 && self.tick.saturating_sub(f.started) as f32 >= b.enrage_secs * hz {
                f.might = b.enrage_might;
                let line = format!("{} becomes enraged!", self.name_of(s));
                self.boss_banner(s, &f, &line);
            }
            if next_phase != f.phase {
                self.bosses.insert(id, f);
                for i in self.bosses[&id].phase + 1..=next_phase {
                    self.enter_phase(s, i);
                }
                continue;
            }
            // Its moves: due ones worked out first, then used with the fight back in place (a blow landing can
            // come back round to `engage`, which must find the fight on).
            let moves = b.phases.get(f.phase).map(|p| p.moves.clone()).unwrap_or_default();
            let mut due = Vec::new();
            for (i, m) in moves.iter().enumerate() {
                if f.next.get(i).is_none_or(|&t| self.tick < t) {
                    continue;
                }
                f.next[i] = self.tick + ((m.every_secs * hz).round() as u32).max(1);
                if let Some(spot) = self.move_spot(s, m.at, &players, tank) {
                    due.push((m.clone(), spot));
                }
            }
            self.bosses.insert(id, f.clone());
            for (m, (x, y)) in due {
                let area = self.defs.action(m.action).map_or(0.0, |d| d.area);
                let r = if m.radius > 0.0 { m.radius } else if area > 0.0 { area } else { 3.0 };
                if !m.say.is_empty() {
                    self.boss_banner(s, &f, &m.say);
                }
                if m.warn_secs > 0.0 {
                    self.mark_seq = self.mark_seq.wrapping_add(1);
                    let mark = Mark { id: self.mark_seq, boss: id, action: m.action, x, y, r, at: self.tick + ((m.warn_secs * hz).round() as u32).max(1) };
                    let fx = self.defs.action(m.action).map(|d| d.fx.clone()).unwrap_or_default();
                    self.boss_note(s, &f, format!("T|{}|{}|{}|{}|{}|{fx}", mark.id, x.round(), y.round(), r, m.warn_secs));
                    self.marks.push(mark);
                } else if self.ents.alive[s] {
                    self.land(s, m.action, x, y, r);
                }
            }
        }
        // Marks whose time is up.
        let due: Vec<Mark> = {
            let tick = self.tick;
            let (due, keep): (Vec<Mark>, Vec<Mark>) = std::mem::take(&mut self.marks).into_iter().partition(|m| m.at <= tick);
            self.marks = keep;
            due
        };
        for m in due {
            if let Some(s) = self.ents.resolve(m.boss).filter(|&s| self.ents.alive[s]) {
                self.land(s, m.action, m.x, m.y, m.r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim, NONE};
    use proto::game::{ActionFile, Behaviour, BossAdd, BossFile, BossMove, BossPhase, EffectFile, GameFile, Hits, MoveAt, Who};
    use proto::world::World;

    const SLAM: u8 = 2;
    const TAUNT: u8 = 3;

    /// A boss (the drone, made one) that slams its tank with a two-second warning, brings two drifters at half
    /// health and enrages after a minute; two players beside it.
    fn arena() -> (Sim, usize, usize, usize) {
        let mut g = GameFile::default_game();
        let hurt = |d: f32| EffectFile { to: Who::Target, attr: "health".into(), delta: d, secs: 0.0, lasts: 0.0 };
        g.actions.push(ActionFile { id: SLAM, name: "slam".into(), range: 0.0, area: 4.0, hits: Hits::Foes, effects: vec![hurt(-30.0)], fx: "fire".into(), ..Default::default() });
        g.actions.push(ActionFile { id: TAUNT, name: "taunt".into(), range: 20.0, hits: Hits::Foes, taunt: true, ..Default::default() });
        let i = g.archetypes.iter().position(|a| a.name == "drone").unwrap();
        let a = &mut g.archetypes[i];
        (a.count, a.attack, a.behaviour, a.rules) = (0, None, Behaviour::Idle, Vec::new());
        a.leash = 100.0;
        a.boss = Some(BossFile {
            phases: vec![
                BossPhase { below: 100.0, moves: vec![BossMove { action: "slam".into(), every_secs: 4.0, warn_secs: 2.0, at: MoveAt::Tank, ..Default::default() }], ..Default::default() },
                BossPhase { below: 50.0, say: "The drone calls for help!".into(), moves: vec![], adds: vec![BossAdd { bot: "drifter".into(), count: 2 }] },
            ],
            enrage_secs: 60.0,
            enrage_might: 3.0,
            defeat: "The drone is down.".into(),
        });
        for a in g.archetypes.iter_mut() {
            a.count = 0;
        }
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        let boss = s.spawn_as(NONE, i as u8, Some((300.0, 300.0))).unwrap();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        (s.ents.x[a], s.ents.y[a]) = (305.0, 300.0);
        (s.ents.x[b], s.ents.y[b]) = (295.0, 300.0);
        (s, boss, a, b)
    }

    fn hit(s: &mut Sim, boss: usize, by: usize, d: f32) {
        let id = s.ents.id_of(by);
        s.add_to(boss, 0, -d, id);
    }

    fn marks_for(s: &Sim, p: usize) -> Vec<String> {
        s.notes.iter().filter(|n| n.0 == p && n.2 == proto::chat::channel::BOSS).map(|n| n.3.clone()).collect()
    }

    #[test]
    fn a_boss_gives_each_player_their_own_spoils_once_a_week() {
        let (mut s, boss, a, b) = arena();
        let medkit = s.items.iter().position(|i| i.name == "medkit").unwrap();
        let k = s.ents.arch[boss] as usize;
        s.archs[k].drops = vec![proto::game::DropRt { item: medkit, chance_pct: 100.0, min: 1, max: 1 }];
        let count = |s: &Sim, p: usize| s.inv[p].iter().filter(|t| t.0 as usize == medkit + 1).map(|t| t.1 as u32).sum::<u32>();
        s.loot_from(boss, &[a, b]);
        assert_eq!((count(&s, a), count(&s, b)), (1, 1), "each their own, into the bag");
        assert!(s.ground.is_empty(), "nothing on the ground to squabble over");
        s.loot_from(boss, &[a]);
        assert_eq!(count(&s, a), 1, "not twice in a week");
        assert!(s.tells.iter().any(|(p, t)| *p == a && t.contains("this week")), "{:?}", s.tells);
        assert_eq!(s.record_of(a).spoils.len(), 1, "kept with the character");
    }

    #[test]
    fn it_goes_for_the_most_threat_and_a_taunt_turns_it() {
        let (mut s, boss, a, b) = arena();
        hit(&mut s, boss, a, 10.0);
        s.step(&[]);
        assert_eq!(s.foe[boss], Some(s.ents.id_of(a)), "the first to strike has it");
        hit(&mut s, boss, b, 10.5);
        s.step(&[]);
        assert_eq!(s.foe[boss], Some(s.ents.id_of(a)), "a little more is not enough to turn it");
        hit(&mut s, boss, b, 5.0);
        s.step(&[]);
        assert_eq!(s.foe[boss], Some(s.ents.id_of(b)), "a tenth more turns it");
        s.step(&[PendingAction { slot: a, action: TAUNT, target: s.ents.id_of(boss), arg: 0 }]);
        s.step(&[]);
        assert_eq!(s.foe[boss], Some(s.ents.id_of(a)), "the taunt takes it back");
    }

    #[test]
    fn healing_someone_in_the_fight_draws_threat() {
        let (mut s, boss, a, b) = arena();
        hit(&mut s, boss, a, 10.0);
        s.step(&[]);
        s.ents.attrs[0][a] = 50.0;
        let bid = s.ents.id_of(b);
        s.add_to(a, 0, 40.0, bid);
        let f = &s.bosses[&s.ents.id_of(boss)];
        assert_eq!(f.threat.iter().find(|t| t.0 == bid).map(|t| t.1), Some(20.0), "half of what was mended");
    }

    #[test]
    fn a_marked_blow_misses_who_walks_out_and_lands_on_who_stays() {
        let (mut s, boss, a, _) = arena();
        hit(&mut s, boss, a, 1.0);
        s.notes.clear();
        // The first slam is marked two seconds in (half its four), then lands two seconds later.
        let mut marked = None;
        for _ in 0..60 {
            s.step(&[]);
            if let Some(m) = marks_for(&s, a).into_iter().find(|m| m.starts_with("T|")) {
                marked = Some(m);
                break;
            }
            s.notes.clear();
        }
        let m = marked.expect("a mark");
        let parts: Vec<&str> = m.split('|').collect();
        assert_eq!((parts[2], parts[3], parts[4], parts[6]), ("305", "300", "4", "fire"), "on the tank, the slam's area: {m}");
        (s.ents.x[a], s.ents.y[a]) = (320.0, 300.0); // out of it
        for _ in 0..45 {
            s.step(&[]);
        }
        assert_eq!(s.ents.attrs[0][a], 100.0, "walked out, untouched");
        // The next one, stood in.
        s.notes.clear();
        let mut lands = false;
        for _ in 0..200 {
            let mark = marks_for(&s, a).into_iter().any(|m| m.starts_with("T|"));
            s.notes.clear();
            if mark {
                lands = true;
                break;
            }
            s.step(&[]);
        }
        assert!(lands);
        let (mx, my) = (s.marks[0].x, s.marks[0].y);
        (s.ents.x[a], s.ents.y[a]) = (mx, my);
        for _ in 0..45 {
            s.step(&[]);
        }
        assert!(s.ents.attrs[0][a] < 100.0, "stood in it: {}", s.ents.attrs[0][a]);
    }

    #[test]
    fn half_health_brings_adds_and_a_reset_heals_it_and_sends_them_away() {
        let (mut s, boss, a, _) = arena();
        let full = s.ents.attrs[0][boss];
        hit(&mut s, boss, a, full * 0.6);
        s.step(&[]);
        let f = &s.bosses[&s.ents.id_of(boss)];
        assert_eq!((f.phase, f.adds.len()), (1, 2), "into the second phase, with its two drifters");
        assert!(s.notes.iter().any(|n| n.0 == a && n.3.contains("calls for help")));
        let adds: Vec<u32> = f.adds.clone();
        assert!(adds.iter().all(|&d| s.ents.resolve(d).is_some_and(|d| s.foe[d] == Some(s.ents.id_of(a)))), "the adds come for the tank");
        // Everyone runs off: whole again, the adds gone, the fight over.
        (s.ents.x[a], s.ents.y[a]) = (900.0, 900.0);
        s.step(&[]);
        assert!(s.bosses.is_empty());
        assert_eq!(s.ents.attrs[0][boss], full);
        assert!(adds.iter().all(|&d| s.ents.resolve(d).is_none()));
    }

    #[test]
    fn it_enrages_and_its_fall_is_announced() {
        let (mut s, boss, a, _) = arena();
        hit(&mut s, boss, a, 1.0);
        s.step(&[]);
        let calm = s.scaled(boss, a, 0, -10.0);
        let id = s.ents.id_of(boss);
        s.tick += 61 * 20; // a minute and more into the fight
        s.bosses.get_mut(&id).unwrap().started = 0;
        s.step(&[]);
        assert_eq!(s.scaled(boss, a, 0, -10.0), calm * 3.0, "enraged");
        hit(&mut s, boss, a, 200.0);
        s.notes.clear();
        s.step(&[]);
        assert!(s.bosses.is_empty());
        assert!(s.notes.iter().any(|n| n.0 == a && n.3 == "The drone is down."), "{:?}", s.notes);
    }
}

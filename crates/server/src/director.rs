//! Weather and events: the things a game master does to the world while it runs.
//!
//! Both are data in the game file (see `proto::game::{WeatherDef, EventDef}`), so they are built in the console and
//! applied live without dropping anyone. Weather is a state with effects the server enforces (attribute drains on
//! everything alive, how far bots see) and a name, colour and intensity that clients are told so they can draw it.
//! An event is a list of steps (announce, change the weather, spawn bots, hit an area, clear bots) that runs when
//! fired by hand or on its own timer.
//!
//! The director runs on the sim thread just before each step, so a drain that takes health to zero is handled by
//! the same respawn rule as damage from an action.

use std::collections::VecDeque;

use proto::game::{EventDef, GameFile, Step, WeatherDef};

use crate::net::Clients;
use crate::sim::{Sim, WeatherView, GHOST, NONE};

const LOG_CAP: usize = 100;

#[derive(Clone, Debug)]
pub struct Active {
    pub name: String,
    /// Tick at which it ends, or None for until someone changes it.
    pub until: Option<u32>,
}

#[derive(Default)]
pub struct Director {
    weather: Vec<WeatherDef>,
    events: Vec<EventDef>,
    pub active: Option<Active>,
    next_due: Vec<u32>,
    pub fired: Vec<u64>,
    pending: Vec<usize>,
    /// Newest last: (tick, what happened).
    pub log: VecDeque<(u32, String)>,
    info_dirty: bool,
}

impl Director {
    pub fn new() -> Director {
        Director::default()
    }

    /// Adopt the definitions of a (validated) game file. Timers of events that did not change keep running; a weather
    /// that no longer exists ends.
    pub fn sync(&mut self, f: &GameFile, tick: u32, hz: u32) {
        let old: Vec<(String, u32, u32, u64)> = self
            .events
            .iter()
            .enumerate()
            .map(|(i, e)| (e.name.clone(), e.every_secs, self.next_due[i], self.fired[i]))
            .collect();
        self.weather = f.weather.clone();
        self.events = f.events.clone();
        self.next_due.clear();
        self.fired.clear();
        for e in &self.events {
            let kept = old.iter().find(|o| o.0 == e.name);
            match kept {
                Some(&(_, every, due, fired)) if every == e.every_secs => {
                    self.next_due.push(due);
                    self.fired.push(fired);
                }
                Some(&(_, _, _, fired)) => {
                    self.next_due.push(tick + e.every_secs * hz);
                    self.fired.push(fired);
                }
                None => {
                    self.next_due.push(tick + e.every_secs * hz);
                    self.fired.push(0);
                }
            }
        }
        self.pending.clear();
        if let Some(a) = &self.active {
            if !self.weather.iter().any(|w| w.name == a.name) {
                self.active = None;
            }
        }
        self.info_dirty = true;
    }

    pub fn weather_names(&self) -> Vec<&str> {
        self.weather.iter().map(|w| w.name.as_str()).collect()
    }

    /// Seconds until each event next comes due on its own (None for hand-fired ones).
    pub fn next_in_secs(&self, i: usize, tick: u32, hz: u32) -> Option<u32> {
        let e = self.events.get(i)?;
        (e.every_secs > 0).then(|| self.next_due[i].saturating_sub(tick) / hz.max(1))
    }

    /// For the console: each event with its fire count and time to its next automatic run.
    pub fn event_rows(&self, tick: u32, hz: u32) -> Vec<serde_json::Value> {
        (0..self.events.len())
            .map(|i| serde_json::json!({ "name": self.events[i].name, "fired": self.fired[i], "next_in_secs": self.next_in_secs(i, tick, hz) }))
            .collect()
    }

    fn note(&mut self, tick: u32, text: String) {
        if self.log.len() >= LOG_CAP {
            self.log.pop_front();
        }
        self.log.push_back((tick, text));
    }

    /// Start a weather (or with None, clear it). `secs` of 0 means until changed.
    pub fn set_weather(&mut self, sim: &mut Sim, name: Option<&str>, secs: u32) -> Result<(), String> {
        let hz = sim.tick_hz;
        match name {
            None => {
                if self.active.take().is_some() {
                    self.note(sim.tick, "weather cleared".into());
                }
            }
            Some(n) => {
                if !self.weather.iter().any(|w| w.name == n) {
                    return Err(format!("no weather named '{n}'"));
                }
                self.active = Some(Active { name: n.to_string(), until: (secs > 0).then(|| sim.tick + secs * hz) });
                self.note(sim.tick, format!("weather: {n}{}", if secs > 0 { format!(" for {secs}s") } else { String::new() }));
            }
        }
        self.apply_weather(sim);
        Ok(())
    }

    fn apply_weather(&mut self, sim: &mut Sim) {
        let def = self.active.as_ref().and_then(|a| self.weather.iter().find(|w| w.name == a.name));
        sim.sight_mult = def.map_or(1.0, |w| w.sight_mult);
        sim.weather = def.map(|w| WeatherView { name: w.name.clone(), color: w.color.clone(), intensity: w.intensity });
        self.info_dirty = true;
    }

    /// Queue an event to run at the next tick.
    pub fn fire(&mut self, name: &str) -> Result<(), String> {
        let i = self.events.iter().position(|e| e.name == name).ok_or_else(|| format!("no event named '{name}'"))?;
        self.pending.push(i);
        Ok(())
    }

    /// Run before each sim step. Returns true when what clients are told (the weather) changed.
    pub fn tick(&mut self, sim: &mut Sim, clients: &mut Clients) -> bool {
        let now = sim.tick;
        if self.active.is_some() != sim.weather.is_some() {
            self.apply_weather(sim); // the world was rebuilt, or the weather's definition went away
        }
        if let Some(a) = &self.active {
            if a.until.is_some_and(|u| now >= u) {
                let n = a.name.clone();
                self.active = None;
                self.note(now, format!("weather '{n}' ended"));
                self.apply_weather(sim);
            }
        }
        self.drain(sim);
        for i in 0..self.events.len() {
            let e = &self.events[i];
            if e.every_secs == 0 || now < self.next_due[i] {
                continue;
            }
            self.next_due[i] = now + e.every_secs * sim.tick_hz;
            if sim.rng.f32() * 100.0 < e.chance_pct {
                self.run(i, sim, clients);
            }
        }
        for i in std::mem::take(&mut self.pending) {
            self.run(i, sim, clients);
        }
        std::mem::take(&mut self.info_dirty)
    }

    /// The weather's attribute drains, applied to everything alive (players and bots) once per tick.
    fn drain(&self, sim: &mut Sim) {
        let Some(def) = self.active.as_ref().and_then(|a| self.weather.iter().find(|w| w.name == a.name)) else { return };
        for d in &def.drain {
            let Some(a) = sim.defs.attrs.iter().position(|x| x.name == d.attr) else { continue };
            let (lo, hi, delta) = (sim.defs.attrs[a].min, sim.defs.attrs[a].max, d.per_sec * sim.dt);
            for s in 0..sim.ents.high {
                if sim.ents.alive[s] && sim.ents.owner[s] != GHOST {
                    let v = &mut sim.ents.attrs[a][s];
                    *v = (*v + delta).clamp(lo, hi);
                }
            }
        }
    }

    fn run(&mut self, i: usize, sim: &mut Sim, clients: &mut Clients) {
        let (name, steps) = (self.events[i].name.clone(), self.events[i].steps.clone());
        self.fired[i] += 1;
        self.note(sim.tick, format!("event '{name}' fired"));
        for st in &steps {
            let said = match st {
                Step::Announce { text } => match proto::chat::sanitize(text) {
                    Some(t) => {
                        announce(sim, clients, &t);
                        format!("announced: {t}")
                    }
                    None => continue,
                },
                Step::Weather { name: w, duration_secs } => match self.set_weather(sim, w.as_deref(), *duration_secs) {
                    Ok(()) => continue, // set_weather already logged it
                    Err(e) => e,
                },
                Step::Spawn { archetype, count, place } => {
                    let Some(a) = sim.archs.iter().position(|x| &x.name == archetype) else { continue };
                    let area = place.as_ref().and_then(|p| sim.places.iter().find(|x| &x.name == p)).map(|p| (p.x, p.y, p.r));
                    let mut n = 0;
                    for _ in 0..*count {
                        let at = area.map(|(x, y, r)| {
                            let (ang, d) = (sim.rng.f32() * std::f32::consts::TAU, r * sim.rng.f32().sqrt());
                            (x + ang.cos() * d, y + ang.sin() * d)
                        });
                        if sim.spawn_as(NONE, a as u8, at).is_none() {
                            break;
                        }
                        n += 1;
                    }
                    format!("spawned {n} {archetype}{}", place.as_ref().map(|p| format!(" at {p}")).unwrap_or_default())
                }
                Step::Hit { attr, delta, place } => {
                    let Some(a) = sim.defs.attrs.iter().position(|x| &x.name == attr) else { continue };
                    let (lo, hi) = (sim.defs.attrs[a].min, sim.defs.attrs[a].max);
                    let area = place.as_ref().and_then(|p| sim.places.iter().find(|x| &x.name == p)).map(|p| (p.x, p.y, p.r));
                    let mut n = 0;
                    for s in 0..sim.ents.high {
                        if !sim.ents.alive[s] || sim.ents.owner[s] == GHOST {
                            continue;
                        }
                        if let Some((x, y, r)) = area {
                            if (sim.ents.x[s] - x).powi(2) + (sim.ents.y[s] - y).powi(2) > r * r {
                                continue;
                            }
                        }
                        let v = &mut sim.ents.attrs[a][s];
                        *v = (*v + delta).clamp(lo, hi);
                        n += 1;
                    }
                    format!("{attr} {delta:+} on {n}{}", place.as_ref().map(|p| format!(" at {p}")).unwrap_or_default())
                }
                Step::Drop { item, .. } => format!("drop of {item} (items not live yet)"),
                Step::Clear { archetype } => {
                    let Some(a) = sim.archs.iter().position(|x| &x.name == archetype) else { continue };
                    let mut n = 0;
                    for s in 0..sim.ents.high {
                        if sim.ents.alive[s] && sim.ents.owner[s] == NONE && sim.ents.arch[s] == a as u8 {
                            sim.ents.despawn(s);
                            n += 1;
                        }
                    }
                    format!("removed {n} {archetype}")
                }
            };
            self.note(sim.tick, format!("  {said}"));
        }
    }
}

/// A line from the server itself, to every connected player. Sender 0 marks it as the server's.
pub fn announce(sim: &Sim, clients: &mut Clients, text: &str) {
    let ch = proto::chat::channel::SHOUT;
    for c in clients.slab.iter_mut().flatten() {
        c.chat.deliver(0, ch, text);
    }
    clients.log_chat(sim.tick, 0, ch, text);
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::game::{Drain, WeatherDef};
    use proto::world::World;

    fn setup(f: impl FnOnce(&mut GameFile)) -> (Sim, Director, Clients, GameFile) {
        let mut g = GameFile::default_game();
        f(&mut g);
        let mut sim = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 256, 20, 3);
        let mut d = Director::new();
        d.sync(&g, 0, 20);
        (sim, d, Clients::default(), g)
    }
    fn step(s: &mut Sim, d: &mut Director, c: &mut Clients, n: u32) {
        for _ in 0..n {
            d.tick(s, c);
            s.step(&[]);
        }
    }
    fn storm() -> WeatherDef {
        WeatherDef { name: "storm".into(), color: "#445566".into(), intensity: 0.8, sight_mult: 0.5, drain: vec![Drain { attr: "health".into(), per_sec: -10.0 }] }
    }
    fn health(s: &Sim, slot: usize) -> f32 {
        s.ents.attrs[s.defs.attrs.iter().position(|a| a.name == "health").unwrap()][slot]
    }

    #[test]
    fn weather_drains_attributes_changes_sight_tells_clients_and_ends() {
        let (mut s, mut d, mut c, _) = setup(|g| g.weather.push(storm()));
        let a = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        let before = health(&s, a);
        d.set_weather(&mut s, Some("storm"), 2).unwrap();
        assert_eq!(s.sight_mult, 0.5);
        assert_eq!(s.weather.as_ref().unwrap().name, "storm");
        step(&mut s, &mut d, &mut c, 20);
        let lost = before - health(&s, a);
        assert!((lost - 10.0).abs() < 1.5, "10 per second for about a second: {lost}");
        step(&mut s, &mut d, &mut c, 30);
        assert!(s.weather.is_none() && s.sight_mult == 1.0, "ended after its two seconds");
        assert!(d.log.iter().any(|(_, t)| t.contains("ended")));
        assert!(d.set_weather(&mut s, Some("hail"), 0).is_err());
    }

    #[test]
    fn a_drain_to_zero_is_a_death_like_any_other() {
        let (mut s, mut d, mut c, _) = setup(|g| {
            let mut w = storm();
            w.drain[0].per_sec = -200.0;
            g.weather.push(w);
        });
        let a = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        d.set_weather(&mut s, Some("storm"), 0).unwrap();
        step(&mut s, &mut d, &mut c, 40);
        assert!(s.stats.respawns > 0, "health on_zero is respawn in the default game");
        assert!(health(&s, a) >= 0.0);
    }

    #[test]
    fn a_timed_event_runs_on_its_interval_and_chance_zero_never_does() {
        let steps = vec![Step::Spawn { archetype: "drifter".into(), count: 2, place: None }];
        let (mut s, mut d, mut c, _) = setup(|g| {
            g.archetypes[0].name = "drifter".into();
            g.events.push(EventDef { name: "trickle".into(), every_secs: 5, chance_pct: 100.0, steps: steps.clone() });
            g.events.push(EventDef { name: "never".into(), every_secs: 5, chance_pct: 0.0, steps: steps.clone() });
        });
        let base = s.ents.live;
        step(&mut s, &mut d, &mut c, 99);
        assert_eq!(s.ents.live, base, "not due yet");
        step(&mut s, &mut d, &mut c, 2);
        assert_eq!(s.ents.live, base + 2);
        assert_eq!(d.fired, vec![1, 0]);
        step(&mut s, &mut d, &mut c, 100);
        assert_eq!(s.ents.live, base + 4);
    }

    #[test]
    fn steps_spawn_in_a_place_hit_an_area_announce_and_clear() {
        let (mut s, mut d, mut c, _) = setup(|g| {
            let arch = g.archetypes[0].name.clone();
            g.events.push(EventDef {
                name: "meteor".into(),
                every_secs: 0,
                chance_pct: 100.0,
                steps: vec![
                    Step::Spawn { archetype: arch.clone(), count: 10, place: Some("plaza".into()) },
                    Step::Hit { attr: "health".into(), delta: -30.0, place: Some("plaza".into()) },
                    Step::Announce { text: "Impact at the plaza".into() },
                    Step::Weather { name: None, duration_secs: 1 },
                ],
            });
            g.events.push(EventDef { name: "sweep".into(), every_secs: 0, chance_pct: 100.0, steps: vec![Step::Clear { archetype: arch }] });
        });
        let far = s.spawn_as(NONE, 0, Some((450.0, 450.0))).unwrap();
        let (px, py, pr) = { let p = s.places.iter().find(|p| p.name == "plaza").unwrap(); (p.x, p.y, p.r) };
        d.fire("meteor").unwrap();
        d.tick(&mut s, &mut c);
        let in_plaza: Vec<usize> = (0..s.ents.high).filter(|&k| s.ents.alive[k] && (s.ents.x[k] - px).hypot(s.ents.y[k] - py) <= pr + 0.1).collect();
        assert!(in_plaza.len() >= 10, "ten bots landed in the plaza: {}", in_plaza.len());
        assert!(in_plaza.iter().all(|&k| health(&s, k) <= 71.0), "all of them were hit");
        assert!(health(&s, far) > 99.0, "the one far away was not");
        assert_eq!(c.chat_log.back().unwrap().3, "Impact at the plaza");
        assert_eq!(c.chat_log.back().unwrap().1, 0, "from the server");
        d.fire("sweep").unwrap();
        d.tick(&mut s, &mut c);
        assert!((0..s.ents.high).all(|k| !s.ents.alive[k] || s.ents.arch[k] != 0));
        assert!(d.fire("nope").is_err());
    }

    #[test]
    fn syncing_keeps_running_timers_and_drops_a_vanished_weather() {
        let (mut s, mut d, mut c, mut g) = setup(|g| {
            g.weather.push(storm());
            g.events.push(EventDef { name: "tick".into(), every_secs: 10, chance_pct: 100.0, steps: vec![Step::Announce { text: "hi".into() }] });
        });
        step(&mut s, &mut d, &mut c, 100);
        let due = d.next_in_secs(0, s.tick, 20).unwrap();
        d.set_weather(&mut s, Some("storm"), 0).unwrap();
        g.weather.clear();
        d.sync(&g, s.tick, 20);
        assert_eq!(d.next_in_secs(0, s.tick, 20).unwrap(), due, "an edit elsewhere does not reset the timer");
        d.tick(&mut s, &mut c);
        assert!(d.active.is_none());
        assert!(s.weather.is_none() && s.sight_mult == 1.0);
    }
}

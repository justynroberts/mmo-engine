//! Weather and events: the things a game master does to the world while it runs.
//!
//! Both are data in the game file (see `proto::game::{WeatherDef, EventDef}`), so they are built in the console and
//! applied live without dropping anyone. Weather is a state with effects the server enforces (attribute drains on
//! everything alive, how far bots see) and a name, colour and intensity that clients are told so they can draw it.
//! A weather zone is a weather that stays over one place (`proto::game::WeatherZone`): its effects apply only to what
//! is inside, and clients draw it there as mist in its colour.
//! An event is a list of steps (announce, change the weather, spawn bots, hit an area, clear bots, switch a zone,
//! wait, wait for its bots to be beaten, reward) that runs when fired by hand, on its own timer, or when something
//! starts it (`When`: players gather at its place, a time of day, another event finishing). A run goes through its
//! steps over time: a raid warns, waits, sends waves, waits for them to be beaten and rewards those who were there,
//! or, if time runs out, fails and runs its `fail_steps` instead. Bots an event raises are its own (`Ents::fleeting`):
//! beaten, they are gone, never saved, and if the run fails (or is stopped) any still standing leave with it.
//!
//! The director runs on the sim thread just before each step, so a drain that takes health to zero is handled by
//! the same respawn rule as damage from an action.

use std::collections::{HashMap, VecDeque};

use proto::game::{EventDef, GameFile, Outcome, Step, WeatherDef, WeatherZone, When};

use crate::net::Clients;
use crate::sim::{Sim, WeatherView, ZoneView, GHOST, NONE};

const LOG_CAP: usize = 100;
/// How far past its place's edge an event's `nearby` messages and its participants reach.
pub const NEARBY: f32 = 60.0;

/// One run of an event, going through its steps.
pub struct Run {
    pub name: String,
    steps: Vec<Step>,
    fail_steps: Vec<Step>,
    /// The event's place as it was when the run started.
    place: Option<(f32, f32, f32)>,
    pub pc: usize,
    pub failing: bool,
    wait: Wait,
    /// Bots this run raised that are still standing (entity ids).
    pub foes: Vec<u32>,
    pub foes_total: u32,
    /// Players who were there while it ran (entity ids): at its place, or anywhere for an event with no place.
    pub people: Vec<u32>,
    pub started: u32,
    deadline: Option<u32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Wait {
    No,
    Until(u32),
    /// Until every foe is beaten, failing at this tick if one is given.
    Defeat(Option<u32>),
}

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
    zones: Vec<WeatherZone>,
    /// Which zones are on now: as the game file says, until an event or the console switches one.
    pub zone_on: Vec<bool>,
    zones_dirty: bool,
    /// Events in progress.
    pub runs: Vec<Run>,
    /// Tick each event was last started by its `when`, for the cooldown.
    when_at: HashMap<String, u32>,
    /// Events that finished since the triggers last looked, and whether they succeeded.
    finished: Vec<(String, bool)>,
    /// How each event last ended, for the console.
    pub outcome: HashMap<String, &'static str>,
    /// Where the day was when last looked (for `When::Time`).
    last_phase: Option<f64>,
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
        // A zone keeps what an event or the console made it while its definition is unchanged; a new or edited
        // zone starts as the file says.
        let was: Vec<(WeatherZone, bool)> = self.zones.iter().cloned().zip(self.zone_on.iter().copied()).collect();
        self.zones = f.zones.clone();
        self.zone_on = self.zones.iter().map(|z| was.iter().find(|w| &w.0 == z).map_or(z.active, |w| w.1)).collect();
        self.zones_dirty = true;
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

    /// Switch a weather zone on or off.
    pub fn set_zone(&mut self, sim: &mut Sim, name: &str, on: bool) -> Result<(), String> {
        let i = self.zones.iter().position(|z| z.name == name).ok_or_else(|| format!("no weather zone named '{name}'"))?;
        if self.zone_on[i] != on {
            self.zone_on[i] = on;
            self.note(sim.tick, format!("zone '{name}' {}", if on { "on" } else { "off" }));
        }
        self.apply_zones(sim);
        Ok(())
    }

    /// For the console: each zone and whether it is on.
    pub fn zone_rows(&self) -> Vec<serde_json::Value> {
        self.zones.iter().zip(&self.zone_on).map(|(z, on)| serde_json::json!({ "name": z.name, "on": on })).collect()
    }

    /// Put the zones that are on where the sim and clients see them (their places can have moved).
    fn apply_zones(&mut self, sim: &mut Sim) {
        let mut out = Vec::new();
        for (z, _) in self.zones.iter().zip(&self.zone_on).filter(|(_, on)| **on) {
            let (Some(p), Some(w)) = (sim.places.iter().find(|p| p.name == z.place), self.weather.iter().find(|w| w.name == z.weather)) else { continue };
            out.push(ZoneView { name: z.name.clone(), x: p.x, y: p.y, r: p.r, height: z.height, weather: w.name.clone(), color: w.color.clone(), intensity: w.intensity, sight_mult: w.sight_mult });
        }
        if out != sim.zones {
            sim.zones = out;
            self.info_dirty = true;
        }
        self.zones_dirty = false;
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
            .map(|i| {
                let name = &self.events[i].name;
                serde_json::json!({ "name": name, "fired": self.fired[i], "next_in_secs": self.next_in_secs(i, tick, hz), "last": self.outcome.get(name) })
            })
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

    /// Queue an event to start at the next tick.
    pub fn fire(&mut self, name: &str) -> Result<(), String> {
        let i = self.events.iter().position(|e| e.name == name).ok_or_else(|| format!("no event named '{name}'"))?;
        if self.runs.iter().any(|r| r.name == name) {
            return Err(format!("'{name}' is already running"));
        }
        self.pending.push(i);
        Ok(())
    }

    /// End a run now, as if it never finished: its bots leave, its fail steps do not run, nothing follows it.
    pub fn stop(&mut self, sim: &mut Sim, name: &str) -> Result<(), String> {
        let i = self.runs.iter().position(|r| r.name == name).ok_or_else(|| format!("'{name}' is not running"))?;
        let run = self.runs.remove(i);
        let gone = dismiss(sim, &run.foes);
        self.outcome.insert(name.to_string(), "stopped");
        self.note(sim.tick, format!("event '{name}' stopped{}", if gone > 0 { format!(", {gone} of its bots left") } else { String::new() }));
        Ok(())
    }

    /// Run before each sim step. Returns true when what clients are told (the weather) changed.
    pub fn tick(&mut self, sim: &mut Sim, clients: &mut Clients) -> bool {
        let now = sim.tick;
        if self.active.is_some() != sim.weather.is_some() {
            self.apply_weather(sim); // the world was rebuilt, or the weather's definition went away
        }
        if self.zones_dirty || (sim.zones.is_empty() && self.zone_on.iter().any(|&o| o)) {
            self.apply_zones(sim); // new definitions, or the world was rebuilt
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
                self.start(i, sim, clients, "on its timer");
            }
        }
        self.triggers(sim, clients);
        for i in std::mem::take(&mut self.pending) {
            self.start(i, sim, clients, "by hand");
        }
        let mut runs = std::mem::take(&mut self.runs);
        runs.retain_mut(|r| self.advance(r, sim, clients));
        runs.append(&mut self.runs); // (nothing starts while runs advance, but keep anything that did)
        self.runs = runs;
        std::mem::take(&mut self.info_dirty)
    }

    /// Start the events whose `when` has come true (each at most once per its cooldown, and never twice at once).
    fn triggers(&mut self, sim: &mut Sim, clients: &mut Clients) {
        let (now, hz) = (sim.tick, sim.tick_hz.max(1));
        let finished = std::mem::take(&mut self.finished);
        // The day is looked at once a second; a part of it "comes" when the phase passes its mark.
        let phase = (now % hz == 0).then(|| sim.daylight.phase(unix_ms()));
        let (was, ns) = (self.last_phase, sim.daylight.night_share());
        if phase.is_some() {
            self.last_phase = phase;
        }
        for i in 0..self.events.len() {
            let Some(when) = self.events[i].when.clone() else { continue };
            let name = self.events[i].name.clone();
            if self.runs.iter().any(|r| r.name == name) || self.when_at.get(&name).is_some_and(|&t| now.wrapping_sub(t) < self.events[i].cooldown_secs * hz) {
                continue;
            }
            let why = match when {
                When::Players { count } if now % hz == 0 => {
                    let area = self.events[i].place.as_deref().and_then(|p| place_area(sim, p));
                    let n = players(sim, clients, area.map(|(x, y, r)| (x, y, r + NEARBY))).len() as u32;
                    (n >= count).then(|| format!("{n} player{} there", if n == 1 { "" } else { "s" }))
                }
                When::Time { at } => match (was, phase) {
                    (Some(a), Some(b)) if crossed(a, b, at.phase(ns)) => Some(format!("at {at:?}").to_lowercase()),
                    _ => None,
                },
                When::After { event, outcome } => finished.iter().find(|(n, ok)| n == &event && match outcome {
                    Outcome::Success => *ok,
                    Outcome::Failure => !*ok,
                    Outcome::Either => true,
                }).map(|(n, ok)| format!("after '{n}' {}", if *ok { "succeeded" } else { "failed" })),
                _ => None,
            };
            if let Some(why) = why {
                self.when_at.insert(name, now);
                self.start(i, sim, clients, &why);
            }
        }
    }

    /// Begin a run of event `i` (unless one is already going).
    fn start(&mut self, i: usize, sim: &mut Sim, clients: &mut Clients, why: &str) {
        let e = &self.events[i];
        if self.runs.iter().any(|r| r.name == e.name) {
            self.note(sim.tick, format!("event '{}' is still running, so not again {why}", e.name));
            return;
        }
        let place = e.place.as_deref().and_then(|p| place_area(sim, p));
        let hz = sim.tick_hz;
        let mut run = Run {
            name: e.name.clone(),
            steps: e.steps.clone(),
            fail_steps: e.fail_steps.clone(),
            place,
            pc: 0,
            failing: false,
            wait: Wait::No,
            foes: Vec::new(),
            foes_total: 0,
            people: Vec::new(),
            started: sim.tick,
            deadline: (e.limit_secs > 0).then(|| sim.tick + e.limit_secs * hz),
        };
        self.fired[i] += 1;
        self.note(sim.tick, format!("event '{}' started {why}", run.name));
        run.people = players(sim, clients, run.place.map(|(x, y, r)| (x, y, r + NEARBY)));
        self.runs.push(run);
    }

    /// Take a run as far as it goes this tick. False once it has ended.
    fn advance(&mut self, run: &mut Run, sim: &mut Sim, clients: &mut Clients) -> bool {
        let now = sim.tick;
        if now % sim.tick_hz.max(1) == 0 {
            for id in players(sim, clients, run.place.map(|(x, y, r)| (x, y, r + NEARBY))) {
                if !run.people.contains(&id) {
                    run.people.push(id);
                }
            }
        }
        run.foes.retain(|&id| sim.ents.resolve(id).is_some());
        if run.deadline.is_some_and(|d| now >= d) && !run.failing {
            self.fail(run, sim.tick, "ran out of time");
        }
        loop {
            match run.wait {
                Wait::Until(t) if now < t => return true,
                Wait::Defeat(_) if run.foes.is_empty() => {
                    self.note(now, format!("  '{}': all {} beaten", run.name, run.foes_total));
                }
                Wait::Defeat(Some(t)) if now >= t => {
                    if run.failing {
                        return self.end(run, sim, false);
                    }
                    self.fail(run, now, "did not beat them in time");
                    continue;
                }
                Wait::Defeat(_) => return true,
                _ => {}
            }
            run.wait = Wait::No;
            let steps = if run.failing { &run.fail_steps } else { &run.steps };
            let Some(st) = steps.get(run.pc).cloned() else { return self.end(run, sim, !run.failing) };
            run.pc += 1;
            self.step(run, &st, sim, clients);
        }
    }

    fn fail(&mut self, run: &mut Run, tick: u32, why: &str) {
        self.note(tick, format!("event '{}' failed: {why}", run.name));
        run.failing = true;
        run.pc = 0;
        run.wait = Wait::No;
    }

    /// A run is over, and events that follow it may start. If it failed, what is left of its bots leaves (they
    /// have had their way); if it succeeded, any still standing stay until beaten (a plain wave with no `Defeat`).
    fn end(&mut self, run: &Run, sim: &mut Sim, ok: bool) -> bool {
        let gone = if ok { 0 } else { dismiss(sim, &run.foes) };
        self.note(sim.tick, format!("event '{}' {}{}", run.name, if ok { "succeeded" } else { "is over (failed)" }, if gone > 0 { format!(", {gone} of its bots left") } else { String::new() }));
        self.outcome.insert(run.name.clone(), if ok { "succeeded" } else { "failed" });
        self.finished.push((run.name.clone(), ok));
        false
    }

    /// The weather's attribute drains, applied to everything alive (players and bots) once per tick, and each zone's
    /// to what is inside it.
    fn drain(&self, sim: &mut Sim) {
        if let Some(def) = self.active.as_ref().and_then(|a| self.weather.iter().find(|w| w.name == a.name)) {
            Self::drain_in(sim, def, None);
        }
        for z in sim.zones.clone() {
            if let Some(def) = self.weather.iter().find(|w| w.name == z.weather) {
                Self::drain_in(sim, def, Some((z.x, z.y, z.r)));
            }
        }
    }

    fn drain_in(sim: &mut Sim, def: &WeatherDef, area: Option<(f32, f32, f32)>) {
        for d in &def.drain {
            let Some(a) = sim.defs.attrs.iter().position(|x| x.name == d.attr) else { continue };
            let (lo, hi, delta) = (sim.defs.attrs[a].min, sim.defs.attrs[a].max, d.per_sec * sim.dt);
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
            }
        }
    }

    /// Do one step of a run (`Wait` and `Defeat` only set what it waits for).
    fn step(&mut self, run: &mut Run, st: &Step, sim: &mut Sim, clients: &mut Clients) {
        let run_place = run.place;
        let here = |p: &Option<String>, sim: &Sim| match p {
            Some(p) => place_area(sim, p),
            None => run_place,
        };
        let hz = sim.tick_hz;
        let said = match st {
            Step::Announce { text, banner, nearby } => match proto::chat::sanitize(text) {
                Some(t) => {
                    let area = if *nearby { run.place.map(|(x, y, r)| (x, y, r + NEARBY)) } else { None };
                    let ch = if *banner { proto::chat::channel::BANNER } else { proto::chat::channel::SHOUT };
                    let n = tell(sim, clients, &t, ch, area);
                    format!("{}: {t}{}", if *banner { "banner" } else { "announced" }, if *nearby { format!(" (to {n} nearby)") } else { String::new() })
                }
                None => return,
            },
            Step::Weather { name: w, duration_secs } => match self.set_weather(sim, w.as_deref(), *duration_secs) {
                Ok(()) => return, // set_weather already logged it
                Err(e) => e,
            },
            Step::Spawn { archetype, count, place, name } => {
                let Some(a) = sim.archs.iter().position(|x| &x.name == archetype) else { return };
                let area = here(place, sim);
                let mut n = 0;
                for _ in 0..*count {
                    let at = area.map(|(x, y, r)| {
                        let (ang, d) = (sim.rng.f32() * std::f32::consts::TAU, r * sim.rng.f32().sqrt());
                        (x + ang.cos() * d, y + ang.sin() * d)
                    });
                    let Some(slot) = sim.spawn_as(NONE, a as u8, at) else { break };
                    sim.ents.fleeting[slot] = true;
                    if let Some(nm) = name {
                        sim.set_name(slot, nm, false);
                    }
                    run.foes.push(sim.ents.id_of(slot));
                    run.foes_total += 1;
                    n += 1;
                }
                let full = if n < *count { format!(" of {count}: no room for more bots (the rest of the table is kept for players)") } else { String::new() };
                format!("spawned {n} {}{}{full}", name.clone().unwrap_or_else(|| archetype.clone()), if area.is_some() { " there" } else { "" })
            }
            Step::Hit { attr, delta, place } => {
                let Some(a) = sim.defs.attrs.iter().position(|x| &x.name == attr) else { return };
                let (lo, hi) = (sim.defs.attrs[a].min, sim.defs.attrs[a].max);
                let area = here(place, sim);
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
                format!("{attr} {delta:+} on {n}{}", if area.is_some() { " there" } else { "" })
            }
            Step::Drop { item, count, place } => {
                let Some(i) = sim.items.iter().position(|x| &x.name == item) else { return };
                let area = here(place, sim);
                sim.scatter_ground(i, *count, area);
                format!("dropped {count} {item}{}", if area.is_some() { " there" } else { "" })
            }
            Step::Zone { zone, on } => match self.set_zone(sim, zone, *on) {
                Ok(()) => return, // set_zone already logged it
                Err(e) => e,
            },
            Step::Clear { archetype } => {
                let Some(a) = sim.archs.iter().position(|x| &x.name == archetype) else { return };
                let mut n = 0;
                for s in 0..sim.ents.high {
                    if sim.ents.alive[s] && sim.ents.owner[s] == NONE && sim.ents.arch[s] == a as u8 {
                        sim.ents.despawn(s);
                        n += 1;
                    }
                }
                format!("removed {n} {archetype}")
            }
            Step::Wait { secs } => {
                run.wait = Wait::Until(sim.tick + secs * hz);
                format!("waiting {secs}s")
            }
            Step::Defeat { within_secs } => {
                run.wait = Wait::Defeat((*within_secs > 0).then(|| sim.tick + within_secs * hz));
                format!("waiting for {} to be beaten{}", run.foes.len(), if *within_secs > 0 { format!(" within {within_secs}s") } else { String::new() })
            }
            Step::Reward { item, count, everyone } => {
                let Some(i) = sim.items.iter().position(|x| &x.name == item) else { return };
                let who: Vec<u32> = if *everyone { players(sim, clients, None) } else { run.people.clone() };
                let mut n = 0;
                for id in who {
                    if let Some(slot) = sim.ents.resolve(id) {
                        sim.give_or_drop(slot, i, *count);
                        n += 1;
                    }
                }
                format!("rewarded {n} player{} with {count} {item}", if n == 1 { "" } else { "s" })
            }
        };
        self.note(sim.tick, format!("  {said}"));
    }

    /// For the console: what each run is doing.
    pub fn run_rows(&self, tick: u32, hz: u32) -> Vec<serde_json::Value> {
        let hz = hz.max(1);
        self.runs
            .iter()
            .map(|r| {
                let of = if r.failing { r.fail_steps.len() } else { r.steps.len() };
                let waiting = match r.wait {
                    Wait::No => None,
                    Wait::Until(t) => Some(format!("waiting, {}s left", t.saturating_sub(tick) / hz)),
                    Wait::Defeat(t) => Some(format!("{} of {} still standing{}", r.foes.len(), r.foes_total, t.map(|t| format!(", {}s left", t.saturating_sub(tick) / hz)).unwrap_or_default())),
                };
                serde_json::json!({
                    "name": r.name, "step": r.pc, "of": of, "failing": r.failing, "waiting": waiting,
                    "foes": r.foes.len(), "people": r.people.len(), "secs": tick.saturating_sub(r.started) / hz,
                    "left_secs": r.deadline.map(|d| d.saturating_sub(tick) / hz),
                })
            })
            .collect()
    }
}

/// A place's circle by name.
fn place_area(sim: &Sim, name: &str) -> Option<(f32, f32, f32)> {
    sim.places.iter().find(|p| p.name == name).map(|p| (p.x, p.y, p.r))
}

/// The players online (entity ids), or those inside a circle.
fn players(sim: &Sim, clients: &Clients, area: Option<(f32, f32, f32)>) -> Vec<u32> {
    clients
        .slab
        .iter()
        .flatten()
        .filter(|c| matches!(c.zstate, crate::net::ZState::Here) && sim.ents.alive[c.slot])
        .filter(|c| area.is_none_or(|(x, y, r)| (sim.ents.x[c.slot] - x).powi(2) + (sim.ents.y[c.slot] - y).powi(2) <= r * r))
        .map(|c| sim.ents.id_of(c.slot))
        .collect()
}

/// Send a run's bots that are still standing away. Returns how many went.
fn dismiss(sim: &mut Sim, foes: &[u32]) -> usize {
    let mut n = 0;
    for &id in foes {
        if let Some(s) = sim.ents.resolve(id) {
            sim.ents.despawn(s);
            n += 1;
        }
    }
    n
}

/// Whether the day went past `mark` going from phase `a` to `b` (phases wrap at 1).
fn crossed(a: f64, b: f64, mark: f64) -> bool {
    if a <= b {
        a < mark && mark <= b
    } else {
        mark > a || mark <= b // wrapped past midnight-of-the-cycle
    }
}

fn unix_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

/// A line from the server to every player, or to those inside a circle, on a channel (a shout, or a banner).
/// Returns how many it reached.
pub fn tell(sim: &Sim, clients: &mut Clients, text: &str, ch: u8, area: Option<(f32, f32, f32)>) -> usize {
    let mut n = 0;
    for c in clients.slab.iter_mut().flatten() {
        if let Some((x, y, r)) = area {
            if (sim.ents.x[c.slot] - x).powi(2) + (sim.ents.y[c.slot] - y).powi(2) > r * r {
                continue;
            }
        }
        c.chat.deliver(0, ch, text);
        n += 1;
    }
    clients.log_chat(sim.tick, 0, ch, text);
    n
}


#[cfg(test)]
mod tests {
    use super::*;
    use proto::game::WeatherZone;
    use proto::game::{Drain, WeatherDef};
    use proto::world::World;

    fn setup(f: impl FnOnce(&mut GameFile)) -> (Sim, Director, Clients, GameFile) {
        let mut g = GameFile::default_game();
        f(&mut g);
        let sim = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 256, 20, 3);
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
    fn a_weather_zone_acts_only_inside_its_place_and_events_switch_it() {
        let (mut s, mut d, mut c, _) = setup(|g| {
            g.weather.push(storm());
            g.places.push(proto::game::Place { name: "haunt".into(), x: 300.0, y: 300.0, r: 20.0, title: String::new(), exit: None });
            g.zones.push(WeatherZone { name: "red mist".into(), place: "haunt".into(), weather: "storm".into(), active: true, height: 30.0 });
            g.events.push(EventDef { name: "lift".into(), every_secs: 0, chance_pct: 100.0, steps: vec![Step::Zone { zone: "red mist".into(), on: false }], ..EventDef::default() });
        });
        let inside = s.spawn_as(NONE, 0, Some((305.0, 300.0))).unwrap();
        let outside = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        s.ents.dir_x[inside] = 0;
        let (hi, ho) = (health(&s, inside), health(&s, outside));
        assert!(d.tick(&mut s, &mut c), "the zone is news for clients");
        assert_eq!(s.zones.len(), 1);
        assert_eq!((s.sight_at(300.0, 300.0), s.sight_at(100.0, 100.0)), (0.5, 1.0), "bots see less only inside");
        step(&mut s, &mut d, &mut c, 20);
        assert!(hi - health(&s, inside) > 5.0, "inside, the storm drains");
        assert_eq!(health(&s, outside), ho, "outside, nothing");
        // An event lifts it.
        d.fire("lift").unwrap();
        assert!(d.tick(&mut s, &mut c));
        assert!(s.zones.is_empty() && s.sight_at(300.0, 300.0) == 1.0);
        assert_eq!(d.zone_rows()[0]["on"], false);
        assert!(d.set_zone(&mut s, "nowhere", true).is_err());
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
        let steps = vec![Step::Spawn { archetype: "drifter".into(), count: 2, place: None, name: None }];
        let (mut s, mut d, mut c, _) = setup(|g| {
            g.archetypes[0].name = "drifter".into();
            g.events.push(EventDef { name: "trickle".into(), every_secs: 5, chance_pct: 100.0, steps: steps.clone(), ..EventDef::default() });
            g.events.push(EventDef { name: "never".into(), every_secs: 5, chance_pct: 0.0, steps: steps.clone(), ..EventDef::default() });
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
                    Step::Spawn { archetype: arch.clone(), count: 10, place: Some("plaza".into()), name: None },
                    Step::Hit { attr: "health".into(), delta: -30.0, place: Some("plaza".into()) },
                    Step::Announce { text: "Impact at the plaza".into(), banner: false, nearby: false },
                    Step::Weather { name: None, duration_secs: 1 },
                ],
                ..EventDef::default()
            });
            g.events.push(EventDef { name: "sweep".into(), every_secs: 0, chance_pct: 100.0, steps: vec![Step::Clear { archetype: arch }], ..EventDef::default() });
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
            g.events.push(EventDef { name: "tick".into(), every_secs: 10, chance_pct: 100.0, steps: vec![Step::Announce { text: "hi".into(), banner: false, nearby: false }], ..EventDef::default() });
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
    /// A raid at the plaza: a warning, a wait, a named boss and two guards, a fight to win in time, then a word.
    fn raid(within: u32) -> EventDef {
        EventDef {
            name: "raid".into(),
            place: Some("plaza".into()),
            steps: vec![
                Step::Announce { text: "They are coming".into(), banner: true, nearby: false },
                Step::Wait { secs: 1 },
                Step::Spawn { archetype: "drifter".into(), count: 1, place: None, name: Some("Morvane".into()) },
                Step::Spawn { archetype: "drifter".into(), count: 2, place: None, name: None },
                Step::Defeat { within_secs: within },
                Step::Announce { text: "The plaza holds".into(), banner: true, nearby: false },
            ],
            fail_steps: vec![Step::Announce { text: "The plaza has fallen".into(), banner: false, nearby: false }],
            ..EventDef::default()
        }
    }
    fn alive_named(s: &Sim, name: &str) -> usize {
        (0..s.ents.high).filter(|&k| s.ents.alive[k] && s.ents.name[k].as_deref() == Some(name)).count()
    }

    #[test]
    fn a_raid_waits_sends_its_bots_and_succeeds_once_they_are_beaten() {
        let (mut s, mut d, mut c, _) = setup(|g| {
            g.archetypes[0].name = "drifter".into();
            g.events.push(raid(60));
        });
        let base = s.ents.live;
        d.fire("raid").unwrap();
        step(&mut s, &mut d, &mut c, 1);
        assert!(d.fire("raid").is_err(), "not twice at once");
        step(&mut s, &mut d, &mut c, 10);
        assert_eq!(s.ents.live, base, "still waiting its second");
        assert_eq!(c.chat_log.back().unwrap().2, proto::chat::channel::BANNER, "the warning is a banner");
        step(&mut s, &mut d, &mut c, 15);
        assert_eq!(s.ents.live, base + 3);
        assert_eq!(alive_named(&s, "Morvane"), 1, "the boss has its name");
        let (px, py, pr) = { let p = s.places.iter().find(|p| p.name == "plaza").unwrap(); (p.x, p.y, p.r) };
        let foes: Vec<usize> = (0..s.ents.high).filter(|&k| s.ents.alive[k] && s.ents.fleeting[k]).collect();
        assert_eq!(foes.len(), 3);
        assert!(foes.iter().all(|&k| (s.ents.x[k] - px).hypot(s.ents.y[k] - py) <= pr + 8.0), "at the event's place (they may have taken a step)");
        assert_eq!(d.run_rows(s.tick, 20)[0]["foes"], 3);
        // Beaten: gone, not respawned, and the run carries on to its last word.
        let h = s.defs.attrs.iter().position(|a| a.name == "health").unwrap();
        for &k in &foes {
            s.ents.attrs[h][k] = 0.0;
        }
        step(&mut s, &mut d, &mut c, 3);
        assert_eq!(s.ents.live, base, "beaten bots of an event are gone");
        assert!(d.runs.is_empty());
        assert_eq!(d.outcome["raid"], "succeeded");
        assert_eq!(c.chat_log.back().unwrap().3, "The plaza holds");
        assert!(d.log.iter().any(|(_, t)| t.contains("all 3 beaten")));
    }

    #[test]
    fn a_raid_that_runs_out_of_time_fails_its_bots_leave_and_what_follows_failure_starts() {
        let (mut s, mut d, mut c, _) = setup(|g| {
            g.archetypes[0].name = "drifter".into();
            g.events.push(raid(2));
            g.events.push(EventDef {
                name: "aftermath".into(),
                when: Some(When::After { event: "raid".into(), outcome: Outcome::Failure }),
                steps: vec![Step::Announce { text: "Rebuild".into(), banner: false, nearby: false }],
                ..EventDef::default()
            });
        });
        let base = s.ents.live;
        d.fire("raid").unwrap();
        step(&mut s, &mut d, &mut c, 30);
        assert_eq!(s.ents.live, base + 3);
        step(&mut s, &mut d, &mut c, 40);
        assert_eq!(d.outcome["raid"], "failed");
        assert_eq!(s.ents.live, base, "the raiders leave when it is over");
        assert!(c.chat_log.iter().any(|l| l.3 == "The plaza has fallen"), "its fail steps ran");
        assert!(!c.chat_log.iter().any(|l| l.3 == "The plaza holds"));
        step(&mut s, &mut d, &mut c, 2);
        assert!(c.chat_log.iter().any(|l| l.3 == "Rebuild"), "an event that follows a failure started");
        assert!(d.log.iter().any(|(_, t)| t.contains("started after 'raid' failed")));
        // Stopping: the run ends where it is and its bots go.
        d.fire("raid").unwrap();
        step(&mut s, &mut d, &mut c, 25);
        assert_eq!(s.ents.live, base + 3);
        d.stop(&mut s, "raid").unwrap();
        assert_eq!((s.ents.live, d.outcome["raid"]), (base, "stopped"));
        assert!(d.stop(&mut s, "raid").is_err());
    }

    #[test]
    fn a_time_of_day_comes_when_the_day_passes_its_mark() {
        assert!(crossed(0.10, 0.12, 0.11));
        assert!(!crossed(0.10, 0.12, 0.30));
        assert!(crossed(0.98, 0.01, 0.0), "dawn, across the wrap");
        assert!(crossed(0.98, 0.01, 0.99));
        assert!(!crossed(0.98, 0.01, 0.5));
        assert!(!crossed(0.4, 0.4, 0.4), "standing still is not passing");
        let ns = 1.0 / 3.0;
        assert_eq!(proto::game::DayPart::Dusk.phase(ns), 1.0 - ns);
        assert!((proto::game::DayPart::Night.phase(ns) - (1.0 - ns / 2.0)).abs() < 1e-12);
    }
}

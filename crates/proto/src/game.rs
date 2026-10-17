//! The game file: one JSON document that holds everything data driven.
//!
//! Attributes, actions and effects become [`Defs`] (shared with clients and
//! covered by the handshake content hash). Archetypes describe server-side
//! bots: they are never sent to clients, so editing them does not invalidate
//! anyone's connection. The test UI edits exactly this document.
//!
//! Everything is validated before it is applied, so a typo in the UI produces
//! an error message and the running game is left untouched.

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::defs::{ActionDef, AttrDef, Defs, Effect, OnZero};
use crate::snapshot::{HEADER_LEN, MAX_REPL};

pub const MAX_ATTRS: usize = 16;
pub const MAX_ACTIONS: usize = 32;
pub const MAX_ARCHETYPES: usize = 64;
/// Snapshots must fit one datagram: header + visible * (id, x, y, attrs).
const SNAPSHOT_BUDGET: usize = 1100;

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum OnZeroFile {
    #[default]
    None,
    Respawn,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AttrFile {
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    #[serde(default = "d_bits")]
    pub bits: u8,
    #[serde(default)]
    pub replicated: bool,
    #[serde(default)]
    pub regen_per_sec: f32,
    #[serde(default)]
    pub on_zero: OnZeroFile,
}
fn d_bits() -> u8 {
    8
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Who {
    #[serde(rename = "self")]
    Actor,
    Target,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EffectFile {
    pub to: Who,
    pub attr: String,
    pub delta: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    pub attr: String,
    pub min: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ActionFile {
    pub id: u8,
    pub name: String,
    #[serde(default)]
    pub cooldown_ticks: u16,
    pub range: f32,
    #[serde(default)]
    pub requires: Option<Requires>,
    #[serde(default)]
    pub effects: Vec<EffectFile>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Behaviour {
    /// Stands still.
    Idle,
    /// Random walk.
    #[default]
    Wander,
    /// Walks toward the nearest other entity it can see.
    Chase,
    /// Walks away from the nearest other entity it can see.
    Flee,
}

/// A named circular area: a spawn point, a hangout, a market square.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub name: String,
    pub x: f32,
    pub y: f32,
    #[serde(default = "d_place_r")]
    pub r: f32,
}
fn d_place_r() -> f32 {
    12.0
}

/// A named path bots can walk. `looped` wraps from the last point to the first; otherwise they turn around.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub name: String,
    pub points: Vec<[f32; 2]>,
    #[serde(default, rename = "loop")]
    pub looped: bool,
}

/// What a `sees` condition looks for.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Seek {
    Player,
    Bot,
    Any,
}

/// When a rule applies.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cond {
    Always,
    Sees {
        who: Seek,
        #[serde(default = "d_sight")]
        within: f32,
    },
    AttrBelow {
        attr: String,
        value: f32,
    },
    AttrAbove {
        attr: String,
        value: f32,
    },
    InPlace {
        place: String,
    },
    /// Each decision (twice a second) this is true with probability `pct` percent.
    Chance {
        pct: f32,
    },
}

/// What a rule makes the bot do. `say` has no effect on movement and the rules after it still run.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Act {
    Idle,
    Wander,
    Chase,
    Flee,
    /// Chase the nearest target and use the archetype's attack action when in range.
    Attack,
    Patrol {
        route: String,
    },
    Goto {
        place: String,
    },
    Say {
        text: String,
        #[serde(default)]
        shout: bool,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub when: Cond,
    pub then: Act,
}

pub const MAX_RULES: usize = 16;
pub const MAX_PLACES: usize = 64;
pub const MAX_ROUTES: usize = 64;
pub const MAX_ROUTE_POINTS: usize = 64;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Archetype {
    pub name: String,
    #[serde(default = "d_color")]
    pub color: String,
    /// How many to spawn when the game file is applied.
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub behaviour: Behaviour,
    #[serde(default = "d_one")]
    pub speed_mult: f32,
    /// How far it can see other entities (chase and flee), in world units.
    #[serde(default = "d_sight")]
    pub sight: f32,
    /// Action (by name) used on the nearest visible entity when in range.
    #[serde(default)]
    pub attack: Option<String>,
    /// Starting values that differ from the attribute defaults, by attribute name.
    #[serde(default)]
    pub attrs: BTreeMap<String, f32>,
    /// Place (by name) where these bots spawn and which they drift back to when wandering.
    #[serde(default)]
    pub home: Option<String>,
    /// Checked in order, first movement rule that matches wins. With none, `behaviour` applies.
    #[serde(default)]
    pub rules: Vec<Rule>,
}
fn d_color() -> String {
    "#888888".into()
}
fn d_one() -> f32 {
    1.0
}
fn d_sight() -> f32 {
    40.0
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GameFile {
    #[serde(default = "d_speed")]
    pub move_speed: f32,
    #[serde(default = "d_radius")]
    pub entity_radius: f32,
    #[serde(default = "d_interest")]
    pub interest_radius: f32,
    #[serde(default = "d_visible")]
    pub max_visible: u32,
    pub attrs: Vec<AttrFile>,
    #[serde(default)]
    pub actions: Vec<ActionFile>,
    #[serde(default)]
    pub archetypes: Vec<Archetype>,
    #[serde(default)]
    pub places: Vec<Place>,
    #[serde(default)]
    pub routes: Vec<Route>,
}
fn d_speed() -> f32 {
    20.0
}
fn d_radius() -> f32 {
    0.5
}
fn d_interest() -> f32 {
    128.0
}
fn d_visible() -> u32 {
    64
}

/// Server-side runtime form of an archetype: names resolved to indices.
#[derive(Clone, Debug)]
pub struct ArchRt {
    pub name: String,
    pub color: String,
    pub count: u32,
    pub behaviour: Behaviour,
    pub speed_mult: f32,
    pub sight: f32,
    pub attack: Option<u8>,
    /// Starting value for every attribute (defaults with overrides applied).
    pub start: Vec<f32>,
    pub home: Option<usize>,
    pub rules: Vec<RuleRt>,
    /// Farthest any `sees` condition or chase reaches, for one neighbour scan.
    pub reach: f32,
}

#[derive(Clone, Debug)]
pub enum CondRt {
    Always,
    Sees { who: Seek, within: f32 },
    AttrBelow { attr: usize, value: f32 },
    AttrAbove { attr: usize, value: f32 },
    InPlace { place: usize },
    Chance { pct: f32 },
}

#[derive(Clone, Debug)]
pub enum ActRt {
    Idle,
    Wander,
    Chase,
    Flee,
    Attack,
    Patrol { route: usize },
    Goto { place: usize },
    Say { text: String, shout: bool },
}

#[derive(Clone, Debug)]
pub struct RuleRt {
    pub when: CondRt,
    pub then: ActRt,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub defs: Defs,
    pub archs: Vec<ArchRt>,
    pub places: Vec<Place>,
    pub routes: Vec<Route>,
}

impl GameFile {
    /// Same content as `Defs::default_game()`, plus a few example bots.
    pub fn default_game() -> GameFile {
        let attr = |name: &str, replicated: bool, regen: f32, on_zero| AttrFile {
            name: name.into(),
            min: 0.0,
            max: 100.0,
            default: 100.0,
            bits: 8,
            replicated,
            regen_per_sec: regen,
            on_zero,
        };
        let bot = |name: &str, color: &str, count, behaviour, speed_mult: f32, attack: Option<&str>| Archetype {
            name: name.into(),
            color: color.into(),
            count,
            behaviour,
            speed_mult,
            sight: 40.0,
            attack: attack.map(Into::into),
            attrs: BTreeMap::new(),
            home: None,
            rules: Vec::new(),
        };
        let mut drone = bot("drone", "#e0457b", 4, Behaviour::Chase, 1.0, Some("zap"));
        drone.attrs.insert("health".into(), 60.0);
        // An example of the rule builder: a guard who patrols and warns players away. None spawn by default.
        let mut guard = bot("guard", "#6c8ef5", 0, Behaviour::Wander, 0.7, None);
        guard.home = Some("plaza".into());
        guard.rules = vec![
            Rule { when: Cond::Sees { who: Seek::Player, within: 30.0 }, then: Act::Say { text: "Move along, citizen.".into(), shout: false } },
            Rule { when: Cond::Always, then: Act::Patrol { route: "ring road".into() } },
        ];
        GameFile {
            places: vec![Place { name: "plaza".into(), x: 200.0, y: 200.0, r: 20.0 }],
            routes: vec![Route { name: "ring road".into(), points: vec![[180.0, 180.0], [300.0, 180.0], [300.0, 300.0], [180.0, 300.0]], looped: true }],
            move_speed: 20.0,
            entity_radius: 0.5,
            interest_radius: 128.0,
            max_visible: 64,
            attrs: vec![
                attr("health", true, 0.0, OnZeroFile::Respawn),
                attr("energy", false, 5.0, OnZeroFile::None),
            ],
            actions: vec![ActionFile {
                id: 1,
                name: "zap".into(),
                cooldown_ticks: 10,
                range: 8.0,
                requires: Some(Requires { attr: "energy".into(), min: 5.0 }),
                effects: vec![
                    EffectFile { to: Who::Target, attr: "health".into(), delta: -10.0 },
                    EffectFile { to: Who::Actor, attr: "energy".into(), delta: -5.0 },
                ],
            }],
            archetypes: vec![
                bot("drifter", "#4aa3a0", 12, Behaviour::Wander, 0.5, None),
                bot("civilian", "#c9a227", 8, Behaviour::Flee, 0.8, None),
                drone,
                guard,
            ],
        }
    }

    /// Validate and resolve names. The error text is meant for a human in the UI.
    pub fn build(&self) -> Result<Game, String> {
        fn fin(v: f32, what: &str) -> Result<(), String> {
            if v.is_finite() {
                Ok(())
            } else {
                Err(format!("{what} must be a finite number"))
            }
        }
        if !(self.move_speed > 0.0 && self.move_speed <= 1000.0) {
            return Err("move_speed must be in (0, 1000]".into());
        }
        if !(self.entity_radius >= 0.05 && self.entity_radius <= 16.0) {
            return Err("entity_radius must be 0.05..16".into());
        }
        if !(self.interest_radius >= 8.0 && self.interest_radius <= 1024.0) {
            return Err("interest_radius must be 8..1024".into());
        }
        if self.max_visible == 0 || self.max_visible > 255 {
            return Err("max_visible must be 1..255".into());
        }
        if self.attrs.is_empty() || self.attrs.len() > MAX_ATTRS {
            return Err(format!("need 1 to {MAX_ATTRS} attributes"));
        }
        let mut names = HashSet::new();
        let mut attrs = Vec::new();
        let mut replicated = Vec::new();
        for (i, a) in self.attrs.iter().enumerate() {
            let n = a.name.trim();
            if n.is_empty() || n.len() > 32 || n != a.name {
                return Err(format!("attribute #{i}: name must be 1..32 characters, no edge spaces"));
            }
            if !names.insert(a.name.clone()) {
                return Err(format!("duplicate attribute name '{}'", a.name));
            }
            for (v, w) in [(a.min, "min"), (a.max, "max"), (a.default, "default"), (a.regen_per_sec, "regen_per_sec")] {
                fin(v, &format!("attribute '{}' {w}", a.name))?;
            }
            if a.min >= a.max {
                return Err(format!("attribute '{}': min must be below max", a.name));
            }
            if a.default < a.min || a.default > a.max {
                return Err(format!("attribute '{}': default must be between min and max", a.name));
            }
            if a.bits != 8 && a.bits != 16 {
                return Err(format!("attribute '{}': bits must be 8 or 16", a.name));
            }
            if a.replicated {
                replicated.push(i);
            }
            attrs.push(AttrDef {
                name: a.name.clone(),
                min: a.min,
                max: a.max,
                default: a.default,
                bits: a.bits,
                replicated: a.replicated,
                regen_per_sec: a.regen_per_sec,
                on_zero: match a.on_zero {
                    OnZeroFile::None => OnZero::None,
                    OnZeroFile::Respawn => OnZero::Respawn,
                },
            });
        }
        if replicated.len() > MAX_REPL {
            return Err(format!("at most {MAX_REPL} attributes can be replicated"));
        }
        let wire: usize = replicated.iter().map(|&i| if attrs[i].bits <= 8 { 1 } else { 2 }).sum();
        let worst = HEADER_LEN + self.max_visible as usize * (8 + wire);
        if worst > SNAPSHOT_BUDGET {
            return Err(format!(
                "max_visible {} with these replicated attributes could make a {worst} byte snapshot (limit {SNAPSHOT_BUDGET}); lower max_visible",
                self.max_visible
            ));
        }
        let idx = |name: &str, ctx: &str| -> Result<u8, String> {
            self.attrs
                .iter()
                .position(|a| a.name == name)
                .map(|i| i as u8)
                .ok_or_else(|| format!("{ctx}: unknown attribute '{name}'"))
        };

        if self.actions.len() > MAX_ACTIONS {
            return Err(format!("at most {MAX_ACTIONS} actions"));
        }
        let (mut ids, mut anames) = (HashSet::new(), HashSet::new());
        let mut actions = Vec::new();
        for a in &self.actions {
            if a.id == 0 {
                return Err(format!("action '{}': id 0 is reserved for 'no action'", a.name));
            }
            if !ids.insert(a.id) {
                return Err(format!("duplicate action id {}", a.id));
            }
            if a.name.is_empty() || !anames.insert(a.name.clone()) {
                return Err(format!("action names must be unique and non-empty ('{}')", a.name));
            }
            if !(a.range.is_finite() && a.range >= 0.0 && a.range <= self.interest_radius) {
                return Err(format!("action '{}': range must be 0..interest_radius", a.name));
            }
            let ctx = format!("action '{}'", a.name);
            let requires = match &a.requires {
                Some(r) => {
                    fin(r.min, &format!("{ctx} requires.min"))?;
                    Some((idx(&r.attr, &ctx)?, r.min))
                }
                None => None,
            };
            let mut effects = Vec::new();
            for e in &a.effects {
                fin(e.delta, &format!("{ctx} effect delta"))?;
                let attr = idx(&e.attr, &ctx)?;
                effects.push(match e.to {
                    Who::Target => Effect::AddToTarget { attr, delta: e.delta },
                    Who::Actor => Effect::AddToSelf { attr, delta: e.delta },
                });
            }
            actions.push(ActionDef {
                id: a.id,
                name: a.name.clone(),
                cooldown_ticks: a.cooldown_ticks,
                range: a.range,
                requires,
                effects,
            });
        }

        let defs = Defs {
            attrs,
            actions,
            replicated,
            move_speed: self.move_speed,
            entity_radius: self.entity_radius,
            interest_radius: self.interest_radius,
            max_visible: self.max_visible as usize,
        };

        if self.archetypes.len() > MAX_ARCHETYPES {
            return Err(format!("at most {MAX_ARCHETYPES} archetypes"));
        }
        if self.places.len() > MAX_PLACES || self.routes.len() > MAX_ROUTES {
            return Err(format!("at most {MAX_PLACES} places and {MAX_ROUTES} routes"));
        }
        let coord = |v: f32| v.is_finite() && (0.0..=4095.0).contains(&v);
        let mut pnames = HashSet::new();
        for p in &self.places {
            if p.name.is_empty() || p.name.len() > 32 || !pnames.insert(p.name.as_str()) {
                return Err(format!("place names must be unique, 1..32 characters ('{}')", p.name));
            }
            if !(coord(p.x) && coord(p.y) && p.r.is_finite() && (1.0..=512.0).contains(&p.r)) {
                return Err(format!("place '{}': x and y must be 0..4095 and r 1..512", p.name));
            }
        }
        let mut rnames = HashSet::new();
        for r in &self.routes {
            if r.name.is_empty() || r.name.len() > 32 || !rnames.insert(r.name.as_str()) {
                return Err(format!("route names must be unique, 1..32 characters ('{}')", r.name));
            }
            if r.points.len() < 2 || r.points.len() > MAX_ROUTE_POINTS {
                return Err(format!("route '{}': needs 2..{MAX_ROUTE_POINTS} points", r.name));
            }
            if !r.points.iter().all(|p| coord(p[0]) && coord(p[1])) {
                return Err(format!("route '{}': points must be 0..4095", r.name));
            }
        }
        let place_ix = |n: &str, ctx: &str| {
            self.places.iter().position(|p| p.name == n).ok_or_else(|| format!("{ctx}: unknown place '{n}'"))
        };
        let route_ix = |n: &str, ctx: &str| {
            self.routes.iter().position(|p| p.name == n).ok_or_else(|| format!("{ctx}: unknown route '{n}'"))
        };
        let mut seen = HashSet::new();
        let mut archs = Vec::new();
        for a in &self.archetypes {
            let ctx = format!("archetype '{}'", a.name);
            if a.name.is_empty() || a.name.len() > 32 || !seen.insert(a.name.clone()) {
                return Err(format!("archetype names must be unique, 1..32 characters ('{}')", a.name));
            }
            let c = a.color.as_bytes();
            if !(c.len() == 7 && c[0] == b'#' && c[1..].iter().all(|b| b.is_ascii_hexdigit())) {
                return Err(format!("{ctx}: color must look like #aabbcc"));
            }
            if !(a.speed_mult.is_finite() && (0.0..=10.0).contains(&a.speed_mult)) {
                return Err(format!("{ctx}: speed_mult must be 0..10"));
            }
            if !(a.sight.is_finite() && a.sight >= 0.0 && a.sight <= self.interest_radius) {
                return Err(format!("{ctx}: sight must be 0..interest_radius"));
            }
            if a.count > 100_000 {
                return Err(format!("{ctx}: count is capped at 100000"));
            }
            let attack = match &a.attack {
                Some(n) => Some(
                    defs.actions
                        .iter()
                        .find(|d| &d.name == n)
                        .map(|d| d.id)
                        .ok_or_else(|| format!("{ctx}: unknown attack action '{n}'"))?,
                ),
                None => None,
            };
            let mut start: Vec<f32> = defs.attrs.iter().map(|d| d.default).collect();
            for (k, &v) in &a.attrs {
                let i = idx(k, &ctx)? as usize;
                let d = &defs.attrs[i];
                if !(v.is_finite() && v >= d.min && v <= d.max) {
                    return Err(format!("{ctx}: {k} must be between {} and {}", d.min, d.max));
                }
                start[i] = v;
            }
            let home = match &a.home {
                Some(n) => Some(place_ix(n, &ctx)?),
                None => None,
            };
            if a.rules.len() > MAX_RULES {
                return Err(format!("{ctx}: at most {MAX_RULES} rules"));
            }
            let mut reach = a.sight;
            let mut rules = Vec::new();
            for (n, r) in a.rules.iter().enumerate() {
                let rc = format!("{ctx} rule {}", n + 1);
                let when = match &r.when {
                    Cond::Always => CondRt::Always,
                    Cond::Sees { who, within } => {
                        if !(within.is_finite() && *within > 0.0 && *within <= self.interest_radius) {
                            return Err(format!("{rc}: 'within' must be 1..interest_radius"));
                        }
                        reach = reach.max(*within);
                        CondRt::Sees { who: *who, within: *within }
                    }
                    Cond::AttrBelow { attr, value } | Cond::AttrAbove { attr, value } => {
                        if !value.is_finite() {
                            return Err(format!("{rc}: value must be a number"));
                        }
                        let i = idx(attr, &rc)? as usize;
                        if matches!(r.when, Cond::AttrBelow { .. }) {
                            CondRt::AttrBelow { attr: i, value: *value }
                        } else {
                            CondRt::AttrAbove { attr: i, value: *value }
                        }
                    }
                    Cond::InPlace { place } => CondRt::InPlace { place: place_ix(place, &rc)? },
                    Cond::Chance { pct } => {
                        if !(pct.is_finite() && (0.0..=100.0).contains(pct)) {
                            return Err(format!("{rc}: chance must be 0..100"));
                        }
                        CondRt::Chance { pct: *pct }
                    }
                };
                let then = match &r.then {
                    Act::Idle => ActRt::Idle,
                    Act::Wander => ActRt::Wander,
                    Act::Chase => ActRt::Chase,
                    Act::Flee => ActRt::Flee,
                    Act::Attack => {
                        if a.attack.is_none() {
                            return Err(format!("{rc}: 'attack' needs the archetype to have an attack action"));
                        }
                        ActRt::Attack
                    }
                    Act::Patrol { route } => ActRt::Patrol { route: route_ix(route, &rc)? },
                    Act::Goto { place } => ActRt::Goto { place: place_ix(place, &rc)? },
                    Act::Say { text, shout } => {
                        let clean = crate::chat::sanitize(text).ok_or_else(|| format!("{rc}: say needs some text"))?;
                        ActRt::Say { text: clean, shout: *shout }
                    }
                };
                rules.push(RuleRt { when, then });
            }
            archs.push(ArchRt {
                home,
                rules,
                reach,
                name: a.name.clone(),
                color: a.color.clone(),
                count: a.count,
                behaviour: a.behaviour,
                speed_mult: a.speed_mult,
                sight: a.sight,
                attack,
                start,
            });
        }
        Ok(Game { defs, archs, places: self.places.clone(), routes: self.routes.clone() })
    }

    pub fn from_json(s: &str) -> Result<GameFile, String> {
        serde_json::from_str(s).map_err(|e| format!("json: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialise")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_file_matches_builtin_defs() {
        let g = GameFile::default_game().build().unwrap();
        assert_eq!(g.defs.content_hash(), Defs::default_game().content_hash());
        assert_eq!(g.archs.len(), 4);
        assert_eq!(g.archs[2].start[0], 60.0);
        assert_eq!(g.archs[2].attack, Some(1));
    }

    #[test]
    fn json_roundtrip() {
        let f = GameFile::default_game();
        let back = GameFile::from_json(&f.to_json()).unwrap();
        assert_eq!(f, back);
    }

    #[test]
    fn minimal_file_uses_defaults() {
        let f = GameFile::from_json(r#"{"attrs":[{"name":"hp","min":0,"max":10,"default":10}]}"#).unwrap();
        let g = f.build().unwrap();
        assert_eq!(g.defs.move_speed, 20.0);
        assert!(g.defs.replicated.is_empty());
    }

    #[test]
    fn editing_content_changes_hash_but_archetypes_do_not() {
        let base = GameFile::default_game();
        let h = base.build().unwrap().defs.content_hash();
        let mut a = base.clone();
        a.archetypes[0].count += 5;
        a.archetypes[1].speed_mult = 2.0;
        assert_eq!(a.build().unwrap().defs.content_hash(), h);
        let mut b = base.clone();
        b.attrs[0].max = 200.0;
        assert_ne!(b.build().unwrap().defs.content_hash(), h);
    }

    fn err(f: impl FnOnce(&mut GameFile)) -> String {
        let mut g = GameFile::default_game();
        f(&mut g);
        g.build().unwrap_err()
    }

    #[test]
    fn bad_input_gives_readable_errors() {
        assert!(err(|g| g.attrs[0].min = 500.0).contains("min must be below max"));
        assert!(err(|g| g.attrs[1].name = "health".into()).contains("duplicate attribute"));
        assert!(err(|g| g.attrs[0].bits = 12).contains("bits"));
        assert!(err(|g| g.actions[0].effects[0].attr = "mana".into()).contains("unknown attribute 'mana'"));
        assert!(err(|g| g.actions[0].id = 0).contains("reserved"));
        assert!(err(|g| g.archetypes[2].attack = Some("nuke".into())).contains("unknown attack"));
        assert!(err(|g| g.archetypes[0].color = "red".into()).contains("color"));
        assert!(err(|g| g.attrs[0].default = f32::NAN).contains("finite"));
        assert!(err(|g| g.max_visible = 200).contains("snapshot"));
        assert!(err(|g| {
            for a in g.attrs.iter_mut() {
                a.replicated = true;
            }
            for n in ["a", "b", "c"] {
                g.attrs.push(AttrFile { name: n.into(), min: 0.0, max: 1.0, default: 0.0, bits: 8, replicated: true, regen_per_sec: 0.0, on_zero: OnZeroFile::None });
            }
        })
        .contains("replicated"));
        assert!(GameFile::from_json(r#"{"attrs":[],"bogus":1}"#).is_err());
    }

    fn with(f: impl FnOnce(&mut GameFile)) -> Result<Game, String> {
        let mut g = GameFile::default_game();
        f(&mut g);
        g.build()
    }

    #[test]
    fn rules_places_and_routes_round_trip_through_json() {
        let g = GameFile::default_game();
        let back = GameFile::from_json(&g.to_json()).unwrap();
        assert_eq!(g, back);
        let guard = g.build().unwrap().archs.into_iter().find(|a| a.name == "guard").unwrap();
        assert_eq!(guard.rules.len(), 2);
        assert_eq!(guard.home, Some(0));
        assert_eq!(guard.reach, 40.0);
    }

    #[test]
    fn old_files_without_rules_places_or_routes_still_load() {
        let old = r#"{"attrs":[{"name":"health","min":0,"max":100,"default":100,"bits":8,"replicated":true,"regen_per_sec":0,"on_zero":"none"}],
            "archetypes":[{"name":"x","behaviour":"wander"}]}"#;
        let g = GameFile::from_json(old).unwrap().build().unwrap();
        assert!(g.archs[0].rules.is_empty() && g.places.is_empty() && g.routes.is_empty());
    }

    #[test]
    fn bad_rules_places_and_routes_are_explained() {
        let guard = |g: &mut GameFile| g.archetypes.iter().position(|a| a.name == "guard").unwrap();
        let cases: Vec<(&str, Box<dyn Fn(&mut GameFile)>)> = vec![
            ("unknown place", Box::new(move |g| { let i = guard(g); g.archetypes[i].home = Some("nowhere".into()); })),
            ("unknown route", Box::new(move |g| { let i = guard(g); g.archetypes[i].rules[1].then = Act::Patrol { route: "none".into() }; })),
            ("unknown attribute", Box::new(move |g| { let i = guard(g); g.archetypes[i].rules[0].when = Cond::AttrBelow { attr: "mana".into(), value: 1.0 }; })),
            ("needs the archetype to have an attack", Box::new(move |g| { let i = guard(g); g.archetypes[i].rules[1].then = Act::Attack; })),
            ("some text", Box::new(move |g| { let i = guard(g); g.archetypes[i].rules[0].then = Act::Say { text: " \n ".into(), shout: false }; })),
            ("chance must be", Box::new(move |g| { let i = guard(g); g.archetypes[i].rules[0].when = Cond::Chance { pct: 150.0 }; })),
            ("'within'", Box::new(move |g| { let i = guard(g); g.archetypes[i].rules[0].when = Cond::Sees { who: Seek::Any, within: 100000.0 }; })),
            ("at most 16 rules", Box::new(move |g| { let i = guard(g); let r = g.archetypes[i].rules[1].clone(); g.archetypes[i].rules = vec![r; 17]; })),
            ("unique", Box::new(|g| g.places.push(g.places[0].clone()))),
            ("x and y", Box::new(|g| g.places[0].x = f32::NAN)),
            ("r 1..512", Box::new(|g| g.places[0].r = 0.0)),
            ("needs 2", Box::new(|g| g.routes[0].points.truncate(1))),
            ("points must be", Box::new(|g| g.routes[0].points[0] = [-5.0, 1.0])),
        ];
        for (needle, edit) in cases {
            let err = with(|g| edit(g)).expect_err(needle);
            assert!(err.contains(needle), "expected '{needle}' in '{err}'");
        }
    }

    #[test]
    fn unknown_condition_kinds_are_rejected_at_parse_time() {
        let bad = GameFile::default_game().to_json().replace("\"always\"", "\"whenever\"");
        assert!(GameFile::from_json(&bad).is_err());
    }

    #[test]
    fn say_text_is_cleaned_when_the_file_is_built() {
        let g = with(|g| {
            let i = g.archetypes.iter().position(|a| a.name == "guard").unwrap();
            g.archetypes[i].rules[0].then = Act::Say { text: "  hi\u{7}   there ".into(), shout: true };
        })
        .unwrap();
        let guard = g.archs.iter().find(|a| a.name == "guard").unwrap();
        assert!(matches!(&guard.rules[0].then, ActRt::Say { text, shout: true } if text == "hi there"));
    }
}

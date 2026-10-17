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
}

#[derive(Clone, Debug)]
pub struct Game {
    pub defs: Defs,
    pub archs: Vec<ArchRt>,
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
        };
        let mut drone = bot("drone", "#e0457b", 4, Behaviour::Chase, 1.0, Some("zap"));
        drone.attrs.insert("health".into(), 60.0);
        GameFile {
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
            archs.push(ArchRt {
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
        Ok(Game { defs, archs })
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
        assert_eq!(g.archs.len(), 3);
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
}

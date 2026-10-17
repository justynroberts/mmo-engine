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
    /// What it may leave behind when it dies.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drops: Vec<DropDef>,
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
    /// Hand-placed walls as `[x0, y0, x1, y1]`. Absent means the world's walls are generated from its seed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub walls: Option<Vec<[f32; 4]>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<ItemDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weather: Vec<WeatherDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventDef>,
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


// ---------------------------------------------------------------- items

pub const MAX_ITEMS: usize = 32;
/// Inventory slots per player.
pub const INV_SLOTS: usize = 8;
/// Action ids the client uses for items, so no wire change is needed for them: use slot N is ACT_USE + N, drop
/// slot N is ACT_DROP + N, and ACT_PICKUP takes the nearest thing on the ground in reach. Game actions stay below.
pub const ACT_USE: u8 = 200;
pub const ACT_DROP: u8 = 216;
pub const ACT_PICKUP: u8 = 250;
/// How close you must be to pick something up, in world units.
pub const PICKUP_RANGE: f32 = 3.0;

/// A change to one of the user's attributes when an item is used.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UseEffect {
    pub attr: String,
    pub delta: f32,
}

/// A kind of item: what it looks like on the ground and in the bag, how many fit in a slot, and what using it does.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub name: String,
    #[serde(default = "d_item_color")]
    pub color: String,
    /// How many share a slot (1..99).
    #[serde(default = "d_stack")]
    pub stack_max: u8,
    /// Changes applied to whoever uses it. An item with none can be carried and dropped but does nothing.
    #[serde(default)]
    pub uses: Vec<UseEffect>,
    /// Whether using it uses one up.
    #[serde(default = "d_true")]
    pub consume: bool,
}
fn d_item_color() -> String {
    "#cccc66".into()
}
fn d_stack() -> u8 {
    10
}
fn d_true() -> bool {
    true
}
fn d_one_u8() -> u8 {
    1
}

/// A chance that a bot of this type leaves an item behind when it dies.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DropDef {
    pub item: String,
    #[serde(default = "d_chance")]
    pub chance_pct: f32,
    #[serde(default = "d_one_u8")]
    pub min: u8,
    #[serde(default = "d_one_u8")]
    pub max: u8,
}

#[derive(Clone, Debug)]
pub struct ItemRt {
    pub name: String,
    pub color: String,
    pub stack_max: u8,
    pub uses: Vec<(usize, f32)>,
    pub consume: bool,
}

#[derive(Clone, Debug)]
pub struct DropRt {
    pub item: usize,
    pub chance_pct: f32,
    pub min: u8,
    pub max: u8,
}

// ---------------------------------------------------------------- weather and events

pub const MAX_WEATHER: usize = 16;
pub const MAX_EVENTS: usize = 32;
pub const MAX_STEPS: usize = 12;
pub const MAX_EVENT_SPAWN: u32 = 500;

/// A per-second change to one attribute of everything in the world while a weather is active.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Drain {
    pub attr: String,
    /// Negative hurts, positive heals.
    pub per_sec: f32,
}

/// A kind of weather. Clients are told its name, colour and intensity (to draw it); the server applies the
/// effects: attribute drains on every player and bot, and how far bots can see.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WeatherDef {
    pub name: String,
    #[serde(default = "d_weather_color")]
    pub color: String,
    /// 0 (barely there) to 1 (thick), for how it is drawn.
    #[serde(default = "d_half")]
    pub intensity: f32,
    /// Multiplies how far bots see: fog under 1, clear air over it.
    #[serde(default = "d_one")]
    pub sight_mult: f32,
    #[serde(default)]
    pub drain: Vec<Drain>,
}
fn d_weather_color() -> String {
    "#8899aa".into()
}
fn d_half() -> f32 {
    0.5
}
fn d_duration() -> u32 {
    120
}
fn d_spawn_count() -> u32 {
    5
}
fn d_chance() -> f32 {
    100.0
}

/// One thing an event does. Steps run in order, all in the same tick.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// A server message to every player.
    Announce { text: String },
    /// Start a weather for a while, or with no name end the current one.
    Weather {
        #[serde(default)]
        name: Option<String>,
        #[serde(default = "d_duration")]
        duration_secs: u32,
    },
    /// Add bots, spread over a place or anywhere.
    Spawn {
        archetype: String,
        #[serde(default = "d_spawn_count")]
        count: u32,
        #[serde(default)]
        place: Option<String>,
    },
    /// Change an attribute once for everything inside a place, or everywhere.
    Hit {
        attr: String,
        delta: f32,
        #[serde(default)]
        place: Option<String>,
    },
    /// Remove every live bot of a type.
    Clear { archetype: String },
    /// Put items on the ground, scattered over a place or anywhere.
    Drop {
        item: String,
        #[serde(default = "d_spawn_count")]
        count: u32,
        #[serde(default)]
        place: Option<String>,
    },
}

/// Something that happens: on demand from the console, or on a timer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    pub name: String,
    /// Run itself every this many seconds (at least 5). 0 means only when fired by hand.
    #[serde(default)]
    pub every_secs: u32,
    /// Each time it comes due, the chance in percent that it really happens.
    #[serde(default = "d_chance")]
    pub chance_pct: f32,
    #[serde(default)]
    pub steps: Vec<Step>,
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
    pub drops: Vec<DropRt>,
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
    pub items: Vec<ItemRt>,
    pub places: Vec<Place>,
    pub routes: Vec<Route>,
}

/// Most walls a game file may place.
pub const MAX_WALLS: usize = 2000;

/// A usable item with the given effects (none makes it a keepsake or trade good).
fn item(name: &str, color: &str, stack_max: u8, uses: &[(&str, f32)]) -> ItemDef {
    ItemDef { name: name.into(), color: color.into(), stack_max, uses: uses.iter().map(|&(a, d)| UseEffect { attr: a.into(), delta: d }).collect(), consume: true }
}

/// What the sample bots leave behind.
fn sample_drops(name: &str) -> Vec<DropDef> {
    let d = |item: &str, chance_pct: f32, min: u8, max: u8| DropDef { item: item.into(), chance_pct, min, max };
    match name {
        "drifter" => vec![d("scrap", 35.0, 1, 2)],
        "civilian" => vec![d("ration", 50.0, 1, 2), d("scrap", 20.0, 1, 1)],
        "drone" => vec![d("battery", 60.0, 1, 1), d("scrap", 40.0, 1, 3), d("power core", 8.0, 1, 1)],
        "hound" => vec![d("medkit", 40.0, 1, 1), d("toxin vial", 15.0, 1, 1)],
        "guard" => vec![d("stim", 30.0, 1, 1), d("ration", 30.0, 1, 1)],
        "merchant" => vec![d("scrap", 80.0, 2, 5), d("data chip", 25.0, 1, 1)],
        "sentry" => vec![d("power core", 35.0, 1, 1), d("data chip", 20.0, 1, 1)],
        _ => Vec::new(),
    }
}

impl GameFile {
    /// The world this file describes: its own walls, or the ones generated from `seed`.
    pub fn world(&self, size: u16, seed: u64) -> Result<crate::world::World, String> {
        let Some(walls) = &self.walls else { return Ok(crate::world::World::generate(size, seed)) };
        if walls.len() > MAX_WALLS {
            return Err(format!("at most {MAX_WALLS} walls"));
        }
        let sf = size as f32;
        let mut boxes = Vec::with_capacity(walls.len());
        for (i, w) in walls.iter().enumerate() {
            if w.iter().any(|v| !v.is_finite()) || w[0] < 0.0 || w[1] < 0.0 || w[2] > sf || w[3] > sf || w[0] >= w[2] || w[1] >= w[3] {
                return Err(format!("wall {i} {w:?} must satisfy 0 <= x0 < x1 <= {size} and 0 <= y0 < y1 <= {size}"));
            }
            boxes.push(crate::world::Aabb { x0: w[0], y0: w[1], x1: w[2], y1: w[3] });
        }
        Ok(crate::world::World::from_boxes(sf, seed, boxes))
    }

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
            drops: sample_drops(name),
            rules: Vec::new(),
        };
        let mut drone = bot("drone", "#e0457b", 4, Behaviour::Chase, 1.0, Some("zap"));
        drone.attrs.insert("health".into(), 60.0);
        // Examples of the rule builder: guards who patrol and warn players away, merchants who work a market loop and call out to passers-by, hounds that hunt fast, and sentries that hold a watchtower and attack whoever comes close.
        let mut guard = bot("guard", "#6c8ef5", 3, Behaviour::Wander, 0.7, None);
        guard.home = Some("plaza".into());
        guard.rules = vec![
            Rule { when: Cond::Sees { who: Seek::Player, within: 30.0 }, then: Act::Say { text: "Move along, citizen.".into(), shout: false } },
            Rule { when: Cond::Always, then: Act::Patrol { route: "ring road".into() } },
        ];
        let mut merchant = bot("merchant", "#d98f2b", 6, Behaviour::Wander, 0.6, None);
        merchant.home = Some("market".into());
        merchant.rules = vec![
            Rule { when: Cond::Sees { who: Seek::Player, within: 25.0 }, then: Act::Say { text: "Fresh wares, friend!".into(), shout: false } },
            Rule { when: Cond::Always, then: Act::Patrol { route: "market loop".into() } },
        ];
        let mut hound = bot("hound", "#b5e853", 3, Behaviour::Chase, 1.3, Some("zap"));
        hound.attrs.insert("health".into(), 40.0);
        hound.sight = 60.0;
        let mut sentry = bot("sentry", "#9b6cf5", 4, Behaviour::Idle, 0.9, Some("zap"));
        sentry.home = Some("watchtower".into());
        sentry.rules = vec![
            Rule { when: Cond::Sees { who: Seek::Player, within: 35.0 }, then: Act::Attack },
            Rule { when: Cond::Always, then: Act::Goto { place: "watchtower".into() } },
        ];
        GameFile {
            places: vec![
                Place { name: "plaza".into(), x: 200.0, y: 200.0, r: 20.0 },
                Place { name: "market".into(), x: 420.0, y: 160.0, r: 16.0 },
                Place { name: "watchtower".into(), x: 330.0, y: 430.0, r: 10.0 },
            ],
            routes: vec![
                Route { name: "ring road".into(), points: vec![[180.0, 180.0], [300.0, 180.0], [300.0, 300.0], [180.0, 300.0]], looped: true },
                Route { name: "market loop".into(), points: vec![[390.0, 130.0], [450.0, 130.0], [450.0, 190.0], [390.0, 190.0]], looped: true },
            ],
            walls: None,
            items: vec![
                item("medkit", "#ff4d6d", 5, &[("health", 40.0)]),
                item("battery", "#f5d90a", 5, &[("energy", 50.0)]),
                item("ration", "#c98b4a", 8, &[("health", 10.0), ("energy", 10.0)]),
                item("stim", "#35e0c2", 3, &[("health", 25.0), ("energy", 25.0)]),
                item("power core", "#8f7bff", 2, &[("energy", 100.0)]),
                item("toxin vial", "#9be22b", 3, &[("health", -30.0)]),
                item("scrap", "#9aa3ad", 20, &[]),
                item("data chip", "#4fc3ff", 5, &[]),
            ],
            weather: Vec::new(),
            events: Vec::new(),
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
                merchant,
                hound,
                sentry,
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
            if a.id >= ACT_USE {
                return Err(format!("action '{}': ids from {ACT_USE} up are reserved for items", a.name));
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
        let mut item_names = HashSet::new();
        let mut items: Vec<ItemRt> = Vec::new();
        if self.items.len() > MAX_ITEMS {
            return Err(format!("at most {MAX_ITEMS} items"));
        }
        for it in &self.items {
            let ctx = format!("item '{}'", it.name);
            if it.name.trim().is_empty() || it.name.len() > 32 || it.name.trim() != it.name || !item_names.insert(it.name.clone()) {
                return Err(format!("item names must be unique, 1..32 characters, no edge spaces ('{}')", it.name));
            }
            let c = it.color.as_bytes();
            if !(c.len() == 7 && c[0] == b'#' && c[1..].iter().all(|b| b.is_ascii_hexdigit())) {
                return Err(format!("{ctx}: color must look like #aabbcc"));
            }
            if it.stack_max == 0 || it.stack_max > 99 {
                return Err(format!("{ctx}: stack_max must be 1..99"));
            }
            if it.uses.len() > 4 {
                return Err(format!("{ctx}: at most 4 effects"));
            }
            let mut uses = Vec::new();
            for u in &it.uses {
                if !u.delta.is_finite() || u.delta.abs() > 100_000.0 {
                    return Err(format!("{ctx}: effect delta must be a number"));
                }
                let a = self.attrs.iter().position(|x| x.name == u.attr).ok_or_else(|| format!("{ctx}: unknown attribute '{}'", u.attr))?;
                uses.push((a, u.delta));
            }
            items.push(ItemRt { name: it.name.clone(), color: it.color.clone(), stack_max: it.stack_max, uses, consume: it.consume });
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
            let mut drops = Vec::new();
            if a.drops.len() > 8 {
                return Err(format!("{ctx}: at most 8 drops"));
            }
            for d in &a.drops {
                let item = items.iter().position(|x| x.name == d.item).ok_or_else(|| format!("{ctx}: drop of unknown item '{}'", d.item))?;
                if !(d.chance_pct.is_finite() && (0.0..=100.0).contains(&d.chance_pct)) {
                    return Err(format!("{ctx}: drop chance must be 0..100"));
                }
                if d.min == 0 || d.min > d.max || d.max > 99 {
                    return Err(format!("{ctx}: drop amounts need 1 <= min <= max <= 99"));
                }
                drops.push(DropRt { item, chance_pct: d.chance_pct, min: d.min, max: d.max });
            }
            archs.push(ArchRt {
                drops,
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
        self.check_director()?;
        Ok(Game { defs, archs, items, places: self.places.clone(), routes: self.routes.clone() })
    }

    pub fn from_json(s: &str) -> Result<GameFile, String> {
        serde_json::from_str(s).map_err(|e| format!("json: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialise")
    }
}

impl GameFile {
    /// Weather and events refer to attributes, bot types and places by name; make sure every reference lands.
    fn check_director(&self) -> Result<(), String> {
        let attr = |n: &str| self.attrs.iter().any(|a| a.name == n);
        let arch = |n: &str| self.archetypes.iter().any(|a| a.name == n);
        let place = |n: &str| self.places.iter().any(|p| p.name == n);
        let plain = |n: &str, what: &str| -> Result<(), String> {
            if n.trim().is_empty() || n.len() > 32 || n.trim() != n {
                return Err(format!("{what}: name must be 1..32 characters, no edge spaces"));
            }
            Ok(())
        };
        if self.weather.len() > MAX_WEATHER {
            return Err(format!("at most {MAX_WEATHER} weathers"));
        }
        if self.events.len() > MAX_EVENTS {
            return Err(format!("at most {MAX_EVENTS} events"));
        }
        let mut seen = HashSet::new();
        for w in &self.weather {
            plain(&w.name, "weather")?;
            if !seen.insert(w.name.as_str()) {
                return Err(format!("duplicate weather '{}'", w.name));
            }
            if !(0.0..=1.0).contains(&w.intensity) {
                return Err(format!("weather '{}': intensity must be 0..1", w.name));
            }
            if !(w.sight_mult >= 0.1 && w.sight_mult <= 4.0) {
                return Err(format!("weather '{}': sight_mult must be 0.1..4", w.name));
            }
            if w.color.len() != 7 || !w.color.starts_with('#') || !w.color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(format!("weather '{}': colour must look like #aabbcc", w.name));
            }
            if w.drain.len() > 4 {
                return Err(format!("weather '{}': at most 4 drains", w.name));
            }
            for d in &w.drain {
                if !attr(&d.attr) {
                    return Err(format!("weather '{}': unknown attribute '{}'", w.name, d.attr));
                }
                if !d.per_sec.is_finite() || d.per_sec.abs() > 1000.0 {
                    return Err(format!("weather '{}': drain per_sec must be a number within 1000", w.name));
                }
            }
        }
        let mut seen = HashSet::new();
        for e in &self.events {
            plain(&e.name, "event")?;
            if !seen.insert(e.name.as_str()) {
                return Err(format!("duplicate event '{}'", e.name));
            }
            if e.every_secs != 0 && !(5..=86_400).contains(&e.every_secs) {
                return Err(format!("event '{}': every_secs must be 0 (by hand) or 5..86400", e.name));
            }
            if !(0.0..=100.0).contains(&e.chance_pct) {
                return Err(format!("event '{}': chance_pct must be 0..100", e.name));
            }
            if e.steps.is_empty() || e.steps.len() > MAX_STEPS {
                return Err(format!("event '{}': needs 1 to {MAX_STEPS} steps", e.name));
            }
            for (i, st) in e.steps.iter().enumerate() {
                let ctx = format!("event '{}' step {}", e.name, i + 1);
                match st {
                    Step::Announce { text } => {
                        if text.trim().is_empty() || text.len() > 120 {
                            return Err(format!("{ctx}: the message must be 1..120 bytes"));
                        }
                    }
                    Step::Weather { name, duration_secs } => {
                        if let Some(n) = name {
                            if !self.weather.iter().any(|w| &w.name == n) {
                                return Err(format!("{ctx}: unknown weather '{n}'"));
                            }
                            if *duration_secs == 0 || *duration_secs > 86_400 {
                                return Err(format!("{ctx}: duration must be 1..86400 seconds"));
                            }
                        }
                    }
                    Step::Spawn { archetype, count, place: pl } => {
                        if !arch(archetype) {
                            return Err(format!("{ctx}: unknown bot type '{archetype}'"));
                        }
                        if *count == 0 || *count > MAX_EVENT_SPAWN {
                            return Err(format!("{ctx}: count must be 1..{MAX_EVENT_SPAWN}"));
                        }
                        if let Some(p) = pl {
                            if !place(p) {
                                return Err(format!("{ctx}: unknown place '{p}'"));
                            }
                        }
                    }
                    Step::Hit { attr: a, delta, place: pl } => {
                        if !attr(a) {
                            return Err(format!("{ctx}: unknown attribute '{a}'"));
                        }
                        if !delta.is_finite() {
                            return Err(format!("{ctx}: delta must be a number"));
                        }
                        if let Some(p) = pl {
                            if !place(p) {
                                return Err(format!("{ctx}: unknown place '{p}'"));
                            }
                        }
                    }
                    Step::Clear { archetype } => {
                        if !arch(archetype) {
                            return Err(format!("{ctx}: unknown bot type '{archetype}'"));
                        }
                    }
                    Step::Drop { item, count, place: pl } => {
                        if !self.items.iter().any(|i| &i.name == item) {
                            return Err(format!("{ctx}: unknown item '{item}'"));
                        }
                        if *count == 0 || *count > MAX_EVENT_SPAWN {
                            return Err(format!("{ctx}: count must be 1..{MAX_EVENT_SPAWN}"));
                        }
                        if let Some(p) = pl {
                            if !place(p) {
                                return Err(format!("{ctx}: unknown place '{p}'"));
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hand_placed_walls_replace_the_generated_ones_and_are_validated() {
        let mut g = GameFile::default_game();
        assert!(!g.world(512, 1).unwrap().boxes.is_empty(), "no walls given: generated from the seed");
        g.walls = Some(vec![[10.0, 10.0, 20.0, 200.0]]);
        let w = g.world(512, 1).unwrap();
        assert_eq!(w.boxes.len(), 1);
        assert!(w.blocked(15.0, 100.0, 0.5) && !w.blocked(100.0, 100.0, 0.5));
        g.walls = Some(vec![]);
        assert!(g.world(512, 1).unwrap().boxes.is_empty(), "an empty list means an open field");
        for bad in [[20.0, 10.0, 10.0, 20.0], [0.0, 0.0, 600.0, 10.0], [-1.0, 0.0, 5.0, 5.0], [f32::NAN, 0.0, 5.0, 5.0]] {
            g.walls = Some(vec![bad]);
            assert!(g.world(512, 1).is_err(), "{bad:?}");
        }
        g.walls = Some(vec![[0.0, 0.0, 1.0, 1.0]; MAX_WALLS + 1]);
        assert!(g.world(512, 1).is_err());
        // Survives the file round trip, and absent walls stay absent.
        g.walls = Some(vec![[1.0, 2.0, 3.0, 4.0]]);
        assert_eq!(GameFile::from_json(&g.to_json()).unwrap().walls, g.walls);
        assert!(!GameFile::default_game().to_json().contains("walls"));
    }

    use super::*;

    #[test]
    fn default_file_matches_builtin_defs() {
        let g = GameFile::default_game().build().unwrap();
        assert_eq!(g.defs.content_hash(), Defs::default_game().content_hash());
        assert_eq!(g.archs.len(), 7);
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

    #[test]
    fn items_and_drops_are_validated_by_name_and_old_files_still_load() {
        let mut g = GameFile::default_game();
        assert!(g.build().is_ok());
        let ok = g.clone();
        let bad = |f: &dyn Fn(&mut GameFile), needle: &str| {
            let mut x = ok.clone();
            f(&mut x);
            let e = x.build().err().unwrap_or_default();
            assert!(e.contains(needle), "wanted '{needle}' in '{e}'");
        };
        bad(&|x| x.items[0].uses[0].attr = "mana".into(), "unknown attribute");
        bad(&|x| x.items[0].stack_max = 0, "stack_max");
        bad(&|x| x.items[1].name = x.items[0].name.clone(), "unique");
        bad(&|x| x.items[0].color = "red".into(), "color");
        bad(&|x| x.archetypes[0].drops = vec![DropDef { item: "nope".into(), chance_pct: 10.0, min: 1, max: 1 }], "unknown item");
        bad(&|x| x.archetypes[0].drops = vec![DropDef { item: "medkit".into(), chance_pct: 101.0, min: 1, max: 1 }], "chance");
        bad(&|x| x.archetypes[0].drops = vec![DropDef { item: "medkit".into(), chance_pct: 10.0, min: 3, max: 2 }], "amounts");
        bad(&|x| x.actions[0].id = ACT_USE, "reserved for items");
        // A file from before items existed has none and still builds; items never change the wire hash.
        let old: GameFile = serde_json::from_str(&{
            let mut v: serde_json::Value = serde_json::from_str(&ok.to_json()).unwrap();
            v.as_object_mut().unwrap().remove("items");
            for a in v["archetypes"].as_array_mut().unwrap() {
                a.as_object_mut().unwrap().remove("drops");
            }
            v.to_string()
        })
        .unwrap();
        assert!(old.build().is_ok());
        let mut more = ok.clone();
        more.items.push(ItemDef { name: "gadget".into(), color: "#999999".into(), stack_max: 9, uses: vec![], consume: true });
        assert_eq!(ok.build().unwrap().defs.content_hash(), more.build().unwrap().defs.content_hash());
        g.items.clear();
        assert!(g.build().is_err(), "drops of items that no longer exist are caught");
    }

    #[test]
    fn weather_and_events_are_validated_by_name_and_old_files_still_load() {
        let mut g = GameFile::default_game();
        assert!(g.weather.is_empty() && g.events.is_empty());
        assert!(!g.to_json().contains("weather"), "empty lists are not written, so old files round-trip unchanged");
        g.weather.push(WeatherDef { name: "storm".into(), color: "#445566".into(), intensity: 0.8, sight_mult: 0.5, drain: vec![Drain { attr: "health".into(), per_sec: -1.0 }] });
        g.events.push(EventDef {
            name: "raid".into(),
            every_secs: 60,
            chance_pct: 50.0,
            steps: vec![
                Step::Announce { text: "Raiders at the plaza".into() },
                Step::Weather { name: Some("storm".into()), duration_secs: 30 },
                Step::Spawn { archetype: g.archetypes[0].name.clone(), count: 5, place: Some("plaza".into()) },
                Step::Hit { attr: "health".into(), delta: -10.0, place: Some("plaza".into()) },
                Step::Clear { archetype: g.archetypes[0].name.clone() },
            ],
        });
        g.build().expect("a consistent game builds");
        let back = GameFile::from_json(&g.to_json()).unwrap();
        assert_eq!(back, g);

        let bad = |f: &dyn Fn(&mut GameFile), needle: &str| {
            let mut h = g.clone();
            f(&mut h);
            let e = h.build().err().unwrap_or_else(|| panic!("should refuse: {needle}"));
            assert!(e.contains(needle), "{e} should mention {needle}");
        };
        bad(&|h| h.weather[0].drain[0].attr = "mana".into(), "unknown attribute 'mana'");
        bad(&|h| h.weather[0].intensity = 2.0, "intensity");
        bad(&|h| h.weather[0].color = "red".into(), "colour");
        bad(&|h| h.weather[0].sight_mult = 0.0, "sight_mult");
        bad(&|h| h.events[0].every_secs = 2, "every_secs");
        bad(&|h| h.events[0].steps.clear(), "steps");
        bad(&|h| h.events[0].steps[1] = Step::Weather { name: Some("hail".into()), duration_secs: 5 }, "unknown weather 'hail'");
        bad(&|h| h.events[0].steps[2] = Step::Spawn { archetype: "ghost".into(), count: 1, place: None }, "unknown bot type");
        bad(&|h| h.events[0].steps[2] = Step::Spawn { archetype: g.archetypes[0].name.clone(), count: 9999, place: None }, "count");
        bad(&|h| h.events[0].steps[3] = Step::Hit { attr: "health".into(), delta: -1.0, place: Some("nowhere".into()) }, "unknown place");
        bad(&|h| h.events.push(h.events[0].clone()), "duplicate event");
        // Removing a bot type that an event spawns is caught too.
        bad(&|h| { let n = h.archetypes[0].name.clone(); h.archetypes.retain(|a| a.name != n) }, "unknown bot type");
        // Weather and events change nothing on the wire.
        let mut w = g.clone();
        w.weather.clear();
        w.events.clear();
        assert_eq!(g.build().unwrap().defs.content_hash(), w.build().unwrap().defs.content_hash());
    }
}

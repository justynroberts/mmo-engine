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
    /// Spread `delta` over this many seconds instead of all at once (a burn, a curse, a slow heal). 0 = at once.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub secs: f32,
    /// Take it back after this many seconds (a buff, a curse that lifts): what it changed is undone. 0 = for good.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lasts: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// Who an action is aimed at: another entity (the default), or the one using it (a self heal, a nova round you).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Aim {
    #[default]
    Target,
    #[serde(rename = "self")]
    Caster,
}

/// Which side an action lands on. Players are one side and bots the other: a heal for friends never mends a
/// wolf, and fire for foes never burns the player beside you.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Hits {
    #[default]
    Any,
    Foes,
    Friends,
}

/// Holding a target still: mez (no moving or acting, broken by any damage), stun (the same, not broken) or
/// root (no moving; it can still act).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HoldKind {
    Mez = 1,
    Stun = 2,
    Root = 3,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct HoldFile {
    pub kind: HoldKind,
    pub secs: f32,
}

/// How a spell looks to clients (`ActionFile::fx`); each client draws them its own way.
pub const FX: [&str; 10] = ["fire", "frost", "shock", "holy", "shadow", "nature", "arcane", "poison", "mind", "blood"];
/// The widest an area may be, in world units.
pub const MAX_AREA: f32 = 64.0;
/// The longest a hold or an effect over time may last, in seconds.
pub const MAX_HOLD_SECS: f32 = 30.0;
pub const MAX_OVER_SECS: f32 = 60.0;
/// The longest a temporary effect (`EffectFile::lasts`) or a levitation may last, in seconds.
pub const MAX_LAST_SECS: f32 = 300.0;
/// The reserved attribute that multiplies an entity's speed (1 = as usual). Replicated, it lets a client predict
/// its own hasted or slowed steps.
pub const SPEED_ATTR: &str = "speed";
/// The reserved attribute a held entity shows: its hold kind (1 mez, 2 stun, 3 root) while held, else 0. A game
/// that defines it (replicated) lets clients stop predicting moves the server will refuse, and draw the hold.
pub const HELD_ATTR: &str = "held";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    pub attr: String,
    pub min: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
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
    /// Spells. Every field below is optional and none is in the content hash, so they apply live.
    #[serde(default, skip_serializing_if = "is_default")]
    pub aim: Aim,
    /// Radius around the target (or the caster, aimed at self): effects "to target" land on everyone in it. 0 = one.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub area: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub hits: Hits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hold: Option<HoldFile>,
    /// How it looks: one of `FX`, or empty for a plain blow.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fx: String,
    /// Clients draw it flying from the caster to the target.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bolt: bool,
    /// Free text to group spells by in a spellbook ("black", "white", ...).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub school: String,
    /// What it does, in words players read (a tooltip), whatever the spell is called.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desc: String,
    /// Lift everyone it lands on off the ground for this many seconds: blows and areas miss them, as in a jump.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub lift: f32,
    /// The level a player must be to use it (a spell learnt later). Bots use what their type is given.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub level: u8,
    /// Used on a boss, it puts the user at the top of its threat (`Archetype::boss`): it turns on them.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub taunt: bool,
}

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
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

/// Where a townsperson works by day (`Archetype::work`); found in the world file round its home, so one bot type
/// works in every town it lives in. By night it is home (its `indoors` rules), and at dusk it may be at the square.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Work {
    /// A stall or doorway on one of the town's squares (traders, townsfolk).
    Square,
    /// The water's edge nearest the town: the sea or a lake (fishers).
    Water,
    /// The fields just beyond the town (farmers, herders).
    Fields,
    /// Up and down the town's roads (the watch).
    Streets,
    /// About the square, playing chase with the others at play (children).
    Play,
}

/// The time of year (`Rules::season_days`): it changes how the land looks, how crops grow, what food costs and how
/// hard the nights are.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Season {
    Spring,
    Summer,
    Autumn,
    Winter,
}

impl Season {
    pub const ALL: [Season; 4] = [Season::Spring, Season::Summer, Season::Autumn, Season::Winter];
    /// The season `day` (`Daylight::day`) falls in, `days` to a season, `shift` days on: None with no seasons.
    pub fn of(day: f64, days: f32, shift: f32) -> Option<Season> {
        (days > 0.0).then(|| Season::ALL[(((day + shift as f64) / days as f64).floor() as i64).rem_euclid(4) as usize])
    }
    pub fn name(self) -> &'static str {
        match self {
            Season::Spring => "spring",
            Season::Summer => "summer",
            Season::Autumn => "autumn",
            Season::Winter => "winter",
        }
    }
}

/// A named circular area: a spawn point, a hangout, a market square. `name` is what rules and events refer to;
/// `title`, if set, is what players are told when they walk in or out ("Hollow Market"), and makes it a location.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub name: String,
    pub x: f32,
    pub y: f32,
    #[serde(default = "d_place_r")]
    pub r: f32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Makes the place a zone line: a player who walks in arrives at `x, y` in `zone` (a zone of a `--zone-lines`
    /// world, by name), or on this map when `zone` is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit: Option<Exit>,
    /// What a client plays inside it over the sounds of the land round it (one of `SOUNDS`); empty for nothing extra.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sound: String,
    /// The music a client plays inside it (one of `MUSIC`); empty for whatever the moment calls for.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub music: String,
    /// Players cannot harm each other inside it, even with `pvp` on (a town, a sanctuary).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub safe: bool,
}
/// The ambiences a place may have, and its music (the client's sounds: godot/sounds, made by scripts/make_audio.py).
pub const SOUNDS: [&str; 12] = ["sea", "river", "forest", "night", "wind", "rain", "fire", "town", "market", "crypt", "holy", "battle"];
pub const MUSIC: [&str; 10] = ["explore", "town", "night", "battle", "eerie", "holy", "city", "capital", "tavern", "none"];
fn d_place_r() -> f32 {
    12.0
}

/// Where a zone line leads.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct Exit {
    /// The zone's name (`--zone-names`); empty for somewhere on this map.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub zone: String,
    pub x: f32,
    pub y: f32,
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
    /// Sees a bot of one type (by name) within `within`: the one targeted by chase, flee and attack.
    SeesKind {
        bot: String,
        #[serde(default = "d_sight")]
        within: f32,
    },
    /// The time of day is in one of these parts (`DayPart::during`): dawn the early morning, day the rest of the
    /// daylight, dusk the evening, night the dark hours.
    During {
        parts: Vec<DayPart>,
    },
    /// The weather where the bot stands is one of these (by name); with none listed, any weather at all.
    Weather {
        #[serde(default)]
        names: Vec<String>,
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
    /// Go home (the type's `home` place) and go indoors: once there it is out of sight until another rule moves it.
    Indoors,
    /// Say one of these lines, picked at random, to players close by: shown over its head, not in the chat. Like
    /// `say`, it has no effect on movement.
    Chat {
        lines: Vec<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub when: Cond,
    pub then: Act,
}

pub const MAX_RULES: usize = 16;
pub const MAX_PLACES: usize = 256;
/// The widest a place may be (a lake, a mountain range, a weather zone's ground on a big map), in world units.
pub const MAX_PLACE_R: f32 = 8192.0;
/// Longest place title players see, in characters.
pub const MAX_TITLE: usize = 40;
pub const MAX_ROUTES: usize = 64;
pub const MAX_ROUTE_POINTS: usize = 64;

/// Where a fallen bot comes back.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RespawnAt {
    #[default]
    Anywhere,
    Home,
    Here,
}

impl RespawnAt {
    pub fn is_anywhere(&self) -> bool {
        *self == RespawnAt::Anywhere
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Archetype {
    pub name: String,
    /// What players see it called (absent: its name, each word capitalised), so a type can keep a short name in
    /// the file and go by something else in the world, "Trader" for "ceannai".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
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
    /// Experience for beating one (shared by every player who hurt it in the last half minute).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub xp: u32,
    /// Its level (1 when absent): it hits and takes blows as a player of that level would.
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub level: u8,
    /// What it sells (talk to one to trade): items at a price in the game's `currency`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shop: Vec<ShopItem>,
    /// Seconds one lasts before it is gone of itself (0 for as long as it lives): a fire burns down, a fair ends.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub lasts_secs: u32,
    /// Seconds one that falls stays gone before it comes back (0, at once).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub respawn_secs: f32,
    /// Where it comes back: anywhere free (the default), in its home place, or where it fell (a quarry, a tree).
    #[serde(default, skip_serializing_if = "RespawnAt::is_anywhere")]
    pub respawn_at: RespawnAt,
    /// How far a player it is fighting may get before it gives up the chase, in world units. 0 (absent) means
    /// three times its sight, and at least `MIN_LEASH`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub leash: f32,
    /// The faction it belongs to (`GameFile::factions`, by name): beating one changes a player's standing with it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<String>,
    /// A loot table (`GameFile::loot`) rolled when it falls, on top of its `drops`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loot: Option<String>,
    /// Given names for the bots of this type, one each ("Bridget" the Seed Trader): absent, they go by the type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<String>,
    /// Family names to go with `names` ("Bridget Kavanagh"): each bot gets a pairing no other bot in the world has
    /// (a name with a space in it, "Brother Aidan", stands alone).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surnames: Vec<String>,
    /// More places it lives in besides `home`: its bots are shared out over all of them, one each in turn, so one
    /// type of townsfolk can people every town.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub homes: Vec<String>,
    /// It comes for players out in the dark (`Rules::night_ambush_pct`): spawned near them at night, gone by day.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ambush: bool,
    /// Its trade: where in its home it spends the day (`Work`). None keeps it wandering the streets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work: Option<Work>,
    /// A cutpurse: it lifts coin from those it stands beside (`Rules::lift_pct`), runs from the watch once the
    /// cry goes up, and drops what it took when it falls.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thief: bool,
    /// A dog: it keeps at the heel of one of the townsfolk of its home, waits at their door while they are in, and
    /// barks at strangers and at a cutpurse on the run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pet: bool,
    /// A beast from a farm that wanders off (a cow, a goat): townsfolk ask a player to bring it home, and it follows
    /// whoever leads it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stray: bool,
    /// Tends the sick in a town the plague is in (a plague doctor): goes to them and, now and then, cures one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub healer: bool,
    /// A boss: it fights by threat and a script of moves, phases and adds (`BossFile`) as well as its rules.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boss: Option<BossFile>,
}

/// How a boss fights (`Archetype::boss`, server `boss.rs`). Not in the content hash: it applies live.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BossFile {
    /// In order of `below`, highest first; the first is where the fight starts. At least one.
    pub phases: Vec<BossPhase>,
    /// Seconds into a fight it enrages (0, never): its blows then count `enrage_might` times.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub enrage_secs: f32,
    #[serde(default = "d_enrage_might")]
    pub enrage_might: f32,
    /// Said as a banner to those fighting it when it falls.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub defeat: String,
}

fn d_enrage_might() -> f32 {
    3.0
}

/// One stretch of a boss fight: from when its health falls below `below` percent until the next phase.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BossPhase {
    /// Health percent at which this phase begins (100 for the first).
    pub below: f32,
    /// A banner for those fighting it, as the phase begins.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub say: String,
    #[serde(default)]
    pub moves: Vec<BossMove>,
    /// Bots that come to its side as the phase begins, round it, already in the fight; gone if it resets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub adds: Vec<BossAdd>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BossAdd {
    pub bot: String,
    pub count: u32,
}

/// Who or where a boss move is aimed at.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum MoveAt {
    /// The one at the top of its threat.
    #[default]
    Tank,
    /// Anyone in the fight, at random.
    Random,
    /// The one in the fight farthest from it.
    Farthest,
    /// The ground round the boss itself.
    #[serde(rename = "self")]
    Boss,
}

/// An action a boss uses every so often. With `warn_secs` the spot is marked on the ground first and the blow
/// lands on whoever is still in it when the time is up.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct BossMove {
    /// An action (by name): its effects, hold, lift and look.
    pub action: String,
    pub every_secs: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub warn_secs: f32,
    #[serde(default, skip_serializing_if = "is_default")]
    pub at: MoveAt,
    /// The marked circle's radius (0: the action's own area, or a small circle if it has none).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub radius: f32,
    /// A banner as it begins (the warning), for those fighting it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub say: String,
}

pub const MAX_BOSS_PHASES: usize = 8;
pub const MAX_BOSS_MOVES: usize = 8;
pub const MAX_BOSS_ADDS: u32 = 16;
/// The longest warning a move may give, in seconds.
pub const MAX_WARN_SECS: f32 = 10.0;

/// A boss's script with names resolved: action ids and bot type indices.
#[derive(Clone, Debug, Default)]
pub struct BossRt {
    pub phases: Vec<BossPhaseRt>,
    pub enrage_secs: f32,
    pub enrage_might: f32,
    pub defeat: String,
}

#[derive(Clone, Debug, Default)]
pub struct BossPhaseRt {
    /// As a fraction of full health (0..1).
    pub below: f32,
    pub say: String,
    pub moves: Vec<BossMoveRt>,
    /// (bot type index, count)
    pub adds: Vec<(usize, u32)>,
}

#[derive(Clone, Debug, Default)]
pub struct BossMoveRt {
    pub action: u8,
    pub every_secs: f32,
    pub warn_secs: f32,
    pub at: MoveAt,
    pub radius: f32,
    pub say: String,
}

/// The shortest leash a bot type gets when it gives none (so a short-sighted one still chases a little way).
pub const MIN_LEASH: f32 = 60.0;
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
    /// How much faster than `move_speed` everyone moves on a road (1 = no faster).
    #[serde(default = "d_road")]
    pub road_speed: f32,
    /// Players may harm each other (outside places marked `safe`); the default. Off, only bots are fair game.
    /// Not in the content hash: it applies live.
    #[serde(default = "d_true", skip_serializing_if = "is_true")]
    pub pvp: bool,
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
    /// Quests bots give players (`QuestFile`). Not in the content hash: edits apply live.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quests: Vec<QuestFile>,
    /// Weather that stays in one place: a place and a weather, drawn as mist over it and in effect only inside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zones: Vec<WeatherZone>,
    /// When it is light and dark (`daylight.rs`). Absent is the default cycle. Not in the content hash: an edit
    /// applies live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daylight: Option<crate::daylight::Daylight>,
    /// Experience and levels; absent, there are none. Not in the content hash: an edit applies live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub levels: Option<LevelsFile>,
    /// The item that is money: what shops take and pay (`Archetype::shop`, `ItemDef::value`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    /// Character classes a player chooses from once (`ClassFile`). Not in the content hash: edits apply live.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub classes: Vec<ClassFile>,
    /// Limits on building, against one player (or a crowd) filling the map. Not in the content hash.
    #[serde(default, skip_serializing_if = "BuildLimits::is_default")]
    pub building: BuildLimits,
    /// Factions bot types belong to, and how players stand with them. Not in the content hash: edits apply live.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub factions: Vec<FactionFile>,
    /// Loot tables, crafting, farming, land and dungeons (`realm.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loot: Vec<crate::realm::LootTable>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recipes: Vec<crate::realm::Recipe>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub crops: Vec<crate::realm::CropDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plots: Option<crate::realm::PlotRules>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dungeons: Vec<crate::realm::DungeonDef>,
    /// Tuning numbers: reaches, timers, sizes (`rules.rs`). Not in the content hash: edits apply live.
    #[serde(default, skip_serializing_if = "crate::rules::Rules::is_default")]
    pub rules: crate::rules::Rules,
    /// What a new character starts with in their bag (a guest, each time they come). Not in the content hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub starter: Vec<crate::realm::Stack>,
    /// Where new characters come into the world, and those whose saved spot is gone (a new map): a place by name.
    /// Absent, anywhere free. Not in the content hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_place: Option<String>,
    /// "/" commands that act something out for those nearby (`/wave`, `/bow Name`). Not in the content hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emotes: Vec<Emote>,
    /// Players who may take god mode in the game (`/god`, the G key) by their account tag (`secure::account_tag`,
    /// 16 hex digits): the identity key proves who they are, so no admin console has to be reachable. Not in the
    /// content hash.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gods: Vec<String>,
    /// What townsfolk say, by occasion (`proto::talk`): any occasion named here replaces its default lines. Not in
    /// the content hash.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub talk: std::collections::BTreeMap<String, Vec<String>>,
}

/// An emote: `/cmd` says `text` to everyone in `say_reach`, or `at` when a name follows (`/wave Bob`); `{name}` is
/// who acts it, `{target}` who it is at. `anim` is a clip the client plays on the one who acts it, if their model
/// has one by that name (chat channel `EMOTE`, "anim|line").
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Emote {
    pub cmd: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub anim: String,
    /// Held until they move (`/sit`, `/sleep`): shown in `POSE_ATTR`, so clients keep playing `anim` and those who
    /// come by later see it too.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stays: bool,
}

/// If the game file has an attribute of this name, the server keeps it at the held emote a player is in (its index
/// + 1, 0 for none: `Emote::stays`), until they move. Make it replicated and clients draw who is sitting.

impl Emote {
    /// The line for `who` (and `target`, when one was named and the emote has an `at` line).
    pub fn line(&self, who: &str, target: Option<&str>) -> String {
        match target.filter(|_| !self.at.is_empty()) {
            Some(t) => self.at.replace("{name}", who).replace("{target}", t),
            None => self.text.replace("{name}", who),
        }
    }
}

/// How much may be built: blocks per player (by account; guests' blocks belong to nobody and count only against
/// the region) and per region (`blockfeed::REGION` cells square, 256 world units). 0 means no limit.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildLimits {
    #[serde(default = "d_per_player")]
    pub per_player: u32,
    #[serde(default = "d_per_region")]
    pub per_region: u32,
}
fn d_per_player() -> u32 {
    20_000
}
fn d_per_region() -> u32 {
    60_000
}
impl Default for BuildLimits {
    fn default() -> Self {
        BuildLimits { per_player: d_per_player(), per_region: d_per_region() }
    }
}
impl BuildLimits {
    pub fn is_default(&self) -> bool {
        *self == BuildLimits::default()
    }
}

pub const MAX_FACTIONS: usize = 32;
/// Standing runs from -STANDING_MAX (hated) to STANDING_MAX (revered); everyone starts at 0.
pub const STANDING_MAX: i32 = 1000;

/// A faction: who its members are is said by bot types (`Archetype::faction`). Beating one of its members moves a
/// player's standing with it by `kill` (and with every faction that `hates` this one by that faction's `bounty`).
/// Below `hostile`, its members who can fight attack that player on sight, its traders will not deal with them and
/// its quest givers will not speak to them. Standing below 0 creeps back towards 0 by one a minute.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct FactionFile {
    pub name: String,
    /// What players see ("The Folk of the Vale"); empty, the name.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// Its colour, for its members (players who joined it with `/faction`, and its bot types) in clients.
    #[serde(default = "d_faction_color")]
    pub color: String,
    #[serde(default = "d_kill")]
    pub kill: i32,
    #[serde(default = "d_hostile")]
    pub hostile: i32,
    /// Factions whose members falling raises standing with this one, by `bounty` each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hates: Vec<String>,
    #[serde(default = "d_bounty")]
    pub bounty: i32,
}
fn d_faction_color() -> String {
    "#c0c0c0".into()
}
fn d_kill() -> i32 {
    -10
}
fn d_hostile() -> i32 {
    -30
}
fn d_bounty() -> i32 {
    2
}

impl FactionFile {
    /// What players call it.
    pub fn shown(&self) -> String {
        if self.title.is_empty() { crate::names::bot(&self.name) } else { self.title.clone() }
    }

    /// How a standing reads.
    pub fn mood(&self, standing: i32) -> &'static str {
        if standing < self.hostile {
            "hostile"
        } else if standing < 0 {
            "wary"
        } else if standing < 30 {
            "neutral"
        } else if standing < 150 {
            "friendly"
        } else {
            "honoured"
        }
    }
}

/// Experience and levels. A character earns experience for beating bots (their type's `xp`), for finding named
/// places, and from quests; enough of it raises its level, up to `cap`. Each level above the first makes what it
/// does stronger (`might`, a share more per level) and what it suffers weaker (`toughness`), so a bar of health
/// stays a bar of health at every level.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LevelsFile {
    #[serde(default = "d_cap")]
    pub cap: u8,
    /// Experience to reach level 2; level L needs `base * (L - 1) ^ exponent` in all.
    #[serde(default = "d_base_xp")]
    pub base: f32,
    #[serde(default = "d_exponent")]
    pub exponent: f32,
    /// Each level above the first: blows and spells land this share harder (damage and heals alike)...
    #[serde(default = "d_might")]
    pub might: f32,
    /// ...and harm taken is divided by 1 + this share.
    #[serde(default = "d_toughness")]
    pub toughness: f32,
    /// For walking into a named place (a title) for the first time.
    #[serde(default = "d_discover")]
    pub discover_xp: u32,
    /// Life experience: a little for everything a player does.
    #[serde(default)]
    pub life: LifeXp,
    /// What reaching a level gives, besides being stronger: items (coin is an item too) and a line.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rewards: Vec<LevelReward>,
    /// Pools that grow with level (`{"health": 10, "mana": 8}`): a player's ceiling for each starts at its
    /// default and each level above the first adds this much, up to the attribute's max (so set the max to what
    /// the cap should reach). Reaching a level, or rising again, fills it. Players only; bots keep their own.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub grow: BTreeMap<String, f32>,
}

/// Reaching `level` gives `items` (into the bag; what does not fit lands at their feet) and says `text`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LevelReward {
    pub level: u8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<crate::realm::Stack>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
}

/// Life experience: a trickle of experience for living in the world, not only for fighting. Each is experience
/// per deed (fractions add up); 0 turns that one off. It is banked and paid every ten seconds, at most `per_min`
/// a minute, so a busy spell is worth something, but not much; and only new ground and new deeds pay (`trail`, `repeat`),
/// so a macro pacing a loop or repeating one action earns nothing.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields, default)]
pub struct LifeXp {
    /// For each 100 units walked (or ridden, or swum).
    pub walk: f32,
    /// For each action that is used (a blow, a spell).
    pub act: f32,
    /// For each block built or broken.
    pub build: f32,
    /// For everything else done: an item used or picked up, something bought or sold, a word with someone, a quest
    /// taken.
    pub deed: f32,
    /// For each seed planted, and each ripe crop brought in.
    pub plant: f32,
    pub harvest: f32,
    /// For each minute in the world while awake (moved or did something in the last five), however it was spent,
    /// and more for each such minute with another player within speaking reach (`rules.say_reach`).
    pub minute: f32,
    pub company: f32,
    pub per_min: u32,
    /// Ground a player remembers having walked, in 32-unit cells: walking pays, and keeps them awake, only while
    /// it reaches cells not among the last `trail`, so pacing, circling or an input held against a wall earns
    /// nothing once the loop is learnt. 0 turns this off.
    pub trail: u16,
    /// Deeds a player remembers having done (the same block built and broken, the same item dropped and picked
    /// up, the same line said): one among the last `repeat` pays nothing and does not wake them. 0 turns this off.
    pub repeat: u8,
}

impl Default for LifeXp {
    fn default() -> Self {
        LifeXp { walk: 1.5, act: 0.5, build: 0.5, deed: 1.0, plant: 1.0, harvest: 3.0, minute: 1.0, company: 1.0, per_min: 30, trail: 64, repeat: 8 }
    }
}

fn d_cap() -> u8 {
    20
}
fn d_base_xp() -> f32 {
    100.0
}
fn d_exponent() -> f32 {
    1.6
}
fn d_might() -> f32 {
    0.06
}
fn d_toughness() -> f32 {
    0.05
}
fn d_discover() -> u32 {
    10
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn the_curve_climbs_and_stops_at_the_cap() {
        let l = LevelsFile { cap: 20, base: 100.0, exponent: 1.6, might: 0.06, toughness: 0.05, discover_xp: 10, life: Default::default(), rewards: vec![], grow: Default::default() };
        assert_eq!((l.xp_for(1), l.xp_for(2)), (0, 100));
        assert!((2..=20).all(|v| l.xp_for(v) > l.xp_for(v - 1)));
        assert_eq!((l.level_of(0), l.level_of(99), l.level_of(100), l.level_of(u32::MAX)), (1, 1, 2, 20));
        assert!((l.might_at(11) - 1.6).abs() < 1e-6 && (l.toughness_at(1) - 1.0).abs() < 1e-6);
    }
}

impl LevelsFile {
    /// A player's ceiling for an attribute that grows (`grow`) at `level`, from its default, up to its max.
    pub fn ceiling(&self, name: &str, default: f32, max: f32, level: u8) -> Option<f32> {
        self.grow.get(name).map(|per| (default + per * (level.max(1) - 1) as f32).min(max))
    }

    /// Experience needed, in all, to be at `level` (0 for level 1).
    pub fn xp_for(&self, level: u8) -> u32 {
        if level <= 1 {
            return 0;
        }
        (self.base * ((level - 1) as f32).powf(self.exponent)).round() as u32
    }

    /// The level that much experience makes, up to the cap.
    pub fn level_of(&self, xp: u32) -> u8 {
        let mut l = 1;
        while l < self.cap && xp >= self.xp_for(l + 1) {
            l += 1;
        }
        l
    }

    /// How much harder something at `level` hits (and heals).
    pub fn might_at(&self, level: u8) -> f32 {
        1.0 + self.might * (level.max(1) - 1) as f32
    }

    /// What harm taken at `level` is divided by.
    pub fn toughness_at(&self, level: u8) -> f32 {
        1.0 + self.toughness * (level.max(1) - 1) as f32
    }
}

/// The reserved attribute that shows an entity's level (replicated, so everyone sees it on nameplates).
pub const LEVEL_ATTR: &str = "level";
pub const MAX_LEVEL: u8 = 100;
fn d_speed() -> f32 {
    20.0
}
fn is_true(v: &bool) -> bool {
    *v
}

fn d_road() -> f32 {
    crate::world::ROAD_SPEED
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
/// Inventory slots per player, and in a player's bank. The action bar shows the first 8.
pub const INV_SLOTS: usize = 24;
pub const BANK_SLOTS: usize = 24;
/// Bag slots the one-byte item actions reach (`ACT_USE + slot`, `ACT_DROP + slot`); `ACT_BAG` reaches them all.
pub const QUICK_SLOTS: usize = 16;
/// The bag and the bank (`market.rs`): the operation (`bag_op`) in the input's `arg` high byte, a slot in its low
/// byte, a second slot in `target`.
pub const ACT_BAG: u8 = 240;
pub mod bag_op {
    pub const USE: u8 = 0;
    pub const DROP: u8 = 1;
    /// Bag slot to bag slot `target`: onto a stack of the same thing, or swapping places.
    pub const MOVE: u8 = 2;
    /// Half the stack into the first empty slot.
    pub const SPLIT: u8 = 3;
    /// Bag slot into the bank, bank slot back into the bag, bank slot to bank slot `target` (only at a market).
    pub const DEPOSIT: u8 = 4;
    pub const WITHDRAW: u8 = 5;
    pub const BANK_MOVE: u8 = 6;
}
/// Trading with another player (`trade_op` in the high byte of `arg`).
pub const ACT_TRADE: u8 = 241;
pub mod trade_op {
    /// Ask the player `target` (an entity id) to trade, or say yes to them asking.
    pub const ASK: u8 = 0;
    /// Offer what is in bag slot `arg & 0xff`, `target` of it (0: the whole stack).
    pub const OFFER: u8 = 1;
    /// Take back the stack offered at index `arg & 0xff`.
    pub const TAKE_BACK: u8 = 2;
    /// Offer `target` of the game's money.
    pub const COIN: u8 = 3;
    /// Accept what is on the table; when both have, the swap is made.
    pub const READY: u8 = 4;
    pub const CANCEL: u8 = 5;
}
/// The auction house (`auction_op` in the high byte of `arg`; only at a market).
pub const ACT_AUCTION: u8 = 242;
/// Hail a bot (H): `target` is its id, or 0 for the nearest that will stop within `rules.hail_reach`. One that
/// does not fight stands still for `rules.hail_secs` (`Sim::hail`).
pub const ACT_HAIL: u8 = 243;
pub mod auction_op {
    /// A page (`arg & 0xff`) of listings, of item tag `target` (0: everything).
    pub const BROWSE: u8 = 0;
    /// Buy listing `target`.
    pub const BUY: u8 = 1;
    /// Put up bag slot `arg & 0xff` for `target` money.
    pub const SELL: u8 = 2;
    /// Take down your listing `target`.
    pub const CANCEL: u8 = 3;
    /// A page of your own listings.
    pub const MINE: u8 = 4;
}
/// Action ids the client uses for items, so no wire change is needed for them: use slot N is ACT_USE + N, drop
/// slot N is ACT_DROP + N, and ACT_PICKUP takes the nearest thing on the ground in reach. Game actions stay below.
/// Talk to the bot targeted (a quest giver): it offers a quest, takes one handed in, or counts as a "talk" step.
pub const ACT_TALK: u8 = 232;
/// Take the quest offered (the input's `arg` is its index in the game's quests).
pub const ACT_ACCEPT: u8 = 233;
/// Give up a quest (`arg`, its index).
pub const ACT_ABANDON: u8 = 234;
pub const MAX_QUESTS: usize = 512;
/// How many quests a player may have in hand (taken, not yet handed in) at once.
pub const MAX_HELD_QUESTS: usize = 25;
pub const MAX_QUEST_STEPS: usize = 8;

/// A quest: a bot type gives it (`giver`), a player does its steps in order, then hands it in (to `turn_in`, or the
/// giver) for the rewards. `after` makes a chain; `level` holds it back until then.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct QuestFile {
    pub name: String,
    /// What players see: the quest's title, what the giver says offering it, and on handing it in.
    pub title: String,
    #[serde(default)]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub done_text: String,
    /// The bot type that offers it.
    pub giver: String,
    /// The bot type it is handed in to (absent: the giver).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_in: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub level: u8,
    /// A quest that must be done first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<String>,
    pub steps: Vec<QuestStep>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub xp: u32,
    /// Items given on handing it in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reward: Vec<QuestItem>,
    /// May be taken again once done.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub repeatable: bool,
    /// May be taken again once a day (the UTC day), after it is handed in.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub daily: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct QuestItem {
    pub item: String,
    pub count: u8,
}

/// One thing to do. Beating bots counts for everyone who hurt the one that fell, and for players near them.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum QuestStep {
    /// Beat `count` bots of a type.
    Kill { archetype: String, count: u16 },
    /// Walk into a place.
    Visit { place: String },
    /// Carry `count` of an item (taken when the quest is handed in).
    Collect { item: String, count: u8 },
    /// Talk to a bot of a type.
    Talk { archetype: String },
    /// Build `count` blocks, of one material (a `BLOCK_NAMES` entry) or of any.
    Build {
        count: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        material: Option<String>,
    },
}

impl QuestStep {
    /// How many of it there are to do (kills, items), 1 for the rest.
    pub fn need(&self) -> u16 {
        match self {
            QuestStep::Kill { count, .. } => (*count).max(1),
            QuestStep::Collect { count, .. } => (*count).max(1) as u16,
            QuestStep::Build { count, .. } => (*count).max(1),
            _ => 1,
        }
    }
}

pub const ACT_USE: u8 = 200;
pub const ACT_DROP: u8 = 216;
pub const ACT_PICKUP: u8 = 250;
/// Jump: airborne for JUMP_SECS, and while airborne no action can target you (a dodge). Ignored mid-air.
pub const ACT_JUMP: u8 = 251;
pub const JUMP_SECS: f32 = 0.6;
/// Build: put a block in the cell in the target (see `build_target`) at the layer and of the material in the
/// input's `arg` (`build_arg`). It must rest on the ground or touch another block on a face, so blocks bridge gaps
/// and leave windows.
pub const ACT_BUILD: u8 = 252;
/// Break: take out the block in the target's cell at the `arg` layer (`build_arg`, any material). Only its owner
/// can (or anyone, for a block nobody owns).
pub const ACT_BREAK: u8 = 253;
/// Light: put your light on (`arg` 1) or out (`arg` 0). Only does anything when the game has a `LIT_ATTR` attribute.
pub const ACT_LIGHT: u8 = 254;
/// How far from a column's centre you can build or break, in world units, sideways and up or down.
pub const BUILD_REACH: f32 = 8.0;
/// Most blocks in one column: the layers it spans above its base.
pub const MAX_STACK: usize = crate::world::COLUMN_LAYERS as usize;
/// Block materials (the client's theme decides what each looks like; 12 is a lamp and 13 a fire, which give light at
/// night; 14 is tilled soil, and 15 and 16 a crop growing and ripe). Text gives each a character: `0`-`9`, then
/// `a`-`z` (see `block_char`).
pub const BLOCK_KINDS: u8 = 17;
/// Tilled soil: built like any block; a seed planted beside it grows on top of it.
pub const SOIL: u8 = 14;
/// A crop growing, and ripe (`realm::CropDef`). Nobody builds these: planting a seed makes the first, and time the
/// second.
pub const SPROUT: u8 = 15;
pub const RIPE: u8 = 16;
/// What the materials are called, by number (an item's `block` names one). Same order as the clients' lists.
pub const BLOCK_NAMES: [&str; BLOCK_KINDS as usize] = [
    "stone", "timber", "red brick", "plaster", "brick", "rough brick", "roof tiles", "iron", "glass", "slate", "mossy stone", "sandstone", "lamp", "fire", "soil", "sprout", "ripe crop",
];

/// A buildable material by name (any case), or None for an unknown one or a crop.
pub fn material(name: &str) -> Option<u8> {
    BLOCK_NAMES.iter().position(|n| n.eq_ignore_ascii_case(name.trim())).map(|k| k as u8).filter(|&k| k < SPROUT)
}

/// The target of a build or break: the column's cell.
pub fn build_target(cx: u32, cy: u32) -> u32 {
    (cx & 0x7fff) | (cy & 0x7fff) << 15
}

pub fn parse_build_target(t: u32) -> (u32, u32) {
    (t & 0x7fff, (t >> 15) & 0x7fff)
}

/// The input's `arg` for a build or break: the layer in the low byte, the material in the high one.
pub fn build_arg(layer: i8, kind: u8) -> u16 {
    layer as u8 as u16 | (kind as u16) << 8
}

pub fn parse_build_arg(arg: u16) -> (i8, u8) {
    (arg as u8 as i8, (arg >> 8) as u8)
}

/// A material's character in block text (`0`-`9`, `a`-`z`), and back; None for anything else.
pub fn block_char(kind: u8) -> char {
    char::from_digit(kind as u32, 36).unwrap_or('?')
}

pub fn block_kind(c: u8) -> Option<u8> {
    (c as char).to_digit(36).map(|d| d as u8).filter(|&k| k < BLOCK_KINDS)
}
/// If the game file has an attribute of this name, the server keeps it at the share of the current jump still to
/// come (its max at take-off, falling to its min on landing). Make it replicated and every client can draw the arc.
pub const AIRBORNE_ATTR: &str = "airborne";
/// If the game file has an attribute of this name, it says whether a player's light is on (its max) or out (its
/// min), set by `ACT_LIGHT`. Make it replicated and clients hide an unlit player's name in the dark; at night bots
/// see an unlit player only at `Rules::sneak_sight` of their range. That is sneaking.
pub const LIT_ATTR: &str = "lit";

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
    /// What players see it called (absent: its name), so the file can keep "siol coirce" and the bag show
    /// "Oat seed".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
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
    /// What a merchant pays for one (in the game's `currency`); 0, they will not buy it.
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub value: u16,
    /// Worn, not used up: using it puts it on (in its slot), adding to the wearer's might and toughness.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equip: Option<EquipFile>,
    /// A mount: using it rides (or gets down), adding this to the `speed` attribute while riding. Any harm, or
    /// using an action, gets you down.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub mount: f32,
    /// How rare it is: clients colour it by this, and a drop of a rare one or better is announced.
    #[serde(default, skip_serializing_if = "crate::realm::Rarity::is_common")]
    pub rarity: crate::realm::Rarity,
    /// A block of this material (`BLOCK_NAMES`): building one uses one up, and breaking your own gives it back. A
    /// material no item is a block of costs nothing to build.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<String>,
}

fn is_zero_u16(v: &u16) -> bool {
    *v == 0
}

/// Where something is worn, and what it adds (shares, like a level's: 0.1 is a tenth harder or tougher).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EquipFile {
    pub slot: GearSlot,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub might: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub toughness: f32,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GearSlot {
    Weapon = 0,
    Armour = 1,
    Trinket = 2,
}
pub const GEAR_SLOTS: usize = 3;

/// Something a bot type sells, at a price in the game's `currency`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShopItem {
    pub item: String,
    pub price: u16,
}

/// A character class: what it is called and how it plays. A player picks one the first time they come into a
/// game that has classes, and keeps it. `might` and `toughness` add to their level's and their gear's; `spells`
/// are the actions (by name) it may use besides the plain attack (the game's first action), all of them when
/// empty; `look` is the model a client draws it with (the fantasy theme has knight, barbarian, mage, rogue).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ClassFile {
    pub name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub desc: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub look: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub might: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub toughness: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spells: Vec<String>,
    /// Schools of spells it may use as well (`ActionFile::school`): every action of one of these schools, and the
    /// actions with no school (plain blows). `/class Name` picks a class over chat, as the class picker does.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schools: Vec<String>,
}

/// A class as the server uses it: its bonuses and the action ids it may use (`any`: all of them).
#[derive(Clone, Debug)]
pub struct ClassRt {
    pub name: String,
    pub title: String,
    pub desc: String,
    pub look: String,
    pub might: f32,
    pub toughness: f32,
    pub spells: Vec<u8>,
    /// It names neither spells nor schools: every action is its to use.
    pub any: bool,
}

pub const MAX_CLASSES: usize = 15;
pub const POSE_ATTR: &str = "pose";

/// The reserved attribute that shows a player's class: its index + 1, 0 for none (and for every bot), so a client
/// can draw players by class and tell them from bots.
pub const CLASS_ATTR: &str = "class";
/// Choose class `arg` (once: a character keeps the class it chose).
pub const ACT_CLASS: u8 = 238;
/// Rise again after falling: a player whose health runs out stays down where they fell (health held at its
/// minimum, out of sight and out of reach) until they send this, then comes back in the nearest safe place.
pub const ACT_RISE: u8 = 239;

/// Buy item `arg` of the shop of the bot targeted.
pub const ACT_BUY: u8 = 235;
/// Sell what is in bag slot `arg` to the merchant targeted.
pub const ACT_SELL: u8 = 236;
/// Take off what is worn in gear slot `arg` (back into the bag).
pub const ACT_UNEQUIP: u8 = 237;
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
    pub value: u16,
    pub equip: Option<EquipFile>,
    pub mount: f32,
    pub rarity: crate::realm::Rarity,
    /// The material this is a block of.
    pub block: Option<u8>,
    /// `ItemDef::title`, empty when it has none (`shown`).
    pub title: String,
}

impl ItemRt {
    /// What players call it.
    pub fn shown(&self) -> &str {
        if self.title.is_empty() { &self.name } else { &self.title }
    }
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
pub const MAX_ZONES: usize = 32;

/// A weather that stays over one place (red mist round a haunted ruin, green mist hiding a castle): clients draw it as
/// mist in the weather's colour, standing `height` units tall over the place, and inside it the weather's effects
/// (drains, how far bots see) apply as if it were the weather everywhere. Events switch it on and off.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WeatherZone {
    pub name: String,
    pub place: String,
    pub weather: String,
    #[serde(default = "d_true")]
    pub active: bool,
    #[serde(default = "d_zone_height")]
    pub height: f32,
}
fn d_zone_height() -> f32 {
    30.0
}
pub const MAX_STEPS: usize = 24;
/// Longest time an event's waits, time limit and cooldown may name, in seconds (a day).
pub const MAX_EVENT_SECS: u32 = 86_400;
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

fn is_false(b: &bool) -> bool {
    !*b
}

/// One thing an event does. Steps run in order, straight on until one waits (`Wait`, `Defeat`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    /// A server message: to every player, or with `nearby` only to those at the event's place. A `banner` goes
    /// across the middle of the screen as well as into chat.
    Announce {
        text: String,
        #[serde(default, skip_serializing_if = "is_false")]
        banner: bool,
        #[serde(default, skip_serializing_if = "is_false")]
        nearby: bool,
    },
    /// Start a weather for a while, or with no name end the current one.
    Weather {
        #[serde(default)]
        name: Option<String>,
        #[serde(default = "d_duration")]
        duration_secs: u32,
    },
    /// Add bots, spread over a place (the event's, if the step names none) or anywhere. They belong to this run of
    /// the event: killed, they are gone (no respawn), and `Defeat` waits for them. `name` names them (a boss).
    Spawn {
        archetype: String,
        #[serde(default = "d_spawn_count")]
        count: u32,
        #[serde(default)]
        place: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,
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
    /// Switch a weather zone on or off.
    Zone { zone: String, on: bool },
    /// Pause before the next step.
    Wait { secs: u32 },
    /// Wait until every bot this run has spawned is beaten. After `within_secs` (0: no limit but the event's own) the
    /// event has failed, and its `fail_steps` run instead.
    Defeat {
        #[serde(default)]
        within_secs: u32,
    },
    /// Put items in the bags of the players who took part (were at the event's place while it ran; everyone
    /// online, for an event with no place), or with `everyone` every player online. What does not fit lands at
    /// their feet.
    Reward {
        item: String,
        #[serde(default = "d_one_count")]
        count: u32,
        #[serde(default, skip_serializing_if = "is_false")]
        everyone: bool,
    },
    /// The townsfolk of the event's place come out to its square for `secs` (a market, a feast, a wedding), and
    /// now and then say a line of `occasion` (`GameFile::talk`) and dance. With `announce`, those nearby are told
    /// so, `{who}` and `{other}` filled with two of the townsfolk there (the couple) and `{place}` with the town.
    Gather {
        #[serde(default = "d_gather_secs")]
        secs: u32,
        #[serde(default = "d_gather_occasion")]
        occasion: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        announce: Option<String>,
        #[serde(default, skip_serializing_if = "is_false")]
        dance: bool,
    },
    /// The sickness comes to the event's town for `secs`: `first` of its folk fall ill, and it spreads to those who
    /// stand close (`spread_pct` a minute for each one beside the sick). Each case runs its course and ends in
    /// recovery or, `deaths_pct` of the time for townsfolk, death; a player it weakens but does not kill. A healing
    /// draught used beside the sick cures them, and the town's shops shut until it has passed.
    Plague {
        #[serde(default = "d_plague_secs")]
        secs: u32,
        #[serde(default = "d_plague_spread")]
        spread_pct: f32,
        #[serde(default = "d_plague_deaths")]
        deaths_pct: f32,
        #[serde(default = "d_plague_first")]
        first: u8,
    },
}
fn d_plague_secs() -> u32 {
    1500
}
fn d_plague_spread() -> f32 {
    12.0
}
fn d_plague_deaths() -> f32 {
    20.0
}
fn d_plague_first() -> u8 {
    3
}
fn d_gather_secs() -> u32 {
    600
}
fn d_gather_occasion() -> String {
    "market".into()
}
fn d_one_count() -> u32 {
    1
}

/// What starts an event on its own, besides its timer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum When {
    /// At least `count` players at the event's place (anywhere, for an event with no place).
    Players {
        #[serde(default = "d_one_count")]
        count: u32,
    },
    /// The day reaches this part (`daylight`): `dawn` sunrise, `day` midday, `dusk` sunset, `night` midnight.
    Time { at: DayPart },
    /// The year turns to this season (`Rules::season_days`).
    Season { is: Season },
    /// Another event has just finished.
    After {
        event: String,
        #[serde(default)]
        outcome: Outcome,
    },
    /// A set time by the calendar (UTC): `hour`:`minute` every day, or only on `weekday` (0 Monday .. 6 Sunday).
    Calendar {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weekday: Option<u8>,
        hour: u8,
        #[serde(default)]
        minute: u8,
    },
}

impl When {
    /// Whether a calendar time is the minute `unix_min` (minutes since 1970, UTC). 1 January 1970 was a Thursday.
    pub fn calendar_at(weekday: Option<u8>, hour: u8, minute: u8, unix_min: u64) -> bool {
        let day_min = unix_min % 1440;
        let wd = ((unix_min / 1440) + 3) % 7; // 0 Monday
        day_min == hour as u64 * 60 + minute as u64 && weekday.is_none_or(|w| w as u64 == wd)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DayPart {
    Dawn,
    Day,
    Dusk,
    Night,
}

impl DayPart {
    /// Which part of the day `phase` (0..1 from sunrise) is in, for `Cond::During`: the first and last 15% of the
    /// daylight are dawn and dusk, dusk runs on into the first 15% of the night, and the rest of the dark is night.
    pub fn during(phase: f64, night_share: f64) -> DayPart {
        let light = 1.0 - night_share;
        let p = phase.rem_euclid(1.0);
        if p < light * 0.15 {
            DayPart::Dawn
        } else if p < light * 0.85 {
            DayPart::Day
        } else if p < light + night_share * 0.15 {
            DayPart::Dusk
        } else {
            DayPart::Night
        }
    }

    /// Where in the day (`daylight::Daylight::phase`) this part starts, given the night's share of it.
    pub fn phase(self, night_share: f64) -> f64 {
        match self {
            DayPart::Dawn => 0.0,
            DayPart::Day => (1.0 - night_share) / 2.0,
            DayPart::Dusk => 1.0 - night_share,
            DayPart::Night => 1.0 - night_share / 2.0,
        }
    }
}

/// How an event ended: it ran to its last step, or a time limit ran out first (or the console stopped it).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    #[default]
    Success,
    Failure,
    Either,
}

/// Something that happens: fired by hand, on a timer, or when something in the world starts it (`when`). It runs
/// its steps over time (they can wait), so one event can be a whole raid: a warning, waves, a boss, a reward.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EventDef {
    pub name: String,
    /// Run itself every this many seconds (at least 5). 0 means only when fired by hand (or `when`).
    #[serde(default)]
    pub every_secs: u32,
    /// Each time it comes due, the chance in percent that it really happens.
    #[serde(default = "d_chance")]
    pub chance_pct: f32,
    /// Start when this happens, too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when: Option<When>,
    /// Least seconds from one start by `when` to the next.
    #[serde(default = "d_cooldown")]
    pub cooldown_secs: u32,
    /// Where it happens, for a local event: steps that name no place happen here, `nearby` messages reach the
    /// players here, and they are the ones who take part (and are rewarded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    /// Longest it may run before it has failed (0: no limit).
    #[serde(default)]
    pub limit_secs: u32,
    /// It happens in a town (a place with houses) picked each time it starts, one with players at it before one
    /// without, rather than at `place`: a market day, a feast, a wedding that comes round the towns. Its steps'
    /// messages may say `{place}`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub roams: bool,
    #[serde(default)]
    pub steps: Vec<Step>,
    /// What happens instead if it fails: a `Defeat` or the time limit ran out.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fail_steps: Vec<Step>,
}
fn d_cooldown() -> u32 {
    600
}

impl Default for EventDef {
    fn default() -> Self {
        EventDef { name: String::new(), every_secs: 0, chance_pct: 100.0, when: None, cooldown_secs: d_cooldown(), place: None, limit_secs: 0, roams: false, steps: Vec::new(), fail_steps: Vec::new() }
    }
}

/// Server-side runtime form of an archetype: names resolved to indices.
#[derive(Clone, Debug)]
pub struct ArchRt {
    pub name: String,
    /// `Archetype::title`, empty when it has none (`shown`).
    pub title: String,
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
    pub xp: u32,
    pub level: u8,
    /// (item index, price)
    pub shop: Vec<(usize, u16)>,
    pub lasts_secs: u32,
    pub respawn_secs: f32,
    pub respawn_at: RespawnAt,
    /// How far a player it is fighting may get before it gives up (`Archetype::leash`, the default filled in).
    pub leash: f32,
    /// Its faction's index in `Game::factions`.
    pub faction: Option<usize>,
    /// Its loot table (`Realm::loot`).
    pub loot: Option<usize>,
    /// `Archetype::names` and `surnames`.
    pub names: Vec<String>,
    pub surnames: Vec<String>,
    /// Every place it lives in: `home` first, then `Archetype::homes` (empty without a home).
    pub homes: Vec<usize>,
    pub ambush: bool,
    pub work: Option<Work>,
    pub thief: bool,
    pub pet: bool,
    pub stray: bool,
    pub healer: bool,
    pub boss: Option<BossRt>,
}

impl ArchRt {
    /// What players call it.
    pub fn shown(&self) -> String {
        if self.title.is_empty() { crate::names::bot(&self.name) } else { self.title.clone() }
    }
}

/// What players call the bot type named `name` among `archs` (its name capitalised if there is none).
pub fn shown_kind(archs: &[ArchRt], name: &str) -> String {
    archs.iter().find(|a| a.name == name).map_or_else(|| crate::names::bot(name), ArchRt::shown)
}

#[derive(Clone, Debug)]
pub enum CondRt {
    Always,
    Sees { who: Seek, within: f32 },
    AttrBelow { attr: usize, value: f32 },
    AttrAbove { attr: usize, value: f32 },
    InPlace { place: usize },
    Chance { pct: f32 },
    SeesKind { kind: u8, within: f32 },
    /// A bit per `DayPart` (dawn 1, day 2, dusk 4, night 8).
    During { parts: u8 },
    Weather { names: Vec<String> },
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
    Indoors,
    Chat { lines: Vec<String> },
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
    pub daylight: crate::daylight::Daylight,
    pub pvp: bool,
    pub levels: Option<LevelsFile>,
    pub quests: Vec<QuestFile>,
    /// The money item's index.
    pub currency: Option<usize>,
    pub classes: Vec<ClassRt>,
    pub building: BuildLimits,
    pub factions: Vec<FactionFile>,
    /// Loot tables, recipes, crops, plots and dungeons, resolved (`realm.rs`).
    pub realm: crate::realm::Realm,
    pub rules: crate::rules::Rules,
    /// The starter kit, as (item index, count).
    pub starter: Vec<(usize, u8)>,
    /// `GameFile::start_place`, as a place index.
    pub start_place: Option<usize>,
    pub emotes: Vec<Emote>,
    pub gods: Vec<String>,
    pub talk: crate::talk::Talk,
}

/// Most walls a game file may place.
pub const MAX_WALLS: usize = 2000;

/// A usable item with the given effects (none makes it a keepsake or trade good).
fn item(name: &str, color: &str, stack_max: u8, uses: &[(&str, f32)]) -> ItemDef {
    ItemDef { name: name.into(), color: color.into(), stack_max, uses: uses.iter().map(|&(a, d)| UseEffect { attr: a.into(), delta: d }).collect(), consume: true, value: 0, equip: None, mount: 0.0, rarity: Default::default(), title: String::new(), block: None }
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
    pub fn world(&self, size: u32, seed: u64) -> Result<crate::world::World, String> {
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
            title: String::new(),
            names: vec![],
            surnames: vec![],
            homes: vec![],
            ambush: false,
            work: None,
            thief: false,
            pet: false,
            stray: false,
            healer: false,
            boss: None,
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
            xp: 0,
            level: 0,
            shop: Vec::new(),
            lasts_secs: 0,
            respawn_secs: 0.0,
            respawn_at: RespawnAt::Anywhere,
            leash: 0.0,
            faction: None,
            loot: None,
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
                Place { name: "plaza".into(), x: 200.0, y: 200.0, r: 20.0, title: "The Plaza".into(), exit: None, sound: String::new(), music: String::new(), safe: false },
                Place { name: "market".into(), x: 420.0, y: 160.0, r: 16.0, title: "Market Square".into(), exit: None, sound: String::new(), music: String::new(), safe: false },
                Place { name: "watchtower".into(), x: 330.0, y: 430.0, r: 10.0, title: "The Watchtower".into(), exit: None, sound: String::new(), music: String::new(), safe: false },
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
            zones: Vec::new(),
            daylight: None,
            levels: None,
            quests: Vec::new(),
            currency: None,
            classes: Vec::new(),
            building: BuildLimits::default(),
            factions: Vec::new(),
            loot: Vec::new(),
            recipes: Vec::new(),
            crops: Vec::new(),
            plots: None,
            dungeons: Vec::new(),
            rules: Default::default(),
            starter: Vec::new(),
            start_place: None,
            emotes: Vec::new(),
            gods: Vec::new(),
            talk: Default::default(),
            events: Vec::new(),
            move_speed: 20.0,
            road_speed: crate::world::ROAD_SPEED,
            pvp: true,
            entity_radius: 0.5,
            interest_radius: 128.0,
            max_visible: 64,
            attrs: vec![
                attr("health", true, 0.0, OnZeroFile::Respawn),
                attr("energy", false, 5.0, OnZeroFile::None),
                AttrFile { default: 0.0, ..attr(AIRBORNE_ATTR, true, 0.0, OnZeroFile::None) },
                AttrFile { min: 0.0, max: 1.0, default: 1.0, ..attr(LIT_ATTR, true, 0.0, OnZeroFile::None) },
            ],
            actions: vec![ActionFile {
                id: 1,
                name: "zap".into(),
                cooldown_ticks: 10,
                range: 8.0,
                requires: Some(Requires { attr: "energy".into(), min: 5.0 }),
                effects: vec![
                    EffectFile { to: Who::Target, attr: "health".into(), delta: -10.0, secs: 0.0, lasts: 0.0 },
                    EffectFile { to: Who::Actor, attr: "energy".into(), delta: -5.0, secs: 0.0, lasts: 0.0 },
                ],
                ..Default::default()
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

    /// A bot type's `boss` section checked, with its names resolved.
    fn boss_rt(&self, b: &BossFile, defs: &Defs, ctx: &str) -> Result<BossRt, String> {
        if b.phases.is_empty() || b.phases.len() > MAX_BOSS_PHASES {
            return Err(format!("{ctx}: a boss has 1..{MAX_BOSS_PHASES} phases"));
        }
        if !(b.enrage_secs.is_finite() && (0.0..=3600.0).contains(&b.enrage_secs)) || !(b.enrage_might.is_finite() && (1.0..=20.0).contains(&b.enrage_might)) {
            return Err(format!("{ctx}: enrage_secs is 0..3600 and enrage_might 1..20"));
        }
        let line = |t: &str| t.chars().count() <= crate::chat::MAX_TEXT && !t.chars().any(char::is_control);
        if !line(&b.defeat) {
            return Err(format!("{ctx}: defeat is one line of at most {} characters", crate::chat::MAX_TEXT));
        }
        let mut phases = Vec::new();
        let mut last = f32::INFINITY;
        for (i, p) in b.phases.iter().enumerate() {
            let pc = format!("{ctx}: phase {}", i + 1);
            if !(p.below.is_finite() && p.below > 0.0 && p.below <= 100.0 && p.below < last) {
                return Err(format!("{pc}: below is a health percent above 0 and at most 100, lower than the phase before"));
            }
            last = p.below;
            if !line(&p.say) {
                return Err(format!("{pc}: say is one line of at most {} characters", crate::chat::MAX_TEXT));
            }
            if p.moves.len() > MAX_BOSS_MOVES {
                return Err(format!("{pc}: at most {MAX_BOSS_MOVES} moves"));
            }
            let mut moves = Vec::new();
            for m in &p.moves {
                let action = defs.actions.iter().find(|d| d.name == m.action).map(|d| d.id).ok_or_else(|| format!("{pc}: unknown action '{}'", m.action))?;
                if !(m.every_secs.is_finite() && (0.5..=600.0).contains(&m.every_secs)) {
                    return Err(format!("{pc}: '{}' every_secs is 0.5..600", m.action));
                }
                if !(m.warn_secs.is_finite() && (0.0..=MAX_WARN_SECS).contains(&m.warn_secs)) {
                    return Err(format!("{pc}: '{}' warn_secs is 0..{MAX_WARN_SECS}", m.action));
                }
                if !(m.radius.is_finite() && (0.0..=MAX_AREA).contains(&m.radius)) {
                    return Err(format!("{pc}: '{}' radius is 0..{MAX_AREA}", m.action));
                }
                if !line(&m.say) {
                    return Err(format!("{pc}: '{}' say is one line of at most {} characters", m.action, crate::chat::MAX_TEXT));
                }
                moves.push(BossMoveRt { action, every_secs: m.every_secs, warn_secs: m.warn_secs, at: m.at, radius: m.radius, say: m.say.clone() });
            }
            let mut adds = Vec::new();
            for a in &p.adds {
                let k = self.archetypes.iter().position(|x| x.name == a.bot).ok_or_else(|| format!("{pc}: there is no bot type '{}' to add", a.bot))?;
                if self.archetypes[k].boss.is_some() {
                    return Err(format!("{pc}: an add cannot be a boss ('{}')", a.bot));
                }
                if a.count == 0 || a.count > MAX_BOSS_ADDS {
                    return Err(format!("{pc}: '{}' count is 1..{MAX_BOSS_ADDS}", a.bot));
                }
                adds.push((k, a.count));
            }
            phases.push(BossPhaseRt { below: p.below / 100.0, say: p.say.clone(), moves, adds });
        }
        Ok(BossRt { phases, enrage_secs: b.enrage_secs, enrage_might: b.enrage_might, defeat: b.defeat.clone() })
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
        self.rules.check()?;
        if !(self.move_speed > 0.0 && self.move_speed <= 1000.0) {
            return Err("move_speed must be in (0, 1000]".into());
        }
        if !(self.road_speed >= 1.0 && self.road_speed <= 4.0) {
            return Err("road_speed must be 1..4 (1 = roads are no faster)".into());
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
        let worst = HEADER_LEN + self.max_visible as usize * (crate::snapshot::ENTRY_LEN + wire);
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
                if !(e.secs.is_finite() && (0.0..=MAX_OVER_SECS).contains(&e.secs)) {
                    return Err(format!("{ctx}: an effect's secs must be 0..{MAX_OVER_SECS}"));
                }
                if !(e.lasts.is_finite() && (0.0..=MAX_LAST_SECS).contains(&e.lasts)) {
                    return Err(format!("{ctx}: an effect's lasts must be 0..{MAX_LAST_SECS}"));
                }
                if e.lasts > 0.0 && e.secs > 0.0 {
                    return Err(format!("{ctx}: an effect is spread over time (secs) or taken back later (lasts), not both"));
                }
                let attr = idx(&e.attr, &ctx)?;
                effects.push(match e.to {
                    Who::Target => Effect::AddToTarget { attr, delta: e.delta, secs: e.secs, lasts: e.lasts },
                    Who::Actor => Effect::AddToSelf { attr, delta: e.delta, secs: e.secs, lasts: e.lasts },
                });
            }
            if !(a.area.is_finite() && (0.0..=MAX_AREA).contains(&a.area)) {
                return Err(format!("{ctx}: area must be 0..{MAX_AREA}"));
            }
            if let Some(h) = a.hold {
                if !(h.secs.is_finite() && h.secs > 0.0 && h.secs <= MAX_HOLD_SECS) {
                    return Err(format!("{ctx}: hold secs must be above 0 and at most {MAX_HOLD_SECS}"));
                }
            }
            if !a.fx.is_empty() && !FX.contains(&a.fx.as_str()) {
                return Err(format!("{ctx}: fx must be one of {}", FX.join(", ")));
            }
            if a.school.chars().count() > 24 || a.school.chars().any(char::is_control) {
                return Err(format!("{ctx}: school is at most 24 characters"));
            }
            if a.desc.chars().count() > 200 || a.desc.chars().any(char::is_control) {
                return Err(format!("{ctx}: desc is at most 200 characters, on one line"));
            }
            if !(a.lift.is_finite() && (0.0..=MAX_LAST_SECS).contains(&a.lift)) {
                return Err(format!("{ctx}: lift must be 0..{MAX_LAST_SECS} seconds"));
            }
            actions.push(ActionDef {
                id: a.id,
                name: a.name.clone(),
                cooldown_ticks: a.cooldown_ticks,
                range: a.range,
                requires,
                effects,
                aim: a.aim,
                area: a.area,
                hits: a.hits,
                hold: a.hold.map(|h| (h.kind, h.secs)),
                fx: a.fx.clone(),
                bolt: a.bolt,
                school: a.school.clone(),
                desc: a.desc.clone(),
                lift: a.lift,
                level: a.level,
                taunt: a.taunt,
            });
        }

        let defs = Defs {
            attrs,
            actions,
            replicated,
            move_speed: self.move_speed,
            road_speed: self.road_speed,
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
        // Anywhere a world can reach (the game file does not know this world's size; a world file can set it).
        let coord = |v: f32| v.is_finite() && (0.0..=crate::world::MAX_WORLD_SIZE as f32).contains(&v);
        let mut pnames = HashSet::new();
        for p in &self.places {
            if p.name.is_empty() || p.name.len() > 32 || !pnames.insert(p.name.as_str()) {
                return Err(format!("place names must be unique, 1..32 characters ('{}')", p.name));
            }
            if !(coord(p.x) && coord(p.y) && p.r.is_finite() && (1.0..=MAX_PLACE_R).contains(&p.r)) {
                return Err(format!("place '{}': x and y must be 0..65536 and r 1..{MAX_PLACE_R}", p.name));
            }
            if let Some(e) = &p.exit {
                if !(coord(e.x) && coord(e.y)) || e.zone.len() > 32 {
                    return Err(format!("place '{}': an exit goes to x and y 0..65536, in a zone named in at most 32 characters", p.name));
                }
                if e.zone.is_empty() && (e.x - p.x).hypot(e.y - p.y) <= p.r {
                    return Err(format!("place '{}': an exit on this map must lead somewhere outside the place itself", p.name));
                }
            }
            if !p.sound.is_empty() && !SOUNDS.contains(&p.sound.as_str()) {
                return Err(format!("place '{}': sound is one of {}", p.name, SOUNDS.join(", ")));
            }
            if !p.music.is_empty() && !MUSIC.contains(&p.music.as_str()) {
                return Err(format!("place '{}': music is one of {}", p.name, MUSIC.join(", ")));
            }
            if p.title.chars().count() > MAX_TITLE || p.title.trim() != p.title || p.title.chars().any(char::is_control) {
                return Err(format!("place '{}': a title is at most {MAX_TITLE} characters, no leading or trailing spaces", p.name));
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
                return Err(format!("route '{}': points must be 0..65536", r.name));
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
            if it.title.len() > 32 || it.title.trim() != it.title {
                return Err(format!("{ctx}: a title is at most 32 characters, no leading or trailing spaces"));
            }
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
            if let Some(e) = &it.equip {
                if !(e.might.is_finite() && e.toughness.is_finite() && (0.0..=2.0).contains(&e.might) && (0.0..=2.0).contains(&e.toughness)) {
                    return Err(format!("item '{}': equip might and toughness are 0..2", it.name));
                }
            }
            if !(it.mount.is_finite() && (0.0..=4.0).contains(&it.mount)) {
                return Err(format!("item '{}': mount is 0..4", it.name));
            }
            let block = match &it.block {
                None => None,
                Some(m) => {
                    let k = material(m).ok_or_else(|| format!("{ctx}: block '{m}' is not a material ({})", BLOCK_NAMES[..SPROUT as usize].join(", ")))?;
                    if items.iter().any(|x| x.block == Some(k)) {
                        return Err(format!("{ctx}: another item is already a block of {}", BLOCK_NAMES[k as usize]));
                    }
                    Some(k)
                }
            };
            items.push(ItemRt {
                name: it.name.clone(),
                color: it.color.clone(),
                stack_max: it.stack_max,
                uses,
                consume: it.consume,
                value: it.value,
                equip: it.equip.clone(),
                mount: it.mount,
                rarity: it.rarity,
                block,
                title: it.title.clone(),
            });
        }
        let mut seen = HashSet::new();
        let mut archs = Vec::new();
        for a in &self.archetypes {
            let ctx = format!("archetype '{}'", a.name);
            if a.name.is_empty() || a.name.len() > 32 || !seen.insert(a.name.clone()) {
                return Err(format!("archetype names must be unique, 1..32 characters ('{}')", a.name));
            }
            if a.title.len() > 32 || a.title.trim() != a.title || a.title.contains('|') {
                return Err(format!("{ctx}: a title is at most 32 characters, no leading or trailing spaces, no |"));
            }
            if a.names.len() > 200 || a.surnames.len() > 200 {
                return Err(format!("{ctx}: at most 200 names and 200 surnames"));
            }
            if !a.surnames.is_empty() && a.names.is_empty() {
                return Err(format!("{ctx}: surnames need names to go with"));
            }
            for n in a.names.iter().chain(&a.surnames) {
                if n.trim().is_empty() || n.len() > 20 || n.trim() != n || n.contains('|') || n.chars().any(char::is_control) {
                    return Err(format!("{ctx}: a name is 1..20 characters, no leading or trailing spaces, no | ('{n}')"));
                }
            }
            if !(a.respawn_secs.is_finite() && (0.0..=86_400.0).contains(&a.respawn_secs)) {
                return Err(format!("{ctx}: respawn_secs is 0..86400"));
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
            if a.homes.len() > MAX_PLACES {
                return Err(format!("{ctx}: at most {MAX_PLACES} homes"));
            }
            if !a.homes.is_empty() && home.is_none() {
                return Err(format!("{ctx}: more homes need a home first"));
            }
            let mut homes: Vec<usize> = home.into_iter().collect();
            for n in &a.homes {
                homes.push(place_ix(n, &ctx)?);
            }
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
                    Cond::SeesKind { bot: kind, within } => {
                        if !(within.is_finite() && *within > 0.0 && *within <= self.interest_radius) {
                            return Err(format!("{rc}: 'within' must be 1..interest_radius"));
                        }
                        let k = self.archetypes.iter().position(|x| x.name == *kind).ok_or_else(|| format!("{rc}: no bot type '{kind}'"))?;
                        reach = reach.max(*within);
                        CondRt::SeesKind { kind: k as u8, within: *within }
                    }
                    Cond::During { parts } => {
                        if parts.is_empty() {
                            return Err(format!("{rc}: 'during' needs at least one part of the day"));
                        }
                        CondRt::During { parts: parts.iter().fold(0u8, |m, p| m | (1 << *p as u8)) }
                    }
                    Cond::Weather { names } => {
                        for n in names {
                            if !self.weather.iter().any(|w| w.name == *n) {
                                return Err(format!("{rc}: no weather '{n}'"));
                            }
                        }
                        CondRt::Weather { names: names.clone() }
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
                    Act::Indoors => {
                        if a.home.is_none() {
                            return Err(format!("{rc}: 'indoors' needs the bot type to have a home"));
                        }
                        ActRt::Indoors
                    }
                    Act::Chat { lines } => {
                        if lines.is_empty() || lines.len() > 16 {
                            return Err(format!("{rc}: chat needs 1 to 16 lines"));
                        }
                        let clean = lines.iter().map(|l| crate::chat::sanitize(l).ok_or_else(|| format!("{rc}: an empty chat line"))).collect::<Result<Vec<_>, _>>()?;
                        ActRt::Chat { lines: clean }
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
            if !(a.leash.is_finite() && (0.0..=10_000.0).contains(&a.leash)) {
                return Err(format!("{ctx}: leash must be 0..10000"));
            }
            let leash = if a.leash > 0.0 { a.leash } else { (a.sight * 3.0).max(MIN_LEASH) };
            let faction = match &a.faction {
                Some(f) => Some(self.factions.iter().position(|x| &x.name == f).ok_or_else(|| format!("{ctx}: there is no faction '{f}'"))?),
                None => None,
            };
            let boss = match &a.boss {
                Some(b) => Some(self.boss_rt(b, &defs, &ctx)?),
                None => None,
            };
            archs.push(ArchRt {
                boss,
                leash,
                faction,
                loot: a.loot.as_ref().and_then(|l| self.loot.iter().position(|t| &t.name == l)),
                drops,
                home,
                rules,
                reach,
                name: a.name.clone(),
                title: a.title.clone(),
                names: a.names.clone(),
                surnames: a.surnames.clone(),
                homes,
                ambush: a.ambush,
                work: a.work,
                thief: a.thief,
                pet: a.pet,
                stray: a.stray,
                healer: a.healer,
                color: a.color.clone(),
                count: a.count,
                behaviour: a.behaviour,
                speed_mult: a.speed_mult,
                sight: a.sight,
                attack,
                start,
                xp: a.xp,
                level: a.level.clamp(1, MAX_LEVEL),
                lasts_secs: a.lasts_secs,
                respawn_secs: a.respawn_secs,
                respawn_at: a.respawn_at,
                shop: a
                    .shop
                    .iter()
                    .map(|s| self.items.iter().position(|i| i.name == s.item).map(|k| (k, s.price)).ok_or_else(|| format!("bot type '{}': sells '{}', which is not an item", a.name, s.item)))
                    .collect::<Result<Vec<_>, String>>()?,
            });
        }
        self.check_director()?;
        self.check_quests()?;
        if self.classes.len() > MAX_CLASSES {
            return Err(format!("at most {MAX_CLASSES} classes"));
        }
        let mut cnames = HashSet::new();
        let mut classes = Vec::new();
        for c in &self.classes {
            if c.name.trim().is_empty() || c.name.len() > 32 || !cnames.insert(c.name.as_str()) || c.title.trim().is_empty() || c.title.chars().count() > 32 {
                return Err(format!("class '{}': a unique name and a title of at most 32", c.name));
            }
            if c.desc.chars().count() > 300 || !(c.might.is_finite() && c.toughness.is_finite() && (0.0..=2.0).contains(&c.might) && (0.0..=2.0).contains(&c.toughness)) {
                return Err(format!("class '{}': desc at most 300 characters, might and toughness 0..2", c.name));
            }
            let mut spells = c
                .spells
                .iter()
                .map(|n| self.actions.iter().find(|a| &a.name == n).map(|a| a.id).ok_or_else(|| format!("class '{}': '{n}' is not an action", c.name)))
                .collect::<Result<Vec<u8>, String>>()?;
            if c.schools.len() > 8 || c.schools.iter().any(|s| s.trim().is_empty() || s.chars().count() > 32) {
                return Err(format!("class '{}': at most 8 schools, each 1..32 characters", c.name));
            }
            if !c.schools.is_empty() {
                for a in &self.actions {
                    let school = a.school.as_str();
                    let ok = school.is_empty() || c.schools.iter().any(|s| s.eq_ignore_ascii_case(school));
                    if ok && !spells.contains(&a.id) {
                        spells.push(a.id);
                    }
                }
            }
            classes.push(ClassRt { name: c.name.clone(), title: c.title.clone(), desc: c.desc.clone(), look: c.look.clone(), might: c.might, toughness: c.toughness, spells, any: c.spells.is_empty() && c.schools.is_empty() });
        }
        self.check_factions()?;
        let currency = match &self.currency {
            Some(c) => Some(self.items.iter().position(|i| &i.name == c).ok_or_else(|| format!("currency '{c}' is not an item"))?),
            None => None,
        };
        let daylight = self.daylight.clone().unwrap_or_default();
        daylight.validate()?;
        if let Some(l) = &self.levels {
            if l.cap < 1 || l.cap > MAX_LEVEL {
                return Err(format!("levels.cap must be 1..{MAX_LEVEL}"));
            }
            for (name, &per) in &l.grow {
                if !self.attrs.iter().any(|a| &a.name == name) || !(per.is_finite() && (0.0..=10_000.0).contains(&per)) {
                    return Err(format!("levels.grow: '{name}' must be an attribute, growing by 0..10000 a level"));
                }
            }
            for (v, what, lo, hi) in [
                (l.base, "base", 1.0, 1e7),
                (l.exponent, "exponent", 0.5, 4.0),
                (l.might, "might", 0.0, 1.0),
                (l.toughness, "toughness", 0.0, 1.0),
                (l.life.walk, "life.walk", 0.0, 1000.0),
                (l.life.act, "life.act", 0.0, 1000.0),
                (l.life.build, "life.build", 0.0, 1000.0),
                (l.life.deed, "life.deed", 0.0, 1000.0),
                (l.life.plant, "life.plant", 0.0, 1000.0),
                (l.life.harvest, "life.harvest", 0.0, 1000.0),
                (l.life.minute, "life.minute", 0.0, 1000.0),
                (l.life.company, "life.company", 0.0, 1000.0),
            ] {
                if !(v.is_finite() && (lo..=hi).contains(&v)) {
                    return Err(format!("levels.{what} must be {lo}..{hi}"));
                }
            }
            if l.life.trail > 1024 || l.life.repeat > 64 {
                return Err("levels.life: trail at most 1024 cells, repeat at most 64 deeds".into());
            }
            if l.rewards.len() > MAX_LEVEL as usize {
                return Err(format!("levels.rewards: at most {MAX_LEVEL}"));
            }
            for r in &l.rewards {
                if r.level < 2 || r.level > l.cap || r.items.len() > INV_SLOTS || r.text.len() > 200 {
                    return Err(format!("levels.rewards: level {} must be 2..cap, with at most {INV_SLOTS} stacks and a short line", r.level));
                }
                for s in &r.items {
                    if s.count == 0 || !self.items.iter().any(|i| i.name == s.item) {
                        return Err(format!("levels.rewards: level {}: '{}' is not an item (or its count is 0)", r.level, s.item));
                    }
                }
            }
        }
        let realm = self.realm()?;
        if self.starter.len() > INV_SLOTS {
            return Err(format!("starter: at most {INV_SLOTS} stacks"));
        }
        let starter = self
            .starter
            .iter()
            .map(|s| match self.items.iter().position(|i| i.name == s.item) {
                Some(k) if s.count >= 1 => Ok((k, s.count)),
                Some(_) => Err(format!("starter: count of '{}' must be at least 1", s.item)),
                None => Err(format!("starter: '{}' is not an item", s.item)),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if self.emotes.len() > 200 {
            return Err("emotes: at most 200".into());
        }
        let mut cmds = std::collections::HashSet::new();
        for e in &self.emotes {
            if e.cmd.is_empty() || e.cmd.len() > 16 || !e.cmd.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                return Err(format!("emotes: '{}' must be 1..16 lowercase letters or digits", e.cmd));
            }
            if !cmds.insert(e.cmd.as_str()) {
                return Err(format!("emotes: '/{}' twice", e.cmd));
            }
            if e.text.trim().is_empty() || e.text.len() > 100 || e.at.len() > 100 || e.anim.len() > 32 || e.anim.contains('|') {
                return Err(format!("emotes: '/{}' needs a text (at most 100), an at line of at most 100 and a short anim", e.cmd));
            }
        }
        if self.gods.len() > 50 || self.gods.iter().any(|t| t.len() != 16 || !t.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))) {
            return Err("gods: at most 50 account tags, each 16 lowercase hex digits".into());
        }
        let talk = crate::talk::Talk::build(&self.talk)?;
        let start_place = match &self.start_place {
            Some(n) => Some(self.places.iter().position(|p| &p.name == n).ok_or_else(|| format!("start_place: there is no place '{n}'"))?),
            None => None,
        };
        Ok(Game { defs, archs, items, places: self.places.clone(), routes: self.routes.clone(), daylight, pvp: self.pvp, levels: self.levels.clone(), quests: self.quests.clone(), currency, classes, building: self.building, factions: self.factions.clone(), realm, rules: self.rules, starter, start_place, emotes: self.emotes.clone(), gods: self.gods.clone(), talk })
    }

    /// Factions: names unique, numbers in range, and what they hate there.
    fn check_factions(&self) -> Result<(), String> {
        if self.factions.len() > MAX_FACTIONS {
            return Err(format!("at most {MAX_FACTIONS} factions"));
        }
        let mut seen = std::collections::HashSet::new();
        for f in &self.factions {
            let ctx = format!("faction '{}'", f.name);
            if f.name.trim().is_empty() || f.name.len() > 48 || !seen.insert(f.name.as_str()) {
                return Err(format!("{ctx}: faction names must be unique, non-empty and at most 48 bytes"));
            }
            if f.title.chars().count() > MAX_TITLE {
                return Err(format!("{ctx}: title at most {MAX_TITLE} characters"));
            }
            if !f.color.is_empty() {
                crate::realm::color_ok(&f.color, &ctx)?;
            }
            for (v, what) in [(f.kill, "kill"), (f.hostile, "hostile"), (f.bounty, "bounty")] {
                if !(-STANDING_MAX..=STANDING_MAX).contains(&v) {
                    return Err(format!("{ctx}: {what} must be -{STANDING_MAX}..{STANDING_MAX}"));
                }
            }
            if let Some(h) = f.hates.iter().find(|h| *h == &f.name || !self.factions.iter().any(|o| &o.name == *h)) {
                return Err(format!("{ctx}: hates '{h}', which is not another faction"));
            }
        }
        Ok(())
    }

    pub fn from_json(s: &str) -> Result<GameFile, String> {
        serde_json::from_str(s).map_err(|e| format!("json: {e}"))
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("serialise")
    }
}

impl GameFile {
    /// One event step's references and numbers.
    fn check_step(&self, st: &Step, ctx: &str, has_place: bool) -> Result<(), String> {
        let attr = |n: &str| self.attrs.iter().any(|a| a.name == n);
        let arch = |n: &str| self.archetypes.iter().any(|a| a.name == n);
        let place = |n: &str| self.places.iter().any(|p| p.name == n);
        match st {
            Step::Announce { text, nearby, .. } => {
                if text.trim().is_empty() || text.len() > 120 {
                    return Err(format!("{ctx}: the message must be 1..120 bytes"));
                }
                if *nearby && !has_place {
                    return Err(format!("{ctx}: a message to those nearby needs the event to have a place"));
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
            Step::Spawn { archetype, count, place: pl, name } => {
                if let Some(n) = name {
                    if n.trim().is_empty() || n.chars().count() > 32 {
                        return Err(format!("{ctx}: a name is 1..32 characters"));
                    }
                }
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
            Step::Zone { zone, .. } => {
                if !self.zones.iter().any(|z| &z.name == zone) {
                    return Err(format!("{ctx}: unknown weather zone '{zone}'"));
                }
            }
            Step::Wait { secs } => {
                if *secs == 0 || *secs > MAX_EVENT_SECS {
                    return Err(format!("{ctx}: a wait is 1..{MAX_EVENT_SECS} seconds"));
                }
            }
            Step::Defeat { within_secs } => {
                if *within_secs > MAX_EVENT_SECS {
                    return Err(format!("{ctx}: at most {MAX_EVENT_SECS} seconds"));
                }
            }
            Step::Reward { item, count, .. } => {
                if !self.items.iter().any(|i| &i.name == item) {
                    return Err(format!("{ctx}: unknown item '{item}'"));
                }
                if *count == 0 || *count > 255 {
                    return Err(format!("{ctx}: count must be 1..255"));
                }
            }
            Step::Plague { secs, spread_pct, deaths_pct, first } => {
                if !has_place {
                    return Err(format!("{ctx}: a plague needs the event to have a place, or to roam"));
                }
                if *secs == 0 || *secs > MAX_EVENT_SECS {
                    return Err(format!("{ctx}: a plague lasts 1..{MAX_EVENT_SECS} seconds"));
                }
                if !(0.0..=100.0).contains(spread_pct) || !(0.0..=100.0).contains(deaths_pct) {
                    return Err(format!("{ctx}: spread_pct and deaths_pct are 0..100"));
                }
                if *first == 0 || *first > 50 {
                    return Err(format!("{ctx}: first is 1..50"));
                }
            }
            Step::Gather { secs, occasion, announce, .. } => {
                if !has_place {
                    return Err(format!("{ctx}: a gathering needs the event to have a place"));
                }
                if *secs == 0 || *secs > MAX_EVENT_SECS {
                    return Err(format!("{ctx}: a gathering is 1..{MAX_EVENT_SECS} seconds"));
                }
                if !crate::talk::DEFAULTS.iter().any(|d| d.0 == occasion) {
                    return Err(format!("{ctx}: no talk occasion '{occasion}'"));
                }
                if announce.as_ref().is_some_and(|t| t.trim().is_empty() || t.len() > 120) {
                    return Err(format!("{ctx}: the message must be 1..120 bytes"));
                }
            }
        }
        Ok(())
    }

    /// Weather and events refer to attributes, bot types and places by name; make sure every reference lands.
    /// Quests: names unique, and everything they name (bot types, places, items, earlier quests) there.
    fn check_quests(&self) -> Result<(), String> {
        if self.quests.len() > MAX_QUESTS {
            return Err(format!("at most {MAX_QUESTS} quests"));
        }
        let arch = |n: &str| self.archetypes.iter().any(|a| a.name == n);
        let mut names = HashSet::new();
        for q in &self.quests {
            let ctx = format!("quest '{}'", q.name);
            if q.name.trim().is_empty() || q.name.len() > 48 || !names.insert(q.name.as_str()) {
                return Err(format!("{ctx}: quest names must be unique, non-empty and at most 48 bytes"));
            }
            if q.title.trim().is_empty() || q.title.chars().count() > 64 || q.text.chars().count() > 600 || q.done_text.chars().count() > 600 {
                return Err(format!("{ctx}: a title (at most 64 characters) and texts of at most 600"));
            }
            if !arch(&q.giver) || q.turn_in.as_deref().is_some_and(|t| !arch(t)) {
                return Err(format!("{ctx}: giver and turn_in must be bot types"));
            }
            if q.after.as_deref().is_some_and(|a| a == q.name || !self.quests.iter().any(|o| o.name == a)) {
                return Err(format!("{ctx}: after must be another quest"));
            }
            if q.steps.is_empty() || q.steps.len() > MAX_QUEST_STEPS {
                return Err(format!("{ctx}: 1 to {MAX_QUEST_STEPS} steps"));
            }
            for st in &q.steps {
                let ok = match st {
                    QuestStep::Kill { archetype, count } => arch(archetype) && *count >= 1,
                    QuestStep::Visit { place } => self.places.iter().any(|p| &p.name == place),
                    QuestStep::Collect { item, count } => self.items.iter().any(|i| &i.name == item) && *count >= 1,
                    QuestStep::Talk { archetype } => arch(archetype),
                    QuestStep::Build { count, material } => *count >= 1 && material.as_deref().is_none_or(|m| BLOCK_NAMES[..SPROUT as usize].contains(&m)),
                };
                if !ok {
                    return Err(format!("{ctx}: a step names something that is not there (or counts nothing)"));
                }
            }
            if q.reward.iter().any(|r| r.count == 0 || !self.items.iter().any(|i| i.name == r.item)) {
                return Err(format!("{ctx}: rewards must be items the game has, at least one each"));
            }
        }
        Ok(())
    }

    fn check_director(&self) -> Result<(), String> {
        let attr = |n: &str| self.attrs.iter().any(|a| a.name == n);
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
            if e.steps.is_empty() || e.steps.len() > MAX_STEPS || e.fail_steps.len() > MAX_STEPS {
                return Err(format!("event '{}': needs 1 to {MAX_STEPS} steps (and at most {MAX_STEPS} if it fails)", e.name));
            }
            if let Some(p) = &e.place {
                if !place(p) {
                    return Err(format!("event '{}': unknown place '{p}'", e.name));
                }
            }
            if e.limit_secs > MAX_EVENT_SECS || e.cooldown_secs > MAX_EVENT_SECS {
                return Err(format!("event '{}': a time limit and cooldown are at most {MAX_EVENT_SECS} seconds", e.name));
            }
            match &e.when {
                Some(When::Players { count }) if *count == 0 || *count > 10_000 => {
                    return Err(format!("event '{}': players to start it must be 1..10000", e.name));
                }
                Some(When::After { event, .. }) if event == &e.name || !self.events.iter().any(|x| &x.name == event) => {
                    return Err(format!("event '{}': starts after '{event}', which is not another event", e.name));
                }
                Some(When::Calendar { weekday, hour, minute }) if *hour > 23 || *minute > 59 || weekday.is_some_and(|w| w > 6) => {
                    return Err(format!("event '{}': a calendar time is hour 0..23, minute 0..59, weekday 0 (Monday)..6", e.name));
                }
                _ => {}
            }
            for (i, st) in e.steps.iter().map(|s| (false, s)).chain(e.fail_steps.iter().map(|s| (true, s))).enumerate() {
                let (failing, st) = st;
                let ctx = if failing { format!("event '{}' fail step {}", e.name, i + 1 - e.steps.len()) } else { format!("event '{}' step {}", e.name, i + 1) };
                self.check_step(st, &ctx, e.place.is_some() || e.roams)?;
            }
        }
        if self.zones.len() > MAX_ZONES {
            return Err(format!("at most {MAX_ZONES} weather zones"));
        }
        let mut seen = HashSet::new();
        for z in &self.zones {
            plain(&z.name, "weather zone")?;
            if !seen.insert(z.name.as_str()) {
                return Err(format!("duplicate weather zone '{}'", z.name));
            }
            if !place(&z.place) {
                return Err(format!("weather zone '{}': unknown place '{}'", z.name, z.place));
            }
            if !self.weather.iter().any(|w| w.name == z.weather) {
                return Err(format!("weather zone '{}': unknown weather '{}'", z.name, z.weather));
            }
            if !(z.height >= 2.0 && z.height <= 500.0) {
                return Err(format!("weather zone '{}': height must be 2..500", z.name));
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
    fn a_boss_is_checked_and_resolved() {
        let boss = |phases: Vec<BossPhase>| BossFile { phases, enrage_secs: 0.0, enrage_might: 3.0, defeat: String::new() };
        let phase = |below: f32, action: &str, add: Option<&str>| BossPhase {
            below,
            say: String::new(),
            moves: vec![BossMove { action: action.into(), every_secs: 5.0, warn_secs: 2.0, ..Default::default() }],
            adds: add.map(|b| vec![BossAdd { bot: b.into(), count: 2 }]).unwrap_or_default(),
        };
        let mut g = GameFile::default_game();
        g.archetypes[2].boss = Some(boss(vec![phase(100.0, "zap", None), phase(40.0, "zap", Some("drifter"))]));
        let built = g.build().unwrap();
        let b = built.archs[2].boss.as_ref().unwrap();
        assert_eq!((b.phases[1].below, b.phases[1].moves[0].action, b.phases[1].adds[0]), (0.4, 1, (0, 2)));
        // It survives a round trip through JSON, and a file without it reads as before.
        let back: GameFile = serde_json::from_str(&serde_json::to_string(&g).unwrap()).unwrap();
        assert_eq!(back.archetypes[2].boss, g.archetypes[2].boss);
        assert!(err(|g| g.archetypes[2].boss = Some(boss(vec![]))).contains("1..8 phases"));
        assert!(err(|g| g.archetypes[2].boss = Some(boss(vec![phase(100.0, "zap", None), phase(100.0, "zap", None)]))).contains("lower than the phase before"));
        assert!(err(|g| g.archetypes[2].boss = Some(boss(vec![phase(100.0, "nuke", None)]))).contains("unknown action 'nuke'"));
        assert!(err(|g| g.archetypes[2].boss = Some(boss(vec![phase(100.0, "zap", Some("nobody"))]))).contains("no bot type 'nobody'"));
        assert!(err(|g| {
            g.archetypes[0].boss = Some(boss(vec![phase(100.0, "zap", None)]));
            g.archetypes[2].boss = Some(boss(vec![phase(100.0, "zap", Some("drifter"))]));
        })
        .contains("an add cannot be a boss"));
        assert!(err(|g| {
            let mut p = phase(100.0, "zap", None);
            p.moves[0].warn_secs = 60.0;
            g.archetypes[2].boss = Some(boss(vec![p]));
        })
        .contains("warn_secs"));
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
            ("r 1..8192", Box::new(|g| g.places[0].r = 0.0)),
            ("a title is at most", Box::new(|g| g.places[0].title = "x".repeat(41))),
            ("a title is at most", Box::new(|g| g.places[0].title = " Market".into())),
            ("an exit goes to", Box::new(|g| g.places[0].exit = Some(Exit { zone: String::new(), x: -1.0, y: 5.0 }))),
            ("outside the place itself", Box::new(|g| { let (x, y) = (g.places[0].x, g.places[0].y); g.places[0].exit = Some(Exit { zone: String::new(), x, y }); })),
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
        more.items.push(ItemDef { name: "gadget".into(), color: "#999999".into(), stack_max: 9, uses: vec![], consume: true, value: 0, equip: None, mount: 0.0, rarity: Default::default(), title: String::new(), block: None });
        assert_eq!(ok.build().unwrap().defs.content_hash(), more.build().unwrap().defs.content_hash());
        g.items.clear();
        assert!(g.build().is_err(), "drops of items that no longer exist are caught");
    }

    #[test]
    fn emotes_have_plain_unique_commands_and_fill_in_names() {
        let mut g = GameFile::default_game();
        let wave = Emote { cmd: "wave".into(), text: "{name} waves.".into(), at: "{name} waves at {target}.".into(), anim: "wave".into(), stays: false };
        g.emotes = vec![wave.clone(), Emote { cmd: "nod".into(), text: "{name} nods.".into(), at: String::new(), anim: String::new(), stays: false }];
        let game = g.build().unwrap();
        assert_eq!(game.emotes[0].line("Aoife", Some("Bran")), "Aoife waves at Bran.");
        assert_eq!(game.emotes[1].line("Aoife", Some("Bran")), "Aoife nods.", "no at line: the plain one");
        g.emotes.push(wave);
        assert!(g.build().unwrap_err().contains("twice"));
        g.emotes.pop();
        g.emotes[1].cmd = "Nod!".into();
        assert!(g.build().is_err());
    }

    #[test]
    fn the_starter_kit_names_items_and_bot_respawns_are_checked() {
        let mut g = GameFile::default_game();
        let first = g.items[0].name.clone();
        g.starter = vec![crate::realm::Stack { item: first, count: 3 }];
        assert_eq!(g.build().unwrap().starter, vec![(0, 3)]);
        g.starter[0].item = "nothing at all".into();
        assert!(g.build().unwrap_err().contains("starter"));
        g.starter.clear();
        g.archetypes[0].respawn_secs = -1.0;
        assert!(g.build().is_err());
        let a: Archetype = serde_json::from_str(r#"{"name": "rock", "respawn_secs": 60, "respawn_at": "here"}"#).unwrap();
        assert_eq!((a.respawn_secs, a.respawn_at), (60.0, RespawnAt::Here));
    }

    #[test]
    fn an_item_may_be_a_block_of_one_material_and_each_material_of_one_item() {
        assert_eq!(BLOCK_NAMES.len(), BLOCK_KINDS as usize);
        assert_eq!((material("Red Brick"), material("soil"), material("sprout"), material("mud")), (Some(2), Some(SOIL), None, None));
        let mut g = GameFile::default_game();
        assert!(!g.to_json().contains("\"block\""), "items without one write no block, so old files round-trip");
        g.items[0].block = Some("stone".into());
        let built = g.build().unwrap();
        assert_eq!(built.items[0].block, Some(0));
        g.items[1].block = Some("Stone".into());
        assert!(g.build().unwrap_err().contains("already a block of stone"));
        g.items[1].block = Some("ripe crop".into());
        assert!(g.build().is_err(), "crops come from seeds");
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
            ..EventDef::default()
        });
        g.events[0].steps = vec![
                Step::Announce { text: "Raiders at the plaza".into(), banner: true, nearby: false },
                Step::Weather { name: Some("storm".into()), duration_secs: 30 },
                Step::Spawn { archetype: g.archetypes[0].name.clone(), count: 5, place: Some("plaza".into()), name: None },
                Step::Hit { attr: "health".into(), delta: -10.0, place: Some("plaza".into()) },
                Step::Clear { archetype: g.archetypes[0].name.clone() },
            ];
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
        bad(&|h| h.events[0].steps[2] = Step::Spawn { archetype: "ghost".into(), count: 1, place: None, name: None }, "unknown bot type");
        bad(&|h| h.events[0].steps[2] = Step::Spawn { archetype: g.archetypes[0].name.clone(), count: 9999, place: None, name: None }, "count");
        bad(&|h| h.events[0].steps[3] = Step::Hit { attr: "health".into(), delta: -1.0, place: Some("nowhere".into()) }, "unknown place");
        bad(&|h| h.events.push(h.events[0].clone()), "duplicate event");
        // Raids and local events: a place to happen at, waits, a fight to win, rewards, and what starts them.
        bad(&|h| h.events[0].steps[0] = Step::Announce { text: "near".into(), banner: false, nearby: true }, "needs the event to have a place");
        bad(&|h| h.events[0].place = Some("nowhere".into()), "unknown place 'nowhere'");
        bad(&|h| h.events[0].steps.push(Step::Wait { secs: 0 }), "a wait is");
        bad(&|h| h.events[0].fail_steps.push(Step::Reward { item: "gold".into(), count: 1, everyone: false }), "fail step 1: unknown item 'gold'");
        bad(&|h| h.events[0].when = Some(When::After { event: "raid".into(), outcome: Outcome::Success }), "not another event");
        bad(&|h| h.events[0].when = Some(When::Players { count: 0 }), "players to start it");
        bad(&|h| h.events[0].steps.push(Step::Spawn { archetype: h.archetypes[0].name.clone(), count: 1, place: None, name: Some(" ".into()) }), "a name is");
        let mut raid = g.clone();
        raid.events[0].place = Some("plaza".into());
        raid.events[0].limit_secs = 600;
        raid.events[0].when = Some(When::Time { at: DayPart::Dusk });
        raid.events[0].steps = vec![
            Step::Announce { text: "To arms".into(), banner: true, nearby: true },
            Step::Wait { secs: 10 },
            Step::Spawn { archetype: g.archetypes[0].name.clone(), count: 3, place: None, name: Some("Morvane".into()) },
            Step::Defeat { within_secs: 120 },
        ];
        raid.events[0].fail_steps = vec![Step::Announce { text: "The plaza falls".into(), banner: true, nearby: false }];
        raid.build().expect("a raid builds");
        assert_eq!(GameFile::from_json(&raid.to_json()).unwrap(), raid, "and round-trips");
        assert!(GameFile::from_json(&g.to_json()).unwrap().events[0].cooldown_secs == 600, "old files get the default cooldown");
        // Removing a bot type that an event spawns is caught too.
        bad(&|h| { let n = h.archetypes[0].name.clone(); h.archetypes.retain(|a| a.name != n) }, "unknown bot type");
        // Weather zones name a place and a weather, and events switch them by name.
        let mut z = g.clone();
        z.zones.push(WeatherZone { name: "haunt".into(), place: "plaza".into(), weather: "storm".into(), active: true, height: 30.0 });
        z.events[0].steps.push(Step::Zone { zone: "haunt".into(), on: false });
        z.build().expect("a zone over a known place with a known weather builds");
        assert_eq!(GameFile::from_json(&z.to_json()).unwrap(), z);
        let zbad = |f: &dyn Fn(&mut GameFile), needle: &str| {
            let mut h = z.clone();
            f(&mut h);
            let e = h.build().err().unwrap_or_else(|| panic!("should refuse: {needle}"));
            assert!(e.contains(needle), "{e} should mention {needle}");
        };
        zbad(&|h| h.zones[0].place = "nowhere".into(), "unknown place 'nowhere'");
        zbad(&|h| h.zones[0].weather = "hail".into(), "unknown weather 'hail'");
        zbad(&|h| h.zones[0].height = 0.5, "height");
        zbad(&|h| h.zones.push(h.zones[0].clone()), "duplicate weather zone");
        zbad(&|h| h.events[0].steps[5] = Step::Zone { zone: "elsewhere".into(), on: true }, "unknown weather zone");
        // Weather and events change nothing on the wire.
        let mut w = g.clone();
        w.weather.clear();
        w.events.clear();
        assert_eq!(g.build().unwrap().defs.content_hash(), w.build().unwrap().defs.content_hash());
    }
}

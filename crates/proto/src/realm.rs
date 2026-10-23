//! What makes a game a world to live in rather than a sandbox: sides to fight for (factions), classes, loot tables
//! and rarity, crafting, farming, land and dungeons. All of it lives in the game file next to the rest, and none of
//! it is in the content hash, so editing any of it applies live without dropping anyone.
//!
//! The file form (`FactionDef`, `ClassDef`, ...) names things; `GameFile::realm` checks the names and resolves them
//! to indices in a `Realm`, which is what the server works from.

use serde::{Deserialize, Serialize};

use crate::game::GameFile;

/// Most of each kind of entry, so a game file cannot make the server walk a huge list per kill or craft.
pub const MAX_FACTIONS: usize = 16;
pub const MAX_CLASSES: usize = 16;
pub const MAX_LOOT_TABLES: usize = 64;
pub const MAX_LOOT_ENTRIES: usize = 32;
pub const MAX_RECIPES: usize = 128;
pub const MAX_CROPS: usize = 32;
pub const MAX_DUNGEONS: usize = 32;

/// If the game file has an attribute of this name, the server keeps it at each player's faction (index + 1, 0 for
/// none). Make it replicated and clients can tell friend from foe.
pub const FACTION_ATTR: &str = "faction";

/// Likewise for each player's class (index + 1, 0 for none): replicated, clients know which spells are theirs.
pub const CLASS_ATTR: &str = "class";

/// How rare an item is: clients colour its name by this, and a drop of `Rare` or better is announced.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
#[serde(rename_all = "lowercase")]
pub enum Rarity {
    #[default]
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub fn is_common(&self) -> bool {
        *self == Rarity::Common
    }

    pub fn title(self) -> &'static str {
        match self {
            Rarity::Common => "common",
            Rarity::Uncommon => "uncommon",
            Rarity::Rare => "rare",
            Rarity::Epic => "epic",
            Rarity::Legendary => "legendary",
        }
    }
}

/// A side players can join (`/faction name`). Players on the same side never harm each other, whatever `pvp` says.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct FactionDef {
    pub name: String,
    /// What players read ("The Iron Crown"); the name when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default = "d_faction_color")]
    pub color: String,
}

fn d_faction_color() -> String {
    "#c0c0c0".into()
}

/// A calling (`/class name`): which schools of spells it may use. Actions without a school (a plain blow) are for
/// everyone. In a game with classes, a player who has not picked one uses only those.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClassDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    pub schools: Vec<String>,
}

/// What a bot type leaves when it falls (`Archetype::loot` names one): `rolls` picks, each weighted among the
/// entries and `nothing`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LootTable {
    pub name: String,
    #[serde(default = "d_one_u8")]
    pub rolls: u8,
    /// Weight of a roll giving nothing at all.
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub nothing: u32,
    pub entries: Vec<LootEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LootEntry {
    pub item: String,
    #[serde(default = "d_one_u32")]
    pub weight: u32,
    #[serde(default = "d_one_u8")]
    pub min: u8,
    #[serde(default = "d_one_u8")]
    pub max: u8,
}

/// Some of one item.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Stack {
    pub item: String,
    #[serde(default = "d_one_u8")]
    pub count: u8,
}

/// `/craft name`: takes `needs` from the bag and gives `count` of `makes`, optionally only near a place (a forge).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub name: String,
    pub needs: Vec<Stack>,
    pub makes: String,
    #[serde(default = "d_one_u8")]
    pub count: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub near: Option<String>,
}

/// A crop: using a `seed` item while standing by tilled soil plants it; after `grow_secs` the sprout is ripe, and
/// breaking the ripe crop gives `min..=max` of `yields`. Breaking a sprout gives the seed back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CropDef {
    pub name: String,
    pub seed: String,
    pub yields: String,
    #[serde(default = "d_one_u8")]
    pub min: u8,
    #[serde(default = "d_one_u8")]
    pub max: u8,
    #[serde(default = "d_grow")]
    pub grow_secs: f32,
}

fn d_grow() -> f32 {
    300.0
}

/// Land: `/claim` takes a square of `size` blocks on a side centred where you stand, and only you (and your guild)
/// may build or break inside it. `/unclaim` gives it up.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlotRules {
    #[serde(default = "d_plot_size")]
    pub size: u16,
    #[serde(default = "d_one_u8")]
    pub per_player: u8,
}

fn d_plot_size() -> u16 {
    16
}

/// A dungeon: whoever walks into `place` (with their party) gets their own copy of its monsters, which nobody else
/// sees or meets. When the last of them falls the party is rewarded; when everyone has left it closes.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DungeonDef {
    pub name: String,
    pub place: String,
    pub bots: Vec<DungeonBot>,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub xp: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward: Option<Stack>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DungeonBot {
    pub bot: String,
    #[serde(default = "d_one_u8")]
    pub count: u8,
}

fn d_one_u8() -> u8 {
    1
}

fn d_one_u32() -> u32 {
    1
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClassRt {
    pub name: String,
    pub title: String,
    pub schools: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LootRt {
    pub rolls: u8,
    pub nothing: u32,
    /// (item, weight, min, max)
    pub entries: Vec<(usize, u32, u8, u8)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecipeRt {
    pub name: String,
    pub needs: Vec<(usize, u8)>,
    pub makes: usize,
    pub count: u8,
    pub near: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CropRt {
    pub name: String,
    pub seed: usize,
    pub yields: usize,
    pub min: u8,
    pub max: u8,
    pub grow_secs: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DungeonRt {
    pub name: String,
    pub place: usize,
    /// (bot type, count)
    pub bots: Vec<(usize, u8)>,
    pub xp: u32,
    pub reward: Option<(usize, u8)>,
}

/// The checked, resolved form of the realm parts of a game file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Realm {
    pub factions: Vec<FactionDef>,
    pub classes: Vec<ClassRt>,
    pub loot: Vec<LootRt>,
    pub recipes: Vec<RecipeRt>,
    pub crops: Vec<CropRt>,
    pub plots: Option<PlotRules>,
    pub dungeons: Vec<DungeonRt>,
}

impl Realm {
    pub fn faction(&self, name: &str) -> Option<usize> {
        let n = name.trim();
        self.factions.iter().position(|f| f.name.eq_ignore_ascii_case(n) || (!f.title.is_empty() && f.title.eq_ignore_ascii_case(n)))
    }

    pub fn class(&self, name: &str) -> Option<usize> {
        let n = name.trim();
        self.classes.iter().position(|c| c.name.eq_ignore_ascii_case(n) || (!c.title.is_empty() && c.title.eq_ignore_ascii_case(n)))
    }

    pub fn recipe(&self, name: &str) -> Option<usize> {
        let n = name.trim();
        self.recipes.iter().position(|r| r.name.eq_ignore_ascii_case(n))
    }

    /// Whether a player of class `class` (index + 1, 0 for none) may use an action of `school`.
    pub fn may_use(&self, class: u8, school: &str) -> bool {
        if self.classes.is_empty() || school.is_empty() {
            return true;
        }
        class > 0 && self.classes.get(class as usize - 1).is_some_and(|c| c.schools.iter().any(|s| s.eq_ignore_ascii_case(school)))
    }
}

fn name_ok(name: &str, what: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 32 || name.chars().any(char::is_control) {
        return Err(format!("{what} names must be 1..32 characters"));
    }
    Ok(())
}

fn unique<'a>(names: impl Iterator<Item = &'a str>, what: &str) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for n in names {
        name_ok(n, what)?;
        if !seen.insert(n.to_lowercase()) {
            return Err(format!("two {what}s are called '{n}'"));
        }
    }
    Ok(())
}

fn color_ok(c: &str, ctx: &str) -> Result<(), String> {
    let hex = c.strip_prefix('#').unwrap_or("");
    if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err(format!("{ctx}: colour must be #rrggbb"));
    }
    Ok(())
}

fn amounts(min: u8, max: u8, ctx: &str) -> Result<(), String> {
    if min == 0 || min > max || max > 99 {
        return Err(format!("{ctx}: amounts need 1 <= min <= max <= 99"));
    }
    Ok(())
}

impl GameFile {
    /// Check the realm parts and resolve their names (items, bot types, places) to indices.
    pub fn realm(&self) -> Result<Realm, String> {
        let item = |n: &str, ctx: &str| self.items.iter().position(|i| i.name == n).ok_or_else(|| format!("{ctx}: '{n}' is not an item"));
        let place = |n: &str, ctx: &str| self.places.iter().position(|p| p.name == n).ok_or_else(|| format!("{ctx}: '{n}' is not a place"));
        let bot = |n: &str, ctx: &str| self.archetypes.iter().position(|a| a.name == n).ok_or_else(|| format!("{ctx}: '{n}' is not a bot type"));

        if self.factions.len() > MAX_FACTIONS {
            return Err(format!("at most {MAX_FACTIONS} factions"));
        }
        unique(self.factions.iter().map(|f| f.name.as_str()), "faction")?;
        for f in &self.factions {
            color_ok(&f.color, &format!("faction '{}'", f.name))?;
            if f.title.chars().count() > 48 {
                return Err(format!("faction '{}': title is at most 48 characters", f.name));
            }
        }

        if self.classes.len() > MAX_CLASSES {
            return Err(format!("at most {MAX_CLASSES} classes"));
        }
        unique(self.classes.iter().map(|c| c.name.as_str()), "class")?;
        let mut classes = Vec::new();
        for c in &self.classes {
            if c.schools.is_empty() || c.schools.len() > 8 {
                return Err(format!("class '{}': give it 1 to 8 schools", c.name));
            }
            classes.push(ClassRt { name: c.name.clone(), title: c.title.clone(), schools: c.schools.clone() });
        }

        if self.loot.len() > MAX_LOOT_TABLES {
            return Err(format!("at most {MAX_LOOT_TABLES} loot tables"));
        }
        unique(self.loot.iter().map(|t| t.name.as_str()), "loot table")?;
        let mut loot = Vec::new();
        for t in &self.loot {
            let ctx = format!("loot table '{}'", t.name);
            if t.entries.is_empty() || t.entries.len() > MAX_LOOT_ENTRIES {
                return Err(format!("{ctx}: give it 1 to {MAX_LOOT_ENTRIES} entries"));
            }
            if t.rolls == 0 || t.rolls > 8 {
                return Err(format!("{ctx}: rolls must be 1..8"));
            }
            let mut entries = Vec::new();
            for e in &t.entries {
                if e.weight == 0 || e.weight > 1_000_000 {
                    return Err(format!("{ctx}: weights must be 1..1000000"));
                }
                amounts(e.min, e.max, &ctx)?;
                entries.push((item(&e.item, &ctx)?, e.weight, e.min, e.max));
            }
            if t.nothing > 1_000_000 {
                return Err(format!("{ctx}: nothing must be 0..1000000"));
            }
            loot.push(LootRt { rolls: t.rolls, nothing: t.nothing, entries });
        }
        for a in &self.archetypes {
            if let Some(l) = &a.loot {
                if !self.loot.iter().any(|t| &t.name == l) {
                    return Err(format!("bot type '{}': '{l}' is not a loot table", a.name));
                }
            }
        }

        if self.recipes.len() > MAX_RECIPES {
            return Err(format!("at most {MAX_RECIPES} recipes"));
        }
        unique(self.recipes.iter().map(|r| r.name.as_str()), "recipe")?;
        let mut recipes = Vec::new();
        for r in &self.recipes {
            let ctx = format!("recipe '{}'", r.name);
            if r.needs.is_empty() || r.needs.len() > 6 {
                return Err(format!("{ctx}: needs 1 to 6 ingredients"));
            }
            let needs = r.needs.iter().map(|s| {
                amounts(s.count, s.count, &ctx)?;
                Ok((item(&s.item, &ctx)?, s.count))
            });
            let needs = needs.collect::<Result<Vec<_>, String>>()?;
            amounts(r.count, r.count, &ctx)?;
            let near = r.near.as_deref().map(|p| place(p, &ctx)).transpose()?;
            recipes.push(RecipeRt { name: r.name.clone(), needs, makes: item(&r.makes, &ctx)?, count: r.count, near });
        }

        if self.crops.len() > MAX_CROPS {
            return Err(format!("at most {MAX_CROPS} crops"));
        }
        unique(self.crops.iter().map(|c| c.name.as_str()), "crop")?;
        let mut crops = Vec::new();
        for c in &self.crops {
            let ctx = format!("crop '{}'", c.name);
            amounts(c.min, c.max, &ctx)?;
            if !(c.grow_secs.is_finite() && (1.0..=7.0 * 86_400.0).contains(&c.grow_secs)) {
                return Err(format!("{ctx}: grow_secs must be 1..604800"));
            }
            let seed = item(&c.seed, &ctx)?;
            if crops.iter().any(|o: &CropRt| o.seed == seed) {
                return Err(format!("{ctx}: another crop grows from '{}'", c.seed));
            }
            crops.push(CropRt { name: c.name.clone(), seed, yields: item(&c.yields, &ctx)?, min: c.min, max: c.max, grow_secs: c.grow_secs });
        }

        if let Some(p) = &self.plots {
            if !(4..=128).contains(&p.size) || p.per_player == 0 || p.per_player > 8 {
                return Err("plots: size must be 4..128 and per_player 1..8".into());
            }
        }

        if self.dungeons.len() > MAX_DUNGEONS {
            return Err(format!("at most {MAX_DUNGEONS} dungeons"));
        }
        unique(self.dungeons.iter().map(|d| d.name.as_str()), "dungeon")?;
        let mut dungeons = Vec::new();
        for d in &self.dungeons {
            let ctx = format!("dungeon '{}'", d.name);
            if d.bots.is_empty() || d.bots.len() > 8 || d.bots.iter().map(|b| b.count as u32).sum::<u32>() > 64 {
                return Err(format!("{ctx}: 1 to 8 kinds of bot, at most 64 in all"));
            }
            let bots = d.bots.iter().map(|b| Ok((bot(&b.bot, &ctx)?, b.count.max(1)))).collect::<Result<Vec<_>, String>>()?;
            let reward = match &d.reward {
                Some(s) => {
                    amounts(s.count, s.count, &ctx)?;
                    Some((item(&s.item, &ctx)?, s.count))
                }
                None => None,
            };
            let at = place(&d.place, &ctx)?;
            if dungeons.iter().any(|o: &DungeonRt| o.place == at) {
                return Err(format!("{ctx}: another dungeon is at '{}'", d.place));
            }
            dungeons.push(DungeonRt { name: d.name.clone(), place: at, bots, xp: d.xp, reward });
        }

        Ok(Realm { factions: self.factions.clone(), classes, loot, recipes, crops, plots: self.plots.clone(), dungeons })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game() -> GameFile {
        let mut g = GameFile::default_game();
        g.items.push(crate::game::ItemDef { name: "carrot seed".into(), ..g.items[0].clone() });
        g.items.push(crate::game::ItemDef { name: "carrot".into(), ..g.items[0].clone() });
        g
    }

    #[test]
    fn a_game_with_none_of_it_has_an_empty_realm() {
        assert_eq!(GameFile::default_game().realm().unwrap(), Realm::default());
    }

    #[test]
    fn names_resolve_and_bad_ones_are_refused() {
        let mut g = game();
        g.factions = vec![FactionDef { name: "red".into(), title: "The Red".into(), color: "#ff0000".into() }];
        g.classes = vec![ClassDef { name: "mage".into(), title: String::new(), schools: vec!["fire".into()] }];
        g.crops = vec![CropDef { name: "carrot".into(), seed: "carrot seed".into(), yields: "carrot".into(), min: 1, max: 3, grow_secs: 60.0 }];
        g.recipes = vec![Recipe { name: "stew".into(), needs: vec![Stack { item: "carrot".into(), count: 2 }], makes: "carrot seed".into(), count: 1, near: None }];
        let r = g.realm().unwrap();
        assert_eq!(r.faction("THE RED"), Some(0));
        assert_eq!(r.class("Mage"), Some(0));
        assert_eq!(r.crops[0].yields, g.items.len() - 1);
        assert!(r.may_use(1, "Fire") && !r.may_use(1, "frost") && !r.may_use(0, "fire") && r.may_use(0, ""));

        let mut bad = g.clone();
        bad.recipes[0].makes = "nothing".into();
        assert!(bad.realm().unwrap_err().contains("not an item"));
        let mut bad = g.clone();
        bad.archetypes[0].loot = Some("missing".into());
        assert!(bad.realm().unwrap_err().contains("not a loot table"));
        let mut bad = g;
        bad.factions.push(bad.factions[0].clone());
        assert!(bad.realm().unwrap_err().contains("two factions"));
    }
}

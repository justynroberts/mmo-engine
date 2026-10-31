//! What makes a game a world to live in rather than a sandbox: sides to fight for (factions), classes, loot tables
//! and rarity, crafting, farming, land and dungeons. All of it lives in the game file next to the rest, and none of
//! it is in the content hash, so editing any of it applies live without dropping anyone.
//!
//! Factions (`game::FactionFile`) and classes (`game::ClassFile`) are the game file's own; the rest here is named
//! by its file form (`LootTable`, `Recipe`, ...), and `GameFile::realm` checks the names and resolves them
//! to indices in a `Realm`, which is what the server works from.

use serde::{Deserialize, Serialize};

use crate::game::GameFile;

/// Most of each kind of entry, so a game file cannot make the server walk a huge list per kill or craft.
pub const MAX_LOOT_TABLES: usize = 64;
pub const MAX_LOOT_ENTRIES: usize = 32;
pub const MAX_RECIPES: usize = 128;
pub const MAX_CROPS: usize = 32;
pub const MAX_DUNGEONS: usize = 32;

/// If the game file has an attribute of this name, the server keeps it at each player's faction, the side they joined
/// with `/faction` (index + 1, 0 for none). Make it replicated and clients can tell friend from foe.
pub const FACTION_ATTR: &str = "faction";

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
    /// A skill (`GameFile::skills`) it needs `level` of, and raises while it is not far below the maker; past the
    /// level the maker sometimes makes one more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u16")]
    pub level: u16,
}

fn is_zero_u16(n: &u16) -> bool {
    *n == 0
}

/// The deeds a character counts, for achievements: these, or "kill:<bot type>" for one kind.
pub const DEEDS: [&str; 8] = ["kills", "quests", "places", "crafts", "harvests", "builds", "falls", "dungeons"];

/// An achievement: `count` of one deed (`DEEDS`, or "kill:<bot type>"), earned once, with experience and perhaps a
/// title the player may then wear (`grants`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Achievement {
    pub name: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    pub deed: String,
    pub count: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub grants: String,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub xp: u32,
}

/// A skill that rises with use, from 0 to `max`: crafting with recipes that name it, farming crops that name it,
/// gathering by beating bot types that name it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SkillDef {
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default = "d_skill_max")]
    pub max: u16,
}

fn d_skill_max() -> u16 {
    100
}

impl SkillDef {
    pub fn shown(&self) -> &str {
        if self.title.is_empty() { &self.name } else { &self.title }
    }
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
    /// A skill harvesting it raises, and each 25 of it a sheaf more.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
}

fn d_grow() -> f32 {
    300.0
}

/// Land: `/claim` takes a square of `size` blocks on a side centred where you stand, for `cost` of the game's
/// currency, and only you (and your guild) may build or break inside it. `/plot grow` buys the next of `sizes`, a
/// bigger square about the same centre; `/unclaim` gives it up (nothing is paid back).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlotRules {
    #[serde(default = "d_plot_size")]
    pub size: u16,
    #[serde(default = "d_one_u8")]
    pub per_player: u8,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub cost: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<PlotSize>,
}

/// A bigger plot to grow into, and what growing to it costs.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PlotSize {
    pub size: u16,
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub cost: u32,
    /// The level a player must have reached to grow to it (0 for any).
    #[serde(default, skip_serializing_if = "is_zero_u8")]
    pub level: u8,
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
    /// How many players the group finder brings together for it (3 when left out).
    #[serde(default = "d_group", skip_serializing_if = "is_default_group")]
    pub group: u8,
}

fn d_group() -> u8 {
    3
}

fn is_default_group(n: &u8) -> bool {
    *n == 3
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

fn is_zero_u8(v: &u8) -> bool {
    *v == 0
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
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
    pub skill: Option<usize>,
    pub level: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CropRt {
    pub name: String,
    pub seed: usize,
    pub yields: usize,
    pub min: u8,
    pub max: u8,
    pub grow_secs: f32,
    pub skill: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DungeonRt {
    pub name: String,
    pub place: usize,
    /// (bot type, count)
    pub bots: Vec<(usize, u8)>,
    pub xp: u32,
    pub reward: Option<(usize, u8)>,
    pub group: u8,
}

/// The checked, resolved form of the realm parts of a game file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Realm {
    pub loot: Vec<LootRt>,
    pub recipes: Vec<RecipeRt>,
    pub crops: Vec<CropRt>,
    pub plots: Option<PlotRules>,
    pub dungeons: Vec<DungeonRt>,
    pub skills: Vec<SkillDef>,
    pub achievements: Vec<Achievement>,
    /// Per bot type (index in `GameFile::archetypes`): the skill beating one raises.
    pub arch_skill: Vec<Option<usize>>,
}

/// A faction by name or title, any case (what a player types after `/faction`).
pub fn find_faction(factions: &[crate::game::FactionFile], name: &str) -> Option<usize> {
    let n = name.trim();
    factions.iter().position(|f| f.name.eq_ignore_ascii_case(n) || (!f.title.is_empty() && f.title.eq_ignore_ascii_case(n)))
}

/// A class by name or title, any case (what a player types after `/class`).
pub fn find_class(classes: &[crate::game::ClassRt], name: &str) -> Option<usize> {
    let n = name.trim();
    classes.iter().position(|c| c.name.eq_ignore_ascii_case(n) || c.title.eq_ignore_ascii_case(n))
}

impl Realm {
    pub fn recipe(&self, name: &str) -> Option<usize> {
        let n = name.trim();
        self.recipes.iter().position(|r| r.name.eq_ignore_ascii_case(n))
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

pub(crate) fn color_ok(c: &str, ctx: &str) -> Result<(), String> {
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

        if self.achievements.len() > 256 {
            return Err("at most 256 achievements".into());
        }
        unique(self.achievements.iter().map(|a| a.name.as_str()), "achievement")?;
        for a in &self.achievements {
            let ctx = format!("achievement '{}'", a.name);
            let known = DEEDS.contains(&a.deed.as_str()) || a.deed.strip_prefix("kill:").is_some_and(|k| self.archetypes.iter().any(|x| x.name == k));
            if !known {
                return Err(format!("{ctx}: deed must be one of {} or kill:<bot type>", DEEDS.join(", ")));
            }
            if a.count == 0 || a.title.is_empty() || a.title.len() > 64 || a.grants.len() > 32 || a.grants.contains('|') {
                return Err(format!("{ctx}: a title, a count of at least 1, and a title it grants of at most 32 characters"));
            }
        }

        if self.skills.len() > 32 {
            return Err("at most 32 skills".into());
        }
        unique(self.skills.iter().map(|s| s.name.as_str()), "skill")?;
        if let Some(s) = self.skills.iter().find(|s| s.max == 0 || s.max > 1000) {
            return Err(format!("skill '{}': max must be 1..1000", s.name));
        }
        let skill = |n: &Option<String>, ctx: &str| -> Result<Option<usize>, String> {
            n.as_deref().map(|n| self.skills.iter().position(|s| s.name == n).ok_or_else(|| format!("{ctx}: '{n}' is not a skill"))).transpose()
        };
        let mut arch_skill = self.archetypes.iter().map(|a| skill(&a.skill, &format!("bot type '{}'", a.name))).collect::<Result<Vec<_>, String>>()?;
        if arch_skill.iter().all(Option::is_none) {
            arch_skill.clear(); // (read with get: a game with no gathering keeps none)
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
            let sk = skill(&r.skill, &ctx)?;
            if sk.is_some_and(|k| r.level > self.skills[k].max) {
                return Err(format!("{ctx}: a level past its skill's max"));
            }
            recipes.push(RecipeRt { name: r.name.clone(), needs, makes: item(&r.makes, &ctx)?, count: r.count, near, skill: sk, level: r.level });
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
            crops.push(CropRt { name: c.name.clone(), seed, yields: item(&c.yields, &ctx)?, min: c.min, max: c.max, grow_secs: c.grow_secs, skill: skill(&c.skill, &ctx)? });
        }

        if let Some(p) = &self.plots {
            if !(4..=128).contains(&p.size) || p.per_player == 0 || p.per_player > 8 {
                return Err("plots: size must be 4..128 and per_player 1..8".into());
            }
            let mut last = p.size;
            for s in &p.sizes {
                if s.size <= last || s.size > 128 {
                    return Err("plots: each of sizes must be bigger than the one before (and size), up to 128".into());
                }
                last = s.size;
            }
            if p.sizes.len() > 8 || p.cost > 1_000_000 || p.sizes.iter().any(|s| s.cost > 1_000_000) {
                return Err("plots: at most 8 sizes, and costs up to 1000000".into());
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
            if d.group == 0 {
                return Err(format!("dungeon '{}': a group of at least 1", d.name));
            }
            dungeons.push(DungeonRt { name: d.name.clone(), place: at, bots, xp: d.xp, reward, group: d.group });
        }

        Ok(Realm { loot, recipes, crops, plots: self.plots.clone(), dungeons, skills: self.skills.clone(), achievements: self.achievements.clone(), arch_skill })
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
        g.factions = vec![crate::game::FactionFile { name: "red".into(), title: "The Red".into(), color: "#ff0000".into(), ..Default::default() }];
        g.crops = vec![CropDef { name: "carrot".into(), seed: "carrot seed".into(), yields: "carrot".into(), min: 1, max: 3, grow_secs: 60.0, skill: None }];
        g.recipes = vec![Recipe { name: "stew".into(), needs: vec![Stack { item: "carrot".into(), count: 2 }], makes: "carrot seed".into(), count: 1, near: None, skill: None, level: 0 }];
        let r = g.realm().unwrap();
        assert_eq!(find_faction(&g.factions, "THE RED"), Some(0));
        assert_eq!(r.crops[0].yields, g.items.len() - 1);

        let mut bad = g.clone();
        bad.recipes[0].makes = "nothing".into();
        assert!(bad.realm().unwrap_err().contains("not an item"));
        let mut bad = g.clone();
        bad.archetypes[0].loot = Some("missing".into());
        assert!(bad.realm().unwrap_err().contains("not a loot table"));
        let mut bad = g;
        bad.factions[0].color = "red".into();
        assert!(bad.build().unwrap_err().contains("colour"));
    }
}

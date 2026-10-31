//! The server half of `proto::realm`: factions, classes, loot tables, land, farming, guilds, crafting and dungeons.
//!
//! Players reach all of it through "/" chat commands (`command`), so every client already has a way in:
//! `/faction`, `/class`, `/claim`, `/unclaim`, `/plot`, `/guild ...`, `/g`, `/craft`, `/recipes`. The rest happens
//! in the world: a seed used beside tilled soil plants it, breaking a ripe crop harvests it, a bot with a loot table
//! rolls it when it falls, and walking into a dungeon's place gives you and your party your own copy of its
//! monsters (a "phase": entities of another phase are not sent to you, and nothing crosses between phases).
//!
//! Guilds, plots and crops are kept in the store's world table under "realm" (`Saved`), keyed by account tag and
//! by name, so editing the game file never scrambles them.

use std::collections::HashMap;

use proto::chat::channel;
use proto::game::{RIPE, SOIL, SPROUT};
use proto::world::{block_key, BLOCK};
use serde::{Deserialize, Serialize};

use crate::net::Clients;
use crate::sim::{Sim, DRIVEN, NONE};

/// Most guilds (how many in one is `Rules::guild_size`).
pub const GUILDS_MAX: usize = 500;
/// Most plots in a world, and growing crops.
pub const PLOTS_MAX: usize = 20_000;
pub const CROPS_MAX: usize = 50_000;
/// How far past a dungeon place's edge a player still counts as inside (so stepping on the edge does not flicker).
pub const DUNGEON_SLACK: f32 = 1.5;
/// How far round a player the land held is told to them (world units).
pub const LAND_REACH: f32 = 160.0;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Guild {
    pub name: String,
    /// Account tags.
    pub leader: String,
    pub members: Vec<String>,
}

/// A square of land, in block cells: `x0..x1`, `y0..y1`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Rect {
    pub fn has(&self, cx: u32, cy: u32) -> bool {
        (self.x0..self.x1).contains(&cx) && (self.y0..self.y1).contains(&cy)
    }

    pub fn meets(&self, o: &Rect) -> bool {
        self.x0 < o.x1 && o.x0 < self.x1 && self.y0 < o.y1 && o.y0 < self.y1
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Plot {
    /// The owner's account tag.
    pub owner: String,
    pub area: Rect,
    /// What its owner calls it (`/plot name`): told to those who walk in, listed by `/plots`, found by `/visit`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
}

/// A crop in the ground: which, and how long until it is ripe (0 once it is).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SavedCrop {
    pub key: u32,
    pub z: i16,
    pub crop: String,
    pub secs_left: f32,
}

/// What is kept between runs.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Saved {
    #[serde(default)]
    pub guilds: Vec<Guild>,
    #[serde(default)]
    pub plots: Vec<Plot>,
    #[serde(default)]
    pub crops: Vec<SavedCrop>,
    /// The auction house (`market.rs`): what is for sale, what is owed to sellers, and the next listing's id.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub listings: Vec<crate::market::Listing>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post: Vec<crate::market::Post>,
    #[serde(default)]
    pub next_listing: u32,
    /// Letters waiting to be read (`mail.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mail: Vec<crate::mail::Letter>,
}

/// One party's copy of a dungeon.
#[derive(Clone, Debug)]
pub struct Run {
    pub dungeon: usize,
    pub phase: u32,
    /// The party it belongs to (`Sim::party`), or the solo player's entity id with the top bit set.
    pub owner: u32,
    /// Its monsters, by entity id.
    pub bots: Vec<u32>,
    pub cleared: bool,
    /// The tick it last had a player in it.
    pub seen: u32,
}

/// The realm's live state.
#[derive(Default)]
pub struct State {
    /// Per slot: the faction joined (index + 1 in `Sim::factions`, 0 for none), phase (0 for the shared world), and a
    /// guild invitation waiting (guild index + 1). A player's class is `Sim::class`.
    pub faction: Vec<u8>,
    pub phase: Vec<u32>,
    pub guild_invite: Vec<u16>,
    /// Per slot: the name of the named plot they were last seen in ("" for none), to tell them when they walk in.
    pub plot_here: Vec<String>,
    /// Per slot: the land near them as last told (chat channel LAND), so it goes again only when it changes.
    pub land_sent: Vec<String>,
    pub guilds: Vec<Guild>,
    pub plots: Vec<Plot>,
    /// Growing and ripe crops: (column key, layer) -> (crop name, tick it ripens).
    pub crops: HashMap<(u32, i16), (String, u32)>,
    pub runs: Vec<Run>,
    next_phase: u32,
    /// Something kept between runs changed and should be saved.
    pub dirty: bool,
}

impl State {
    pub fn new(cap: usize) -> State {
        State { faction: vec![0; cap], phase: vec![0; cap], guild_invite: vec![0; cap], plot_here: vec![String::new(); cap], land_sent: vec![String::new(); cap], ..Default::default() }
    }

    pub fn reset(&mut self, slot: usize) {
        self.faction[slot] = 0;
        self.phase[slot] = 0;
        self.guild_invite[slot] = 0;
        self.plot_here[slot].clear();
        self.land_sent[slot].clear();
    }

    /// The guild an account belongs to.
    pub fn guild_of(&self, tag: &str) -> Option<usize> {
        self.guilds.iter().position(|g| g.members.iter().any(|m| m == tag))
    }
}

/// The text after a command word.
fn split(text: &str) -> (String, String) {
    let t = text.trim_start_matches('/');
    let (cmd, rest) = t.split_once(' ').unwrap_or((t, ""));
    (cmd.to_lowercase(), rest.trim().to_string())
}

impl Sim {
    /// A player's account tag (None for a guest).
    pub fn tag_of(&self, s: usize) -> Option<String> {
        let a = self.ents.account[s];
        (a > 0).then(|| self.blocks.owners.get(a as usize - 1).cloned()).flatten()
    }

    fn is_live_player(&self, s: usize) -> bool {
        self.ents.alive[s] && self.ents.owner[s] < DRIVEN
    }

    /// Whether `a` and `b` are on the same side: both in the same faction.
    pub fn allies(&self, a: usize, b: usize) -> bool {
        let (fa, fb) = (self.rs.faction[a], self.rs.faction[b]);
        fa != 0 && fa == fb
    }

    /// Whether two entities share a world: the same phase, or both players (players always see each other).
    pub fn same_phase(&self, a: usize, b: usize) -> bool {
        self.rs.phase[a] == self.rs.phase[b] || (self.ents.owner[a] < DRIVEN && self.ents.owner[b] < DRIVEN)
    }

    /// Whether the player in `s` may build or break in cell (cx, cy): nobody's land, their own, or their guild's.
    pub fn may_build(&self, s: usize, cx: u32, cy: u32) -> bool {
        let Some(p) = self.rs.plots.iter().find(|p| p.area.has(cx, cy)) else { return true };
        let Some(me) = self.tag_of(s) else { return false };
        if p.owner == me {
            return true;
        }
        matches!((self.rs.guild_of(&p.owner), self.rs.guild_of(&me)), (Some(a), Some(b)) if a == b)
    }

    /// Roll a fallen bot's loot table onto the ground where it fell.
    #[cfg(test)]
    pub fn roll_loot(&mut self, slot: usize) {
        let (x, y) = (self.ents.x[slot], self.ents.y[slot]);
        for (item, n, (jx, jy)) in self.roll_table(slot, true) {
            self.put_ground(x + jx, y + jy, item, n.min(99) as u8);
        }
    }

    /// Roll a fallen bot's loot table: (item, count, where it lands) for each that came up (drawn straight after it
    /// with `scatter`), anything rare announced to the players near it.
    pub fn roll_table(&mut self, slot: usize, scatter: bool) -> Vec<(usize, u32, (f32, f32))> {
        let mut out = Vec::new();
        let Some(t) = self.archs.get(self.ents.arch[slot] as usize).and_then(|a| a.loot) else { return out };
        let Some(table) = self.realm.loot.get(t).cloned() else { return out };
        let (x, y) = (self.ents.x[slot], self.ents.y[slot]);
        let total: u64 = table.nothing as u64 + table.entries.iter().map(|e| e.1 as u64).sum::<u64>();
        for _ in 0..table.rolls {
            let mut pick = (self.rng.next_u32() as u64 * total) >> 32;
            if pick < table.nothing as u64 {
                continue;
            }
            pick -= table.nothing as u64;
            let Some(&(item, _, min, max)) = table.entries.iter().find(|e| {
                if pick < e.1 as u64 {
                    true
                } else {
                    pick -= e.1 as u64;
                    false
                }
            }) else {
                continue;
            };
            let n = min as u32 + self.rng.next_u32() % ((max - min) as u32 + 1);
            let at = if scatter { self.jitter() } else { (0.0, 0.0) };
            out.push((item, n, at));
            let rarity = self.items[item].rarity;
            if rarity >= proto::realm::Rarity::Rare {
                let line = format!("A {} {} falls from the {}!", rarity.title(), self.items[item].shown(), self.name_of(slot));
                let r2 = self.defs.interest_radius * self.defs.interest_radius;
                for p in 0..self.ents.high {
                    if self.is_live_player(p) && (self.ents.x[p] - x).powi(2) + (self.ents.y[p] - y).powi(2) <= r2 {
                        self.tells.push((p, line.clone()));
                    }
                }
            }
        }
        out
    }

    /// Plant the crop that grows from item `item` in the nearest tilled soil within reach that has room on top.
    /// False if the item is not a seed or there is nowhere to plant it.
    pub fn plant(&mut self, s: usize, item: usize) -> bool {
        let Some(crop) = self.realm.crops.iter().position(|c| c.seed == item) else { return false };
        if !self.realm.crops.is_empty() && self.rs.crops.len() >= CROPS_MAX {
            return false;
        }
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let (ex, ey) = ((x / BLOCK) as i64, (y / BLOCK) as i64);
        let mut best: Option<(f32, u32, u32, i16)> = None;
        for cy in ey - 2..=ey + 2 {
            for cx in ex - 2..=ex + 2 {
                if cx < 0 || cy < 0 {
                    continue;
                }
                let (cx, cy) = (cx as u32, cy as u32);
                let Some(col) = self.blocks.get(block_key(cx, cy)) else { continue };
                // The top block of the column, which must be soil with an empty layer above it.
                let Some(top) = (0..col.kinds.len()).rev().find(|&k| col.kinds[k] != 0) else { continue };
                let z = col.base as i16 + top as i16;
                if col.kinds[top] != SOIL + 1 || col.index(z + 1).is_none() || !self.may_build(s, cx, cy) {
                    continue;
                }
                let d = ((cx as f32 + 0.5) * BLOCK - x).powi(2) + ((cy as f32 + 0.5) * BLOCK - y).powi(2);
                if d <= proto::game::BUILD_REACH * proto::game::BUILD_REACH && best.map_or(true, |b| d < b.0) {
                    best = Some((d, cx, cy, z + 1));
                }
            }
        }
        let Some((_, cx, cy, z)) = best else { return false };
        let key = block_key(cx, cy);
        let base = self.blocks.get(key).map(|c| c.base).unwrap_or(0);
        self.blocks.put(key, base, z, SPROUT, self.ents.account[s]);
        if let Some(c) = self.blocks.get(key) {
            self.world.set_column(cx, cy, c.column());
        }
        let ticks = (self.realm.crops[crop].grow_secs * self.season_growth() * self.tick_hz as f32).round() as u32;
        self.rs.crops.insert((key, z), (self.realm.crops[crop].name.clone(), self.tick + ticks.max(1)));
        self.rs.dirty = true;
        true
    }

    /// A growing or ripe crop was broken by `s`: a sprout gives its seed back, a ripe one its harvest.
    pub fn harvest(&mut self, s: usize, key: u32, z: i16, kind: u8) {
        let Some((name, _)) = self.rs.crops.remove(&(key, z)) else { return };
        self.rs.dirty = true;
        let Some(c) = self.realm.crops.iter().find(|c| c.name == name).cloned() else { return };
        if kind == RIPE {
            self.lived(s, crate::life::Deed::Harvest);
            let skilled = c.skill.map_or(0, |k| self.skill_level(s, k) as u32 / 25);
            let n = c.min as u32 + self.rng.next_u32() % ((c.max - c.min) as u32 + 1) + self.season_yield() + skilled;
            self.give_or_drop(s, c.yields, n);
            if let Some(k) = c.skill {
                self.raise_skill(s, k);
            }
            self.deed(s, "harvests", 1);
        } else if kind == SPROUT {
            self.give_or_drop(s, c.seed, 1);
        }
    }

    /// Once a second: crops ripen, dungeons open and close, and the `faction` attribute follows each player's side.
    pub fn system_realm(&mut self) {
        let now = self.tick;
        let ripe: Vec<(u32, i16)> = self.rs.crops.iter().filter(|(_, v)| v.1 != 0 && v.1 <= now).map(|(k, _)| *k).collect();
        for (key, z) in ripe {
            if let Some(v) = self.rs.crops.get_mut(&(key, z)) {
                v.1 = 0;
            }
            let still = self.blocks.get(key).and_then(|c| c.index(z).map(|k| (c.kinds[k], c.owners[k], c.base)));
            match still {
                Some((k, owner, base)) if k == SPROUT + 1 => {
                    self.blocks.take(key, z);
                    self.blocks.put(key, base, z, RIPE, owner);
                }
                _ => {
                    self.rs.crops.remove(&(key, z)); // the block went some other way (a world edit)
                }
            }
            self.rs.dirty = true;
        }
        if let Some(a) = self.defs.attrs.iter().position(|d| d.name == proto::realm::FACTION_ATTR) {
            for s in 0..self.ents.high {
                if self.is_live_player(s) {
                    self.ents.attrs[a][s] = self.rs.faction[s] as f32;
                }
            }
        }
        self.system_dungeons();
        self.system_plot_names();
        self.system_land();
        self.system_finder();
    }

    /// Each player is told the land held within `LAND_REACH` of them, theirs marked, when that changes (chat channel
    /// LAND), so a client can draw its edges and say what standing there means.
    fn system_land(&mut self) {
        if self.realm.plots.is_none() {
            return;
        }
        for s in 0..self.ents.high {
            if !self.is_live_player(s) {
                continue;
            }
            let me = self.tag_of(s);
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            let hz = self.tick_hz.max(1) as f32;
            let mut entries = Vec::new();
            // What was told, with the tick the next crop ripens rather than seconds left, so a clock running down
            // is not news (the client counts down itself).
            let mut said = Vec::new();
            for p in &self.rs.plots {
                let [x0, y0, x1, y1] = [p.area.x0, p.area.y0, p.area.x1, p.area.y1].map(|v| v as f32 * BLOCK);
                if x0 - LAND_REACH > x || x1 + LAND_REACH < x || y0 - LAND_REACH > y || y1 + LAND_REACH < y {
                    continue;
                }
                let mine = Some(&p.owner) == me.as_ref();
                // On one's own land: when the soonest crop growing there is ripe (0 for none growing).
                let next = if mine {
                    self.rs.crops.iter().filter(|(&(key, _), v)| v.1 != 0 && p.area.has(key & 0x7fff, key >> 15)).map(|(_, v)| v.1).min().unwrap_or(0)
                } else {
                    0
                };
                let secs = if next == 0 { 0 } else { (next.saturating_sub(self.tick) as f32 / hz).ceil() as u32 };
                let name: String = p.name.chars().filter(|c| !matches!(c, ',' | ';' | '|')).collect();
                let head = format!("{},{},{},{},{}", x0 as i32, y0 as i32, x1 as i32, y1 as i32, mine as u8);
                said.push(format!("{head},{next},{name}"));
                entries.push(format!("{head},{secs},{name}"));
            }
            let all = said.join(";");
            if all == self.rs.land_sent[s] {
                continue;
            }
            self.rs.land_sent[s] = all;
            // As few lines as will carry it, each inside a chat line.
            let mut line = String::from("L0|");
            for e in &entries {
                if line.len() + e.len() + 1 > proto::chat::MAX_TEXT {
                    self.notes.push((s, 0, proto::chat::channel::LAND, std::mem::replace(&mut line, String::from("L1|"))));
                }
                if !line.ends_with('|') {
                    line.push(';');
                }
                line.push_str(e);
            }
            self.notes.push((s, 0, proto::chat::channel::LAND, line));
        }
    }

    /// Once a second: a player who walks into a named plot is told whose land it is.
    fn system_plot_names(&mut self) {
        if self.rs.plots.iter().all(|p| p.name.is_empty()) && self.rs.plot_here.iter().all(|n| n.is_empty()) {
            return;
        }
        for s in 0..self.ents.high {
            if !self.is_live_player(s) {
                continue;
            }
            let (cx, cy) = ((self.ents.x[s] / BLOCK) as u32, (self.ents.y[s] / BLOCK) as u32);
            let name = self.rs.plots.iter().find(|p| !p.name.is_empty() && p.area.has(cx, cy)).map_or("", |p| p.name.as_str());
            if name != self.rs.plot_here[s] {
                if !name.is_empty() {
                    self.tells.push((s, format!("You walk into {name}.")));
                }
                self.rs.plot_here[s] = name.to_string();
            }
        }
    }

    fn system_dungeons(&mut self) {
        for d in 0..self.realm.dungeons.len() {
            let Some(place) = self.places.get(self.realm.dungeons[d].place).cloned() else { continue };
            let r2 = (place.r + DUNGEON_SLACK).powi(2);
            for s in 0..self.ents.high {
                if !self.is_live_player(s) {
                    continue;
                }
                let inside = (self.ents.x[s] - place.x).powi(2) + (self.ents.y[s] - place.y).powi(2) <= r2;
                let ph = self.rs.phase[s];
                let mine = self.rs.runs.iter().position(|r| r.phase == ph && ph != 0);
                if inside && ph == 0 {
                    let owner = if self.party[s] != 0 { self.party[s] } else { self.ents.id_of(s) | 1 << 31 };
                    let run = match self.rs.runs.iter().position(|r| r.dungeon == d && r.owner == owner) {
                        Some(i) => i,
                        None => self.open_run(d, owner, place.x, place.y, place.r),
                    };
                    self.rs.phase[s] = self.rs.runs[run].phase;
                    let line = format!("You enter {}.", self.realm.dungeons[d].name);
                    self.tells.push((s, line));
                } else if !inside && mine.is_some_and(|i| self.rs.runs[i].dungeon == d) {
                    self.rs.phase[s] = 0;
                    self.tells.push((s, format!("You leave {}.", self.realm.dungeons[d].name)));
                }
            }
        }
        // Each run: a player still in it keeps it open; all its monsters down clears it; long empty closes it.
        let idle = proto::rules::Rules::ticks(self.rules.dungeon_idle_secs, self.tick_hz);
        let mut i = 0;
        while i < self.rs.runs.len() {
            let phase = self.rs.runs[i].phase;
            let inside: Vec<usize> = (0..self.ents.high).filter(|&s| self.is_live_player(s) && self.rs.phase[s] == phase).collect();
            if !inside.is_empty() {
                self.rs.runs[i].seen = self.tick;
            }
            let alive = self.rs.runs[i].bots.iter().filter(|&&id| self.ents.resolve(id).is_some()).count();
            if alive == 0 && !self.rs.runs[i].cleared {
                self.rs.runs[i].cleared = true;
                let d = self.realm.dungeons[self.rs.runs[i].dungeon].clone();
                for &s in &inside {
                    if d.xp > 0 {
                        self.award(s, d.xp, None);
                    }
                    if let Some((item, n)) = d.reward {
                        self.give_or_drop(s, item, n as u32);
                    }
                    self.tells.push((s, format!("{} is cleared!", d.name)));
                    self.deed(s, "dungeons", 1);
                }
            }
            if self.tick.saturating_sub(self.rs.runs[i].seen) > idle {
                let run = self.rs.runs.swap_remove(i);
                for id in run.bots {
                    if let Some(s) = self.ents.resolve(id) {
                        self.ents.despawn(s);
                    }
                }
                continue;
            }
            i += 1;
        }
    }

    fn open_run(&mut self, d: usize, owner: u32, x: f32, y: f32, r: f32) -> usize {
        self.rs.next_phase = self.rs.next_phase.wrapping_add(1).max(1);
        let phase = self.rs.next_phase;
        let mut bots = Vec::new();
        for (arch, n) in self.realm.dungeons[d].bots.clone() {
            for _ in 0..n {
                let ang = self.rng.f32() * std::f32::consts::TAU;
                let dist = self.rng.f32().sqrt() * r * 0.8;
                let Some(slot) = self.spawn_as(NONE, arch as u8, Some((x + ang.cos() * dist, y + ang.sin() * dist))) else { continue };
                self.ents.fleeting[slot] = true;
                self.rs.phase[slot] = phase;
                bots.push(self.ents.id_of(slot));
            }
        }
        self.rs.runs.push(Run { dungeon: d, phase, owner, bots, cleared: false, seen: self.tick });
        self.rs.runs.len() - 1
    }

    /// What is kept between runs.
    pub fn realm_saved(&self) -> Saved {
        let hz = self.tick_hz.max(1) as f32;
        let mut crops: Vec<SavedCrop> = self
            .rs
            .crops
            .iter()
            .map(|(&(key, z), (crop, at))| SavedCrop { key, z, crop: crop.clone(), secs_left: if *at == 0 { 0.0 } else { at.saturating_sub(self.tick) as f32 / hz } })
            .collect();
        crops.sort_by_key(|c| (c.key, c.z));
        Saved {
            guilds: self.rs.guilds.clone(),
            plots: self.rs.plots.clone(),
            crops,
            listings: self.market.listings.clone(),
            post: self.market.post.clone(),
            next_listing: self.market.next_id,
            mail: self.market.mail.clone(),
        }
    }

    /// Bring back what was kept. Call after the blocks are in: a crop whose block is gone is dropped.
    pub fn realm_restore(&mut self, saved: Saved) {
        self.rs.guilds = saved.guilds;
        self.rs.plots = saved.plots;
        self.market.listings = saved.listings;
        self.market.post = saved.post;
        self.market.mail = saved.mail;
        self.market.next_id = saved.next_listing.max(self.market.listings.iter().map(|l| l.id + 1).max().unwrap_or(1)).max(1);
        self.rs.crops.clear();
        for c in saved.crops {
            let there = self.blocks.get(c.key).and_then(|col| col.index(c.z).map(|k| col.kinds[k])).is_some_and(|k| k == SPROUT + 1 || k == RIPE + 1);
            if there {
                let at = if c.secs_left <= 0.0 { 0 } else { self.tick + ((c.secs_left * self.tick_hz as f32) as u32).max(1) };
                self.rs.crops.insert((c.key, c.z), (c.crop, at));
            }
        }
    }
}

fn say(clients: &mut Clients, slot: usize, text: &str) {
    if let Some(c) = clients.slab.iter_mut().flatten().find(|c| c.slot == slot) {
        c.chat.deliver(0, channel::SAY, text);
    }
}

fn tell(clients: &mut Clients, slot: usize, from: u32, ch: u8, text: &str) {
    if let Some(c) = clients.slab.iter_mut().flatten().find(|c| c.slot == slot) {
        c.chat.deliver(from, ch, text);
    }
}

/// The commands this module answers, for the help line.
pub const HELP: &str = "/faction Name, /class Name, /claim, /unclaim, /plot [grow|name Name], /plots, /visit Plot, /guild create|invite|accept|leave|kick, /g words, /craft Name, /recipes, /skills, /title [Name|none], /talents, /talent Name, /respec, /look cape|helmet, /trade Name";

/// Handle a realm "/" command. Returns false if it is not one of ours.
pub fn command(sim: &mut Sim, clients: &mut Clients, slot: usize, text: &str) -> bool {
    let (cmd, rest) = split(text);
    match cmd.as_str() {
        "faction" | "side" => faction(sim, clients, slot, &rest),
        "class" => class(sim, clients, slot, &rest),
        "title" | "titles" => sim.title_command(slot, &rest),
        "look" | "appearance" => sim.look_command(slot, &rest),
        "talents" | "talent" | "respec" => sim.talent_command(slot, if cmd == "talents" { "list" } else { cmd.as_str() }, &rest),
        "claim" => claim(sim, clients, slot),
        "unclaim" => unclaim(sim, clients, slot),
        "plot" => plot(sim, clients, slot, &rest),
        "plots" => plots(sim, clients, slot),
        "visit" => visit(sim, clients, slot, &rest),
        "guild" => guild(sim, clients, slot, &rest),
        "g" => guild_say(sim, clients, slot, &rest),
        "craft" => craft(sim, clients, slot, &rest),
        "trade" => match find(sim, &rest) {
            Some(t) => {
                let id = sim.ents.id_of(t);
                sim.trade_action(slot, (proto::game::trade_op::ASK as u16) << 8, id);
            }
            None => say(clients, slot, "Nobody of that name is about."),
        },
        "skills" => {
            let line = sim.skills_line(slot);
            say(clients, slot, &line);
        }
        "recipes" => {
            let line = if sim.realm.recipes.is_empty() {
                "Nothing can be made here.".to_string()
            } else {
                let list: Vec<String> = sim
                    .realm
                    .recipes
                    .iter()
                    .map(|r| {
                        let needs: Vec<String> = r.needs.iter().map(|&(i, n)| format!("{n} {}", sim.items[i].shown())).collect();
                        let skill = r.skill.map(|k| format!(", {} {}", sim.realm.skills[k].shown(), r.level)).unwrap_or_default();
                        format!("{} ({}{skill})", r.name, needs.join(", "))
                    })
                    .collect();
                format!("Recipes: {}", list.join("; "))
            };
            say(clients, slot, &line);
        }
        _ => return false,
    }
    true
}

/// Whether a player stands somewhere safe (where changing side is allowed once chosen).
fn settled(sim: &Sim, slot: usize) -> bool {
    let (x, y) = (sim.ents.x[slot], sim.ents.y[slot]);
    sim.places.iter().any(|p| p.safe && (p.x - x).powi(2) + (p.y - y).powi(2) <= p.r * p.r)
}

fn faction(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    if sim.factions.is_empty() {
        return say(clients, slot, "There are no factions in this world.");
    }
    if rest.is_empty() {
        let mine = sim.rs.faction[slot];
        let names: Vec<&str> = sim.factions.iter().map(|f| f.name.as_str()).collect();
        let now = if mine == 0 { "none".to_string() } else { sim.factions[mine as usize - 1].name.clone() };
        return say(clients, slot, &format!("Your faction: {now}. Factions: {}. /faction Name", names.join(", ")));
    }
    let Some(f) = proto::realm::find_faction(&sim.factions, rest) else { return say(clients, slot, "No faction of that name.") };
    if sim.rs.faction[slot] != 0 && !settled(sim, slot) {
        return say(clients, slot, "You can change sides only in a safe place.");
    }
    // A side that thinks you its enemy will not have you (beating its people is what puts you there).
    if sim.standing(slot, f) < sim.factions[f].hostile {
        return say(clients, slot, &format!("{} will not have you: your standing with them is too low.", sim.factions[f].shown()));
    }
    sim.rs.faction[slot] = f as u8 + 1;
    sim.progress_ver[slot] = sim.progress_ver[slot].wrapping_add(1); // saved at once
    say(clients, slot, &format!("You fight for {}.", sim.factions[f].shown()));
}

/// `/class Name` chooses a class the way the class picker does: once (a character keeps the class it chose).
fn class(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    if sim.classes.is_empty() {
        return say(clients, slot, "There are no classes in this world.");
    }
    if rest.is_empty() || sim.class[slot] != 0 {
        let names: Vec<String> = sim.classes.iter().map(|c| c.name.clone()).collect();
        let now = sim.class_of(slot).map_or("none".to_string(), |c| c.title.clone());
        let how = if sim.class[slot] == 0 { " /class Name" } else { " (chosen for good)" };
        return say(clients, slot, &format!("Your class: {now}. Classes: {}.{how}", names.join(", ")));
    }
    let Some(c) = proto::realm::find_class(&sim.classes, rest) else { return say(clients, slot, "No class of that name.") };
    sim.choose_class(slot, c as u16);
}

fn here(sim: &Sim, slot: usize) -> (u32, u32) {
    ((sim.ents.x[slot] / BLOCK) as u32, (sim.ents.y[slot] / BLOCK) as u32)
}

fn claim(sim: &mut Sim, clients: &mut Clients, slot: usize) {
    let Some(rules) = sim.realm.plots.clone() else { return say(clients, slot, "Land cannot be claimed in this world.") };
    let Some(me) = sim.tag_of(slot) else { return say(clients, slot, "Guests cannot own land.") };
    if sim.rs.plots.iter().filter(|p| p.owner == me).count() >= rules.per_player as usize {
        return say(clients, slot, &format!("You hold all the land you may ({}). /unclaim one first.", rules.per_player));
    }
    if sim.rs.plots.len() >= PLOTS_MAX {
        return say(clients, slot, "No more land can be claimed.");
    }
    let (cx, cy) = here(sim, slot);
    let area = square(sim, cx, cy, rules.size);
    if let Err(why) = free_land(sim, &area, None) {
        return say(clients, slot, why);
    }
    if let Err(why) = pay(sim, slot, rules.cost) {
        return say(clients, slot, &why);
    }
    sim.rs.plots.push(Plot { owner: me, area, name: String::new() });
    sim.rs.dirty = true;
    let (w, h) = (area.x1 - area.x0, area.y1 - area.y0);
    let paces = |n: u32| (n as f32 * BLOCK) as u32;
    say(clients, slot, &format!("This land is yours: {w} by {h} blocks ({} by {} paces) round where you stand. Only you and your guild may build here.", paces(w), paces(h)));
}

fn unclaim(sim: &mut Sim, clients: &mut Clients, slot: usize) {
    let (cx, cy) = here(sim, slot);
    let me = sim.tag_of(slot);
    match sim.rs.plots.iter().position(|p| p.area.has(cx, cy) && Some(&p.owner) == me.as_ref()) {
        Some(i) => {
            sim.rs.plots.swap_remove(i);
            sim.rs.dirty = true;
            say(clients, slot, "You give up this land.");
        }
        None => say(clients, slot, "You hold no land here."),
    }
}

/// A square of `size` blocks about (cx, cy), inside the world.
fn square(sim: &Sim, cx: u32, cy: u32, size: u16) -> Rect {
    let half = size as u32 / 2;
    let edge = (sim.world.size / BLOCK) as u32;
    let (x0, y0) = (cx.saturating_sub(half), cy.saturating_sub(half));
    Rect { x0, y0, x1: (x0 + size as u32).min(edge), y1: (y0 + size as u32).min(edge) }
}

/// Whether land may be held: nobody else's (plot `except` aside, the one growing) and no town (a safe place belongs
/// to everyone).
fn free_land(sim: &Sim, area: &Rect, except: Option<usize>) -> Result<(), &'static str> {
    if sim.rs.plots.iter().enumerate().any(|(i, p)| Some(i) != except && p.area.meets(area)) {
        return Err("That overlaps land someone holds.");
    }
    let town = sim.places.iter().any(|p| {
        let (px, py) = (p.x / BLOCK, p.y / BLOCK);
        let r = p.r / BLOCK;
        p.safe && px + r > area.x0 as f32 && px - r < area.x1 as f32 && py + r > area.y0 as f32 && py - r < area.y1 as f32
    });
    if town {
        return Err("Nobody may claim land in a town.");
    }
    Ok(())
}

/// Take a price in the game's currency from a player's bag (nothing to pay in a game without one, or at 0).
fn pay(sim: &mut Sim, slot: usize, cost: u32) -> Result<(), String> {
    let Some(c) = sim.currency.filter(|_| cost > 0 && !sim.ents.god[slot]) else { return Ok(()) };
    if !crate::market::take(&mut sim.inv[slot], c as u8 + 1, cost) {
        return Err(format!("That costs {cost} {}, and you have not got it.", sim.items[c].shown()));
    }
    sim.inv_ver[slot] = sim.inv_ver[slot].wrapping_add(1);
    Ok(())
}

/// The plot of yours you stand in.
fn my_plot_here(sim: &Sim, slot: usize) -> Option<usize> {
    let (cx, cy) = here(sim, slot);
    let me = sim.tag_of(slot)?;
    sim.rs.plots.iter().position(|p| p.area.has(cx, cy) && p.owner == me)
}

/// `/plot` says whose land this is; `/plot grow` buys the next size up; `/plot name Words` names yours.
fn plot(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    let (sub, arg) = split(rest);
    match sub.as_str() {
        // What people type first: the same as /claim and /unclaim.
        "claim" => claim(sim, clients, slot),
        "unclaim" => unclaim(sim, clients, slot),
        "grow" => {
            let Some(i) = my_plot_here(sim, slot) else { return say(clients, slot, "Stand on your own land to grow it.") };
            let a = sim.rs.plots[i].area;
            let now = (a.x1 - a.x0).max(a.y1 - a.y0);
            let Some(next) = sim.realm.plots.as_ref().and_then(|r| r.sizes.iter().find(|s| s.size as u32 > now).copied()) else {
                return say(clients, slot, "Your land is as big as land comes.");
            };
            let area = square(sim, (a.x0 + a.x1) / 2, (a.y0 + a.y1) / 2, next.size);
            if sim.levels.is_some() && sim.ents.level[slot] < next.level && !sim.ents.god[slot] {
                return say(clients, slot, &format!("Land that big wants a level {} holder.", next.level));
            }
            if let Err(why) = free_land(sim, &area, Some(i)) {
                return say(clients, slot, why);
            }
            if let Err(why) = pay(sim, slot, next.cost) {
                return say(clients, slot, &why);
            }
            sim.rs.plots[i].area = area;
            sim.rs.dirty = true;
            say(clients, slot, &format!("Your land grows to {} by {} blocks.", area.x1 - area.x0, area.y1 - area.y0));
        }
        "name" => {
            let Some(i) = my_plot_here(sim, slot) else { return say(clients, slot, "Stand on your own land to name it.") };
            let name: String = arg.chars().filter(|c| !c.is_control()).take(32).collect::<String>().trim().to_string();
            if !name.is_empty() && sim.rs.plots.iter().enumerate().any(|(k, p)| k != i && p.name.eq_ignore_ascii_case(&name)) {
                return say(clients, slot, "Another holding has that name.");
            }
            let line = if name.is_empty() { "Your land has no name now.".to_string() } else { format!("Your land is {name}.") };
            sim.rs.plots[i].name = name;
            sim.rs.dirty = true;
            say(clients, slot, &line);
        }
        _ => {
            let (cx, cy) = here(sim, slot);
            let line = match sim.rs.plots.iter().find(|p| p.area.has(cx, cy)) {
                None => "Nobody holds this land.".to_string(),
                Some(p) if Some(&p.owner) == sim.tag_of(slot).as_ref() => {
                    let called = if p.name.is_empty() { " /plot name Words to name it.".to_string() } else { format!(" It is {}.", p.name) };
                    format!("This land is yours, {} by {} blocks.{called}", p.area.x1 - p.area.x0, p.area.y1 - p.area.y0)
                }
                Some(p) => {
                    let guild = sim.rs.guild_of(&p.owner).map(|g| format!(" (of {})", sim.rs.guilds[g].name)).unwrap_or_default();
                    let called = if p.name.is_empty() { String::new() } else { format!(": {}", p.name) };
                    format!("This land is held by another{guild}{called}.")
                }
            };
            say(clients, slot, &line);
        }
    }
}

/// `/plots`: the named holdings, nearest first.
fn plots(sim: &mut Sim, clients: &mut Clients, slot: usize) {
    let (cx, cy) = here(sim, slot);
    let mut named: Vec<(u32, &str)> = sim.rs.plots.iter().filter(|p| !p.name.is_empty()).map(|p| (cx.abs_diff((p.area.x0 + p.area.x1) / 2) + cy.abs_diff((p.area.y0 + p.area.y1) / 2), p.name.as_str())).collect();
    named.sort();
    let n = named.len();
    let list: Vec<&str> = named.iter().take(30).map(|&(_, name)| name).collect();
    let line = if n == 0 { "No holding has a name yet.".to_string() } else { format!("{n} named holdings: {}. /visit Name", list.join(", ")) };
    say(clients, slot, &line);
}

/// `/visit Name`: which way a named holding lies, and how far.
fn visit(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    let want = rest.trim();
    let Some(p) = sim.rs.plots.iter().find(|p| !p.name.is_empty() && p.name.eq_ignore_ascii_case(want)) else {
        return say(clients, slot, "No holding of that name. /plots lists them.");
    };
    let (tx, ty) = ((p.area.x0 + p.area.x1) as f32 * BLOCK / 2.0, (p.area.y0 + p.area.y1) as f32 * BLOCK / 2.0);
    let (dx, dy) = (tx - sim.ents.x[slot], ty - sim.ents.y[slot]);
    let d = dx.hypot(dy);
    let line = if p.area.has(here(sim, slot).0, here(sim, slot).1) {
        format!("You are in {}.", p.name)
    } else {
        format!("{} lies {:.0} to the {}, at {:.0}, {:.0}.", p.name, d, compass(dx, dy), tx, ty)
    };
    say(clients, slot, &line);
}

/// One of eight directions (y grows south).
fn compass(dx: f32, dy: f32) -> &'static str {
    const NAMES: [&str; 8] = ["east", "south-east", "south", "south-west", "west", "north-west", "north", "north-east"];
    let k = ((dy.atan2(dx) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
    NAMES[k as usize]
}

/// A player by display name, ignoring case.
fn find(sim: &Sim, name: &str) -> Option<usize> {
    let want = name.trim().to_lowercase();
    (0..sim.ents.high).find(|&s| sim.is_live_player(s) && sim.ents.name[s].as_deref().is_some_and(|n| n.to_lowercase() == want))
}

/// Everyone online in guild `g`.
fn online(sim: &Sim, g: usize) -> Vec<usize> {
    (0..sim.ents.high).filter(|&s| sim.is_live_player(s) && sim.tag_of(s).is_some_and(|t| sim.rs.guilds[g].members.contains(&t))).collect()
}

fn guild(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    let Some(me) = sim.tag_of(slot) else { return say(clients, slot, "Guests cannot join guilds.") };
    let (sub, arg) = split(rest);
    let mine = sim.rs.guild_of(&me);
    let my_name = sim.name_of(slot).to_string();
    match sub.as_str() {
        "create" | "found" => {
            if mine.is_some() {
                return say(clients, slot, "You are in a guild already. /guild leave");
            }
            let name: String = arg.chars().filter(|c| !c.is_control()).take(32).collect::<String>().trim().to_string();
            if name.chars().count() < 3 {
                return say(clients, slot, "Name your guild: /guild create Name (3 to 32 letters)");
            }
            if sim.rs.guilds.iter().any(|g| g.name.eq_ignore_ascii_case(&name)) {
                return say(clients, slot, "There is a guild of that name.");
            }
            if sim.rs.guilds.len() >= GUILDS_MAX {
                return say(clients, slot, "No more guilds can be founded.");
            }
            sim.rs.guilds.push(Guild { name: name.clone(), leader: me.clone(), members: vec![me] });
            sim.rs.dirty = true;
            say(clients, slot, &format!("You found {name}. /guild invite Name"));
        }
        "invite" | "inv" => {
            let Some(g) = mine else { return say(clients, slot, "You are in no guild.") };
            let Some(to) = find(sim, &arg) else { return say(clients, slot, "Nobody of that name is here.") };
            let Some(their) = sim.tag_of(to) else { return say(clients, slot, "Guests cannot join guilds.") };
            if sim.rs.guild_of(&their).is_some() {
                return say(clients, slot, &format!("{} is in a guild already.", sim.name_of(to)));
            }
            if sim.rs.guilds[g].members.len() >= sim.rules.guild_size as usize {
                return say(clients, slot, "Your guild is full.");
            }
            sim.rs.guild_invite[to] = g as u16 + 1;
            let gname = sim.rs.guilds[g].name.clone();
            say(clients, to, &format!("{my_name} asks you to join {gname}. /guild accept"));
            say(clients, slot, &format!("You ask {} to join {gname}.", sim.name_of(to)));
        }
        "accept" | "join" => {
            let inv = sim.rs.guild_invite[slot];
            sim.rs.guild_invite[slot] = 0;
            let Some(g) = (inv as usize).checked_sub(1).filter(|&g| g < sim.rs.guilds.len()) else { return say(clients, slot, "No guild has asked you.") };
            if mine.is_some() {
                return say(clients, slot, "You are in a guild already.");
            }
            sim.rs.guilds[g].members.push(me);
            sim.rs.dirty = true;
            let line = format!("{my_name} joins {}.", sim.rs.guilds[g].name);
            for m in online(sim, g) {
                say(clients, m, &line);
            }
        }
        "leave" => {
            let Some(g) = mine else { return say(clients, slot, "You are in no guild.") };
            let name = sim.rs.guilds[g].name.clone();
            let rest_online: Vec<usize> = online(sim, g).into_iter().filter(|&m| m != slot).collect();
            let gd = &mut sim.rs.guilds[g];
            gd.members.retain(|m| *m != me);
            if gd.members.is_empty() {
                sim.rs.guilds.remove(g);
                for i in sim.rs.guild_invite.iter_mut() {
                    // Indices after it moved down one; invitations to it are void.
                    if *i as usize == g + 1 {
                        *i = 0;
                    } else if *i as usize > g + 1 {
                        *i -= 1;
                    }
                }
            } else if gd.leader == me {
                gd.leader = gd.members[0].clone();
            }
            sim.rs.dirty = true;
            for m in rest_online {
                say(clients, m, &format!("{my_name} leaves {name}."));
            }
            say(clients, slot, &format!("You leave {name}."));
        }
        "kick" => {
            let Some(g) = mine else { return say(clients, slot, "You are in no guild.") };
            if sim.rs.guilds[g].leader != me {
                return say(clients, slot, "Only the guild's leader can do that.");
            }
            let Some(who) = find(sim, &arg) else { return say(clients, slot, "Nobody of that name is here.") };
            let Some(their) = sim.tag_of(who).filter(|t| *t != me && sim.rs.guilds[g].members.contains(t)) else { return say(clients, slot, "They are not in your guild.") };
            sim.rs.guilds[g].members.retain(|m| *m != their);
            sim.rs.dirty = true;
            say(clients, who, &format!("You are no longer in {}.", sim.rs.guilds[g].name));
            say(clients, slot, &format!("{} is out of the guild.", sim.name_of(who)));
        }
        _ => {
            let line = match mine {
                None => "You are in no guild. /guild create Name, or be invited.".to_string(),
                Some(g) => {
                    let names: Vec<String> = online(sim, g).iter().map(|&m| sim.name_of(m).to_string()).collect();
                    let gd = &sim.rs.guilds[g];
                    format!("{}: {} members, {} online ({}). /g words to talk to them.", gd.name, gd.members.len(), names.len(), names.join(", "))
                }
            };
            say(clients, slot, &line);
        }
    }
}

fn guild_say(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    let Some(g) = sim.tag_of(slot).and_then(|t| sim.rs.guild_of(&t)) else { return say(clients, slot, "You are in no guild.") };
    let msg: String = rest.chars().filter(|c| !c.is_control()).take(200).collect();
    if msg.trim().is_empty() {
        return;
    }
    let me = sim.ents.id_of(slot);
    for m in online(sim, g) {
        tell(clients, m, me, channel::GUILD, &msg);
    }
    clients.log_chat(sim.tick, me, channel::GUILD, &msg);
}

fn craft(sim: &mut Sim, clients: &mut Clients, slot: usize, rest: &str) {
    if rest.is_empty() {
        return say(clients, slot, "Make what? /craft Name (/recipes lists them)");
    }
    let Some(r) = sim.realm.recipe(rest).map(|r| sim.realm.recipes[r].clone()) else { return say(clients, slot, "No recipe of that name. /recipes") };
    if let Some(p) = r.near.and_then(|p| sim.places.get(p)) {
        let (x, y) = (sim.ents.x[slot], sim.ents.y[slot]);
        if (p.x - x).powi(2) + (p.y - y).powi(2) > p.r * p.r {
            let at = if p.title.is_empty() { p.name.clone() } else { p.title.clone() };
            return say(clients, slot, &format!("That is made at {at}."));
        }
    }
    let lv = r.skill.map_or(0, |k| sim.skill_level(slot, k));
    if let Some(k) = r.skill.filter(|_| lv < r.level) {
        return say(clients, slot, &format!("You need {} {} to make that (you have {lv}).", sim.realm.skills[k].shown(), r.level));
    }
    let have = |item: usize| sim.inv[slot].iter().filter(|b| b.0 as usize == item + 1).map(|b| b.1 as u32).sum::<u32>();
    if let Some(&(item, n)) = r.needs.iter().find(|&&(item, n)| have(item) < n as u32) {
        return say(clients, slot, &format!("You need {n} {}.", sim.items[item].shown()));
    }
    for &(item, n) in &r.needs {
        let mut left = n as u32;
        for b in sim.inv[slot].iter_mut().rev() {
            if left == 0 {
                break;
            }
            if b.0 as usize == item + 1 {
                let t = (b.1 as u32).min(left);
                b.1 -= t as u8;
                left -= t;
                if b.1 == 0 {
                    b.0 = 0;
                }
            }
        }
    }
    sim.inv_ver[slot] = sim.inv_ver[slot].wrapping_add(1);
    // Skill past the recipe's level now and then makes one more batch (up to half the time); under the level and a
    // span, making it teaches.
    let extra = r.skill.is_some() && sim.rng.f32() < ((lv - r.level) as f32 / 200.0).min(0.5);
    let count = r.count as u32 * if extra { 2 } else { 1 };
    sim.give_or_drop(slot, r.makes, count);
    let what = sim.items[r.makes].shown().to_string();
    say(clients, slot, &format!("You make {count} {what}{}.", if extra { ", your skill making the most of it" } else { "" }));
    if let Some(k) = r.skill.filter(|_| lv < r.level + crate::skills::GAIN_SPAN) {
        sim.raise_skill(slot, k);
    }
    sim.deed(slot, "crafts", 1);
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::game::{build_arg, build_target, ClassFile, FactionFile, GameFile, ACT_BREAK, ACT_BUILD};
    use proto::realm::*;
    use proto::world::World;

    fn sim_with(f: impl FnOnce(&mut GameFile)) -> Sim {
        let mut g = GameFile::default_game();
        g.archetypes.iter_mut().for_each(|a| a.count = 0);
        f(&mut g);
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, Vec::new()), 64, 20, 1);
        s.terrain = Default::default();
        s
    }

    fn player(s: &mut Sim, x: f32, y: f32, tag: &str) -> usize {
        let p = s.spawn(0).unwrap();
        s.ents.x[p] = x;
        s.ents.y[p] = y;
        s.set_account(p, Some(tag));
        p
    }

    #[test]
    fn allies_never_hurt_each_other_and_foes_do() {
        let mut s = sim_with(|g| {
            g.factions = vec![
                FactionFile { name: "red".into(), color: "#ff0000".into(), ..Default::default() },
                FactionFile { name: "blue".into(), color: "#0000ff".into(), ..Default::default() },
            ];
        });
        let (a, b, c) = (player(&mut s, 100.0, 100.0, "a"), player(&mut s, 101.0, 100.0, "b"), player(&mut s, 100.0, 101.0, "c"));
        s.rs.faction[a] = 1;
        s.rs.faction[b] = 1;
        s.rs.faction[c] = 2;
        assert!(s.allies(a, b) && !s.allies(a, c) && !s.allies(b, c));
        let hp = |s: &Sim, p| s.ents.attrs[0][p];
        let zap = s.defs.actions.iter().find(|a| a.name == "zap").unwrap().id;
        let before = (hp(&s, b), hp(&s, c));
        for (t, tick) in [(b, 0), (c, 40)] {
            s.tick += tick;
            let id = s.ents.id_of(t);
            s.step(&[crate::sim::PendingAction { slot: a, action: zap, target: id, arg: 0 }]);
        }
        assert_eq!(hp(&s, b), before.0, "an ally is not harmed");
        assert!(hp(&s, c) < before.1, "a foe is");
    }

    #[test]
    fn classes_gate_schools() {
        let mut s = sim_with(|g| {
            let zap = g.actions.iter_mut().find(|a| a.name == "zap").unwrap();
            zap.school = "fire".into();
            g.classes = vec![
                ClassFile { name: "mage".into(), title: "Mage".into(), schools: vec!["fire".into()], ..Default::default() },
                ClassFile { name: "monk".into(), title: "Monk".into(), schools: vec!["frost".into()], ..Default::default() },
            ];
        });
        let (a, t) = (player(&mut s, 100.0, 100.0, "a"), player(&mut s, 101.0, 100.0, "t"));
        let zap = s.defs.actions.iter().find(|a| a.name == "zap").unwrap().id;
        let hit = |s: &mut Sim| {
            let before = s.ents.attrs[0][t];
            s.tick += 40;
            let id = s.ents.id_of(t);
            s.step(&[crate::sim::PendingAction { slot: a, action: zap, target: id, arg: 0 }]);
            s.ents.attrs[0][t] < before
        };
        s.class[a] = 2;
        assert!(!hit(&mut s), "a monk has no fire");
        s.class[a] = 1;
        assert!(hit(&mut s), "a mage does");
    }

    #[test]
    fn loot_tables_roll_and_rare_drops_are_announced() {
        let mut s = sim_with(|g| {
            g.items.push(proto::game::ItemDef { name: "ember blade".into(), rarity: Rarity::Epic, ..g.items[0].clone() });
            g.loot = vec![LootTable { name: "boss".into(), rolls: 2, nothing: 0, entries: vec![LootEntry { item: "ember blade".into(), weight: 1, min: 1, max: 1 }] }];
            g.archetypes[0].loot = Some("boss".into());
            g.archetypes[0].drops.clear();
        });
        let p = player(&mut s, 100.0, 100.0, "a");
        let b = s.spawn_as(NONE, 0, Some((102.0, 100.0))).unwrap();
        s.ents.x[b] = 102.0;
        s.ents.y[b] = 100.0;
        s.roll_loot(b);
        let blade = s.items.iter().position(|i| i.name == "ember blade").unwrap();
        assert_eq!(s.ground.iter().filter(|g| g.item as usize == blade).count(), 2, "two rolls, both the blade");
        assert!(s.tells.iter().any(|(who, t)| *who == p && t.contains("epic ember blade")), "{:?}", s.tells);
    }

    #[test]
    fn plots_keep_strangers_out_but_let_the_guild_in() {
        let mut s = sim_with(|g| g.plots = Some(PlotRules { size: 8, per_player: 1, cost: 0, sizes: vec![] }));
        let (owner, mate, stranger) = (player(&mut s, 100.0, 100.0, "own"), player(&mut s, 100.0, 100.0, "mate"), player(&mut s, 100.0, 100.0, "far"));
        let mut clients = Clients::default();
        claim(&mut s, &mut clients, owner);
        assert_eq!(s.rs.plots.len(), 1);
        let (cx, cy) = here(&s, owner);
        assert!(s.may_build(owner, cx, cy) && !s.may_build(mate, cx, cy) && !s.may_build(stranger, cx, cy));
        s.rs.guilds.push(Guild { name: "Hearth".into(), leader: "own".into(), members: vec!["own".into(), "mate".into()] });
        assert!(s.may_build(mate, cx, cy) && !s.may_build(stranger, cx, cy));
        assert!(s.may_build(stranger, cx + 20, cy), "outside the plot anyone may");
        claim(&mut s, &mut clients, stranger);
        assert_eq!(s.rs.plots.len(), 1, "overlapping land is refused");
        // The build action itself refuses the stranger (a cell beside them: nobody builds where someone stands).
        let id = build_target(cx + 2, cy);
        let act = |s: &mut Sim, who: usize, a: u8| {
            s.tick += 40;
            let before = s.blocks.total;
            s.step(&[crate::sim::PendingAction { slot: who, action: a, target: id, arg: build_arg(0, 0) }]);
            s.blocks.total != before
        };
        assert!(!act(&mut s, stranger, ACT_BUILD));
        assert!(act(&mut s, owner, ACT_BUILD));
        assert!(!act(&mut s, stranger, ACT_BREAK));
    }

    #[test]
    fn players_are_told_the_land_near_them_when_it_changes() {
        let mut s = sim_with(|g| g.plots = Some(PlotRules { size: 8, per_player: 1, cost: 0, sizes: vec![] }));
        let (owner, near, far) = (player(&mut s, 100.0, 100.0, "own"), player(&mut s, 150.0, 100.0, "near"), player(&mut s, 480.0, 480.0, "far"));
        let mut clients = Clients::default();
        claim(&mut s, &mut clients, owner);
        s.system_land();
        let told = |s: &Sim, p: usize| s.notes.iter().filter(|n| n.0 == p && n.2 == proto::chat::channel::LAND).map(|n| n.3.clone()).collect::<Vec<_>>();
        let mine = told(&s, owner);
        assert_eq!(mine.len(), 1);
        assert!(mine[0].starts_with("L0|") && mine[0].ends_with(",1,0,"), "{mine:?}");
        assert!(told(&s, near)[0].ends_with(",0,0,"), "someone else's");
        assert!(told(&s, far).is_empty(), "too far to be told");
        s.notes.clear();
        s.system_land();
        assert!(s.notes.is_empty(), "nothing new, nothing sent");
        // A crop growing on it: the owner hears when it will be ripe, once, not every second as the time runs down.
        let (cx, cy) = here(&s, owner);
        let ripens = s.tick + 30 * s.tick_hz;
        s.rs.crops.insert((proto::world::block_key(cx, cy), 0), ("oats".into(), ripens));
        s.system_land();
        assert!(told(&s, owner)[0].ends_with(",1,30,"), "{:?}", told(&s, owner));
        s.notes.clear();
        s.tick += s.tick_hz;
        s.system_land();
        assert!(s.notes.is_empty(), "the clock running down is not news");
        s.rs.crops.clear();
        unclaim(&mut s, &mut clients, owner);
        s.system_land();
        assert_eq!(told(&s, owner), vec!["L0|".to_string()], "the land gone");
    }

    #[test]
    fn land_costs_coin_grows_bigger_takes_a_name_and_greets_whoever_walks_in() {
        let mut s = sim_with(|g| {
            g.items.push(proto::game::ItemDef { name: "silver".into(), uses: vec![], stack_max: 50, ..g.items[0].clone() });
            g.currency = Some("silver".into());
            g.plots = Some(PlotRules { size: 8, per_player: 1, cost: 5, sizes: vec![proto::realm::PlotSize { size: 16, cost: 10, level: 0 }] });
        });
        let silver = s.items.iter().position(|i| i.name == "silver").unwrap();
        let coin = |s: &Sim, p: usize| s.inv[p].iter().filter(|t| t.0 as usize == silver + 1).map(|t| t.1 as u32).sum::<u32>();
        let (owner, walker) = (player(&mut s, 100.0, 100.0, "own"), player(&mut s, 400.0, 400.0, "walk"));
        let mut clients = Clients::default();
        claim(&mut s, &mut clients, owner);
        assert!(s.rs.plots.is_empty(), "no coin, no land");
        s.give(owner, silver, 20);
        claim(&mut s, &mut clients, owner);
        assert_eq!((s.rs.plots.len(), coin(&s, owner)), (1, 15));
        plot(&mut s, &mut clients, owner, "grow");
        let a = s.rs.plots[0].area;
        assert_eq!((a.x1 - a.x0, coin(&s, owner)), (16, 5), "the next size up, paid for");
        plot(&mut s, &mut clients, owner, "grow");
        assert_eq!((s.rs.plots[0].area, coin(&s, owner)), (a, 5), "no bigger size: nothing changes");
        plot(&mut s, &mut clients, owner, "name The Old Mill");
        assert_eq!(s.rs.plots[0].name, "The Old Mill");
        s.system_plot_names();
        assert!(s.tells.iter().any(|(who, t)| *who == owner && t == "You walk into The Old Mill."));
        assert!(!s.tells.iter().any(|(who, _)| *who == walker));
        s.ents.x[walker] = 100.0;
        s.ents.y[walker] = 100.0;
        s.tells.clear();
        s.system_plot_names();
        s.system_plot_names();
        assert_eq!(s.tells.iter().filter(|(who, _)| *who == walker).count(), 1, "told once, on the way in");
        assert_eq!((compass(1.0, 0.0), compass(0.0, -1.0), compass(-1.0, 1.0)), ("east", "north", "south-west"));
    }

    #[test]
    fn seeds_grow_into_crops_that_harvest_and_survive_a_restart() {
        let mut s = sim_with(|g| {
            g.items.push(proto::game::ItemDef { name: "carrot seed".into(), uses: vec![], ..g.items[0].clone() });
            g.items.push(proto::game::ItemDef { name: "carrot".into(), uses: vec![], stack_max: 20, ..g.items[0].clone() });
            g.crops = vec![CropDef { name: "carrot".into(), seed: "carrot seed".into(), yields: "carrot".into(), min: 3, max: 3, grow_secs: 2.0, skill: None }];
        });
        let p = player(&mut s, 100.0, 100.0, "farmer");
        let (seed, carrot) = (s.items.iter().position(|i| i.name == "carrot seed").unwrap(), s.items.iter().position(|i| i.name == "carrot").unwrap());
        let (cx, cy) = (51u32, 50u32);
        let act = |s: &mut Sim, a: u8, z: i8, kind: u8| {
            s.tick += 40;
            s.step(&[crate::sim::PendingAction { slot: p, action: a, target: build_target(cx, cy), arg: build_arg(z, kind) }]);
        };
        act(&mut s, ACT_BUILD, 0, SOIL);
        act(&mut s, ACT_BUILD, 1, SPROUT);
        assert_eq!(s.blocks.total, 1, "nobody builds a sprout by hand");
        assert!(!s.plant(p, carrot), "not a seed");
        assert!(s.plant(p, seed));
        let key = block_key(cx, cy);
        let kind = |s: &Sim| s.blocks.get(key).map(|c| c.kinds[1]);
        assert_eq!(kind(&s), Some(SPROUT + 1));
        // A restart halfway: it carries on growing from where it was.
        s.tick += 20;
        let saved = s.realm_saved();
        assert!((saved.crops[0].secs_left - 1.0).abs() < 0.1, "{saved:?}");
        s.rs.crops.clear();
        s.realm_restore(saved);
        s.tick += 25;
        s.system_realm();
        assert_eq!(kind(&s), Some(RIPE + 1), "ripe");
        act(&mut s, ACT_BREAK, 1, 0);
        let have: u32 = s.inv[p].iter().filter(|b| b.0 as usize == carrot + 1).map(|b| b.1 as u32).sum();
        assert_eq!(have, 3, "the harvest");
        assert!(s.rs.crops.is_empty());
    }

    #[test]
    fn crafting_takes_the_ingredients_and_gives_the_make() {
        let mut s = sim_with(|g| {
            g.recipes = vec![Recipe { name: "stim".into(), needs: vec![Stack { item: "medkit".into(), count: 2 }, Stack { item: "battery".into(), count: 1 }], makes: "stim".into(), count: 1, near: None, skill: None, level: 0 }];
        });
        let p = player(&mut s, 100.0, 100.0, "a");
        let ix = |s: &Sim, n: &str| s.items.iter().position(|i| i.name == n).unwrap();
        let (medkit, battery, stim) = (ix(&s, "medkit"), ix(&s, "battery"), ix(&s, "stim"));
        let mut clients = Clients::default();
        s.give(p, medkit, 2);
        craft(&mut s, &mut clients, p, "stim");
        assert_eq!(s.inv[p].iter().filter(|b| b.0 as usize == stim + 1).count(), 0, "missing the battery");
        s.give(p, battery, 1);
        craft(&mut s, &mut clients, p, "Stim");
        assert_eq!(s.inv[p].iter().filter(|b| b.0 as usize == stim + 1).count(), 1);
        assert!(s.inv[p].iter().all(|b| b.0 as usize != medkit + 1 && b.0 as usize != battery + 1));
    }

    #[test]
    fn a_recipe_needs_its_skill_and_teaches_it() {
        let mut s = sim_with(|g| {
            g.skills = vec![SkillDef { name: "medicine".into(), title: "Medicine".into(), max: 100 }];
            g.recipes = vec![
                Recipe { name: "stim".into(), needs: vec![Stack { item: "medkit".into(), count: 1 }], makes: "stim".into(), count: 1, near: None, skill: Some("medicine".into()), level: 0 },
                Recipe { name: "big stim".into(), needs: vec![Stack { item: "medkit".into(), count: 1 }], makes: "stim".into(), count: 1, near: None, skill: Some("medicine".into()), level: 2 },
            ];
        });
        let p = player(&mut s, 100.0, 100.0, "a");
        let medkit = s.items.iter().position(|i| i.name == "medkit").unwrap();
        let mut clients = Clients::default();
        s.give(p, medkit, 5);
        craft(&mut s, &mut clients, p, "big stim");
        assert_eq!(s.skill_level(p, 0), 0, "too unskilled to try");
        craft(&mut s, &mut clients, p, "stim");
        craft(&mut s, &mut clients, p, "stim");
        assert_eq!(s.skill_level(p, 0), 2, "each make teaches");
        craft(&mut s, &mut clients, p, "big stim");
        assert_eq!(s.skill_level(p, 0), 3, "now it can be made");
    }

    #[test]
    fn guilds_form_and_share_the_save() {
        let mut s = sim_with(|_| {});
        let (a, b) = (player(&mut s, 100.0, 100.0, "aa"), player(&mut s, 100.0, 100.0, "bb"));
        s.set_name(b, "Bea", true);
        let mut clients = Clients::default();
        guild(&mut s, &mut clients, a, "create Hearth");
        guild(&mut s, &mut clients, a, "invite Bea");
        guild(&mut s, &mut clients, b, "accept");
        assert_eq!(s.rs.guilds[0].members, vec!["aa".to_string(), "bb".to_string()]);
        let saved = s.realm_saved();
        guild(&mut s, &mut clients, a, "leave");
        assert_eq!(s.rs.guilds[0].leader, "bb", "the guild passes on");
        guild(&mut s, &mut clients, b, "leave");
        assert!(s.rs.guilds.is_empty(), "the last one out ends it");
        s.realm_restore(saved);
        assert_eq!(s.rs.guilds[0].name, "Hearth");
    }

    #[test]
    fn a_dungeon_gives_each_party_its_own_monsters() {
        let mut s = sim_with(|g| {
            g.places.push(proto::game::Place { name: "crypt".into(), x: 300.0, y: 300.0, r: 20.0, title: "The Crypt".into(), exit: None, sound: String::new(), music: String::new(), safe: false });
            g.dungeons = vec![DungeonDef { name: "The Crypt".into(), place: "crypt".into(), bots: vec![DungeonBot { bot: g.archetypes[0].name.clone(), count: 3 }], xp: 0, reward: Some(Stack { item: "medkit".into(), count: 1 }), group: 3 }];
        });
        let (a, b) = (player(&mut s, 300.0, 300.0, "a"), player(&mut s, 305.0, 300.0, "b"));
        s.system_realm();
        assert_eq!(s.rs.runs.len(), 2, "two solo players, two copies");
        assert_ne!(s.rs.phase[a], s.rs.phase[b]);
        let theirs: Vec<usize> = s.rs.runs[1].bots.iter().filter_map(|&id| s.ents.resolve(id)).collect();
        assert_eq!(theirs.len(), 3);
        assert!(theirs.iter().all(|&m| !s.same_phase(a, m) && s.same_phase(b, m)), "a cannot meet b's monsters");
        assert!(s.same_phase(a, b), "players still see each other");
        // Clearing it rewards whoever is inside.
        for id in s.rs.runs[0].bots.clone() {
            if let Some(m) = s.ents.resolve(id) {
                s.ents.despawn(m);
            }
        }
        s.system_realm();
        let medkit = s.items.iter().position(|i| i.name == "medkit").unwrap();
        assert!(s.inv[a].iter().any(|b| b.0 as usize == medkit + 1));
        assert!(s.tells.iter().any(|(w, t)| *w == a && t.contains("cleared")));
        // Leaving puts you back in the shared world, and an empty run closes after a while.
        s.ents.x[a] = 100.0;
        s.system_realm();
        assert_eq!(s.rs.phase[a], 0);
        s.ents.x[b] = 100.0;
        s.tick += proto::rules::Rules::ticks(s.rules.dungeon_idle_secs, s.tick_hz) + 1;
        s.system_realm();
        assert!(s.rs.runs.is_empty());
        assert!(theirs.iter().all(|&m| !s.ents.alive[m]), "its monsters went with it");
    }
}

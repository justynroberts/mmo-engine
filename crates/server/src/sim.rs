//! The simulation: SoA entity storage, spatial grid, and the systems that run
//! each tick. Nothing here knows about sockets or clients. Health is just
//! attribute 0, and "combat" is just the data-driven effects on an action.

use proto::defs::{Defs, Effect, OnZero};
use proto::game::{ArchRt, Behaviour, Game};
use proto::world::{integrate, qpos, World};
use proto::{Entry, Rng};

/// "No owner": the entity is an NPC rather than a player.
pub const NONE: u32 = u32::MAX;
/// Owned by the admin UI ("possessed"): the NPC AI leaves it alone.
pub const DRIVEN: u32 = u32::MAX - 1;
/// A mirror of an entity that lives in a neighbouring zone: it is seen and nothing more.
pub const GHOST: u32 = u32::MAX - 2;
/// Archetype of entities that are not bots (players, plain NPCs).
pub const NO_ARCH: u8 = u8::MAX;
pub const SLOT_MASK: u32 = 0x00FF_FFFF;

pub struct Entities {
    pub alive: Vec<bool>,
    pub gen: Vec<u8>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub dir_x: Vec<i8>,
    pub dir_y: Vec<i8>,
    /// The short pause after any action (at most `GCD_SECS`), before the next may be used.
    pub cooldown_until: Vec<u32>,
    /// Each action's own cooldown: the tick it may be used again, at `slot * MAX_ACTIONS + action index`.
    pub ready_at: Vec<u32>,
    /// Owning client index, or `NONE`.
    pub owner: Vec<u32>,
    /// Archetype index, or `NO_ARCH`.
    pub arch: Vec<u8>,
    /// Units per second at full input.
    pub speed: Vec<f32>,
    /// Patrol progress: next waypoint index and direction.
    pub wp: Vec<u8>,
    pub wp_fwd: Vec<bool>,
    /// Tick before which this bot will not speak again.
    pub say_at: Vec<u32>,
    /// God mode (admin only): attributes held at their maximum every tick, so it cannot die or run dry.
    pub god: Vec<bool>,
    /// Ticks of the current jump still to come (0 = on the ground).
    pub air: Vec<u16>,
    /// Held by a spell (`proto::game::HoldKind` as u8, 0 for free) until this tick.
    pub held: Vec<u8>,
    pub held_until: Vec<u32>,
    /// Levitating (a spell's `lift`) until this tick: the jump stays at its height till then.
    pub lift_until: Vec<u32>,
    /// Experience and level (`GameFile::levels`); a bot's level is its type's.
    pub xp: Vec<u32>,
    pub level: Vec<u8>,
    /// Players who hurt it lately: (player id, tick), newest last, for sharing the experience when it falls.
    pub hurt_by: Vec<Vec<(u32, u32)>>,
    /// The named places a player has found (by place name).
    pub found: Vec<Vec<String>>,
    /// A player's quests, and the one a giver has just put to them (an index, or `wire::NO_OFFER`).
    pub quests: Vec<Vec<crate::quests::QuestRun>>,
    pub offer: Vec<u16>,
    /// The block layer the feet are in (`proto::world::GROUND` on the bare ground).
    pub z: Vec<i8>,
    /// The account a player builds as (`Blocks::owner`), 0 for none.
    pub account: Vec<u16>,
    /// One array per attribute, indexed by slot (structure of arrays).
    pub attrs: Vec<Vec<f32>>,
    /// A player's display name (`proto::names`); None for bots, which go by their type.
    pub name: Vec<Option<std::sync::Arc<str>>>,
    /// Whether the player chose that name (saved and kept unique) or was given one (not saved).
    pub name_claimed: Vec<bool>,
    /// Raised by an event (`director`): killed, it is gone instead of respawning, and it is never saved.
    pub fleeting: Vec<bool>,
    /// Slots whose name may have changed since the web gateway last looked (spawned, removed, renamed).
    pub renamed: Vec<u32>,
    free: Vec<u32>,
    pub high: usize,
    pub live: usize,
    /// Bots alive (entities spawned with an archetype), kept below `cap - player_reserve` so bots never take
    /// the slots that players need to join.
    pub bots: usize,
    counted: Vec<bool>,
}

impl Entities {
    pub fn new(cap: usize, n_attrs: usize) -> Self {
        assert!(cap <= SLOT_MASK as usize);
        Entities {
            alive: vec![false; cap],
            gen: vec![0; cap],
            x: vec![0.0; cap],
            y: vec![0.0; cap],
            dir_x: vec![0; cap],
            dir_y: vec![0; cap],
            cooldown_until: vec![0; cap],
            ready_at: vec![0; cap * proto::game::MAX_ACTIONS],
            god: vec![false; cap],
            counted: vec![false; cap],
            bots: 0,
            air: vec![0; cap],
            held: vec![0; cap],
            held_until: vec![0; cap],
            lift_until: vec![0; cap],
            xp: vec![0; cap],
            level: vec![1; cap],
            hurt_by: vec![Vec::new(); cap],
            found: vec![Vec::new(); cap],
            quests: vec![Vec::new(); cap],
            offer: vec![proto::wire::NO_OFFER; cap],
            z: vec![proto::world::GROUND; cap],
            account: vec![0; cap],
            owner: vec![NONE; cap],
            arch: vec![NO_ARCH; cap],
            speed: vec![0.0; cap],
            wp: vec![0; cap],
            wp_fwd: vec![true; cap],
            say_at: vec![0; cap],
            attrs: (0..n_attrs).map(|_| vec![0.0; cap]).collect(),
            name: vec![None; cap],
            name_claimed: vec![false; cap],
            fleeting: vec![false; cap],
            renamed: Vec::new(),
            free: Vec::new(),
            // Slot 0 is never used, so entity id 0 always means the server (chat from 0 is the server speaking).
            high: 1,
            live: 0,
        }
    }

    pub fn cap(&self) -> usize {
        self.alive.len()
    }

    /// Stable wire id: slot in the low 24 bits, generation in the high 8, so a
    /// recycled slot is never mistaken for the entity that used to live there.
    #[inline]
    pub fn id_of(&self, slot: usize) -> u32 {
        slot as u32 | (self.gen[slot] as u32) << 24
    }

    #[inline]
    pub fn resolve(&self, id: u32) -> Option<usize> {
        let slot = (id & SLOT_MASK) as usize;
        if slot < self.high && self.alive[slot] && self.gen[slot] == (id >> 24) as u8 {
            Some(slot)
        } else {
            None
        }
    }

    fn alloc(&mut self) -> Option<usize> {
        if let Some(s) = self.free.pop() {
            return Some(s as usize);
        }
        if self.high < self.cap() {
            self.high += 1;
            return Some(self.high - 1);
        }
        None
    }

    pub fn despawn(&mut self, slot: usize) {
        if self.alive[slot] {
            if self.counted[slot] {
                self.counted[slot] = false;
                self.bots -= 1;
            }
            self.alive[slot] = false;
            self.gen[slot] = self.gen[slot].wrapping_add(1);
            self.owner[slot] = NONE;
            self.account[slot] = 0;
            self.name[slot] = None;
            self.name_claimed[slot] = false;
            self.fleeting[slot] = false;
            self.renamed.push(slot as u32);
            self.free.push(slot as u32);
            self.live -= 1;
        }
    }
}

/// Unit direction toward an offset as input bytes; (0, 0) inside `stop` units.
fn dir_to(dx: f32, dy: f32, stop: f32) -> (i8, i8) {
    let d2 = dx * dx + dy * dy;
    if d2 <= stop * stop || d2 == 0.0 {
        return (0, 0);
    }
    let d = d2.sqrt();
    let q = |v: f32| (v * 127.0).round().clamp(-127.0, 127.0) as i8;
    (q(dx / d), q(dy / d))
}

/// Uniform grid rebuilt every tick with a counting sort: no allocation, no
/// hashing, and a neighbour query is a 3x3 walk over contiguous slices.
pub struct Grid {
    cell: f32,
    gw: usize,
    start: Vec<u32>,
    cursor: Vec<u32>,
    items: Vec<u32>,
    cell_of: Vec<u32>,
}

impl Grid {
    pub fn new(world_size: f32, cell: f32, cap: usize) -> Self {
        // A cell only has to be at least the interest radius; a huge world with a small radius would otherwise
        // want hundreds of millions of cells, so cells grow to keep the table near a million.
        let cell = cell.max(world_size / 1024.0);
        let gw = (world_size / cell).ceil() as usize + 1;
        Grid {
            cell,
            gw,
            start: vec![0; gw * gw + 1],
            cursor: vec![0; gw * gw],
            items: vec![0; cap],
            cell_of: vec![0; cap],
        }
    }

    #[inline]
    fn cell_index(&self, x: f32, y: f32) -> usize {
        let cx = ((x / self.cell) as usize).min(self.gw - 1);
        let cy = ((y / self.cell) as usize).min(self.gw - 1);
        cy * self.gw + cx
    }

    pub fn rebuild(&mut self, e: &Entities) {
        self.start.fill(0);
        for s in 0..e.high {
            if e.alive[s] {
                let c = self.cell_index(e.x[s], e.y[s]);
                self.cell_of[s] = c as u32;
                self.start[c + 1] += 1;
            }
        }
        for i in 0..self.gw * self.gw {
            self.start[i + 1] += self.start[i];
        }
        self.cursor.copy_from_slice(&self.start[..self.gw * self.gw]);
        for s in 0..e.high {
            if e.alive[s] {
                let c = self.cell_of[s] as usize;
                self.items[self.cursor[c] as usize] = s as u32;
                self.cursor[c] += 1;
            }
        }
    }

    /// Calls `f(slot)` for every entity in the 3x3 cells around `(x, y)`.
    /// The caller does the exact distance test. `cell` must be >= the query radius.
    #[inline]
    pub fn for_neighbours(&self, x: f32, y: f32, mut f: impl FnMut(u32)) {
        let cx = ((x / self.cell) as usize).min(self.gw - 1);
        let cy = ((y / self.cell) as usize).min(self.gw - 1);
        let y0 = cy.saturating_sub(1);
        let y1 = (cy + 1).min(self.gw - 1);
        let x0 = cx.saturating_sub(1);
        let x1 = (cx + 1).min(self.gw - 1);
        for gy in y0..=y1 {
            let row = gy * self.gw;
            let a = self.start[row + x0] as usize;
            let b = self.start[row + x1 + 1] as usize;
            for &s in &self.items[a..b] {
                f(s);
            }
        }
    }
}

/// Part of an effect still to come: `per_tick` added to `attr` of entity `id` for `left` more ticks.
struct OverTime {
    id: u32,
    attr: usize,
    per_tick: f32,
    left: u32,
    /// Who cast it (an entity id), for the credit when it finishes something off.
    src: u32,
}

/// A temporary effect to take back (`EffectFile::lasts`): add `delta` to `attr` of entity `id` at tick `at`.
struct Undo {
    id: u32,
    attr: usize,
    delta: f32,
    at: u32,
}

/// How many ticks a cast is repeated to nearby clients (a dropped packet should not lose a fireball).
pub const CAST_TICKS: u32 = 3;

/// The longest pause any action puts before the next one (a shared "global cooldown"); an action's own cooldown,
/// often longer, holds back only that action.
pub const GCD_SECS: f32 = 0.5;

pub struct PendingAction {
    pub slot: usize,
    pub action: u8,
    pub target: u32,
    /// The input's extra argument (a build's or break's layer and material, `proto::game::build_arg`).
    pub arg: u16,
}

#[derive(Default, Clone, Copy)]
pub struct SimStats {
    pub zaps: u64,
    pub respawns: u64,
    pub rejected_actions: u64,
    pub jumps: u64,
    /// Actions that missed because the target was in the air.
    pub dodges: u64,
    /// Blocks built and broken.
    pub builds: u64,
    pub breaks: u64,
}

pub struct Sim {
    pub defs: Defs,
    pub world: World,
    pub ents: Entities,
    pub grid: Grid,
    pub tick: u32,
    pub tick_hz: u32,
    pub dt: f32,
    pub rng: Rng,
    pub stats: SimStats,
    /// Wire entry for every live slot, rebuilt once per tick. Snapshots copy
    /// from here, so quantisation cost is O(entities), not O(clients x visible).
    pub entry_cache: Vec<Entry>,
    pub archs: Vec<ArchRt>,
    /// Every name may have changed (bot types edited): the web gateway republishes them all.
    pub names_all: bool,
    pub places: Vec<proto::game::Place>,
    pub routes: Vec<proto::game::Route>,
    /// When it is light and dark; clients draw it, and at night bots see unlit players less far.
    pub daylight: proto::daylight::Daylight,
    /// Whether it is night now (worked out from `daylight` each tick).
    pub night: bool,
    /// Players may harm each other outside safe places (`GameFile::pvp`).
    pub pvp: bool,
    /// The `lit` attribute, when the game has one (sneaking).
    lit_attr: Option<usize>,
    /// Lines bots want to say this tick: (slot, shout, text). Drained by the network layer.
    pub npc_say: Vec<(usize, bool, String)>,
    /// Actions requested by bots this tick.
    npc_actions: Vec<PendingAction>,
    /// Effects spread over time (burns, curses, slow heals) still running.
    over_time: Vec<OverTime>,
    undo: Vec<Undo>,
    /// The `speed` attribute, when the game has one: it multiplies movement.
    speed_attr: Option<usize>,
    /// Experience and levels, when the game has them.
    pub levels: Option<proto::game::LevelsFile>,
    /// Lines for one player only (a level reached, a place found): (slot, text). Drained by the network layer.
    pub tells: Vec<(usize, String)>,
    /// Bumped when a player's level or experience changes, so the network layer sends it (`wire::kind::PROGRESS`).
    pub progress_ver: Vec<u32>,
    /// The game's quests, and a version per player bumped when theirs change (`wire::kind::QUESTS`).
    pub quests: Vec<proto::game::QuestFile>,
    pub quest_ver: Vec<u32>,
    /// Parties (`social.rs`): each player's party (0 for none), an invitation waiting (the inviter's id), who last
    /// whispered to them, and a version bumped when their party changes (`wire::kind::PARTY`).
    pub party: Vec<u32>,
    pub invite: Vec<u32>,
    pub last_whisper: Vec<u32>,
    pub party_ver: Vec<u32>,
    pub next_party: u32,
    /// Money, gear and mounts (`trade.rs`): the money item, what each player wears (item index + 1 per slot), how
    /// much a mount adds to their speed while they ride (0 on foot), and a version bumped when either changes.
    pub currency: Option<usize>,
    pub gear: Vec<[u8; proto::game::GEAR_SLOTS]>,
    pub mounted: Vec<f32>,
    pub gear_ver: Vec<u32>,
    /// Encounters (`encounter.rs`): the player a bot is locked in a fight with (an id), the tick a blow last
    /// passed between them, the bots locked on each player (ids), and a version bumped when those change
    /// (`wire::kind::ENCOUNTER`).
    pub foe: Vec<Option<u32>>,
    pub foe_at: Vec<u32>,
    pub pursuers: Vec<Vec<u32>>,
    /// The players each player is in a fight with (pvp): (their id, the tick of the last blow between them).
    pub rivals: Vec<Vec<(u32, u32)>>,
    pub pursuer_ver: Vec<u32>,
    /// Actions used in the last few ticks, newest last, for the network layer to tell clients near them
    /// (`proto::wire::kind::CASTS`): (tick, caster slot, cast).
    pub casts: Vec<(u32, usize, proto::wire::Cast)>,
    cast_seq: u32,
    /// Actions injected by the admin UI for the next tick.
    pub admin_actions: Vec<PendingAction>,
    /// This process's strip of the world (x range) when running as one zone of several: bots stay inside it.
    pub zone: Option<(f32, f32)>,
    /// How far bots see right now, as a multiple of what their type says (set by the weather).
    pub sight_mult: f32,
    /// What clients are told about the current weather, if any.
    pub weather: Option<WeatherView>,
    /// The weather zones that are on (the director keeps this).
    pub zones: Vec<ZoneView>,
    /// Item kinds (from the game file) and who carries what, indexed by entity slot.
    pub items: Vec<proto::game::ItemRt>,
    pub inv: Vec<crate::items::Bag>,
    /// Bumped whenever a bag changes, so the network layer knows to resend it.
    pub inv_ver: Vec<u32>,
    pub item_cd: Vec<u32>,
    /// Slots bots may not take, so players can always join (the server's --max-clients).
    pub player_reserve: usize,
    pub ground: Vec<crate::items::GroundItem>,
    pub next_ground: u32,
    /// What players have built (mirrored into `world` as solid columns).
    pub blocks: crate::blocks::Blocks,
    /// The ground's heights, which say which layer a new block column starts in.
    pub terrain: crate::worldfile::Terrain,
    /// Walkability grid and flow fields toward places and route points (see `nav.rs`).
    pub nav: crate::nav::Nav,
    /// Per slot: the static point a bot last decided to walk to and how near counts as there (negative for
    /// none), so between decisions it can keep following the flow field round corners.
    nav_goal: Vec<(f32, f32, f32)>,
}

/// The part of the weather that clients draw.
#[derive(Clone, Debug, PartialEq)]
pub struct WeatherView {
    pub name: String,
    pub color: String,
    pub intensity: f32,
}

/// A weather zone that is on: where it is, what clients draw there, and how far bots inside it see.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ZoneView {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub r: f32,
    pub height: f32,
    pub weather: String,
    pub color: String,
    pub intensity: f32,
    #[serde(skip)]
    pub sight_mult: f32,
}

/// How far a bot sees entity `t`, as a share of its sight: an unlit player at night is hard to see.
#[inline]
fn seen_at(e: &Entities, lit: usize, t: usize) -> f32 {
    if e.owner[t] < DRIVEN && e.attrs[lit][t] < 0.5 {
        proto::game::SNEAK_SIGHT
    } else {
        1.0
    }
}

impl Sim {
    /// The `lit` attribute when sneaking is in play right now (a game that has one, at night).
    fn sneak(&self) -> Option<usize> {
        self.lit_attr.filter(|_| self.night)
    }

    /// How far bots see at (x, y), as a multiple of what their type says: inside a weather zone its weather decides
    /// (the most blinding, where zones overlap), elsewhere the weather everywhere does.
    pub fn sight_at(&self, x: f32, y: f32) -> f32 {
        let mut inside: Option<f32> = None;
        for z in &self.zones {
            if (x - z.x).powi(2) + (y - z.y).powi(2) <= z.r * z.r {
                inside = Some(inside.map_or(z.sight_mult, |m| m.min(z.sight_mult)));
            }
        }
        inside.unwrap_or(self.sight_mult)
    }

    pub fn new(game: Game, world: World, cap: usize, tick_hz: u32, seed: u64) -> Self {
        let Game { defs, archs, places, routes, items, daylight, pvp, levels, quests, currency } = game;
        let mut world = world;
        world.road_speed = defs.road_speed;
        let ents = Entities::new(cap, defs.attrs.len());
        let grid = Grid::new(world.size, defs.interest_radius, cap);
        Sim {
            defs,
            world,
            ents,
            grid,
            tick: 0,
            tick_hz,
            dt: 1.0 / tick_hz as f32,
            rng: Rng::new(seed ^ 0x5EED),
            stats: SimStats::default(),
            entry_cache: vec![Entry::default(); cap],
            archs,
            names_all: true,
            places,
            routes,
            daylight,
            night: false,
            pvp,
            lit_attr: None,
            npc_say: Vec::new(),
            npc_actions: Vec::new(),
            over_time: Vec::new(),
            undo: Vec::new(),
            speed_attr: None,
            levels,
            tells: Vec::new(),
            progress_ver: vec![0; cap],
            quests,
            quest_ver: vec![0; cap],
            party: vec![0; cap],
            invite: vec![0; cap],
            last_whisper: vec![0; cap],
            party_ver: vec![0; cap],
            next_party: 0,
            currency,
            gear: vec![[0; proto::game::GEAR_SLOTS]; cap],
            mounted: vec![0.0; cap],
            gear_ver: vec![0; cap],
            foe: vec![None; cap],
            foe_at: vec![0; cap],
            pursuers: vec![Vec::new(); cap],
            rivals: vec![Vec::new(); cap],
            pursuer_ver: vec![0; cap],
            casts: Vec::new(),
            cast_seq: 0,
            admin_actions: Vec::new(),
            zone: None,
            sight_mult: 1.0,
            weather: None,
            zones: Vec::new(),
            items,
            inv: vec![[(0, 0); proto::game::INV_SLOTS]; cap],
            inv_ver: vec![0; cap],
            item_cd: vec![0; cap],
            player_reserve: 0,
            ground: Vec::new(),
            next_ground: 1,
            blocks: Default::default(),
            terrain: Default::default(),
            nav: Default::default(),
            nav_goal: vec![(0.0, 0.0, -1.0); cap],
        }
    }

    pub fn spawn(&mut self, owner: u32) -> Option<usize> {
        self.spawn_as(owner, NO_ARCH, None)
    }

    /// Spawn an entity of an archetype (or a plain one with `NO_ARCH`), at a
    /// given spot if it is free, otherwise at a random free spot.
    pub fn spawn_as(&mut self, owner: u32, arch: u8, at: Option<(f32, f32)>) -> Option<usize> {
        let r = self.defs.entity_radius;
        if arch != NO_ARCH && self.ents.bots + self.player_reserve >= self.ents.cap() {
            return None; // the rest of the table is kept for players
        }
        let slot = self.ents.alloc()?;
        if arch != NO_ARCH {
            self.ents.counted[slot] = true;
            self.ents.bots += 1;
        }
        let home = if owner == NONE { self.archs.get(arch as usize).and_then(|a| a.home) } else { None };
        let (mut x, mut y) = match at {
            Some((x, y)) if !self.world.blocked_by_walls(x, y, r) => (x, y),
            _ => self.spawn_point(home, r),
        };
        if let (Some((lo, hi)), true) = (self.zone, owner == NONE) {
            // A zone's bots live in its strip: look for a spot there, else take the nearest edge.
            for _ in 0..64 {
                if x >= lo + r && x <= hi - r {
                    break;
                }
                (x, y) = self.spawn_point(home, r);
            }
            x = x.clamp(lo + r, (hi - r).max(lo + r));
            if self.world.blocked(x, y, r) {
                (x, y) = self.world.random_spawn(&mut self.rng, r);
                x = x.clamp(lo + r, (hi - r).max(lo + r));
            }
        }
        let speed = self.defs.move_speed * self.archs.get(arch as usize).map_or(1.0, |a| a.speed_mult);
        let e = &mut self.ents;
        e.alive[slot] = true;
        e.x[slot] = x;
        e.y[slot] = y;
        e.z[slot] = self.world.settle(x, y, r, proto::world::GROUND);
        e.dir_x[slot] = 0;
        e.dir_y[slot] = 0;
        e.cooldown_until[slot] = 0;
        e.ready_at[slot * proto::game::MAX_ACTIONS..(slot + 1) * proto::game::MAX_ACTIONS].fill(0);
        e.god[slot] = false;
        e.air[slot] = 0;
        e.held[slot] = 0;
        e.lift_until[slot] = 0;
        e.xp[slot] = 0;
        e.level[slot] = self.archs.get(arch as usize).map_or(1, |a| a.level.max(1));
        e.hurt_by[slot].clear();
        e.found[slot].clear();
        e.quests[slot].clear();
        e.offer[slot] = proto::wire::NO_OFFER;
        self.party[slot] = 0;
        self.invite[slot] = 0;
        self.last_whisper[slot] = 0;
        self.party_ver[slot] = self.party_ver[slot].wrapping_add(1);
        self.gear[slot] = [0; proto::game::GEAR_SLOTS];
        self.mounted[slot] = 0.0;
        self.gear_ver[slot] = self.gear_ver[slot].wrapping_add(1);
        self.foe[slot] = None;
        self.pursuers[slot].clear();
        self.rivals[slot].clear();
        self.pursuer_ver[slot] = self.pursuer_ver[slot].wrapping_add(1);
        e.owner[slot] = owner;
        e.arch[slot] = arch;
        e.speed[slot] = speed;
        e.wp[slot] = 0;
        e.wp_fwd[slot] = true;
        e.say_at[slot] = 0;
        e.name[slot] = None;
        e.name_claimed[slot] = false;
        e.renamed.push(slot as u32);
        e.live += 1;
        self.nav_goal[slot].2 = -1.0;
        self.reset_attrs(slot);
        self.clear_inv(slot);
        self.item_cd[slot] = 0;
        Some(slot)
    }

    /// Everything about an entity worth saving.
    pub fn record_of(&self, slot: usize) -> crate::store::Record {
        crate::store::Record {
            x: self.ents.x[slot],
            y: self.ents.y[slot],
            attrs: self.defs.attrs.iter().enumerate().map(|(i, d)| (d.name.clone(), self.ents.attrs[i][slot])).collect(),
            inv: self.inv_names(slot),
            name: if self.ents.name_claimed[slot] { self.ents.name[slot].as_deref().unwrap_or("").to_string() } else { String::new() },
            zone: None,
            xp: self.ents.xp[slot],
            found: self.ents.found[slot].clone(),
            quests: self.ents.quests[slot].iter().map(|r| (r.name.clone(), r.step, r.count, r.state)).collect(),
            gear: self.gear_names(slot),
        }
    }

    /// Give a player a display name: `claimed` for one they chose (saved with them), otherwise one they were given.
    pub fn set_name(&mut self, slot: usize, name: &str, claimed: bool) {
        self.ents.name[slot] = Some(name.into());
        self.ents.name_claimed[slot] = claimed;
        self.ents.renamed.push(slot as u32);
    }

    /// What everyone calls an entity: a player's name, a bot's type, "Traveller" for a guest from another zone.
    pub fn name_of(&self, slot: usize) -> std::sync::Arc<str> {
        if let Some(n) = &self.ents.name[slot] {
            return n.clone();
        }
        match self.archs.get(self.ents.arch[slot] as usize) {
            Some(a) => proto::names::bot(&a.name).into(),
            None if self.ents.owner[slot] == GHOST => "Traveller".into(),
            None => "Guest".into(),
        }
    }

    /// Move an entity straight to (x, y), standing on whatever is there, if it is inside the world and not in a
    /// wall. Returns whether it moved.
    pub fn put(&mut self, slot: usize, x: f32, y: f32) -> bool {
        let r = self.defs.entity_radius;
        let size = self.world.size;
        if !(x.is_finite() && y.is_finite() && x >= r && y >= r && x <= size - r && y <= size - r) {
            return false;
        }
        // Somewhere blocked now (built over, a tree grown there): the nearest free spot, rings outward to 32 units.
        let (x, y) = if self.world.blocked_by_walls(x, y, r) {
            let found = (1..=16).find_map(|ring| {
                let d = ring as f32 * 2.0;
                (0..ring * 8).find_map(|k| {
                    let a = k as f32 / (ring * 8) as f32 * std::f32::consts::TAU;
                    let (px, py) = (x + d * a.cos(), y + d * a.sin());
                    (px >= r && py >= r && px <= size - r && py <= size - r && !self.world.blocked_by_walls(px, py, r)).then_some((px, py))
                })
            });
            match found {
                Some(p) => p,
                None => return false,
            }
        } else {
            (x, y)
        };
        self.ents.x[slot] = x;
        self.ents.y[slot] = y;
        // On top of whatever is there now (it may have been built over while away).
        self.ents.z[slot] = self.world.settle(x, y, r, proto::world::GROUND);
        true
    }

    /// Put a saved character back. Position is kept only if it is still a legal spot
    /// (the world may have changed); attributes are matched by name and clamped to
    /// their current range, so a changed game file never produces an invalid value.
    pub fn restore(&mut self, slot: usize, rec: &crate::store::Record) {
        self.put(slot, rec.x, rec.y);
        for (i, d) in self.defs.attrs.iter().enumerate() {
            if let Some(&v) = rec.attrs.get(&d.name) {
                if v.is_finite() {
                    self.ents.attrs[i][slot] = v.clamp(d.min, d.max);
                }
            }
        }
        if !rec.inv.is_empty() {
            self.restore_inv(slot, &rec.inv);
        }
        if let Ok(n) = proto::names::clean(&rec.name) {
            self.set_name(slot, &n, true);
        }
        self.ents.xp[slot] = rec.xp;
        self.ents.level[slot] = self.levels.as_ref().map_or(1, |l| l.level_of(rec.xp));
        self.ents.found[slot] = rec.found.clone();
        self.progress_ver[slot] = self.progress_ver[slot].wrapping_add(1);
        // Quests the game still has, by name (a step past a shortened quest's end makes it ready to hand in).
        self.ents.quests[slot] = rec
            .quests
            .iter()
            .filter_map(|(name, step, count, state)| {
                let q = self.quests.iter().find(|q| &q.name == name)?;
                let state = if *state == proto::wire::QUEST_ACTIVE && *step as usize >= q.steps.len() { proto::wire::QUEST_READY } else { *state };
                Some(crate::quests::QuestRun { name: name.clone(), step: *step, count: *count, state })
            })
            .collect();
        self.quest_ver[slot] = self.quest_ver[slot].wrapping_add(1);
        self.restore_gear(slot, &rec.gear);
    }

    /// An inert copy of a neighbour zone's entity. It is visible to interest management and nothing else:
    /// not simulated, not targetable, not saved, not counted as a bot.
    pub fn spawn_ghost(&mut self, x: f32, y: f32, arch: u8) -> Option<usize> {
        let slot = self.ents.alloc()?;
        let e = &mut self.ents;
        e.alive[slot] = true;
        e.x[slot] = x;
        e.y[slot] = y;
        e.z[slot] = proto::world::GROUND;
        e.dir_x[slot] = 0;
        e.dir_y[slot] = 0;
        e.owner[slot] = GHOST;
        e.name[slot] = None;
        e.name_claimed[slot] = false;
        e.renamed.push(slot as u32);
        e.arch[slot] = arch;
        e.speed[slot] = 0.0;
        e.live += 1;
        Some(slot)
    }

    /// Entities that are mirrors of a neighbour's.
    pub fn ghost_count(&self) -> usize {
        (0..self.ents.high).filter(|&s| self.ents.alive[s] && self.ents.owner[s] == GHOST).count()
    }

    /// Swap in a new set of walls (and ground) without disconnecting anyone. Anything that is now inside a wall
    /// (or outside the world) is moved to a free spot; standing on blocks is fine. Returns how many entities
    /// were moved.
    pub fn replace_world(&mut self, world: World, terrain: crate::worldfile::Terrain) -> usize {
        self.world = world;
        self.world.road_speed = self.defs.road_speed;
        self.terrain = terrain;
        self.nav.clear();
        // Built blocks stay where they are, except those the new world's walls cover or its ground buries.
        let cols = self.block_texts();
        self.set_blocks(cols);
        let r = self.defs.entity_radius;
        let mut moved = 0;
        for s in 0..self.ents.high {
            if !self.ents.alive[s] {
                continue;
            }
            if self.world.blocked_by_walls(self.ents.x[s], self.ents.y[s], r) {
                let (x, y) = self.world.random_spawn(&mut self.rng, r);
                self.ents.x[s] = x;
                self.ents.y[s] = y;
                moved += 1;
            }
            self.ents.z[s] = self.world.settle(self.ents.x[s], self.ents.y[s], r, self.ents.z[s]);
        }
        moved
    }

    /// The blocks as they are, in the form `set_blocks` takes (so they can be put back after a change).
    pub fn block_texts(&self) -> Vec<crate::blocks::ColText> {
        let tag = |o: u16| (o > 0).then(|| self.blocks.owners.get(o as usize - 1).cloned()).flatten();
        self.blocks
            .cols
            .iter()
            .map(|(&key, c)| crate::blocks::ColText {
                key,
                base: Some(c.base),
                layers: c.kinds.iter().zip(&c.owners).map(|(&k, &o)| (k != 0).then(|| (k - 1, tag(o)))).collect(),
            })
            .collect()
    }

    /// Put these block columns in the world (after a restart, a world change or a reset), keeping only the
    /// blocks inside the world, clear of its walls and above its ground. A column whose text gives no base
    /// starts at its ground's. Returns how many columns were left out.
    pub fn set_blocks(&mut self, cols: Vec<crate::blocks::ColText>) -> usize {
        use proto::world::{block_cell, Column, BLOCK};
        for k in self.blocks.forget_all() {
            let (cx, cy) = block_cell(k);
            self.world.set_column(cx, cy, Column { base: 0, mask: 0 });
        }
        let mut dropped = 0;
        for c in cols {
            let (cx, cy) = block_cell(c.key);
            let (mx, my) = ((cx as f32 + 0.5) * BLOCK, (cy as f32 + 0.5) * BLOCK);
            let base = self.terrain.base(cx, cy);
            let from = c.base.unwrap_or(base) as i16;
            let mut col = crate::blocks::Col::new(base);
            for (k, layer) in c.layers.into_iter().enumerate() {
                let (Some((kind, owner)), Some(i)) = (layer, col.index(from + k as i16)) else { continue };
                col.kinds[i] = kind + 1;
                col.owners[i] = self.blocks.owner(owner.as_deref());
            }
            // A box just inside the cell: it meets walls and the edge, never the neighbouring blocks.
            let n = col.count();
            if n == 0 || self.blocks.total + n > crate::blocks::MAX_BLOCKS || self.world.blocked_by_walls(mx, my, BLOCK / 2.0 - 0.01) {
                dropped += 1;
                continue;
            }
            self.blocks.set_col(c.key, Some(col));
            self.world.set_column(cx, cy, col.column());
        }
        dropped
    }

    /// The layer an entity's feet count as being in, for building next to it: on the bare ground, the base
    /// of the column at `base`.
    fn feet(&self, s: usize, base: i8) -> i16 {
        let z = self.ents.z[s];
        if z == proto::world::GROUND { base as i16 } else { z as i16 }
    }

    /// Whether a block could go in layer `z` of this cell (whose column starts at `base`) without landing on
    /// anyone: inside the world, clear of walls, and no body in that layer there. `None` for `z` asks whether
    /// anyone at all stands in the cell. The grid is from the end of last tick, but it only picks the
    /// candidates: their positions are this tick's, and nobody moves a whole grid cell in a tick.
    fn cell_free(&self, cx: u32, cy: u32, base: i8, z: Option<i16>) -> bool {
        use proto::world::BLOCK;
        let (mx, my) = ((cx as f32 + 0.5) * BLOCK, (cy as f32 + 0.5) * BLOCK);
        if self.world.blocked_by_walls(mx, my, BLOCK / 2.0 - 0.01) {
            return false;
        }
        let r = self.defs.entity_radius;
        let mut occupied = false;
        let e = &self.ents;
        self.grid.for_neighbours(mx, my, |t| {
            let t = t as usize;
            if e.alive[t] && (e.x[t] - mx).abs() < BLOCK / 2.0 + r && (e.y[t] - my).abs() < BLOCK / 2.0 + r {
                occupied |= z.is_none_or(|z| self.feet(t, base) == z);
            }
        });
        !occupied
    }

    /// Set whole columns at once (the admin's way to place or clear blocks): each becomes exactly the layers
    /// given, counted from its ground, owned by nobody, or clear when none are. A new column goes only where
    /// nobody stands. Returns how many columns were set and how many refused.
    pub fn put_columns(&mut self, cols: Vec<crate::blocks::ColText>) -> (usize, usize) {
        use proto::world::block_cell;
        let (mut set, mut refused) = (0, 0);
        for c in cols {
            let (cx, cy) = block_cell(c.key);
            let base = self.terrain.base(cx, cy);
            let mut col = crate::blocks::Col::new(base);
            for (k, layer) in c.layers.iter().enumerate().take(col.kinds.len()) {
                if let Some((kind, _)) = layer {
                    col.kinds[k] = kind + 1;
                }
            }
            let n = col.count();
            let had = self.blocks.get(c.key).map_or(0, |c| c.count());
            if had == 0 && n > 0 && (!self.cell_free(cx, cy, base, None) || self.blocks.total + n > crate::blocks::MAX_BLOCKS) {
                refused += 1;
                continue;
            }
            self.blocks.set_col(c.key, Some(col));
            self.world.set_column(cx, cy, col.column());
            set += 1;
        }
        (set, refused)
    }

    /// Build (put a block in a cell's layer) or break (take one out), for the entity in `s`. Checked here: in
    /// reach, inside the world, not in a wall, not on anyone, resting on the ground or against another block,
    /// under the column and world limits; and only the owner breaks a block (anyone, one nobody owns).
    fn build_action(&mut self, s: usize, action: u8, target: u32, arg: u16) -> bool {
        use proto::game::{parse_build_arg, parse_build_target, ACT_BREAK, BLOCK_KINDS, BUILD_REACH};
        use proto::world::{block_key, BLOCK};
        let (cx, cy) = parse_build_target(target);
        let (z, kind) = parse_build_arg(arg);
        let z = z as i16;
        if kind >= BLOCK_KINDS {
            return false;
        }
        let (bx, by) = (cx as f32 * BLOCK, cy as f32 * BLOCK);
        if bx + BLOCK > self.world.size || by + BLOCK > self.world.size {
            return false;
        }
        let (mx, my) = (bx + BLOCK / 2.0, by + BLOCK / 2.0);
        let (dx, dy) = (mx - self.ents.x[s], my - self.ents.y[s]);
        let key = block_key(cx, cy);
        let base = self.blocks.get(key).map_or_else(|| self.terrain.base(cx, cy), |c| c.base);
        let (ex, ey) = ((self.ents.x[s] / BLOCK) as u32, (self.ents.y[s] / BLOCK) as u32);
        let my_base = self.blocks.get(block_key(ex, ey)).map_or_else(|| self.terrain.base(ex, ey), |c| c.base);
        let dz = ((z - self.feet(s, my_base)) as f32 + 0.5) * BLOCK;
        if dx * dx + dy * dy > BUILD_REACH * BUILD_REACH || dz.abs() > BUILD_REACH {
            return false;
        }
        if action == ACT_BREAK {
            let Some(col) = self.blocks.get(key) else { return false };
            let Some(k) = col.index(z).filter(|&k| col.kinds[k] != 0) else { return false };
            if col.owners[k] != 0 && col.owners[k] != self.ents.account[s] {
                return false; // someone else's
            }
            self.blocks.take(key, z);
            let mask = self.blocks.get(key).map(|c| c.column()).unwrap_or(proto::world::Column { base, mask: 0 });
            self.world.set_column(cx, cy, mask);
            self.stats.breaks += 1;
            return true;
        }
        let col = self.blocks.get(key).copied().unwrap_or_else(|| crate::blocks::Col::new(base));
        if col.index(z).is_none() || col.solid(z) || self.blocks.total >= crate::blocks::MAX_BLOCKS {
            return false;
        }
        // It rests on the ground, or against a block above, below or beside it.
        let beside = |c: Option<(u32, u32)>| c.and_then(|(x, y)| self.blocks.get(block_key(x, y))).is_some_and(|n| n.solid(z));
        let supported = z == base as i16
            || col.solid(z - 1)
            || col.solid(z + 1)
            || beside(cx.checked_sub(1).map(|x| (x, cy)))
            || beside(Some((cx + 1, cy)))
            || beside(cy.checked_sub(1).map(|y| (cx, y)))
            || beside(Some((cx, cy + 1)));
        if !supported || !self.cell_free(cx, cy, base, Some(z)) {
            return false;
        }
        let owner = self.ents.account[s];
        self.blocks.put(key, base, z, kind, owner);
        let mask = self.blocks.get(key).map(|c| c.column()).unwrap_or(proto::world::Column { base, mask: 0 });
        self.world.set_column(cx, cy, mask);
        self.stats.builds += 1;
        true
    }

    /// Who a player builds as: their account's tag, or None for a guest (whose blocks nobody owns).
    pub fn set_account(&mut self, slot: usize, tag: Option<&str>) {
        self.ents.account[slot] = self.blocks.owner(tag);
    }

    /// A free point inside the home place if there is one, else anywhere free.
    fn spawn_point(&mut self, home: Option<usize>, r: f32) -> (f32, f32) {
        if let Some(p) = home.and_then(|h| self.places.get(h)).cloned() {
            let size = self.world.size;
            for _ in 0..24 {
                let ang = self.rng.f32() * std::f32::consts::TAU;
                let d = self.rng.f32().sqrt() * p.r;
                let (x, y) = ((p.x + ang.cos() * d).clamp(r, size - r), (p.y + ang.sin() * d).clamp(r, size - r));
                if !self.world.blocked(x, y, r) {
                    return (x, y);
                }
            }
        }
        self.world.random_spawn(&mut self.rng, r)
    }

    /// Attributes back to their starting values (archetype overrides included).
    fn reset_attrs(&mut self, slot: usize) {
        let a = self.ents.arch[slot] as usize;
        for (i, def) in self.defs.attrs.iter().enumerate() {
            self.ents.attrs[i][slot] = self.archs.get(a).map_or(def.default, |r| r.start[i]);
        }
    }

    /// Everything about the bots worth saving: which archetype, where, and their attributes.
    pub fn bot_records(&self) -> crate::store::WorldSave {
        let mut bots = Vec::new();
        for s in 0..self.ents.high {
            if self.ents.alive[s] && self.ents.arch[s] != NO_ARCH && self.ents.owner[s] >= DRIVEN && !self.ents.fleeting[s] {
                let Some(a) = self.archs.get(self.ents.arch[s] as usize) else { continue };
                let rec = self.record_of(s);
                bots.push(crate::store::BotRec { arch: a.name.clone(), x: rec.x, y: rec.y, attrs: rec.attrs });
            }
        }
        crate::store::WorldSave { bots }
    }

    /// Bring a saved world back: every saved bot whose archetype still exists returns where it
    /// was with the attributes it had, then each archetype is topped up to its `count`. So
    /// `count` is the starting population of a new world and the minimum of a returning one;
    /// bots added by hand survive a restart.
    pub fn populate_from(&mut self, saved: &crate::store::WorldSave) {
        self.populate_bots_with(Some(saved));
    }

    /// Replace the bot population with what the archetypes ask for.
    pub fn populate_bots(&mut self) {
        self.populate_bots_with(None);
    }

    fn populate_bots_with(&mut self, saved: Option<&crate::store::WorldSave>) {
        for s in 0..self.ents.high {
            if self.ents.alive[s] && self.ents.arch[s] != NO_ARCH && self.ents.owner[s] >= DRIVEN {
                self.ents.despawn(s);
            }
        }
        let mut have = vec![0u32; self.archs.len()];
        let all = saved.map_or(&[][..], |w| &w.bots[..]);
        for (k, b) in all.iter().enumerate() {
            let Some(a) = self.archs.iter().position(|a| a.name == b.arch) else { continue };
            let Some(slot) = self.spawn_as(NONE, a as u8, Some((b.x, b.y))) else {
                eprintln!("{} saved bot(s) did not fit: the rest of the entity table is kept for players", all.len() - k);
                return;
            };
            // A spot that is no longer legal falls back to a fresh one inside spawn_as; attributes still apply.
            self.restore(slot, &crate::store::Record { x: f32::NAN, y: f32::NAN, attrs: b.attrs.clone(), inv: Vec::new(), name: String::new(), zone: None, ..Default::default() });
            have[a] += 1;
        }
        for a in 0..self.archs.len() {
            for _ in have[a]..self.archs[a].count {
                if self.spawn_as(NONE, a as u8, None).is_none() {
                    return;
                }
            }
        }
    }

    /// Advance the world by one fixed step.
    pub fn step(&mut self, pending: &[PendingAction]) {
        self.tick += 1;
        if self.tick % self.tick_hz.max(1) == 1 {
            self.lit_attr = self.defs.attrs.iter().position(|a| a.name == proto::game::LIT_ATTR);
            self.night = self.daylight.phase(proto::daylight::now_ms()) >= 1.0 - self.daylight.night_share();
        }
        self.npc_actions.clear();
        let old = self.tick.saturating_sub(CAST_TICKS);
        self.casts.retain(|c| c.0 > old);
        self.system_npc_ai();
        self.system_regen();
        self.system_movement();
        self.system_actions(pending);
        let npc = std::mem::take(&mut self.npc_actions);
        self.system_actions(&npc);
        self.npc_actions = npc;
        let adm = std::mem::take(&mut self.admin_actions);
        self.system_actions(&adm);
        self.system_spells();
        self.system_air();
        self.system_rules();
        if self.tick % self.tick_hz.max(1) == 0 {
            self.expire_ground();
            self.system_discover();
            self.system_quests();
            self.system_encounters();
        }
        self.grid.rebuild(&self.ents);
        for slot in 0..self.ents.high {
            if self.ents.alive[slot] {
                self.entry_cache[slot] = self.entry_of(slot);
            }
        }
    }

    /// Steer toward a point that does not move (a place, a route point): down its flow field when one is
    /// built and the bot is in its window, otherwise straight at it. Remembers the point so the bot keeps
    /// following the field between decisions.
    fn toward_static(&mut self, s: usize, tx: f32, ty: f32, stop: f32) -> (i8, i8) {
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let (dx, dy) = (tx - x, ty - y);
        if dx * dx + dy * dy <= stop * stop {
            return (0, 0);
        }
        self.nav_goal[s] = (tx, ty, stop);
        let r = self.defs.entity_radius;
        self.nav.steer(&self.world, r, self.tick, x, y, tx, ty, true).unwrap_or_else(|| dir_to(dx, dy, stop))
    }

    /// Between decisions, turn a bot walking to a static point along its (already built) flow field. Where
    /// the field has nothing to say the bot keeps its direction, as it did before there were fields.
    fn follow_field(&mut self, s: usize) {
        let (tx, ty, _) = self.nav_goal[s];
        let r = self.defs.entity_radius;
        if let Some((dx, dy)) = self.nav.steer(&self.world, r, self.tick, self.ents.x[s], self.ents.y[s], tx, ty, false) {
            self.ents.dir_x[s] = dx;
            self.ents.dir_y[s] = dy;
        }
    }

    /// Random-walk step, drifting back toward the home place when outside it.
    fn wander_dir(&mut self, s: usize, a: usize) -> (i8, i8) {
        if let Some(p) = self.archs.get(a).and_then(|r| r.home).and_then(|h| self.places.get(h)) {
            let (dx, dy) = (p.x - self.ents.x[s], p.y - self.ents.y[s]);
            if dx * dx + dy * dy > p.r * p.r {
                let (px, py, pr) = (p.x, p.y, p.r);
                return self.toward_static(s, px, py, pr);
            }
        }
        if self.rng.below(4) == 0 {
            (0, 0)
        } else {
            const D: [i8; 3] = [-127, 0, 127];
            (D[self.rng.below(3) as usize], D[self.rng.below(3) as usize])
        }
    }

    /// Evaluate an archetype's rule stack for one bot. Returns false when no
    /// movement rule matched, so the plain `behaviour` still applies.
    fn run_rules(&mut self, s: usize, a: usize) -> bool {
        use proto::game::{ActRt, CondRt, Seek};
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let m = self.sight_at(x, y);
        let (reach, attack) = (self.archs[a].reach * m.max(1.0), self.archs[a].attack);
        let attack_range = attack.and_then(|id| self.defs.action(id)).map_or(0.0, |d| d.range);

        // One neighbour scan: nearest player, nearest other bot, nearest of either.
        let mut near: [Option<(f32, usize)>; 3] = [None; 3];
        let scan = reach.max(attack_range);
        if scan > 0.0 {
            let e = &self.ents;
            let me_arch = e.arch[s];
            let sneak = self.sneak();
            self.grid.for_neighbours(x, y, |t| {
                let t = t as usize;
                if t == s {
                    return;
                }
                let (dx, dy) = (e.x[t] - x, e.y[t] - y);
                let d2 = dx * dx + dy * dy;
                let m = sneak.map_or(1.0, |lit| seen_at(e, lit, t));
                if d2 > scan * scan * m * m {
                    return;
                }
                if e.owner[t] == GHOST {
                    return;
                }
                let cat = if e.owner[t] < DRIVEN { 0 } else if e.arch[t] == me_arch && me_arch != NO_ARCH { return } else { 1 };
                for c in [cat, 2] {
                    if near[c].map_or(true, |(b, _)| d2 < b) {
                        near[c] = Some((d2, t));
                    }
                }
            });
        }

        // Locked in a fight (`encounter.rs`): the one it is fighting is who it sees, chases and flees, however far.
        let foe = self.locked(s);
        let n_rules = self.archs[a].rules.len();
        for ri in 0..n_rules {
            let (when, then) = {
                let r = &self.archs[a].rules[ri];
                (r.when.clone(), r.then.clone())
            };
            let mut target: Option<(f32, usize)> = None;
            let hit = match when {
                CondRt::Always => true,
                CondRt::Sees { who: Seek::Player | Seek::Any, .. } if foe.is_some() => {
                    target = foe;
                    true
                }
                CondRt::Sees { who, within } => {
                    let n = near[match who {
                        Seek::Player => 0,
                        Seek::Bot => 1,
                        Seek::Any => 2,
                    }];
                    target = n.filter(|(d2, _)| *d2 <= (within * m) * (within * m));
                    target.is_some()
                }
                CondRt::AttrBelow { attr, value } => self.ents.attrs[attr][s] < value,
                CondRt::AttrAbove { attr, value } => self.ents.attrs[attr][s] > value,
                CondRt::InPlace { place } => {
                    let p = &self.places[place];
                    (p.x - x).powi(2) + (p.y - y).powi(2) <= p.r * p.r
                }
                CondRt::Chance { pct } => self.rng.f32() * 100.0 < pct,
            };
            if !hit {
                continue;
            }
            // A bot in a fight that can fight back does, whatever the rule says, unless it says to run.
            let then = if foe.is_some() && attack.is_some() && !matches!(then, ActRt::Flee | ActRt::Say { .. }) { ActRt::Attack } else { then };
            // Chasing, fleeing and attacking need someone: the one it is fighting, else a `sees` target, else the
            // nearest anyone within the archetype's own sight.
            let sight = self.archs[a].sight * m;
            let foe = || foe.or(target).or(near[2].filter(|(d2, _)| *d2 <= sight * sight));
            let steer: (i8, i8) = match then {
                ActRt::Say { text, shout } => {
                    if self.tick >= self.ents.say_at[s] {
                        self.ents.say_at[s] = self.tick + self.tick_hz * 6;
                        self.npc_say.push((s, shout, text));
                    }
                    continue;
                }
                ActRt::Idle => (0, 0),
                ActRt::Wander => self.wander_dir(s, a),
                ActRt::Chase | ActRt::Flee | ActRt::Attack => {
                    let Some((d2, t)) = foe() else { continue };
                    let (dx, dy) = (self.ents.x[t] - x, self.ents.y[t] - y);
                    if matches!(then, ActRt::Attack) {
                        if let Some(act) = attack {
                            if d2 <= attack_range * attack_range {
                                self.npc_actions.push(PendingAction { slot: s, action: act, target: self.ents.id_of(t), arg: 0 });
                            }
                        }
                    }
                    match then {
                        ActRt::Flee => dir_to(-dx, -dy, 1.0),
                        _ => dir_to(dx, dy, 1.0),
                    }
                }
                ActRt::Goto { place } => {
                    let p = &self.places[place];
                    let (px, py, stop) = (p.x, p.y, (p.r * 0.5).max(1.0));
                    self.toward_static(s, px, py, stop)
                }
                ActRt::Patrol { route } => {
                    let r = &self.routes[route];
                    let n = r.points.len();
                    let mut i = (self.ents.wp[s] as usize).min(n - 1);
                    let mut fwd = self.ents.wp_fwd[s];
                    let [px, py] = r.points[i];
                    if (px - x).powi(2) + (py - y).powi(2) < 9.0 {
                        if fwd {
                            if i + 1 < n {
                                i += 1;
                            } else if r.looped {
                                i = 0;
                            } else {
                                fwd = false;
                                i = n.saturating_sub(2);
                            }
                        } else if i > 0 {
                            i -= 1;
                        } else {
                            fwd = true;
                            i = 1.min(n - 1);
                        }
                        self.ents.wp[s] = i as u8;
                        self.ents.wp_fwd[s] = fwd;
                    }
                    let [px, py] = r.points[i];
                    self.toward_static(s, px, py, 0.5)
                }
            };
            self.ents.dir_x[s] = steer.0;
            self.ents.dir_y[s] = steer.1;
            return true;
        }
        false
    }

    /// Bots are ordinary entities. Their "AI" writes the same direction fields
    /// a player's input would, and queues the same action requests, at a low
    /// rate (every 10th tick, staggered).
    fn system_npc_ai(&mut self) {
        let tick = self.tick;
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] != NONE {
                continue;
            }
            if (tick as usize + s) % 10 != 0 {
                if (tick as usize + s) % 2 == 0 && self.nav_goal[s].2 >= 0.0 {
                    self.follow_field(s);
                }
                continue;
            }
            self.nav_goal[s].2 = -1.0;
            let a = self.ents.arch[s];
            if self.archs.get(a as usize).is_some_and(|r| !r.rules.is_empty()) && self.run_rules(s, a as usize) {
                continue;
            }
            let a = self.ents.arch[s];
            let (behaviour, sight, attack, attack_range) = match self.archs.get(a as usize) {
                Some(r) => (
                    r.behaviour,
                    r.sight * self.sight_at(self.ents.x[s], self.ents.y[s]),
                    r.attack,
                    r.attack.and_then(|id| self.defs.action(id)).map_or(0.0, |d| d.range),
                ),
                None => (Behaviour::Wander, 0.0, None, 0.0),
            };
            let wants_target = matches!(behaviour, Behaviour::Chase | Behaviour::Flee) || attack.is_some();
            let mut near: Option<(f32, usize)> = None;
            if wants_target {
                let reach = sight.max(attack_range);
                let (x, y) = (self.ents.x[s], self.ents.y[s]);
                let e = &self.ents;
                let sneak = self.sneak();
                self.grid.for_neighbours(x, y, |t| {
                    let t = t as usize;
                    if t == s || e.arch[t] == a && a != NO_ARCH {
                        return;
                    }
                    let (dx, dy) = (e.x[t] - x, e.y[t] - y);
                    let d2 = dx * dx + dy * dy;
                    let m = sneak.map_or(1.0, |lit| seen_at(e, lit, t));
                    if d2 <= reach * reach * m * m && near.map_or(true, |(b, _)| d2 < b) {
                        near = Some((d2, t));
                    }
                });
            }
            let toward = |near: Option<(f32, usize)>, e: &Entities, sign: f32| -> Option<(i8, i8)> {
                let (d2, t) = near?;
                if d2 < 1.0 {
                    return Some((0, 0));
                }
                let d = d2.sqrt();
                let q = |v: f32| (v * 127.0).round().clamp(-127.0, 127.0) as i8;
                Some((q(sign * (e.x[t] - e.x[s]) / d), q(sign * (e.y[t] - e.y[s]) / d)))
            };
            // Locked in a fight: after (or away from) that one however far, and one that can attack goes for them.
            let foe = self.locked(s);
            let near = foe.or(near);
            let seen = |n: &(f32, usize)| foe.is_some() || n.0 <= sight * sight;
            let steer = match behaviour {
                Behaviour::Idle | Behaviour::Wander if foe.is_some() && attack.is_some() => toward(foe, &self.ents, 1.0),
                Behaviour::Idle => Some((0, 0)),
                Behaviour::Chase => toward(near.filter(seen), &self.ents, 1.0),
                Behaviour::Flee => toward(near.filter(seen), &self.ents, -1.0),
                Behaviour::Wander => None,
            };
            let (dx, dy) = match steer {
                Some(d) => d,
                None if behaviour == Behaviour::Idle => (0, 0),
                None => self.wander_dir(s, a as usize),
            };
            self.ents.dir_x[s] = dx;
            self.ents.dir_y[s] = dy;
            if let (Some(act), Some((d2, t))) = (attack, near) {
                if d2 <= attack_range * attack_range {
                    self.npc_actions.push(PendingAction { slot: s, action: act, target: self.ents.id_of(t), arg: 0 });
                }
            }
        }
    }

    fn system_regen(&mut self) {
        for (a, def) in self.defs.attrs.iter().enumerate() {
            if def.regen_per_sec == 0.0 {
                continue;
            }
            let d = def.regen_per_sec * self.dt;
            let max = def.max;
            for v in &mut self.ents.attrs[a][..self.ents.high] {
                *v = (*v + d).min(max);
            }
        }
    }

    fn system_movement(&mut self) {
        let r = self.defs.entity_radius;
        let dt = self.dt;
        let e = &mut self.ents;
        for s in 0..e.high {
            // Standing still on the bare ground changes nothing; up on blocks it may (one broken underfoot).
            if !e.alive[s] || (e.dir_x[s] == 0 && e.dir_y[s] == 0 && e.z[s] == proto::world::GROUND) || e.held[s] != 0 {
                continue;
            }
            let speed = e.speed[s] * self.speed_attr.map_or(1.0, |a| e.attrs[a][s]);
            let (mut nx, ny, mut nz) = integrate(&self.world, e.x[s], e.y[s], e.z[s], e.dir_x[s], e.dir_y[s], speed, dt, r);
            if let (Some((lo, hi)), true) = (self.zone, e.owner[s] == NONE) {
                nx = nx.clamp(lo + r, (hi - r).max(lo + r));
                match self.world.enter(nx, ny, r, e.z[s]) {
                    Some(z) => nz = z,
                    None => continue,
                }
            }
            e.x[s] = nx;
            e.y[s] = ny;
            e.z[s] = nz;
        }
    }

    /// Generic action resolution. Every check here is server side: the client
    /// only said "I want to use action N on target T".
    fn system_actions(&mut self, pending: &[PendingAction]) {
        for p in pending {
            if !self.ents.alive[p.slot] {
                continue;
            }
            if p.action == proto::game::ACT_LIGHT {
                if let Some(a) = self.lit_attr.or_else(|| self.defs.attrs.iter().position(|d| d.name == proto::game::LIT_ATTR)) {
                    let d = &self.defs.attrs[a];
                    self.ents.attrs[a][p.slot] = if p.arg > 0 { d.max } else { d.min };
                }
                continue;
            }
            if p.action == proto::game::ACT_JUMP {
                if self.ents.air[p.slot] == 0 {
                    self.ents.air[p.slot] = self.jump_ticks();
                    self.stats.jumps += 1;
                } else {
                    self.stats.rejected_actions += 1; // no jumping in mid-air
                }
                continue;
            }
            if p.action == proto::game::ACT_BUILD || p.action == proto::game::ACT_BREAK {
                if !self.build_action(p.slot, p.action, p.target, p.arg) {
                    self.stats.rejected_actions += 1;
                }
                continue;
            }
            if matches!(
                p.action,
                proto::game::ACT_TALK | proto::game::ACT_ACCEPT | proto::game::ACT_ABANDON | proto::game::ACT_BUY | proto::game::ACT_SELL | proto::game::ACT_UNEQUIP
            ) {
                let ok = match p.action {
                    proto::game::ACT_TALK => self.talk(p.slot, p.target),
                    proto::game::ACT_ACCEPT => self.accept(p.slot, p.arg),
                    proto::game::ACT_ABANDON => self.abandon(p.slot, p.arg),
                    proto::game::ACT_BUY => self.buy(p.slot, p.target, p.arg),
                    proto::game::ACT_SELL => self.sell(p.slot, p.target, p.arg),
                    _ => self.unequip(p.slot, p.arg),
                };
                if !ok {
                    self.stats.rejected_actions += 1;
                }
                continue;
            }
            if p.action >= proto::game::ACT_USE {
                if !self.item_action(p.slot, p.action) {
                    self.stats.rejected_actions += 1;
                }
                continue;
            }
            if !self.use_action(p) {
                self.stats.rejected_actions += 1;
            }
        }
    }

    /// A game action (a blow, a spell): checks, then its effects on the target, on everyone in its area, and
    /// on the caster. False if it was refused. A target that jumped clear dodges it, and the cooldown is kept.
    fn use_action(&mut self, p: &PendingAction) -> bool {
        use proto::game::{Aim, Hits};
        let Some(k) = self.defs.actions.iter().position(|a| a.id == p.action) else { return false };
        if self.mounted[p.slot] > 0.0 {
            self.dismount(p.slot); // into the fight, out of the saddle
            self.gear_ver[p.slot] = self.gear_ver[p.slot].wrapping_add(1);
        }
        let def = &self.defs.actions[k];
        let own = p.slot * proto::game::MAX_ACTIONS + k;
        let (aim, area, hits, hold, cooldown, range, lift) = (def.aim, def.area, def.hits, def.hold, def.cooldown_ticks, def.range, def.lift);
        let e = &self.ents;
        if self.tick < e.cooldown_until[p.slot] || self.tick < e.ready_at[own] || matches!(e.held[p.slot], 1 | 2) {
            return false; // mezzed or stunned: no acting
        }
        if e.owner[p.slot] < DRIVEN && self.levels.is_some() && def.level > e.level[p.slot] {
            return false; // not learnt yet
        }
        if let Some((attr, min)) = def.requires {
            if e.attrs[attr as usize][p.slot] < min {
                return false;
            }
        }
        let player = |s: usize| e.owner[s] < DRIVEN;
        let harmful = def.hold.is_some() || def.effects.iter().any(|f| matches!(*f, Effect::AddToTarget { delta, .. } if delta < 0.0));
        // Player against player: only with pvp on, and neither of them in a safe place.
        let pvp_here = self.pvp && !self.in_safe(p.slot);
        let fair = |s: usize| s == p.slot || !player(s) || !player(p.slot) || (pvp_here && !self.in_safe(s));
        let lands = |s: usize| match hits {
            Hits::Any => !harmful || fair(s),
            Hits::Foes => player(s) != player(p.slot) || (s != p.slot && fair(s)),
            Hits::Friends => player(s) == player(p.slot),
        };
        // Where it lands: on the target (which must be in reach and of the side it is for), or on the caster.
        let at = match aim {
            Aim::Caster => p.slot,
            // A spell for friends with no friend picked (no target, yourself, or a foe) lands on the caster.
            Aim::Target if hits == Hits::Friends && e.resolve(p.target).filter(|&t| t != p.slot && lands(t)).is_none() => p.slot,
            Aim::Target => {
                let Some(t) = e.resolve(p.target).filter(|&t| e.owner[t] != GHOST) else { return false };
                if t == p.slot || !lands(t) {
                    return false;
                }
                if e.air[t] > 0 && area == 0.0 {
                    self.stats.dodges += 1; // it jumped clear; the cooldown is not spent
                    return true;
                }
                let (dx, dy) = (e.x[t] - e.x[p.slot], e.y[t] - e.y[p.slot]);
                if dx * dx + dy * dy > range * range {
                    return false;
                }
                t
            }
        };
        let mut struck: Vec<usize> = Vec::new();
        if area > 0.0 {
            let (cx, cy) = (e.x[at], e.y[at]);
            let r2 = area * area;
            self.grid.for_neighbours(cx, cy, |s| {
                let s = s as usize;
                let (dx, dy) = (e.x[s] - cx, e.y[s] - cy);
                if e.alive[s] && e.owner[s] != GHOST && dx * dx + dy * dy <= r2 && e.air[s] == 0 && lands(s) && (s != p.slot || aim == Aim::Caster) {
                    struck.push(s);
                }
            });
        } else if aim == Aim::Target || lands(at) {
            struck.push(at);
        }
        let effects = self.defs.action(p.action).map(|d| d.effects.clone()).unwrap_or_default();
        for eff in &effects {
            let (attr, delta, secs, lasts, to_target) = match *eff {
                Effect::AddToTarget { attr, delta, secs, lasts } => (attr as usize, delta, secs, lasts, true),
                Effect::AddToSelf { attr, delta, secs, lasts } => (attr as usize, delta, secs, lasts, false),
            };
            let on: &[usize] = if to_target { &struck } else { std::slice::from_ref(&p.slot) };
            let src = self.ents.id_of(p.slot);
            for &s in on {
                let delta = if to_target { self.scaled(p.slot, s, attr, delta) } else { delta };
                if secs > 0.0 {
                    let ticks = ((secs * self.tick_hz as f32).round() as u32).max(1);
                    self.over_time.push(OverTime { id: self.ents.id_of(s), attr, per_tick: delta / ticks as f32, left: ticks, src });
                } else if lasts > 0.0 {
                    // What it really changed (the attribute's range may have cut it short) is what comes back off.
                    let before = self.ents.attrs[attr][s];
                    self.add_to(s, attr, delta, src);
                    let at = self.tick + ((lasts * self.tick_hz as f32).round() as u32).max(1);
                    self.undo.push(Undo { id: self.ents.id_of(s), attr, delta: before - self.ents.attrs[attr][s], at });
                } else {
                    self.add_to(s, attr, delta, src);
                }
            }
        }
        if lift > 0.0 {
            let (half, until) = (self.jump_ticks() / 2 + 1, self.tick + ((lift * self.tick_hz as f32).round() as u32).max(1));
            for &s in &struck {
                self.ents.air[s] = self.ents.air[s].max(half);
                self.ents.lift_until[s] = until;
            }
        }
        if let Some((kind, secs)) = hold {
            let until = self.tick + ((secs * self.tick_hz as f32).round() as u32).max(1);
            for &s in &struck {
                if s != p.slot && !self.ents.god[s] {
                    self.ents.held[s] = kind as u8;
                    self.ents.held_until[s] = until;
                }
            }
        }
        let gcd = (GCD_SECS * self.tick_hz as f32).round() as u32;
        self.ents.cooldown_until[p.slot] = self.tick + (cooldown as u32).min(gcd);
        self.ents.ready_at[own] = self.tick + cooldown as u32;
        self.cast_seq = self.cast_seq.wrapping_add(1);
        let target = if at != p.slot { self.ents.id_of(at) } else { 0 };
        let cast = proto::wire::Cast { seq: self.cast_seq, caster: self.ents.id_of(p.slot), target, action: p.action };
        self.casts.push((self.tick, p.slot, cast));
        self.stats.zaps += 1;
        true
    }

    /// Whether an entity stands in a place marked safe (no harm between players there).
    pub(crate) fn in_safe(&self, s: usize) -> bool {
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        self.places.iter().any(|p| p.safe && (p.x - x).powi(2) + (p.y - y).powi(2) <= p.r * p.r)
    }

    /// A blow's or a heal's health change between two levels: the user's might, and for harm the target's toughness.
    pub(crate) fn scaled(&self, from: usize, to: usize, attr: usize, delta: f32) -> f32 {
        if attr != proto::defs::ATTR_HEALTH as usize {
            return delta;
        }
        // Level and gear together: what each adds, on top of 1.
        let (gm, _) = self.gear_bonus(from);
        let (_, gt) = self.gear_bonus(to);
        let might = self.levels.as_ref().map_or(1.0, |l| l.might_at(self.ents.level[from])) + gm;
        let tough = self.levels.as_ref().map_or(1.0, |l| l.toughness_at(self.ents.level[to])) + gt;
        let d = delta * might;
        if d < 0.0 { d / tough } else { d }
    }

    /// Add to an attribute, within its range. Damage (health going down) breaks a mez, and a player who did it
    /// (`src`, an entity id; 0 for none) is remembered for the experience if it falls.
    pub(crate) fn add_to(&mut self, s: usize, attr: usize, delta: f32, src: u32) {
        let a = &self.defs.attrs[attr];
        let v = &mut self.ents.attrs[attr][s];
        *v = (*v + delta).clamp(a.min, a.max);
        if attr == proto::defs::ATTR_HEALTH as usize && delta < 0.0 {
            if self.ents.held[s] == proto::game::HoldKind::Mez as u8 {
                self.ents.held[s] = 0;
            }
            if self.mounted[s] > 0.0 {
                self.dismount(s); // knocked out of the saddle
                self.gear_ver[s] = self.gear_ver[s].wrapping_add(1);
            }
            if let Some(by) = self.ents.resolve(src).filter(|&b| b != s) {
                self.engage(s, by);
            }
            if let Some(by) = self.ents.resolve(src).filter(|&b| b != s && self.ents.owner[b] < DRIVEN) {
                let id = self.ents.id_of(by);
                let list = &mut self.ents.hurt_by[s];
                list.retain(|h| h.0 != id);
                if list.len() >= 8 {
                    list.remove(0);
                }
                list.push((id, self.tick));
            }
        }
    }

    /// A bot has fallen: every player who hurt it in the last half minute gets its type's experience.
    fn credit(&mut self, s: usize) {
        let xp = self.archs.get(self.ents.arch[s] as usize).map_or(0, |a| a.xp);
        let since = self.tick.saturating_sub(30 * self.tick_hz);
        let mut who: Vec<usize> = self.ents.hurt_by[s].iter().filter(|h| h.1 >= since).filter_map(|h| self.ents.resolve(h.0)).collect();
        self.ents.hurt_by[s].clear();
        // Their party mates near it share in it too.
        let r2 = crate::social::PARTY_RANGE * crate::social::PARTY_RANGE;
        for p in who.clone() {
            for m in self.party_of(p) {
                if !who.contains(&m) && (self.ents.x[m] - self.ents.x[s]).powi(2) + (self.ents.y[m] - self.ents.y[s]).powi(2) <= r2 {
                    who.push(m);
                }
            }
        }
        if let Some(arch) = self.archs.get(self.ents.arch[s] as usize).map(|a| a.name.clone()) {
            self.quest_kill(&arch, self.ents.x[s], self.ents.y[s], &who);
        }
        if xp == 0 {
            return;
        }
        for p in who {
            self.award(p, xp, None);
        }
    }

    /// Give a player experience (and a line saying why, if given); a new level is announced to them.
    pub fn award(&mut self, s: usize, xp: u32, why: Option<String>) {
        let Some(l) = self.levels.as_ref() else { return };
        let before = self.ents.level[s];
        self.ents.xp[s] = self.ents.xp[s].saturating_add(xp);
        let now = l.level_of(self.ents.xp[s]);
        if let Some(w) = why {
            self.tells.push((s, w));
        }
        if now > before {
            self.ents.level[s] = now;
            self.tells.push((s, format!("You have reached level {now}!")));
        }
        self.progress_ver[s] = self.progress_ver[s].wrapping_add(1);
    }

    /// Once a second: players walking into a named place for the first time find it, for a little experience.
    fn system_discover(&mut self) {
        let Some(give) = self.levels.as_ref().map(|l| l.discover_xp) else { return };
        let mut found = Vec::new();
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= DRIVEN {
                continue;
            }
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            for p in &self.places {
                if !p.title.is_empty() && (p.x - x).powi(2) + (p.y - y).powi(2) <= p.r * p.r && !self.ents.found[s].contains(&p.name) {
                    found.push((s, p.name.clone(), p.title.clone()));
                }
            }
        }
        for (s, name, title) in found {
            self.ents.found[s].push(name);
            self.award(s, give, Some(format!("Discovered {title} (+{give} experience)")));
        }
    }

    /// Effects over time tick on; holds run out; the `held` attribute (when the game has one) shows each hold.
    fn system_spells(&mut self) {
        let mut runs = std::mem::take(&mut self.over_time);
        runs.retain_mut(|o| {
            let Some(s) = self.ents.resolve(o.id) else { return false };
            self.add_to(s, o.attr, o.per_tick, o.src);
            o.left -= 1;
            o.left > 0
        });
        runs.append(&mut self.over_time);
        self.over_time = runs;
        let now = self.tick;
        let mut due = std::mem::take(&mut self.undo);
        due.retain(|u| {
            if u.at > now {
                return true;
            }
            if let Some(s) = self.ents.resolve(u.id) {
                self.add_to(s, u.attr, u.delta, 0);
            }
            false
        });
        due.append(&mut self.undo);
        self.undo = due;
        self.speed_attr = self.defs.attrs.iter().position(|a| a.name == proto::game::SPEED_ATTR);
        let attr = self.defs.attrs.iter().position(|a| a.name == proto::game::HELD_ATTR);
        let level_attr = self.defs.attrs.iter().position(|a| a.name == proto::game::LEVEL_ATTR);
        for s in 0..self.ents.high {
            if let Some(a) = level_attr {
                let d = &self.defs.attrs[a];
                self.ents.attrs[a][s] = (self.ents.level[s] as f32).clamp(d.min, d.max);
            }
            if self.ents.held[s] != 0 && (self.tick >= self.ents.held_until[s] || !self.ents.alive[s]) {
                self.ents.held[s] = 0;
            }
            if let Some(a) = attr {
                let d = &self.defs.attrs[a];
                self.ents.attrs[a][s] = (self.ents.held[s] as f32).clamp(d.min, d.max);
            }
        }
    }

    /// Data-driven rules: an attribute that hits its minimum can trigger a
    /// respawn. This is where "death" lives, and it is just a rule on health.
    fn jump_ticks(&self) -> u16 {
        ((proto::game::JUMP_SECS * self.tick_hz as f32).round() as u16).max(1)
    }

    /// Jumps count down, and the `airborne` attribute (when the game has one) mirrors how much of each is left.
    fn system_air(&mut self) {
        let total = self.jump_ticks() as f32;
        let attr = self.defs.attrs.iter().position(|a| a.name == proto::game::AIRBORNE_ATTR);
        for s in 0..self.ents.high {
            if !self.ents.alive[s] {
                continue;
            }
            // Levitating: held at the top of the jump until it runs out, then down as from a jump.
            if self.ents.air[s] > 0 && self.tick >= self.ents.lift_until[s] {
                self.ents.air[s] -= 1;
            }
            if let Some(a) = attr {
                let d = &self.defs.attrs[a];
                self.ents.attrs[a][s] = d.min + (d.max - d.min) * self.ents.air[s] as f32 / total;
            }
        }
    }

    fn system_rules(&mut self) {
        // God mode first, so whatever this tick took is restored before the respawn rule can see a zero.
        for s in 0..self.ents.high {
            if self.ents.alive[s] && self.ents.god[s] {
                for (a, def) in self.defs.attrs.iter().enumerate() {
                    // Not the reserved ones the server keeps itself (a jump, a hold, speed, level, a light).
                    let reserved = [proto::game::AIRBORNE_ATTR, proto::game::HELD_ATTR, proto::game::SPEED_ATTR, proto::game::LEVEL_ATTR, proto::game::LIT_ATTR];
                    if !reserved.contains(&def.name.as_str()) {
                        self.ents.attrs[a][s] = def.max;
                    }
                }
            }
        }
        for a in 0..self.defs.attrs.len() {
            if self.defs.attrs[a].on_zero != OnZero::Respawn {
                continue;
            }
            let min = self.defs.attrs[a].min;
            for s in 0..self.ents.high {
                if self.ents.alive[s] && self.ents.owner[s] != GHOST && self.ents.attrs[a][s] <= min {
                    if self.is_bot(s) {
                        self.credit(s);
                        self.loot_from(s);
                    }
                    self.end_encounters(s); // whoever falls, the fight is over
                    if self.ents.fleeting[s] {
                        self.ents.despawn(s); // an event's bot: beaten is beaten
                        continue;
                    }
                    let (x, y) = self.world.random_spawn(&mut self.rng, self.defs.entity_radius);
                    self.ents.x[s] = x;
                    self.ents.y[s] = y;
                    self.reset_attrs(s);
                    self.ents.air[s] = 0;
                    self.ents.held[s] = 0;
                    self.ents.lift_until[s] = 0;
                    self.stats.respawns += 1;
                }
            }
        }
    }

    /// Wire entry for a slot (quantised position and replicated attributes).
    #[inline]
    pub fn entry_of(&self, slot: usize) -> Entry {
        let e = &self.ents;
        let mut attrs = [0u16; proto::snapshot::MAX_REPL];
        for (k, &a) in self.defs.replicated.iter().enumerate() {
            attrs[k] = self.defs.quantize(a, e.attrs[a][slot]);
        }
        Entry { id: e.id_of(slot), x: qpos(e.x[slot]), y: qpos(e.y[slot]), z: e.z[slot], attrs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::defs::{ACTION_ZAP, ATTR_ENERGY, ATTR_HEALTH};

    fn sim() -> Sim {
        let game = proto::game::GameFile::default_game().build().unwrap();
        let world = World::from_boxes(512.0, 1, vec![]);
        Sim::new(game, world, 64, 20, 1)
    }

    fn place(s: &mut Sim, slot: usize, x: f32, y: f32) {
        s.ents.x[slot] = x;
        s.ents.y[slot] = y;
    }

    #[test]
    fn spells_land_on_their_side_in_their_area_hold_and_tick_over_time() {
        use proto::game::{ActionFile, Aim, EffectFile, Hits, HoldFile, HoldKind, Who};
        let mut g = proto::game::GameFile::default_game();
        let hurt = |d: f32, secs: f32| EffectFile { to: Who::Target, attr: "health".into(), delta: d, secs, lasts: 0.0 };
        g.actions.push(ActionFile { id: 2, name: "nova".into(), range: 0.0, aim: Aim::Caster, area: 10.0, hits: Hits::Foes, effects: vec![hurt(-20.0, 0.0)], ..Default::default() });
        g.actions.push(ActionFile { id: 3, name: "mend".into(), range: 20.0, hits: Hits::Friends, effects: vec![hurt(20.0, 1.0)], ..Default::default() });
        g.actions.push(ActionFile { id: 4, name: "sleep".into(), range: 20.0, hits: Hits::Foes, hold: Some(HoldFile { kind: HoldKind::Mez, secs: 5.0 }), ..Default::default() });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let h = ATTR_HEALTH as usize;
        s.pvp = false; // sides only: players never foes to each other
        let [a, b, c, d] = [0, NONE, 1, NONE].map(|owner| s.spawn(owner).unwrap()); // a and c players, b and d not
        for (e, x) in [(a, 100.0), (b, 104.0), (c, 95.0), (d, 130.0)] {
            place(&mut s, e, x, 100.0);
        }
        s.grid.rebuild(&s.ents);
        let act = |s: &mut Sim, action: u8, t: usize| {
            let before = s.stats.rejected_actions;
            let target = s.ents.id_of(t);
            s.step(&[PendingAction { slot: a, action, target, arg: 0 }]);
            s.stats.rejected_actions == before
        };
        // A nova round the caster burns the bot beside it, not the player beside it, the far bot or itself.
        assert!(act(&mut s, 2, a));
        assert_eq!([s.ents.attrs[h][a], s.ents.attrs[h][b], s.ents.attrs[h][c], s.ents.attrs[h][d]], [100.0, 80.0, 100.0, 100.0]);
        assert_eq!(s.casts.last().map(|k| (k.2.target, k.2.action)), Some((0, 2)), "aimed at self: no target");
        // Mending is for friends: refused on the bot; on the player it comes back over a second.
        s.ents.attrs[h][c] = 50.0;
        s.ents.attrs[h][a] = 50.0;
        assert!(act(&mut s, 3, b), "a foe picked: it lands on the caster instead");
        assert!(act(&mut s, 3, c));
        assert!(s.ents.attrs[h][c] > 50.0 && s.ents.attrs[h][c] < 55.0, "a little at once, not all");
        for _ in 0..25 {
            s.step(&[]);
        }
        assert!((s.ents.attrs[h][c] - 70.0).abs() < 0.01, "all 20 over the second: {}", s.ents.attrs[h][c]);
        // Sleep holds the bot where it is, until something hurts it.
        place(&mut s, b, 104.0, 100.0); // it wandered while the mend ran
        assert!(act(&mut s, 4, b));
        assert_eq!(s.ents.held[b], HoldKind::Mez as u8);
        s.ents.dir_x[b] = 127;
        let x = s.ents.x[b];
        for _ in 0..5 {
            s.step(&[]);
        }
        assert_eq!(s.ents.x[b], x, "held still");
        s.ents.cooldown_until[a] = 0;
        s.ents.ready_at.fill(0);
        assert!(act(&mut s, ACTION_ZAP, b));
        assert_eq!(s.ents.held[b], 0, "damage broke the mez");
        s.step(&[]);
        assert!(s.ents.x[b] > x, "free to move again");
    }

    #[test]
    fn players_harm_each_other_only_with_pvp_on_and_never_in_a_safe_place() {
        use proto::game::{ActionFile, Aim, EffectFile, Hits, Who};
        let mut g = proto::game::GameFile::default_game();
        let hurt = EffectFile { to: Who::Target, attr: "health".into(), delta: -20.0, secs: 0.0, lasts: 0.0 };
        g.actions.push(ActionFile { id: 2, name: "nova".into(), aim: Aim::Caster, area: 10.0, hits: Hits::Foes, effects: vec![hurt], ..Default::default() });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let h = ATTR_HEALTH as usize;
        let [a, b] = [0, 1].map(|owner| s.spawn(owner).unwrap());
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 104.0, 100.0);
        s.grid.rebuild(&s.ents);
        s.pvp = false;
        let go = |s: &mut Sim, action: u8| {
            s.ents.cooldown_until[a] = 0;
            s.ents.ready_at.fill(0);
            s.ents.attrs[h][b] = 100.0;
            let target = s.ents.id_of(b);
            s.step(&[PendingAction { slot: a, action, target, arg: 0 }]);
            place(s, b, 104.0, 100.0);
            100.0 - s.ents.attrs[h][b]
        };
        assert_eq!((go(&mut s, ACTION_ZAP), go(&mut s, 2)), (0.0, 0.0), "pvp off: neither the blow nor the nova");
        s.pvp = true;
        assert_eq!((go(&mut s, ACTION_ZAP), go(&mut s, 2)), (10.0, 20.0), "pvp on: both land");
        s.places.push(proto::game::Place { name: "town".into(), x: 104.0, y: 100.0, r: 3.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: true });
        assert_eq!((go(&mut s, ACTION_ZAP), go(&mut s, 2)), (0.0, 0.0), "the target stands in a safe place");
    }

    #[test]
    fn a_haste_speeds_you_up_for_a_while_and_a_levitation_holds_you_aloft() {
        use proto::game::{ActionFile, Aim, AttrFile, EffectFile, OnZeroFile, Who};
        let mut g = proto::game::GameFile::default_game();
        g.attrs.push(AttrFile { name: "speed".into(), min: 0.5, max: 3.0, default: 1.0, bits: 8, replicated: false, regen_per_sec: 0.0, on_zero: OnZeroFile::None });
        let quick = EffectFile { to: Who::Target, attr: "speed".into(), delta: 0.6, secs: 0.0, lasts: 1.0 };
        g.actions.push(ActionFile { id: 2, name: "haste".into(), aim: Aim::Caster, effects: vec![quick], ..Default::default() });
        g.actions.push(ActionFile { id: 3, name: "levitate".into(), aim: Aim::Caster, lift: 2.0, ..Default::default() });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let sp = s.defs.attrs.iter().position(|a| a.name == "speed").unwrap();
        let a = s.spawn(0).unwrap();
        place(&mut s, a, 100.0, 100.0);
        let run = |s: &mut Sim, ticks: usize| {
            let x = s.ents.x[a];
            s.ents.dir_x[a] = 127;
            for _ in 0..ticks {
                s.step(&[]);
            }
            s.ents.dir_x[a] = 0;
            s.ents.x[a] - x
        };
        let plain = run(&mut s, 5);
        s.defs.actions[1].cooldown_ticks = 400;
        s.step(&[PendingAction { slot: a, action: 2, target: 0, arg: 0 }]);
        assert!((s.ents.attrs[sp][a] - 1.6).abs() < 1e-4);
        let hasted = run(&mut s, 5);
        assert!((hasted / plain - 1.6).abs() < 0.02, "plain {plain}, hasted {hasted}");
        for _ in 0..20 {
            s.step(&[]);
        }
        assert!((s.ents.attrs[sp][a] - 1.0).abs() < 1e-4, "it wore off: {}", s.ents.attrs[sp][a]);
        // The haste's long cooldown holds back only the haste: levitating needs no wait.
        assert!(s.tick < s.ents.ready_at[a * proto::game::MAX_ACTIONS + 1]);
        s.step(&[PendingAction { slot: a, action: 3, target: 0, arg: 0 }]);
        for _ in 0..30 {
            s.step(&[]);
        }
        assert!(s.ents.air[a] > 0, "still aloft after 1.5 s");
        for _ in 0..30 {
            s.step(&[]);
        }
        assert_eq!(s.ents.air[a], 0, "landed");
    }

    #[test]
    fn beating_bots_and_finding_places_earns_experience_levels_and_spells() {
        use proto::game::{ActionFile, LevelsFile};
        let mut g = proto::game::GameFile::default_game();
        g.levels = Some(LevelsFile { cap: 5, base: 100.0, exponent: 1.0, might: 0.5, toughness: 0.0, discover_xp: 10 });
        g.archetypes[0].xp = 60;
        g.actions.push(ActionFile { id: 2, name: "late".into(), range: 8.0, level: 2, ..Default::default() });
        let ford = proto::game::Place { name: "ford".into(), x: 300.0, y: 300.0, r: 10.0, title: "The Ford".into(), exit: None, sound: String::new(), music: String::new(), safe: false };
        g.places.push(ford);
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let h = ATTR_HEALTH as usize;
        let p = s.spawn(0).unwrap();
        let wolf = s.spawn_as(NONE, 0, None).unwrap();
        place(&mut s, p, 100.0, 100.0);
        place(&mut s, wolf, 104.0, 100.0);
        s.grid.rebuild(&s.ents);
        let id = s.ents.id_of(wolf);
        // Not learnt yet at level 1.
        let before = s.stats.rejected_actions;
        s.step(&[PendingAction { slot: p, action: 2, target: id, arg: 0 }]);
        assert_eq!(s.stats.rejected_actions, before + 1);
        // Finish it off: the one who hurt it gets its experience.
        s.ents.attrs[h][wolf] = 5.0;
        s.ents.cooldown_until[p] = 0;
        s.step(&[PendingAction { slot: p, action: ACTION_ZAP, target: id, arg: 0 }]);
        s.step(&[]);
        assert_eq!((s.ents.xp[p], s.ents.level[p]), (60, 1));
        // A named place found once is worth a little, and only once.
        place(&mut s, p, 302.0, 300.0);
        for _ in 0..45 {
            s.step(&[]);
        }
        assert_eq!(s.ents.xp[p], 70);
        assert!(s.tells.iter().any(|t| t.1.contains("Discovered The Ford")));
        // Over 100: level 2, which hits half as hard again (might 0.5 per level).
        s.award(p, 40, None);
        assert_eq!(s.ents.level[p], 2);
        assert!((s.scaled(p, wolf, h, -10.0) + 15.0).abs() < 1e-4);
        let rec = s.record_of(p);
        assert_eq!((rec.xp, rec.found.clone()), (110, vec!["ford".to_string()]));
    }

    #[test]
    fn zap_in_range_changes_attributes_and_sets_cooldown() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 105.0, 100.0);
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 90.0);
        // Full energy, minus the 5 the zap costs (regen clamps at max before the action runs).
        assert!((s.ents.attrs[ATTR_ENERGY as usize][a] - 95.0).abs() < 1e-3);
        // Cooldown blocks an immediate second zap.
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 90.0);
        assert_eq!(s.stats.rejected_actions, 1);
    }

    #[test]
    fn blocks_stack_bridge_and_stop_movement_and_only_their_owner_breaks_them() {
        use proto::game::{build_arg, build_target, ACT_BREAK, ACT_BUILD, MAX_STACK};
        use proto::world::{block_key, GROUND};
        let mut s = sim();
        s.world = World::from_boxes(512.0, 1, vec![proto::world::Aabb { x0: 120.0, y0: 0.0, x1: 130.0, y1: 512.0 }]);
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        s.set_account(a, Some("aaaaaaaaaaaaaaaa"));
        s.set_account(b, Some("bbbbbbbbbbbbbbbb"));
        place(&mut s, a, 100.0, 101.0);
        place(&mut s, b, 300.0, 300.0);
        s.grid.rebuild(&s.ents);
        let act = |s: &mut Sim, who: usize, action: u8, cx: u32, cy: u32, z: i8| {
            let before = s.stats.rejected_actions;
            s.step(&[PendingAction { slot: who, action, target: build_target(cx, cy), arg: build_arg(z, 11) }]);
            s.stats.rejected_actions == before
        };
        // Column (53, 50) is 6 units east: in reach. Flat ground, so it starts in layer 0; blocks stack up to
        // what reach allows (a few layers above the feet), and a block cannot float with nothing to rest on.
        assert!(!act(&mut s, a, ACT_BUILD, 53, 50, 1), "nothing under it or beside it");
        for z in 0..3 {
            assert!(act(&mut s, a, ACT_BUILD, 53, 50, z), "layer {z}");
        }
        assert!(!act(&mut s, a, ACT_BUILD, 53, 50, 2), "already there");
        assert!(!act(&mut s, a, ACT_BUILD, 53, 50, 6), "out of reach upwards");
        assert_eq!(s.world.column(53, 50).unwrap().mask, 0b111);
        // A bridge back towards us: a block beside the top one, in the air over (52, 50), then one beyond it.
        assert!(act(&mut s, a, ACT_BUILD, 52, 50, 2) && act(&mut s, a, ACT_BUILD, 51, 50, 2));
        assert!(s.world.column(52, 50).unwrap().solid(2) && !s.world.column(52, 50).unwrap().solid(0));
        // Refused: out of reach sideways, on yourself, inside a wall.
        assert!(!act(&mut s, a, ACT_BUILD, 60, 50, 0));
        assert!(!act(&mut s, a, ACT_BUILD, 50, 50, 0));
        place(&mut s, a, 118.0, 101.0);
        assert!(!act(&mut s, a, ACT_BUILD, 61, 50, 0), "inside the wall at x 120..130");
        place(&mut s, a, 100.0, 101.0);
        // Walking east goes under the bridge and stops at the 3-high column.
        s.ents.dir_x[a] = 127;
        for _ in 0..40 {
            s.step(&[]);
        }
        s.ents.dir_x[a] = 0;
        assert!(s.ents.x[a] < 106.0 && s.ents.x[a] > 104.0, "stopped at the block: {}", s.ents.x[a]);
        // Someone else cannot break it, wherever they stand.
        place(&mut s, b, 104.0, 99.0);
        assert!(!act(&mut s, b, ACT_BREAK, 53, 50, 2), "not theirs");
        // Its owner breaks any of its blocks, not just the top: the bottom two, leaving the top one up there.
        assert!(act(&mut s, a, ACT_BREAK, 53, 50, 0) && act(&mut s, a, ACT_BREAK, 53, 50, 1));
        assert_eq!(s.world.column(53, 50).unwrap().mask, 0b100, "the top block stays, held up by the bridge");
        // Now the way is clear underneath it too: walking on east goes under, on the ground.
        place(&mut s, b, 300.0, 300.0);
        s.ents.dir_x[a] = 127;
        for _ in 0..8 {
            s.step(&[]);
        }
        s.ents.dir_x[a] = 0;
        assert!(s.ents.x[a] > 106.0, "walked under: {}", s.ents.x[a]);
        assert!(s.ents.z[a] == 0 || s.ents.z[a] == GROUND, "on the ground: {}", s.ents.z[a]);
        // A low step is climbed: one block at (57, 50) (x 114..116), walked onto from the east.
        place(&mut s, a, 110.0, 101.0);
        assert!(act(&mut s, a, ACT_BUILD, 57, 50, 0));
        place(&mut s, a, 117.5, 101.0);
        s.ents.dir_x[a] = -127;
        let mut climbed = false;
        for _ in 0..10 {
            s.step(&[]);
            climbed |= s.ents.z[a] == 1;
        }
        s.ents.dir_x[a] = 0;
        assert!(climbed, "stepped up onto the block");
        assert!(MAX_STACK >= 8);
        // A new world keeps the blocks, except where its walls now stand.
        s.replace_world(World::from_boxes(512.0, 1, vec![proto::world::Aabb { x0: 104.0, y0: 98.0, x1: 109.0, y1: 104.0 }]), Default::default());
        assert!(s.blocks.get(block_key(57, 50)).is_some() && s.blocks.get(block_key(51, 50)).is_some());
        assert!(s.blocks.get(block_key(53, 50)).is_none() && s.blocks.get(block_key(52, 50)).is_none());
        assert_eq!(s.blocks.total, 2);
    }

    #[test]
    fn jumping_dodges_attacks_lands_and_shows_in_the_airborne_attribute() {
        let mut s = sim();
        let air = s.defs.attrs.iter().position(|a| a.name == proto::game::AIRBORNE_ATTR).expect("default game has airborne");
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 105.0, 100.0);
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: b, action: proto::game::ACT_JUMP, target: 0, arg: 0 }]);
        assert!(s.ents.air[b] > 0);
        assert!(s.ents.attrs[air][b] > 80.0, "just took off: {}", s.ents.attrs[air][b]);
        // A zap at a jumping target misses, and the attacker keeps its cooldown and energy.
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 100.0);
        assert_eq!(s.stats.dodges, 1);
        assert_eq!(s.ents.cooldown_until[a], 0);
        // No second jump in mid-air.
        s.step(&[PendingAction { slot: b, action: proto::game::ACT_JUMP, target: 0, arg: 0 }]);
        assert_eq!(s.stats.jumps, 1);
        for _ in 0..20 {
            s.step(&[]);
        }
        assert_eq!(s.ents.air[b], 0);
        assert_eq!(s.ents.attrs[air][b], 0.0);
        // Landed: the zap connects.
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 90.0);
    }

    #[test]
    fn bots_cannot_take_the_slots_kept_for_players() {
        let mut s = sim(); // 64 slots
        s.player_reserve = 60;
        let n = (0..10).filter(|_| s.spawn_as(NONE, 0, None).is_some()).count();
        assert_eq!(n, 4);
        assert!(s.spawn(NONE).is_some(), "a player still gets in");
        // Freeing a bot frees its place in the bot budget.
        let bot = (0..s.ents.high).find(|&i| s.ents.alive[i] && s.ents.arch[i] != NO_ARCH).unwrap();
        s.ents.despawn(bot);
        assert!(s.spawn_as(NONE, 0, None).is_some());
        assert!(s.spawn_as(NONE, 0, None).is_none());
    }

    #[test]
    fn god_mode_keeps_attributes_full_and_never_respawns() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 105.0, 100.0);
        s.ents.god[b] = true;
        s.ents.attrs[ATTR_HEALTH as usize][b] = 5.0; // one zap from death
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 100.0);
        assert_eq!((s.ents.x[b], s.ents.y[b]), (105.0, 100.0), "a god is not respawned");
        assert_eq!(s.stats.respawns, 0);
        // A reused slot starts mortal.
        s.ents.despawn(b);
        let c = s.spawn(1).unwrap();
        assert!(!s.ents.god[c]);
    }

    #[test]
    fn out_of_range_and_bad_targets_are_rejected() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 200.0, 100.0);
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.ents.attrs[0][b], 100.0);
        // Self target, unknown action, stale generation.
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: s.ents.id_of(a), arg: 0 }]);
        s.step(&[PendingAction { slot: a, action: 99, target: tid, arg: 0 }]);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid ^ (1 << 24), arg: 0 }]);
        assert_eq!(s.stats.zaps, 0);
        assert_eq!(s.stats.rejected_actions, 4);
    }

    #[test]
    fn health_zero_triggers_respawn_rule() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        s.ents.attrs[0][b] = 10.0;
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 101.0, 100.0);
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid, arg: 0 }]);
        assert_eq!(s.stats.respawns, 1);
        assert_eq!(s.ents.attrs[0][b], 100.0);
    }

    #[test]
    fn recycled_slot_gets_new_generation() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let old = s.ents.id_of(a);
        s.ents.despawn(a);
        assert!(s.ents.resolve(old).is_none());
        let a2 = s.spawn(0).unwrap();
        assert_eq!(a, a2);
        assert_ne!(old, s.ents.id_of(a2));
        assert!(s.ents.resolve(old).is_none());
    }

    #[test]
    fn grid_finds_neighbours_and_skips_far_entities() {
        let mut s = sim();
        let a = s.spawn(NONE).unwrap();
        let b = s.spawn(NONE).unwrap();
        let c = s.spawn(NONE).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 150.0, 120.0);
        place(&mut s, c, 450.0, 450.0);
        s.grid.rebuild(&s.ents);
        let mut found = Vec::new();
        s.grid.for_neighbours(100.0, 100.0, |x| found.push(x as usize));
        assert!(found.contains(&a) && found.contains(&b));
        assert!(!found.contains(&c));
    }

    #[test]
    fn movement_uses_server_speed() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        place(&mut s, a, 100.0, 100.0);
        s.ents.dir_x[a] = 127;
        for _ in 0..20 {
            s.step(&[]);
        }
        // 20 ticks at 20 Hz is one second at 20 units/s.
        assert!((s.ents.x[a] - 120.0).abs() < 0.01, "{}", s.ents.x[a]);
    }

    /// The collision-parity gate: the client's predictor and the real server step
    /// must agree through a maze of obstacles, for thousands of ticks of random input.
    #[test]
    fn client_prediction_matches_the_server_in_a_dense_world() {
        use proto::world::Aabb;
        use proto::Predictor;
        let mut rng = proto::Rng::new(77);
        let boxes: Vec<Aabb> = (0..220)
            .map(|_| {
                let (w, h) = (rng.range(4.0, 30.0), rng.range(4.0, 30.0));
                let (x0, y0) = (rng.range(0.0, 480.0 - w), rng.range(0.0, 480.0 - h));
                Aabb { x0, y0, x1: x0 + w, y1: y0 + h }
            })
            .collect();
        let game = proto::game::GameFile::default_game().build().unwrap();
        let (speed, radius) = (game.defs.move_speed, game.defs.entity_radius);
        let mut s = Sim::new(game, World::from_boxes(480.0, 3, boxes), 8, 20, 1);
        for a in s.archs.iter_mut() {
            a.count = 0;
        }
        let p_slot = s.spawn(0).unwrap();
        let mut pred = Predictor::new(s.ents.x[p_slot], s.ents.y[p_slot], speed, 20, radius);
        let (mut blocked_ticks, mut moved) = (0u32, 0u32);
        let mut dir = (0i8, 0i8);
        for seq in 1..=6000u32 {
            if seq % 15 == 1 {
                const D: [i8; 3] = [-127, 0, 127];
                dir = (D[rng.below(3) as usize], D[rng.below(3) as usize]);
            }
            let before = (s.ents.x[p_slot], s.ents.y[p_slot]);
            s.ents.dir_x[p_slot] = dir.0;
            s.ents.dir_y[p_slot] = dir.1;
            s.step(&[]);
            pred.apply_input(&s.world, seq, dir.0, dir.1);
            let after = (s.ents.x[p_slot], s.ents.y[p_slot]);
            assert_eq!((pred.x, pred.y), after, "diverged at tick {seq}");
            if after == before && dir != (0, 0) {
                blocked_ticks += 1;
            } else if after != before {
                moved += 1;
            }
        }
        // The test only means something if the walls were actually hit.
        assert!(blocked_ticks > 100 && moved > 1000, "blocked {blocked_ticks}, moved {moved}");
    }

    // ---- rule builder behaviour

    use proto::game::{Act, Archetype, Behaviour, Cond, GameFile, Place, Route, Rule, Seek};

    fn arch(name: &str, behaviour: Behaviour, rules: Vec<Rule>) -> Archetype {
        Archetype {
            name: name.into(),
            color: "#112233".into(),
            count: 0,
            behaviour,
            speed_mult: 1.0,
            sight: 40.0,
            attack: None,
            attrs: Default::default(),
            home: None,
            rules,
            drops: Vec::new(),
            xp: 0,
            level: 0,
            shop: Vec::new(),
            leash: 0.0,
        }
    }

    fn sim_with(f: impl FnOnce(&mut GameFile)) -> Sim {
        let mut g = GameFile::default_game();
        g.archetypes.clear();
        f(&mut g);
        Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 3)
    }

    fn run(s: &mut Sim, ticks: u32) {
        for _ in 0..ticks {
            s.step(&[]);
        }
    }

    #[test]
    fn at_night_bots_see_an_unlit_player_only_close_up() {
        // A chaser with 40 units of sight and a player 20 units east; the default game has `lit`.
        let setup = |phase: f64, lit: bool| {
            let mut s = sim_with(|g| {
                g.archetypes = vec![arch("hound", Behaviour::Chase, vec![])];
                g.daylight = Some(proto::daylight::Daylight::Fixed { phase });
            });
            let bot = s.spawn_as(NONE, 0, Some((200.0, 200.0))).unwrap();
            let me = s.spawn(7).unwrap();
            place(&mut s, me, 220.0, 200.0);
            s.step(&[PendingAction { slot: me, action: proto::game::ACT_LIGHT, target: 0, arg: lit as u16 }]);
            run(&mut s, 12);
            // Seen, it heads straight east at the player; unseen, it wanders where it likes.
            (s.ents.dir_x[bot], s.ents.dir_y[bot]) == (127, 0)
        };
        assert!(!setup(0.9, false), "night, light out: 20 units is past 40 x 0.35, so the hound does not come");
        assert!(setup(0.9, true), "night, light on: seen");
        assert!(setup(0.3, false), "by day the light makes no difference");
    }

    #[test]
    fn patrol_walks_the_route_in_order_and_loops() {
        let mut s = sim_with(|g| {
            g.routes = vec![Route { name: "sq".into(), points: vec![[100.0, 100.0], [140.0, 100.0], [140.0, 140.0], [100.0, 140.0]], looped: true }];
            g.archetypes = vec![arch("g", Behaviour::Idle, vec![Rule { when: Cond::Always, then: Act::Patrol { route: "sq".into() } }])];
        });
        let b = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        let mut visited = Vec::new();
        for _ in 0..1200 {
            s.step(&[]);
            let (x, y) = (s.ents.x[b], s.ents.y[b]);
            for (i, p) in [[100.0f32, 100.0], [140.0, 100.0], [140.0, 140.0], [100.0, 140.0]].iter().enumerate() {
                if (x - p[0]).hypot(y - p[1]) < 3.0 && visited.last() != Some(&i) {
                    visited.push(i);
                }
            }
        }
        assert!(visited.len() >= 9, "only visited {visited:?}");
        for w in visited.windows(2) {
            assert_eq!(w[1], (w[0] + 1) % 4, "out of order: {visited:?}");
        }
    }

    #[test]
    fn open_routes_turn_around_at_the_ends() {
        let mut s = sim_with(|g| {
            g.routes = vec![Route { name: "line".into(), points: vec![[100.0, 100.0], [150.0, 100.0]], looped: false }];
            g.archetypes = vec![arch("g", Behaviour::Idle, vec![Rule { when: Cond::Always, then: Act::Patrol { route: "line".into() } }])];
        });
        let b = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        let (mut hit_far, mut came_back) = (false, false);
        for _ in 0..800 {
            s.step(&[]);
            let x = s.ents.x[b];
            hit_far |= x > 147.0;
            came_back |= hit_far && x < 103.0;
        }
        assert!(hit_far && came_back);
    }

    #[test]
    fn bots_spawn_in_their_home_place_and_wander_stays_near_it() {
        let mut s = sim_with(|g| {
            g.places = vec![Place { name: "home".into(), x: 250.0, y: 250.0, r: 15.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false }];
            let mut a = arch("w", Behaviour::Wander, vec![]);
            a.home = Some("home".into());
            a.count = 20;
            g.archetypes = vec![a];
        });
        s.populate_bots();
        for sl in (0..s.ents.high).filter(|&sl| s.ents.alive[sl]) {
            let d = (s.ents.x[sl] - 250.0).hypot(s.ents.y[sl] - 250.0);
            assert!(d <= 15.01, "spawned {d} from the centre");
        }
        run(&mut s, 3000);
        for sl in (0..s.ents.high).filter(|&sl| s.ents.alive[sl]) {
            let d = (s.ents.x[sl] - 250.0).hypot(s.ents.y[sl] - 250.0);
            assert!(d < 15.0 + 10.0, "wandered {d} from home");
        }
    }

    #[test]
    fn first_matching_rule_wins_and_low_health_beats_chasing() {
        let rules = vec![
            Rule { when: Cond::AttrBelow { attr: "health".into(), value: 30.0 }, then: Act::Flee },
            Rule { when: Cond::Sees { who: Seek::Player, within: 50.0 }, then: Act::Chase },
        ];
        let mut s = sim_with(|g| g.archetypes = vec![arch("b", Behaviour::Idle, rules)]);
        let player = s.spawn(0).unwrap();
        place(&mut s, player, 200.0, 200.0);
        let bot = s.spawn_as(NONE, 0, Some((220.0, 200.0))).unwrap();
        run(&mut s, 12);
        assert!(s.ents.x[bot] < 220.0, "healthy bot should approach the player, x={}", s.ents.x[bot]);
        s.ents.attrs[ATTR_HEALTH as usize][bot] = 10.0;
        place(&mut s, player, 200.0, 200.0);
        place(&mut s, bot, 210.0, 200.0);
        s.ents.dir_x[bot] = 0;
        run(&mut s, 40);
        assert!(s.ents.x[bot] > 215.0, "hurt bot should run away, x={}", s.ents.x[bot]);
    }

    /// A cup open to the south around (250, 175), its base between the bot inside it and the place at (250, 260).
    fn cup() -> World {
        use proto::world::Aabb;
        World::from_boxes(
            512.0,
            1,
            vec![
                Aabb { x0: 200.0, y0: 200.0, x1: 300.0, y1: 204.0 },
                Aabb { x0: 200.0, y0: 140.0, x1: 204.0, y1: 204.0 },
                Aabb { x0: 296.0, y0: 140.0, x1: 300.0, y1: 204.0 },
            ],
        )
    }

    /// Ticks a bot in the cup takes to reach the place behind it, if it does within a minute.
    fn walk_out_of_the_cup(s: &mut Sim, bot: usize) -> Option<u32> {
        (0..1200).find(|_| {
            s.step(&[]);
            (s.ents.x[bot] - 250.0).hypot(s.ents.y[bot] - 260.0) < 6.0
        })
    }

    fn cup_sim(per_tick: u32) -> (Sim, usize) {
        let mut s = sim_with(|g| {
            g.places = vec![Place { name: "far".into(), x: 250.0, y: 260.0, r: 6.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false }];
            g.archetypes = vec![arch("b", Behaviour::Idle, vec![Rule { when: Cond::Always, then: Act::Goto { place: "far".into() } }])];
        });
        s.replace_world(cup(), Default::default());
        s.nav.per_tick = per_tick;
        let bot = s.spawn_as(NONE, 0, Some((250.0, 175.0))).unwrap();
        (s, bot)
    }

    #[test]
    fn bots_find_their_way_round_walls_to_a_place() {
        let (mut s, bot) = cup_sim(1);
        let t = walk_out_of_the_cup(&mut s, bot);
        assert!(t.is_some_and(|t| t < 600), "took {t:?}, at {},{}", s.ents.x[bot], s.ents.y[bot]);
        // Without fields (none may be built) the bot pushes straight into the cup's base for ever.
        let (mut s, bot) = cup_sim(0);
        assert_eq!(walk_out_of_the_cup(&mut s, bot), None);
        assert!(s.ents.y[bot] < 200.0 && s.nav.field_count() == 0);
    }

    #[test]
    fn route_points_are_followed_round_walls_too() {
        let mut s = sim_with(|g| {
            g.routes = vec![Route { name: "out".into(), points: vec![[250.0, 175.0], [250.0, 260.0]], looped: false }];
            g.archetypes = vec![arch("p", Behaviour::Idle, vec![Rule { when: Cond::Always, then: Act::Patrol { route: "out".into() } }])];
        });
        s.replace_world(cup(), Default::default());
        let bot = s.spawn_as(NONE, 0, Some((250.0, 175.0))).unwrap();
        assert!(walk_out_of_the_cup(&mut s, bot).is_some());
    }

    #[test]
    fn a_new_world_drops_the_navigation_grid() {
        let (mut s, bot) = cup_sim(1);
        run(&mut s, 30);
        assert!(s.nav.field_count() == 1 && s.nav.tile_count() > 0);
        let moved = s.replace_world(World::from_boxes(512.0, 1, vec![]), Default::default());
        assert_eq!((moved, s.nav.field_count(), s.nav.tile_count()), (0, 0, 0));
        // The old field would still send it the long way round; with the walls gone it walks straight there.
        place(&mut s, bot, 250.0, 175.0);
        let t = walk_out_of_the_cup(&mut s, bot);
        assert!(t.is_some_and(|t| t < 120), "took {t:?}");
    }

    #[test]
    fn say_fires_once_per_cooldown_and_does_not_stop_movement_rules() {
        let rules = vec![
            Rule { when: Cond::Sees { who: Seek::Player, within: 30.0 }, then: Act::Say { text: "Halt".into(), shout: false } },
            Rule { when: Cond::Always, then: Act::Goto { place: "gate".into() } },
        ];
        let mut s = sim_with(|g| {
            g.places = vec![Place { name: "gate".into(), x: 300.0, y: 100.0, r: 6.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: false }];
            g.archetypes = vec![arch("b", Behaviour::Idle, rules)];
        });
        let p = s.spawn(0).unwrap();
        place(&mut s, p, 100.0, 110.0);
        let bot = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        let mut said = 0;
        for _ in 0..100 {
            s.step(&[]);
            said += s.npc_say.len();
            s.npc_say.clear();
        }
        assert_eq!(said, 1, "one line, then the 6 second cooldown (100 ticks is 5 s)");
        assert!(s.ents.x[bot] > 150.0, "kept walking to the gate while talking, x={}", s.ents.x[bot]);
        run(&mut s, 400);
        let (x, y) = (s.ents.x[bot], s.ents.y[bot]);
        assert!((x - 300.0).hypot(y - 100.0) <= 6.0, "arrived at the gate: ({x}, {y})");
    }

    #[test]
    fn rules_that_do_not_match_fall_back_to_the_plain_behaviour() {
        let rules = vec![Rule { when: Cond::Sees { who: Seek::Player, within: 20.0 }, then: Act::Flee }];
        let mut s = sim_with(|g| g.archetypes = vec![arch("b", Behaviour::Idle, rules)]);
        let bot = s.spawn_as(NONE, 0, Some((100.0, 100.0))).unwrap();
        run(&mut s, 100);
        assert_eq!((s.ents.x[bot], s.ents.y[bot]), (100.0, 100.0), "idle fallback does not move");
    }

    #[test]
    fn attack_rule_closes_in_and_fires_only_when_in_range() {
        let mut s = sim_with(|g| {
            let mut a = arch("d", Behaviour::Idle, vec![Rule { when: Cond::Sees { who: Seek::Player, within: 60.0 }, then: Act::Attack }]);
            a.attack = Some("zap".into());
            g.archetypes = vec![a];
        });
        let p = s.spawn(0).unwrap();
        place(&mut s, p, 200.0, 200.0);
        let _bot = s.spawn_as(NONE, 0, Some((250.0, 200.0))).unwrap();
        run(&mut s, 5);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][p], 100.0, "too far to hit yet");
        run(&mut s, 120);
        assert!(s.ents.attrs[ATTR_HEALTH as usize][p] < 100.0, "should have been zapped");
    }

    #[test]
    fn restore_matches_attributes_by_name_clamps_and_rejects_bad_positions() {
        use crate::store::Record;
        let mut s = sim();
        let p = s.spawn(0).unwrap();
        let mut rec = Record { x: 123.0, y: 45.0, attrs: Default::default(), inv: Vec::new(), name: String::new(), zone: None, ..Default::default() };
        rec.attrs.insert("health".into(), 37.0);
        rec.attrs.insert("energy".into(), 9999.0); // beyond the current max
        rec.attrs.insert("removed_attr".into(), 5.0); // no longer exists
        s.restore(p, &rec);
        assert_eq!((s.ents.x[p], s.ents.y[p]), (123.0, 45.0));
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][p], 37.0);
        assert_eq!(s.ents.attrs[ATTR_ENERGY as usize][p], 100.0, "clamped to the attribute's max");
        // Out of the world, or not a number: position is left alone, attributes still apply.
        let (x0, y0) = (s.ents.x[p], s.ents.y[p]);
        for (x, y) in [(-5.0, 10.0), (9999.0, 10.0), (f32::NAN, 1.0), (10.0, f32::INFINITY)] {
            s.restore(p, &Record { x, y, attrs: Default::default(), inv: Vec::new(), name: String::new(), zone: None, ..Default::default() });
            assert_eq!((s.ents.x[p], s.ents.y[p]), (x0, y0), "({x}, {y})");
        }
        // A saved NaN attribute is ignored rather than poisoning the entity.
        let mut bad = Record { x: 100.0, y: 100.0, attrs: Default::default(), inv: Vec::new(), name: String::new(), zone: None, ..Default::default() };
        bad.attrs.insert("health".into(), f32::NAN);
        s.restore(p, &bad);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][p], 37.0);
        // A new attribute missing from the record keeps its default (new entities start there).
        let q = s.spawn(1).unwrap();
        s.restore(q, &Record { x: 50.0, y: 50.0, attrs: Default::default(), inv: Vec::new(), name: String::new(), zone: None, ..Default::default() });
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][q], 100.0);
        // Saved standing where there is a wall now: put back at the nearest free spot, not anywhere at all.
        s.world = World::from_boxes(512.0, 1, vec![proto::world::Aabb { x0: 200.0, y0: 200.0, x1: 206.0, y1: 206.0 }]);
        s.restore(q, &Record { x: 203.0, y: 203.0, ..Default::default() });
        let (x, y) = (s.ents.x[q], s.ents.y[q]);
        assert!(!s.world.blocked_by_walls(x, y, s.defs.entity_radius) && (x - 203.0).hypot(y - 203.0) < 8.0, "at ({x}, {y})");
    }

    #[test]
    fn a_saved_world_returns_bots_where_they_were_drops_unknown_archetypes_and_tops_up() {
        use crate::store::BotRec;
        let mut s = sim();
        let counts = |s: &Sim| {
            let mut n = vec![0usize; s.archs.len()];
            for i in 0..s.ents.high {
                if s.ents.alive[i] && s.ents.arch[i] != NO_ARCH && s.ents.owner[i] >= DRIVEN {
                    n[s.ents.arch[i] as usize] += 1;
                }
            }
            n
        };
        s.populate_bots();
        let fresh = counts(&s);
        let drone = s.archs.iter().position(|a| a.name == "drone").unwrap();
        // Hurt one drone and note where everything is, then save and rebuild the world.
        let hurt = (0..s.ents.high).find(|&i| s.ents.alive[i] && s.ents.arch[i] == drone as u8).unwrap();
        s.ents.attrs[ATTR_HEALTH as usize][hurt] = 7.0;
        let mut saved = s.bot_records();
        assert_eq!(saved.bots.len(), fresh.iter().sum::<usize>());
        // An archetype that has since been deleted from the game file, and one extra drone added by hand.
        saved.bots.push(BotRec { arch: "ghost".into(), x: 10.0, y: 10.0, attrs: Default::default() });
        saved.bots.push(BotRec { arch: "drone".into(), x: 77.0, y: 88.0, attrs: Default::default() });
        // Lose one archetype's bots entirely from the save: it must be topped back up to `count`.
        let hound = s.archs.iter().position(|a| a.name == "hound").unwrap();
        saved.bots.retain(|b| b.arch != "hound");
        let mut t = sim();
        t.populate_from(&saved);
        let n = counts(&t);
        assert_eq!(n[drone], fresh[drone] + 1, "the hand-added drone survives");
        assert_eq!(n[hound], fresh[hound], "a missing archetype is topped up to its count");
        assert_eq!(n.iter().sum::<usize>(), fresh.iter().sum::<usize>() + 1, "the unknown archetype is dropped");
        let at = |t: &Sim, x: f32, y: f32| (0..t.ents.high).any(|i| t.ents.alive[i] && (t.ents.x[i] - x).abs() < 0.01 && (t.ents.y[i] - y).abs() < 0.01);
        assert!(at(&t, 77.0, 88.0));
        let (bx, by) = (s.ents.x[hurt], s.ents.y[hurt]);
        let back = (0..t.ents.high).find(|&i| t.ents.alive[i] && (t.ents.x[i] - bx).abs() < 0.01 && (t.ents.y[i] - by).abs() < 0.01).expect("the hurt drone is where it was");
        assert_eq!(t.ents.attrs[ATTR_HEALTH as usize][back], 7.0, "and still hurt");
    }

    /// What solid scenery and built blocks cost the movement step. Run with:
    /// cargo test -p server --release movement_cost -- --ignored --nocapture
    #[test]
    #[ignore]
    fn movement_cost() {
        use proto::world::{Aabb, Column};
        let mut rng = Rng::new(7);
        let houses: Vec<Aabb> = (0..340)
            .map(|_| {
                let (x, y) = (rng.range(50.0, 1990.0), rng.range(50.0, 1990.0));
                Aabb { x0: x, y0: y, x1: x + rng.range(6.0, 16.0), y1: y + rng.range(6.0, 12.0) }
            })
            .collect();
        let trees: Vec<Aabb> = (0..3600)
            .map(|_| {
                let (x, y, h) = (rng.range(10.0, 2030.0), rng.range(10.0, 2030.0), rng.range(0.4, 0.6));
                Aabb { x0: x - h, y0: y - h, x1: x + h, y1: y + h }
            })
            .collect();
        let run = |label: &str, boxes: Vec<Aabb>, blocks: usize| {
            let game = proto::game::GameFile::default_game().build().unwrap();
            let mut s = Sim::new(game, World::from_boxes(2048.0, 1, boxes), 4200, 20, 1);
            let mut r = Rng::new(3);
            for _ in 0..blocks {
                let (cx, cy) = (r.below(1024), r.below(1024));
                s.world.set_column(cx, cy, Column { base: 0, mask: (1 << (1 + r.below(4))) - 1 });
            }
            let slots: Vec<usize> = (0..4000).filter_map(|_| s.spawn(NONE)).collect();
            for (i, &k) in slots.iter().enumerate() {
                s.ents.dir_x[k] = [127, -127, 0, 90][i % 4];
                s.ents.dir_y[k] = [0, 90, 127, -90][i % 4];
            }
            for _ in 0..20 {
                s.system_movement();
            }
            let t = std::time::Instant::now();
            let ticks = 400;
            for _ in 0..ticks {
                s.system_movement();
            }
            let us = t.elapsed().as_secs_f64() * 1e6 / ticks as f64;
            println!("{label:<40} {us:>7.0} us per tick for 4000 moving ({:.1}% of a 50 ms tick)", us / 500.0);
        };
        run("340 houses", houses.clone(), 0);
        run("340 houses + 3600 solid trees", houses.iter().chain(&trees).copied().collect(), 0);
        run("houses + trees + 2000 block columns", houses.iter().chain(&trees).copied().collect(), 2000);
        run("houses + trees + 50000 block columns", houses.iter().chain(&trees).copied().collect(), 50_000);
    }
}

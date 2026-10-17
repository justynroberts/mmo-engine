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
/// Archetype of entities that are not bots (players, plain NPCs).
pub const NO_ARCH: u8 = u8::MAX;
const SLOT_MASK: u32 = 0x00FF_FFFF;

pub struct Entities {
    pub alive: Vec<bool>,
    pub gen: Vec<u8>,
    pub x: Vec<f32>,
    pub y: Vec<f32>,
    pub dir_x: Vec<i8>,
    pub dir_y: Vec<i8>,
    pub cooldown_until: Vec<u32>,
    /// Owning client index, or `NONE`.
    pub owner: Vec<u32>,
    /// Archetype index, or `NO_ARCH`.
    pub arch: Vec<u8>,
    /// Units per second at full input.
    pub speed: Vec<f32>,
    /// One array per attribute, indexed by slot (structure of arrays).
    pub attrs: Vec<Vec<f32>>,
    free: Vec<u32>,
    pub high: usize,
    pub live: usize,
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
            owner: vec![NONE; cap],
            arch: vec![NO_ARCH; cap],
            speed: vec![0.0; cap],
            attrs: (0..n_attrs).map(|_| vec![0.0; cap]).collect(),
            free: Vec::new(),
            high: 0,
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
            self.alive[slot] = false;
            self.gen[slot] = self.gen[slot].wrapping_add(1);
            self.owner[slot] = NONE;
            self.free.push(slot as u32);
            self.live -= 1;
        }
    }
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

pub struct PendingAction {
    pub slot: usize,
    pub action: u8,
    pub target: u32,
}

#[derive(Default, Clone, Copy)]
pub struct SimStats {
    pub zaps: u64,
    pub respawns: u64,
    pub rejected_actions: u64,
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
    /// Actions requested by bots this tick.
    npc_actions: Vec<PendingAction>,
    /// Actions injected by the admin UI for the next tick.
    pub admin_actions: Vec<PendingAction>,
}

impl Sim {
    pub fn new(game: Game, world: World, cap: usize, tick_hz: u32, seed: u64) -> Self {
        let Game { defs, archs } = game;
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
            npc_actions: Vec::new(),
            admin_actions: Vec::new(),
        }
    }

    pub fn spawn(&mut self, owner: u32) -> Option<usize> {
        self.spawn_as(owner, NO_ARCH, None)
    }

    /// Spawn an entity of an archetype (or a plain one with `NO_ARCH`), at a
    /// given spot if it is free, otherwise at a random free spot.
    pub fn spawn_as(&mut self, owner: u32, arch: u8, at: Option<(f32, f32)>) -> Option<usize> {
        let r = self.defs.entity_radius;
        let slot = self.ents.alloc()?;
        let (x, y) = match at {
            Some((x, y)) if !self.world.blocked(x, y, r) => (x, y),
            _ => self.world.random_spawn(&mut self.rng, r),
        };
        let speed = self.defs.move_speed * self.archs.get(arch as usize).map_or(1.0, |a| a.speed_mult);
        let e = &mut self.ents;
        e.alive[slot] = true;
        e.x[slot] = x;
        e.y[slot] = y;
        e.dir_x[slot] = 0;
        e.dir_y[slot] = 0;
        e.cooldown_until[slot] = 0;
        e.owner[slot] = owner;
        e.arch[slot] = arch;
        e.speed[slot] = speed;
        e.live += 1;
        self.reset_attrs(slot);
        Some(slot)
    }

    /// Attributes back to their starting values (archetype overrides included).
    fn reset_attrs(&mut self, slot: usize) {
        let a = self.ents.arch[slot] as usize;
        for (i, def) in self.defs.attrs.iter().enumerate() {
            self.ents.attrs[i][slot] = self.archs.get(a).map_or(def.default, |r| r.start[i]);
        }
    }

    /// Replace the bot population with what the archetypes ask for.
    pub fn populate_bots(&mut self) {
        for s in 0..self.ents.high {
            if self.ents.alive[s] && self.ents.arch[s] != NO_ARCH && self.ents.owner[s] >= DRIVEN {
                self.ents.despawn(s);
            }
        }
        for a in 0..self.archs.len() {
            for _ in 0..self.archs[a].count {
                if self.spawn_as(NONE, a as u8, None).is_none() {
                    return;
                }
            }
        }
    }

    /// Advance the world by one fixed step.
    pub fn step(&mut self, pending: &[PendingAction]) {
        self.tick += 1;
        self.npc_actions.clear();
        self.system_npc_ai();
        self.system_regen();
        self.system_movement();
        self.system_actions(pending);
        let npc = std::mem::take(&mut self.npc_actions);
        self.system_actions(&npc);
        self.npc_actions = npc;
        let adm = std::mem::take(&mut self.admin_actions);
        self.system_actions(&adm);
        self.system_rules();
        self.grid.rebuild(&self.ents);
        for slot in 0..self.ents.high {
            if self.ents.alive[slot] {
                self.entry_cache[slot] = self.entry_of(slot);
            }
        }
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
                continue;
            }
            let a = self.ents.arch[s];
            let (behaviour, sight, attack, attack_range) = match self.archs.get(a as usize) {
                Some(r) => (
                    r.behaviour,
                    r.sight,
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
                self.grid.for_neighbours(x, y, |t| {
                    let t = t as usize;
                    if t == s || e.arch[t] == a && a != NO_ARCH {
                        return;
                    }
                    let (dx, dy) = (e.x[t] - x, e.y[t] - y);
                    let d2 = dx * dx + dy * dy;
                    if d2 <= reach * reach && near.map_or(true, |(b, _)| d2 < b) {
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
            let steer = match behaviour {
                Behaviour::Idle => Some((0, 0)),
                Behaviour::Chase => toward(near.filter(|n| n.0 <= sight * sight), &self.ents, 1.0),
                Behaviour::Flee => toward(near.filter(|n| n.0 <= sight * sight), &self.ents, -1.0),
                Behaviour::Wander => None,
            };
            let (dx, dy) = match steer {
                Some(d) => d,
                None if behaviour == Behaviour::Idle => (0, 0),
                None => {
                    if self.rng.below(4) == 0 {
                        (0, 0)
                    } else {
                        const D: [i8; 3] = [-127, 0, 127];
                        (D[self.rng.below(3) as usize], D[self.rng.below(3) as usize])
                    }
                }
            };
            self.ents.dir_x[s] = dx;
            self.ents.dir_y[s] = dy;
            if let (Some(act), Some((d2, t))) = (attack, near) {
                if d2 <= attack_range * attack_range {
                    self.npc_actions.push(PendingAction { slot: s, action: act, target: self.ents.id_of(t) });
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
            if !e.alive[s] || (e.dir_x[s] == 0 && e.dir_y[s] == 0) {
                continue;
            }
            let (nx, ny) = integrate(&self.world, e.x[s], e.y[s], e.dir_x[s], e.dir_y[s], e.speed[s], dt, r);
            e.x[s] = nx;
            e.y[s] = ny;
        }
    }

    /// Generic action resolution. Every check here is server side: the client
    /// only said "I want to use action N on target T".
    fn system_actions(&mut self, pending: &[PendingAction]) {
        for p in pending {
            let e = &mut self.ents;
            if !e.alive[p.slot] {
                continue;
            }
            let Some(def) = self.defs.action(p.action) else {
                self.stats.rejected_actions += 1;
                continue;
            };
            if self.tick < e.cooldown_until[p.slot] {
                self.stats.rejected_actions += 1;
                continue;
            }
            if let Some((attr, min)) = def.requires {
                if e.attrs[attr as usize][p.slot] < min {
                    self.stats.rejected_actions += 1;
                    continue;
                }
            }
            let Some(t) = e.resolve(p.target) else {
                self.stats.rejected_actions += 1;
                continue;
            };
            if t == p.slot {
                self.stats.rejected_actions += 1;
                continue;
            }
            let dx = e.x[t] - e.x[p.slot];
            let dy = e.y[t] - e.y[p.slot];
            if dx * dx + dy * dy > def.range * def.range {
                self.stats.rejected_actions += 1;
                continue;
            }
            for eff in &def.effects {
                let (attr, delta, who) = match *eff {
                    Effect::AddToTarget { attr, delta } => (attr as usize, delta, t),
                    Effect::AddToSelf { attr, delta } => (attr as usize, delta, p.slot),
                };
                let a = &self.defs.attrs[attr];
                let v = &mut e.attrs[attr][who];
                *v = (*v + delta).clamp(a.min, a.max);
            }
            e.cooldown_until[p.slot] = self.tick + def.cooldown_ticks as u32;
            self.stats.zaps += 1;
        }
    }

    /// Data-driven rules: an attribute that hits its minimum can trigger a
    /// respawn. This is where "death" lives, and it is just a rule on health.
    fn system_rules(&mut self) {
        for a in 0..self.defs.attrs.len() {
            if self.defs.attrs[a].on_zero != OnZero::Respawn {
                continue;
            }
            let min = self.defs.attrs[a].min;
            for s in 0..self.ents.high {
                if self.ents.alive[s] && self.ents.attrs[a][s] <= min {
                    let (x, y) = self.world.random_spawn(&mut self.rng, self.defs.entity_radius);
                    self.ents.x[s] = x;
                    self.ents.y[s] = y;
                    self.reset_attrs(s);
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
        Entry { id: e.id_of(slot), x: qpos(e.x[slot]), y: qpos(e.y[slot]), attrs }
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
    fn zap_in_range_changes_attributes_and_sets_cooldown() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 105.0, 100.0);
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 90.0);
        // Full energy, minus the 5 the zap costs (regen clamps at max before the action runs).
        assert!((s.ents.attrs[ATTR_ENERGY as usize][a] - 95.0).abs() < 1e-3);
        // Cooldown blocks an immediate second zap.
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid }]);
        assert_eq!(s.ents.attrs[ATTR_HEALTH as usize][b], 90.0);
        assert_eq!(s.stats.rejected_actions, 1);
    }

    #[test]
    fn out_of_range_and_bad_targets_are_rejected() {
        let mut s = sim();
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        place(&mut s, a, 100.0, 100.0);
        place(&mut s, b, 200.0, 100.0);
        let tid = s.ents.id_of(b);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid }]);
        assert_eq!(s.ents.attrs[0][b], 100.0);
        // Self target, unknown action, stale generation.
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: s.ents.id_of(a) }]);
        s.step(&[PendingAction { slot: a, action: 99, target: tid }]);
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid ^ (1 << 24) }]);
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
        s.step(&[PendingAction { slot: a, action: ACTION_ZAP, target: tid }]);
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
}

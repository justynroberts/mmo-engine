//! Inventories, items on the ground, using and dropping, and loot.
//!
//! Everything is decided here on the server. A client says "use slot 3" through the ordinary input action byte
//! and is told what its bag and its surroundings look like; it never says what it has.

use crate::sim::{Sim, DRIVEN, NO_ARCH};
use proto::game::{ItemRt, ACT_DROP, ACT_PICKUP, ACT_USE, INV_SLOTS, PICKUP_RANGE};

/// One bag slot: (item index + 1, count). Item 0 means the slot is empty.
pub type Bag = [(u8, u8); INV_SLOTS];

pub const GROUND_CAP: usize = 1000;
/// How long something lies there before it fades away.
pub const GROUND_LIFE_SECS: u32 = 300;
/// Ticks between item actions by one character, so a held key cannot flood.
pub const ITEM_CD_TICKS: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct GroundItem {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub item: u8,
    pub count: u8,
    pub born: u32,
}

impl Sim {
    pub fn clear_inv(&mut self, slot: usize) {
        self.inv[slot] = [(0, 0); INV_SLOTS];
        self.bump_inv(slot);
    }

    fn bump_inv(&mut self, slot: usize) {
        self.inv_ver[slot] = self.inv_ver[slot].wrapping_add(1);
    }

    /// Give items, and what does not fit in the bag lands at their feet. Returns how many went into the bag.
    pub fn give_or_drop(&mut self, slot: usize, item: usize, count: u32) -> u32 {
        let left = self.give(slot, item, count);
        if left > 0 {
            let (x, y) = (self.ents.x[slot], self.ents.y[slot]);
            let stack = self.items[item].stack_max.max(1) as u32;
            for n in (0..left.div_ceil(stack)).map(|k| (left - k * stack).min(stack)) {
                self.put_ground(x, y, item, n as u8);
            }
        }
        count - left
    }

    /// Add to a bag, topping up existing stacks first. Returns how many did not fit.
    pub fn give(&mut self, slot: usize, item: usize, count: u32) -> u32 {
        let Some(def) = self.items.get(item) else { return count };
        let max = def.stack_max as u32;
        let tag = item as u8 + 1;
        let mut left = count;
        for s in 0..INV_SLOTS {
            if left == 0 {
                break;
            }
            let (t, c) = self.inv[slot][s];
            if t == tag && (c as u32) < max {
                let add = (max - c as u32).min(left);
                self.inv[slot][s].1 = (c as u32 + add) as u8;
                left -= add;
            }
        }
        for s in 0..INV_SLOTS {
            if left == 0 {
                break;
            }
            if self.inv[slot][s].0 == 0 {
                let add = max.min(left);
                self.inv[slot][s] = (tag, add as u8);
                left -= add;
            }
        }
        if left != count {
            self.bump_inv(slot);
        }
        left
    }

    pub fn put_ground(&mut self, x: f32, y: f32, item: usize, count: u8) {
        if count == 0 || item >= self.items.len() {
            return;
        }
        if self.ground.len() >= GROUND_CAP {
            self.ground.remove(0);
        }
        let id = self.next_ground;
        self.next_ground = self.next_ground.wrapping_add(1).max(1);
        let size = self.world.size;
        self.ground.push(GroundItem { id, x: x.clamp(0.0, size), y: y.clamp(0.0, size), item: item as u8, count, born: self.tick });
    }

    /// Put `piles` single items down, spread over a circle (or anywhere in the world).
    pub fn scatter_ground(&mut self, item: usize, piles: u32, area: Option<(f32, f32, f32)>) {
        let r = self.defs.entity_radius;
        for _ in 0..piles {
            let (x, y) = match area {
                Some((cx, cy, cr)) => {
                    let (a, d) = (self.rng.f32() * std::f32::consts::TAU, cr * self.rng.f32().sqrt());
                    (cx + a.cos() * d, cy + a.sin() * d)
                }
                None => self.world.random_spawn(&mut self.rng, r),
            };
            self.put_ground(x, y, item, 1);
        }
    }

    /// What a bot of this type leaves behind, rolled once at the spot where it fell.
    pub fn loot_from(&mut self, slot: usize) {
        let a = self.ents.arch[slot];
        if a == NO_ARCH {
            return;
        }
        let drops = self.archs[a as usize].drops.clone();
        let (x, y) = (self.ents.x[slot], self.ents.y[slot]);
        for d in drops {
            if self.rng.f32() * 100.0 >= d.chance_pct {
                continue;
            }
            let span = (d.max - d.min) as u32 + 1;
            let n = d.min as u32 + self.rng.next_u32() % span;
            let (jx, jy) = ((self.rng.f32() - 0.5) * 2.0, (self.rng.f32() - 0.5) * 2.0);
            self.put_ground(x + jx, y + jy, d.item, n.min(99) as u8);
        }
    }

    /// Use, drop or pick up, as the action byte says. A pickup takes the pile `target` names (a ground item id) when
    /// it is in reach, or the nearest one in reach when `target` is 0. Returns false if the action was not an item
    /// action.
    pub fn item_action(&mut self, slot: usize, action: u8, target: u32) -> bool {
        let n = INV_SLOTS as u8;
        let kind = if (ACT_USE..ACT_USE + n).contains(&action) {
            0
        } else if (ACT_DROP..ACT_DROP + n).contains(&action) {
            1
        } else if action == ACT_PICKUP {
            2
        } else {
            return false;
        };
        if self.tick < self.item_cd[slot] {
            return true;
        }
        let ok = match kind {
            0 => self.use_slot(slot, (action - ACT_USE) as usize),
            1 => self.drop_slot(slot, (action - ACT_DROP) as usize),
            _ => self.pick_up(slot, target),
        };
        if ok {
            self.item_cd[slot] = self.tick + ITEM_CD_TICKS;
            self.lived(slot, crate::life::Deed::Deed);
        }
        true
    }

    fn use_slot(&mut self, slot: usize, i: usize) -> bool {
        let (tag, count) = self.inv[slot][i];
        if tag == 0 {
            return false;
        }
        let def: ItemRt = self.items[tag as usize - 1].clone();
        if def.equip.is_some() {
            return self.equip(slot, i);
        }
        if def.mount > 0.0 {
            let ok = self.ride(slot, i);
            self.gear_ver[slot] = self.gear_ver[slot].wrapping_add(1);
            return ok;
        }
        if def.uses.is_empty() {
            return false;
        }
        for &(attr, delta) in &def.uses {
            let a = &self.defs.attrs[attr];
            let v = &mut self.ents.attrs[attr][slot];
            *v = (*v + delta).clamp(a.min, a.max);
        }
        if def.consume {
            self.inv[slot][i] = if count <= 1 { (0, 0) } else { (tag, count - 1) };
        }
        self.bump_inv(slot);
        true
    }

    fn drop_slot(&mut self, slot: usize, i: usize) -> bool {
        let (tag, count) = self.inv[slot][i];
        if tag == 0 {
            return false;
        }
        self.inv[slot][i] = (0, 0);
        self.bump_inv(slot);
        let (x, y) = (self.ents.x[slot], self.ents.y[slot]);
        let (jx, jy) = ((self.rng.f32() - 0.5) * 1.5, (self.rng.f32() - 0.5) * 1.5);
        self.put_ground(x + jx, y + jy, tag as usize - 1, count);
        true
    }

    fn pick_up(&mut self, slot: usize, target: u32) -> bool {
        let (px, py) = (self.ents.x[slot], self.ents.y[slot]);
        let mut best: Option<(f32, usize)> = None;
        for (k, g) in self.ground.iter().enumerate() {
            let d2 = (g.x - px).powi(2) + (g.y - py).powi(2);
            if target != 0 && g.id != target {
                continue;
            }
            if d2 <= PICKUP_RANGE * PICKUP_RANGE && best.is_none_or(|(b, _)| d2 < b) {
                best = Some((d2, k));
            }
        }
        let Some((_, k)) = best else { return false };
        let (item, count) = (self.ground[k].item as usize, self.ground[k].count as u32);
        let left = self.give(slot, item, count);
        if left == count {
            return false; // bag full
        }
        if left == 0 {
            self.ground.swap_remove(k);
        } else {
            self.ground[k].count = left as u8;
        }
        true
    }

    /// Let go of things that have lain about too long.
    pub fn expire_ground(&mut self) {
        let life = GROUND_LIFE_SECS * self.tick_hz;
        let now = self.tick;
        if self.ground.iter().any(|g| now.wrapping_sub(g.born) > life) {
            self.ground.retain(|g| now.wrapping_sub(g.born) <= life);
        }
    }

    /// A new item list (the game file changed without changing the wire): keep what still exists, by name.
    pub fn set_items(&mut self, new: Vec<ItemRt>) {
        let map: Vec<Option<u8>> = self.items.iter().map(|o| new.iter().position(|n| n.name == o.name).map(|i| i as u8 + 1)).collect();
        let fix = |tag: u8, count: u8, new: &[ItemRt]| -> (u8, u8) {
            match map.get((tag as usize).wrapping_sub(1)).copied().flatten() {
                Some(t) => (t, count.min(new[t as usize - 1].stack_max)),
                None => (0, 0),
            }
        };
        for s in 0..self.inv.len() {
            let mut changed = false;
            for i in 0..INV_SLOTS {
                let (t, c) = self.inv[s][i];
                if t != 0 {
                    let n = fix(t, c, &new);
                    changed |= n != (t, c);
                    self.inv[s][i] = n;
                }
            }
            if changed {
                self.bump_inv(s);
            }
        }
        let mut g = std::mem::take(&mut self.ground);
        g.retain_mut(|x| {
            let (t, c) = fix(x.item + 1, x.count, &new);
            x.item = t.wrapping_sub(1);
            x.count = c;
            t != 0
        });
        self.ground = g;
        self.items = new;
    }

    /// The bag as names, for saving.
    pub fn inv_names(&self, slot: usize) -> Vec<(String, u8)> {
        self.inv[slot].iter().filter(|(t, _)| *t != 0).map(|&(t, c)| (self.items[t as usize - 1].name.clone(), c)).collect()
    }

    /// Put a saved bag back; names that no longer exist are dropped, counts are clamped.
    pub fn restore_inv(&mut self, slot: usize, saved: &[(String, u8)]) {
        self.inv[slot] = [(0, 0); INV_SLOTS];
        for (name, c) in saved {
            if let Some(i) = self.items.iter().position(|x| &x.name == name) {
                self.give(slot, i, *c as u32);
            }
        }
        self.bump_inv(slot);
    }

    pub fn is_bot(&self, slot: usize) -> bool {
        self.ents.owner[slot] >= DRIVEN && self.ents.arch[slot] != NO_ARCH
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::PendingAction;
    use proto::game::GameFile;
    use proto::world::World;

    fn sim_with_items() -> Sim {
        let mut g = GameFile::default_game();
        g.items = vec![
            proto::game::ItemDef { name: "medkit".into(), color: "#ff4466".into(), stack_max: 3, uses: vec![proto::game::UseEffect { attr: "health".into(), delta: 40.0 }], consume: true, value: 0, equip: None, mount: 0.0 },
            proto::game::ItemDef { name: "scrap".into(), color: "#aaaaaa".into(), stack_max: 10, uses: vec![], consume: true, value: 0, equip: None, mount: 0.0 },
        ];
        for a in &mut g.archetypes {
            a.drops.clear();
        }
        let game = g.build().unwrap();
        Sim::new(game, World::from_boxes(512.0, 1, vec![]), 64, 20, 1)
    }

    fn act(s: &mut Sim, slot: usize, a: u8) {
        s.step(&[PendingAction { slot, action: a, target: 0, arg: 0 }]);
        for _ in 0..ITEM_CD_TICKS {
            s.step(&[]);
        }
    }

    #[test]
    fn stacks_fill_then_spill_and_overflow_is_reported() {
        let mut s = sim_with_items();
        let p = s.spawn(0).unwrap();
        assert_eq!(s.give(p, 0, 7), 0);
        assert_eq!(&s.inv[p][..3], &[(1, 3), (1, 3), (1, 1)]);
        assert_eq!(s.give(p, 0, 2), 0);
        assert_eq!(s.inv[p][2], (1, 3));
        // 8 slots x 3 = 24 medkits, 9 are in.
        assert_eq!(s.give(p, 0, 20), 20 - 15);
    }

    #[test]
    fn using_heals_consumes_and_clamps_and_an_empty_use_does_nothing() {
        let mut s = sim_with_items();
        let p = s.spawn(0).unwrap();
        let hp = proto::defs::ATTR_HEALTH as usize;
        s.ents.attrs[hp][p] = 30.0;
        s.give(p, 0, 2);
        s.give(p, 1, 1);
        act(&mut s, p, ACT_USE);
        assert!((s.ents.attrs[hp][p] - 70.0).abs() < 1.0, "{}", s.ents.attrs[hp][p]);
        assert_eq!(s.inv[p][0], (1, 1));
        act(&mut s, p, ACT_USE + 1); // scrap has no use
        assert_eq!(s.inv[p][1], (2, 1));
        act(&mut s, p, ACT_USE + 5); // empty slot
        act(&mut s, p, ACT_USE);
        act(&mut s, p, ACT_USE);
        assert_eq!(s.inv[p][0], (0, 0));
        assert!(s.ents.attrs[hp][p] <= 100.0);
    }

    #[test]
    fn dropping_puts_it_on_the_ground_and_pickup_takes_the_nearest_in_reach_only() {
        let mut s = sim_with_items();
        let p = s.spawn(0).unwrap();
        s.ents.x[p] = 100.0;
        s.ents.y[p] = 100.0;
        s.give(p, 1, 4);
        act(&mut s, p, ACT_DROP + 0);
        assert_eq!(s.inv[p][0], (0, 0));
        assert_eq!(s.ground.len(), 1);
        s.put_ground(300.0, 300.0, 1, 1); // far away: out of reach
        act(&mut s, p, ACT_PICKUP);
        assert_eq!(s.inv[p][0], (2, 4));
        assert_eq!(s.ground.len(), 1);
        act(&mut s, p, ACT_PICKUP);
        assert_eq!(s.ground.len(), 1, "the far one stays");
    }

    #[test]
    fn pickup_can_name_the_pile_it_wants() {
        let mut s = sim_with_items();
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (100.0, 100.0);
        s.put_ground(100.5, 100.0, 0, 1); // the nearer one
        s.put_ground(102.0, 100.0, 1, 2);
        let want = s.ground[1].id;
        s.step(&[PendingAction { slot: p, action: ACT_PICKUP, target: want, arg: 0 }]);
        assert_eq!(s.inv[p][0], (2, 2), "the one asked for, not the nearest");
        assert_eq!(s.ground.len(), 1);
        // A pile out of reach is not taken, even when named.
        for _ in 0..ITEM_CD_TICKS {
            s.step(&[]);
        }
        s.put_ground(140.0, 100.0, 1, 1);
        let far = s.ground.last().unwrap().id;
        s.step(&[PendingAction { slot: p, action: ACT_PICKUP, target: far, arg: 0 }]);
        assert_eq!(s.ground.len(), 2);
    }

    #[test]
    fn a_full_bag_leaves_the_item_where_it_is() {
        let mut s = sim_with_items();
        let p = s.spawn(0).unwrap();
        s.ents.x[p] = 50.0;
        s.ents.y[p] = 50.0;
        s.give(p, 0, 24);
        s.put_ground(50.0, 50.0, 0, 1);
        act(&mut s, p, ACT_PICKUP);
        assert_eq!(s.ground.len(), 1);
    }

    #[test]
    fn items_fade_and_the_ground_is_capped() {
        let mut s = sim_with_items();
        s.put_ground(1.0, 1.0, 0, 1);
        s.tick += GROUND_LIFE_SECS * 20 + 1;
        s.expire_ground();
        assert!(s.ground.is_empty());
        for _ in 0..GROUND_CAP + 50 {
            s.put_ground(1.0, 1.0, 0, 1);
        }
        assert_eq!(s.ground.len(), GROUND_CAP);
    }

    #[test]
    fn bags_save_by_name_and_survive_a_changed_item_list() {
        let mut s = sim_with_items();
        let p = s.spawn(0).unwrap();
        s.give(p, 0, 2);
        s.give(p, 1, 5);
        let names = s.inv_names(p);
        assert_eq!(names, vec![("medkit".to_string(), 2), ("scrap".to_string(), 5)]);
        let q = s.spawn(1).unwrap();
        s.restore_inv(q, &[("scrap".into(), 200), ("ghost".into(), 1)]);
        let q_names = s.inv_names(q);
        assert_eq!(q_names.len(), 8, "a bag only holds 8 stacks and unknown names are dropped");
        assert!(q_names.iter().all(|(n, c)| n == "scrap" && *c == 10));
        // Reorder the list and drop medkit: scrap follows its name, medkit vanishes.
        let mut g = GameFile::default_game();
        for a in &mut g.archetypes {
            a.drops.clear();
        }
        g.items = vec![proto::game::ItemDef { name: "scrap".into(), color: "#aaaaaa".into(), stack_max: 4, uses: vec![], consume: true, value: 0, equip: None, mount: 0.0 }];
        s.put_ground(5.0, 5.0, 0, 1);
        s.put_ground(6.0, 6.0, 1, 3);
        s.set_items(g.build().unwrap().items);
        assert_eq!(s.inv_names(p), vec![("scrap".to_string(), 4)]);
        assert_eq!(s.ground.len(), 1);
        assert_eq!((s.ground[0].item, s.ground[0].count), (0, 3));
    }

    #[test]
    fn a_bot_that_falls_leaves_its_drops_at_its_feet() {
        let mut g = GameFile::default_game();
        g.items = vec![proto::game::ItemDef { name: "scrap".into(), color: "#aaaaaa".into(), stack_max: 10, uses: vec![], consume: true, value: 0, equip: None, mount: 0.0 }];
        for a in &mut g.archetypes {
            a.drops.clear();
        }
        let name = g.archetypes[0].name.clone();
        g.archetypes[0].drops = vec![proto::game::DropDef { item: "scrap".into(), chance_pct: 100.0, min: 2, max: 4 }];
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let a = s.archs.iter().position(|x| x.name == name).unwrap() as u8;
        let b = s.spawn_as(crate::sim::NONE, a, Some((200.0, 200.0))).unwrap();
        s.ents.attrs[proto::defs::ATTR_HEALTH as usize][b] = 0.0;
        s.step(&[]);
        assert_eq!(s.ground.len(), 1);
        assert!((2..=4).contains(&s.ground[0].count));
    }
}

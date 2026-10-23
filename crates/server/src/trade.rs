//! Shops, gear and mounts.
//!
//! A bot type with a `shop` sells those items for the game's `currency`, and buys any item with a `value`. Gear
//! (an item with `equip`) is put on by using it, into its slot (weapon, armour, trinket), and adds its might and
//! toughness to the wearer's level's. A mount (an item with `mount`) is ridden by using it: it adds to the
//! `speed` attribute until the rider uses it again, uses an action, or is hurt. All of it is checked here; a
//! client only says which item and whom.

use crate::sim::{Sim, DRIVEN};
use proto::game::{GEAR_SLOTS, INV_SLOTS, TALK_RANGE};

impl Sim {
    fn bump_gear(&mut self, s: usize) {
        self.gear_ver[s] = self.gear_ver[s].wrapping_add(1);
    }

    /// The merchant `target` (an entity id) if it is a bot with a shop within talking range of player `s`.
    fn merchant(&self, s: usize, target: u32) -> Option<usize> {
        let t = self.ents.resolve(target)?;
        let (dx, dy) = (self.ents.x[t] - self.ents.x[s], self.ents.y[t] - self.ents.y[s]);
        let shop = self.archs.get(self.ents.arch[t] as usize).is_some_and(|a| !a.shop.is_empty());
        (shop && dx * dx + dy * dy <= TALK_RANGE * TALK_RANGE && self.ents.owner[s] < DRIVEN).then_some(t)
    }

    /// How much money a player carries.
    pub fn money(&self, s: usize) -> u32 {
        let Some(c) = self.currency else { return 0 };
        self.inv[s].iter().filter(|b| b.0 as usize == c + 1).map(|b| b.1 as u32).sum()
    }

    fn pay(&mut self, s: usize, mut n: u32) {
        let Some(c) = self.currency else { return };
        for b in self.inv[s].iter_mut() {
            if n == 0 {
                break;
            }
            if b.0 as usize == c + 1 {
                let t = (b.1 as u32).min(n);
                b.1 -= t as u8;
                n -= t;
                if b.1 == 0 {
                    b.0 = 0;
                }
            }
        }
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
    }

    /// Buy item `index` of the merchant's shop. False if it cannot be (out of reach, too dear, a full bag).
    pub fn buy(&mut self, s: usize, target: u32, index: u16) -> bool {
        let (Some(t), Some(_)) = (self.merchant(s, target), self.currency) else { return false };
        if let Some(why) = self.refuses(t, s) {
            self.tells.push((s, why));
            return false;
        }
        let Some(&(item, price)) = self.archs[self.ents.arch[t] as usize].shop.get(index as usize) else { return false };
        if self.money(s) < price as u32 {
            self.tells.push((s, "You cannot afford that.".into()));
            return false;
        }
        if self.give(s, item, 1) > 0 {
            self.tells.push((s, "Your bag is full.".into()));
            return false;
        }
        self.pay(s, price as u32);
        self.tells.push((s, format!("Bought {} for {price}.", self.items[item].name)));
        true
    }

    /// Sell one of what is in bag slot `i` to the merchant (for the item's value). False if they will not buy it.
    pub fn sell(&mut self, s: usize, target: u32, i: u16) -> bool {
        let (Some(t), Some(c)) = (self.merchant(s, target), self.currency) else { return false };
        if let Some(why) = self.refuses(t, s) {
            self.tells.push((s, why));
            return false;
        }
        let Some(&(tag, count)) = self.inv[s].get(i as usize) else { return false };
        if tag == 0 || tag as usize - 1 == c {
            return false;
        }
        let value = self.items[tag as usize - 1].value;
        if value == 0 {
            self.tells.push((s, "They will not buy that.".into()));
            return false;
        }
        self.inv[s][i as usize] = if count <= 1 { (0, 0) } else { (tag, count - 1) };
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
        self.give_or_drop(s, c, value as u32);
        self.tells.push((s, format!("Sold {} for {value}.", self.items[tag as usize - 1].name)));
        true
    }

    /// Put on the gear in bag slot `i`; what was worn there goes back into the bag in its place.
    pub fn equip(&mut self, s: usize, i: usize) -> bool {
        let (tag, count) = self.inv[s][i];
        let Some(e) = self.items.get(tag as usize - 1).and_then(|d| d.equip.clone()) else { return false };
        let g = e.slot as usize;
        let was = self.gear[s][g];
        self.gear[s][g] = tag;
        self.inv[s][i] = if count <= 1 { (0, 0) } else { (tag, count - 1) };
        if was != 0 {
            if self.inv[s][i].0 == 0 {
                self.inv[s][i] = (was, 1);
            } else {
                self.give_or_drop(s, was as usize - 1, 1);
            }
        }
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
        self.bump_gear(s);
        true
    }

    /// Take off what is worn in gear slot `g`, back into the bag.
    pub fn unequip(&mut self, s: usize, g: u16) -> bool {
        let g = g as usize;
        if g >= GEAR_SLOTS || self.gear[s][g] == 0 {
            return false;
        }
        let tag = self.gear[s][g];
        if self.give(s, tag as usize - 1, 1) > 0 {
            self.tells.push((s, "Your bag is full.".into()));
            return false;
        }
        self.gear[s][g] = 0;
        self.bump_gear(s);
        true
    }

    /// What a player's gear adds to their might and toughness.
    pub fn gear_bonus(&self, s: usize) -> (f32, f32) {
        self.gear[s].iter().filter(|&&t| t != 0).filter_map(|&t| self.items.get(t as usize - 1)?.equip.as_ref()).fold((0.0, 0.0), |a, e| (a.0 + e.might, a.1 + e.toughness))
    }

    /// Ride the mount in bag slot `i`, or get down if already riding.
    pub fn ride(&mut self, s: usize, i: usize) -> bool {
        let Some(attr) = self.defs.attrs.iter().position(|a| a.name == proto::game::SPEED_ATTR) else {
            self.tells.push((s, "There is nothing to ride in this world.".into()));
            return false;
        };
        if self.mounted[s] > 0.0 {
            self.dismount(s);
            return true;
        }
        let tag = self.inv[s][i].0;
        let Some(bonus) = self.items.get(tag as usize - 1).map(|d| d.mount).filter(|&m| m > 0.0) else { return false };
        let a = &self.defs.attrs[attr];
        let v = &mut self.ents.attrs[attr][s];
        let before = *v;
        *v = (*v + bonus).clamp(a.min, a.max);
        self.mounted[s] = *v - before;
        true
    }

    /// Get down from a mount (riding into a fight, or hurt).
    pub fn dismount(&mut self, s: usize) {
        if self.mounted[s] <= 0.0 {
            return;
        }
        if let Some(attr) = self.defs.attrs.iter().position(|a| a.name == proto::game::SPEED_ATTR) {
            let a = &self.defs.attrs[attr];
            let v = &mut self.ents.attrs[attr][s];
            *v = (*v - self.mounted[s]).clamp(a.min, a.max);
        }
        self.mounted[s] = 0.0;
    }

    /// Gear as the wire carries it (`wire::kind::GEAR`): item index + 1 per slot, 0 for nothing worn.
    pub fn gear_of(&self, s: usize) -> [u8; GEAR_SLOTS] {
        self.gear[s]
    }

    /// Gear by item name, for saving.
    pub fn gear_names(&self, s: usize) -> Vec<String> {
        self.gear[s].iter().map(|&t| if t == 0 { String::new() } else { self.items.get(t as usize - 1).map_or(String::new(), |d| d.name.clone()) }).collect()
    }

    pub fn restore_gear(&mut self, s: usize, names: &[String]) {
        for (g, n) in names.iter().enumerate().take(GEAR_SLOTS) {
            self.gear[s][g] = self.items.iter().position(|d| &d.name == n && d.equip.as_ref().is_some_and(|e| e.slot as usize == g)).map_or(0, |k| k as u8 + 1);
        }
        self.bump_gear(s);
    }
}

/// Bag slots are what selling takes, so they must fit the action's argument.
const _: () = assert!(INV_SLOTS < 256);

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim, NONE};
    use proto::game::{AttrFile, EquipFile, GameFile, GearSlot, ItemDef, OnZeroFile, ShopItem, ACT_BUY, ACT_SELL, ACT_USE};
    use proto::world::World;

    fn item(name: &str, value: u16) -> ItemDef {
        ItemDef { name: name.into(), color: "#ffffff".into(), stack_max: 50, uses: vec![], consume: true, value, equip: None, mount: 0.0 }
    }

    #[test]
    fn buy_sell_wear_and_ride() {
        let mut g = GameFile::default_game();
        g.items.push(item("silver", 0));
        g.items.push(ItemDef { equip: Some(EquipFile { slot: GearSlot::Weapon, might: 0.5, toughness: 0.0 }), ..item("sword", 4) });
        g.items.push(ItemDef { mount: 0.8, ..item("horse", 0) });
        g.attrs.push(AttrFile { name: "speed".into(), min: 0.5, max: 3.0, default: 1.0, bits: 8, replicated: false, regen_per_sec: 0.0, on_zero: OnZeroFile::None });
        g.currency = Some("silver".into());
        let merchant = g.archetypes.iter().position(|a| a.name == "merchant").unwrap();
        g.archetypes[merchant].shop = vec![ShopItem { item: "sword".into(), price: 10 }];
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let (silver, sword, horse) = (s.items.iter().position(|i| i.name == "silver").unwrap(), s.items.iter().position(|i| i.name == "sword").unwrap(), s.items.iter().position(|i| i.name == "horse").unwrap());
        let p = s.spawn(0).unwrap();
        let m = s.spawn_as(NONE, merchant as u8, Some((100.0, 104.0))).unwrap();
        let foe = s.spawn(1).unwrap();
        s.ents.x[p] = 100.0;
        s.ents.y[p] = 100.0;
        s.ents.x[m] = 100.0;
        s.ents.y[m] = 104.0;
        let mid = s.ents.id_of(m);
        let act = |s: &mut Sim, action: u8, target: u32, arg: u16| {
            s.item_cd[p] = 0;
            s.step(&[PendingAction { slot: p, action, target, arg }]);
        };
        // Too poor, then rich enough: the sword for 10 silver.
        act(&mut s, ACT_BUY, mid, 0);
        assert_eq!(s.inv[p].iter().filter(|b| b.0 as usize == sword + 1).count(), 0);
        s.give(p, silver, 12);
        act(&mut s, ACT_BUY, mid, 0);
        assert_eq!(s.money(p), 2);
        let at = s.inv[p].iter().position(|b| b.0 as usize == sword + 1).unwrap();
        // Worn, it hits half as hard again.
        act(&mut s, ACT_USE + at as u8, 0, 0);
        assert_eq!(s.gear[p][0] as usize, sword + 1);
        assert!((s.scaled(p, foe, 0, -10.0) + 15.0).abs() < 1e-4);
        // Sold back (take it off first) for its value.
        act(&mut s, proto::game::ACT_UNEQUIP, 0, 0);
        let at = s.inv[p].iter().position(|b| b.0 as usize == sword + 1).unwrap();
        act(&mut s, ACT_SELL, mid, at as u16);
        assert_eq!(s.money(p), 6);
        // Riding adds to speed until hurt.
        s.give(p, horse, 1);
        let at = s.inv[p].iter().position(|b| b.0 as usize == horse + 1).unwrap();
        act(&mut s, ACT_USE + at as u8, 0, 0);
        let sp = s.defs.attrs.iter().position(|a| a.name == "speed").unwrap();
        assert!((s.ents.attrs[sp][p] - 1.8).abs() < 1e-4);
        s.ents.attrs[0][p] = 100.0;
        let fid = s.ents.id_of(foe);
        s.add_to(p, 0, -5.0, fid);
        assert!((s.ents.attrs[sp][p] - 1.0).abs() < 1e-4, "knocked off: {}", s.ents.attrs[sp][p]);
        // Saved by name.
        assert_eq!(s.record_of(p).gear, vec![String::new(), String::new(), String::new()]);
    }

    #[test]
    fn a_party_forms_shares_a_kill_and_ends_when_one_is_left() {
        let mut g = GameFile::default_game();
        g.levels = Some(proto::game::LevelsFile { cap: 10, base: 1000.0, exponent: 1.0, might: 0.0, toughness: 0.0, discover_xp: 0 });
        g.archetypes[0].xp = 30;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let [a, b] = [0, 1].map(|o| s.spawn(o).unwrap());
        s.next_party = 7;
        s.party[a] = 7;
        s.party[b] = 7;
        assert_eq!(s.party_of(a).len(), 2);
        // a hurts a bot; b, in the party and near, shares the experience.
        let wolf = s.spawn_as(NONE, 0, None).unwrap();
        for (e, x) in [(a, 100.0), (b, 120.0), (wolf, 104.0)] {
            s.ents.x[e] = x;
            s.ents.y[e] = 100.0;
        }
        let (aid, now) = (s.ents.id_of(a), s.tick);
        s.ents.hurt_by[wolf].push((aid, now));
        s.ents.attrs[0][wolf] = 0.0;
        s.step(&[]);
        assert_eq!((s.ents.xp[a], s.ents.xp[b]), (30, 30));
        s.leave_party(a);
        assert_eq!((s.party[a], s.party[b]), (0, 0), "a party of one is no party");
    }
}

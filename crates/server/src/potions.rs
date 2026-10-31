//! Potions: an item with `potion` (`proto::game::PotionFile`) is drunk like any other (its `uses` at once) and then
//! stays on the drinker for its `secs`: harder blows and lighter wounds (added in `Sim::scaled`), more speed (added
//! to the `speed` attribute and taken off again), and its `every` changes each second. Drinking the same potion again
//! starts it afresh; different potions add up. `rules.potion_cooldown_secs` must pass between two drinks. A fall ends
//! them all. What a player has on them goes to them on chat channel 21 when it changes:
//! "P0|title,colour,secs left;..." ("P0|" for none).

use crate::sim::Sim;

/// A potion working on someone: its item, the tick it ends, and the speed it added.
#[derive(Clone, Debug, PartialEq)]
pub struct Draught {
    pub item: usize,
    pub until: u32,
    pub sped: f32,
}

impl Sim {
    /// Drink the potion in bag slot `i`. False if it is not one or cannot be drunk yet.
    pub(crate) fn drink(&mut self, s: usize, i: usize) -> bool {
        let (tag, count) = self.inv[s][i];
        let item = tag as usize - 1;
        let Some(p) = self.items.get(item).and_then(|d| d.potion.clone()) else { return false };
        if self.ents.fallen[s] {
            return false;
        }
        if self.tick < self.potion_at[s] && !self.ents.god[s] {
            let left = (self.potion_at[s] - self.tick) as f32 / self.tick_hz as f32;
            self.tells.push((s, format!("You cannot drink another yet ({left:.0} s).")));
            return false;
        }
        self.potion_at[s] = self.tick + proto::rules::Rules::ticks(self.rules.potion_cooldown_secs, self.tick_hz);
        if let Some(k) = self.draughts[s].iter().position(|d| d.item == item) {
            let old = self.draughts[s].remove(k);
            self.unspeed(s, old.sped);
        }
        let sped = self.speed_by(s, p.speed);
        self.draughts[s].push(Draught { item, until: self.tick + proto::rules::Rules::ticks(p.secs, self.tick_hz), sped });
        for (attr, delta) in self.items[item].uses.clone() {
            let a = &self.defs.attrs[attr];
            let v = &mut self.ents.attrs[attr][s];
            *v = (*v + delta).clamp(a.min, a.max);
        }
        if self.items[item].consume {
            self.inv[s][i] = if count <= 1 { (0, 0) } else { (tag, count - 1) };
        }
        self.bump_inv(s);
        self.tells.push((s, format!("You drink the {} ({:.0} s).", self.items[item].shown(), p.secs)));
        true
    }

    /// Add `by` to the player's speed attribute, within its range; what was added.
    fn speed_by(&mut self, s: usize, by: f32) -> f32 {
        if by <= 0.0 {
            return 0.0;
        }
        let Some(attr) = self.defs.attrs.iter().position(|a| a.name == proto::game::SPEED_ATTR) else { return 0.0 };
        let a = &self.defs.attrs[attr];
        let v = &mut self.ents.attrs[attr][s];
        let before = *v;
        *v = (*v + by).clamp(a.min, a.max);
        *v - before
    }

    fn unspeed(&mut self, s: usize, by: f32) {
        if by <= 0.0 {
            return;
        }
        if let Some(attr) = self.defs.attrs.iter().position(|a| a.name == proto::game::SPEED_ATTR) {
            let a = &self.defs.attrs[attr];
            let v = &mut self.ents.attrs[attr][s];
            *v = (*v - by).clamp(a.min, a.max);
        }
    }

    /// What the potions on a player add, as (might, toughness) shares.
    pub fn potion_bonus(&self, s: usize) -> (f32, f32) {
        self.draughts[s].iter().filter_map(|d| self.items.get(d.item)?.potion.as_ref()).fold((0.0, 0.0), |a, p| (a.0 + p.might, a.1 + p.toughness))
    }

    /// Once a second: each potion's `every`, an end to those run out (and to all of a fallen player's), and each
    /// player told what they have on them when it changes.
    pub fn system_potions(&mut self) {
        for s in 0..self.ents.high {
            if self.draughts[s].is_empty() && self.draughts_sent[s].is_empty() {
                continue;
            }
            if !self.ents.alive[s] {
                self.draughts[s].clear();
                self.draughts_sent[s].clear();
                continue;
            }
            let now = self.tick;
            let fallen = self.ents.fallen[s];
            let mut k = 0;
            while k < self.draughts[s].len() {
                let d = self.draughts[s][k].clone();
                if fallen || now >= d.until || d.item >= self.items.len() {
                    self.draughts[s].remove(k);
                    self.unspeed(s, d.sped);
                    continue;
                }
                if let Some(p) = self.items[d.item].potion.clone() {
                    for (attr, delta) in p.every {
                        if attr == proto::defs::ATTR_HEALTH as usize && delta < 0.0 {
                            self.add_to(s, attr, delta, 0);
                        } else {
                            let a = &self.defs.attrs[attr];
                            let v = &mut self.ents.attrs[attr][s];
                            *v = (*v + delta).clamp(a.min, a.max);
                        }
                    }
                }
                k += 1;
            }
            // Told only when the set changes: the client counts the seconds down itself.
            let key: Vec<String> = self.draughts[s].iter().map(|d| format!("{}@{}", d.item, d.until)).collect();
            let key = key.join(";");
            if key == self.draughts_sent[s] {
                continue;
            }
            self.draughts_sent[s] = key;
            let hz = self.tick_hz as f32;
            let list: Vec<String> = self.draughts[s]
                .iter()
                .map(|d| format!("{},{},{:.0}", self.items[d.item].shown().replace([',', ';', '|'], " "), self.items[d.item].color, (d.until - now) as f32 / hz))
                .collect();
            self.notes.push((s, 0, proto::chat::channel::POTIONS, format!("P0|{}", list.join(";"))));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::{GameFile, ItemDef, PotionFile, UseEffect};
    use proto::world::World;

    fn potion(name: &str, p: PotionFile) -> ItemDef {
        ItemDef { name: name.into(), color: "#aa3366".into(), stack_max: 5, uses: vec![], consume: true, value: 0, equip: None, mount: 0.0, rarity: Default::default(), title: String::new(), block: None, dye: None, furniture: None, look: None, potion: Some(p) }
    }

    #[test]
    fn a_potion_lasts_its_time_adds_its_strength_and_heals_each_second_with_a_wait_between_drinks() {
        let mut g = GameFile::default_game();
        g.items.push(potion("might", PotionFile { secs: 3.0, might: 0.5, ..Default::default() }));
        g.items.push(potion("renewal", PotionFile { secs: 5.0, every: vec![UseEffect { attr: "health".into(), delta: 4.0 }], ..Default::default() }));
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let might = s.items.iter().position(|d| d.name == "might").unwrap();
        let renewal = s.items.iter().position(|d| d.name == "renewal").unwrap();
        let p = s.spawn(0).unwrap();
        let foe = s.spawn(1).unwrap();
        s.give(p, might, 1);
        s.give(p, renewal, 1);
        let before = s.scaled(p, foe, 0, -10.0);
        let slot = |s: &Sim, item: usize| s.inv[p].iter().position(|t| t.0 as usize == item + 1).unwrap();
        assert!(s.drink(p, slot(&s, might)));
        assert!(s.scaled(p, foe, 0, -10.0) < before, "harder blows while it lasts");
        assert!(!s.drink(p, slot(&s, renewal)), "too soon after the last");
        s.tick += proto::rules::Rules::ticks(s.rules.potion_cooldown_secs, s.tick_hz);
        s.ents.attrs[0][p] = 50.0;
        assert!(s.drink(p, slot(&s, renewal)));
        s.system_potions();
        assert_eq!(s.ents.attrs[0][p], 54.0, "renewal heals each second");
        assert!(s.draughts[p].iter().all(|d| d.item == renewal), "might ran out");
        assert!((s.scaled(p, foe, 0, -10.0) - before).abs() < 1e-4);
        assert!(s.notes.iter().any(|n| n.0 == p && n.2 == proto::chat::channel::POTIONS && n.3.starts_with("P0|renewal,#aa3366,")), "{:?}", s.notes);
        s.tick += 6 * s.tick_hz as u32;
        s.system_potions();
        assert!(s.draughts[p].is_empty());
        assert!(s.notes.iter().any(|n| n.0 == p && n.3 == "P0|"));
    }
}

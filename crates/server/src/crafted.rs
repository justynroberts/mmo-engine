//! A crafter's name on what they make: a stack made with `/craft` carries its maker's name, kept beside the bag slot
//! it is in (`Sim::maker`, by slot, with the item it was for so a slot reused by something else forgets it) and beside
//! what is worn (`gear_maker`). It follows the stack when it is moved, split, worn and taken off, goes when the slot
//! empties, and is saved with the character. Trading, selling or posting a thing leaves the name behind. Each player
//! is told their own on chat channel 20 (`proto::chat::channel::MAKERS`), "K0|slot,name;..." (bag slots, then
//! "g0".."g2" for what is worn), when it changes.

use crate::sim::Sim;
use proto::game::{GEAR_SLOTS, INV_SLOTS};

/// A slot's maker: the item it is for (tag, item index + 1) and the name; tag 0 for none.
pub type Mark = (u8, String);

impl Sim {
    /// Put `n` of `item`, made by `maker`, into the bag: onto a stack of the same thing by the same hand, else into an
    /// empty slot, the name with it. What does not fit goes as plain stacks (or to the ground).
    pub fn give_made(&mut self, s: usize, item: usize, n: u32, maker: &str) {
        let tag = item as u8 + 1;
        let max = self.items.get(item).map_or(1, |d| d.stack_max) as u32;
        let mut left = n;
        for i in 0..INV_SLOTS {
            if left == 0 {
                break;
            }
            let (t, c) = self.inv[s][i];
            let same = t == tag && self.maker[s][i].0 == tag && self.maker[s][i].1 == maker && (c as u32) < max;
            if same || t == 0 {
                let put = (max - if t == 0 { 0 } else { c as u32 }).min(left);
                self.inv[s][i] = (tag, if t == 0 { put } else { c as u32 + put } as u8);
                self.maker[s][i] = (tag, maker.to_string());
                left -= put;
            }
        }
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
        if left > 0 {
            self.give_or_drop(s, item, left);
        }
    }

    /// The maker of what is in bag slot `i`, if it was made and is still there.
    pub fn made_by(&self, s: usize, i: usize) -> Option<&str> {
        let m = &self.maker[s][i];
        (m.0 != 0 && m.0 == self.inv[s][i].0).then_some(m.1.as_str())
    }

    /// A stack moved from slot `a` to `b` (swapped, or poured into a stack of the same thing).
    pub fn makers_moved(&mut self, s: usize, a: usize, b: usize) {
        if self.inv[s][a].0 == 0 {
            // Poured in, or moved to an empty slot: b keeps its own name, or takes a's.
            if self.made_by(s, b).is_none() {
                self.maker[s][b] = std::mem::take(&mut self.maker[s][a]);
            }
            self.maker[s][a] = (0, String::new());
        } else {
            self.maker[s].swap(a, b);
        }
    }

    /// Slot `e` was split off slot `a`: it carries the same name.
    pub fn makers_split(&mut self, s: usize, a: usize, e: usize) {
        self.maker[s][e] = self.maker[s][a].clone();
    }

    /// Bag slot `i` was worn in gear slot `g`, and what was worn there came back into slot `i` (or into the bag).
    pub fn makers_worn(&mut self, s: usize, i: usize, g: usize, was: u8) {
        let mark = std::mem::take(&mut self.maker[s][i]);
        let old = std::mem::replace(&mut self.gear_maker[s][g], mark);
        if was != 0 && self.inv[s][i].0 == was {
            self.maker[s][i] = old;
        }
    }

    /// What was worn in gear slot `g` went back into the bag: its name to the first stack of it without one.
    pub fn makers_taken_off(&mut self, s: usize, g: usize, tag: u8) {
        let mark = std::mem::take(&mut self.gear_maker[s][g]);
        if mark.0 != tag {
            return;
        }
        if let Some(i) = (0..INV_SLOTS).find(|&i| self.inv[s][i].0 == tag && self.made_by(s, i).is_none()) {
            self.maker[s][i] = mark;
        }
    }

    /// Once a second: names on slots that have emptied or hold something else go, and each player is told theirs
    /// when that changes.
    pub fn system_makers(&mut self) {
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= crate::sim::DRIVEN {
                continue;
            }
            for i in 0..INV_SLOTS {
                if self.maker[s][i].0 != 0 && self.maker[s][i].0 != self.inv[s][i].0 {
                    self.maker[s][i] = (0, String::new());
                }
            }
            for g in 0..GEAR_SLOTS {
                if self.gear_maker[s][g].0 != 0 && self.gear_maker[s][g].0 != self.gear[s][g] {
                    self.gear_maker[s][g] = (0, String::new());
                }
            }
            let mut entries: Vec<String> = (0..INV_SLOTS).filter_map(|i| self.made_by(s, i).map(|n| format!("{i},{}", n.replace([',', ';', '|'], " ")))).collect();
            entries.extend((0..GEAR_SLOTS).filter(|&g| self.gear_maker[s][g].0 != 0).map(|g| format!("g{g},{}", self.gear_maker[s][g].1.replace([',', ';', '|'], " "))));
            let all = entries.join(";");
            if self.makers_sent[s] == all {
                continue;
            }
            self.makers_sent[s] = all;
            let mut line = String::from("K0|");
            for e in &entries {
                if line.len() + e.len() + 1 > proto::chat::MAX_TEXT {
                    self.notes.push((s, 0, proto::chat::channel::MAKERS, std::mem::replace(&mut line, String::from("K1|"))));
                }
                if !line.ends_with('|') {
                    line.push(';');
                }
                line.push_str(e);
            }
            self.notes.push((s, 0, proto::chat::channel::MAKERS, line));
        }
    }

    /// For saving: each made stack in the bag, and what is worn, by slot ("g0".. for gear).
    pub fn makers_saved(&self, s: usize) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = (0..INV_SLOTS).filter_map(|i| self.made_by(s, i).map(|n| (i.to_string(), n.to_string()))).collect();
        v.extend((0..GEAR_SLOTS).filter(|&g| self.gear_maker[s][g].0 != 0 && self.gear_maker[s][g].0 == self.gear[s][g]).map(|g| (format!("g{g}"), self.gear_maker[s][g].1.clone())));
        v
    }

    /// Bring the names back after the bag and gear are restored.
    pub fn makers_restore(&mut self, s: usize, saved: &[(String, String)]) {
        self.maker[s] = vec![(0, String::new()); INV_SLOTS];
        self.gear_maker[s] = vec![(0, String::new()); GEAR_SLOTS];
        for (slot, name) in saved {
            if let Some(g) = slot.strip_prefix('g').and_then(|g| g.parse::<usize>().ok()).filter(|&g| g < GEAR_SLOTS) {
                self.gear_maker[s][g] = (self.gear[s][g], name.clone());
            } else if let Some(i) = slot.parse::<usize>().ok().filter(|&i| i < INV_SLOTS) {
                self.maker[s][i] = (self.inv[s][i].0, name.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::realm::{Recipe, Stack};
    use proto::world::World;

    #[test]
    fn what_is_made_carries_its_makers_name_through_moves_splits_and_the_save() {
        let mut g = GameFile::default_game();
        g.recipes = vec![Recipe { name: "kit".into(), needs: vec![Stack { item: "scrap".into(), count: 1 }], makes: "medkit".into(), count: 2, near: None, skill: None, level: 0 }];
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        s.set_name(p, "Brid", true);
        let scrap = s.items.iter().position(|i| i.name == "scrap").unwrap();
        s.give(p, scrap, 1);
        crate::realm::command(&mut s, &mut crate::net::Clients::default(), p, "/craft kit");
        let at = (0..proto::game::INV_SLOTS).find(|&i| s.made_by(p, i) == Some("Brid")).expect("made, with the name");
        s.bag_action(p, (proto::game::bag_op::SPLIT as u16) << 8 | at as u16, 0);
        assert_eq!((0..proto::game::INV_SLOTS).filter(|&i| s.made_by(p, i) == Some("Brid")).count(), 2, "both halves");
        s.bag_action(p, (proto::game::bag_op::MOVE as u16) << 8 | at as u16, 20);
        assert_eq!(s.made_by(p, 20), Some("Brid"), "moved with it");
        s.system_makers();
        assert!(s.notes.iter().any(|n| n.0 == p && n.2 == proto::chat::channel::MAKERS && n.3.contains("20,Brid")), "{:?}", s.notes);
        let rec = s.record_of(p);
        assert_eq!(rec.made.len(), 2);
        s.makers_restore(p, &rec.made);
        assert_eq!(s.made_by(p, 20), Some("Brid"), "and kept");
    }
}

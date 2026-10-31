//! Furnishing: an item with `furniture` is a piece to set down. Used on your own land (or your guild's) it is put
//! where you stand, turned a quarter more than the last one you set; `/unfurnish` picks up the nearest of yours
//! within reach and gives it back. Pieces are kept with the realm's save, and each player is told the pieces near
//! them on chat channel 19 (`proto::chat::channel::FURNITURE`) when that changes, for clients to draw.

use serde::{Deserialize, Serialize};

use crate::sim::Sim;
use proto::world::BLOCK;

/// Most pieces in the world, and how near a piece must be to pick it up.
pub const MAX_PIECES: usize = 20_000;
const REACH: f32 = 3.0;

/// A piece of furniture set down: where, turned how far (quarter turns), the item it is, and whose.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Piece {
    pub x: f32,
    pub y: f32,
    pub turn: u8,
    pub item: String,
    pub owner: String,
}

impl Sim {
    /// Set the furniture item in bag slot `i` down where the player in `s` stands. False (and why, told) if not.
    pub(crate) fn furnish(&mut self, s: usize, i: usize) -> bool {
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let (cx, cy) = ((x / BLOCK) as u32, (y / BLOCK) as u32);
        let Some(me) = self.tag_of(s) else { return false };
        let own = self.rs.plots.iter().any(|p| p.area.has(cx, cy)) && self.may_build(s, cx, cy);
        if !own {
            self.tells.push((s, "Furniture goes on your own land (/claim some first).".into()));
            return false;
        }
        if self.rs.furniture.len() >= MAX_PIECES {
            self.tells.push((s, "No more furniture can be set down.".into()));
            return false;
        }
        let (tag, count) = self.inv[s][i];
        let item = self.items[tag as usize - 1].name.clone();
        let turn = (self.rs.furniture.iter().filter(|p| p.owner == me).count() % 4) as u8;
        self.rs.furniture.push(Piece { x, y, turn, item: item.clone(), owner: me });
        self.rs.dirty = true;
        self.inv[s][i] = if count <= 1 { (0, 0) } else { (tag, count - 1) };
        self.bump_inv(s);
        self.tells.push((s, format!("You set the {} down. /unfurnish beside it picks it up again.", self.items[tag as usize - 1].shown())));
        true
    }

    /// `/unfurnish`: the nearest piece of yours within reach, back into the bag.
    pub fn unfurnish(&mut self, s: usize) {
        let Some(me) = self.tag_of(s) else { return };
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        let near = self
            .rs
            .furniture
            .iter()
            .enumerate()
            .filter(|(_, p)| p.owner == me && (p.x - x).hypot(p.y - y) <= REACH)
            .min_by(|a, b| (a.1.x - x).hypot(a.1.y - y).total_cmp(&(b.1.x - x).hypot(b.1.y - y)))
            .map(|(k, _)| k);
        let Some(k) = near else {
            self.tells.push((s, "None of your furniture is in reach.".into()));
            return;
        };
        let p = self.rs.furniture.swap_remove(k);
        self.rs.dirty = true;
        match self.items.iter().position(|i| i.name == p.item) {
            Some(item) => {
                self.give_or_drop(s, item, 1);
                self.tells.push((s, format!("You pick up the {}.", self.items[item].shown())));
            }
            None => self.tells.push((s, "It crumbles: the game has no such thing any more.".into())),
        }
    }

    /// Each player: the furniture within `realm::LAND_REACH`, when it changes: "U0|x,y,turn,piece;..." ("U1|" goes
    /// on, an empty "U0|" for none).
    pub(crate) fn system_furniture(&mut self) {
        if self.rs.furniture.is_empty() && self.rs.furniture_sent.iter().all(|t| t.is_empty()) {
            return;
        }
        let reach = crate::realm::LAND_REACH;
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= crate::sim::DRIVEN {
                continue;
            }
            let (x, y) = (self.ents.x[s], self.ents.y[s]);
            let entries: Vec<String> = self
                .rs
                .furniture
                .iter()
                .filter(|p| (p.x - x).abs() <= reach && (p.y - y).abs() <= reach)
                .filter_map(|p| {
                    let piece = self.items.iter().find(|i| i.name == p.item)?.furniture.clone()?;
                    Some(format!("{:.1},{:.1},{},{piece}", p.x, p.y, p.turn))
                })
                .collect();
            let all = entries.join(";");
            if self.rs.furniture_sent.get(s).is_some_and(|t| *t == all) {
                continue;
            }
            if self.rs.furniture_sent.len() <= s {
                self.rs.furniture_sent.resize(s + 1, String::new());
            }
            self.rs.furniture_sent[s] = all;
            let mut line = String::from("U0|");
            for e in &entries {
                if line.len() + e.len() + 1 > proto::chat::MAX_TEXT {
                    self.notes.push((s, 0, proto::chat::channel::FURNITURE, std::mem::replace(&mut line, String::from("U1|"))));
                }
                if !line.ends_with('|') {
                    line.push(';');
                }
                line.push_str(e);
            }
            self.notes.push((s, 0, proto::chat::channel::FURNITURE, line));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::realm::PlotRules;
    use proto::world::World;

    #[test]
    fn furniture_goes_down_on_your_land_is_told_to_those_near_and_comes_back_up() {
        let mut g = GameFile::default_game();
        g.plots = Some(PlotRules { size: 8, per_player: 1, cost: 0, sizes: vec![] });
        let mut stool = g.items[0].clone();
        (stool.name, stool.uses, stool.furniture, stool.consume) = ("stool".into(), vec![], Some("d:stool".into()), true);
        g.items.push(stool);
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (300.0, 300.0);
        s.set_account(p, Some("own"));
        let item = s.items.iter().position(|i| i.name == "stool").unwrap();
        s.give(p, item, 1);
        let slot = s.inv[p].iter().position(|b| b.0 as usize == item + 1).unwrap();
        assert!(!s.use_slot(p, slot), "not on your own land yet");
        crate::realm::command(&mut s, &mut crate::net::Clients::default(), p, "/claim");
        assert!(s.use_slot(p, slot));
        assert_eq!(s.rs.furniture.len(), 1);
        s.system_furniture();
        assert!(s.notes.iter().any(|n| n.0 == p && n.2 == proto::chat::channel::FURNITURE && n.3 == "U0|300.0,300.0,0,d:stool"), "{:?}", s.notes);
        s.unfurnish(p);
        assert!(s.rs.furniture.is_empty() && s.inv[p].iter().any(|b| b.0 as usize == item + 1), "back in the bag");
    }
}

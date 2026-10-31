//! Faction war: places marked `hold` are seats a faction can take and keep. Players fight for the faction they
//! joined (`/faction`). A faction whose members stand in a hold, with none of its holder's there and no other side
//! contesting, takes it after `rules.capture_secs`; the world is told. Every ten minutes each online member of a
//! holder is paid `rules.hold_tithe` coin a hold. Who holds what is kept with the realm's save and told to clients on
//! chat channel 18 (`proto::chat::channel::HOLDS`) when it changes; `/holds` lists it.

use std::collections::HashMap;

use crate::sim::Sim;

/// How often the holders are paid (seconds).
const TITHE_SECS: u32 = 600;

/// A hold's state: who has it ("" nobody), who is taking it and how far they have got (seconds).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hold {
    pub faction: String,
    pub taking: String,
    pub progress: u32,
}

/// The holds' state, by place name.
#[derive(Default)]
pub struct Holds {
    pub by_place: HashMap<String, Hold>,
    /// Bumped when a hold changes hands; each slot's last told version.
    pub ver: u32,
    pub told: Vec<u32>,
    /// Seconds to the next tithe.
    pub tithe_in: u32,
}

impl Sim {
    /// The places that are holds, with the faction holding each at first.
    fn hold_places(&self) -> Vec<(usize, String)> {
        self.places.iter().enumerate().filter_map(|(i, p)| p.hold.clone().map(|f| (i, f))).collect()
    }

    /// Once a second: holds contested and taken, the tithe, and clients told who holds what.
    pub fn system_holds(&mut self) {
        let places = self.hold_places();
        if places.is_empty() {
            return;
        }
        if self.holds.told.len() < self.ents.cap() {
            self.holds.told.resize(self.ents.cap(), u32::MAX);
        }
        let need = proto::rules::Rules::ticks(self.rules.capture_secs, 1).max(1);
        for (pi, first) in places.iter().cloned() {
            let p = self.places[pi].clone();
            let state = self.holds.by_place.entry(p.name.clone()).or_insert_with(|| Hold { faction: first.clone(), ..Default::default() }).clone();
            // Who of which side stands in it.
            let mut sides: Vec<(String, Vec<usize>)> = Vec::new();
            for s in 0..self.ents.high {
                if !self.ents.alive[s] || self.ents.owner[s] >= crate::sim::DRIVEN || self.ents.fallen[s] || self.rs.faction[s] == 0 {
                    continue;
                }
                if (self.ents.x[s] - p.x).powi(2) + (self.ents.y[s] - p.y).powi(2) > p.r * p.r {
                    continue;
                }
                let Some(f) = self.factions.get(self.rs.faction[s] as usize - 1).map(|f| f.name.clone()) else { continue };
                match sides.iter_mut().find(|e| e.0 == f) {
                    Some(e) => e.1.push(s),
                    None => sides.push((f, vec![s])),
                }
            }
            let defended = sides.iter().any(|e| e.0 == state.faction);
            let mut next = state.clone();
            match sides.as_slice() {
                [(f, who)] if !defended => {
                    if next.taking != *f {
                        next.taking = f.clone();
                        next.progress = 0;
                    }
                    next.progress += 1;
                    let title = if p.title.is_empty() { p.name.clone() } else { p.title.clone() };
                    if next.progress >= need {
                        next.faction = f.clone();
                        next.taking.clear();
                        next.progress = 0;
                        self.holds.ver = self.holds.ver.wrapping_add(1);
                        let side = self.factions.iter().find(|x| x.name == *f).map_or(f.clone(), |x| x.shown());
                        let line = format!("{side} take {title}!");
                        for s in 0..self.ents.high {
                            if self.ents.alive[s] && self.ents.owner[s] < crate::sim::DRIVEN {
                                self.notes.push((s, 0, proto::chat::channel::BANNER, line.clone()));
                            }
                        }
                    } else if next.progress % 15 == 0 || next.progress == 1 {
                        for &s in who {
                            self.tells.push((s, format!("Taking {title}: {} of {need} seconds. Hold your ground.", next.progress)));
                        }
                    }
                }
                _ => {
                    // Defended, contested by more than one side, or empty: the taking slips back.
                    next.progress = if defended { 0 } else { next.progress.saturating_sub(1) };
                    if next.progress == 0 {
                        next.taking.clear();
                    }
                }
            }
            self.holds.by_place.insert(p.name.clone(), next);
        }
        self.pay_tithe(&places);
        self.tell_holds(&places);
    }

    fn pay_tithe(&mut self, places: &[(usize, String)]) {
        if self.holds.tithe_in > 0 {
            self.holds.tithe_in -= 1;
            return;
        }
        self.holds.tithe_in = TITHE_SECS;
        let (Some(coin), tithe) = (self.currency, self.rules.hold_tithe) else { return };
        if tithe == 0 {
            return;
        }
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= crate::sim::DRIVEN || self.rs.faction[s] == 0 {
                continue;
            }
            let Some(mine) = self.factions.get(self.rs.faction[s] as usize - 1).map(|f| f.name.clone()) else { continue };
            let held = places.iter().filter(|(pi, _)| self.holds.by_place.get(&self.places[*pi].name).is_some_and(|h| h.faction == mine)).count() as u32;
            if held > 0 {
                self.give_or_drop(s, coin, tithe * held);
                self.tells.push((s, format!("Your side's {held} hold{} pay you {} {}.", if held == 1 { "" } else { "s" }, tithe * held, self.items[coin].shown())));
            }
        }
    }

    /// Each player not yet told the holds as they stand: "H0|place,faction,colour;..." ("H1|" goes on).
    fn tell_holds(&mut self, places: &[(usize, String)]) {
        let entries: Vec<String> = places
            .iter()
            .map(|(pi, _)| {
                let p = &self.places[*pi];
                let f = self.holds.by_place.get(&p.name).map(|h| h.faction.clone()).unwrap_or_default();
                let (title, colour) = self.factions.iter().find(|x| x.name == f).map_or((String::new(), String::new()), |x| (x.shown(), x.color.clone()));
                format!("{},{},{}", p.name, title.replace([',', ';', '|'], " "), colour)
            })
            .collect();
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= crate::sim::DRIVEN || self.holds.told[s] == self.holds.ver {
                continue;
            }
            self.holds.told[s] = self.holds.ver;
            let mut line = String::from("H0|");
            for e in &entries {
                if line.len() + e.len() + 1 > proto::chat::MAX_TEXT {
                    self.notes.push((s, 0, proto::chat::channel::HOLDS, std::mem::replace(&mut line, String::from("H1|"))));
                }
                if !line.ends_with('|') {
                    line.push(';');
                }
                line.push_str(e);
            }
            self.notes.push((s, 0, proto::chat::channel::HOLDS, line));
        }
    }

    /// `/holds`: each hold and who has it.
    pub fn holds_line(&self) -> String {
        let all: Vec<String> = self
            .hold_places()
            .iter()
            .map(|(pi, first)| {
                let p = &self.places[*pi];
                let f = self.holds.by_place.get(&p.name).map_or(first.clone(), |h| h.faction.clone());
                let who = self.factions.iter().find(|x| x.name == f).map_or("nobody".to_string(), |x| x.shown());
                format!("{} ({who})", if p.title.is_empty() { &p.name } else { &p.title })
            })
            .collect();
        if all.is_empty() { "There are no holds to fight over here.".into() } else { format!("Holds: {}", all.join(", ")) }
    }

    /// For the realm's save: who holds each.
    pub fn holds_saved(&self) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self.holds.by_place.iter().map(|(p, h)| (p.clone(), h.faction.clone())).collect();
        v.sort();
        v
    }

    pub fn holds_restore(&mut self, saved: &[(String, String)]) {
        for (p, f) in saved {
            self.holds.by_place.insert(p.clone(), Hold { faction: f.clone(), ..Default::default() });
        }
        self.holds.ver = self.holds.ver.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::{FactionFile, GameFile};
    use proto::world::World;

    #[test]
    fn a_side_standing_unopposed_takes_a_hold_and_the_world_hears() {
        let mut g = GameFile::default_game();
        g.factions = vec![
            FactionFile { name: "red".into(), color: "#ff0000".into(), ..Default::default() },
            FactionFile { name: "blue".into(), color: "#0000ff".into(), ..Default::default() },
        ];
        g.rules.capture_secs = 3.0;
        let plaza = g.places.iter_mut().find(|p| p.name == "plaza").unwrap();
        plaza.hold = Some("red".into());
        let (px, py) = (plaza.x, plaza.y);
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let (b, r) = (s.spawn(0).unwrap(), s.spawn(1).unwrap());
        s.rs.faction[b] = 2;
        s.rs.faction[r] = 1;
        (s.ents.x[b], s.ents.y[b]) = (px, py);
        (s.ents.x[r], s.ents.y[r]) = (px + 1.0, py);
        for _ in 0..5 {
            s.system_holds();
        }
        assert_eq!(s.holds.by_place["plaza"].faction, "red", "defended: not taken");
        s.ents.x[r] = 500.0;
        for _ in 0..3 {
            s.system_holds();
        }
        assert_eq!(s.holds.by_place["plaza"].faction, "blue", "unopposed: taken");
        assert!(s.notes.iter().any(|n| n.0 == r && n.2 == proto::chat::channel::BANNER && n.3.contains("take The Plaza")), "everyone hears");
        assert!(s.notes.iter().any(|n| n.0 == b && n.2 == proto::chat::channel::HOLDS && n.3.contains("plaza,Blue,#0000ff")), "{:?}", s.notes);
        assert_eq!(s.holds_saved(), vec![("plaza".to_string(), "blue".to_string())]);
    }
}

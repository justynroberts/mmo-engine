//! Boats: places marked `dock` are harbours, and from one a player buys passage to any other (`/sail Name`), paid
//! by the distance, and steps off at the far quay. `/sail` alone, standing at a dock, lists where boats go and what
//! each costs.

use crate::sim::Sim;

/// A passage costs this much, and one more coin for every `PER_COIN` units sailed.
const BASE: u32 = 2;
const PER_COIN: f32 = 400.0;
/// How far round a dock's place counts as being at it.
const QUAY: f32 = 10.0;

impl Sim {
    /// The dock the player in `s` stands at, if any (index in `places`).
    fn dock_here(&self, s: usize) -> Option<usize> {
        let (x, y) = (self.ents.x[s], self.ents.y[s]);
        self.places.iter().position(|p| p.dock && (p.x - x).powi(2) + (p.y - y).powi(2) <= (p.r + QUAY).powi(2))
    }

    fn passage(&self, from: usize, to: usize) -> u32 {
        let (a, b) = (&self.places[from], &self.places[to]);
        BASE + ((a.x - b.x).hypot(a.y - b.y) / PER_COIN) as u32
    }

    fn dock_name(&self, i: usize) -> String {
        let p = &self.places[i];
        if p.title.is_empty() { p.name.clone() } else { p.title.clone() }
    }

    /// `/sail` and `/sail Name`.
    pub fn sail_command(&mut self, s: usize, rest: &str) {
        let Some(here) = self.dock_here(s) else {
            let docks: Vec<String> = (0..self.places.len()).filter(|&i| self.places[i].dock).map(|i| self.dock_name(i)).collect();
            let line = if docks.is_empty() { "There are no boats in this world.".to_string() } else { format!("Boats sail from the docks at {}.", docks.join(", ")) };
            self.tells.push((s, line));
            return;
        };
        let want = rest.trim().to_lowercase();
        if want.is_empty() {
            let routes: Vec<String> = (0..self.places.len()).filter(|&i| self.places[i].dock && i != here).map(|i| format!("{} ({})", self.dock_name(i), self.passage(here, i))).collect();
            self.tells.push((s, format!("From {}: {}. /sail Name to take ship.", self.dock_name(here), routes.join(", "))));
            return;
        }
        let Some(to) = (0..self.places.len()).find(|&i| self.places[i].dock && i != here && (self.places[i].name.to_lowercase() == want || self.places[i].title.to_lowercase() == want)) else {
            self.tells.push((s, "No boat goes there from here. /sail lists where they go.".into()));
            return;
        };
        if self.rs.phase[s] != 0 || self.ents.fallen[s] {
            self.tells.push((s, "Not now.".into()));
            return;
        }
        let cost = self.passage(here, to);
        if let Some(c) = self.currency.filter(|_| !self.ents.god[s]) {
            if !crate::market::take(&mut self.inv[s], c as u8 + 1, cost) {
                self.tells.push((s, format!("Passage to {} is {cost} {}.", self.dock_name(to), self.items[c].shown())));
                return;
            }
            self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
        }
        let p = self.places[to].clone();
        let (a, d) = (self.rng.f32() * std::f32::consts::TAU, self.rng.f32() * p.r * 0.4);
        if !self.put(s, p.x + a.cos() * d, p.y + a.sin() * d) {
            self.put(s, p.x, p.y);
        }
        let name = self.dock_name(to);
        self.notes.push((s, 0, proto::chat::channel::BANNER, format!("Across the water\nto {name}")));
        self.tells.push((s, format!("You take ship from {} and step ashore at {name}.", self.dock_name(here))));
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::world::World;

    #[test]
    fn a_boat_takes_you_from_dock_to_dock_for_the_fare() {
        let mut g = GameFile::default_game();
        for (name, d) in [("plaza", true), ("market", true)] {
            g.places.iter_mut().find(|p| p.name == name).unwrap().dock = d;
        }
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        (s.ents.x[p], s.ents.y[p]) = (200.0, 200.0);
        s.sail_command(p, "market square");
        let m = s.places.iter().find(|x| x.name == "market").unwrap().clone();
        assert!((s.ents.x[p] - m.x).hypot(s.ents.y[p] - m.y) <= m.r, "ashore at the market: {} {}", s.ents.x[p], s.ents.y[p]);
        assert!(s.notes.iter().any(|n| n.0 == p && n.3.contains("Market Square")));
    }
}

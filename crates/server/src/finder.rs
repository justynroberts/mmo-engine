//! The group finder: queue for a dungeon by role, alone or with your party, and when enough have queued the finder
//! makes them a party (a raid past `rules.party_size`) and takes them to the dungeon's door.
//!
//! `/queue` lists the dungeons; `/queue Name [tank|healer|damage]` queues you (and your party, who come as one);
//! `/unqueue` leaves. A group is the dungeon's `group` players: one tank and one healer first when it is three or
//! more, the rest whoever has waited longest; after `rules.finder_wait_secs` without them it takes whoever is there.
//! Each player queued hears how the queue stands on chat channel 16 (`proto::chat::channel::FINDER`) when it
//! changes: "Q|dungeon|queued|wanted|role", or "Q|" once out of it.

use crate::sim::{Sim, DRIVEN};

/// Roles, as queued.
pub const DAMAGE: u8 = 0;
pub const TANK: u8 = 1;
pub const HEALER: u8 = 2;
const ROLES: [&str; 3] = ["damage", "tank", "healer"];
/// How far outside a dungeon's place its door is (world units).
const DOOR_GAP: f32 = 3.0;

/// One place in a queue: a player, or a party queued together by one of them.
#[derive(Clone, Debug, PartialEq)]
pub struct Ticket {
    pub dungeon: usize,
    /// Entity ids of the players on it.
    pub members: Vec<u32>,
    pub role: u8,
    /// The tick it was queued.
    pub at: u32,
}

impl Sim {
    /// `/queue` and `/unqueue`.
    pub fn finder_command(&mut self, slot: usize, cmd: &str, rest: &str) {
        let me = self.ents.id_of(slot);
        if cmd == "unqueue" {
            match self.finder.iter().position(|t| t.members.contains(&me)) {
                Some(i) => {
                    let t = self.finder.remove(i);
                    for s in t.members.iter().filter_map(|&id| self.ents.resolve(id)) {
                        self.tells.push((s, format!("{} leaves the queue for {}.", self.name_of(slot), self.realm.dungeons[t.dungeon].name)));
                    }
                }
                None => self.tells.push((slot, "You are not queued.".into())),
            }
            self.finder_dirty = true;
            return;
        }
        if self.realm.dungeons.is_empty() {
            self.tells.push((slot, "There are no dungeons in this world.".into()));
            return;
        }
        let words: Vec<&str> = rest.split_whitespace().collect();
        let role_word = words.last().map(|w| w.to_lowercase()).filter(|w| ROLES.contains(&w.as_str()));
        let name = if role_word.is_some() { words[..words.len() - 1].join(" ") } else { words.join(" ") };
        if name.is_empty() {
            let list: Vec<String> = self.realm.dungeons.iter().enumerate().map(|(i, d)| format!("{} {} (a group of {})", i + 1, d.name, d.group)).collect();
            self.tells.push((slot, format!("Dungeons: {}. /queue Name [tank|healer|damage]", list.join(", "))));
            return;
        }
        let want = name.to_lowercase();
        let found = name.parse::<usize>().ok().and_then(|n| n.checked_sub(1)).filter(|&n| n < self.realm.dungeons.len()).or_else(|| {
            let place = |d: &proto::realm::DungeonRt| self.places.get(d.place).map(|p| (p.name.to_lowercase(), p.title.to_lowercase())).unwrap_or_default();
            self.realm.dungeons.iter().position(|d| d.name.to_lowercase() == want || place(d).0 == want || place(d).1 == want)
        });
        let Some(d) = found else {
            self.tells.push((slot, "No dungeon of that name. /queue lists them.".into()));
            return;
        };
        let role = role_word.map_or(DAMAGE, |w| ROLES.iter().position(|r| *r == w).unwrap_or(0) as u8);
        let members: Vec<u32> = self.party_of(slot).into_iter().map(|s| self.ents.id_of(s)).collect();
        if members.len() > self.realm.dungeons[d].group as usize {
            self.tells.push((slot, format!("Your party is bigger than {} wants.", self.realm.dungeons[d].name)));
            return;
        }
        self.finder.retain(|t| !t.members.iter().any(|m| members.contains(m)));
        self.finder.push(Ticket { dungeon: d, members: members.clone(), role, at: self.tick });
        self.finder_dirty = true;
        for s in members.iter().filter_map(|&id| self.ents.resolve(id)) {
            self.tells.push((s, format!("Queued for {} as {}. /unqueue to leave.", self.realm.dungeons[d].name, ROLES[role as usize])));
        }
    }

    /// Once a second: drop tickets whose players have gone, make every group that can be made, and tell those
    /// queued how it stands.
    pub fn system_finder(&mut self) {
        if self.finder.is_empty() && !self.finder_dirty {
            return;
        }
        let before = self.finder.len();
        let live = |s: &Sim, id: u32| s.ents.resolve(id).is_some_and(|x| s.ents.alive[x] && s.ents.owner[x] < DRIVEN);
        let finder = std::mem::take(&mut self.finder);
        self.finder = finder.into_iter().filter(|t| t.members.iter().all(|&m| live(self, m))).collect();
        if self.finder.len() != before {
            self.finder_dirty = true;
        }
        for d in 0..self.realm.dungeons.len() {
            while let Some(picked) = self.pick_group(d) {
                self.form_group(d, picked);
                self.finder_dirty = true;
            }
        }
        if self.finder_dirty {
            self.finder_dirty = false;
            self.tell_queues();
        }
    }

    /// The tickets (indices into `finder`) that make a group for dungeon `d`, if any do.
    fn pick_group(&self, d: usize) -> Option<Vec<usize>> {
        let want = self.realm.dungeons[d].group as usize;
        let mut mine: Vec<usize> = (0..self.finder.len()).filter(|&i| self.finder[i].dungeon == d).collect();
        mine.sort_by_key(|&i| self.finder[i].at);
        let total: usize = mine.iter().map(|&i| self.finder[i].members.len()).sum();
        if total < want {
            return None;
        }
        let waited = mine.first().map_or(0, |&i| self.tick.saturating_sub(self.finder[i].at));
        let patient = waited < proto::rules::Rules::ticks(self.rules.finder_wait_secs, self.tick_hz);
        let mut out: Vec<usize> = Vec::new();
        let mut size = 0;
        let take = |i: usize, out: &mut Vec<usize>, size: &mut usize| {
            let n = self.finder[i].members.len();
            if !out.contains(&i) && *size + n <= want {
                out.push(i);
                *size += n;
            }
        };
        // A tank and a healer first, for a group of three or more.
        if want >= 3 {
            for role in [TANK, HEALER] {
                if let Some(&i) = mine.iter().find(|&&i| self.finder[i].role == role) {
                    take(i, &mut out, &mut size);
                } else if patient {
                    return None;
                }
            }
        }
        for &i in &mine {
            take(i, &mut out, &mut size);
        }
        (size == want).then_some(out)
    }

    /// Make the picked tickets one party (a raid if it is bigger than a party), and take them to the door.
    fn form_group(&mut self, d: usize, picked: Vec<usize>) {
        let mut tickets: Vec<Ticket> = picked.iter().map(|&i| self.finder[i].clone()).collect();
        let mut sorted = picked;
        sorted.sort_unstable_by(|a, b| b.cmp(a));
        for i in sorted {
            self.finder.remove(i);
        }
        tickets.sort_by_key(|t| t.at);
        let slots: Vec<usize> = tickets.iter().flat_map(|t| t.members.iter().filter_map(|&id| self.ents.resolve(id))).collect();
        for &s in &slots {
            if self.party[s] != 0 {
                self.leave_party(s);
            }
        }
        self.next_party = self.next_party.wrapping_add(1).max(1);
        let party = self.next_party;
        for &s in &slots {
            self.party[s] = party;
            self.party_ver[s] = self.party_ver[s].wrapping_add(1);
        }
        if slots.len() > self.rules.party_size as usize {
            self.raids.insert(party);
        }
        // The door: just outside the dungeon's place, on the side facing the world's middle, spread out a little.
        let dungeon = self.realm.dungeons[d].clone();
        let names: Vec<String> = slots.iter().map(|&s| self.name_of(s).to_string()).collect();
        if let Some(p) = self.places.get(dungeon.place).cloned() {
            let mid = self.world.size / 2.0;
            let base = (mid - p.y).atan2(mid - p.x);
            for (k, &s) in slots.iter().enumerate() {
                let a = base + (k as f32 - (slots.len() as f32 - 1.0) / 2.0) * 0.12;
                let r = p.r + DOOR_GAP;
                if !self.put(s, p.x + a.cos() * r, p.y + a.sin() * r) {
                    self.put(s, p.x + base.cos() * (r + 4.0), p.y + base.sin() * (r + 4.0));
                }
            }
        }
        for &s in &slots {
            let role = tickets.iter().find(|t| t.members.contains(&self.ents.id_of(s))).map_or(DAMAGE, |t| t.role);
            self.tells.push((s, format!("The finder has a company for {}: {}. You are at its door ({}).", dungeon.name, names.join(", "), ROLES[role as usize])));
        }
    }

    /// Each player queued: how many are queued for their dungeon, of how many wanted; and "Q|" to those out of it
    /// since the last telling.
    fn tell_queues(&mut self) {
        let mut told: Vec<usize> = Vec::new();
        for t in self.finder.clone() {
            let queued: usize = self.finder.iter().filter(|o| o.dungeon == t.dungeon).map(|o| o.members.len()).sum();
            let d = &self.realm.dungeons[t.dungeon];
            let line = format!("Q|{}|{}|{}|{}", d.name, queued, d.group, ROLES[t.role as usize]);
            for s in t.members.iter().filter_map(|&id| self.ents.resolve(id)) {
                self.notes.push((s, 0, proto::chat::channel::FINDER, line.clone()));
                told.push(s);
            }
        }
        for s in std::mem::take(&mut self.finder_told) {
            if !told.contains(&s) && self.ents.alive[s] {
                self.notes.push((s, 0, proto::chat::channel::FINDER, "Q|".into()));
            }
        }
        self.finder_told = told;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proto::game::GameFile;
    use proto::realm::{DungeonBot, DungeonDef};
    use proto::world::World;

    fn sim() -> Sim {
        let mut g = GameFile::default_game();
        g.archetypes.iter_mut().for_each(|a| a.count = 0);
        g.places.push(proto::game::Place { name: "crypt".into(), x: 300.0, y: 300.0, r: 10.0, ..g.places[0].clone() });
        g.dungeons = vec![DungeonDef { name: "The Crypt".into(), place: "crypt".into(), bots: vec![DungeonBot { bot: g.archetypes[0].name.clone(), count: 1 }], xp: 0, reward: None, group: 3 }];
        Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1)
    }

    #[test]
    fn three_queued_become_a_party_at_the_door_with_a_tank_and_healer_first() {
        let mut s = sim();
        let p: Vec<usize> = (0..4).map(|o| s.spawn(o).unwrap()).collect();
        s.finder_command(p[0], "queue", "the crypt");
        s.finder_command(p[1], "queue", "the crypt damage");
        s.finder_command(p[2], "queue", "1 tank");
        s.system_finder();
        assert_eq!(s.finder.len(), 3, "no healer yet, and not waited long: nobody goes");
        assert!(s.notes.iter().any(|n| n.0 == p[0] && n.3 == "Q|The Crypt|3|3|damage"), "{:?}", s.notes);
        s.finder_command(p[3], "queue", "The Crypt healer");
        s.system_finder();
        // The tank, the healer and the one who waited longest go; the other damage waits.
        assert_eq!(s.finder.len(), 1);
        assert_eq!(s.finder[0].members, vec![s.ents.id_of(p[1])]);
        let party = s.party[p[0]];
        assert!(party != 0 && s.party[p[2]] == party && s.party[p[3]] == party && s.party[p[1]] == 0);
        for &m in &[p[0], p[2], p[3]] {
            let d = (s.ents.x[m] - 300.0).hypot(s.ents.y[m] - 300.0);
            assert!(d > 10.0 && d < 20.0, "at the door: {d}");
        }
    }

    #[test]
    fn past_the_wait_whoever_is_queued_goes_and_leaving_tells_you_so() {
        let mut s = sim();
        let p: Vec<usize> = (0..3).map(|o| s.spawn(o).unwrap()).collect();
        for &m in &p {
            s.finder_command(m, "queue", "the crypt");
        }
        s.system_finder();
        assert_eq!(s.finder.len(), 3);
        s.tick += proto::rules::Rules::ticks(s.rules.finder_wait_secs, s.tick_hz) + 1;
        s.system_finder();
        assert!(s.finder.is_empty() && s.party[p[0]] != 0 && s.party[p[0]] == s.party[p[2]]);
        s.notes.clear();
        let q = s.spawn(3).unwrap();
        s.finder_command(q, "queue", "the crypt");
        s.system_finder();
        s.finder_command(q, "unqueue", "");
        s.system_finder();
        assert!(s.notes.iter().any(|n| n.0 == q && n.3 == "Q|"), "{:?}", s.notes);
    }
}

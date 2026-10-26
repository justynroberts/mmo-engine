//! Factions and standing. Bot types belong to factions (`Archetype::faction`); a player's standing with each starts
//! at 0 and moves when they beat its members (down by the faction's `kill`) or members of a faction it hates (up by
//! its `bounty`). Below the faction's `hostile` line its fighters attack the player on sight (an encounter, see
//! `encounter.rs`), its traders will not deal with them and its quest givers will not speak to them. Bad standing
//! is forgiven slowly, one point a minute back towards 0.
//!
//! Players may also join a faction as their side (`/faction`, `realm.rs`): members never harm each other and its
//! fighters never take a member for an enemy.
//!
//! Standing is kept by faction name, so editing the game file never scrambles it, and saved with the character.

use crate::sim::{Sim, DRIVEN};
use proto::game::STANDING_MAX;

impl Sim {
    /// A player's standing with the faction at `f` (an index in `self.factions`).
    pub fn standing(&self, s: usize, f: usize) -> i32 {
        let name = &self.factions[f].name;
        self.standing[s].iter().find(|(n, _)| n == name).map_or(0, |e| e.1)
    }

    fn shift_standing(&mut self, s: usize, f: usize, by: i32) {
        if by == 0 {
            return;
        }
        let name = self.factions[f].name.clone();
        let before = self.standing(s, f);
        let now = (before + by).clamp(-STANDING_MAX, STANDING_MAX);
        match self.standing[s].iter_mut().find(|(n, _)| *n == name) {
            Some(e) => e.1 = now,
            None => self.standing[s].push((name, now)),
        }
        let fac = &self.factions[f];
        let (was, is) = (fac.mood(before), fac.mood(now));
        let word = if by < 0 { "less" } else { "more" };
        let mut line = format!("{} think {word} of you ({now}, {is}).", fac.shown());
        if was != is && is == "hostile" {
            line.push_str(" Their fighters will attack you on sight.");
        }
        self.tells.push((s, line));
    }

    /// The bot at `b` has fallen to the players `who`: their standing moves with its faction and with the
    /// factions that hate it.
    pub(crate) fn faction_kill(&mut self, b: usize, who: &[usize]) {
        let Some(f) = self.archs.get(self.ents.arch[b] as usize).and_then(|a| a.faction) else { return };
        let name = self.factions[f].name.clone();
        let kill = self.factions[f].kill;
        let haters: Vec<(usize, i32)> = self.factions.iter().enumerate().filter(|(_, h)| h.hates.contains(&name)).map(|(k, h)| (k, h.bounty)).collect();
        for &p in who {
            self.shift_standing(p, f, kill);
            for &(h, bounty) in &haters {
                self.shift_standing(p, h, bounty);
            }
        }
    }

    /// Whether the bot at `b` belongs to a faction that `p` stands below hostile with.
    pub fn hostile_to(&self, b: usize, p: usize) -> bool {
        if self.ents.owner[p] >= DRIVEN {
            return false;
        }
        let Some(f) = self.archs.get(self.ents.arch[b] as usize).and_then(|a| a.faction) else { return false };
        // A player who joined the faction (`/faction`) is one of its own.
        self.rs.faction[p] as usize != f + 1 && self.standing(p, f) < self.factions[f].hostile
    }

    /// Why a bot will not deal with a player (trade, quests), if it will not.
    pub fn refuses(&self, b: usize, p: usize) -> Option<String> {
        self.hostile_to(b, p).then(|| format!("The {} will have nothing to do with you.", self.archs[self.ents.arch[b] as usize].shown()))
    }

    /// The `/rep` line: every faction the player is known to, and how they stand.
    pub fn standing_line(&self, s: usize) -> String {
        if self.factions.is_empty() {
            return "There are no factions here.".into();
        }
        let all: Vec<String> = (0..self.factions.len())
            .map(|f| {
                let v = self.standing(s, f);
                format!("{} {v} ({})", self.factions[f].shown(), self.factions[f].mood(v))
            })
            .collect();
        format!("Standing: {}", all.join(", "))
    }

    /// Once a minute: bad standing comes back one point towards 0.
    pub fn system_forgive(&mut self) {
        for list in self.standing.iter_mut() {
            for e in list.iter_mut() {
                if e.1 < 0 {
                    e.1 += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{Sim, NONE};
    use proto::game::{FactionFile, GameFile};
    use proto::world::World;

    /// Civilians of "folk", who hate the drones' "machines"; folk turn on a player at -30.
    fn sim() -> (Sim, usize, u8, u8) {
        let mut g = GameFile::default_game();
        g.factions = vec![
            FactionFile { name: "folk".into(), kill: -20, hostile: -30, hates: vec!["machines".into()], bounty: 5, ..Default::default() },
            FactionFile { name: "machines".into(), kill: -1, hostile: -30, bounty: 2, ..Default::default() },
        ];
        for a in g.archetypes.iter_mut() {
            match a.name.as_str() {
                "civilian" | "guard" => a.faction = Some("folk".into()),
                "drone" => a.faction = Some("machines".into()),
                _ => {}
            }
            if a.name == "guard" {
                a.attack = Some("zap".into());
            }
        }
        let civ = g.archetypes.iter().position(|a| a.name == "civilian").unwrap() as u8;
        let guard = g.archetypes.iter().position(|a| a.name == "guard").unwrap() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (500.0, 500.0);
        (s, me, civ, guard)
    }

    fn beat(s: &mut Sim, me: usize, arch: u8) {
        let b = s.spawn_as(NONE, arch, Some((600.0, 600.0))).unwrap();
        let (id, now) = (s.ents.id_of(me), s.tick);
        s.ents.hurt_by[b].push((id, now));
        s.ents.attrs[0][b] = 0.0;
        s.step(&[]);
    }

    #[test]
    fn killing_civilians_costs_standing_and_killing_their_foes_earns_it() {
        let (mut s, me, civ, _) = sim();
        let drone = s.archs.iter().position(|a| a.name == "drone").unwrap() as u8;
        beat(&mut s, me, civ);
        assert_eq!((s.standing(me, 0), s.standing(me, 1)), (-20, 0));
        beat(&mut s, me, drone);
        assert_eq!((s.standing(me, 0), s.standing(me, 1)), (-15, -1), "a drone beaten: the folk like it");
        assert!(s.tells.iter().any(|t| t.0 == me && t.1.contains("Folk think less of you (-20, wary)")), "{:?}", s.tells);
        // Saved by name and put back.
        let rec = s.record_of(me);
        assert_eq!(rec.standing, vec![("folk".to_string(), -15), ("machines".to_string(), -1)]);
        let other = s.spawn(1).unwrap();
        s.restore(other, &rec);
        assert_eq!(s.standing(other, 0), -15);
    }

    #[test]
    fn below_hostile_guards_attack_on_sight_and_traders_refuse() {
        let (mut s, me, civ, guard) = sim();
        let g = s.spawn_as(NONE, guard, Some((510.0, 500.0))).unwrap();
        for _ in 0..30 {
            s.step(&[]);
        }
        assert_eq!(s.foe[g], None, "a guard leaves an honest player be");
        assert!(s.refuses(g, me).is_none());
        beat(&mut s, me, civ);
        beat(&mut s, me, civ);
        assert!(s.tells.iter().any(|t| t.1.contains("attack you on sight")), "{:?}", s.tells);
        (s.ents.x[g], s.ents.y[g]) = (510.0, 500.0);
        for _ in 0..30 {
            s.step(&[]);
        }
        assert_eq!(s.foe[g], Some(s.ents.id_of(me)), "now it comes for us");
        assert!(s.refuses(g, me).is_some());
        // Forgiven, slowly.
        let before = s.standing(me, 0);
        s.system_forgive();
        assert_eq!(s.standing(me, 0), before + 1);
    }
}

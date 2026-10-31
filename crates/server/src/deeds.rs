//! Deeds, achievements and titles. Each character counts what they do (bots beaten, by type and in all, quests
//! handed in, places found, things made, harvests, blocks built, falls, dungeons cleared); an achievement
//! (`GameFile::achievements`) is a count of one deed, earned once, with experience and perhaps a title. A player
//! wears one title they have earned (`/title Name`), shown with their name (the names feed sends "Name|Title").

use crate::sim::Sim;

impl Sim {
    /// Count `n` more of deed `key` for the player in `s` (see `proto::realm::DEEDS`), earning any achievement it
    /// completes.
    pub fn deed(&mut self, s: usize, key: &str, n: u32) {
        if self.realm.achievements.is_empty() {
            return;
        }
        let now = match self.deeds[s].iter_mut().find(|(k, _)| k == key) {
            Some(e) => {
                e.1 = e.1.saturating_add(n);
                e.1
            }
            None => {
                self.deeds[s].push((key.to_string(), n));
                n
            }
        };
        let due: Vec<proto::realm::Achievement> =
            self.realm.achievements.iter().filter(|a| a.deed == key && now >= a.count && !self.earned[s].contains(&a.name)).cloned().collect();
        for a in due {
            self.earned[s].push(a.name.clone());
            let also = if a.grants.is_empty() { String::new() } else { format!(" You may now be called {} (/title {}).", a.grants, a.grants) };
            let line = format!("Achievement earned: {}.{also}", a.title);
            if a.xp > 0 && self.levels.is_some() {
                self.award(s, a.xp, Some(format!("{line} +{} experience", a.xp)));
            } else {
                self.tells.push((s, line));
            }
        }
    }

    /// How far a player is with deed `key`.
    pub fn deed_count(&self, s: usize, key: &str) -> u32 {
        self.deeds[s].iter().find(|(k, _)| k == key).map_or(0, |e| e.1)
    }

    /// The titles a player has earned.
    pub fn titles_of(&self, s: usize) -> Vec<String> {
        self.realm.achievements.iter().filter(|a| !a.grants.is_empty() && self.earned[s].contains(&a.name)).map(|a| a.grants.clone()).collect()
    }

    /// `/title`: list the titles earned, take one (`/title Name`) or none (`/title none`).
    pub fn title_command(&mut self, s: usize, rest: &str) {
        let have = self.titles_of(s);
        let want = rest.trim();
        if want.is_empty() {
            let line = if have.is_empty() { "You have earned no titles yet.".to_string() } else { format!("Your titles: {}. /title Name to be called one, /title none for none.", have.join(", ")) };
            self.tells.push((s, line));
            return;
        }
        if want.eq_ignore_ascii_case("none") {
            self.title[s].clear();
            self.ents.renamed.push(s as u32);
            self.tells.push((s, "You go by your name alone.".into()));
            return;
        }
        match have.iter().find(|t| t.eq_ignore_ascii_case(want)) {
            Some(t) => {
                self.title[s] = t.clone();
                self.ents.renamed.push(s as u32);
                self.tells.push((s, format!("You are now called {t}.")));
            }
            None => self.tells.push((s, "You have not earned that title.".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::realm::Achievement;
    use proto::world::World;

    #[test]
    fn deeds_earn_achievements_once_and_their_titles_are_worn() {
        let mut g = GameFile::default_game();
        g.achievements = vec![Achievement { name: "cull".into(), title: "The Cull".into(), text: String::new(), deed: "kill:drifter".into(), count: 2, grants: "Drifterbane".into(), xp: 0 }];
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        s.set_name(p, "Brid", true);
        s.deed(p, "kill:drifter", 1);
        assert!(s.earned[p].is_empty());
        s.deed(p, "kill:drifter", 1);
        s.deed(p, "kill:drifter", 1);
        assert_eq!(s.earned[p], vec!["cull".to_string()], "earned once");
        assert_eq!(s.tells.iter().filter(|(q, t)| *q == p && t.starts_with("Achievement earned")).count(), 1);
        s.title_command(p, "drifterbane");
        assert_eq!(&*s.feed_name(p), "Brid|Drifterbane", "worn with the name");
        let rec = s.record_of(p);
        assert_eq!((rec.title.as_str(), rec.earned.len(), rec.deeds.len()), ("Drifterbane", 1, 1));
        s.title_command(p, "none");
        assert_eq!(&*s.feed_name(p), "Brid");
    }
}

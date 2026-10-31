//! Skills that rise with use (`GameFile::skills`): a recipe that names one needs its level and raises it while it
//! is not far below the maker, and past the level now and then makes one more; a crop that names one raises it when
//! harvested, a sheaf more for each 25; a bot type that names one raises it for whoever beats one (gathering).
//! Levels are kept per character by skill name (`Record::skills`); `/skills` lists them.

use crate::sim::Sim;

/// A recipe raises its skill while the maker is under its level plus this.
pub const GAIN_SPAN: u16 = 25;

impl Sim {
    /// A player's level in skill `k` (index in `realm.skills`).
    pub fn skill_level(&self, s: usize, k: usize) -> u16 {
        let Some(name) = self.realm.skills.get(k).map(|d| d.name.as_str()) else { return 0 };
        self.skill_lv[s].iter().find(|(n, _)| n == name).map_or(0, |e| e.1)
    }

    /// Raise a player's skill `k` a point (up to its max), telling them.
    pub fn raise_skill(&mut self, s: usize, k: usize) {
        let Some(def) = self.realm.skills.get(k).cloned() else { return };
        let now = self.skill_level(s, k);
        if now >= def.max {
            return;
        }
        match self.skill_lv[s].iter_mut().find(|(n, _)| *n == def.name) {
            Some(e) => e.1 = now + 1,
            None => self.skill_lv[s].push((def.name.clone(), now + 1)),
        }
        self.tells.push((s, format!("Your {} rises to {}.", def.shown(), now + 1)));
    }

    /// `/skills`: each skill with the player's level in it.
    pub fn skills_line(&self, s: usize) -> String {
        if self.realm.skills.is_empty() {
            return "There are no skills in this world.".into();
        }
        let all: Vec<String> = (0..self.realm.skills.len()).map(|k| format!("{} {}/{}", self.realm.skills[k].shown(), self.skill_level(s, k), self.realm.skills[k].max)).collect();
        format!("Skills: {}", all.join(", "))
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::realm::SkillDef;
    use proto::world::World;

    #[test]
    fn a_skill_rises_to_its_max_and_is_kept_by_name() {
        let mut g = GameFile::default_game();
        g.skills = vec![SkillDef { name: "cooking".into(), title: "Cooking".into(), max: 2 }];
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        for _ in 0..3 {
            s.raise_skill(p, 0);
        }
        assert_eq!(s.skill_level(p, 0), 2, "not past its max");
        assert_eq!(s.record_of(p).skills, vec![("cooking".to_string(), 2)]);
        assert!(s.skills_line(p).contains("Cooking 2/2"));
    }
}

//! Talents: a point for each level past the first, spent in a talent of the player's class (`ClassFile::talents`)
//! with `/talent Name`, up to its ranks and from its level; each rank adds its might and toughness to the class's
//! own (`Sim::scaled`). `/talents` lists them; `/respec` takes every point back, at a market, for
//! `rules.respec_cost` coin. Kept per character by talent name (`Record::talents`).

use crate::sim::Sim;

impl Sim {
    /// Points a player has to spend: one a level past the first, less those spent.
    pub fn talent_points(&self, s: usize) -> u32 {
        let earned = (self.ents.level[s] as u32).saturating_sub(1);
        let spent: u32 = self.talents[s].iter().map(|t| t.1 as u32).sum();
        earned.saturating_sub(spent)
    }

    /// The rank a player has in a talent of their class.
    pub fn talent_rank(&self, s: usize, name: &str) -> u8 {
        self.talents[s].iter().find(|t| t.0 == name).map_or(0, |t| t.1)
    }

    /// What a player's talents add, as (might, toughness) shares.
    pub fn talent_bonus(&self, s: usize) -> (f32, f32) {
        if self.talents[s].is_empty() {
            return (0.0, 0.0);
        }
        let Some(c) = self.class_of(s) else { return (0.0, 0.0) };
        c.talents.iter().fold((0.0, 0.0), |(m, t), d| {
            let r = self.talent_rank(s, &d.name).min(d.ranks) as f32;
            (m + d.might * r, t + d.toughness * r)
        })
    }

    /// `/talents`, `/talent Name` and `/respec`.
    pub fn talent_command(&mut self, s: usize, cmd: &str, rest: &str) {
        let Some(class) = self.class_of(s).cloned() else {
            self.tells.push((s, "Choose a class first (/class Name).".into()));
            return;
        };
        if class.talents.is_empty() {
            self.tells.push((s, format!("The {} has no talents.", class.title)));
            return;
        }
        match cmd {
            "respec" => {
                if self.talents[s].is_empty() {
                    self.tells.push((s, "You have spent no points.".into()));
                    return;
                }
                if !self.at_market(s) {
                    self.tells.push((s, "Find a market (a town or a merchant) to unlearn your talents.".into()));
                    return;
                }
                let cost = self.rules.respec_cost;
                if let Some(c) = self.currency.filter(|_| cost > 0 && !self.ents.god[s]) {
                    if !crate::market::take(&mut self.inv[s], c as u8 + 1, cost) {
                        self.tells.push((s, format!("Unlearning costs {cost} {}.", self.items[c].shown())));
                        return;
                    }
                    self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
                }
                self.talents[s].clear();
                self.tells.push((s, format!("You unlearn your talents: {} points to spend again.", self.talent_points(s))));
            }
            "talent" if !rest.trim().is_empty() => {
                let want = rest.trim().to_lowercase();
                let Some(t) = class.talents.iter().find(|t| t.name.to_lowercase() == want || t.title.to_lowercase() == want) else {
                    self.tells.push((s, "Your class has no talent of that name. /talents lists them.".into()));
                    return;
                };
                let rank = self.talent_rank(s, &t.name);
                let why = if rank >= t.ranks {
                    Some(format!("{} is as high as it goes.", t.title))
                } else if self.levels.is_some() && self.ents.level[s] < t.level {
                    Some(format!("{} wants level {}.", t.title, t.level))
                } else if self.talent_points(s) == 0 {
                    Some("You have no points to spend: one comes with each level.".into())
                } else {
                    None
                };
                if let Some(why) = why {
                    self.tells.push((s, why));
                    return;
                }
                match self.talents[s].iter_mut().find(|e| e.0 == t.name) {
                    Some(e) => e.1 += 1,
                    None => self.talents[s].push((t.name.clone(), 1)),
                }
                self.tells.push((s, format!("{} rises to rank {} of {}.", t.title, rank + 1, t.ranks)));
            }
            _ => {
                let list: Vec<String> = class.talents.iter().map(|t| format!("{} {}/{}", t.title, self.talent_rank(s, &t.name), t.ranks)).collect();
                self.tells.push((s, format!("Talents ({} to spend): {}. /talent Name to learn one.", self.talent_points(s), list.join(", "))));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::{ClassFile, GameFile, LevelsFile, TalentFile};
    use proto::world::World;

    #[test]
    fn points_come_with_levels_buy_ranks_that_add_toughness_and_can_be_taken_back() {
        let mut g = GameFile::default_game();
        g.levels = Some(LevelsFile { cap: 10, base: 100.0, exponent: 1.0, might: 0.0, toughness: 0.0, discover_xp: 0, life: Default::default(), rewards: vec![], grow: Default::default() });
        g.classes = vec![ClassFile {
            name: "guard".into(),
            title: "Guard".into(),
            talents: vec![TalentFile { name: "wall".into(), title: "Shield Wall".into(), ranks: 2, toughness: 0.5, ..Default::default() }],
            ..Default::default()
        }];
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        let foe = s.spawn(1).unwrap();
        s.class[p] = 1;
        s.talent_command(p, "talent", "wall");
        assert_eq!(s.talent_rank(p, "wall"), 0, "no points at level 1");
        s.ents.level[p] = 3;
        let before = s.scaled(foe, p, 0, -10.0);
        s.talent_command(p, "talent", "Shield Wall");
        s.talent_command(p, "talent", "wall");
        s.talent_command(p, "talent", "wall");
        assert_eq!((s.talent_rank(p, "wall"), s.talent_points(p)), (2, 0), "two ranks, no more");
        assert!(s.scaled(foe, p, 0, -10.0) > before, "blows taken hurt less: {} vs {before}", s.scaled(foe, p, 0, -10.0));
        assert_eq!(s.record_of(p).talents, vec![("wall".to_string(), 2)]);
    }
}

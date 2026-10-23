//! Quests: bots of a type offer them, players do their steps in order and hand them in for the rewards.
//!
//! Everything is decided here on the server. A client says "I talk to this one", "I take quest 3", "I give up
//! quest 3" through the input action byte (`ACT_TALK`, `ACT_ACCEPT`, `ACT_ABANDON`), and is told where it stands
//! in a QUESTS packet whenever that changes. Progress is kept by quest name, so editing the game file never
//! scrambles anyone's quests.

use crate::sim::{Sim, DRIVEN};
use proto::game::{QuestStep, TALK_RANGE};
use proto::wire::{QUEST_ACTIVE, QUEST_DONE, QUEST_READY};

/// One of a player's quests: on which step, how far into it, and whether it is ready to hand in or done.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestRun {
    pub name: String,
    pub step: u8,
    pub count: u16,
    pub state: u8,
}

/// How near another player must be to a bot that falls to share a "beat N of them" step.
pub const SHARE_RANGE: f32 = 40.0;

impl Sim {
    fn quest(&self, name: &str) -> Option<(usize, &proto::game::QuestFile)> {
        self.quests.iter().enumerate().find(|(_, q)| q.name == name)
    }

    fn bump_quests(&mut self, s: usize) {
        self.quest_ver[s] = self.quest_ver[s].wrapping_add(1);
    }

    /// How many of an item (by name) a player carries.
    fn carried(&self, s: usize, item: &str) -> u32 {
        let Some(k) = self.items.iter().position(|i| i.name == item) else { return 0 };
        self.inv[s].iter().filter(|b| b.0 as usize == k + 1).map(|b| b.1 as u32).sum()
    }

    /// Take items (by name) out of a bag.
    fn take(&mut self, s: usize, item: &str, mut n: u32) {
        let Some(k) = self.items.iter().position(|i| i.name == item) else { return };
        for b in self.inv[s].iter_mut() {
            if n == 0 {
                break;
            }
            if b.0 as usize == k + 1 {
                let t = (b.1 as u32).min(n);
                b.1 -= t as u8;
                n -= t;
                if b.1 == 0 {
                    b.0 = 0;
                }
            }
        }
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
    }

    /// Could this player take this quest now?
    fn can_take(&self, s: usize, q: &proto::game::QuestFile) -> bool {
        let mine = &self.ents.quests[s];
        let had = mine.iter().find(|r| r.name == q.name);
        let open = match had {
            None => true,
            Some(r) => r.state == QUEST_DONE && q.repeatable,
        };
        let level_ok = self.levels.is_none() || q.level <= self.ents.level[s];
        let after_ok = q.after.as_ref().is_none_or(|a| mine.iter().any(|r| &r.name == a && r.state == QUEST_DONE));
        open && level_ok && after_ok
    }

    /// A player talks to the bot `target` (an entity id): that may count as a step, take a quest handed in, or
    /// put a quest to them. False if there is nobody to talk to in reach.
    pub fn talk(&mut self, s: usize, target: u32) -> bool {
        let Some(t) = self.ents.resolve(target) else { return false };
        let (dx, dy) = (self.ents.x[t] - self.ents.x[s], self.ents.y[t] - self.ents.y[s]);
        if dx * dx + dy * dy > TALK_RANGE * TALK_RANGE || self.ents.owner[s] >= DRIVEN {
            return false;
        }
        let Some(arch) = self.archs.get(self.ents.arch[t] as usize).map(|a| a.name.clone()) else { return false };
        if let Some(why) = self.refuses(t, s) {
            self.tells.push((s, why));
            return true;
        }
        // A step that is just talking to one of these.
        let mut moved = false;
        for i in 0..self.ents.quests[s].len() {
            let run = &self.ents.quests[s][i];
            if run.state != QUEST_ACTIVE {
                continue;
            }
            if let Some((_, q)) = self.quest(&run.name) {
                if matches!(q.steps.get(run.step as usize), Some(QuestStep::Talk { archetype }) if *archetype == arch) {
                    self.advance(s, i);
                    moved = true;
                }
            }
        }
        // Handing in what is ready, to the one it goes back to.
        for i in 0..self.ents.quests[s].len() {
            let run = self.ents.quests[s][i].clone();
            if run.state != QUEST_READY {
                continue;
            }
            let Some((_, q)) = self.quest(&run.name) else { continue };
            if q.turn_in.as_ref().unwrap_or(&q.giver) != &arch {
                continue;
            }
            let q = q.clone();
            // Items asked for must still be carried.
            let short = q.steps.iter().find_map(|st| match st {
                QuestStep::Collect { item, count } if self.carried(s, item) < *count as u32 => Some(item.clone()),
                _ => None,
            });
            if let Some(item) = short {
                self.tells.push((s, format!("{}: you no longer carry enough {item}.", q.title)));
                continue;
            }
            for st in &q.steps {
                if let QuestStep::Collect { item, count } = st {
                    self.take(s, item, *count as u32);
                }
            }
            for r in &q.reward {
                if let Some(k) = self.items.iter().position(|i| i.name == r.item) {
                    self.give_or_drop(s, k, r.count as u32);
                }
            }
            self.ents.quests[s][i].state = QUEST_DONE;
            let said = if q.done_text.is_empty() { format!("Quest done: {}", q.title) } else { format!("{} ({} done)", q.done_text, q.title) };
            if q.xp > 0 && self.levels.is_some() {
                self.award(s, q.xp, Some(format!("{said} +{} experience", q.xp)));
            } else {
                self.tells.push((s, said));
            }
            moved = true;
        }
        // Otherwise, a quest of theirs to put to the player.
        if !moved {
            let offer = self.quests.iter().position(|q| q.giver == arch && self.can_take(s, q));
            if let Some(k) = offer {
                self.ents.offer[s] = k as u16;
            } else {
                self.tells.push((s, format!("{} has nothing for you now.", proto::names::bot(&arch))));
            }
        }
        self.bump_quests(s);
        true
    }

    /// Take the quest at `index` (one that was offered, or that the player could take from a giver in reach).
    pub fn accept(&mut self, s: usize, index: u16) -> bool {
        let i = index as usize;
        let Some(q) = self.quests.get(i) else { return false };
        if self.ents.offer[s] as usize != i || !self.can_take(s, q) {
            return false;
        }
        let held = self.ents.quests[s].iter().filter(|r| r.state != QUEST_DONE).count();
        if held >= proto::game::MAX_HELD_QUESTS {
            self.tells.push((s, format!("Your quest log is full ({held} quests). Hand one in or give one up first.")));
            return false;
        }
        let name = q.name.clone();
        let title = q.title.clone();
        self.ents.quests[s].retain(|r| r.name != name);
        self.ents.quests[s].push(crate::quests::QuestRun { name, step: 0, count: 0, state: QUEST_ACTIVE });
        self.ents.offer[s] = proto::wire::NO_OFFER;
        self.tells.push((s, format!("Quest taken: {title}")));
        self.bump_quests(s);
        let last = self.ents.quests[s].len() - 1;
        self.check_now(s, last);
        true
    }

    /// Give up a quest in hand. An offer can be turned down the same way (`wire::NO_OFFER` just clears it).
    pub fn abandon(&mut self, s: usize, index: u16) -> bool {
        self.ents.offer[s] = proto::wire::NO_OFFER;
        if let Some(q) = self.quests.get(index as usize) {
            let name = q.name.clone();
            self.ents.quests[s].retain(|r| !(r.name == name && r.state != QUEST_DONE));
        }
        self.bump_quests(s);
        true
    }

    /// The step a run is on is done: on to the next, or ready to hand in after the last.
    fn advance(&mut self, s: usize, i: usize) {
        let Some((_, q)) = self.quest(&self.ents.quests[s][i].name) else { return };
        let (n, title, back) = (q.steps.len(), q.title.clone(), q.turn_in.clone().unwrap_or(q.giver.clone()));
        let run = &mut self.ents.quests[s][i];
        run.step += 1;
        run.count = 0;
        if run.step as usize >= n {
            run.state = QUEST_READY;
            self.tells.push((s, format!("{title}: done. Go back to the {}.", proto::names::bot(&back))));
        }
        self.bump_quests(s);
        self.check_now(s, i);
    }

    /// Steps that may already be met when they start (carrying the items, standing in the place).
    fn check_now(&mut self, s: usize, i: usize) {
        let Some(run) = self.ents.quests[s].get(i).cloned() else { return };
        if run.state != QUEST_ACTIVE {
            return;
        }
        let Some(step) = self.quest(&run.name).and_then(|(_, q)| q.steps.get(run.step as usize).cloned()) else { return };
        let done = match &step {
            QuestStep::Visit { place } => self.places.iter().any(|p| {
                &p.name == place && (p.x - self.ents.x[s]).powi(2) + (p.y - self.ents.y[s]).powi(2) <= p.r * p.r
            }),
            QuestStep::Collect { item, count } => {
                let have = self.carried(s, item).min(*count as u32) as u16;
                if have != run.count {
                    self.ents.quests[s][i].count = have;
                    self.bump_quests(s);
                }
                have >= *count as u16
            }
            _ => false,
        };
        if done {
            self.advance(s, i);
        }
    }

    /// A bot of type `arch` has fallen at (x, y): a "beat N" step moves on for `who` (the players who hurt it)
    /// and for every player near it.
    pub fn quest_kill(&mut self, arch: &str, x: f32, y: f32, who: &[usize]) {
        if self.quests.is_empty() {
            return;
        }
        let r2 = SHARE_RANGE * SHARE_RANGE;
        let mut players: Vec<usize> = who.to_vec();
        for s in 0..self.ents.high {
            if self.ents.alive[s] && self.ents.owner[s] < DRIVEN && (self.ents.x[s] - x).powi(2) + (self.ents.y[s] - y).powi(2) <= r2 && !players.contains(&s) {
                players.push(s);
            }
        }
        for s in players {
            for i in 0..self.ents.quests[s].len() {
                let run = &self.ents.quests[s][i];
                if run.state != QUEST_ACTIVE {
                    continue;
                }
                let Some((_, q)) = self.quest(&run.name) else { continue };
                if let Some(QuestStep::Kill { archetype, count }) = q.steps.get(run.step as usize) {
                    if archetype == arch {
                        let need = *count;
                        self.ents.quests[s][i].count += 1;
                        self.bump_quests(s);
                        if self.ents.quests[s][i].count >= need {
                            self.advance(s, i);
                        }
                    }
                }
            }
        }
    }

    /// Once a second: places walked into and items gathered count for the steps waiting on them.
    pub fn system_quests(&mut self) {
        if self.quests.is_empty() {
            return;
        }
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= DRIVEN || self.ents.quests[s].is_empty() {
                continue;
            }
            for i in 0..self.ents.quests[s].len() {
                self.check_now(s, i);
            }
        }
    }

    /// A player's quests as the wire carries them (`wire::kind::QUESTS`), by index in the game's quests now.
    pub fn quest_states(&self, s: usize) -> Vec<proto::wire::QuestState> {
        self.ents.quests[s]
            .iter()
            .filter_map(|r| {
                self.quest(&r.name).map(|(k, _)| proto::wire::QuestState { quest: k as u16, step: r.step, count: r.count, state: r.state })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim, NONE};
    use proto::game::{GameFile, LevelsFile, QuestFile, QuestItem, QuestStep, ACT_ACCEPT, ACT_TALK};
    use proto::wire::{NO_OFFER, QUEST_DONE, QUEST_READY};
    use proto::world::World;

    #[test]
    fn a_quest_is_offered_taken_shared_done_and_handed_in() {
        let mut g = GameFile::default_game();
        g.levels = Some(LevelsFile { cap: 10, base: 1000.0, exponent: 1.0, might: 0.0, toughness: 0.0, discover_xp: 0 });
        let plaza = g.places.iter().find(|p| p.name == "plaza").unwrap().clone();
        g.quests.push(QuestFile {
            name: "cull".into(),
            title: "The cull".into(),
            text: "Thin the drifters.".into(),
            giver: "merchant".into(),
            steps: vec![QuestStep::Kill { archetype: "drifter".into(), count: 2 }, QuestStep::Visit { place: "plaza".into() }],
            xp: 50,
            reward: vec![QuestItem { item: "medkit".into(), count: 1 }],
            ..Default::default()
        });
        let merchant = g.archetypes.iter().position(|a| a.name == "merchant").unwrap() as u8;
        let drifter = g.archetypes.iter().position(|a| a.name == "drifter").unwrap() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        let friend = s.spawn(1).unwrap();
        let m = s.spawn_as(NONE, merchant, Some((100.0, 104.0))).unwrap();
        s.ents.x[me] = 100.0;
        s.ents.y[me] = 100.0;
        let mid = s.ents.id_of(m);
        let act = |s: &mut Sim, slot: usize, action: u8, target: u32, arg: u16| s.step(&[PendingAction { slot, action, target, arg }]);
        // Talking to the merchant puts the quest; taking it starts it.
        s.ents.x[m] = 100.0;
        s.ents.y[m] = 104.0;
        act(&mut s, me, ACT_TALK, mid, 0);
        assert_eq!(s.ents.offer[me], 0);
        act(&mut s, me, ACT_ACCEPT, 0, 0);
        assert_eq!((s.ents.offer[me], s.ents.quests[me].len()), (NO_OFFER, 1));
        // Two drifters fall to a friend standing nearby: it counts for us too.
        for _ in 0..2 {
            let d = s.spawn_as(NONE, drifter, None).unwrap();
            s.ents.x[d] = 110.0;
            s.ents.y[d] = 100.0;
            s.ents.x[friend] = 112.0;
            s.ents.y[friend] = 100.0;
            let (fid, now) = (s.ents.id_of(friend), s.tick);
            s.ents.hurt_by[d].push((fid, now));
            s.ents.attrs[0][d] = 0.0;
            s.step(&[]);
        }
        assert_eq!(s.ents.quests[me][0].step, 1, "on to the visit");
        s.ents.x[me] = plaza.x;
        s.ents.y[me] = plaza.y;
        for _ in 0..25 {
            s.step(&[]);
        }
        assert_eq!(s.ents.quests[me][0].state, QUEST_READY);
        // Back to the merchant: done, with the experience and the medkit.
        s.ents.x[me] = 100.0;
        s.ents.y[me] = 100.0;
        s.ents.x[m] = 100.0;
        s.ents.y[m] = 104.0;
        act(&mut s, me, ACT_TALK, mid, 0);
        assert_eq!(s.ents.quests[me][0].state, QUEST_DONE);
        assert_eq!(s.ents.xp[me], 50);
        assert!(s.inv[me].iter().any(|b| b.0 == 1 && b.1 == 1), "the medkit: {:?}", s.inv[me]);
        // Not offered again (it is not repeatable); saved by name.
        act(&mut s, me, ACT_TALK, mid, 0);
        assert_eq!(s.ents.offer[me], NO_OFFER);
        assert_eq!(s.record_of(me).quests, vec![("cull".to_string(), 2, 0, QUEST_DONE)]);
    }

    #[test]
    fn hundreds_of_quests_and_a_full_hand_is_refused() {
        let mut g = GameFile::default_game();
        for k in 0..proto::game::MAX_QUESTS {
            g.quests.push(QuestFile {
                name: format!("q{k}"),
                title: format!("Errand {k}"),
                giver: "merchant".into(),
                steps: vec![QuestStep::Talk { archetype: "drifter".into() }],
                ..Default::default()
            });
        }
        let merchant = g.archetypes.iter().position(|a| a.name == "merchant").unwrap() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        let m = s.spawn_as(NONE, merchant, Some((100.0, 104.0))).unwrap();
        let mid = s.ents.id_of(m);
        let act = |s: &mut Sim, action: u8, target: u32, arg: u16| s.step(&[PendingAction { slot: me, action, target, arg }]);
        for _ in 0..=proto::game::MAX_HELD_QUESTS {
            (s.ents.x[me], s.ents.y[me], s.ents.x[m], s.ents.y[m]) = (100.0, 100.0, 100.0, 104.0);
            act(&mut s, ACT_TALK, mid, 0);
            let offer = s.ents.offer[me];
            assert_ne!(offer, NO_OFFER);
            act(&mut s, ACT_ACCEPT, 0, offer);
        }
        assert_eq!(s.ents.quests[me].len(), proto::game::MAX_HELD_QUESTS, "the one past a full hand is refused");
        // Indices past 255 reach the wire.
        s.ents.quests[me][0].name = "q400".into();
        assert!(s.quest_states(me).iter().any(|q| q.quest == 400));
    }
}

//! Quests: bots of a type offer them, players do their steps in order and hand them in for the rewards.
//!
//! Everything is decided here on the server. A client says "I talk to this one", "I take quest 3", "I give up
//! quest 3" through the input action byte (`ACT_TALK`, `ACT_ACCEPT`, `ACT_ABANDON`), and is told where it stands
//! in a QUESTS packet whenever that changes. Progress is kept by quest name, so editing the game file never
//! scrambles anyone's quests.

use crate::sim::{Sim, DRIVEN};
use proto::game::QuestStep;
use proto::wire::{QUEST_ACTIVE, QUEST_DONE, QUEST_READY};

/// One of a player's quests: on which step, how far into it, and whether it is ready to hand in or done.
#[derive(Clone, Debug, PartialEq)]
pub struct QuestRun {
    pub name: String,
    pub step: u8,
    pub count: u16,
    pub state: u8,
}


/// Today, as days since 1970 (UTC), for daily quests.
fn today() -> u16 {
    (proto::daylight::now_ms() / 86_400_000.0) as u16
}

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
            // A daily quest done keeps the day it was handed in where its count was.
            Some(r) => r.state == QUEST_DONE && (q.repeatable || (q.daily && r.count != today())),
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
        if dx * dx + dy * dy > self.rules.talk_reach * self.rules.talk_reach || self.ents.owner[s] >= DRIVEN {
            return false;
        }
        let Some(arch) = self.archs.get(self.ents.arch[t] as usize).map(|a| a.name.clone()) else { return false };
        if let Some(why) = self.refuses(t, s) {
            self.tells.push((s, why));
            return true;
        }
        self.hold_for(t, s); // it stands while you talk
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
            self.deed(s, "quests", 1);
            if q.daily {
                self.ents.quests[s][i].count = today();
            }
            if let Some(h) = self.town_of_bot(t) {
                self.change_repute(s, h, 10); // a good turn for one of its folk
            }
            let said = if q.done_text.is_empty() { format!("Quest done: {}", q.title) } else { format!("{} ({} done)", q.done_text, q.title) };
            if !q.closing.is_empty() {
                self.notes.push((s, 0, proto::chat::channel::BANNER, q.closing.clone()));
            }
            if !q.done_text.is_empty() && (!q.opening.is_empty() || !q.closing.is_empty()) {
                self.npc_say.push((t, false, q.done_text.chars().take(proto::chat::MAX_TEXT).collect())); // a story's giver speaks
            }
            if q.xp > 0 && self.levels.is_some() {
                self.award(s, q.xp, Some(format!("{said} +{} experience", q.xp)));
            } else {
                self.tells.push((s, said));
            }
            moved = true;
        }
        // Otherwise, a quest of theirs to put to the player.
        if !moved {
            // A quest tied to a place is offered only by its givers who live there (`QuestFile::offered_at`).
            let home = self.home_of(t).and_then(|h| self.places.get(h)).map(|p| p.name.as_str());
            // One never done before one done before (a repeatable or daily quest early in the list would otherwise
            // be all a giver ever offered, once done).
            let can = |q: &proto::game::QuestFile| q.giver == arch && q.offered_at.as_deref().is_none_or(|p| home == Some(p)) && self.can_take(s, q);
            let new = |q: &proto::game::QuestFile| !self.ents.quests[s].iter().any(|r| r.name == q.name);
            let offer = self.quests.iter().position(|q| can(q) && new(q)).or_else(|| self.quests.iter().position(can));
            if let Some(k) = offer {
                self.ents.offer[s] = k as u16;
            } else {
                self.tells.push((s, format!("{} has nothing for you now.", proto::game::shown_kind(&self.archs, &arch))));
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
        let opening = q.opening.clone();
        self.ents.quests[s].retain(|r| r.name != name);
        self.ents.quests[s].push(crate::quests::QuestRun { name, step: 0, count: 0, state: QUEST_ACTIVE });
        self.ents.offer[s] = proto::wire::NO_OFFER;
        self.tells.push((s, format!("Quest taken: {title}")));
        if !opening.is_empty() {
            self.notes.push((s, 0, proto::chat::channel::BANNER, opening));
        }
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
            self.tells.push((s, format!("{title}: done. Go back to the {}.", proto::game::shown_kind(&self.archs, &back))));
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
        let r2 = self.rules.quest_share_reach * self.rules.quest_share_reach;
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

    /// The player in `s` has built a block of material `kind`: a "build N" step of that material (or of any) moves on.
    pub fn quest_build(&mut self, s: usize, kind: u8) {
        if self.quests.is_empty() {
            return;
        }
        let name = proto::game::BLOCK_NAMES.get(kind as usize).copied().unwrap_or("");
        for i in 0..self.ents.quests[s].len() {
            let run = &self.ents.quests[s][i];
            if run.state != QUEST_ACTIVE {
                continue;
            }
            let Some((_, q)) = self.quest(&run.name) else { continue };
            if let Some(QuestStep::Build { count, material }) = q.steps.get(run.step as usize) {
                if material.as_deref().is_none_or(|m| m == name) {
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
    use super::QuestRun;
    use proto::wire::{NO_OFFER, QUEST_ACTIVE, QUEST_DONE, QUEST_READY};
    use proto::world::World;

    #[test]
    fn a_quest_is_offered_taken_shared_done_and_handed_in() {
        let mut g = GameFile::default_game();
        g.levels = Some(LevelsFile { cap: 10, base: 1000.0, exponent: 1.0, might: 0.0, toughness: 0.0, discover_xp: 0, life: Default::default(), rewards: vec![], grow: Default::default() });
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
    fn building_blocks_of_the_asked_material_moves_a_build_step_on() {
        let mut g = GameFile::default_game();
        g.quests.push(QuestFile {
            name: "shelter".into(),
            title: "A shelter".into(),
            giver: "merchant".into(),
            steps: vec![QuestStep::Build { count: 2, material: Some("timber".into()) }, QuestStep::Build { count: 1, material: None }],
            ..Default::default()
        });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        s.ents.quests[me].push(QuestRun { name: "shelter".into(), step: 0, count: 0, state: QUEST_ACTIVE });
        let timber = proto::game::BLOCK_NAMES.iter().position(|&n| n == "timber").unwrap() as u8;
        s.quest_build(me, 0); // stone: not what is asked
        assert_eq!(s.ents.quests[me][0].count, 0);
        s.quest_build(me, timber);
        s.quest_build(me, timber);
        assert_eq!(s.ents.quests[me][0].step, 1, "on to building anything");
        s.quest_build(me, 0);
        assert_eq!(s.ents.quests[me][0].state, QUEST_READY);
    }

    #[test]
    fn a_giver_offers_a_quest_not_done_before_one_to_repeat() {
        let mut g = GameFile::default_game();
        for (name, repeatable) in [("again", true), ("once", false)] {
            g.quests.push(QuestFile { name: name.into(), title: name.into(), giver: "merchant".into(), steps: vec![QuestStep::Visit { place: "plaza".into() }], repeatable, ..Default::default() });
        }
        let merchant = g.archetypes.iter().position(|a| a.name == "merchant").unwrap() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        let m = s.spawn_as(NONE, merchant, Some((100.0, 104.0))).unwrap();
        (s.ents.x[me], s.ents.y[me], s.ents.x[m], s.ents.y[m]) = (100.0, 100.0, 100.0, 104.0);
        s.ents.quests[me].push(QuestRun { name: "again".into(), step: 1, count: 0, state: QUEST_DONE });
        let id = s.ents.id_of(m);
        s.talk(me, id);
        assert_eq!(s.ents.offer[me], 1, "the one not done yet");
        s.ents.quests[me].push(QuestRun { name: "once".into(), step: 1, count: 0, state: QUEST_DONE });
        s.talk(me, id);
        assert_eq!(s.ents.offer[me], 0, "then the repeatable again");
    }

    #[test]
    fn a_story_quest_opens_with_a_scene() {
        let mut g = GameFile::default_game();
        g.quests.push(QuestFile { name: "tale".into(), title: "A Tale".into(), giver: "merchant".into(), steps: vec![QuestStep::Talk { archetype: "drifter".into() }], opening: "Once upon a time".into(), ..Default::default() });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        s.ents.offer[me] = 0;
        assert!(s.accept(me, 0));
        assert!(s.notes.iter().any(|n| n.0 == me && n.2 == proto::chat::channel::BANNER && n.3 == "Once upon a time"), "{:?}", s.notes);
    }

    #[test]
    fn a_quest_offered_at_a_place_comes_only_from_givers_who_live_there() {
        let mut g = GameFile::default_game();
        let other = g.places.iter().find(|p| p.name != "market").unwrap().name.clone();
        g.quests.push(QuestFile { name: "away".into(), title: "Away".into(), giver: "merchant".into(), offered_at: Some(other.clone()), steps: vec![QuestStep::Visit { place: other }], ..Default::default() });
        g.quests.push(QuestFile { name: "here".into(), title: "Here".into(), giver: "merchant".into(), offered_at: Some("market".into()), steps: vec![QuestStep::Talk { archetype: "drifter".into() }], ..Default::default() });
        let merchant = g.archetypes.iter().position(|a| a.name == "merchant").unwrap() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        let m = s.spawn_as(NONE, merchant, Some((100.0, 104.0))).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (100.0, 100.0);
        assert_eq!(s.places[s.home_of(m).unwrap()].name, "market");
        assert!(s.talk(me, s.ents.id_of(m)));
        assert_eq!(s.ents.offer[me], 1, "the market's merchant offers the market's errand, not the other place's");
    }

    #[test]
    fn a_daily_quest_comes_back_the_next_day() {
        let mut g = GameFile::default_game();
        g.quests.push(QuestFile { name: "bounty".into(), title: "Bounty".into(), giver: "merchant".into(), steps: vec![QuestStep::Talk { archetype: "drifter".into() }], daily: true, ..Default::default() });
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        let q = s.quests[0].clone();
        assert!(s.can_take(me, &q));
        s.ents.quests[me].push(QuestRun { name: "bounty".into(), step: 1, count: super::today(), state: QUEST_DONE });
        assert!(!s.can_take(me, &q), "done today");
        s.ents.quests[me][0].count = super::today() - 1;
        assert!(s.can_take(me, &q), "done yesterday");
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

    /// Plays Hibernia's quests from Áth Cliath as one new character would: talk to every kind of giver in the city,
    /// take what is offered, do each step (beating what is asked or what drops what is asked, at the nearest place
    /// it lives; going to places; speaking to folk), hand in, and again until nothing more is offered. Prints each
    /// quest with the level it was taken at and how far it sends the player, and fails on a quest that cannot be
    /// finished or a character left short of the levels. Slow (the whole game file):
    /// `cargo test -p server --release --bin server hibernia_quests_play_through -- --ignored --nocapture`
    /// (`GAME=path` for another game file).
    #[test]
    #[ignore]
    fn hibernia_quests_play_through() {
        let path = std::env::var("GAME").unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../deploy/hibernia-65536/game.json").to_string());
        let g: GameFile = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(65536.0, 1, vec![]), 4096, 20, 1);
        let start = s.start_place.expect("a start place");
        let (sx, sy) = (s.places[start].x, s.places[start].y);
        let me = s.spawn(0).unwrap();
        s.give_starter(me);
        let arch_of = |s: &Sim, n: &str| s.archs.iter().position(|a| a.name == n);
        let dist = |s: &Sim, h: usize| (s.places[h].x - sx).hypot(s.places[h].y - sy);
        // How far from the start the nearest of a kind lives (or its dungeon's door); None for nowhere in particular.
        let lair = |s: &Sim, a: usize| -> Option<f32> {
            let d = s.archs[a].homes.iter().map(|&h| dist(s, h)).fold(None, |m: Option<f32>, d| Some(m.map_or(d, |m| m.min(d))));
            d.or_else(|| s.realm.dungeons.iter().find(|dg| dg.bots.iter().any(|&(b, _)| b == a)).map(|dg| dist(s, dg.place)))
        };
        // One of each kind, kept beside the player in the start place (and living there, for town-only quests).
        let mut folk: std::collections::HashMap<usize, usize> = Default::default();
        let mut beside = |s: &mut Sim, a: usize| -> u32 {
            let t = *folk.entry(a).or_insert_with(|| s.spawn_as(NONE, a as u8, Some((sx + 2.0, sy))).unwrap());
            s.home_over[t] = Some(start);
            s.ents.x[me] = sx;
            s.ents.y[me] = sy;
            s.ents.x[t] = sx + 2.0;
            s.ents.y[t] = sy;
            s.ents.id_of(t)
        };
        let givers: Vec<usize> = {
            let mut v: Vec<usize> = s.quests.iter().filter_map(|q| arch_of(&s, &q.giver)).collect();
            v.sort();
            v.dedup();
            v
        };
        let mut log: Vec<String> = Vec::new();
        let mut stuck: Vec<String> = Vec::new();
        let mut taken_at: std::collections::HashMap<String, (u8, f32, u32)> = Default::default();
        for _round in 0..400 {
            let mut moved = false;
            // Take everything offered.
            for &a in &givers {
                for _ in 0..8 {
                    let id = beside(&mut s, a);
                    s.ents.offer[me] = NO_OFFER;
                    s.talk(me, id);
                    let k = s.ents.offer[me];
                    // A repeatable or daily quest once only: the play-through is of everything there is to do.
                    if k == NO_OFFER || taken_at.contains_key(&s.quests[k as usize].name) || !s.accept(me, k) {
                        break;
                    }
                    taken_at.insert(s.quests[k as usize].name.clone(), (s.ents.level[me], 0.0, 0));
                    moved = true;
                }
            }
            // Do each step of what is in hand.
            for i in 0..s.ents.quests[me].len() {
                for _ in 0..12 {
                    let run = s.ents.quests[me][i].clone();
                    if run.state != QUEST_ACTIVE {
                        break;
                    }
                    let q = s.quests.iter().find(|q| q.name == run.name).unwrap().clone();
                    let Some(step) = q.steps.get(run.step as usize).cloned() else { break };
                    let far = |_: &Sim, d: Option<f32>| d.unwrap_or(0.0);
                    let mut walk = 0.0f32;
                    let mut kills = 0u32;
                    match &step {
                        QuestStep::Kill { archetype, count } => {
                            let Some(a) = arch_of(&s, archetype) else { stuck.push(format!("{}: no {archetype}", q.name)); break };
                            walk = far(&s, lair(&s, a));
                            for _ in run.count..*count {
                                s.quest_kill(archetype, sx, sy, &[me]);
                                let xp = s.archs[a].xp;
                                s.award(me, xp, None);
                                kills += 1;
                            }
                        }
                        QuestStep::Visit { place } => {
                            let Some(h) = s.places.iter().position(|p| &p.name == place) else { stuck.push(format!("{}: no place {place}", q.name)); break };
                            walk = dist(&s, h);
                            s.ents.x[me] = s.places[h].x;
                            s.ents.y[me] = s.places[h].y;
                            s.system_quests();
                            s.ents.x[me] = sx;
                            s.ents.y[me] = sy;
                        }
                        QuestStep::Talk { archetype } => {
                            let Some(a) = arch_of(&s, archetype) else { stuck.push(format!("{}: no {archetype}", q.name)); break };
                            walk = far(&s, lair(&s, a));
                            let id = beside(&mut s, a);
                            s.talk(me, id);
                        }
                        QuestStep::Build { count, material } => {
                            let k = material.as_deref().and_then(|m| proto::game::BLOCK_NAMES.iter().position(|&n| n == m)).unwrap_or(0) as u8;
                            for _ in run.count..*count {
                                s.quest_build(me, k);
                            }
                        }
                        QuestStep::Collect { item, count } => {
                            let Some(it) = s.items.iter().position(|i| &i.name == item) else { stuck.push(format!("{}: no item {item}", q.name)); break };
                            // Beaten for (the nearest that drops it), bought in the start place, grown, made, or a
                            // quest's reward.
                            let dropper = (0..s.archs.len())
                                .filter_map(|a| s.archs[a].drops.iter().find(|d| d.item == it).map(|d| (a, d.chance_pct / 100.0 * (d.min as f32 + d.max as f32) / 2.0)))
                                .filter_map(|(a, per)| lair(&s, a).map(|d| (a, per, d)))
                                .min_by(|x, y| x.2.total_cmp(&y.2));
                            let sold = s.archs.iter().any(|a| a.shop.iter().any(|&(k, _)| k == it) && a.homes.contains(&start));
                            let grown = s.realm.crops.iter().any(|c| c.yields == it) || s.realm.recipes.iter().any(|r| r.makes == it);
                            let from_quest = s.quests.iter().any(|o| o.name != q.name && o.reward.iter().any(|r| &r.item == item));
                            match dropper {
                                Some((a, per, d)) if !sold && !grown => {
                                    walk = d;
                                    kills = ((*count as f32) / per.max(0.01)).ceil() as u32;
                                    let xp = s.archs[a].xp;
                                    for _ in 0..kills {
                                        s.award(me, xp, None);
                                    }
                                }
                                None if !sold && !grown && !from_quest => {
                                    stuck.push(format!("{}: nowhere to get {item}", q.name));
                                    break;
                                }
                                _ => {}
                            }
                            // Sold or banked: what the quests in hand do not ask for (rewards fill a bag).
                            let wanted: Vec<u8> = s.ents.quests[me].iter().filter(|r| r.state != QUEST_DONE).filter_map(|r| s.quests.iter().find(|q| q.name == r.name)).flat_map(|q| q.steps.iter()).filter_map(|st| match st {
                                QuestStep::Collect { item, .. } => s.items.iter().position(|i| &i.name == item).map(|k| k as u8 + 1),
                                _ => None,
                            }).collect();
                            for b in s.inv[me].iter_mut() {
                                if !wanted.contains(&b.0) {
                                    *b = (0, 0);
                                }
                            }
                            s.give_or_drop(me, it, *count as u32);
                            s.system_quests();
                        }
                    }
                    if let Some(t) = taken_at.get_mut(&q.name) {
                        t.1 = t.1.max(walk);
                        t.2 += kills;
                    }
                    if s.ents.quests[me][i].step == run.step && s.ents.quests[me][i].state == run.state {
                        stuck.push(format!("{}: step {} did not move ({step:?})", q.name, run.step));
                        break;
                    }
                    moved = true;
                }
            }
            // Hand in what is ready.
            for i in 0..s.ents.quests[me].len() {
                let run = s.ents.quests[me][i].clone();
                if run.state != QUEST_READY {
                    continue;
                }
                let q = s.quests.iter().find(|q| q.name == run.name).unwrap().clone();
                let Some(a) = arch_of(&s, q.turn_in.as_ref().unwrap_or(&q.giver)) else { continue };
                let id = beside(&mut s, a);
                s.talk(me, id);
                if s.ents.quests[me][i].state != QUEST_DONE {
                    // What it asks for went to another quest that wanted the same: fetched again.
                    for st in &q.steps {
                        if let QuestStep::Collect { item, count } = st {
                            if let Some(it) = s.items.iter().position(|x| &x.name == item) {
                                s.give_or_drop(me, it, *count as u32);
                            }
                        }
                    }
                    let id = beside(&mut s, a);
                    s.talk(me, id);
                }
                if s.ents.quests[me][i].state == QUEST_DONE {
                    let (lvl, walk, kills) = taken_at.get(&q.name).copied().unwrap_or_default();
                    log.push(format!("{:>3} {:>3} {:>6.1} km {:>4} kills  {:34} -> level {}", q.level, lvl, walk / 1000.0, kills, q.name, s.ents.level[me]));
                    moved = true;
                } else {
                    stuck.push(format!("{}: could not hand in", q.name));
                }
            }
            stuck.sort();
            stuck.dedup();
            if !moved {
                break;
            }
        }
        println!("lvl took  farthest  kills  quest -> level after");
        for l in &log {
            println!("{l}");
        }
        let done: std::collections::HashSet<&str> = s.ents.quests[me].iter().filter(|r| r.state == QUEST_DONE).map(|r| r.name.as_str()).collect();
        let left: Vec<String> = s.quests.iter().filter(|q| !done.contains(q.name.as_str()) && q.offered_at.as_deref().is_none_or(|p| s.places[start].name == p)).map(|q| format!("{} (level {}, from {})", q.name, q.level, q.giver)).collect();
        println!("{} done, level {} ({} xp); not done from here: {}", log.len(), s.ents.level[me], s.ents.xp[me], left.join(", "));
        assert!(stuck.is_empty(), "stuck: {stuck:#?}");
        assert!(s.ents.level[me] >= 15, "only level {} from the quests", s.ents.level[me]);
    }
}

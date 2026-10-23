//! Encounters: a bot and a player who come to blows are locked together. The first blow either way locks the bot
//! onto that player; from then on its rules aim at them (past its sight, whoever else is nearer), and a bot with an
//! attack fights back whatever its rules say, unless a rule tells it to flee. The lock breaks when one of them
//! falls, when the player gets further away than the bot's `leash`, reaches a safe place, or when no blow has
//! passed between them for `GIVE_UP_SECS`: the bot gives up and goes back to its rules.
//!
//! Two players who come to blows (pvp) are in a fight with each other until one of them falls or neither has
//! landed a blow on the other for `PVP_QUIET_SECS`. Nobody chases anybody there: it only says who is fighting.
//!
//! Each player is told who is in a fight with them, bots and players (`wire::kind::ENCOUNTER`), which is what
//! clients play the battle music on.

use crate::sim::{Sim, DRIVEN, NONE};
use proto::wire::MAX_PURSUERS;

/// A chase with no blow landing either way ends after this long.
pub const GIVE_UP_SECS: u32 = 10;
/// Two players are out of their fight once neither has landed a blow on the other for this long.
pub const PVP_QUIET_SECS: u32 = 8;

impl Sim {
    fn is_player(&self, s: usize) -> bool {
        self.ents.owner[s] < DRIVEN
    }

    /// A bot the AI runs (not one the admin drives, not a mirror of another zone's).
    fn is_ai_bot(&self, s: usize) -> bool {
        self.ents.owner[s] == NONE && self.ents.arch[s] != crate::sim::NO_ARCH
    }

    /// One hurt the other (either way round): a bot and a player lock together, or a lock already there is kept
    /// fresh. A bot already locked on someone else stays on them.
    pub(crate) fn engage(&mut self, a: usize, b: usize) {
        if self.is_player(a) && self.is_player(b) {
            self.rival(a, b);
            self.rival(b, a);
            return;
        }
        let (bot, player) = if self.is_ai_bot(a) && self.is_player(b) {
            (a, b)
        } else if self.is_ai_bot(b) && self.is_player(a) {
            (b, a)
        } else {
            return;
        };
        let pid = self.ents.id_of(player);
        match self.foe[bot] {
            Some(f) if f == pid => {
                self.foe_at[bot] = self.tick;
                return;
            }
            Some(f) if self.ents.resolve(f).is_some() => return,
            _ => {}
        }
        if self.pursuers[player].len() >= MAX_PURSUERS {
            return;
        }
        self.foe[bot] = Some(pid);
        self.foe_at[bot] = self.tick;
        self.pursuers[player].push(self.ents.id_of(bot));
        self.pursuer_ver[player] = self.pursuer_ver[player].wrapping_add(1);
    }

    /// `p` is in a fight with the player `o`: noted, or the last blow between them brought up to now.
    fn rival(&mut self, p: usize, o: usize) {
        let oid = self.ents.id_of(o);
        if let Some(r) = self.rivals[p].iter_mut().find(|r| r.0 == oid) {
            r.1 = self.tick;
        } else if self.rivals[p].len() < MAX_PURSUERS {
            self.rivals[p].push((oid, self.tick));
            self.pursuer_ver[p] = self.pursuer_ver[p].wrapping_add(1);
        }
    }

    /// Everyone in a fight with a player, as the ENCOUNTER packet names them: the bots locked on them, then the
    /// players fighting them.
    pub fn fighting(&self, p: usize) -> Vec<u32> {
        self.pursuers[p].iter().copied().chain(self.rivals[p].iter().map(|r| r.0)).collect()
    }

    /// The player a bot is locked on, if still there: (distance squared, slot).
    pub(crate) fn locked(&self, bot: usize) -> Option<(f32, usize)> {
        let p = self.ents.resolve(self.foe[bot]?)?;
        let (dx, dy) = (self.ents.x[p] - self.ents.x[bot], self.ents.y[p] - self.ents.y[bot]);
        Some((dx * dx + dy * dy, p))
    }

    /// A bot lets go of the player it is locked on.
    fn release(&mut self, bot: usize) {
        let Some(f) = self.foe[bot].take() else { return };
        if let Some(p) = self.ents.resolve(f) {
            let id = self.ents.id_of(bot);
            self.pursuers[p].retain(|&b| b != id);
            self.pursuer_ver[p] = self.pursuer_ver[p].wrapping_add(1);
        }
    }

    /// Someone fell: every fight they were in is over (a bot's lock, the bots locked on a player, the players
    /// fighting them).
    pub(crate) fn end_encounters(&mut self, s: usize) {
        self.release(s);
        if !self.rivals[s].is_empty() {
            let id = self.ents.id_of(s);
            for (o, _) in std::mem::take(&mut self.rivals[s]) {
                if let Some(o) = self.ents.resolve(o) {
                    self.rivals[o].retain(|r| r.0 != id);
                    self.pursuer_ver[o] = self.pursuer_ver[o].wrapping_add(1);
                }
            }
            self.pursuer_ver[s] = self.pursuer_ver[s].wrapping_add(1);
        }
        if self.pursuers[s].is_empty() {
            return;
        }
        for b in std::mem::take(&mut self.pursuers[s]) {
            if let Some(b) = self.ents.resolve(b) {
                self.foe[b] = None;
            }
        }
        self.pursuer_ver[s] = self.pursuer_ver[s].wrapping_add(1);
    }

    /// Once a second: bots give up a chase that has got away from them, or gone quiet, or reached a safe place;
    /// players fall out of fights with players that have gone quiet; players stop hearing of those that have gone.
    pub fn system_encounters(&mut self) {
        let quiet = GIVE_UP_SECS * self.tick_hz;
        let calm = PVP_QUIET_SECS * self.tick_hz;
        for p in 0..self.ents.high {
            if self.rivals[p].is_empty() {
                continue;
            }
            let list = std::mem::take(&mut self.rivals[p]);
            let before = list.len();
            let mut kept = Vec::with_capacity(before);
            for (o, at) in list {
                let Some(os) = self.ents.resolve(o).filter(|&o| self.is_player(o)) else { continue }; // they left
                if self.tick.saturating_sub(at) <= calm {
                    kept.push((o, at));
                } else {
                    let name = self.ents.name[os].clone();
                    self.tells.push((p, format!("You and {} break off the fight.", name.as_deref().unwrap_or("your foe"))));
                }
            }
            if kept.len() != before {
                self.pursuer_ver[p] = self.pursuer_ver[p].wrapping_add(1);
            }
            self.rivals[p] = kept;
        }
        for b in 0..self.ents.high {
            let Some(f) = self.foe[b] else { continue };
            let Some(p) = self.ents.resolve(f).filter(|&p| self.is_player(p)) else {
                self.foe[b] = None; // they left
                continue;
            };
            if !self.ents.alive[b] || !self.is_ai_bot(b) {
                self.release(b);
                continue;
            }
            let leash = self.archs.get(self.ents.arch[b] as usize).map_or(proto::game::MIN_LEASH, |a| a.leash);
            let (dx, dy) = (self.ents.x[p] - self.ents.x[b], self.ents.y[p] - self.ents.y[b]);
            let why = if dx * dx + dy * dy > leash * leash {
                Some("gives up the chase")
            } else if self.in_safe(p) {
                Some("will not follow you there")
            } else if self.tick.saturating_sub(self.foe_at[b]) > quiet {
                Some("loses interest")
            } else {
                None
            };
            if let Some(why) = why {
                if let Some(arch) = self.archs.get(self.ents.arch[b] as usize) {
                    self.tells.push((p, format!("The {} {why}.", proto::names::bot(&arch.name))));
                }
                self.release(b);
            }
        }
        // Bots that went without letting go (removed, handed to another zone).
        for p in 0..self.ents.high {
            if self.pursuers[p].is_empty() {
                continue;
            }
            let pid = self.ents.id_of(p);
            let before = self.pursuers[p].len();
            let mut list = std::mem::take(&mut self.pursuers[p]);
            list.retain(|&b| self.ents.resolve(b).is_some_and(|b| self.foe[b] == Some(pid)));
            if list.len() != before {
                self.pursuer_ver[p] = self.pursuer_ver[p].wrapping_add(1);
            }
            self.pursuers[p] = list;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::{PendingAction, Sim, NONE};
    use proto::game::{GameFile, Place};
    use proto::world::World;

    use proto::defs::ACTION_ZAP as ZAP;

    /// A world with a drone (a bot type that chases and zaps) and a player standing by it.
    fn fight() -> (Sim, usize, usize) {
        let g = GameFile::default_game();
        let drone = g.archetypes.iter().position(|a| a.name == "drone").unwrap() as u8;
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        let me = s.spawn(0).unwrap();
        let d = s.spawn_as(NONE, drone, Some((300.0, 300.0))).unwrap();
        (s.ents.x[me], s.ents.y[me]) = (304.0, 300.0);
        (s, me, d)
    }

    #[test]
    fn a_blow_locks_and_death_breaks_it() {
        let (mut s, me, d) = fight();
        let did = s.ents.id_of(d);
        s.step(&[PendingAction { slot: me, action: ZAP, target: did, arg: 0 }]);
        assert_eq!(s.foe[d], Some(s.ents.id_of(me)), "hitting it locks it on");
        assert_eq!(s.pursuers[me], vec![did]);
        // The player falls: the fight is over for both.
        s.ents.attrs[0][me] = 0.0;
        s.step(&[]);
        assert_eq!((s.foe[d], s.pursuers[me].len()), (None, 0));
    }

    #[test]
    fn being_hit_locks_too_and_the_bot_keeps_after_you_past_its_sight() {
        let (mut s, me, d) = fight();
        let mid = s.ents.id_of(me);
        // The drone's own zap lands on us.
        s.step(&[PendingAction { slot: d, action: ZAP, target: mid, arg: 0 }]);
        assert_eq!(s.foe[d], Some(mid));
        // Out past its sight (40) but inside its leash (120): it still comes for us, not for a nearer player.
        let other = s.spawn(1).unwrap();
        (s.ents.x[other], s.ents.y[other]) = (290.0, 300.0);
        (s.ents.x[me], s.ents.y[me]) = (380.0, 300.0);
        (s.ents.x[d], s.ents.y[d]) = (300.0, 300.0);
        for _ in 0..12 {
            s.step(&[]);
        }
        assert!(s.ents.x[d] > 300.0, "it chased east, after us: {}", s.ents.x[d]);
        assert_eq!(s.foe[d], Some(mid));
    }

    #[test]
    fn running_far_enough_or_into_a_safe_place_or_going_quiet_ends_it() {
        // Out of its leash.
        let (mut s, me, d) = fight();
        s.engage(d, me);
        (s.ents.x[me], s.ents.y[me]) = (900.0, 900.0);
        s.system_encounters();
        assert_eq!(s.foe[d], None);
        assert!(s.tells.iter().any(|t| t.0 == me && t.1.contains("gives up the chase")), "{:?}", s.tells);
        // Into a safe place.
        let (mut s, me, d) = fight();
        s.places.push(Place { name: "sanctuary".into(), x: 320.0, y: 300.0, r: 10.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: true });
        s.engage(d, me);
        (s.ents.x[me], s.ents.y[me]) = (320.0, 300.0);
        s.system_encounters();
        assert_eq!(s.foe[d], None);
        // Nothing landing for a while.
        let (mut s, me, d) = fight();
        s.engage(d, me);
        s.tick += super::GIVE_UP_SECS * s.tick_hz + 1;
        s.system_encounters();
        assert_eq!((s.foe[d], s.pursuers[me].len()), (None, 0));
    }

    #[test]
    fn players_who_trade_blows_are_in_a_fight_till_one_falls_or_both_stop() {
        let (mut s, a, _) = fight();
        let b = s.spawn(1).unwrap();
        let (aid, bid) = (s.ents.id_of(a), s.ents.id_of(b));
        s.engage(b, a); // a hit b
        assert_eq!((s.fighting(a), s.fighting(b)), (vec![bid], vec![aid]), "both are in it, whoever struck");
        // Blows keep it going; once neither lands one for a while, it is over for both.
        s.tick += super::PVP_QUIET_SECS * s.tick_hz;
        s.engage(a, b);
        s.system_encounters();
        assert_eq!(s.fighting(a), vec![bid], "a blow just now");
        s.tick += super::PVP_QUIET_SECS * s.tick_hz + 1;
        s.system_encounters();
        assert!(s.fighting(a).is_empty() && s.fighting(b).is_empty());
        assert!(s.tells.iter().any(|t| t.0 == a && t.1.contains("break off")), "{:?}", s.tells);
        // One falls: over at once, for the other too.
        s.engage(a, b);
        s.ents.attrs[0][b] = 0.0;
        s.step(&[]);
        assert!(s.rivals[a].is_empty() && s.rivals[b].is_empty());
    }

    #[test]
    fn a_removed_bot_drops_out_of_the_list() {
        let (mut s, me, d) = fight();
        s.engage(me, d);
        let v = s.pursuer_ver[me];
        s.ents.despawn(d);
        s.system_encounters();
        assert!(s.pursuers[me].is_empty());
        assert_ne!(s.pursuer_ver[me], v);
    }
}

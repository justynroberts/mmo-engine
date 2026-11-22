//! Encounters: a bot and a player who come to blows are locked together. The first blow either way locks the bot
//! onto that player; from then on its rules aim at them (past its sight, whoever else is nearer), and a bot with an
//! attack fights back whatever its rules say, unless a rule tells it to flee. The lock breaks when one of them
//! falls, when the player gets further away than the bot's `leash`, reaches a safe place, or when no blow has
//! passed between them for `Rules::chase_give_up_secs`: the bot gives up and goes back to its rules.
//!
//! Every bot keeps a hate list (`Sim::hate`, a boss its threat in `boss.rs`): the first blow or sight of a foe puts
//! them on it, damage done to it adds the damage, healing someone on it adds half the heal to the healer, and a
//! taunt puts its user on top. It turns on whoever passes the one it is on by a tenth, so a tank can hold it; when
//! the one it is on falls, gets away or reaches a safe place, it goes for the next on its list instead of giving up.
//! A bot that starts a fight calls others of its faction (else its own type) within `Rules::assist_reach` into it,
//! the nearest `HELPERS` of them, once: they join without calling more.
//!
//! Two players who come to blows (pvp) are in a fight with each other until one of them falls or neither has
//! landed a blow on the other for `Rules::pvp_quiet_secs`. Nobody chases anybody there: it only says who is fighting.
//!
//! Each player is told who is in a fight with them, bots and players (`wire::kind::ENCOUNTER`), which is what
//! clients play the battle music on.

use crate::sim::{Sim, DRIVEN, NONE};
use proto::wire::MAX_PURSUERS;

/// Hate a newcomer must have over the one a bot is on, as a share of theirs, before it turns (as a boss does).
const TURN: f32 = 1.1;
/// Most that answer one bot's call for help (the nearest): a camp of brigands stood shoulder to shoulder sends a
/// few, not all of them.
const HELPERS: usize = 3;


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
        if self.ents.god[player] {
            return; // bots pay a god no mind
        }
        self.boss_engaged(bot, player);
        let pid = self.ents.id_of(player);
        let fresh = self.foe[bot].is_none() && self.hate.get(&self.ents.id_of(bot)).map_or(true, |h| h.is_empty());
        self.note_hate(bot, player);
        match self.foe[bot] {
            Some(f) if f == pid => {
                self.foe_at[bot] = self.tick;
                return;
            }
            Some(f) if self.ents.resolve(f).is_some() => return,
            _ => {}
        }
        if self.lock(bot, player) && fresh {
            self.call_for_help(bot, player);
        }
    }

    /// A bot locks on a player (unless too many are on them already).
    fn lock(&mut self, bot: usize, player: usize) -> bool {
        if self.pursuers[player].len() >= MAX_PURSUERS {
            return false;
        }
        self.foe[bot] = Some(self.ents.id_of(player));
        self.foe_at[bot] = self.tick;
        self.pursuers[player].push(self.ents.id_of(bot));
        self.pursuer_ver[player] = self.pursuer_ver[player].wrapping_add(1);
        true
    }

    fn is_boss(&self, s: usize) -> bool {
        self.archs.get(self.ents.arch[s] as usize).is_some_and(|a| a.boss.is_some())
    }

    /// A player on a bot's hate list (a boss keeps its own threat).
    fn note_hate(&mut self, bot: usize, player: usize) {
        if self.is_boss(bot) {
            return;
        }
        let pid = self.ents.id_of(player);
        let list = self.hate.entry(self.ents.id_of(bot)).or_default();
        if !list.iter().any(|t| t.0 == pid) {
            list.push((pid, 1.0));
        }
    }

    /// Hate on a bot from a player: `amount` more, or (taunting) straight to the top; it turns on them if due.
    pub(crate) fn add_hate(&mut self, bot: usize, player: usize, amount: f32, taunt: bool) {
        if !self.is_ai_bot(bot) || !self.is_player(player) || self.ents.god[player] || !self.ents.alive[bot] || self.is_boss(bot) {
            return;
        }
        self.note_hate(bot, player);
        let pid = self.ents.id_of(player);
        let Some(list) = self.hate.get_mut(&self.ents.id_of(bot)) else { return };
        let top = list.iter().map(|t| t.1).fold(0.0f32, f32::max);
        if let Some(t) = list.iter_mut().find(|t| t.0 == pid) {
            t.1 += amount.max(0.0);
            if taunt {
                t.1 = t.1.max(top * TURN + 1.0);
            }
        }
        self.turn_if_due(bot);
    }

    /// A bot goes for the top of its hate list when that passes the one it is on by a tenth (or it is on nobody).
    fn turn_if_due(&mut self, bot: usize) {
        let Some(list) = self.hate.get(&self.ents.id_of(bot)) else { return };
        let Some(top) = list.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)) else { return };
        let cur = self.foe[bot].filter(|&f| self.ents.resolve(f).is_some()).and_then(|c| list.iter().find(|t| t.0 == c).copied());
        if let Some(c) = cur {
            if c.0 == top.0 || top.1 <= c.1 * TURN {
                return;
            }
        }
        let Some(t) = self.ents.resolve(top.0).filter(|&t| self.ents.alive[t] && !self.ents.fallen[t]) else { return };
        let was = self.foe[bot].is_some();
        self.release(bot);
        if self.lock(bot, t) && was {
            if let Some(arch) = self.archs.get(self.ents.arch[bot] as usize) {
                self.tells.push((t, format!("The {} turns on you!", arch.shown())));
            }
        }
    }

    /// A bot starting a fight calls others of its side near it into it: those of its faction, or of its own type
    /// when it has none, that can fight and are in no fight.
    fn call_for_help(&mut self, bot: usize, player: usize) {
        let reach = self.rules.assist_reach;
        if reach <= 0.0 || self.is_boss(bot) {
            return;
        }
        let Some(me) = self.archs.get(self.ents.arch[bot] as usize) else { return };
        if me.attack.is_none() {
            return;
        }
        let (faction, kind) = (me.faction, self.ents.arch[bot]);
        let (x, y) = (self.ents.x[bot], self.ents.y[bot]);
        let cells = (reach / self.grid.cell).ceil().max(1.0) as usize;
        let mut helpers = Vec::new();
        {
            let (e, archs, foe, phase) = (&self.ents, &self.archs, &self.foe, &self.rs.phase);
            self.grid.for_within(x, y, cells, |t| {
                let t = t as usize;
                if t == bot || foe[t].is_some() || !e.alive[t] || e.fallen[t] || e.hidden[t] || e.owner[t] != NONE || phase[t] != phase[bot] {
                    return;
                }
                let Some(a) = archs.get(e.arch[t] as usize) else { return };
                let side = match faction {
                    Some(f) => a.faction == Some(f),
                    None => e.arch[t] == kind,
                };
                let d2 = (e.x[t] - x).powi(2) + (e.y[t] - y).powi(2);
                if side && a.attack.is_some() && a.boss.is_none() && d2 <= reach * reach {
                    helpers.push((d2, t));
                }
            });
        }
        helpers.sort_by(|a, b| a.0.total_cmp(&b.0));
        let helpers: Vec<usize> = helpers.into_iter().take(HELPERS).map(|h| h.1).collect();
        let mut joined = Vec::new();
        for h in helpers {
            self.note_hate(h, player);
            if self.lock(h, player) {
                joined.push(h);
            }
        }
        let shown = |s: &Sim, b: usize| s.archs.get(s.ents.arch[b] as usize).map_or("foe".to_string(), |a| a.shown().to_string());
        match joined.len() {
            0 => {}
            1 => {
                let line = format!("A {} joins the fight!", shown(self, joined[0]));
                self.tells.push((player, line));
            }
            n => {
                let same = joined.iter().all(|&j| self.ents.arch[j] == self.ents.arch[joined[0]]);
                let line = if same { format!("{n} more {} join the fight!", shown(self, joined[0])) } else { format!("{n} more join the fight!") };
                self.tells.push((player, line));
            }
        }
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
    pub(crate) fn release(&mut self, bot: usize) {
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
        if !self.hate.is_empty() {
            let id = self.ents.id_of(s);
            self.hate.remove(&id);
            // Off every list it was on; those it was the one on go for the next (below, once the locks are let go).
            for list in self.hate.values_mut() {
                list.retain(|t| t.0 != id);
            }
        }
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
        let bots = std::mem::take(&mut self.pursuers[s]);
        for &b in &bots {
            if let Some(b) = self.ents.resolve(b) {
                self.foe[b] = None;
            }
        }
        self.pursuer_ver[s] = self.pursuer_ver[s].wrapping_add(1);
        for b in bots {
            if let Some(b) = self.ents.resolve(b) {
                self.turn_if_due(b);
            }
        }
    }

    /// Once a second: hate lists lose the bots that are gone or down and the players that are gone, down, past the
    /// bot's leash or in a safe place; a bot on nobody goes for the next on its list; an empty list is dropped.
    fn tidy_hate(&mut self) {
        if self.hate.is_empty() {
            return;
        }
        let mut lists = std::mem::take(&mut self.hate);
        let mut idle = Vec::new();
        lists.retain(|&bid, list| {
            let Some(b) = self.ents.resolve(bid).filter(|&b| self.ents.alive[b] && !self.ents.fallen[b] && self.is_ai_bot(b)) else { return false };
            let leash = self.archs.get(self.ents.arch[b] as usize).map_or(proto::game::MIN_LEASH, |a| a.leash);
            list.retain(|t| {
                self.ents.resolve(t.0).is_some_and(|p| {
                    self.ents.alive[p]
                        && !self.ents.fallen[p]
                        && !self.ents.god[p]
                        && (self.ents.x[p] - self.ents.x[b]).powi(2) + (self.ents.y[p] - self.ents.y[b]).powi(2) <= leash * leash
                        && !self.in_safe(p)
                })
            });
            if self.foe[b].is_none() && !list.is_empty() {
                idle.push(b);
            }
            !list.is_empty() || self.foe[b].is_some()
        });
        self.hate = lists;
        for b in idle {
            self.turn_if_due(b);
        }
    }

    /// Once a second: bots give up a chase that has got away from them, or gone quiet, or reached a safe place;
    /// players fall out of fights with players that have gone quiet; players stop hearing of those that have gone.
    pub fn system_encounters(&mut self) {
        let quiet = proto::rules::Rules::ticks(self.rules.chase_give_up_secs, self.tick_hz);
        let calm = proto::rules::Rules::ticks(self.rules.pvp_quiet_secs, self.tick_hz);
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
                    self.tells.push((p, format!("The {} {why}.", arch.shown())));
                }
                self.release(b);
                let bid = self.ents.id_of(b);
                if why == "loses interest" {
                    self.hate.remove(&bid); // nobody has struck it a while
                } else if let Some(list) = self.hate.get_mut(&bid) {
                    list.retain(|t| t.0 != f);
                }
                self.turn_if_due(b); // the next on its list, if any
            }
        }
        self.tidy_hate();
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
    fn bots_pay_a_god_no_mind_and_let_go_when_god_mode_starts() {
        let (mut s, me, d) = fight();
        let did = s.ents.id_of(d);
        s.step(&[PendingAction { slot: me, action: ZAP, target: did, arg: 0 }]);
        assert_eq!(s.pursuers[me], vec![did], "locked on");
        s.set_god(me, true);
        assert!(s.pursuers[me].is_empty() && s.foe[d].is_none(), "god mode ends the chase");
        for _ in 0..40 {
            s.step(&[PendingAction { slot: me, action: ZAP, target: did, arg: 0 }]);
        }
        assert!(s.pursuers[me].is_empty() && s.foe[d].is_none(), "a god's blows lock nothing on");
        s.set_god(me, false);
        assert!(s.pursuers[me].is_empty(), "out of god mode with nothing on our heels");
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
        s.places.push(Place { name: "sanctuary".into(), x: 320.0, y: 300.0, r: 10.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: true, hold: None, dock: false });
        s.engage(d, me);
        (s.ents.x[me], s.ents.y[me]) = (320.0, 300.0);
        s.system_encounters();
        assert_eq!(s.foe[d], None);
        // Nothing landing for a while.
        let (mut s, me, d) = fight();
        s.engage(d, me);
        s.tick += proto::rules::Rules::ticks(s.rules.chase_give_up_secs, s.tick_hz) + 1;
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
        s.tick += proto::rules::Rules::ticks(s.rules.pvp_quiet_secs, s.tick_hz);
        s.engage(a, b);
        s.system_encounters();
        assert_eq!(s.fighting(a), vec![bid], "a blow just now");
        s.tick += proto::rules::Rules::ticks(s.rules.pvp_quiet_secs, s.tick_hz) + 1;
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

    /// A second player beside the first, both by the drone.
    fn two() -> (Sim, usize, usize, usize) {
        let (mut s, me, d) = fight();
        let you = s.spawn(1).unwrap();
        (s.ents.x[you], s.ents.y[you]) = (296.0, 300.0);
        (s, me, you, d)
    }

    #[test]
    fn a_bot_turns_on_whoever_hurts_it_most_and_a_taunt_takes_it_back() {
        let (mut s, me, you, d) = two();
        let (mid, yid) = (s.ents.id_of(me), s.ents.id_of(you));
        s.add_to(d, 0, -2.0, mid);
        assert_eq!(s.foe[d], Some(mid), "the first blow locks it");
        s.add_to(d, 0, -2.0, yid);
        assert_eq!(s.foe[d], Some(mid), "level with the one it is on: it stays");
        s.add_to(d, 0, -3.0, yid);
        assert_eq!(s.foe[d], Some(yid), "past them by a tenth: it turns");
        assert_eq!((s.pursuers[me].len(), s.pursuers[you].len()), (0, 1));
        assert!(s.tells.iter().any(|t| t.0 == you && t.1.contains("turns on you")), "{:?}", s.tells);
        s.add_threat(d, me, 0.0, true);
        assert_eq!(s.foe[d], Some(mid), "a taunt puts them on top");
    }

    #[test]
    fn healing_the_one_it_fights_draws_its_hate() {
        let (mut s, me, you, d) = two();
        let (mid, yid) = (s.ents.id_of(me), s.ents.id_of(you));
        s.add_to(d, 0, -4.0, mid);
        s.ents.attrs[0][me] = 10.0;
        s.add_to(me, 0, 20.0, yid); // you mend me
        let list = &s.hate[&s.ents.id_of(d)];
        assert!(list.iter().any(|t| t.0 == yid && t.1 > 1.0), "half of what was mended: {list:?}");
        assert_eq!(s.foe[d], Some(yid), "more than I did to it");
    }

    #[test]
    fn when_the_one_it_is_on_falls_it_goes_for_the_next_on_its_list() {
        let (mut s, me, you, d) = two();
        let (mid, yid) = (s.ents.id_of(me), s.ents.id_of(you));
        s.add_to(d, 0, -4.0, mid);
        s.add_to(d, 0, -1.0, yid);
        assert_eq!(s.foe[d], Some(mid));
        s.ents.attrs[0][me] = 0.0;
        s.step(&[]);
        assert_eq!(s.foe[d], Some(yid), "on to the next");
        assert!(!s.hate[&s.ents.id_of(d)].iter().any(|t| t.0 == mid), "the fallen are off its list");
        // Running out of its leash: off the list, and with nobody left the list goes.
        (s.ents.x[you], s.ents.y[you]) = (900.0, 900.0);
        s.system_encounters();
        assert_eq!(s.foe[d], None);
        assert!(!s.hate.contains_key(&s.ents.id_of(d)));
    }

    #[test]
    fn a_bot_that_starts_a_fight_calls_its_kind_near_it_once() {
        let (mut s, me, d) = fight();
        let mid = s.ents.id_of(me);
        s.rules.assist_reach = 30.0;
        let drone = s.ents.arch[d];
        let near = s.spawn_as(NONE, drone, Some((320.0, 300.0))).unwrap();
        let far = s.spawn_as(NONE, drone, Some((380.0, 300.0))).unwrap();
        (s.ents.x[near], s.ents.y[near]) = (320.0, 300.0);
        (s.ents.x[far], s.ents.y[far]) = (380.0, 300.0);
        (s.ents.x[me], s.ents.y[me]) = (900.0, 900.0); // out of their sight while the grid is built
        s.step(&[]);
        (s.ents.x[me], s.ents.y[me]) = (304.0, 300.0);
        s.add_to(d, 0, -2.0, mid);
        assert_eq!(s.foe[near], Some(mid), "the one beside it joins");
        assert_eq!(s.foe[far], None, "out of reach: it does not");
        assert!(s.tells.iter().any(|t| t.0 == me && t.1.contains("joins the fight")), "{:?}", s.tells);
        // However many stand by, the nearest three come.
        let (mut s2, me2, d2) = fight();
        s2.rules.assist_reach = 30.0;
        let crowd: Vec<usize> = (0..6).map(|i| s2.spawn_as(NONE, drone, Some((302.0 + i as f32 * 3.0, 302.0))).unwrap()).collect();
        for (i, &c) in crowd.iter().enumerate() {
            (s2.ents.x[c], s2.ents.y[c]) = (302.0 + i as f32 * 3.0, 302.0);
        }
        (s2.ents.x[me2], s2.ents.y[me2]) = (900.0, 900.0);
        s2.step(&[]);
        (s2.ents.x[me2], s2.ents.y[me2]) = (304.0, 300.0);
        s2.add_to(d2, 0, -2.0, s2.ents.id_of(me2));
        let came: Vec<usize> = crowd.iter().copied().filter(|&c| s2.foe[c].is_some()).collect();
        assert_eq!(came, crowd[..3].to_vec(), "the nearest three");
        // A blow on one already fighting calls nobody more.
        s.add_to(near, 0, -2.0, mid);
        assert_eq!(s.foe[far], None);
        // Off, every bot fights alone.
        let (mut s, me, d) = fight();
        let other = s.spawn_as(NONE, drone, Some((310.0, 300.0))).unwrap();
        (s.ents.x[other], s.ents.y[other]) = (310.0, 300.0);
        (s.ents.x[me], s.ents.y[me]) = (900.0, 900.0);
        s.step(&[]);
        (s.ents.x[me], s.ents.y[me]) = (304.0, 300.0);
        s.add_to(d, 0, -2.0, s.ents.id_of(me));
        assert_eq!(s.foe[other], None);
    }

    #[test]
    fn a_fighter_lets_a_player_far_over_its_level_be_until_struck() {
        let (mut s, me, d) = fight();
        s.rules.calm_levels = 4;
        s.ents.level[d] = 2;
        s.ents.level[me] = 6;
        let hp = s.ents.attrs[0][me];
        for _ in 0..30 {
            s.step(&[]);
        }
        assert_eq!(s.foe[d], None, "it pays them no mind");
        assert!(s.ents.attrs[0][me] >= hp, "nor strikes them");
        s.ents.level[me] = 5; // three over: still fair game
        for _ in 0..30 {
            s.step(&[]);
        }
        assert!(s.foe[d].is_some() || s.ents.attrs[0][me] < s.defs.attrs[0].max, "it comes for them");
    }
}

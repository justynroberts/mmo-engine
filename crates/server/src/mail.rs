//! Mail: letters to a character by name, whether or not they are on, with coin or goods sent along.
//!
//! `/mail Name words` writes a letter from anywhere. `/send Name count item` sends goods from your bag (the game's
//! currency is an item like any other, so `/send Name 50 gold` sends coin); it must be done at a market, as the
//! bank is. `/mailbox` reads what has come: letters anywhere, and at a market what was sent along goes into the bag
//! (what does not fit waits). A player is told when a letter comes, and again when they next come into the world.
//! Letters are kept with the realm's save, by the account tag of who they are for (`market::Market::mail`).

use serde::{Deserialize, Serialize};

use crate::sim::{Sim, DRIVEN};

/// Longest letter, in characters.
pub const LETTER_MAX: usize = 200;
/// Most letters read out at once by `/mailbox`.
const READ_AT_ONCE: usize = 8;
/// Most letters kept in all.
const MAIL_MAX: usize = 20_000;

/// A letter waiting for someone.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Letter {
    /// The account tag it is for, and the name of who sent it.
    pub to: String,
    pub from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Goods sent along (an item by name), and how many; none when `count` is 0.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub item: String,
    #[serde(default)]
    pub count: u32,
    /// Unix seconds it was sent.
    pub sent: u64,
    /// The one it is for has been told it came (since the server started).
    #[serde(skip)]
    pub told: bool,
}

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

impl Sim {
    /// Letters waiting for an account.
    pub fn letters_for(&self, tag: &str) -> usize {
        self.market.mail.iter().filter(|l| l.to == tag).count()
    }

    /// Post a letter (and goods, already taken from the sender) to the account `to`. Err says why not, and nothing
    /// was posted.
    fn post_letter(&mut self, letter: Letter) -> Result<(), String> {
        if self.letters_for(&letter.to) >= self.rules.mail_waiting as usize || self.market.mail.len() >= MAIL_MAX {
            return Err("Their mailbox is full.".into());
        }
        self.market.mail.push(letter);
        self.rs.dirty = true;
        Ok(())
    }

    /// `/mail`: a letter of words from `s` to the account `to` (called `to_name`).
    pub fn send_letter(&mut self, s: usize, to: &str, to_name: &str, text: &str) -> String {
        let Some(me) = self.tag_of(s).filter(|_| self.ents.owner[s] < DRIVEN) else { return "Guests cannot send letters.".into() };
        if me == to {
            return "You cannot write to yourself.".into();
        }
        let text: String = text.chars().filter(|c| !c.is_control()).take(LETTER_MAX).collect();
        if text.trim().is_empty() {
            return "Write what? /mail Name your words".into();
        }
        let from = self.name_of(s).to_string();
        match self.post_letter(Letter { to: to.to_string(), from, text: text.trim().to_string(), sent: unix_now(), ..Default::default() }) {
            Ok(()) => format!("Your letter is on its way to {to_name}."),
            Err(why) => why,
        }
    }

    /// `/send`: `count` of the item called `item` from `s`'s bag to the account `to`, at a market.
    pub fn send_goods(&mut self, s: usize, to: &str, to_name: &str, count: u32, item: &str) -> String {
        let Some(me) = self.tag_of(s).filter(|_| self.ents.owner[s] < DRIVEN) else { return "Guests cannot send anything.".into() };
        if me == to {
            return "You cannot send to yourself.".into();
        }
        if !self.at_market(s) {
            return "Goods are sent from a market: a safe place, or beside a merchant.".into();
        }
        let want = item.trim().to_lowercase();
        let Some(i) = self.items.iter().position(|d| d.name.to_lowercase() == want || d.shown().to_lowercase() == want) else {
            return format!("There is no such thing as {item}.");
        };
        if count == 0 {
            return "Send how many? /send Name 10 gold".into();
        }
        let tag = i as u8 + 1;
        let have: u32 = self.inv[s].iter().filter(|b| b.0 == tag).map(|b| b.1 as u32).sum();
        if have < count {
            return format!("You have only {have} {}.", self.items[i].shown());
        }
        if self.letters_for(to) >= self.rules.mail_waiting as usize {
            return "Their mailbox is full.".into();
        }
        crate::market::take(&mut self.inv[s], tag, count);
        self.inv_ver[s] = self.inv_ver[s].wrapping_add(1);
        let (from, name, shown) = (self.name_of(s).to_string(), self.items[i].name.clone(), self.items[i].shown().to_string());
        match self.post_letter(Letter { to: to.to_string(), from, item: name, count, sent: unix_now(), ..Default::default() }) {
            Ok(()) => format!("{count} {shown} on its way to {to_name}."),
            Err(why) => {
                crate::market::add(&mut self.inv[s], tag, count, self.items[i].stack_max); // the bag it came from has room
                why
            }
        }
    }

    /// `/mailbox`: read what has come for `s`. Lines to say to them.
    pub fn read_mail(&mut self, s: usize) -> Vec<String> {
        let Some(me) = self.tag_of(s).filter(|_| self.ents.owner[s] < DRIVEN) else { return vec!["Guests get no letters.".into()] };
        let market = self.at_market(s);
        let mut lines = Vec::new();
        let mut k = 0;
        let mut read = 0;
        while k < self.market.mail.len() && read < READ_AT_ONCE {
            if self.market.mail[k].to != me {
                k += 1;
                continue;
            }
            read += 1;
            let l = self.market.mail[k].clone();
            let mut line = if l.text.is_empty() { format!("From {}:", l.from) } else { format!("From {}: {}", l.from, l.text) };
            let mut left = l.count;
            if l.count > 0 {
                match self.items.iter().position(|d| d.name == l.item) {
                    None => left = 0, // an item the game no longer has
                    Some(i) if market => {
                        left = self.give(s, i, l.count);
                        if left < l.count {
                            line.push_str(&format!(" [{} {} into your bag]", l.count - left, self.items[i].shown()));
                        }
                        if left > 0 {
                            line.push_str(&format!(" [{left} more wait: your bag is full]"));
                        }
                    }
                    Some(i) => line.push_str(&format!(" [{} {} wait for you at a market]", l.count, self.items[i].shown())),
                }
            }
            lines.push(line);
            if left == 0 {
                self.market.mail.remove(k);
            } else {
                // Read: the words are not said again, only what still waits.
                let m = &mut self.market.mail[k];
                m.text.clear();
                m.count = left;
                k += 1;
            }
            self.rs.dirty = true;
        }
        if lines.is_empty() {
            lines.push("No letters.".into());
        } else {
            let more = self.letters_for(&me);
            let unread = self.market.mail.iter().filter(|l| l.to == me && !l.text.is_empty()).count();
            if unread > 0 {
                lines.push(format!("{unread} more letters: /mailbox again."));
            } else if more > 0 && !market {
                lines.push("Go to a market (a safe place, or beside a merchant) to take what was sent.".into());
            }
        }
        lines
    }

    /// Every few seconds: those with letters they have not been told of are told.
    pub fn system_mail(&mut self) {
        if self.market.mail.is_empty() || self.tick % (5 * self.tick_hz.max(1)) != 0 {
            return;
        }
        self.system_mail_now();
    }

    /// Tell those with letters they have not been told of, now.
    pub fn system_mail_now(&mut self) {
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= DRIVEN {
                continue;
            }
            let Some(me) = self.tag_of(s) else { continue };
            let mut from = Vec::new();
            for l in self.market.mail.iter_mut().filter(|l| l.to == me && !l.told) {
                l.told = true;
                if !from.contains(&l.from) {
                    from.push(l.from.clone());
                }
            }
            if !from.is_empty() {
                from.truncate(3);
                self.tells.push((s, format!("Mail has come for you from {}: /mailbox to read it.", from.join(", "))));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::{GameFile, ItemDef, Place};
    use proto::world::World;

    /// Two players with accounts, one at a market; gold is the last item and the currency. (Sim, a, b, gold)
    fn world() -> (Sim, usize, usize, usize) {
        let mut g = GameFile::default_game();
        g.items.push(ItemDef { name: "gold".into(), color: "#ffd700".into(), stack_max: 99, uses: Vec::new(), consume: false, value: 0, equip: None, mount: 0.0, rarity: Default::default(), title: String::new(), block: None, dye: None });
        let gold = g.items.len() - 1;
        g.currency = Some("gold".into());
        g.places.push(Place { name: "square".into(), x: 100.0, y: 100.0, r: 20.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: true, hold: None, dock: false });
        for a in g.archetypes.iter_mut() {
            a.count = 0;
        }
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        s.set_account(a, Some("aaaa"));
        s.set_account(b, Some("bbbb"));
        s.set_name(a, "Aoife", true);
        (s.ents.x[a], s.ents.y[a]) = (100.0, 100.0);
        (s.ents.x[b], s.ents.y[b]) = (600.0, 600.0);
        (s, a, b, gold)
    }

    #[test]
    fn a_letter_waits_is_told_of_and_is_read_once() {
        let (mut s, a, b, _) = world();
        assert!(s.send_letter(a, "bbbb", "Brid", "Meet me at the well").contains("on its way"));
        assert_eq!(s.send_letter(a, "aaaa", "Aoife", "hi"), "You cannot write to yourself.");
        s.tick = 5 * s.tick_hz - 1;
        s.step(&[]);
        assert!(s.tells.iter().any(|t| t.0 == b && t.1.contains("Mail has come for you from Aoife")), "{:?}", s.tells);
        let lines = s.read_mail(b);
        assert_eq!(lines[0], "From Aoife: Meet me at the well");
        assert!(s.market.mail.is_empty(), "read and gone");
        assert_eq!(s.read_mail(b), vec!["No letters.".to_string()]);
    }

    #[test]
    fn goods_go_from_a_market_and_are_taken_at_one() {
        let (mut s, a, b, gold) = world();
        s.give(a, gold, 30);
        assert!(s.send_goods(a, "bbbb", "Brid", 50, "gold").contains("only 30"));
        assert!(s.send_goods(a, "bbbb", "Brid", 20, "Gold").contains("on its way"));
        assert_eq!(s.inv[a].iter().filter(|x| x.0 as usize == gold + 1).map(|x| x.1 as u32).sum::<u32>(), 10);
        // Read away from a market: the coin waits.
        let lines = s.read_mail(b);
        assert!(lines[0].contains("wait for you at a market"), "{lines:?}");
        assert_eq!(s.market.mail.len(), 1);
        // At one: into the bag.
        (s.ents.x[b], s.ents.y[b]) = (100.0, 100.0);
        let lines = s.read_mail(b);
        assert!(lines[0].contains("20 gold into your bag"), "{lines:?}");
        assert!(s.market.mail.is_empty());
        // Not from out in the wilds.
        (s.ents.x[a], s.ents.y[a]) = (600.0, 600.0);
        assert!(s.send_goods(a, "bbbb", "Brid", 5, "gold").contains("from a market"));
    }

    #[test]
    fn a_full_mailbox_refuses_and_keeps_the_goods() {
        let (mut s, a, _, gold) = world();
        s.rules.mail_waiting = 1;
        s.give(a, gold, 10);
        assert!(s.send_letter(a, "bbbb", "Brid", "one").contains("on its way"));
        assert_eq!(s.send_letter(a, "bbbb", "Brid", "two"), "Their mailbox is full.");
        assert_eq!(s.send_goods(a, "bbbb", "Brid", 5, "gold"), "Their mailbox is full.");
        assert_eq!(s.inv[a].iter().filter(|x| x.0 as usize == gold + 1).map(|x| x.1 as u32).sum::<u32>(), 10);
    }

    #[test]
    fn players_come_into_the_world_at_the_start_place_and_a_lost_spot_sends_them_there() {
        let mut g = GameFile::default_game();
        g.places.push(Place { name: "gate".into(), x: 800.0, y: 200.0, r: 10.0, title: String::new(), exit: None, sound: String::new(), music: String::new(), safe: true, hold: None, dock: false });
        g.start_place = Some("gate".into());
        for a in g.archetypes.iter_mut() {
            a.count = 0;
        }
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(1024.0, 1, vec![]), 64, 20, 1);
        for k in 0..5 {
            let p = s.spawn(k).unwrap();
            assert!((s.ents.x[p] - 800.0).hypot(s.ents.y[p] - 200.0) <= 10.0, "at the gate: {} {}", s.ents.x[p], s.ents.y[p]);
            // A saved spot off the map (an old, smaller world's) is not taken: they stay at the gate.
            assert!(!s.put(p, 5000.0, 5000.0));
        }
    }
}

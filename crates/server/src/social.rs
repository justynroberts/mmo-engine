//! Talking to people rather than to a place: whispers, who is on, and parties.
//!
//! A chat line that starts with "/" is a command, not something said aloud: `/w Name text` whispers, `/r text`
//! answers the last whisper, `/who` lists who is on, `/rep` shows your standing with the factions, and
//! `/invite Name`, `/accept`, `/decline`, `/leave`, `/p text` and `/party` make and run a party, and `/raid` makes it
//! a raid, which may grow to `rules.raid_size`. `/queue` and `/unqueue` are the group finder (`finder.rs`). `/mail`, `/send` and
//! `/mailbox` are mail (`mail.rs`). A party shares the experience of a bot that falls near its
//! members and their "beat N" quest steps, and its members see each other in a PARTY packet.

use crate::net::Clients;
use crate::sim::{Sim, DRIVEN};
use proto::chat::channel;


impl Sim {
    /// A player's party mates (slots, themselves included), or just them.
    pub fn party_of(&self, s: usize) -> Vec<usize> {
        let p = self.party[s];
        if p == 0 {
            return vec![s];
        }
        (0..self.ents.high).filter(|&o| self.ents.alive[o] && self.ents.owner[o] < DRIVEN && self.party[o] == p).collect()
    }

    /// How many the party `slot` is in may hold: a raid's size once it is one.
    pub fn party_cap(&self, slot: usize) -> usize {
        let p = self.party[slot];
        (if p != 0 && self.raids.contains(&p) { self.rules.raid_size } else { self.rules.party_size }) as usize
    }

    fn bump_party(&mut self, members: &[usize]) {
        for &m in members {
            self.party_ver[m] = self.party_ver[m].wrapping_add(1);
        }
    }

    /// Leave a party (it ends when one is left).
    pub fn leave_party(&mut self, s: usize) -> Vec<usize> {
        let mates = self.party_of(s);
        let p = self.party[s];
        self.party[s] = 0;
        let rest: Vec<usize> = mates.into_iter().filter(|&m| m != s).collect();
        if rest.len() == 1 {
            self.party[rest[0]] = 0;
        }
        if rest.len() <= 1 {
            self.raids.remove(&p);
        }
        self.bump_party(&rest);
        self.party_ver[s] = self.party_ver[s].wrapping_add(1);
        rest
    }
}

/// `/social`: what the Godot social window shows, on chat channel SOCIAL (see `proto::chat::channel::SOCIAL`).
fn social_window(sim: &Sim, clients: &mut Clients, slot: usize) {
    let clip = |t: &str, n: usize| -> String { t.chars().filter(|c| *c != '|').take(n).collect() };
    let mut lines = vec!["X|".to_string()];
    let me = sim.tag_of(slot);
    // A member by account tag: their name if they are on, else the name their identity claimed, else "a member".
    let name_of = |tag: &str| -> String {
        if let Some(s) = (0..sim.ents.high).find(|&s| sim.ents.alive[s] && sim.ents.owner[s] < DRIVEN && sim.tag_of(s).as_deref() == Some(tag)) {
            return sim.name_of(s).to_string();
        }
        clients.names.iter().find(|(_, id)| proto::secure::account_tag(id) == tag).map_or_else(|| "a member".into(), |(n, _)| n.clone())
    };
    match me.as_deref().and_then(|t| sim.rs.guild_of(t)) {
        Some(g) => {
            let guild = &sim.rs.guilds[g];
            lines.push(format!("G|{}|{}|{}|{}", clip(&guild.name, 32), clip(&name_of(&guild.leader), 24), guild.members.len(), (Some(&guild.leader) == me.as_ref()) as u8));
            for m in guild.members.iter().take(24) {
                lines.push(format!("GM|{}", clip(&name_of(m), 24)));
            }
        }
        None => lines.push("G|".into()),
    }
    let letters: Vec<&crate::mail::Letter> = me.as_deref().map(|t| sim.market.mail.iter().filter(|l| l.to == t).collect()).unwrap_or_default();
    lines.push(format!("M|{}", letters.len()));
    for l in letters.iter().take(6) {
        let goods = if l.count > 0 { format!("{} {}", l.count, sim.items.iter().find(|i| i.name == l.item).map_or(l.item.as_str(), |i| i.shown())) } else { String::new() };
        lines.push(format!("ML|{}|{}|{}", clip(&l.from, 24), clip(&goods, 30), clip(&l.text, 50)));
    }
    for f in 0..sim.factions.len().min(10) {
        let v = sim.standing(slot, f);
        lines.push(format!("S|{}|{v}|{}", clip(&sim.factions[f].shown(), 40), sim.factions[f].mood(v)));
    }
    for k in 0..sim.realm.skills.len() {
        lines.push(format!("K|{}|{}|{}", clip(sim.realm.skills[k].shown(), 30), sim.skill_level(slot, k), sim.realm.skills[k].max));
    }
    let mut towns: Vec<(String, i32)> = sim.repute[slot].iter().filter_map(|(n, v)| sim.places.iter().position(|p| &p.name == n).map(|h| (sim.place_called(h), *v))).collect();
    towns.sort();
    for (t, v) in towns.into_iter().take(12) {
        lines.push(format!("T|{}|{v}|{}", clip(&t, 40), crate::repute::Tier::of(v).word()));
    }
    lines.push("E|".into());
    for l in lines {
        tell(clients, slot, 0, channel::SOCIAL, &l);
    }
}

/// A player by display name, ignoring case.
fn find(sim: &Sim, name: &str) -> Option<usize> {
    let want = name.trim().to_lowercase();
    (0..sim.ents.high).find(|&s| {
        sim.ents.alive[s] && sim.ents.owner[s] < DRIVEN && sim.ents.name[s].as_deref().is_some_and(|n| n.to_lowercase() == want)
    })
}

/// Whom a letter goes to: a claimed name (on or not), else someone on now by name. Their account tag.
fn recipient(sim: &Sim, clients: &Clients, name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return None;
    }
    if let Some(id) = clients.names.get(&proto::names::key(name)) {
        return Some(proto::secure::account_tag(id));
    }
    find(sim, name).and_then(|s| sim.tag_of(s))
}

fn tell(clients: &mut Clients, slot: usize, from: u32, ch: u8, text: &str) {
    if let Some(c) = clients.slab.iter_mut().flatten().find(|c| c.slot == slot) {
        c.chat.deliver(from, ch, text);
    }
}

fn say(clients: &mut Clients, slot: usize, text: &str) {
    tell(clients, slot, 0, channel::SAY, text);
}

/// Split "/invite Some Name" into ("invite", "Some Name"). Names may have spaces, so the rest is one argument,
/// except for whispers (the first word after the name is the start of the message: names in a whisper are
/// written as one word, or quoted).
fn split(text: &str) -> (String, String) {
    let t = text.trim_start_matches('/');
    let (cmd, rest) = t.split_once(' ').unwrap_or((t, ""));
    (cmd.to_lowercase(), rest.trim().to_string())
}

/// A whisper's target and message: `"Two Words" hello` or `Name hello`.
fn target_and_text(rest: &str) -> (String, String) {
    if let Some(r) = rest.strip_prefix('"') {
        if let Some((n, m)) = r.split_once('"') {
            return (n.to_string(), m.trim().to_string());
        }
    }
    let (n, m) = rest.split_once(' ').unwrap_or((rest, ""));
    (n.to_string(), m.trim().to_string())
}

/// An emote of the game file's (`/wave`, `/wave Name`), acted out for everyone in speaking reach. The one it is at
/// is a player of that name, or (a bot, a thing) the words as written.
fn emote(sim: &mut Sim, clients: &mut Clients, slot: usize, cmd: &str, rest: &str) -> bool {
    let Some(k) = sim.emotes.iter().position(|e| e.cmd == cmd) else { return false };
    let e = &sim.emotes[k];
    if sim.ents.fallen[slot] {
        return true;
    }
    let target = (!rest.is_empty()).then(|| match find(sim, rest) {
        Some(s) => sim.name_of(s).to_string(),
        None => rest.chars().filter(|c| !c.is_control() && *c != '|').take(24).collect(),
    });
    let mut text = format!("{}|{}", e.anim, e.line(&sim.name_of(slot), target.as_deref()));
    text.truncate(text.char_indices().nth(proto::chat::MAX_TEXT).map_or(text.len(), |(i, _)| i));
    if e.stays {
        sim.pose[slot] = k as u8 + 1;
    }
    crate::net::say_from(sim, clients, slot, channel::EMOTE, &text);
    true
}

/// Handle a "/" line from the player in `slot`. Returns false if it is not a command we know (it is then
/// answered with the list).
pub fn command(sim: &mut Sim, clients: &mut Clients, slot: usize, text: &str) -> bool {
    let me = sim.ents.id_of(slot);
    let my_name = sim.name_of(slot).to_string();
    let (cmd, rest) = split(text);
    match cmd.as_str() {
        "w" | "whisper" | "tell" | "r" | "reply" => {
            let (to, msg) = if cmd.starts_with('r') {
                let back = sim.ents.resolve(sim.last_whisper[slot]).filter(|&s| sim.ents.owner[s] < DRIVEN);
                match back {
                    Some(s) => (Some(s), rest),
                    None => {
                        say(clients, slot, "Nobody to answer.");
                        return true;
                    }
                }
            } else {
                let (n, m) = target_and_text(&rest);
                (find(sim, &n), m)
            };
            let Some(to) = to else {
                say(clients, slot, "Nobody of that name is here.");
                return true;
            };
            if msg.is_empty() || to == slot {
                say(clients, slot, "Whisper what? /w Name your words");
                return true;
            }
            let msg: String = msg.chars().filter(|c| !c.is_control()).take(200).collect();
            tell(clients, to, me, channel::WHISPER, &msg);
            let to_name = sim.name_of(to).to_string();
            tell(clients, slot, sim.ents.id_of(to), channel::WHISPER_SENT, &msg);
            sim.last_whisper[to] = me;
            clients.log_chat(sim.tick, me, channel::WHISPER, &format!("to {to_name}: {msg}"));
        }
        "rep" | "standing" | "reputation" => {
            let line = sim.standing_line(slot);
            say(clients, slot, &line);
            let towns = sim.repute_line(slot);
            say(clients, slot, &towns);
        }
        // God mode for those the game file names (`GameFile::gods`), by the account their identity proves: "/god",
        // "/god on", "/god off". Anyone else is told their tag, so it can be added.
        "god" => {
            let tag = sim.tag_of(slot).unwrap_or_default();
            if tag.is_empty() || !sim.gods.contains(&tag) {
                say(clients, slot, &format!("God mode is not yours here. Your account is {}.", if tag.is_empty() { "a guest" } else { tag.as_str() }));
                return true;
            }
            let on = match rest.trim() {
                "on" => true,
                "off" => false,
                _ => !sim.ents.god[slot],
            };
            sim.set_god(slot, on);
            say(clients, slot, if on { "God mode on: you cannot die and never run out of energy." } else { "God mode off." });
        }
        "who" => {
            let mut names: Vec<String> = (0..sim.ents.high)
                .filter(|&s| sim.ents.alive[s] && sim.ents.owner[s] < DRIVEN)
                .map(|s| {
                    let lv = if sim.levels.is_some() { format!(" ({})", sim.ents.level[s]) } else { String::new() };
                    format!("{}{lv}", sim.name_of(s))
                })
                .collect();
            names.sort();
            let n = names.len();
            names.truncate(40);
            say(clients, slot, &format!("{n} here: {}", names.join(", ")));
        }
        "invite" | "inv" => {
            let Some(to) = find(sim, &rest) else {
                say(clients, slot, "Nobody of that name is here.");
                return true;
            };
            if to == slot {
                return true;
            }
            if sim.party[to] != 0 {
                say(clients, slot, &format!("{} is already in a party.", sim.name_of(to)));
                return true;
            }
            if sim.party_of(slot).len() >= sim.party_cap(slot) {
                say(clients, slot, if sim.raids.contains(&sim.party[slot]) { "Your raid is full." } else { "Your party is full. /raid makes it a raid." });
                return true;
            }
            sim.invite[to] = me;
            say(clients, to, &format!("{my_name} asks you to join their party. /accept or /decline"));
            say(clients, slot, &format!("You ask {} to join you.", sim.name_of(to)));
        }
        "accept" | "join" => {
            let Some(from) = sim.ents.resolve(sim.invite[slot]).filter(|&s| sim.ents.owner[s] < DRIVEN) else {
                say(clients, slot, "Nobody has asked you.");
                return true;
            };
            sim.invite[slot] = 0;
            if sim.party_of(from).len() >= sim.party_cap(from) {
                say(clients, slot, "That party is full now.");
                return true;
            }
            if sim.party[slot] != 0 {
                sim.leave_party(slot);
            }
            if sim.party[from] == 0 {
                sim.next_party = sim.next_party.wrapping_add(1).max(1);
                sim.party[from] = sim.next_party;
            }
            sim.party[slot] = sim.party[from];
            let mates = sim.party_of(slot);
            for &m in &mates {
                say(clients, m, &format!("{my_name} joins the party."));
            }
            sim.bump_party(&mates);
        }
        "decline" => {
            if let Some(from) = sim.ents.resolve(sim.invite[slot]) {
                say(clients, from, &format!("{my_name} would rather not."));
            }
            sim.invite[slot] = 0;
        }
        "leave" => {
            if sim.party[slot] == 0 {
                say(clients, slot, "You are not in a party.");
                return true;
            }
            for m in sim.leave_party(slot) {
                say(clients, m, &format!("{my_name} leaves the party."));
            }
            say(clients, slot, "You leave the party.");
        }
        "p" | "party" if !rest.is_empty() || cmd == "p" => {
            if sim.party[slot] == 0 {
                say(clients, slot, "You are not in a party. /invite Name");
                return true;
            }
            let msg: String = rest.chars().filter(|c| !c.is_control()).take(200).collect();
            for m in sim.party_of(slot) {
                tell(clients, m, me, channel::PARTY, &msg);
            }
            clients.log_chat(sim.tick, me, channel::PARTY, &msg);
        }
        "social" => social_window(sim, clients, slot),
        "queue" | "lfg" | "unqueue" => {
            sim.finder_command(slot, if cmd == "unqueue" { "unqueue" } else { "queue" }, &rest);
        }
        "raid" => {
            if sim.party[slot] == 0 {
                say(clients, slot, "Make a party first: /invite Name, then /raid.");
                return true;
            }
            if sim.raids.insert(sim.party[slot]) {
                let size = sim.rules.raid_size;
                for m in sim.party_of(slot) {
                    say(clients, m, &format!("{my_name} makes the party a raid (up to {size})."));
                }
            } else {
                say(clients, slot, "Your party is a raid already.");
            }
        }
        "party" => {
            let mates: Vec<String> = sim.party_of(slot).iter().map(|&m| sim.name_of(m).to_string()).collect();
            let line = if sim.party[slot] == 0 { "You are not in a party. /invite Name".to_string() } else { format!("Your party: {}", mates.join(", ")) };
            say(clients, slot, &line);
        }
        "mail" | "letter" | "send" => {
            let (name, words) = target_and_text(&rest);
            let Some(to) = recipient(sim, clients, &name) else {
                say(clients, slot, if name.is_empty() { "/mail Name words, or at a market /send Name 10 gold" } else { "There is nobody of that name." });
                return true;
            };
            let line = if cmd == "send" {
                let (n, item) = words.split_once(' ').unwrap_or((&words, ""));
                match n.parse::<u32>() {
                    Ok(n) if !item.trim().is_empty() => sim.send_goods(slot, &to, &name, n, item),
                    _ => "Send what? /send Name 10 gold".to_string(),
                }
            } else {
                sim.send_letter(slot, &to, &name, &words)
            };
            say(clients, slot, &line);
            // Someone on now hears of it at once.
            if line.contains("on its way") {
                sim.system_mail_now();
            }
        }
        "mailbox" | "letters" | "inbox" => {
            for line in sim.read_mail(slot) {
                say(clients, slot, &line);
            }
        }
        _ if crate::realm::command(sim, clients, slot, text) => {}
        _ if emote(sim, clients, slot, &cmd, &rest) => {}
        _ => {
            let emotes: String = sim.emotes.iter().map(|e| format!(", /{}", e.cmd)).collect();
            say(clients, slot, &format!("Commands: /w Name words, /r words, /who, /rep, /invite Name, /accept, /decline, /leave, /p words, /party, /mail Name words, /send Name 10 gold, /mailbox, /name New Name, {}{emotes}", crate::realm::HELP));
            return false;
        }
    }
    true
}

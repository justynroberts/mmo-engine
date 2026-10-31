//! Appearance: a player's cape colour and whether their helmet (or hat) shows, chosen with `/look` from a palette,
//! or a rarer colour put on by using a dye (`ItemDef::dye`); and the weapon in their hand, drawn from the look of
//! what they wield (`ItemDef::look`) or, with `/look weapon Item`, of another item they own. Kept per character
//! (`Record::look`) and served to clients beside names (`/play/looks`), so each draws everyone the same.

use crate::sim::Sim;

/// The colours anyone may choose with `/look cape Name`; dyes give the rest.
pub const PALETTE: [(&str, &str); 10] = [
    ("red", "#a8322d"),
    ("blue", "#2f4f8f"),
    ("green", "#3f7a3a"),
    ("yellow", "#c9a227"),
    ("black", "#22201e"),
    ("white", "#e8e2d4"),
    ("brown", "#6b4a2f"),
    ("grey", "#7d7f80"),
    ("purple", "#5d3a7a"),
    ("saffron", "#d98b2b"),
];

/// A player's look: their cape colour ("" for the model's own) and whether their helmet shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Look {
    pub cape: String,
    pub no_helmet: bool,
    /// An item whose look is shown on the weapon instead of the weapon's own ("" for its own).
    pub weapon_as: String,
}

impl Look {
    /// As saved: "cape,helmet[,item shown on the weapon]" ("#a8322d,0"), "" for the model as it comes.
    pub fn code(&self) -> String {
        if self.cape.is_empty() && !self.no_helmet && self.weapon_as.is_empty() {
            return String::new();
        }
        let base = format!("{},{}", self.cape, if self.no_helmet { 0 } else { 1 });
        if self.weapon_as.is_empty() { base } else { format!("{base},{}", self.weapon_as) }
    }

    pub fn parse(s: &str) -> Look {
        let mut f = s.splitn(3, ',');
        let (cape, helm, weapon) = (f.next().unwrap_or(""), f.next().unwrap_or("1"), f.next().unwrap_or(""));
        Look { cape: if proto::game::is_hex_colour(cape) { cape.to_string() } else { String::new() }, no_helmet: helm == "0", weapon_as: weapon.to_string() }
    }
}

impl Sim {
    /// What clients are told of a player's look: "cape,helmet,weapon look" ("" for the model as it comes), the
    /// weapon's look from the item shown on it (or the one wielded).
    pub fn look_code(&self, s: usize) -> String {
        let l = &self.look[s];
        let shown = |name: &str| self.items.iter().find(|i| i.name == name).and_then(|i| i.look.clone());
        let wielded = self.gear.get(s).and_then(|g| g[0].checked_sub(1)).and_then(|k| self.items.get(k as usize)).and_then(|i| i.look.clone());
        let weapon = if l.weapon_as.is_empty() { wielded } else { shown(&l.weapon_as).or(wielded) }.unwrap_or_default();
        if l.cape.is_empty() && !l.no_helmet && weapon.is_empty() {
            return String::new();
        }
        format!("{},{},{weapon}", l.cape, if l.no_helmet { 0 } else { 1 })
    }

    /// `/look`, `/look cape Colour`, `/look helmet on|off`, `/look weapon [Item]`.
    pub fn look_command(&mut self, s: usize, rest: &str) {
        let words: Vec<String> = rest.split_whitespace().map(|w| w.to_lowercase()).collect();
        match words.first().map(String::as_str) {
            Some("cape" | "cloak") => match words.get(1).and_then(|w| PALETTE.iter().find(|p| p.0 == w)) {
                Some(&(name, hex)) => {
                    self.look[s].cape = hex.to_string();
                    self.ents.renamed.push(s as u32);
                    self.tells.push((s, format!("Your cape is {name} now.")));
                }
                None => {
                    let names: Vec<&str> = PALETTE.iter().map(|p| p.0).collect();
                    self.tells.push((s, format!("Cape colours: {}. Dyes give others.", names.join(", "))));
                }
            },
            Some("helmet" | "helm" | "hat" | "hood") => {
                let off = matches!(words.get(1).map(String::as_str), Some("off" | "hide" | "no"));
                self.look[s].no_helmet = off;
                self.ents.renamed.push(s as u32);
                self.tells.push((s, if off { "You go bareheaded." } else { "You wear your helmet." }.into()));
            }
            Some("weapon" | "wield") => {
                let want = words[1..].join(" ");
                if want.is_empty() {
                    self.look[s].weapon_as.clear();
                    self.ents.renamed.push(s as u32);
                    self.tells.push((s, "Your weapon looks like itself again.".into()));
                    return;
                }
                // Any item with a look that the player carries or wears.
                let owned = |k: usize| self.inv[s].iter().any(|b| b.0 as usize == k + 1) || self.gear[s].iter().any(|&g| g as usize == k + 1);
                match (0..self.items.len()).find(|&k| self.items[k].look.is_some() && owned(k) && (self.items[k].name.to_lowercase() == want || self.items[k].shown().to_lowercase() == want)) {
                    Some(k) => {
                        self.look[s].weapon_as = self.items[k].name.clone();
                        self.ents.renamed.push(s as u32);
                        self.tells.push((s, format!("Your weapon takes on the look of the {}.", self.items[k].shown())));
                    }
                    None => self.tells.push((s, "You carry nothing of that name with a look to show.".into())),
                }
            }
            _ => self.tells.push((s, "/look cape Colour (red, blue, green, ...), /look helmet on|off, /look weapon Item (show another's look). Dyes colour your cape too.".into())),
        }
    }

    /// Use a dye from the bag: the cape takes its colour.
    pub(crate) fn use_dye(&mut self, s: usize, colour: &str, what: &str) {
        self.look[s].cape = colour.to_string();
        self.ents.renamed.push(s as u32);
        self.tells.push((s, format!("You dye your cape with the {what}.")));
    }
}

#[cfg(test)]
mod tests {
    use super::Look;
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::world::World;

    #[test]
    fn a_look_is_chosen_kept_and_read_back() {
        let mut s = Sim::new(GameFile::default_game().build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        assert_eq!(s.look[p].code(), "", "as the model comes");
        s.look_command(p, "cape Blue");
        s.look_command(p, "helmet off");
        assert_eq!(s.look[p].code(), "#2f4f8f,0");
        assert_eq!(s.record_of(p).look, "#2f4f8f,0");
        assert_eq!(Look::parse("#2f4f8f,0"), s.look[p]);
        s.look_command(p, "cape tartan");
        assert_eq!(s.look[p].cape, "#2f4f8f", "not a colour on offer");
    }

    #[test]
    fn what_is_wielded_shows_and_another_items_look_can_be_shown_instead() {
        let mut g = GameFile::default_game();
        let mut sword = g.items[0].clone();
        (sword.name, sword.uses, sword.look, sword.equip) = ("sword".into(), vec![], Some("sword".into()), Some(proto::game::EquipFile { slot: proto::game::GearSlot::Weapon, might: 0.1, toughness: 0.0 }));
        let mut staff = g.items[0].clone();
        (staff.name, staff.uses, staff.look) = ("staff".into(), vec![], Some("staff".into()));
        g.items.extend([sword, staff]);
        let mut s = Sim::new(g.build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let p = s.spawn(0).unwrap();
        let (sw, st) = (s.items.iter().position(|i| i.name == "sword").unwrap(), s.items.iter().position(|i| i.name == "staff").unwrap());
        s.give(p, sw, 1);
        let i = s.inv[p].iter().position(|b| b.0 as usize == sw + 1).unwrap();
        assert!(s.equip(p, i));
        assert_eq!(s.look_code(p), ",1,sword", "the sword in hand");
        s.look_command(p, "weapon staff");
        assert_eq!(s.look[p].weapon_as, "", "not without one");
        s.give(p, st, 1);
        s.look_command(p, "weapon staff");
        assert_eq!(s.look_code(p), ",1,staff", "the staff's look on the sword");
        assert_eq!(Look::parse(&s.record_of(p).look).weapon_as, "staff", "kept");
    }
}

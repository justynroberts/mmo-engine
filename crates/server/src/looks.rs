//! Appearance: a player's cape colour and whether their helmet (or hat) shows, chosen with `/look` from a palette,
//! or a rarer colour put on by using a dye (`ItemDef::dye`). Kept per character (`Record::look`) and served to
//! clients beside names (`/play/looks`), so each draws everyone the same.

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
}

impl Look {
    /// As saved and served: "cape,helmet" ("#a8322d,0"), "" for the model as it comes.
    pub fn code(&self) -> String {
        if self.cape.is_empty() && !self.no_helmet { String::new() } else { format!("{},{}", self.cape, if self.no_helmet { 0 } else { 1 }) }
    }

    pub fn parse(s: &str) -> Look {
        let (cape, helm) = s.split_once(',').unwrap_or((s, "1"));
        Look { cape: if proto::game::is_hex_colour(cape) { cape.to_string() } else { String::new() }, no_helmet: helm == "0" }
    }
}

impl Sim {
    /// `/look`, `/look cape Colour`, `/look helmet on|off`.
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
            _ => self.tells.push((s, "/look cape Colour (red, blue, green, ...), /look helmet on|off. Dyes colour your cape too.".into())),
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
}

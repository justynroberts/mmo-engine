//! Display names: what a player may call themselves, and what everyone else is called.
//!
//! A player claims a name by sending it on the chat channel `NAME` (the session is the proof of who is asking);
//! the server checks it with `clean`, keeps it unique by `key`, saves it with the character and serves every
//! entity's name at `/play/names` (PROTOCOL.md "Names"). A player who never claims one goes by `generated`, picked
//! from their account tag so it is the same every time; a bot goes by its type (`bot`).

/// Shortest and longest name, in characters.
pub const MIN_LEN: usize = 3;
pub const MAX_LEN: usize = 16;

/// Names nobody may take: they would pass for the server speaking.
const RESERVED: &[&str] = &["admin", "administrator", "gm", "god", "moderator", "mod", "server", "system", "staff", "guest"];

/// A name as it may be shown, or why not. Letters (any script), digits, single spaces, apostrophes and hyphens;
/// it starts with a letter and never runs two marks together, so names stay readable and hard to dress up.
pub fn clean(raw: &str) -> Result<String, String> {
    let words: Vec<&str> = raw.split_whitespace().collect();
    let name = words.join(" ");
    let n = name.chars().count();
    if n < MIN_LEN || n > MAX_LEN {
        return Err(format!("A name is {MIN_LEN} to {MAX_LEN} letters long."));
    }
    if !name.chars().next().is_some_and(char::is_alphabetic) {
        return Err("A name starts with a letter.".into());
    }
    let mut last_mark = false;
    for c in name.chars() {
        let mark = c == ' ' || c == '\'' || c == '-';
        if !(c.is_alphabetic() || c.is_ascii_digit() || mark) {
            return Err("A name has letters, digits, spaces, ' and - only.".into());
        }
        if mark && last_mark {
            return Err("A name never puts two of space, ' and - together.".into());
        }
        last_mark = mark;
    }
    if last_mark {
        return Err("A name ends with a letter or digit.".into());
    }
    let lower = name.to_lowercase();
    if RESERVED.iter().any(|r| key(&name) == *r || lower.split([' ', '\'', '-']).any(|w| w == *r)) {
        return Err(format!("\"{name}\" is reserved."));
    }
    Ok(name)
}

/// What two names must not share to be told apart: case and marks do not count, so "Ana-Lee" and "ana lee" clash.
pub fn key(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// A bot's name: its type's, each word capitalised ("cave troll" -> "Cave Troll").
pub fn bot(arch: &str) -> String {
    arch.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_uppercase().chain(c).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

const START: &[&str] = &["Al", "Bran", "Cael", "Dor", "El", "Fen", "Gar", "Hal", "Is", "Jor", "Kel", "Lor", "Mar", "Nor", "Os", "Per", "Quin", "Ros", "Sel", "Tor", "Ul", "Val", "Wil", "Yr"];
const MIDDLE: &[&str] = &["a", "e", "i", "o", "ae", "ia", "or", "en", "ar", "il"];
const END: &[&str] = &["dric", "wyn", "mir", "thas", "ven", "ric", "dan", "wen", "lin", "mund", "ros", "vik", "nar", "sa", "len", "dor"];

/// The name an account goes by until it claims one: three syllables picked by its tag (hex), so it never changes.
/// About 3,800 of them; two players may share one, which is why claiming a name is worth doing.
pub fn generated(account_tag: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in account_tag.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let pick = |list: &[&str], h: u64| list[(h % list.len() as u64) as usize].to_string();
    format!("{}{}{}", pick(START, h), pick(MIDDLE, h >> 16), pick(END, h >> 32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_names_are_tidied_and_kept() {
        assert_eq!(clean("  Ana   Lee ").unwrap(), "Ana Lee");
        assert_eq!(clean("O'Brien").unwrap(), "O'Brien");
        assert_eq!(clean("Zoë-Mae2").unwrap(), "Zoë-Mae2");
        assert_eq!(clean("Ярослав").unwrap(), "Ярослав");
    }

    #[test]
    fn bad_names_say_why() {
        for (raw, why) in [
            ("Al", "3 to 16"),
            ("Abcdefghijklmnopq", "3 to 16"),
            ("2fast", "starts with a letter"),
            ("Ann--Lee", "two of"),
            ("Ann'", "ends with"),
            ("Bob!", "letters, digits"),
            ("Ann\u{202e}Lee", "letters, digits"),
            ("Admin", "reserved"),
            ("The GM", "reserved"),
        ] {
            let e = clean(raw).unwrap_err();
            assert!(e.contains(why), "{raw}: {e}");
        }
    }

    #[test]
    fn keys_ignore_case_and_marks() {
        assert_eq!(key("Ana-Lee"), key("ana lee"));
        assert_ne!(key("Ana Lee"), key("Ana Lea"));
    }

    #[test]
    fn bots_and_defaults() {
        assert_eq!(bot("cave troll"), "Cave Troll");
        assert_eq!(bot("drifter"), "Drifter");
        let g = generated("0123456789abcdef");
        assert_eq!(g, generated("0123456789abcdef"), "the same tag gives the same name");
        assert!(clean(&g).is_ok(), "{g} is a name a player could have claimed");
        assert_ne!(g, generated("fedcba9876543210"));
    }
}

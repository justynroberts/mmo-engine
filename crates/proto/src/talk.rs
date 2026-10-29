//! What townsfolk say, by occasion: quarrels, the cry of thief, greetings, remarks on how a player looks, and the
//! news they pass on. `GameFile::talk` replaces the lines of any occasion it names; the rest keep these. Not in
//! the content hash: editing them applies live.
//!
//! A line may hold `{name}` (the player spoken to), `{who}` and `{other}` (those the news is about), `{place}`
//! (where it happened) and `{greeting}` (good morning, good day, good evening, or a late hour). `market`, `feast`
//! and `wedding` are said at gatherings (an event's `gather` step).

use std::collections::BTreeMap;

/// Every occasion, with its lines when the game file gives none.
pub const DEFAULTS: &[(&str, &[&str])] = &[
    ("insult", &["Say that again, I dare you!", "You cheated me at the market!", "That's my seat, you lout!", "You've a face like a slapped haddock.", "Who are you calling a liar?"]),
    ("give_in", &["Enough! Enough, I yield!", "All right, all right!", "I'll pay, I'll pay!"]),
    ("won", &["And that's the penny you owed me.", "Let that be a lesson to you.", "Next time, mind your tongue."]),
    ("break_up", &["Break it up, you two!", "Enough of that! Off home with you.", "Any more of this and it's the stocks."]),
    ("cheer", &["Go on, hit him!", "A penny on the big one!", "Mind his left!", "Ah, would you look at this.", "Someone fetch the watch!"]),
    ("cry", &["Thief! Stop, thief!", "My purse! Someone stop him!", "Cutpurse! Watch, watch!"]),
    ("noticed", &["Hands off my purse!", "I see you there, you rogue!"]),
    ("stop", &["Stop in the name of the watch!", "Halt, thief!", "After him!"]),
    ("thanks", &["Well done, {name}. That's one less cutpurse in {place}.", "The watch owes you one, {name}.", "Fine work, {name}. Folk will hear of this."]),
    ("greet_new", &["Haven't seen you round here before.", "A stranger! Welcome to {place}.", "{greeting}. New to {place}, are you?", "You're not from these parts."]),
    ("greet_known", &["{greeting}, {name}.", "Back again, {name}?", "Ah, {name}. Good to see you.", "{name}! How's the road treating you?"]),
    ("hurt", &["You're bleeding, {name}. Sit a while.", "You look done in. There's a healer in {place}.", "Who did that to you?"]),
    ("night", &["You shouldn't be abroad at this hour.", "Wolves come close at night. Mind yourself.", "Late to be out, {name}."]),
    ("rain", &["You're soaked through!", "Come in out of the wet.", "Rain again. It never stops."]),
    ("rich", &["That's a heavy purse, {name}. Mind the cutpurses.", "Coin like that draws thieves."]),
    ("news_theft", &["Did you hear? A cutpurse robbed {who} in {place}.", "They say {who} lost their purse in {place}.", "Keep a hand on your purse. {who} was robbed in {place}."]),
    ("news_caught", &["{who} caught a cutpurse in {place}!", "They say {who} ran down a thief in {place}.", "One less cutpurse in {place}, thanks to {who}."]),
    ("news_brawl", &["{who} gave {other} a hiding in {place}.", "There was a fight in {place}. {who} won it, they say.", "{other} came off worst against {who} in {place}."]),
    ("news_ambush", &["Something came out of the dark for {who} near {place}.", "Beasts on the road by night near {place}. {who} barely got away."]),
    ("news_fell", &["Did you hear? {who} fell near {place}.", "They found {who} on the ground near {place}. Hard times."]),
    ("news_level", &["They say {who} is making a name for themselves.", "{who}? I've heard that name. Growing in strength, they say."]),
    ("news_died", &["Did you hear? {who} was killed by {other} near {place}. God rest them.", "They're burying {who} in {place}. {other} got them.", "Poor {who}. Taken by {other}, out past {place}."]),
    ("news_newcomer", &["There's a newcomer in {place}: {who}, they call them.", "Have you met {who}? Just come to live in {place}."]),
    ("news_hero", &["{who}'s a friend to {place}, they say. Saved the town, near enough.", "Folk in {place} speak well of {who}."]),
    ("news_villain", &["Watch out for {who}. They've made trouble in {place}.", "{who}? Folk in {place} won't have them in the door."]),
    ("news_robbed", &["Robbers on the road near {place}! They did for {who}.", "{who} never got to {place}. Robbers, they say."]),
    ("market", &["Fresh bread! Fresh bread!", "Eggs, a penny the half dozen!", "Wool, finest wool!", "Who'll buy my apples?", "Fish, fresh this morning!", "Come see, come see!"]),
    ("feast", &["Another tune! Another!", "Come dance, come dance!", "Sláinte!", "Isn't it a grand night?", "Fill it up again!"]),
    ("wedding", &["To {who} and {other}!", "Long life to the pair of them!", "Didn't she look lovely?", "Give us a song for the couple!"]),
    ("sing", &["♪ Oh the drink, the lovely drink... ♪", "♪ And she's my darlin'... ♪", "Wheresh my door?", "One more, just the one...", "♪ Fol-de-diddle-ay! ♪", "I love you all, I do."]),
    ("play", &["You're it!", "Can't catch me!", "Not fair, I wasn't ready!", "Faster, faster!", "Got you!"]),
    ("bark", &["Woof! Woof!", "Grrr...", "Woof!", "Arf! Arf!"]),
    ("bell", &["There's the bell. Home, all of you!", "Vespers. In you come, children.", "The bell, already? Where did the day go.", "That's the bell. Time we were in."]),
    ("bell_dawn", &["There's the morning bell. Up and at it!", "Matins. Another day, God help us.", "The bell already? I've only just shut my eyes.", "Morning! You'll hear that bell in your sleep."]),
    ("cough", &["*cough* *cough*... sorry. Keep back.", "I'm not right. Haven't been since Tuesday.", "My head's on fire.", "*hacking cough*", "Don't come near me, I've the sickness."]),
    ("plea_cure", &["{name}, have you anything for the fever? A draught, anything...", "Please, {name}, a healing draught. I'm burning up.", "{name}! If you've medicine, God reward you for it."]),
    ("cured", &["The fever's broken! Bless you, {name}.", "I can breathe again. Thank you, {name}.", "You've saved me, {name}."]),
    ("shut", &["We're shut. The sickness. Come back when it's passed.", "Not today. Half the street's down with it.", "Shut up shop till the fever goes, so I have."]),
    ("news_plague", &["Sickness in {place}! Keep away from it if you can.", "There's a fever in {place}. They're dying of it.", "Don't go near {place}. The sickness is on them."]),
    ("news_plague_died", &["{who} died of the sickness in {place}. God rest them.", "The fever took {who} in {place}.", "Another one gone in {place}: {who}, of the sickness."]),
    ("news_plague_over", &["The sickness has passed in {place}, thank God.", "{place} is clean again, they say. The fever's gone.", "They've opened the shops again in {place}. It's over."]),
    ("mourn", &["Rest easy, {who}.", "God keep you, {who}.", "They were good to me, {who} was.", "Gone too soon."]),
    ("newcomer_hello", &["Hello! I'm new here. {greeting}.", "Just moved to {place}. Is it always like this?"]),
    ("greet_hero", &["{name}! The hero of {place}!", "Make way for {name}!", "Your money's no good here, {name}. Well, nearly.", "We owe you, {name}."]),
    ("greet_friend", &["{greeting}, {name}! Good to see you.", "Ah, {name}, a friend of {place}."]),
    ("greet_cold", &["Hm. {name}.", "Mind yourself, {name}. We've heard about you.", "Keep your hands where I can see them."]),
    ("shun", &["Get away from me!", "We don't want your sort in {place}.", "I'll not serve the likes of you.", "Go on, clear off."]),
    ("watch_warn", &["I've my eye on you.", "One wrong move.", "Keep walking."]),
    ("plea_beast", &["Help! There's a {other} at the fields!", "Please, a {other}! It's after the flock!", "You there! A {other}! Can you drive it off?"]),
    ("plea_stray", &["My {other}'s wandered off! Could you bring it home?", "Have you seen my {other}? It got out again. It's out past the fields."]),
    ("plea_thief", &["He took my purse! That way!", "Stop him! The cutpurse, there!"]),
    ("plea_thanks", &["Bless you, {name}! Take this.", "Thank you, {name}. Here, for your trouble.", "I won't forget this, {name}."]),
    ("spring", &["Lambs in every field. It's a good spring.", "Planting time. My back's in bits."]),
    ("summer", &["Hot one today.", "Long days. Good for the hay."]),
    ("autumn", &["Harvest's in, thank God.", "Leaves are turning. Winter's coming.", "Bread's cheap with the harvest in."]),
    ("winter", &["Bitter cold. Keep near a fire.", "Wolves come close in the winter.", "Food's dear this time of year."]),
];

/// The lines for each occasion: the game file's where it gives them, else the defaults.
#[derive(Clone, Debug, Default)]
pub struct Talk(pub BTreeMap<String, Vec<String>>);

impl Talk {
    /// The lines of `occasion` (none for an unknown one).
    pub fn lines(&self, occasion: &str) -> &[String] {
        self.0.get(occasion).map_or(&[], |v| v.as_slice())
    }

    /// Check a game file's `talk` and lay it over the defaults.
    pub fn build(file: &BTreeMap<String, Vec<String>>) -> Result<Talk, String> {
        let mut out: BTreeMap<String, Vec<String>> = DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.iter().map(|l| l.to_string()).collect())).collect();
        for (k, lines) in file {
            if !out.contains_key(k) {
                let known: Vec<&str> = DEFAULTS.iter().map(|d| d.0).collect();
                return Err(format!("talk: no occasion '{k}' (one of {})", known.join(", ")));
            }
            if lines.is_empty() || lines.len() > 40 {
                return Err(format!("talk.{k}: 1..40 lines"));
            }
            if let Some(l) = lines.iter().find(|l| l.trim().is_empty() || l.len() > 160 || l.chars().any(char::is_control)) {
                return Err(format!("talk.{k}: a line is 1..160 characters ('{l}')"));
            }
            out.insert(k.clone(), lines.clone());
        }
        Ok(Talk(out))
    }
}

/// Fill a line's `{key}` holes.
pub fn fill(line: &str, holes: &[(&str, &str)]) -> String {
    holes.iter().fold(line.to_string(), |s, (k, v)| s.replace(&format!("{{{k}}}"), v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_file_replaces_an_occasion_and_keeps_the_rest() {
        let mut f = BTreeMap::new();
        f.insert("cry".to_string(), vec!["Stop him!".to_string()]);
        let t = Talk::build(&f).unwrap();
        assert_eq!(t.lines("cry"), ["Stop him!".to_string()]);
        assert!(!t.lines("greet_new").is_empty());
        f.insert("nonsense".to_string(), vec!["x".to_string()]);
        assert!(Talk::build(&f).is_err());
        assert_eq!(fill("{greeting}, {name}.", &[("name", "Aoife"), ("greeting", "Good day")]), "Good day, Aoife.");
    }
}

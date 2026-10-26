//! The game's tuning numbers: reaches, timers, sizes and shares the server once kept as constants. All of it is
//! `GameFile::rules`, each field defaulting to the value it had as a constant, so a file without it plays as before.
//! None of it is in the content hash: edits apply live. Reaches a client also needs go out in `/play/info` "rules".

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Rules {
    /// How near a bot you talk to it, take its quests and use its shop.
    pub talk_reach: f32,
    /// How near a pile you pick it up.
    pub pickup_reach: f32,
    /// Seconds between any two actions (the global cooldown).
    pub global_cooldown_secs: f32,
    /// Seconds between using two items (potion pacing).
    pub item_cooldown_secs: f32,
    /// Seconds a dropped item or loot pile lies before it fades.
    pub ground_item_secs: f32,
    /// How well a bot sees an unlit player at night, as a share of its sight.
    pub sneak_sight: f32,
    /// Seconds a bot keeps chasing with no blows landed before it gives up.
    pub chase_give_up_secs: f32,
    /// Seconds without a blow before a fight between players is over.
    pub pvp_quiet_secs: f32,
    /// Seconds after hurting a bot in which you still get credit when it falls.
    pub kill_credit_secs: f32,
    /// Most players in a party.
    pub party_size: u8,
    /// How near a kill party mates must be to share its experience.
    pub party_share_reach: f32,
    /// How near a kill other players must be to share its quest credit.
    pub quest_share_reach: f32,
    /// How far a spoken line carries.
    pub say_reach: f32,
    /// How far a shout carries; 0, the whole world.
    pub shout_reach: f32,
    /// How far a bot's passing remark carries.
    pub remark_reach: f32,
    /// Seconds between two lines from one bot.
    pub bot_say_secs: f32,
    /// Most members in a guild.
    pub guild_size: u16,
    /// Seconds an empty dungeon run stays open.
    pub dungeon_idle_secs: f32,
    /// How near two players must be to start a trade, and how far apart before it is called off.
    pub trade_reach: f32,
    pub trade_break: f32,
    /// How near a merchant counts as being at a market (bank, auction house).
    pub market_reach: f32,
    /// Hours a listing stays up at the auction house, and most listings per seller.
    pub listing_hours: f32,
    pub listings_per_seller: u16,
    /// What falling costs a player (all nothing by default): a share of the experience earned into their level (never
    /// a level), a share of the coin in their bag (gone), the chance each other stack of their bag is left where they
    /// fell, and seconds before they may rise.
    pub fall_xp_loss_pct: f32,
    pub fall_coin_loss_pct: f32,
    pub fall_drop_pct: f32,
    pub rise_delay_secs: f32,
    /// How near a bot you hail it (H), and how long it then stands still for you (it walks on sooner if you
    /// leave twice that reach). Only bots that do not fight stop: a trader, a farmer, a quest giver.
    pub hail_reach: f32,
    pub hail_secs: f32,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            talk_reach: 8.0,
            pickup_reach: 3.0,
            global_cooldown_secs: 0.5,
            item_cooldown_secs: 0.25,
            ground_item_secs: 300.0,
            sneak_sight: 0.35,
            chase_give_up_secs: 10.0,
            pvp_quiet_secs: 8.0,
            kill_credit_secs: 30.0,
            party_size: 5,
            party_share_reach: 80.0,
            quest_share_reach: 40.0,
            say_reach: 64.0,
            shout_reach: 0.0,
            remark_reach: 24.0,
            bot_say_secs: 6.0,
            guild_size: 200,
            dungeon_idle_secs: 30.0,
            trade_reach: 12.0,
            trade_break: 24.0,
            market_reach: 16.0,
            listing_hours: 48.0,
            listings_per_seller: 12,
            fall_xp_loss_pct: 0.0,
            fall_coin_loss_pct: 0.0,
            fall_drop_pct: 0.0,
            rise_delay_secs: 0.0,
            hail_reach: 30.0,
            hail_secs: 30.0,
        }
    }
}

impl Rules {
    pub fn is_default(&self) -> bool {
        *self == Rules::default()
    }

    /// Every number finite and in a sane range, or the first that is not.
    pub fn check(&self) -> Result<(), String> {
        let reaches = [
            ("talk_reach", self.talk_reach),
            ("pickup_reach", self.pickup_reach),
            ("party_share_reach", self.party_share_reach),
            ("quest_share_reach", self.quest_share_reach),
            ("say_reach", self.say_reach),
            ("shout_reach", self.shout_reach),
            ("remark_reach", self.remark_reach),
            ("trade_reach", self.trade_reach),
            ("trade_break", self.trade_break),
            ("market_reach", self.market_reach),
            ("hail_reach", self.hail_reach),
        ];
        for (name, v) in reaches {
            if !(v.is_finite() && (0.0..=10_000.0).contains(&v)) {
                return Err(format!("rules: {name} is 0..10000"));
            }
        }
        let secs = [
            ("global_cooldown_secs", self.global_cooldown_secs),
            ("item_cooldown_secs", self.item_cooldown_secs),
            ("ground_item_secs", self.ground_item_secs),
            ("chase_give_up_secs", self.chase_give_up_secs),
            ("pvp_quiet_secs", self.pvp_quiet_secs),
            ("kill_credit_secs", self.kill_credit_secs),
            ("bot_say_secs", self.bot_say_secs),
            ("dungeon_idle_secs", self.dungeon_idle_secs),
            ("listing_hours", self.listing_hours),
            ("rise_delay_secs", self.rise_delay_secs),
            ("hail_secs", self.hail_secs),
        ];
        for (name, v) in secs {
            if !(v.is_finite() && (0.0..=86_400.0).contains(&v)) {
                return Err(format!("rules: {name} is 0..86400"));
            }
        }
        for (name, v) in [("fall_xp_loss_pct", self.fall_xp_loss_pct), ("fall_coin_loss_pct", self.fall_coin_loss_pct), ("fall_drop_pct", self.fall_drop_pct)] {
            if !(v.is_finite() && (0.0..=100.0).contains(&v)) {
                return Err(format!("rules: {name} is 0..100"));
            }
        }
        if !(self.sneak_sight.is_finite() && (0.0..=1.0).contains(&self.sneak_sight)) {
            return Err("rules: sneak_sight is 0..1".into());
        }
        if self.trade_break < self.trade_reach {
            return Err("rules: trade_break must be at least trade_reach".into());
        }
        if self.party_size < 2 || self.guild_size < 2 || self.listings_per_seller < 1 {
            return Err("rules: party_size and guild_size are at least 2, listings_per_seller at least 1".into());
        }
        Ok(())
    }

    /// Seconds as whole ticks at `hz` (at least one, so nothing timed happens at once by rounding).
    pub fn ticks(secs: f32, hz: u32) -> u32 {
        ((secs * hz as f32).round() as u32).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_check_and_a_partial_section_fills_in_the_rest() {
        assert!(Rules::default().check().is_ok());
        let r: Rules = serde_json::from_str(r#"{"party_size": 8}"#).unwrap();
        assert_eq!(r, Rules { party_size: 8, ..Rules::default() });
        assert!(serde_json::from_str::<Rules>(r#"{"party_sise": 8}"#).is_err(), "a misspelt rule is an error, not ignored");
        assert!(Rules { trade_break: 1.0, ..Rules::default() }.check().is_err());
        assert!(Rules { sneak_sight: f32::NAN, ..Rules::default() }.check().is_err());
        assert!(Rules { fall_drop_pct: 101.0, ..Rules::default() }.check().is_err());
        assert_eq!(Rules::ticks(0.25, 20), 5);
    }
}

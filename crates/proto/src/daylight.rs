//! When it is light and when it is dark: the game file's `daylight` schedule.
//!
//! The server never sends the time. It sends the schedule (in `/play/info`), and every client works out where the
//! day is from its own clock, so nothing goes stale between polls and every client agrees. Clients that draw a
//! sky port `phase` (godot/scripts/theme.gd `daylight_phase`); `/play/info` carries a sample `[unix_ms, phase]`
//! computed here, which the Godot smoke test checks its port against.
//!
//! A phase runs 0 .. 1 through one day: the sun rises at 0, is highest at half the day, sets at `1 - night_share`,
//! and the night fills the rest.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "mode", rename_all = "lowercase", deny_unknown_fields)]
pub enum Daylight {
    /// Day and night repeat every `day_minutes`, the night taking `night_share` of it. `offset` shifts the cycle
    /// (it is what "make it night now" in the console changes).
    Cycle {
        day_minutes: f64,
        night_share: f64,
        #[serde(default)]
        offset: f64,
    },
    /// The real day: the sun rises at `sunrise` and sets at `sunset` ("HH:MM") in the time zone `utc_offset_minutes`
    /// east of UTC.
    Clock {
        sunrise: String,
        sunset: String,
        #[serde(default)]
        utc_offset_minutes: i32,
    },
    /// Always the same moment: a phase, 0 .. 1.
    Fixed { phase: f64 },
}

impl Default for Daylight {
    /// A day every 24 minutes, a third of it night.
    fn default() -> Daylight {
        Daylight::Cycle { day_minutes: 24.0, night_share: 1.0 / 3.0, offset: 0.0 }
    }
}

/// "HH:MM" to minutes after midnight.
fn minutes(hm: &str) -> Option<f64> {
    let (h, m) = hm.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some((h * 60 + m) as f64)
}

impl Daylight {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Daylight::Cycle { day_minutes, night_share, offset } => {
                if !(day_minutes.is_finite() && (1.0..=10_080.0).contains(day_minutes)) {
                    return Err("daylight: day_minutes must be 1 .. 10080 (a week)".into());
                }
                if !(night_share.is_finite() && (0.0..=0.9).contains(night_share)) {
                    return Err("daylight: night_share must be 0 .. 0.9".into());
                }
                if !offset.is_finite() {
                    return Err("daylight: offset must be a number".into());
                }
            }
            Daylight::Clock { sunrise, sunset, utc_offset_minutes } => {
                let (Some(r), Some(s)) = (minutes(sunrise), minutes(sunset)) else {
                    return Err("daylight: sunrise and sunset must look like 06:30".into());
                };
                if r == s {
                    return Err("daylight: sunrise and sunset must differ".into());
                }
                if utc_offset_minutes.abs() > 14 * 60 {
                    return Err("daylight: utc_offset_minutes must be within 14 hours".into());
                }
            }
            Daylight::Fixed { phase } => {
                if !(phase.is_finite() && (0.0..1.0).contains(phase)) {
                    return Err("daylight: phase must be 0 .. 1".into());
                }
            }
        }
        Ok(())
    }

    /// The share of the day that is night.
    pub fn night_share(&self) -> f64 {
        match self {
            Daylight::Cycle { night_share, .. } => *night_share,
            Daylight::Clock { sunrise, sunset, .. } => {
                let (r, s) = (minutes(sunrise).unwrap_or(360.0), minutes(sunset).unwrap_or(1140.0));
                (r - s).rem_euclid(1440.0) / 1440.0
            }
            Daylight::Fixed { .. } => 1.0 / 3.0,
        }
    }

    /// Where the day is at `unix_ms`: 0 sunrise, `1 - night_share` sunset.
    pub fn phase(&self, unix_ms: f64) -> f64 {
        match self {
            Daylight::Cycle { day_minutes, offset, .. } => (unix_ms / (day_minutes * 60_000.0) + offset).rem_euclid(1.0),
            Daylight::Clock { sunrise, utc_offset_minutes, .. } => {
                // Minutes since sunrise, local time; the day maps onto 0 .. 1 - night, the night onto the rest.
                let local = (unix_ms / 60_000.0 + *utc_offset_minutes as f64).rem_euclid(1440.0);
                let since = (local - minutes(sunrise).unwrap_or(360.0)).rem_euclid(1440.0);
                let night = self.night_share();
                let day_len = (1.0 - night) * 1440.0;
                if since < day_len {
                    since / day_len * (1.0 - night)
                } else {
                    1.0 - night + (since - day_len) / (night * 1440.0).max(1e-9) * night
                }
            }
            Daylight::Fixed { phase } => *phase,
        }
    }

    /// How many days have passed at `unix_ms` (with the fraction of today), for the seasons: a cycle counts its
    /// own days, a clock the real ones, and a fixed moment is always day 0.
    pub fn day(&self, unix_ms: f64) -> f64 {
        match self {
            Daylight::Cycle { day_minutes, offset, .. } => unix_ms / (day_minutes * 60_000.0) + offset,
            Daylight::Clock { utc_offset_minutes, .. } => (unix_ms / 60_000.0 + *utc_offset_minutes as f64) / 1440.0,
            Daylight::Fixed { .. } => 0.0,
        }
    }

    /// The same schedule moved so that it is `phase` at `unix_ms` (a cycle only; the others are what they are).
    pub fn set_phase(&mut self, unix_ms: f64, phase: f64) -> Result<(), String> {
        match self {
            Daylight::Cycle { day_minutes, offset, .. } => {
                *offset = (phase - unix_ms / (*day_minutes * 60_000.0)).rem_euclid(1.0);
                Ok(())
            }
            Daylight::Fixed { phase: p } => {
                *p = phase.rem_euclid(1.0);
                Ok(())
            }
            Daylight::Clock { .. } => Err("a clock schedule follows the real time; switch to a cycle to set the time".into()),
        }
    }
}

/// Milliseconds since the Unix epoch, now.
pub fn now_ms() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cycle_repeats_and_can_be_moved_to_any_moment() {
        let mut d = Daylight::default();
        assert!(d.validate().is_ok());
        let day = 24.0 * 60_000.0;
        assert!((d.phase(day * 10.25) - 0.25).abs() < 1e-9);
        assert!((d.phase(day * 10.25) - d.phase(day * 11.25)).abs() < 1e-9, "a day later is the same moment");
        d.set_phase(day * 10.25, 0.8).unwrap();
        assert!((d.phase(day * 10.25) - 0.8).abs() < 1e-9);
        assert!((d.phase(day * 10.75) - 0.3).abs() < 1e-9, "and runs on from there");
    }

    #[test]
    fn a_clock_follows_sunrise_and_sunset_in_its_time_zone() {
        let d = Daylight::Clock { sunrise: "06:00".into(), sunset: "18:00".into(), utc_offset_minutes: 60 };
        assert!(d.validate().is_ok());
        assert!((d.night_share() - 0.5).abs() < 1e-9);
        let at = |h: f64| (h - 1.0) * 3_600_000.0; // local hour h, an hour east of UTC
        assert!(d.phase(at(6.0)).abs() < 1e-9, "sunrise");
        assert!((d.phase(at(12.0)) - 0.25).abs() < 1e-9, "noon is half way through the day");
        assert!((d.phase(at(18.0)) - 0.5).abs() < 1e-9, "sunset");
        assert!((d.phase(at(24.0)) - 0.75).abs() < 1e-9, "midnight is half way through the night");
        // A long summer day: the sun up 16 hours.
        let summer = Daylight::Clock { sunrise: "05:00".into(), sunset: "21:00".into(), utc_offset_minutes: 0 };
        assert!((summer.night_share() - 1.0 / 3.0).abs() < 1e-9);
        assert!((summer.phase(13.0 * 3_600_000.0) - 1.0 / 3.0).abs() < 1e-9);
        assert!(summer.clone().set_phase(0.0, 0.5).is_err());
    }

    #[test]
    fn bad_schedules_are_refused() {
        for (d, why) in [
            (Daylight::Cycle { day_minutes: 0.0, night_share: 0.3, offset: 0.0 }, "day_minutes"),
            (Daylight::Cycle { day_minutes: 10.0, night_share: 1.0, offset: 0.0 }, "night_share"),
            (Daylight::Clock { sunrise: "25:00".into(), sunset: "18:00".into(), utc_offset_minutes: 0 }, "06:30"),
            (Daylight::Clock { sunrise: "06:00".into(), sunset: "06:00".into(), utc_offset_minutes: 0 }, "differ"),
            (Daylight::Fixed { phase: 1.5 }, "phase"),
        ] {
            assert!(d.validate().unwrap_err().contains(why), "{d:?}");
        }
        let d: Daylight = serde_json::from_str(r#"{"mode":"fixed","phase":0.8}"#).unwrap();
        assert_eq!(d, Daylight::Fixed { phase: 0.8 });
        assert!(serde_json::from_str::<Daylight>(r#"{"mode":"fixed","phase":0.8,"extra":1}"#).is_err());
    }
}

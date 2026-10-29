//! The year (`Rules::season_days`): which season it is follows from the day (`Daylight::day`), so every client works
//! it out from the schedule as the server does. Spring grows crops faster and winter slower; autumn's harvest
//! yields more and makes food cheap, winter makes it dear; winter nights bring more out of the dark. An event can
//! start when the year turns (`When::Season`), and townsfolk remark on the time of year.

use crate::sim::Sim;
use proto::game::Season;

impl Sim {
    /// The day and season, worked out once a second.
    pub(crate) fn system_season(&mut self) {
        self.day = self.daylight.day(proto::daylight::now_ms());
        self.season = Season::of(self.day, self.rules.season_days, self.rules.season_shift);
    }

    /// How long a crop takes to grow now, as a share of its `grow_secs`.
    pub(crate) fn season_growth(&self) -> f32 {
        match self.season {
            Some(Season::Spring) => 0.75,
            Some(Season::Winter) => 2.0,
            _ => 1.0,
        }
    }

    /// More from a ripe crop at harvest time.
    pub(crate) fn season_yield(&self) -> u32 {
        (self.season == Some(Season::Autumn)) as u32
    }

    /// What food costs now, as a share of its price: an item that is a crop's harvest is cheap in autumn and dear
    /// in winter. Anything else costs what it costs.
    pub(crate) fn season_price(&self, item: usize) -> f32 {
        if !self.realm.crops.iter().any(|c| c.yields == item) {
            return 1.0;
        }
        match self.season {
            Some(Season::Autumn) => 0.8,
            Some(Season::Winter) => 1.3,
            _ => 1.0,
        }
    }

    /// How much more often the dark brings things out: half as much again in winter.
    pub(crate) fn season_night(&self) -> f32 {
        if self.season == Some(Season::Winter) { 1.5 } else { 1.0 }
    }
}

#[cfg(test)]
mod tests {
    use proto::game::Season;

    #[test]
    fn the_year_turns_every_season_days_and_the_shift_moves_it() {
        assert_eq!(Season::of(0.5, 3.0, 0.0), Some(Season::Spring));
        assert_eq!(Season::of(3.2, 3.0, 0.0), Some(Season::Summer));
        assert_eq!(Season::of(11.9, 3.0, 0.0), Some(Season::Winter));
        assert_eq!(Season::of(12.0, 3.0, 0.0), Some(Season::Spring), "round again");
        assert_eq!(Season::of(0.5, 3.0, 9.0), Some(Season::Winter), "shifted");
        assert_eq!(Season::of(-0.5, 3.0, 0.0), Some(Season::Winter), "before day 0");
        assert_eq!(Season::of(5.0, 0.0, 0.0), None, "no seasons");
    }
}

//! The damage meter: what each player dealt and mended in their fight, shared with their party so a group can learn
//! a fight. A fight starts with the first blow or heal after `IDLE_SECS` of quiet and runs while any of the party
//! keeps at it; what was last fought stays up for `SHOWN_SECS`. Each player is sent their party's meter once a
//! second while it changes, on chat channel 22: "M|secs;name,dealt,mended;..." (most dealt first), "M|" to clear.

use crate::sim::{Sim, DRIVEN};

/// Quiet that ends a fight, and how long the last one stays up after.
const IDLE_SECS: u32 = 8;
const SHOWN_SECS: u32 = 60;

/// One player's part in their latest fight.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Meter {
    pub dealt: f32,
    pub mended: f32,
    /// Ticks of the fight's first and latest deed.
    pub start: u32,
    pub last: u32,
}

impl Sim {
    /// `by` dealt (`amount` > 0) or mended (< 0 means a heal of -amount) someone. Only players are counted.
    pub(crate) fn meter_add(&mut self, by: usize, dealt: f32, mended: f32) {
        if self.ents.owner[by] >= DRIVEN || (dealt <= 0.0 && mended <= 0.0) {
            return;
        }
        let idle = IDLE_SECS * self.tick_hz;
        let now = self.tick;
        let m = &mut self.meter[by];
        if now.saturating_sub(m.last) > idle || m.last == 0 {
            // A new fight: joined to the party's if a mate is in one.
            let start = self.party_of(by).iter().filter(|&&o| o != by && self.meter[o].last > 0 && now.saturating_sub(self.meter[o].last) <= idle).map(|&o| self.meter[o].start).min().unwrap_or(now);
            self.meter[by] = Meter { dealt: 0.0, mended: 0.0, start, last: now };
        }
        let m = &mut self.meter[by];
        m.dealt += dealt.max(0.0);
        m.mended += mended.max(0.0);
        m.last = now;
    }

    /// The meter `s` sees: their party's latest fight (them alone when not in one), or empty.
    pub fn meter_line(&self, s: usize) -> String {
        let hz = self.tick_hz.max(1);
        let party = self.party_of(s);
        let latest = party.iter().map(|&o| self.meter[o].last).max().unwrap_or(0);
        if latest == 0 || self.tick.saturating_sub(latest) > SHOWN_SECS * hz {
            return "M|".into();
        }
        // The fight is the one the latest deed was in: everyone whose part ran into it.
        let start = party.iter().filter(|&&o| self.meter[o].last > 0 && latest.saturating_sub(self.meter[o].last) <= IDLE_SECS * hz).map(|&o| self.meter[o].start).min().unwrap_or(latest);
        let mut rows: Vec<(String, f32, f32)> = party
            .iter()
            .filter(|&&o| self.meter[o].last >= start && self.meter[o].last > 0)
            .map(|&o| (self.name_of(o).replace([',', ';', '|'], " "), self.meter[o].dealt, self.meter[o].mended))
            .collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1).then(b.2.total_cmp(&a.2)));
        let secs = (latest.saturating_sub(start) / hz).max(1);
        let list: Vec<String> = rows.iter().map(|r| format!("{},{:.0},{:.0}", r.0, r.1, r.2)).collect();
        format!("M|{secs};{}", list.join(";"))
    }

    /// Once a second: each player sent their meter when it changed.
    pub fn system_meter(&mut self) {
        for s in 0..self.ents.high {
            if !self.ents.alive[s] || self.ents.owner[s] >= DRIVEN {
                continue;
            }
            if self.meter[s].last == 0 && self.meter_sent[s].is_empty() && self.party[s] == 0 {
                continue; // never fought, alone: nothing to work out
            }
            let line = self.meter_line(s);
            if line == self.meter_sent[s] || (line == "M|" && self.meter_sent[s].is_empty()) {
                continue;
            }
            self.meter_sent[s] = line.clone();
            self.notes.push((s, 0, proto::chat::channel::METER, line));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::sim::Sim;
    use proto::game::GameFile;
    use proto::world::World;

    #[test]
    fn a_party_shares_one_meter_for_its_fight_and_a_new_fight_starts_afresh() {
        let mut s = Sim::new(GameFile::default_game().build().unwrap(), World::from_boxes(512.0, 1, vec![]), 64, 20, 1);
        let a = s.spawn(0).unwrap();
        let b = s.spawn(1).unwrap();
        let foe = s.spawn(crate::sim::NONE).unwrap();
        s.set_name(a, "Aoife", true);
        s.set_name(b, "Bran", true);
        s.party[a] = 1;
        s.party[b] = 1;
        let hz = s.tick_hz;
        s.tick = 100;
        let health = proto::defs::ATTR_HEALTH as usize;
        s.ents.attrs[health][foe] = 100.0;
        s.add_to(foe, health, -30.0, s.ents.id_of(a));
        s.tick = 100 + hz;
        s.add_to(foe, health, -12.0, s.ents.id_of(b));
        s.ents.attrs[health][a] = 50.0;
        s.add_to(a, health, 20.0, s.ents.id_of(b));
        let line = s.meter_line(a);
        assert_eq!(line, s.meter_line(b), "one meter for the party");
        assert!(line.starts_with("M|1;Aoife,30,0;Bran,12,20"), "{line}");
        s.system_meter();
        assert!(s.notes.iter().any(|n| n.0 == b && n.2 == proto::chat::channel::METER && n.3 == line));
        // Long quiet, then Bran fights alone: a new fight with only him in it.
        s.tick = 100 + hz * 30;
        s.add_to(foe, health, -5.0, s.ents.id_of(b));
        assert!(s.meter_line(a).ends_with(";Bran,5,0"), "{}", s.meter_line(a));
        // And a minute after the last blow it clears.
        s.tick += hz * 61;
        assert_eq!(s.meter_line(a), "M|");
    }
}

//! Weekly program ("Auto" mode).
//!
//! The stock unit stores up to 10 `from–to / level` slots per weekday in 5 profiles
//! ("Standard 1–3", "User defined 1–2"; `tools/openkwl-import/testdata`). Here a day is a list of
//! switching points: each slot starts at `start_min` and lasts until the next one, also across
//! midnight and days without slots: before the first slot of a day, the most recent earlier
//! slot of the week is still in effect.

pub const MAX_SLOTS: usize = 10;
/// Profile 1–5 here = stock profile index 0–4 (`v00901`).
pub const PROFILES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slot {
    /// Minutes since midnight, 0–1439.
    pub start_min: u16,
    pub level: u8,
}

/// Level in effect at `minute` of weekday `today` (0–6). `day(w)` gives the slots of weekday
/// `w`, sorted by start as validated by the API. Before the day's first slot, the last slot of
/// the most recent day that has one is still in effect (up to a week back, so days without
/// slots continue the program); `None` only if the whole week is empty.
pub fn level_at<D: AsRef<[Slot]>>(day: impl Fn(usize) -> D, today: usize, minute: u16) -> Option<u8> {
    // Today up to `minute`, then whole days back; the 8th look is today's last slot a week ago.
    let mut until = minute;
    for back in 0..8 {
        let slots = day((today + 7 - back) % 7);
        if let Some(s) = slots.as_ref().iter().take_while(|s| s.start_min <= until).last() {
            return Some(s.level);
        }
        until = u16::MAX;
    }
    None
}

/// The next switching point that changes the level running at `minute` of weekday `today`,
/// looking up to a week ahead. Returns the slot and in how many days it comes (0 = later
/// today); `None` if the level never changes.
pub fn next_change<D: AsRef<[Slot]>>(day: impl Fn(usize) -> D, today: usize, minute: u16) -> Option<(Slot, u8)> {
    let current = level_at(&day, today, minute);
    let mut after = i32::from(minute);
    for ahead in 0..8 {
        let slots = day((today + ahead) % 7);
        if let Some(&s) = slots.as_ref().iter().find(|s| i32::from(s.start_min) > after && Some(s.level) != current) {
            return Some((s, ahead as u8));
        }
        after = -1;
    }
    None
}

/// Whether slots are strictly increasing in start time and within the day.
pub fn is_well_formed(slots: &[Slot], max_level: u8) -> bool {
    slots.len() <= MAX_SLOTS
        && slots.iter().all(|s| s.start_min < crate::clock::MINUTES_PER_DAY && s.level <= max_level)
        && slots.windows(2).all(|w| w[0].start_min < w[1].start_min)
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn s(h: u16, m: u16, level: u8) -> Slot {
        Slot { start_min: h * 60 + m, level }
    }

    /// Profile 1, Monday, of the live unit.
    const MON: [Slot; 7] = [s(0, 0, 1), s(6, 30, 3), s(8, 0, 1), s(16, 0, 2), s(18, 0, 3), s(20, 30, 2), s(22, 30, 1)];

    /// A week with the same slots every day.
    fn every_day<'a>(slots: &'a [Slot]) -> impl Fn(usize) -> &'a [Slot] {
        move |_| slots
    }

    #[test]
    fn follows_the_switching_points() {
        let week = every_day(&MON);
        assert_eq!(level_at(&week, 0, 0), Some(1));
        assert_eq!(level_at(&week, 0, 6 * 60 + 29), Some(1));
        assert_eq!(level_at(&week, 0, 6 * 60 + 30), Some(3));
        assert_eq!(level_at(&week, 0, 19 * 60), Some(3));
        assert_eq!(level_at(&week, 0, 23 * 60 + 41), Some(1));
    }

    #[test]
    fn carries_over_midnight() {
        let late = [s(22, 0, 4)];
        let early = [s(6, 0, 2)];
        // Monday late, Tuesday early, nothing else.
        let week = |w: usize| match w {
            0 => &late[..],
            1 => &early[..],
            _ => &[][..],
        };
        assert_eq!(level_at(week, 1, 3 * 60), Some(4), "yesterday's last slot");
        assert_eq!(level_at(week, 1, 6 * 60), Some(2));
        assert_eq!(level_at(week, 1, 12 * 60), Some(2));
        assert_eq!(level_at(week, 2, 12 * 60), Some(2), "an empty day continues Tuesday's level");
        assert_eq!(level_at(week, 6, 12 * 60), Some(2), "five empty days later still");
        assert_eq!(level_at(week, 0, 12 * 60), Some(2), "Monday before its slot: last week's Tuesday");
        assert_eq!(level_at(|_| &[][..], 3, 600), None, "an empty week");
        let only_monday = |w: usize| if w == 0 { &late[..] } else { &[][..] };
        assert_eq!(level_at(only_monday, 0, 12 * 60), Some(4), "a week back: last Monday's slot");
    }

    #[test]
    fn next_change_skips_same_level() {
        let week = every_day(&MON);
        assert_eq!(next_change(&week, 0, 7 * 60), Some((s(8, 0, 1), 0)));
        let same = [s(0, 0, 1), s(8, 0, 2), s(12, 0, 2), s(18, 0, 3)];
        let mixed = |w: usize| if w == 0 { &same[..] } else { &MON[..] };
        assert_eq!(next_change(mixed, 0, 9 * 60), Some((s(18, 0, 3), 0)), "12:00 keeps level 2");
        assert_eq!(next_change(&week, 0, 23 * 60), Some((s(6, 30, 3), 1)), "tomorrow's 00:00 is level 1 too");
        assert_eq!(next_change(every_day(&[s(0, 0, 2)]), 0, 600), None);
        assert_eq!(next_change(|_| &[][..], 0, 600), None);
    }

    #[test]
    fn next_change_looks_a_week_ahead() {
        let mon = [s(8, 0, 3)];
        let fri = [s(8, 0, 1)];
        let week = |w: usize| match w {
            0 => &mon[..],
            4 => &fri[..],
            _ => &[][..],
        };
        assert_eq!(next_change(week, 1, 600), Some((s(8, 0, 1), 3)), "Tuesday: Friday's slot");
        assert_eq!(next_change(week, 4, 600), Some((s(8, 0, 3), 3)), "Friday: next Monday's slot");
        let only = [s(8, 0, 3), s(9, 0, 1)];
        let monday = |w: usize| if w == 0 { &only[..] } else { &[][..] };
        assert_eq!(next_change(monday, 0, 600), Some((s(8, 0, 3), 7)), "next week, same day, earlier time");
    }

    #[test]
    fn well_formed() {
        assert!(is_well_formed(&MON, 4));
        assert!(!is_well_formed(&[s(8, 0, 1), s(8, 0, 2)], 4), "duplicate start");
        assert!(!is_well_formed(&[s(9, 0, 1), s(8, 0, 2)], 4), "unsorted");
        assert!(!is_well_formed(&[s(8, 0, 5)], 4), "level");
        assert!(!is_well_formed(&[Slot { start_min: 1440, level: 1 }], 4));
        assert!(!is_well_formed(&[s(0, 0, 1); 11], 4));
    }
}

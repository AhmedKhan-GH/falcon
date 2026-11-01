//! The time-left estimate in the import bar: a straight-line guess from how
//! long the finished items took, worded for the bar.

use std::time::Duration;

/// Shortest time worth basing an estimate on.
const MIN_ELAPSED: Duration = Duration::from_secs(2);

/// Estimated time left after `done` of `total` items in `elapsed`, or `None`
/// while there is too little to go on (nothing done yet, or under two
/// seconds in) or nothing is left.
pub fn remaining(elapsed: Duration, done: usize, total: usize) -> Option<Duration> {
    if done == 0 || elapsed < MIN_ELAPSED || done >= total {
        return None;
    }
    Some(elapsed.mul_f64((total - done) as f64 / done as f64))
}

/// "About 5 s left", "About 2 min left", "About 1 h 5 min left". Seconds
/// above ten round to the nearest five; longer times round to whole minutes.
pub fn format(d: Duration) -> String {
    let secs = d.as_secs_f64();
    if secs < 55.0 {
        let s = if secs <= 10.0 { secs.ceil().max(1.0) as u64 } else { ((secs / 5.0).round() * 5.0) as u64 };
        if s < 60 {
            return format!("About {s} s left");
        }
    }
    let mins = (secs / 60.0).round().max(1.0) as u64;
    if mins < 60 {
        format!("About {mins} min left")
    } else if mins.is_multiple_of(60) {
        format!("About {} h left", mins / 60)
    } else {
        format!("About {} h {} min left", mins / 60, mins % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn no_estimate_until_one_done_and_two_seconds() {
        assert_eq!(remaining(s(10), 0, 10), None);
        assert_eq!(remaining(Duration::from_millis(1999), 1, 10), None);
        assert!(remaining(s(2), 1, 10).is_some());
    }

    #[test]
    fn no_estimate_when_finished() {
        assert_eq!(remaining(s(10), 10, 10), None);
        assert_eq!(remaining(s(10), 11, 10), None);
    }

    #[test]
    fn linear_estimate() {
        assert_eq!(remaining(s(6), 3, 10), Some(s(14)));
        assert_eq!(remaining(s(10), 1, 2), Some(s(10)));
        assert_eq!(remaining(s(100), 50, 51), Some(s(2)));
    }

    #[test]
    fn short_times_count_up_by_the_second() {
        assert_eq!(format(Duration::from_millis(200)), "About 1 s left");
        assert_eq!(format(s(5)), "About 5 s left");
        assert_eq!(format(Duration::from_millis(5200)), "About 6 s left");
        assert_eq!(format(s(10)), "About 10 s left");
    }

    #[test]
    fn seconds_round_to_five_above_ten() {
        assert_eq!(format(s(11)), "About 10 s left");
        assert_eq!(format(s(13)), "About 15 s left");
        assert_eq!(format(s(22)), "About 20 s left");
        assert_eq!(format(s(53)), "About 55 s left");
    }

    #[test]
    fn minutes_and_hours() {
        assert_eq!(format(s(55)), "About 1 min left");
        assert_eq!(format(s(60)), "About 1 min left");
        assert_eq!(format(s(120)), "About 2 min left");
        assert_eq!(format(s(149)), "About 2 min left");
        assert_eq!(format(s(59 * 60)), "About 59 min left");
        assert_eq!(format(s(3600)), "About 1 h left");
        assert_eq!(format(s(3900)), "About 1 h 5 min left");
        assert_eq!(format(s(2 * 3600 + 29 * 60)), "About 2 h 29 min left");
    }

    #[test]
    fn rounding_up_to_a_minute_never_says_sixty_seconds() {
        assert_eq!(format(s(58)), "About 1 min left");
        assert_eq!(format(Duration::from_millis(57_600)), "About 1 min left");
    }

    #[test]
    fn rounding_to_sixty_minutes_becomes_an_hour() {
        assert_eq!(format(s(59 * 60 + 40)), "About 1 h left");
    }
}

//! Calendar maths for the date picker: parsing and formatting `YYYY-MM-DD`,
//! the cells of a month and month stepping.

use chrono::{Datelike, NaiveDate};

/// Parses `YYYY-MM-DD` (surrounding spaces allowed); `None` for anything else.
pub fn parse_ymd(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

pub fn format_ymd(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// The cells of a month in a Monday-first week: blanks before day 1, then each day.
pub fn month_grid(year: i32, month: u32) -> Vec<Option<NaiveDate>> {
    let Some(first) = NaiveDate::from_ymd_opt(year, month, 1) else { return Vec::new() };
    let blanks = first.weekday().num_days_from_monday() as usize;
    let mut cells = vec![None; blanks];
    cells.extend(first.iter_days().take_while(|d| d.month() == month).map(Some));
    cells
}

/// Steps a month by `delta` months, wrapping across years.
pub fn add_months(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let index = year * 12 + (month as i32 - 1) + delta;
    (index.div_euclid(12), index.rem_euclid(12) as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn days(grid: &[Option<NaiveDate>]) -> usize {
        grid.iter().flatten().count()
    }

    #[test]
    fn leap_february() {
        assert_eq!(days(&month_grid(2024, 2)), 29);
        assert_eq!(days(&month_grid(2025, 2)), 28);
        assert_eq!(days(&month_grid(1900, 2)), 28);
    }

    #[test]
    fn month_starting_on_monday_has_no_blanks() {
        // 2024-01-01 was a Monday.
        let grid = month_grid(2024, 1);
        assert!(grid[0].is_some());
        assert_eq!(grid.len(), 31);
    }

    #[test]
    fn month_starting_on_sunday_has_six_blanks() {
        // 2023-10-01 was a Sunday.
        let grid = month_grid(2023, 10);
        assert_eq!(grid.iter().take_while(|c| c.is_none()).count(), 6);
        assert_eq!(grid[6], NaiveDate::from_ymd_opt(2023, 10, 1));
        assert_eq!(grid.len(), 37);
    }

    #[test]
    fn months_wrap_across_years() {
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(add_months(2026, 3, 0), (2026, 3));
        assert_eq!(add_months(2026, 3, -15), (2024, 12));
    }

    #[test]
    fn parse_and_format_round_trip() {
        let d = parse_ymd("2026-03-09").unwrap();
        assert_eq!(format_ymd(d), "2026-03-09");
        assert_eq!(parse_ymd(" 2026-03-09 "), Some(d));
    }

    #[test]
    fn bad_text_is_rejected() {
        for bad in ["", "2026-13-01", "2026-02-30", "03/09/2026", "2026-3", "abc"] {
            assert_eq!(parse_ymd(bad), None, "{bad}");
        }
    }
}

//! Minimal calendar date (proleptic Gregorian), enough for task dates:
//! parse/format `YYYY-MM-DD`, add days/months/years, weekday.
//! "Today" always comes from the UI, which knows the user's local time zone.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

impl Date {
    pub fn new(year: i32, month: u32, day: u32) -> Option<Date> {
        let valid = (1..=12).contains(&month) && day >= 1 && day <= days_in_month(year, month);
        valid.then_some(Date { year, month, day })
    }

    /// Parse `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<u32>().ok();
        Date::new(num(0..4)? as i32, num(5..7)?, num(8..10)?)
    }

    /// Days since 1970-01-01 (Howard Hinnant's days_from_civil).
    pub fn to_days(self) -> i64 {
        let (y, m, d) = (self.year as i64, self.month as i64, self.day as i64);
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }

    pub fn from_days(days: i64) -> Date {
        let z = days + 719468;
        let era = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as i32;
        Date { year, month, day }
    }

    pub fn add_days(self, n: i64) -> Date {
        Date::from_days(self.to_days() + n)
    }

    /// Add months, clamping to the last day of the month (Jan 31 + 1 month = Feb 28/29).
    pub fn add_months(self, n: i64) -> Date {
        let total = self.year as i64 * 12 + (self.month as i64 - 1) + n;
        let year = total.div_euclid(12) as i32;
        let month = (total.rem_euclid(12) + 1) as u32;
        Date { year, month, day: self.day.min(days_in_month(year, month)) }
    }

    pub fn days_until(self, other: Date) -> i64 {
        other.to_days() - self.to_days()
    }

    /// 0 = Monday … 6 = Sunday.
    pub fn weekday(self) -> u32 {
        // 1970-01-01 was a Thursday (3).
        (self.to_days() + 3).rem_euclid(7) as u32
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn parse_and_format() {
        assert_eq!(d("2026-11-13").to_string(), "2026-11-13");
        for bad in ["2026-13-01", "2026-02-30", "2026-1-01", "20261113", "", "2026-11-1x"] {
            assert_eq!(Date::parse(bad), None, "{bad}");
        }
        assert!(Date::parse("2024-02-29").is_some());
        assert!(Date::parse("2025-02-29").is_none());
    }

    #[test]
    fn days_roundtrip() {
        assert_eq!(d("1970-01-01").to_days(), 0);
        for s in ["1999-12-31", "2000-02-29", "2026-11-13", "2100-03-01", "1600-01-01"] {
            assert_eq!(Date::from_days(d(s).to_days()), d(s));
        }
        assert_eq!(d("2026-12-31").add_days(1), d("2027-01-01"));
        assert_eq!(d("2024-03-01").add_days(-1), d("2024-02-29"));
        assert_eq!(d("2026-11-13").days_until(d("2026-11-20")), 7);
    }

    #[test]
    fn months_clamp_to_month_end() {
        assert_eq!(d("2026-01-31").add_months(1), d("2026-02-28"));
        assert_eq!(d("2024-01-31").add_months(1), d("2024-02-29"));
        assert_eq!(d("2026-11-30").add_months(3), d("2027-02-28"));
        assert_eq!(d("2026-03-15").add_months(-3), d("2025-12-15"));
        assert_eq!(d("2024-02-29").add_months(12), d("2025-02-28"));
    }

    #[test]
    fn weekday() {
        assert_eq!(d("2026-11-13").weekday(), 4); // Friday
        assert_eq!(d("2026-11-16").weekday(), 0); // Monday
    }
}

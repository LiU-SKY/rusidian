//! Recurrence rules as the Obsidian Tasks plugin writes them (`🔁 every …`).
//!
//! Supported:
//! - `every [N|other] day|week|month|year[s]`
//! - `every weekday`, `every weekend`, `every monday, wednesday and friday`
//! - `every [N] week[s] on tuesday[, …]`
//! - `every [N] month[s] on the 15th[, 1st and …]`, `… on the last [day]`,
//!   `… on the 2nd tuesday`, `… on the last friday`
//! - `every [N] year[s] on january 5[th]`
//! - any of the above followed by `when done`

use crate::date::Date;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Day,
    Week,
    Month,
    Year,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pattern {
    /// Every `n` units after the reference date (months clamp to the month end).
    Interval { n: i64, unit: Unit },
    /// On these weekdays (0 = Monday) of every `n`th week.
    Weekdays { n: i64, days: [bool; 7] },
    /// On these days of every `n`th month (-1 = the last day). Months
    /// without such a day are skipped, like rrule does.
    MonthDays { n: i64, days: Vec<i32> },
    /// On the `nth` (1..=4, or -1 = last) `weekday` of every `n`th month.
    MonthWeekday { n: i64, nth: i32, weekday: u32 },
    /// On `month`/`day` of every `n`th year.
    YearDate { n: i64, month: u32, day: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub pattern: Pattern,
    /// Count from the completion day instead of the task's own date.
    pub when_done: bool,
}

const WEEKDAYS: [&[&str]; 7] = [
    &["monday", "mon"],
    &["tuesday", "tue", "tues"],
    &["wednesday", "wed"],
    &["thursday", "thu", "thur", "thurs"],
    &["friday", "fri"],
    &["saturday", "sat"],
    &["sunday", "sun"],
];

const MONTHS: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];

fn weekday_of(word: &str) -> Option<u32> {
    let w = word.trim_end_matches('s'); // "mondays"
    WEEKDAYS.iter().position(|names| names.contains(&w) || names.contains(&word)).map(|i| i as u32)
}

fn month_of(word: &str) -> Option<u32> {
    MONTHS.iter().position(|m| *m == word || (word.len() >= 3 && m.starts_with(word))).map(|i| i as u32 + 1)
}

/// "1st", "2nd", "15th", "15", "first" … "fourth", "last" (-1).
fn ordinal(word: &str) -> Option<i32> {
    match word {
        "first" => return Some(1),
        "second" => return Some(2),
        "third" => return Some(3),
        "fourth" => return Some(4),
        "fifth" => return Some(5),
        "last" => return Some(-1),
        _ => {}
    }
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let n: i32 = digits.parse().ok()?;
    (1..=31).contains(&n).then_some(n)
}

pub fn parse(text: &str) -> Option<Rule> {
    let lower = text.trim().to_lowercase().replace(',', " ");
    let (body, when_done) = match lower.trim_end().strip_suffix("when done") {
        Some(b) => (b.trim().to_string(), true),
        None => (lower.trim().to_string(), false),
    };
    let words: Vec<&str> = body.split_whitespace().filter(|w| *w != "and").collect();
    let (first, rest) = words.split_first()?;
    if *first != "every" {
        return None;
    }
    let pattern = parse_pattern(rest)?;
    Some(Rule { pattern, when_done })
}

fn parse_pattern(words: &[&str]) -> Option<Pattern> {
    let mut i = 0;
    let word = |i: usize| words.get(i).copied();

    match word(0)? {
        "weekday" | "weekdays" if words.len() == 1 => {
            return Some(Pattern::Weekdays { n: 1, days: [true, true, true, true, true, false, false] });
        }
        "weekend" | "weekends" if words.len() == 1 => {
            return Some(Pattern::Weekdays { n: 1, days: [false, false, false, false, false, true, true] });
        }
        w if weekday_of(w).is_some() => {
            // every monday, wednesday and friday
            return weekdays(words).map(|days| Pattern::Weekdays { n: 1, days });
        }
        _ => {}
    }

    let n = match word(0)? {
        "other" => {
            i += 1;
            2
        }
        w => match w.parse::<i64>() {
            Ok(n) if n > 0 => {
                i += 1;
                n
            }
            Ok(_) => return None,
            Err(_) => 1,
        },
    };
    let unit = match word(i)?.trim_end_matches('s') {
        "day" => Unit::Day,
        "week" => Unit::Week,
        "month" => Unit::Month,
        "year" => Unit::Year,
        _ => return None,
    };
    i += 1;
    if i == words.len() {
        return Some(Pattern::Interval { n, unit });
    }
    if word(i)? != "on" {
        return None;
    }
    let on: Vec<&str> = words[i + 1..].iter().copied().filter(|w| *w != "the").collect();
    if on.is_empty() {
        return None;
    }

    match unit {
        Unit::Day => None,
        Unit::Week => weekdays(&on).map(|days| Pattern::Weekdays { n, days }),
        Unit::Month => {
            // "on the 2nd tuesday" / "on the last friday"
            if on.len() == 2 {
                if let (Some(nth), Some(weekday)) = (ordinal(on[0]), weekday_of(on[1])) {
                    if nth == -1 || (1..=4).contains(&nth) {
                        return Some(Pattern::MonthWeekday { n, nth, weekday });
                    }
                    return None;
                }
            }
            // "on the 1st 15th", "on the last [day]"
            let mut days = Vec::new();
            for w in on {
                if w == "day" {
                    continue;
                }
                days.push(ordinal(w)?);
            }
            days.sort_by_key(|d| if *d < 0 { 99 } else { *d });
            days.dedup();
            (!days.is_empty()).then_some(Pattern::MonthDays { n, days })
        }
        Unit::Year => {
            // "on january 5th"
            let [m, d] = on.as_slice() else { return None };
            let month = month_of(m)?;
            let day = ordinal(d).filter(|d| *d > 0)? as u32;
            Some(Pattern::YearDate { n, month, day })
        }
    }
}

fn weekdays(words: &[&str]) -> Option<[bool; 7]> {
    let mut days = [false; 7];
    for w in words {
        days[weekday_of(w)? as usize] = true;
    }
    days.iter().any(|d| *d).then_some(days)
}

/// Monday-based week number, for "every 2 weeks on …".
fn week_index(d: Date) -> i64 {
    (d.to_days() - d.weekday() as i64).div_euclid(7)
}

fn month_index(d: Date) -> i64 {
    d.year as i64 * 12 + d.month as i64 - 1
}

fn month_start(index: i64) -> Date {
    Date::new(index.div_euclid(12) as i32, (index.rem_euclid(12) + 1) as u32, 1).expect("valid month")
}

/// The `nth` (-1 = last) `weekday` in the month starting at `first`.
fn nth_weekday(first: Date, nth: i32, weekday: u32) -> Option<Date> {
    let len = crate::date::days_in_month(first.year, first.month) as i64;
    let offset = (weekday as i64 - first.weekday() as i64).rem_euclid(7);
    let day = if nth > 0 {
        offset + (nth as i64 - 1) * 7
    } else {
        offset + ((len - 1 - offset) / 7) * 7
    };
    (day < len).then(|| first.add_days(day))
}

impl Rule {
    /// The first occurrence strictly after `after`, counting intervals from `after`.
    pub fn next(&self, after: Date) -> Date {
        match &self.pattern {
            Pattern::Interval { n, unit } => match unit {
                Unit::Day => after.add_days(*n),
                Unit::Week => after.add_days(n * 7),
                Unit::Month => after.add_months(*n),
                Unit::Year => after.add_months(n * 12),
            },
            Pattern::Weekdays { n, days } => {
                let base = week_index(after);
                (1..=7 * n + 7)
                    .map(|k| after.add_days(k))
                    .find(|d| days[d.weekday() as usize] && (week_index(*d) - base).rem_euclid(*n) == 0)
                    .unwrap_or_else(|| after.add_days(7 * n))
            }
            Pattern::MonthDays { n, days } => {
                let base = month_index(after);
                for k in (0..=48 * n).step_by(*n as usize) {
                    let first = month_start(base + k);
                    let len = crate::date::days_in_month(first.year, first.month) as i32;
                    for &d in days {
                        let day = if d < 0 { len } else { d };
                        if day > len {
                            continue; // e.g. the 31st in a 30-day month
                        }
                        let date = first.add_days(day as i64 - 1);
                        if date > after {
                            return date;
                        }
                    }
                }
                after.add_months(*n)
            }
            Pattern::MonthWeekday { n, nth, weekday } => {
                let base = month_index(after);
                (0..=48 * n)
                    .step_by(*n as usize)
                    .filter_map(|k| nth_weekday(month_start(base + k), *nth, *weekday))
                    .find(|d| *d > after)
                    .unwrap_or_else(|| after.add_months(*n))
            }
            Pattern::YearDate { n, month, day } => (0..=8 * n)
                .step_by(*n as usize)
                .filter_map(|k| Date::new(after.year + k as i32, *month, *day))
                .find(|d| *d > after)
                .unwrap_or_else(|| after.add_months(n * 12)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn next(rule: &str, after: &str) -> String {
        parse(rule).unwrap_or_else(|| panic!("unparsed: {rule}")).next(d(after)).to_string()
    }

    #[test]
    fn intervals() {
        assert_eq!(next("every day", "2026-11-13"), "2026-11-14");
        assert_eq!(next("every 3 days", "2026-11-13"), "2026-11-16");
        assert_eq!(next("every other week", "2026-11-13"), "2026-11-27");
        assert_eq!(next("Every Month", "2026-01-31"), "2026-02-28");
        assert_eq!(next("every 2 years", "2024-02-29"), "2026-02-28");
        assert!(parse("every 3 days when done").unwrap().when_done);
    }

    #[test]
    fn weekdays() {
        // 2026-11-13 is a Friday
        assert_eq!(next("every weekday", "2026-11-13"), "2026-11-16");
        assert_eq!(next("every weekday", "2026-11-16"), "2026-11-17");
        assert_eq!(next("every weekend", "2026-11-13"), "2026-11-14");
        assert_eq!(next("every monday, wednesday and friday", "2026-11-16"), "2026-11-18");
        assert_eq!(next("every week on Tuesday, Thursday", "2026-11-13"), "2026-11-17");
        // every 2 weeks on monday: skip the next week's Monday
        assert_eq!(next("every 2 weeks on monday", "2026-11-16"), "2026-11-30");
        assert_eq!(next("every 2 weeks on monday, friday", "2026-11-16"), "2026-11-20");
    }

    #[test]
    fn month_days() {
        assert_eq!(next("every month on the 15th", "2026-11-13"), "2026-11-15");
        assert_eq!(next("every month on the 15th", "2026-11-15"), "2026-12-15");
        assert_eq!(next("every month on the 1st and 15th", "2026-11-15"), "2026-12-01");
        assert_eq!(next("every month on the last", "2026-01-31"), "2026-02-28");
        assert_eq!(next("every month on the last day", "2026-02-28"), "2026-03-31");
        assert_eq!(next("every month on the 31st", "2026-03-31"), "2026-05-31"); // April has no 31st
        assert_eq!(next("every 3 months on the 1st", "2026-11-13"), "2027-02-01");
    }

    #[test]
    fn month_weekdays() {
        assert_eq!(next("every month on the 2nd tuesday", "2026-11-01"), "2026-11-10");
        assert_eq!(next("every month on the 2nd tuesday", "2026-11-10"), "2026-12-08");
        assert_eq!(next("every month on the last friday", "2026-11-13"), "2026-11-27");
        assert_eq!(next("every month on the first monday", "2026-11-27"), "2026-12-07");
    }

    #[test]
    fn year_dates() {
        assert_eq!(next("every year on january 5th", "2026-11-13"), "2027-01-05");
        assert_eq!(next("every year on Nov 20", "2026-11-13"), "2026-11-20");
        assert_eq!(next("every year on february 29", "2026-01-01"), "2028-02-29");
    }

    #[test]
    fn rejects_unknown() {
        for rule in ["daily", "every", "every 0 days", "every fortnight", "every day on monday", "every month on the 40th", "every week on someday"] {
            assert!(parse(rule).is_none(), "{rule}");
        }
    }
}

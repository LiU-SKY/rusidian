//! moment.js date formatting as Obsidian does it, in the Korean locale
//! (Obsidian in Korean names days and months the same way): `dddd` is
//! 금요일, `MMMM` is 11월, `Do` is 13일, weeks start on Sunday.
//!
//! Patterns are split the way moment splits them: `[text]` is literal, `\x`
//! escapes one character, and the longest token wins at each place, so
//! the letters of an unbracketed word can turn into tokens ("Daily" is
//! D, a, i, l, y).

use crate::date::{days_in_month, Date};
use regex::Regex;
use std::sync::LazyLock;

/// A piece of a split pattern.
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Text(String),
    Token(String),
}

/// Korean long date formats (`L`, `ll`, `LT` …), as moment's ko locale has them.
fn long_format(token: &str) -> &'static str {
    match token {
        "LT" => "A h:mm",
        "LTS" => "A h:mm:ss",
        "L" | "l" => "YYYY.MM.DD.",
        "LL" | "ll" => "YYYY년 MMMM D일",
        "LLL" | "lll" => "YYYY년 MMMM D일 A h:mm",
        _ => "YYYY년 MMMM D일 dddd A h:mm",
    }
}

/// Replace the localized `L…`/`l…` tokens first, as moment does.
fn expand(pattern: &str) -> String {
    static LONG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\[]*\]|(\\)?(LTS|LT|LL?L?L?|l{1,4})").unwrap());
    LONG.replace_all(pattern, |c: &regex::Captures| match (c.get(1), c.get(2)) {
        (None, Some(t)) => long_format(t.as_str()).to_string(),
        _ => c[0].to_string(),
    })
    .into_owned()
}

/// Split a pattern into literal text and tokens.
pub fn pieces(pattern: &str) -> Vec<Piece> {
    // moment's own token expression, in its order (the first match wins).
    static TOKEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"(?s)\[[^\[]*\]|\\.|[Hh]mm(?:ss)?|Mo|MM?M?M?|Do|DDDo|DD?D?D?|ddd?d?|do?|w[ow]?|W[oW]?|Qo?|N{1,5}|YYYYYY|YYYYY|YYYY|YY|y{2,4}|yo?|gg(?:ggg?)?|GG(?:GGG?)?|e|E|a|A|hh?|HH?|kk?|mm?|ss?|S{1,9}|x|X|zz?|ZZ?|.",
        )
        .unwrap()
    });
    let expanded = expand(pattern);
    let mut out: Vec<Piece> = Vec::new();
    let push_text = |out: &mut Vec<Piece>, s: &str| match out.last_mut() {
        Some(Piece::Text(t)) => t.push_str(s),
        _ => out.push(Piece::Text(s.to_string())),
    };
    for m in TOKEN.find_iter(&expanded) {
        let t = m.as_str();
        if t.starts_with('[') && t.ends_with(']') && t.len() >= 2 {
            push_text(&mut out, &t[1..t.len() - 1]);
        } else if let Some(rest) = t.strip_prefix('\\').filter(|r| !r.is_empty()) {
            push_text(&mut out, rest);
        } else if is_token(t) {
            out.push(Piece::Token(t.to_string()));
        } else {
            push_text(&mut out, t);
        }
    }
    out
}

/// Single characters that are tokens; anything else the expression matched
/// with `.` is plain text.
fn is_token(t: &str) -> bool {
    t.chars().count() > 1 || "MDdwWQYyeEaAhHkmsSxXzZN".contains(t)
}

/// Needs the time of day (or is not supported at all).
pub fn needs_time(token: &str) -> bool {
    matches!(token.chars().next(), Some('H' | 'h' | 'k' | 'm' | 's' | 'S' | 'a' | 'A' | 'x' | 'X' | 'z' | 'Z' | 'N'))
}

const WEEKDAYS: [&str; 7] = ["일", "월", "화", "수", "목", "금", "토"];

/// 0 = Sunday … 6 = Saturday, as JavaScript counts.
fn js_day(d: Date) -> u32 {
    (d.weekday() + 1) % 7
}

fn day_of_year(d: Date) -> u32 {
    (1..d.month).map(|m| days_in_month(d.year, m)).sum::<u32>() + d.day
}

fn days_in_year(year: i32) -> i64 {
    if days_in_month(year, 2) == 29 { 366 } else { 365 }
}

/// moment's firstWeekOffset: where week 1 starts, relative to Jan 1.
fn first_week_offset(year: i32, dow: i64, doy: i64) -> i64 {
    let fwd = 7 + dow - doy; // the January day always in week 1
    let jan = Date::new(year, 1, fwd as u32).unwrap();
    let fwdlw = (7 + js_day(jan) as i64 - dow) % 7;
    -fwdlw + fwd - 1
}

fn weeks_in_year(year: i32, dow: i64, doy: i64) -> i64 {
    (days_in_year(year) - first_week_offset(year, dow, doy) + first_week_offset(year + 1, dow, doy)) / 7
}

/// (week, week-year): ISO weeks with dow 1 / doy 4, the Korean (default)
/// locale's with dow 0 / doy 6.
fn week_of_year(d: Date, dow: i64, doy: i64) -> (i64, i32) {
    let offset = first_week_offset(d.year, dow, doy);
    let week = (day_of_year(d) as i64 - offset - 1).div_euclid(7) + 1;
    if week < 1 {
        (week + weeks_in_year(d.year - 1, dow, doy), d.year - 1)
    } else if week > weeks_in_year(d.year, dow, doy) {
        (week - weeks_in_year(d.year, dow, doy), d.year + 1)
    } else {
        (week, d.year)
    }
}

fn pad(n: i64, width: usize) -> String {
    if n < 0 { format!("-{:0width$}", -n) } else { format!("{n:0width$}") }
}

/// The text of one token for `d` at `time` (hour, minute); None when the
/// token needs a time and none was given, or isn't supported.
pub fn token_text(token: &str, d: Date, time: Option<(u32, u32)>) -> Option<String> {
    let year = d.year as i64;
    let (week, week_year) = week_of_year(d, 0, 6);
    let (iso_week, iso_year) = week_of_year(d, 1, 4);
    let text = match token {
        "M" => d.month.to_string(),
        "MM" => pad(d.month as i64, 2),
        "MMM" | "MMMM" | "Mo" => format!("{}월", d.month),
        "D" => d.day.to_string(),
        "DD" => pad(d.day as i64, 2),
        "Do" => format!("{}일", d.day),
        "DDD" => day_of_year(d).to_string(),
        "DDDD" => pad(day_of_year(d) as i64, 3),
        "DDDo" => format!("{}일", day_of_year(d)),
        "d" | "e" => js_day(d).to_string(),
        "do" => format!("{}일", js_day(d)),
        "dd" | "ddd" => WEEKDAYS[js_day(d) as usize].to_string(),
        "dddd" => format!("{}요일", WEEKDAYS[js_day(d) as usize]),
        "E" => (d.weekday() + 1).to_string(),
        "w" => week.to_string(),
        "ww" => pad(week, 2),
        "wo" => format!("{week}주"),
        "W" => iso_week.to_string(),
        "WW" => pad(iso_week, 2),
        "Wo" => format!("{iso_week}주"),
        "Q" | "Qo" => d.month.div_ceil(3).to_string(),
        "Y" | "YYYY" => pad(year, 4),
        "YY" => pad(year.rem_euclid(100), 2),
        "YYYYY" => pad(year, 5),
        "YYYYYY" => format!("{}{}", if year < 0 { '-' } else { '+' }, pad(year.abs(), 6)),
        "y" | "yo" | "yyy" | "yyyy" => year.to_string(),
        "yy" => pad(year.rem_euclid(100), 2),
        "gg" => pad((week_year as i64).rem_euclid(100), 2),
        "gggg" => pad(week_year as i64, 4),
        "ggggg" => pad(week_year as i64, 5),
        "GG" => pad((iso_year as i64).rem_euclid(100), 2),
        "GGGG" => pad(iso_year as i64, 4),
        "GGGGG" => pad(iso_year as i64, 5),
        _ => {
            let (h, m) = time?;
            let h12 = if h % 12 == 0 { 12 } else { h % 12 };
            let k = if h == 0 { 24 } else { h };
            match token {
                "H" => h.to_string(),
                "HH" => pad(h as i64, 2),
                "h" => h12.to_string(),
                "hh" => pad(h12 as i64, 2),
                "k" => k.to_string(),
                "kk" => pad(k as i64, 2),
                "m" => m.to_string(),
                "mm" => pad(m as i64, 2),
                "s" => "0".into(),
                "ss" => "00".into(),
                "a" | "A" => (if h < 12 { "오전" } else { "오후" }).into(),
                "Hmm" => format!("{h}{m:02}"),
                "Hmmss" => format!("{h}{m:02}00"),
                "hmm" => format!("{h12}{m:02}"),
                "hmmss" => format!("{h12}{m:02}00"),
                t if t.starts_with('S') => "0".repeat(t.len()),
                _ => return None,
            }
        }
    };
    Some(text)
}

/// Format `d` at `time`. Tokens that can't be filled (time-only tokens
/// without a time, time zones, eras) are kept as written.
pub fn format(pattern: &str, d: Date, time: Option<(u32, u32)>) -> String {
    pieces(pattern)
        .into_iter()
        .map(|p| match p {
            Piece::Text(t) => t,
            Piece::Token(t) => token_text(&t, d, time).unwrap_or(t),
        })
        .collect()
}

/// The first word (letters outside `[ ]`) of `pattern` holding a token
/// that a date alone can't fill, e.g. "Daily" in "YYYY-MM-DD - Daily".
pub fn bad_word(pattern: &str) -> Option<String> {
    static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\[]*\]|\\.|[A-Za-z]+").unwrap());
    let day = Date::new(2000, 1, 1)?;
    WORD.find_iter(pattern)
        .map(|m| m.as_str())
        .filter(|w| w.chars().all(|c| c.is_ascii_alphabetic()))
        .find(|w| pieces(w).iter().any(|p| matches!(p, Piece::Token(t) if token_text(t, day, None).is_none())))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn date_tokens_in_korean() {
        let day = d("2026-11-13"); // a Friday
        assert_eq!(format("YYYY-MM-DD dddd", day, None), "2026-11-13 금요일");
        assert_eq!(format("YY.M.D (ddd)", day, None), "26.11.13 (금)");
        assert_eq!(format("MMMM Do", day, None), "11월 13일");
        assert_eq!(format("DDDD DDD [Q]Q", day, None), "317 317 Q4");
        assert_eq!(format("d e E", day, None), "5 5 5");
        assert_eq!(format("ll", day, None), "2026년 11월 13일");
        assert_eq!(format("L", day, None), "2026.11.13.");
        assert_eq!(format(r"[Week] \W", day, None), "Week W");
        assert_eq!(format("YYYY-MM-DD HH:mm A", day, Some((14, 5))), "2026-11-13 14:05 오후");
        // Time tokens without a time stay as written.
        assert_eq!(format("HH:mm", day, None), "HH:mm");
    }

    #[test]
    fn weeks_like_moment() {
        // ISO: 2027-01-01 (Fri) is in 2026's week 53.
        assert_eq!(format("GGGG-[W]WW", d("2027-01-01"), None), "2026-W53");
        assert_eq!(format("GGGG-[W]WW", d("2026-01-01"), None), "2026-W01");
        assert_eq!(format("GGGG-[W]WW", d("2024-12-30"), None), "2025-W01");
        // Korean weeks start on Sunday; week 1 holds Jan 1.
        assert_eq!(format("gggg-[W]ww", d("2026-01-01"), None), "2026-W01");
        assert_eq!(format("gggg-[W]ww", d("2026-01-04"), None), "2026-W02"); // a Sunday
        assert_eq!(format("gggg-[W]ww", d("2025-12-28"), None), "2026-W01"); // the Sunday before
        assert_eq!(format("gggg-[W]ww wo", d("2026-11-13"), None), "2026-W46 46주");
    }

    #[test]
    fn words_split_into_tokens_like_moment() {
        let p = pieces("YYYY - Daily");
        let tokens: Vec<&str> = p.iter().filter_map(|p| if let Piece::Token(t) = p { Some(t.as_str()) } else { None }).collect();
        // "l" expands to the long date before splitting.
        assert_eq!(tokens[..4], ["YYYY", "D", "a", "YYYY"]);
        assert_eq!(bad_word("YYYY/YYYY-MM-DD - Daily").as_deref(), Some("Daily"));
        assert_eq!(bad_word("YYYY-MM-DD [Daily] dddd"), None);
        assert_eq!(bad_word("YYYY-MM-DD HH"), Some("HH".into()));
        assert!(needs_time("a"));
        assert!(!needs_time("D"));
        assert_eq!(format("[Daily] YYYY", d("2026-11-13"), None), "Daily 2026");
    }
}

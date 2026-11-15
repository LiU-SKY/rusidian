//! iCalendar (`.ics`) events for the calendar view: subscriptions such as
//! Google Calendar's "secret address in iCal format".
//!
//! Handled: all-day and timed events, `TZID`/UTC/floating times, `DTEND`
//! or `DURATION`, `RRULE` (DAILY/WEEKLY/MONTHLY/YEARLY with INTERVAL,
//! COUNT, UNTIL, BYDAY incl. `2TU`/`-1FR`, BYMONTHDAY, BYMONTH, BYSETPOS),
//! `EXDATE`, changed occurrences (`RECURRENCE-ID`) and cancelled events.
//! Times stay as wall-clock time in their zone; the UI converts them to
//! local time (it knows the user's zone), so expansion keeps the wall time
//! across daylight-saving changes like calendar apps do.

use crate::date::Date;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

/// A wall-clock date and time (seconds since midnight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Naive {
    pub date: Date,
    pub secs: u32,
}

impl Naive {
    fn add_secs(self, secs: i64) -> Naive {
        let total = self.date.to_days() * 86_400 + self.secs as i64 + secs;
        Naive { date: Date::from_days(total.div_euclid(86_400)), secs: total.rem_euclid(86_400) as u32 }
    }

    fn secs_until(self, other: Naive) -> i64 {
        (other.date.to_days() - self.date.to_days()) * 86_400 + other.secs as i64 - self.secs as i64
    }

    fn fmt(self, all_day: bool) -> String {
        if all_day {
            self.date.to_string()
        } else {
            let (h, m, s) = (self.secs / 3600, self.secs / 60 % 60, self.secs % 60);
            format!("{}T{h:02}:{m:02}:{s:02}", self.date)
        }
    }
}

/// When an event happens, as written.
#[derive(Debug, Clone, PartialEq)]
struct When {
    at: Naive,
    all_day: bool,
    /// `Some("UTC")`, an IANA zone, or `None` for floating (local) time.
    tz: Option<String>,
}

/// One occurrence in the requested range, for the UI.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Occurrence {
    pub uid: String,
    pub summary: String,
    pub location: Option<String>,
    pub description: Option<String>,
    pub all_day: bool,
    /// `YYYY-MM-DD` (all-day) or `YYYY-MM-DDTHH:MM:SS`, wall time in `tz`.
    pub start: String,
    /// Exclusive end, same form as `start`.
    pub end: String,
    /// `"UTC"`, an IANA zone name, or null for local time.
    pub tz: Option<String>,
}

// ----------------------------------------------------------------- lexing

/// Content lines with folding undone (a line starting with a space or tab
/// continues the previous one).
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        if !line.is_empty() {
            out.push(line.to_string());
        }
    }
    out
}

struct Prop {
    name: String,
    params: HashMap<String, String>,
    value: String,
}

/// `NAME;PARAM=VALUE;P2="quoted:value":VALUE`
fn parse_prop(line: &str) -> Option<Prop> {
    let mut in_quote = false;
    let mut colon = None;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_quote = !in_quote,
            ':' if !in_quote => {
                colon = Some(i);
                break;
            }
            _ => {}
        }
    }
    let colon = colon?;
    let (head, value) = (&line[..colon], &line[colon + 1..]);
    let mut parts = head.split(';');
    let name = parts.next()?.trim().to_ascii_uppercase();
    let params = parts
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_ascii_uppercase(), v.trim().trim_matches('"').to_string()))
        .collect();
    Some(Prop { name, params, value: value.to_string() })
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// Common Windows zone names (Outlook) → IANA.
const WINDOWS_ZONES: &[(&str, &str)] = &[
    ("Korea Standard Time", "Asia/Seoul"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("India Standard Time", "Asia/Kolkata"),
    ("GMT Standard Time", "Europe/London"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central Europe Standard Time", "Europe/Budapest"),
    ("Eastern Standard Time", "America/New_York"),
    ("Central Standard Time", "America/Chicago"),
    ("Mountain Standard Time", "America/Denver"),
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("UTC", "UTC"),
    ("Coordinated Universal Time", "UTC"),
];

/// A TZID as an IANA name: Windows names mapped, `/mozilla.org/…/Asia/Seoul`
/// prefixes dropped.
fn normalize_tz(tzid: &str) -> String {
    let t = tzid.trim();
    if let Some((_, iana)) = WINDOWS_ZONES.iter().find(|(w, _)| w.eq_ignore_ascii_case(t)) {
        return iana.to_string();
    }
    if t.starts_with('/') {
        let parts: Vec<&str> = t.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() >= 2 {
            return parts[parts.len() - 2..].join("/");
        }
    }
    t.to_string()
}

/// `20261120`, `20261120T090000` or `20261120T090000Z`.
fn parse_naive(value: &str) -> Option<(Naive, bool, bool)> {
    let v = value.trim();
    let (v, utc) = match v.strip_suffix(['Z', 'z']) {
        Some(rest) => (rest, true),
        None => (v, false),
    };
    let num = |s: &str| s.parse::<u32>().ok();
    let date = Date::new(v.get(0..4)?.parse().ok()?, num(v.get(4..6)?)?, num(v.get(6..8)?)?)?;
    if v.len() == 8 {
        return Some((Naive { date, secs: 0 }, true, false));
    }
    if v.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let (h, m) = (num(v.get(9..11)?)?, num(v.get(11..13)?)?);
    let s = v.get(13..15).and_then(num).unwrap_or(0);
    (h < 24 && m < 60 && s < 61).then_some((Naive { date, secs: h * 3600 + m * 60 + s.min(59) }, false, utc))
}

fn parse_when(prop: &Prop) -> Option<When> {
    let (at, date_only, utc) = parse_naive(&prop.value)?;
    let all_day = date_only || prop.params.get("VALUE").is_some_and(|v| v.eq_ignore_ascii_case("DATE"));
    let tz = if all_day {
        None
    } else if utc {
        Some("UTC".to_string())
    } else {
        prop.params.get("TZID").map(|t| normalize_tz(t))
    };
    Some(When { at, all_day, tz })
}

/// `P1D`, `PT1H30M`, `P2W`, `-PT15M` → seconds.
fn parse_duration(v: &str) -> Option<i64> {
    let v = v.trim();
    let (sign, v) = match v.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, v.strip_prefix('+').unwrap_or(v)),
    };
    let v = v.strip_prefix('P')?;
    let mut total = 0i64;
    let mut num = String::new();
    let mut in_time = false;
    for c in v.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => num.push(c),
            _ => {
                let n: i64 = num.parse().ok()?;
                num.clear();
                total += n * match (c, in_time) {
                    ('W', false) => 7 * 86_400,
                    ('D', false) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    num.is_empty().then_some(sign * total)
}

// ----------------------------------------------------------------- events

#[derive(Debug, Clone, Default)]
struct RawEvent {
    uid: String,
    summary: String,
    location: Option<String>,
    description: Option<String>,
    start: Option<When>,
    end: Option<When>,
    duration: Option<i64>,
    rrule: Option<String>,
    exdates: Vec<Naive>,
    recurrence_id: Option<Naive>,
    cancelled: bool,
}

fn parse_events(text: &str) -> Vec<RawEvent> {
    let mut events = Vec::new();
    let mut current: Option<RawEvent> = None;
    // Depth of components nested in the event (VALARM), whose props we skip.
    let mut nested = 0;
    for line in unfold(text) {
        let Some(prop) = parse_prop(&line) else { continue };
        let value_upper = prop.value.trim().to_ascii_uppercase();
        match (prop.name.as_str(), current.as_mut()) {
            ("BEGIN", None) if value_upper == "VEVENT" => current = Some(RawEvent::default()),
            ("BEGIN", Some(_)) => nested += 1,
            ("END", Some(_)) if nested > 0 => nested -= 1,
            ("END", Some(_)) if value_upper == "VEVENT" => events.extend(current.take()),
            (_, Some(_)) if nested > 0 => {}
            (name, Some(ev)) => match name {
                "UID" => ev.uid = prop.value.trim().to_string(),
                "SUMMARY" => ev.summary = unescape(&prop.value),
                "LOCATION" => ev.location = Some(unescape(&prop.value)).filter(|s| !s.trim().is_empty()),
                "DESCRIPTION" => ev.description = Some(unescape(&prop.value)).filter(|s| !s.trim().is_empty()),
                "DTSTART" => ev.start = parse_when(&prop),
                "DTEND" => ev.end = parse_when(&prop),
                "DURATION" => ev.duration = parse_duration(&prop.value),
                "RRULE" => ev.rrule = Some(prop.value.trim().to_string()),
                "EXDATE" => ev.exdates.extend(prop.value.split(',').filter_map(|v| parse_naive(v).map(|n| n.0))),
                "RECURRENCE-ID" => ev.recurrence_id = parse_naive(&prop.value).map(|n| n.0),
                "STATUS" => ev.cancelled = value_upper == "CANCELLED",
                _ => {}
            },
            _ => {}
        }
    }
    events
}

// ----------------------------------------------------------------- RRULE

#[derive(Debug, Clone, Copy, PartialEq)]
enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone)]
struct RRule {
    freq: Freq,
    interval: i64,
    count: Option<usize>,
    until: Option<Naive>,
    /// (ordinal, weekday 0 = Monday); ordinal 0 = every such weekday.
    by_day: Vec<(i32, u32)>,
    by_month_day: Vec<i32>,
    by_month: Vec<u32>,
    by_set_pos: Vec<i32>,
}

fn parse_rrule(s: &str) -> Option<RRule> {
    let mut rule = RRule {
        freq: Freq::Daily,
        interval: 1,
        count: None,
        until: None,
        by_day: Vec::new(),
        by_month_day: Vec::new(),
        by_month: Vec::new(),
        by_set_pos: Vec::new(),
    };
    let mut freq = None;
    for part in s.split(';') {
        let Some((k, v)) = part.split_once('=') else { continue };
        let list = || v.split(',').map(str::trim).filter(|x| !x.is_empty());
        match k.trim().to_ascii_uppercase().as_str() {
            "FREQ" => {
                freq = Some(match v.trim().to_ascii_uppercase().as_str() {
                    "DAILY" => Freq::Daily,
                    "WEEKLY" => Freq::Weekly,
                    "MONTHLY" => Freq::Monthly,
                    "YEARLY" => Freq::Yearly,
                    _ => return None, // HOURLY etc. are not shown
                })
            }
            "INTERVAL" => rule.interval = v.trim().parse().ok().filter(|n: &i64| *n > 0)?,
            "COUNT" => rule.count = v.trim().parse().ok(),
            "UNTIL" => rule.until = parse_naive(v).map(|n| n.0),
            "BYDAY" => {
                for d in list() {
                    let split = d.len().checked_sub(2)?;
                    let day = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"]
                        .iter()
                        .position(|x| x.eq_ignore_ascii_case(&d[split..]))? as u32;
                    let ord = if split == 0 { 0 } else { d[..split].parse().ok()? };
                    rule.by_day.push((ord, day));
                }
            }
            "BYMONTHDAY" => rule.by_month_day = list().filter_map(|x| x.parse().ok()).collect(),
            "BYMONTH" => rule.by_month = list().filter_map(|x| x.parse().ok()).collect(),
            "BYSETPOS" => rule.by_set_pos = list().filter_map(|x| x.parse().ok()).collect(),
            _ => {}
        }
    }
    rule.freq = freq?;
    Some(rule)
}

fn month_len(year: i32, month: u32) -> u32 {
    crate::date::days_in_month(year, month)
}

/// Days of `year`/`month` matching BYMONTHDAY / BYDAY (or `default_day`).
fn days_in_month_matching(rule: &RRule, year: i32, month: u32, default_day: u32) -> Vec<Date> {
    let len = month_len(year, month);
    let first = Date::new(year, month, 1).expect("valid month");
    let mut days: Vec<Date> = Vec::new();
    if !rule.by_month_day.is_empty() {
        for &d in &rule.by_month_day {
            let day = if d < 0 { len as i32 + d + 1 } else { d };
            if day >= 1 && day as u32 <= len {
                days.push(first.add_days(day as i64 - 1));
            }
        }
    }
    if !rule.by_day.is_empty() {
        let mut by_day = Vec::new();
        for &(ord, wd) in &rule.by_day {
            let all: Vec<Date> =
                (0..len as i64).map(|i| first.add_days(i)).filter(|d| d.weekday() == wd).collect();
            match ord {
                0 => by_day.extend(all),
                n if n > 0 => by_day.extend(all.get(n as usize - 1)),
                n => by_day.extend(all.len().checked_sub((-n) as usize).and_then(|i| all.get(i))),
            }
        }
        // Both given: a day must match both (RFC 5545).
        days = if rule.by_month_day.is_empty() { by_day } else { days.into_iter().filter(|d| by_day.contains(d)).collect() };
    }
    if rule.by_month_day.is_empty() && rule.by_day.is_empty() && default_day <= len {
        days.push(first.add_days(default_day as i64 - 1));
    }
    days.sort();
    days.dedup();
    days
}

fn apply_set_pos(rule: &RRule, mut days: Vec<Date>) -> Vec<Date> {
    if rule.by_set_pos.is_empty() {
        return days;
    }
    days.sort();
    let mut out: Vec<Date> = rule
        .by_set_pos
        .iter()
        .filter_map(|&p| if p > 0 { days.get(p as usize - 1) } else { days.len().checked_sub((-p) as usize).and_then(|i| days.get(i)) })
        .copied()
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Start times of the series up to `limit` (inclusive), honouring COUNT.
fn expand(rule: &RRule, start: Naive, limit: Date) -> Vec<Naive> {
    const MAX_PERIODS: i64 = 20_000;
    const MAX_OCCURRENCES: usize = 5_000;
    let mut out = Vec::new();
    let mut produced = 0usize;
    let base = start.date;
    for k in 0..MAX_PERIODS {
        let step = k * rule.interval;
        let candidates: Vec<Date> = match rule.freq {
            Freq::Daily => {
                let d = base.add_days(step);
                let ok = (rule.by_month.is_empty() || rule.by_month.contains(&d.month))
                    && (rule.by_day.is_empty() || rule.by_day.iter().any(|&(_, wd)| wd == d.weekday()))
                    && (rule.by_month_day.is_empty() || rule.by_month_day.contains(&(d.day as i32)));
                if ok { vec![d] } else { vec![] }
            }
            Freq::Weekly => {
                let monday = base.add_days(-(base.weekday() as i64) + step * 7);
                let days: Vec<u32> = if rule.by_day.is_empty() {
                    vec![base.weekday()]
                } else {
                    rule.by_day.iter().map(|&(_, wd)| wd).collect()
                };
                let mut c: Vec<Date> = days.into_iter().map(|wd| monday.add_days(wd as i64)).collect();
                c.retain(|d| rule.by_month.is_empty() || rule.by_month.contains(&d.month));
                c.sort();
                c
            }
            Freq::Monthly => {
                let m = base.add_months(step);
                if !rule.by_month.is_empty() && !rule.by_month.contains(&m.month) {
                    vec![]
                } else {
                    days_in_month_matching(rule, m.year, m.month, base.day)
                }
            }
            Freq::Yearly => {
                let year = base.year + step as i32;
                let months: Vec<u32> = if rule.by_month.is_empty() { vec![base.month] } else { rule.by_month.clone() };
                let mut c = Vec::new();
                for month in months {
                    if (1..=12).contains(&month) {
                        c.extend(days_in_month_matching(rule, year, month, base.day));
                    }
                }
                c
            }
        };
        let period_start = match rule.freq {
            Freq::Daily => base.add_days(step),
            Freq::Weekly => base.add_days(-(base.weekday() as i64) + step * 7),
            Freq::Monthly => Date::new(base.add_months(step).year, base.add_months(step).month, 1).unwrap(),
            Freq::Yearly => Date::new(base.year + step as i32, 1, 1).unwrap(),
        };
        if period_start > limit {
            break;
        }
        for d in apply_set_pos(rule, candidates) {
            let at = Naive { date: d, secs: start.secs };
            if at < start {
                continue;
            }
            if rule.until.is_some_and(|u| at > u) || rule.count.is_some_and(|c| produced >= c) {
                return out;
            }
            produced += 1;
            if d <= limit {
                out.push(at);
            }
            if out.len() >= MAX_OCCURRENCES {
                return out;
            }
        }
    }
    out
}

// ------------------------------------------------------------------ API

/// Occurrences of the events in `text` that overlap `from..=to` (dates in
/// the events' own zones, with a day of margin each side for zone shifts;
/// the UI trims precisely).
pub fn occurrences(text: &str, from: Date, to: Date) -> Vec<Occurrence> {
    let (from, to) = (from.add_days(-1), to.add_days(1));
    let events = parse_events(text);
    // Changed occurrences of a series, by (uid, original start).
    let overridden: HashSet<(String, Naive)> =
        events.iter().filter_map(|e| e.recurrence_id.map(|r| (e.uid.clone(), r))).collect();

    let mut out = Vec::new();
    for ev in &events {
        let Some(start) = ev.start.clone() else { continue };
        if ev.cancelled {
            continue;
        }
        let duration = match (&ev.end, ev.duration) {
            (Some(end), _) => start.at.secs_until(end.at).max(0),
            (None, Some(d)) => d.max(0),
            (None, None) if start.all_day => 86_400,
            (None, None) => 0,
        };
        let starts: Vec<Naive> = match (ev.recurrence_id, ev.rrule.as_deref().and_then(parse_rrule)) {
            (None, Some(rule)) => expand(&rule, start.at, to)
                .into_iter()
                .filter(|s| !ev.exdates.iter().any(|x| x.date == s.date && (start.all_day || x.secs == s.secs)))
                .filter(|s| !overridden.contains(&(ev.uid.clone(), *s)))
                .collect(),
            _ => vec![start.at],
        };
        for s in starts {
            let e = s.add_secs(duration);
            // Overlaps the range (all-day ends are exclusive).
            if s.date > to || e.date < from || (start.all_day && e.date <= from) {
                continue;
            }
            out.push(Occurrence {
                uid: ev.uid.clone(),
                summary: if ev.summary.trim().is_empty() { "(제목 없음)".into() } else { ev.summary.clone() },
                location: ev.location.clone(),
                description: ev.description.as_ref().map(|d| clip(d, 500)),
                all_day: start.all_day,
                start: s.fmt(start.all_day),
                end: e.fmt(start.all_day),
                tz: start.tz.clone(),
            });
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then_with(|| a.summary.cmp(&b.summary)));
    out
}

/// Whether `text` looks like an iCalendar file at all.
pub fn looks_like_ics(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').trim_start().to_ascii_uppercase().starts_with("BEGIN:VCALENDAR")
}

fn clip(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn cal(events: &str) -> String {
        format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{events}END:VCALENDAR\r\n")
    }

    fn starts(text: &str, from: &str, to: &str) -> Vec<String> {
        occurrences(text, d(from), d(to)).into_iter().map(|o| o.start).collect()
    }

    #[test]
    fn single_events_all_day_timed_and_folding() {
        let text = cal(concat!(
            "BEGIN:VEVENT\r\nUID:a\r\nSUMMARY:회의\\, 기획 \r\n 리뷰\r\nDTSTART;TZID=Asia/Seoul:20261120T090000\r\n",
            "DTEND;TZID=Asia/Seoul:20261120T103000\r\nLOCATION:3층\r\nDESCRIPTION:첫 줄\\n둘째 줄\r\n",
            "BEGIN:VALARM\r\nTRIGGER:-PT15M\r\nDESCRIPTION:알림\r\nEND:VALARM\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:b\r\nSUMMARY:휴가\r\nDTSTART;VALUE=DATE:20261123\r\nDTEND;VALUE=DATE:20261125\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:c\r\nSUMMARY:UTC call\r\nDTSTART:20261121T230000Z\r\nDURATION:PT45M\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:x\r\nSUMMARY:취소됨\r\nSTATUS:CANCELLED\r\nDTSTART:20261121T100000\r\nEND:VEVENT\r\n",
        ));
        let all = occurrences(&text, d("2026-11-16"), d("2026-11-30"));
        assert_eq!(all.len(), 3);
        let a = &all[0];
        assert_eq!((a.summary.as_str(), a.start.as_str(), a.end.as_str()), ("회의, 기획 리뷰", "2026-11-20T09:00:00", "2026-11-20T10:30:00"));
        assert_eq!(a.tz.as_deref(), Some("Asia/Seoul"));
        assert_eq!(a.location.as_deref(), Some("3층"));
        assert_eq!(a.description.as_deref(), Some("첫 줄\n둘째 줄"));
        let c = &all[1];
        assert_eq!((c.start.as_str(), c.end.as_str(), c.tz.as_deref()), ("2026-11-21T23:00:00", "2026-11-21T23:45:00", Some("UTC")));
        let b = &all[2];
        assert!(b.all_day);
        assert_eq!((b.start.as_str(), b.end.as_str(), b.tz.as_deref()), ("2026-11-23", "2026-11-25", None));
        // Out of range.
        assert!(occurrences(&text, d("2026-12-05"), d("2026-12-31")).is_empty());
    }

    #[test]
    fn weekly_with_exdate_and_changed_occurrence() {
        let text = cal(concat!(
            "BEGIN:VEVENT\r\nUID:s\r\nSUMMARY:스탠드업\r\nDTSTART;TZID=Asia/Seoul:20261102T093000\r\nDTEND;TZID=Asia/Seoul:20261102T094500\r\n",
            "RRULE:FREQ=WEEKLY;BYDAY=MO,WE\r\nEXDATE;TZID=Asia/Seoul:20261111T093000\r\nEND:VEVENT\r\n",
            "BEGIN:VEVENT\r\nUID:s\r\nRECURRENCE-ID;TZID=Asia/Seoul:20261116T093000\r\nSUMMARY:스탠드업 (늦게)\r\n",
            "DTSTART;TZID=Asia/Seoul:20261116T110000\r\nDTEND;TZID=Asia/Seoul:20261116T111500\r\nEND:VEVENT\r\n",
        ));
        // Mon 11/9, (Wed 11/11 excluded), Mon 11/16 moved to 11:00, Wed 11/18.
        let all = occurrences(&text, d("2026-11-09"), d("2026-11-19"));
        let got: Vec<(&str, &str)> = all.iter().map(|o| (o.start.as_str(), o.summary.as_str())).collect();
        assert_eq!(
            got,
            [
                ("2026-11-09T09:30:00", "스탠드업"),
                ("2026-11-16T11:00:00", "스탠드업 (늦게)"),
                ("2026-11-18T09:30:00", "스탠드업"),
            ]
        );
    }

    #[test]
    fn rrule_patterns() {
        let ev = |start: &str, rule: &str| {
            cal(&format!("BEGIN:VEVENT\r\nUID:r\r\nSUMMARY:x\r\nDTSTART:{start}\r\nRRULE:{rule}\r\nEND:VEVENT\r\n"))
        };
        // Daily, every 2 days, 3 times in total.
        assert_eq!(
            starts(&ev("20261101T080000", "FREQ=DAILY;INTERVAL=2;COUNT=3"), "2026-11-01", "2026-11-30"),
            ["2026-11-01T08:00:00", "2026-11-03T08:00:00", "2026-11-05T08:00:00"]
        );
        // Monthly on the 2nd Tuesday until the end of the year.
        assert_eq!(
            starts(&ev("20261110T190000", "FREQ=MONTHLY;BYDAY=2TU;UNTIL=20261231T235959Z"), "2026-11-01", "2027-02-28"),
            ["2026-11-10T19:00:00", "2026-12-08T19:00:00"]
        );
        // Last weekday of the month.
        assert_eq!(
            starts(&ev("20261130", "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1"), "2026-11-01", "2027-01-31"),
            ["2026-11-30", "2026-12-31", "2027-01-29"]
        );
        // Monthly on the 31st skips short months; yearly birthdays.
        // (The range gets a day of margin each side for zone shifts.)
        assert_eq!(starts(&ev("20260131", "FREQ=MONTHLY"), "2026-02-02", "2026-04-29"), ["2026-03-31"]);
        assert_eq!(starts(&ev("19900515", "FREQ=YEARLY"), "2026-01-01", "2027-12-31"), ["2026-05-15", "2027-05-15"]);
        // Every other week on Friday.
        assert_eq!(
            starts(&ev("20261106T180000", "FREQ=WEEKLY;INTERVAL=2;BYDAY=FR"), "2026-11-01", "2026-11-30"),
            ["2026-11-06T18:00:00", "2026-11-20T18:00:00"]
        );
        // Unsupported frequency: shown once, at its start.
        assert_eq!(starts(&ev("20261101T080000", "FREQ=HOURLY"), "2026-11-01", "2026-11-30"), ["2026-11-01T08:00:00"]);
    }

    #[test]
    fn zones_and_durations() {
        assert_eq!(normalize_tz("Korea Standard Time"), "Asia/Seoul");
        assert_eq!(normalize_tz("/mozilla.org/20050126_1/Asia/Seoul"), "Asia/Seoul");
        assert_eq!(normalize_tz("Europe/Paris"), "Europe/Paris");
        assert_eq!(parse_duration("PT1H30M"), Some(5400));
        assert_eq!(parse_duration("P1W"), Some(604_800));
        assert_eq!(parse_duration("P1DT2H"), Some(93_600));
        assert_eq!(parse_duration("-PT15M"), Some(-900));
        assert_eq!(parse_duration("1H"), None);
        assert!(looks_like_ics("\u{feff}BEGIN:VCALENDAR\r\n"));
        assert!(!looks_like_ics("<html>"));
    }
}

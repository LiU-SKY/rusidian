//! Data for the calendar view: which days have a daily note, and which
//! tasks fall on the days shown.

use crate::daily::{DailyConfig, DailyMatcher};
use crate::date::Date;
use crate::index::{TaskItem, VaultIndex};
use serde::Serialize;

/// Upper bound on the days one request may cover (a month grid is 42).
pub const MAX_DAYS: i64 = 62;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CalendarDay {
    pub date: String,
    /// Path of that day's daily note, if it exists.
    pub note: Option<String>,
}

/// A task placed on a day it doesn't name itself (see `Calendar::daily_tasks`).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DatedTask {
    pub date: String,
    #[serde(flatten)]
    pub item: TaskItem,
}

/// A task to notify about at `at`.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Reminder {
    /// `YYYY-MM-DD HH:MM`, local time.
    pub at: String,
    #[serde(flatten)]
    pub item: TaskItem,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Calendar {
    pub days: Vec<CalendarDay>,
    /// Tasks with a due/scheduled/start/done date inside the range.
    pub tasks: Vec<TaskItem>,
    /// Timed tasks without dates in the daily notes of the range (Day
    /// Planner style: `- [ ] 09:00 회의` in today's note), on that note's day.
    pub daily_tasks: Vec<DatedTask>,
    /// Set when the daily-note settings can't be used (unsupported date
    /// format); `days[].note` is then always empty.
    pub daily_error: Option<String>,
}

pub fn build(index: &VaultIndex, config: &DailyConfig, start: Date, end: Date) -> Result<Calendar, String> {
    let span = start.days_until(end);
    if span < 0 {
        return Err("캘린더 범위가 잘못되었습니다.".into());
    }
    if span >= MAX_DAYS {
        return Err(format!("캘린더 범위는 최대 {MAX_DAYS}일입니다."));
    }

    let mut daily_error = None;
    let days = (0..=span)
        .map(|i| {
            let date = start.add_days(i);
            let note = match config.note_path(date) {
                Ok(path) => index.file_path(&path).map(str::to_string),
                Err(e) => {
                    daily_error.get_or_insert(e);
                    None
                }
            };
            CalendarDay { date: date.to_string(), note }
        })
        .collect();

    let (from, to) = (start.to_string(), end.to_string());
    let matcher = config.matcher();
    let daily_tasks = index
        .tasks(true)
        .into_iter()
        .filter(|t| t.task.time.is_some() && own_day(&t.task).is_none())
        .filter_map(|item| {
            let date = matcher.as_ref()?.date_of(&item.path)?.to_string();
            (date >= from && date <= to).then_some(DatedTask { date, item })
        })
        .collect();

    Ok(Calendar {
        days,
        tasks: index.tasks_between(&from, &to),
        daily_tasks,
        daily_error,
    })
}

/// The day a task names: due, else scheduled, else start.
fn own_day(t: &crate::tasks::Task) -> Option<&str> {
    t.due.as_deref().or(t.scheduled.as_deref()).or(t.start.as_deref())
}

/// Open tasks to notify about between `from` and `to` (`YYYY-MM-DD HH:MM`,
/// inclusive): an explicit reminder, or else the start time of a timed task
/// on its day (its own date, or its daily note's).
pub fn reminders(index: &VaultIndex, matcher: Option<&DailyMatcher>, from: &str, to: &str) -> Vec<Reminder> {
    let mut out: Vec<Reminder> = index
        .tasks(false)
        .into_iter()
        .filter_map(|item| {
            let at = match (&item.task.reminder, &item.task.time) {
                (Some(r), _) => r.clone(),
                (None, Some(time)) => {
                    let day = match own_day(&item.task) {
                        Some(d) => d.to_string(),
                        None => matcher?.date_of(&item.path)?.to_string(),
                    };
                    format!("{day} {time}")
                }
                (None, None) => return None,
            };
            (at.as_str() >= from && at.as_str() <= to).then_some(Reminder { at, item })
        })
        .collect();
    out.sort_by(|a, b| a.at.cmp(&b.at));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    fn vault(files: &[(&str, &str)]) -> (tempfile::TempDir, VaultIndex) {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        let index = VaultIndex::build(dir.path()).unwrap();
        (dir, index)
    }

    #[test]
    fn marks_daily_notes_and_collects_tasks_in_range() {
        let (_d, idx) = vault(&[
            ("Daily/2026-11-02.md", "- [ ] in note, no date"),
            ("Daily/2026-11-30.md", ""),
            ("daily/2026-12-01.md", ""), // outside the range
            ("Tasks.md", concat!(
                "- [ ] due inside 📅 2026-11-10\n",
                "- [ ] scheduled inside ⏳ 2026-11-11\n",
                "- [x] done inside ✅ 2026-11-12\n",
                "- [ ] outside 📅 2026-12-05\n",
                "- [ ] no date\n",
            )),
        ]);
        let config = DailyConfig { folder: "Daily".into(), ..Default::default() };
        let cal = build(&idx, &config, d("2026-11-01"), d("2026-11-30")).unwrap();

        assert_eq!(cal.days.len(), 30);
        assert_eq!(cal.days[0].date, "2026-11-01");
        assert_eq!(cal.days[0].note, None);
        assert_eq!(cal.days[1].note.as_deref(), Some("Daily/2026-11-02.md"));
        assert_eq!(cal.days[29].note.as_deref(), Some("Daily/2026-11-30.md"));
        assert_eq!(cal.daily_error, None);

        let texts: Vec<_> = cal.tasks.iter().map(|t| t.task.text.as_str()).collect();
        assert_eq!(texts, ["due inside", "scheduled inside", "done inside"]);
    }

    #[test]
    fn timed_tasks_in_daily_notes_and_reminders() {
        let (_d, idx) = vault(&[
            ("Daily/2026-11-13.md", "- [ ] 09:00 - 09:30 스탠드업\n- [ ] 날짜 없음\n- [ ] 14:00 외부 📅 2026-11-20\n"),
            ("Tasks.md", "- [ ] 전화 ⏰ 2026-11-13 10:15\n- [x] 끝남 ⏰ 2026-11-13 10:20\n- [ ] 13:00 점심 ⏳ 2026-11-13\n"),
        ]);
        let config = DailyConfig { folder: "Daily".into(), ..Default::default() };
        let cal = build(&idx, &config, d("2026-11-09"), d("2026-11-15")).unwrap();
        let daily: Vec<_> = cal.daily_tasks.iter().map(|t| (t.date.as_str(), t.item.task.text.as_str())).collect();
        assert_eq!(daily, [("2026-11-13", "스탠드업")]);

        let matcher = config.matcher();
        let at = |from: &str, to: &str| {
            reminders(&idx, matcher.as_ref(), from, to).into_iter().map(|r| r.at).collect::<Vec<_>>()
        };
        assert_eq!(at("2026-11-13 00:00", "2026-11-13 23:59"), ["2026-11-13 09:00", "2026-11-13 10:15", "2026-11-13 13:00"]);
        assert_eq!(at("2026-11-13 10:00", "2026-11-13 10:30"), ["2026-11-13 10:15"]);
        assert_eq!(at("2026-11-20 00:00", "2026-11-20 23:59"), ["2026-11-20 14:00"]);
    }

    #[test]
    fn note_lookup_is_case_insensitive() {
        let (_d, idx) = vault(&[("journal/2026-11-13.md", "")]);
        let config = DailyConfig { folder: "Journal".into(), ..Default::default() };
        let cal = build(&idx, &config, d("2026-11-13"), d("2026-11-13")).unwrap();
        assert_eq!(cal.days[0].note.as_deref(), Some("journal/2026-11-13.md"));
    }

    #[test]
    fn unsupported_format_is_reported_not_fatal() {
        let (_d, idx) = vault(&[("T.md", "- [ ] x 📅 2026-11-13")]);
        let config = DailyConfig { format: "YYYY-MM-DD HH".into(), ..Default::default() };
        let cal = build(&idx, &config, d("2026-11-13"), d("2026-11-14")).unwrap();
        assert!(cal.daily_error.is_some());
        assert!(cal.days.iter().all(|day| day.note.is_none()));
        assert_eq!(cal.tasks.len(), 1);
    }

    #[test]
    fn rejects_bad_ranges() {
        let (_d, idx) = vault(&[]);
        let config = DailyConfig::default();
        assert!(build(&idx, &config, d("2026-11-13"), d("2026-11-12")).is_err());
        assert!(build(&idx, &config, d("2026-01-01"), d("2026-12-31")).is_err());
        assert!(build(&idx, &config, d("2026-11-01"), d("2026-12-12")).is_ok()); // 42-day grid
    }
}

//! Tasks, compatible with the Obsidian Tasks plugin (emoji format).
//!
//! `- [ ] Pay rent 🔁 every month 📅 2026-12-01 ⏫ #home`
//!
//! Completing a task follows the plugin: the status becomes `x`, a done date
//! `✅ YYYY-MM-DD` is appended, and for recurring tasks the next occurrence
//! is inserted on the line above with its dates moved forward.

use crate::date::Date;
use crate::markdown::{checked_line, prose_lines, replace_line, split_lines};
use crate::recur;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusKind {
    Todo,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Highest,
    High,
    Medium,
    Normal,
    Low,
    Lowest,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Task {
    /// 0-based line in the note.
    pub line: usize,
    /// The whole line as written; used to check nothing changed before editing.
    pub raw: String,
    /// The character between the brackets.
    pub status: char,
    pub kind: StatusKind,
    /// Description without dates, priority, recurrence, ids (tags are kept).
    pub text: String,
    pub due: Option<String>,
    pub scheduled: Option<String>,
    pub start: Option<String>,
    pub created: Option<String>,
    pub done: Option<String>,
    pub cancelled: Option<String>,
    pub priority: Priority,
    pub recurrence: Option<String>,
    pub tags: Vec<String>,
    /// Start time `HH:MM` written at the start of the description (Day Planner style).
    pub time: Option<String>,
    /// End time `HH:MM` of a `09:00 - 10:30` range.
    pub end_time: Option<String>,
    /// Reminder `YYYY-MM-DD HH:MM` (`⏰ …` or `(@…)`, Reminder plugin).
    pub reminder: Option<String>,
}

const D: &str = r"(\d{4}-\d{2}-\d{2})";
const VS: &str = r"\x{FE0F}?"; // optional emoji variation selector

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// A date field in emoji form (`📅 2026-11-20`, group 1) or the Tasks
/// plugin's Dataview form (`[due:: 2026-11-20]`, group 2).
fn date_field(emoji: &str, key: &str) -> Regex {
    re(&format!(r"(?:(?:{emoji}){VS}\s*{D}|[\[(](?i:{key})::\s*{D}\s*[\])])"))
}

/// The first non-empty capture group.
fn group<'a>(c: &regex::Captures<'a>) -> Option<regex::Match<'a>> {
    c.iter().skip(1).flatten().next()
}

/// prefix (indent / blockquote) | list marker | status char | body
static TASK_LINE: LazyLock<Regex> =
    LazyLock::new(|| re(r"^((?:[ \t]*>)*[ \t]*)([-*+]|\d+[.)])[ \t]+\[(.)\](?:[ \t]+(.*))?$"));
static DUE: LazyLock<Regex> = LazyLock::new(|| date_field("📅|📆|🗓", "due"));
static SCHEDULED: LazyLock<Regex> = LazyLock::new(|| date_field("⏳|⌛", "scheduled"));
static START: LazyLock<Regex> = LazyLock::new(|| date_field("🛫", "start"));
static CREATED: LazyLock<Regex> = LazyLock::new(|| date_field("➕", "created"));
static DONE: LazyLock<Regex> = LazyLock::new(|| date_field("✅", "completion"));
static CANCELLED: LazyLock<Regex> = LazyLock::new(|| date_field("❌", "cancelled"));
static RECURRENCE: LazyLock<Regex> =
    LazyLock::new(|| re(&format!(r"(?:🔁{VS}\s*([a-zA-Z0-9, ]+)|[\[(](?i:repeat)::\s*([^\]\)\n]+?)\s*[\])])")));
static PRIORITY: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(r"(?:(🔺|⏫|🔼|🔽|⏬){VS}|[\[(](?i:priority)::\s*(?i:(highest|high|medium|low|lowest))\s*[\])])"))
});
static ID: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(
        r"(?:🆔{VS}\s*[a-zA-Z0-9_-]+|⛔{VS}\s*[a-zA-Z0-9_,-]+|🏁{VS}\s*[a-zA-Z]+|[\[(](?i:id|dependsOn|onCompletion)::[^\])\n]*[\])])"
    ))
});
/// Reminder plugin: `⏰ 2026-11-20 09:00` (groups 1, 2) or `(@2026-11-20 09:00)` (groups 3, 4).
static REMINDER: LazyLock<Regex> = LazyLock::new(|| {
    re(&format!(r"(?:⏰{VS}\s*{D}\s+(\d{{1,2}}:\d{{2}})|\(@{D}(?:\s+(\d{{1,2}}:\d{{2}}))?\))"))
});
/// Day Planner style time at the start of the description: `09:30 - 10:00 …`.
static TIME: LazyLock<Regex> =
    LazyLock::new(|| re(r"^(\d{1,2}):(\d{2})(?:\s*[-–~]\s*(\d{1,2}):(\d{2}))?(?:\s+|$)"));
/// Any Tasks-plugin Dataview field: the line is written in that format.
static DATAVIEW: LazyLock<Regex> = LazyLock::new(|| {
    re(r"[\[(](?i:due|scheduled|start|created|completion|cancelled|priority|repeat|id|dependsOn|onCompletion)::")
});
static BLOCK_ID: LazyLock<Regex> = LazyLock::new(|| re(r"\s\^[a-zA-Z0-9-]+\s*$"));
static TAG: LazyLock<Regex> = LazyLock::new(|| re(r"(?:^|\s)(#[\p{L}\p{N}_/-]+)"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| re(r"\s{2,}"));

/// `HH:MM` if valid (hours 0–23).
fn hhmm(h: &str, m: &str) -> Option<String> {
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then(|| format!("{h:02}:{m:02}"))
}

/// All tasks in a note, skipping fenced code blocks.
pub fn parse_tasks(text: &str) -> Vec<Task> {
    prose_lines(text).filter_map(|(n, line)| parse_line(n, line)).collect()
}

pub fn parse_line(line_no: usize, line: &str) -> Option<Task> {
    let caps = TASK_LINE.captures(line)?;
    let status = caps[3].chars().next()?;
    let body = caps.get(4).map(|m| m.as_str()).unwrap_or("");

    let date = |r: &Regex| r.captures(body).and_then(|c| group(&c).map(|m| m.as_str().to_string()));
    let priority = match PRIORITY.captures(body).and_then(|c| group(&c).map(|m| m.as_str().to_lowercase())).as_deref() {
        Some("🔺" | "highest") => Priority::Highest,
        Some("⏫" | "high") => Priority::High,
        Some("🔼" | "medium") => Priority::Medium,
        Some("🔽" | "low") => Priority::Low,
        Some("⏬" | "lowest") => Priority::Lowest,
        _ => Priority::Normal,
    };
    let reminder = REMINDER.captures(body).map(|c| {
        let (day, time) = match c.get(1) {
            Some(day) => (day.as_str(), c.get(2).map(|m| m.as_str())),
            None => (&c[3], c.get(4).map(|m| m.as_str())),
        };
        let time = time.and_then(|t| t.split_once(':')).and_then(|(h, m)| hhmm(h, m));
        format!("{day} {}", time.unwrap_or_else(|| "09:00".into()))
    });

    let mut text = body.to_string();
    for r in [&*DUE, &*SCHEDULED, &*START, &*CREATED, &*DONE, &*CANCELLED, &*RECURRENCE, &*PRIORITY, &*ID, &*REMINDER] {
        text = r.replace_all(&text, " ").into_owned();
    }
    text = BLOCK_ID.replace(&text, "").into_owned();
    let mut text = SPACES.replace_all(text.trim(), " ").into_owned();
    let (mut time, mut end_time) = (None, None);
    if let Some(c) = TIME.captures(&text) {
        if let Some(start) = hhmm(&c[1], &c[2]) {
            time = Some(start);
            end_time = c.get(3).zip(c.get(4)).and_then(|(h, m)| hhmm(h.as_str(), m.as_str()));
            text = text[c.get(0).unwrap().end()..].to_string();
        }
    }
    if text.is_empty() {
        return None;
    }

    let tags = TAG
        .captures_iter(body)
        .map(|c| c[1].to_string())
        .filter(|t| !t[1..].chars().all(|c| c.is_ascii_digit()))
        .collect();

    Some(Task {
        line: line_no,
        raw: line.to_string(),
        status,
        kind: match status {
            'x' | 'X' => StatusKind::Done,
            '-' => StatusKind::Cancelled,
            '/' => StatusKind::InProgress,
            _ => StatusKind::Todo,
        },
        text,
        due: date(&DUE),
        scheduled: date(&SCHEDULED),
        start: date(&START),
        created: date(&CREATED),
        done: date(&DONE),
        cancelled: date(&CANCELLED),
        priority,
        recurrence: RECURRENCE.captures(body).and_then(|c| group(&c).map(|m| m.as_str().trim().to_string())),
        tags,
        time,
        end_time,
        reminder,
    })
}

// ------------------------------------------------------------------ toggle

/// Toggle the task on `line_no` of `text`. `expected` is the line as the UI
/// last saw it; if the file changed since, nothing is modified.
/// Returns the new file text.
pub fn toggle(text: &str, line_no: usize, expected: &str, today: Date) -> Result<String, String> {
    let mut lines = split_lines(text);
    let Some(&(current, ending)) = lines.get(line_no) else {
        return Err("할일이 있던 줄이 사라졌습니다. 목록을 새로고침하세요.".into());
    };
    if current != expected {
        return Err("그사이 파일이 바뀌었습니다. 목록을 새로고침한 뒤 다시 시도하세요.".into());
    }
    let task = parse_line(line_no, current).ok_or("할일 줄이 아닙니다.")?;

    let (new_line, insert_above) = match task.kind {
        StatusKind::Done => (remove_field(&set_status(current, ' '), &DONE), None),
        StatusKind::Cancelled => (remove_field(&set_status(current, ' '), &CANCELLED), None),
        StatusKind::Todo | StatusKind::InProgress => {
            let next = match &task.recurrence {
                Some(rule) => Some(next_occurrence(current, &task, rule, today)?),
                None => None,
            };
            let mut done = set_status(current, 'x');
            if task.done.is_none() {
                let field = if DATAVIEW.is_match(current) { format!("[completion:: {today}]") } else { format!("✅ {today}") };
                done = append_field(&done, &field);
            }
            (done, next)
        }
    };

    let mut out = String::with_capacity(text.len() + 64);
    for (i, (content, end)) in lines.drain(..).enumerate() {
        if i == line_no {
            if let Some(next) = &insert_above {
                out.push_str(next);
                out.push_str(if ending.is_empty() { "\n" } else { ending });
            }
            out.push_str(&new_line);
        } else {
            out.push_str(content);
        }
        out.push_str(end);
    }
    Ok(out)
}

fn set_status(line: &str, status: char) -> String {
    let caps = TASK_LINE.captures(line).expect("caller checked this is a task line");
    let m = caps.get(3).unwrap();
    format!("{}{}{}", &line[..m.start()], status, &line[m.end()..])
}

fn remove_field(line: &str, field: &Regex) -> String {
    let Some(m) = field.find(line) else { return line.to_string() };
    let before = line[..m.start()].trim_end_matches([' ', '\t']);
    format!("{before}{}", &line[m.end()..])
}

/// Add ` field` at the end, but before a trailing block id (`^abc`).
fn append_field(line: &str, field: &str) -> String {
    match BLOCK_ID.find(line) {
        Some(m) => format!("{} {field}{}", line[..m.start()].trim_end(), &line[m.start()..]),
        None => format!("{} {field}", line.trim_end()),
    }
}

/// Replace the date inside every match of `field` using `f`.
fn map_dates(line: &str, field: &Regex, f: impl Fn(Date) -> Date) -> String {
    field
        .replace_all(line, |c: &regex::Captures| {
            let whole = &c[0];
            match c.iter().skip(1).flatten().find(|m| Date::parse(m.as_str()).is_some()) {
                Some(m) => {
                    let moved = f(Date::parse(m.as_str()).unwrap()).to_string();
                    let at = m.start() - c.get(0).unwrap().start();
                    format!("{}{moved}{}", &whole[..at], &whole[at + m.len()..])
                }
                None => whole.to_string(),
            }
        })
        .into_owned()
}

/// The next occurrence of a recurring task as a new line (status `[ ]`).
fn next_occurrence(line: &str, task: &Task, rule_text: &str, today: Date) -> Result<String, String> {
    let rule = recur::parse(rule_text).ok_or_else(|| {
        format!("반복 규칙 \"{rule_text}\"은(는) 아직 지원하지 않습니다. 이 할일은 Obsidian에서 완료해 주세요.")
    })?;

    let mut next = set_status(line, ' ');
    next = remove_field(&next, &DONE);
    next = remove_field(&next, &CANCELLED);
    next = BLOCK_ID.replace(&next, "").into_owned(); // block ids must stay unique

    let reference = [&task.due, &task.scheduled, &task.start]
        .into_iter()
        .flatten()
        .find_map(|d| Date::parse(d));
    if let Some(reference) = reference {
        let base = if rule.when_done { today } else { reference };
        let shift = reference.days_until(rule.next(base));
        for field in [&*DUE, &*SCHEDULED, &*START, &*REMINDER] {
            next = map_dates(&next, field, |d| d.add_days(shift));
        }
    }
    if task.created.is_some() {
        next = map_dates(&next, &CREATED, |_| today);
    }
    Ok(next)
}

// -------------------------------------------------------------- edit / delete

/// The editable parts of a task (the task edit dialog).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct TaskEdit {
    /// Description, tags included, without time or fields.
    pub text: String,
    pub priority: Option<Priority>,
    pub due: Option<String>,
    pub scheduled: Option<String>,
    pub start: Option<String>,
    pub recurrence: Option<String>,
    /// `HH:MM`, written at the start of the description.
    pub time: Option<String>,
    pub end_time: Option<String>,
    /// `YYYY-MM-DD HH:MM`.
    pub reminder: Option<String>,
}

fn clean(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn check_date(v: Option<&str>, what: &str) -> Result<(), String> {
    match v {
        Some(s) if Date::parse(s).is_none() => Err(format!("{what} 날짜가 잘못되었습니다: {s}")),
        _ => Ok(()),
    }
}

fn check_time(v: Option<&str>, what: &str) -> Result<Option<String>, String> {
    v.map(|s| s.split_once(':').and_then(|(h, m)| hhmm(h, m)).ok_or_else(|| format!("{what} 시간이 잘못되었습니다: {s}")))
        .transpose()
}

/// Rebuild `line` (a task) from `edit`, keeping its list prefix, status,
/// created/done/cancelled dates, ids and block id, in the line's own
/// format (emoji, or Dataview `[key:: value]` fields).
pub fn render_edit(line: &str, edit: &TaskEdit) -> Result<String, String> {
    let caps = TASK_LINE.captures(line).ok_or("할일 줄이 아닙니다.")?;
    let task = parse_line(0, line).ok_or("할일 줄이 아닙니다.")?;
    let body = caps.get(4).map(|m| m.as_str()).unwrap_or("");
    let head = &line[..caps.get(3).unwrap().end() + 1]; // through "]"

    let text = SPACES.replace_all(edit.text.trim(), " ").into_owned();
    if text.is_empty() || text.contains('\n') {
        return Err("할일 내용을 입력하세요.".into());
    }
    let (due, scheduled, start) = (clean(&edit.due), clean(&edit.scheduled), clean(&edit.start));
    check_date(due, "마감")?;
    check_date(scheduled, "예정")?;
    check_date(start, "시작")?;
    let recurrence = clean(&edit.recurrence);
    if let Some(r) = recurrence {
        if recur::parse(r).is_none() {
            return Err(format!("반복 규칙 \"{r}\"을(를) 이해하지 못했습니다. 예: every week, every weekday, every month on the 15th"));
        }
    }
    let time = check_time(clean(&edit.time), "시작")?;
    let end_time = match time {
        Some(_) => check_time(clean(&edit.end_time), "끝")?,
        None => None,
    };
    let reminder = match clean(&edit.reminder) {
        None => None,
        Some(r) => {
            let (day, t) = r.split_once([' ', 'T']).ok_or_else(|| format!("알림 형식이 잘못되었습니다: {r}"))?;
            check_date(Some(day), "알림")?;
            Some(format!("{day} {}", check_time(Some(t), "알림")?.unwrap_or_default()))
        }
    };

    let dataview = DATAVIEW.is_match(body);
    let field = |emoji: &str, key: &str, value: &str| {
        if dataview { format!("[{key}:: {value}]") } else { format!("{emoji} {value}") }
    };
    let mut parts: Vec<String> = Vec::new();
    match (&time, &end_time) {
        (Some(t), Some(e)) => parts.push(format!("{t} - {e} {text}")),
        (Some(t), None) => parts.push(format!("{t} {text}")),
        _ => parts.push(text),
    }
    let priority = edit.priority.unwrap_or(task.priority);
    let prio = match priority {
        Priority::Highest => Some(("🔺", "highest")),
        Priority::High => Some(("⏫", "high")),
        Priority::Medium => Some(("🔼", "medium")),
        Priority::Normal => None,
        Priority::Low => Some(("🔽", "low")),
        Priority::Lowest => Some(("⏬", "lowest")),
    };
    if let Some((emoji, word)) = prio {
        parts.push(if dataview { format!("[priority:: {word}]") } else { emoji.to_string() });
    }
    if let Some(r) = recurrence {
        parts.push(field("🔁", "repeat", r));
    }
    if let Some(d) = &task.created {
        parts.push(field("➕", "created", d));
    }
    if let Some(d) = start {
        parts.push(field("🛫", "start", d));
    }
    if let Some(d) = scheduled {
        parts.push(field("⏳", "scheduled", d));
    }
    if let Some(d) = due {
        parts.push(field("📅", "due", d));
    }
    if let Some(r) = reminder {
        // Keep the Reminder plugin style the line already used.
        parts.push(if body.contains("(@") { format!("(@{r})") } else { format!("⏰ {r}") });
    }
    if let Some(d) = &task.cancelled {
        parts.push(field("❌", "cancelled", d));
    }
    if let Some(d) = &task.done {
        parts.push(field("✅", "completion", d));
    }
    parts.extend(ID.find_iter(body).map(|m| m.as_str().to_string()));
    let mut out = format!("{head} {}", parts.join(" "));
    if let Some(m) = BLOCK_ID.find(line) {
        out.push(' ');
        out.push_str(m.as_str().trim());
    }
    Ok(out)
}

/// `text` with the task on `line_no` rewritten from `edit`.
pub fn edit(text: &str, line_no: usize, expected: &str, edit: &TaskEdit) -> Result<String, String> {
    let current = checked_line(&split_lines(text), line_no, expected)?;
    replace_line(text, line_no, expected, Some(&render_edit(current, edit)?))
}

/// `text` without the task on `line_no`.
pub fn delete(text: &str, line_no: usize, expected: &str) -> Result<String, String> {
    let current = checked_line(&split_lines(text), line_no, expected)?;
    parse_line(line_no, current).ok_or("할일 줄이 아닙니다.")?;
    replace_line(text, line_no, expected, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn today() -> Date {
        Date::parse("2026-11-13").unwrap()
    }

    fn one(line: &str) -> Task {
        parse_line(0, line).unwrap_or_else(|| panic!("not a task: {line}"))
    }

    #[test]
    fn parses_all_fields() {
        let t = one("- [ ] 월세 내기 #집 🔁 every month ⏫ ➕ 2026-11-01 🛫 2026-11-20 ⏳ 2026-11-25 📅 2026-12-01 🆔 abc ^blk1");
        assert_eq!(t.text, "월세 내기 #집");
        assert_eq!(t.kind, StatusKind::Todo);
        assert_eq!(t.priority, Priority::High);
        assert_eq!(t.recurrence.as_deref(), Some("every month"));
        assert_eq!(t.created.as_deref(), Some("2026-11-01"));
        assert_eq!(t.start.as_deref(), Some("2026-11-20"));
        assert_eq!(t.scheduled.as_deref(), Some("2026-11-25"));
        assert_eq!(t.due.as_deref(), Some("2026-12-01"));
        assert_eq!(t.tags, ["#집"]);
    }

    #[test]
    fn statuses_and_list_forms() {
        assert_eq!(one("- [x] a ✅ 2026-11-01").kind, StatusKind::Done);
        assert_eq!(one("- [X] a").kind, StatusKind::Done);
        assert_eq!(one("* [/] a").kind, StatusKind::InProgress);
        assert_eq!(one("+ [-] a ❌ 2026-11-02").cancelled.as_deref(), Some("2026-11-02"));
        assert_eq!(one("1. [ ] numbered").text, "numbered");
        assert_eq!(one("    - [ ] nested").text, "nested");
        assert_eq!(one("> - [ ] quoted").text, "quoted");
        assert_eq!(one("- [?] custom").kind, StatusKind::Todo);
        assert_eq!(one("- [ ] 📆 2026-11-20 alt calendar emoji").due.as_deref(), Some("2026-11-20"));
        assert_eq!(one("- [ ] variation ⏳\u{FE0F} 2026-11-21").scheduled.as_deref(), Some("2026-11-21"));
        assert_eq!(one("- [ ] #2026 is not a tag #real").tags, ["#real"]);
    }

    #[test]
    fn non_tasks() {
        for line in ["- [ ]", "- [ ]   ", "-[ ] no space", "- [] empty", "text [ ] here", "- item", "[ ] bare"] {
            assert!(parse_line(0, line).is_none(), "{line:?}");
        }
        let text = "- [ ] real\n```\n- [ ] in code\n```\n- [x] also real";
        let tasks = parse_tasks(text);
        assert_eq!(tasks.iter().map(|t| t.line).collect::<Vec<_>>(), [0, 4]);
    }

    #[test]
    fn complete_and_reopen() {
        let text = "# Todo\n- [ ] 장보기 📅 2026-11-14\n- [ ] other\n";
        let done = toggle(text, 1, "- [ ] 장보기 📅 2026-11-14", today()).unwrap();
        assert_eq!(done, "# Todo\n- [x] 장보기 📅 2026-11-14 ✅ 2026-11-13\n- [ ] other\n");

        let reopened = toggle(&done, 1, "- [x] 장보기 📅 2026-11-14 ✅ 2026-11-13", today()).unwrap();
        assert_eq!(reopened, text);
    }

    #[test]
    fn keeps_crlf_and_block_ids() {
        let text = "- [ ] a ^id-1\r\n- [ ] b\r\n";
        let out = toggle(text, 0, "- [ ] a ^id-1", today()).unwrap();
        assert_eq!(out, "- [x] a ✅ 2026-11-13 ^id-1\r\n- [ ] b\r\n");
        // last line without a trailing newline
        assert_eq!(toggle("- [ ] z", 0, "- [ ] z", today()).unwrap(), "- [x] z ✅ 2026-11-13");
    }

    #[test]
    fn cancelled_reopens() {
        let out = toggle("- [-] a ❌ 2026-11-01\n", 0, "- [-] a ❌ 2026-11-01", today()).unwrap();
        assert_eq!(out, "- [ ] a\n");
    }

    #[test]
    fn refuses_when_line_changed() {
        let text = "- [ ] edited elsewhere\n";
        assert!(toggle(text, 0, "- [ ] original", today()).is_err());
        assert!(toggle(text, 5, "- [ ] edited elsewhere", today()).is_err());
        assert!(toggle("plain text\n", 0, "plain text", today()).is_err());
    }

    #[test]
    fn recurring_weekly_inserts_next_above() {
        let line = "- [ ] 운동 🔁 every week 📅 2026-11-13 ^gym";
        let out = toggle(&format!("{line}\n"), 0, line, today()).unwrap();
        assert_eq!(
            out,
            "- [ ] 운동 🔁 every week 📅 2026-11-20\n- [x] 운동 🔁 every week 📅 2026-11-13 ✅ 2026-11-13 ^gym\n"
        );
    }

    #[test]
    fn recurring_monthly_clamps_and_shifts_all_dates() {
        let line = "  - [ ] 카드값 🔁 every month 🛫 2026-01-25 ⏳ 2026-01-28 📅 2026-01-31 ➕ 2026-01-01";
        let out = toggle(line, 0, line, today()).unwrap();
        let next = out.lines().next().unwrap();
        // due is the reference: Jan 31 + 1 month = Feb 28; others move by the same 28 days
        assert_eq!(
            next,
            "  - [ ] 카드값 🔁 every month 🛫 2026-02-22 ⏳ 2026-02-25 📅 2026-02-28 ➕ 2026-11-13"
        );
    }

    #[test]
    fn recurring_when_done_uses_today() {
        let line = "- [ ] 물주기 🔁 every 3 days when done 📅 2026-11-01";
        let out = toggle(line, 0, line, today()).unwrap();
        assert!(out.starts_with("- [ ] 물주기 🔁 every 3 days when done 📅 2026-11-16\n"), "{out}");
    }

    #[test]
    fn unsupported_recurrence_is_refused() {
        let line = "- [ ] 회의 🔁 every fortnight 📅 2026-11-16";
        assert!(toggle(line, 0, line, today()).is_err());
    }

    #[test]
    fn recurring_on_weekdays() {
        // 2026-11-13 is a Friday: the next weekday is Monday.
        let line = "- [ ] 출근 🔁 every weekday 📅 2026-11-13 ⏰ 2026-11-13 08:30";
        let out = toggle(line, 0, line, today()).unwrap();
        assert!(out.starts_with("- [ ] 출근 🔁 every weekday 📅 2026-11-16 ⏰ 2026-11-16 08:30\n"), "{out}");
        let line = "- [ ] 회의 🔁 every week on Monday 📅 2026-11-16";
        assert!(toggle(line, 0, line, today()).unwrap().starts_with("- [ ] 회의 🔁 every week on Monday 📅 2026-11-23\n"));
    }

    #[test]
    fn dataview_format() {
        let line = "- [ ] 보고서 [priority:: high] [repeat:: every month on the 1st] [due:: 2026-12-01] [id:: r1]";
        let t = one(line);
        assert_eq!(t.text, "보고서");
        assert_eq!(t.priority, Priority::High);
        assert_eq!(t.recurrence.as_deref(), Some("every month on the 1st"));
        assert_eq!(t.due.as_deref(), Some("2026-12-01"));
        let out = toggle(line, 0, line, today()).unwrap();
        assert_eq!(
            out,
            "- [ ] 보고서 [priority:: high] [repeat:: every month on the 1st] [due:: 2027-01-01] [id:: r1]\n\
             - [x] 보고서 [priority:: high] [repeat:: every month on the 1st] [due:: 2026-12-01] [id:: r1] [completion:: 2026-11-13]"
        );
        assert_eq!(one("- [x] a (completion:: 2026-11-02)").done.as_deref(), Some("2026-11-02"));
    }

    #[test]
    fn times_and_reminders() {
        let t = one("- [ ] 09:30 - 10:00 스탠드업 #work 📅 2026-11-16");
        assert_eq!((t.time.as_deref(), t.end_time.as_deref()), (Some("09:30"), Some("10:00")));
        assert_eq!(t.text, "스탠드업 #work");
        assert_eq!(one("- [ ] 7:05 약 먹기").time.as_deref(), Some("07:05"));
        assert_eq!(one("- [ ] 25:00 not a time").time, None);
        assert_eq!(one("- [ ] 전화 ⏰ 2026-11-20 14:00").reminder.as_deref(), Some("2026-11-20 14:00"));
        let t = one("- [ ] 전화 (@2026-11-20 9:15)");
        assert_eq!(t.reminder.as_deref(), Some("2026-11-20 09:15"));
        assert_eq!(t.text, "전화");
        assert_eq!(one("- [ ] 날짜만 (@2026-11-20)").reminder.as_deref(), Some("2026-11-20 09:00"));
    }

    fn edit_of(t: &Task) -> TaskEdit {
        TaskEdit {
            text: t.text.clone(),
            priority: Some(t.priority),
            due: t.due.clone(),
            scheduled: t.scheduled.clone(),
            start: t.start.clone(),
            recurrence: t.recurrence.clone(),
            time: t.time.clone(),
            end_time: t.end_time.clone(),
            reminder: t.reminder.clone(),
        }
    }

    #[test]
    fn edit_rewrites_in_canonical_order() {
        let line = "  - [/] 📅 2026-11-20 장보기 #집 ➕ 2026-11-01 🆔 a1 ^blk";
        let mut e = edit_of(&one(line));
        e.text = "마트 장보기 #집".into();
        e.priority = Some(Priority::High);
        e.scheduled = Some("2026-11-19".into());
        e.time = Some("18:00".into());
        e.reminder = Some("2026-11-19T17:30".into());
        assert_eq!(
            render_edit(line, &e).unwrap(),
            "  - [/] 18:00 마트 장보기 #집 ⏫ ➕ 2026-11-01 ⏳ 2026-11-19 📅 2026-11-20 ⏰ 2026-11-19 17:30 🆔 a1 ^blk"
        );
        // An unchanged edit of a canonical line is a no-op.
        let canonical = "- [x] 운동 🔁 every week 🛫 2026-11-01 📅 2026-11-13 ✅ 2026-11-13";
        assert_eq!(render_edit(canonical, &edit_of(&one(canonical))).unwrap(), canonical);
        // Dataview lines stay Dataview.
        let dv = "- [ ] a [due:: 2026-11-20]";
        let mut e = edit_of(&one(dv));
        e.recurrence = Some("every day".into());
        assert_eq!(render_edit(dv, &e).unwrap(), "- [ ] a [repeat:: every day] [due:: 2026-11-20]");
    }

    #[test]
    fn edit_validates() {
        let line = "- [ ] a";
        let base = edit_of(&one(line));
        for bad in [
            TaskEdit { text: "  ".into(), ..base.clone() },
            TaskEdit { due: Some("2026-13-01".into()), ..base.clone() },
            TaskEdit { recurrence: Some("whenever".into()), ..base.clone() },
            TaskEdit { time: Some("9am".into()), ..base.clone() },
            TaskEdit { reminder: Some("tomorrow".into()), ..base.clone() },
        ] {
            assert!(render_edit(line, &bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn edit_and_delete_in_file() {
        let text = "# T\r\n- [ ] a\r\n- [ ] b\r\n";
        let mut e = edit_of(&one("- [ ] a"));
        e.due = Some("2026-11-20".into());
        assert_eq!(edit(text, 1, "- [ ] a", &e).unwrap(), "# T\r\n- [ ] a 📅 2026-11-20\r\n- [ ] b\r\n");
        assert_eq!(delete(text, 1, "- [ ] a").unwrap(), "# T\r\n- [ ] b\r\n");
        assert!(delete(text, 1, "- [ ] changed").is_err());
        assert!(delete(text, 0, "# T").is_err());
    }
}

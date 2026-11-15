//! Daily notes, following Obsidian's core "Daily notes" settings
//! stored in `<vault>/.obsidian/daily-notes.json`:
//! `{ "folder": "Daily", "format": "YYYY-MM-DD", "template": "Templates/Daily" }`.
//!
//! The date format is moment.js's, in the Korean locale (see `moment.rs`):
//! weekday and month names, week numbers, day of the year, `[literal]` text.
//! Tokens a date alone can't fill (the time of day, time zones) are reported
//! instead of guessed, so we never create a daily note under the wrong name.

use crate::date::Date;
use crate::moment::{self, Piece};
use crate::vault;
use regex::Regex;
use serde::Deserialize;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

const DEFAULT_FORMAT: &str = "YYYY-MM-DD";

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct DailyConfig {
    #[serde(default)]
    pub folder: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub template: String,
}

pub fn load_config(root: &Path) -> DailyConfig {
    fs::read_to_string(root.join(".obsidian").join("daily-notes.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Format `date` with a moment.js pattern. Errs when the pattern needs more
/// than a date, pointing at the word to put in `[ ]`.
pub fn format_date(pattern: &str, date: Date) -> Result<String, String> {
    if pattern.matches('[').count() > pattern.matches(']').count() {
        return Err(format!("날짜 형식의 [ ]가 닫히지 않았습니다: {pattern}"));
    }
    let mut out = String::new();
    for p in moment::pieces(pattern) {
        match p {
            Piece::Text(t) => out.push_str(&t),
            Piece::Token(t) => match moment::token_text(&t, date, None) {
                Some(text) => out.push_str(&text),
                None => return Err(unfillable(pattern, &t)),
            },
        }
    }
    Ok(out)
}

fn unfillable(pattern: &str, token: &str) -> String {
    match moment::bad_word(pattern) {
        // A real word (not a token like "HH") whose letters became tokens.
        Some(word) if word.chars().any(|c| !word.starts_with(c)) => format!(
            "데일리 노트 날짜 형식 \"{pattern}\"에서 \"{word}\"의 글자가 날짜·시간 기호로 읽힙니다 (Obsidian도 같음). \
             글자 그대로 쓰려면 Obsidian 설정 → 데일리 노트에서 \"[{word}]\"처럼 대괄호로 감싸세요."
        ),
        _ => format!("데일리 노트 날짜 형식 \"{pattern}\"의 \"{token}\"은(는) 시간이나 시간대라 날짜만으로 채울 수 없습니다."),
    }
}

/// What a capture group of the matcher holds.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Group {
    Year4,
    Year2,
    Month,
    Day,
    DayOfYear,
}

/// Recognises daily-note paths and reads the date back out of them.
pub struct DailyMatcher {
    regex: Regex,
    /// What each capture group holds, in order.
    groups: Vec<Group>,
    config: DailyConfig,
}

impl DailyMatcher {
    /// The date of a vault-relative path if it is a daily note.
    pub fn date_of(&self, path: &str) -> Option<Date> {
        let caps = self.regex.captures(path)?;
        let (mut year, mut month, mut day, mut doy) = (None::<i32>, None::<u32>, None::<u32>, None::<u32>);
        for (i, kind) in self.groups.iter().enumerate() {
            let n: u32 = caps.get(i + 1)?.as_str().parse().ok()?;
            match kind {
                Group::Year4 => year = Some(n as i32),
                // Two-digit years: assume 2000-2099.
                Group::Year2 => year = Some(2000 + n as i32),
                Group::Month => month = Some(n),
                Group::Day => day = Some(n),
                Group::DayOfYear => doy = Some(n),
            }
        }
        let year = year?;
        let date = match (month, day, doy) {
            (Some(m), Some(d), _) => Date::new(year, m, d)?,
            (_, _, Some(n)) if n >= 1 => Date::new(year, 1, 1)?.add_days(n as i64 - 1),
            _ => return None,
        };
        // Every part of the name (weekday, a repeated year …) must agree with the date.
        let expected = self.config.note_path(date).ok()?;
        (expected.to_lowercase() == path.to_lowercase()).then_some(date)
    }
}

impl DailyConfig {
    fn format(&self) -> &str {
        if self.format.trim().is_empty() { DEFAULT_FORMAT } else { self.format.trim() }
    }

    /// Build a matcher for this config's daily-note paths
    /// (`None` if the date format can't name a day).
    pub fn matcher(&self) -> Option<DailyMatcher> {
        // A format that can't be filled from a date names no daily note.
        format_date(self.format(), Date::new(2000, 1, 1)?).ok()?;
        let mut pattern = String::from("(?i)^");
        let folder = self.folder.trim().trim_matches('/');
        if !folder.is_empty() {
            pattern.push_str(&regex::escape(folder));
            pattern.push('/');
        }
        let mut groups = Vec::new();
        for p in moment::pieces(self.format()) {
            let t = match p {
                Piece::Text(s) => {
                    pattern.push_str(&regex::escape(&s));
                    continue;
                }
                Piece::Token(t) => t,
            };
            let (re, group) = match t.as_str() {
                "YYYY" | "Y" => (r"(\d{4})", Some(Group::Year4)),
                "YY" => (r"(\d{2})", Some(Group::Year2)),
                "M" => (r"(\d{1,2})", Some(Group::Month)),
                "MM" => (r"(\d{2})", Some(Group::Month)),
                "MMM" | "MMMM" | "Mo" => (r"(\d{1,2})월", Some(Group::Month)),
                "D" => (r"(\d{1,2})", Some(Group::Day)),
                "DD" => (r"(\d{2})", Some(Group::Day)),
                "Do" => (r"(\d{1,2})일", Some(Group::Day)),
                "DDD" => (r"(\d{1,3})", Some(Group::DayOfYear)),
                "DDDD" => (r"(\d{3})", Some(Group::DayOfYear)),
                "DDDo" => (r"(\d{1,3})일", Some(Group::DayOfYear)),
                "dd" | "ddd" => ("[일월화수목금토]", None),
                "dddd" => ("[일월화수목금토]요일", None),
                "do" => (r"\d일", None),
                "wo" | "Wo" => (r"\d{1,2}주", None),
                "YYYYYY" => (r"[+-]\d{6}", None),
                _ => (r"[+-]?\d+", None),
            };
            pattern.push_str(re);
            if let Some(g) = group {
                groups.push(g);
            }
        }
        pattern.push_str(r"\.md$");
        Some(DailyMatcher { regex: Regex::new(&pattern).ok()?, groups, config: self.clone() })
    }

    /// Vault-relative path of the daily note for `date`.
    pub fn note_path(&self, date: Date) -> Result<String, String> {
        let name = format_date(self.format(), date)?;
        let folder = self.folder.trim().trim_matches('/');
        Ok(if folder.is_empty() { format!("{name}.md") } else { format!("{folder}/{name}.md") })
    }

    /// Contents for a brand-new daily note: the template with
    /// `{{title}}`, `{{date}}`, `{{date:FORMAT}}` and `{{time}}` filled in.
    pub fn initial_content(&self, root: &Path, date: Date, time: &str) -> String {
        let template = self.template.trim();
        if template.is_empty() {
            return String::new();
        }
        let rel = if template.to_lowercase().ends_with(".md") {
            template.to_string()
        } else {
            format!("{template}.md")
        };
        let Ok(raw) = vault::read_text(root, &rel) else {
            return String::new();
        };
        let title = format_date(self.format(), date)
            .map(|n| n.rsplit('/').next().unwrap_or(&n).to_string())
            .unwrap_or_default();
        static VAR: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"\{\{\s*(title|date|time)\s*(?::([^}]*))?\}\}").unwrap());
        let at = crate::templates::Moment::parse(&date.to_string(), time)
            .unwrap_or(crate::templates::Moment { date, hour: 0, minute: 0 });
        VAR.replace_all(&raw, |c: &regex::Captures| match (&c[1], c.get(2)) {
            ("title", _) => title.clone(),
            ("time", None) => time.to_string(),
            ("date", None) => date.to_string(),
            (_, Some(fmt)) => crate::templates::format_moment(fmt.as_str().trim(), at),
            _ => c[0].to_string(),
        })
        .into_owned()
    }
}

/// A new Tasks-style line: `- [ ] text 📅 due`.
pub fn task_line(text: &str, due: Option<Date>) -> Result<String, String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.is_empty() {
        return Err("할일 내용을 입력하세요.".into());
    }
    Ok(match due {
        Some(d) => format!("- [ ] {text} 📅 {d}"),
        None => format!("- [ ] {text}"),
    })
}

/// Append `line` to `existing` (or to a fresh note) as its own line.
pub fn append_line(existing: &str, line: &str) -> String {
    let mut out = existing.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(line);
    out.push('\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse(s).unwrap()
    }

    #[test]
    fn formats_supported_tokens() {
        let date = d("2026-03-07");
        assert_eq!(format_date("YYYY-MM-DD", date).unwrap(), "2026-03-07");
        assert_eq!(format_date("YY.M.D", date).unwrap(), "26.3.7");
        assert_eq!(format_date("YYYY/MM/YYYY-MM-DD", date).unwrap(), "2026/03/2026-03-07");
        assert_eq!(format_date("YYYY년 MM월 DD일", date).unwrap(), "2026년 03월 07일");
        assert_eq!(format_date("[Day] YYYY-MM-DD", date).unwrap(), "Day 2026-03-07");
    }

    #[test]
    fn more_moment_tokens() {
        let date = d("2026-03-07"); // a Saturday
        assert_eq!(format_date("YYYY-MM-DD dddd", date).unwrap(), "2026-03-07 토요일");
        assert_eq!(format_date("YYYY년 MMMM Do (ddd)", date).unwrap(), "2026년 3월 7일 (토)");
        assert_eq!(format_date("gggg-[W]ww", date).unwrap(), "2026-W10");
        assert_eq!(format_date("YYYY-DDDD", date).unwrap(), "2026-066");
        assert!(format_date("[unclosed", date).is_err());
        // Time of day can't name a daily note.
        assert!(format_date("YYYY-MM-DD HH", date).unwrap_err().contains("\"HH\""));
        // An unbracketed word is read as tokens, as in Obsidian: say which word to bracket.
        let err = format_date("YYYY/YYYY-MM-DD - Daily", date).unwrap_err();
        assert!(err.contains("\"Daily\"") && err.contains("[Daily]"), "{err}");
        assert_eq!(format_date("YYYY/YYYY-MM-DD - [Daily]", date).unwrap(), "2026/2026-03-07 - Daily");
    }

    #[test]
    fn config_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // no config -> vault root, YYYY-MM-DD
        assert_eq!(load_config(root).note_path(d("2026-11-13")).unwrap(), "2026-11-13.md");

        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(
            root.join(".obsidian/daily-notes.json"),
            r#"{"folder":"Journal/","format":"YYYY/MM-DD","template":"Templates/Daily"}"#,
        )
        .unwrap();
        let cfg = load_config(root);
        assert_eq!(cfg.note_path(d("2026-11-13")).unwrap(), "Journal/2026/11-13.md");

        fs::create_dir_all(root.join("Templates")).unwrap();
        fs::write(
            root.join("Templates/Daily.md"),
            "# {{title}}\n작성: {{date}} {{time}}\n주: {{date:YYYY.MM}}\n{{date:dddd}}\n",
        )
        .unwrap();
        assert_eq!(
            cfg.initial_content(root, d("2026-11-13"), "09:30"),
            "# 11-13\n작성: 2026-11-13 09:30\n주: 2026.11\n금요일\n"
        );
    }

    #[test]
    fn reads_dates_back_from_paths() {
        let cfg = DailyConfig { folder: "Journal".into(), format: "YYYY/MM/YYYY-MM-DD".into(), ..Default::default() };
        let m = cfg.matcher().unwrap();
        assert_eq!(m.date_of("Journal/2026/11/2026-11-13.md"), Some(d("2026-11-13")));
        assert_eq!(m.date_of("journal/2026/11/2026-11-13.md"), Some(d("2026-11-13"))); // case
        assert_eq!(m.date_of("Journal/2026/11/2025-11-13.md"), None); // years disagree
        assert_eq!(m.date_of("Journal/2026/13/2026-13-01.md"), None); // invalid date
        assert_eq!(m.date_of("Other/2026/11/2026-11-13.md"), None);
        assert_eq!(m.date_of("Journal/2026/11/2026-11-13 extra.md"), None);

        let m = DailyConfig { format: "YY.M.D[일기]".into(), ..Default::default() }.matcher().unwrap();
        assert_eq!(m.date_of("26.3.7일기.md"), Some(d("2026-03-07")));

        let m = DailyConfig::default().matcher().unwrap();
        assert_eq!(m.date_of("2026-11-13.md"), Some(d("2026-11-13")));
        assert_eq!(m.date_of("Sub/2026-11-13.md"), None);

        assert!(DailyConfig { format: "HH".into(), ..Default::default() }.matcher().is_none());

        // Names, weekdays and days of the year are read back and checked.
        let m = DailyConfig { format: "YYYY년 MMMM Do dddd".into(), ..Default::default() }.matcher().unwrap();
        assert_eq!(m.date_of("2026년 11월 13일 금요일.md"), Some(d("2026-11-13")));
        assert_eq!(m.date_of("2026년 11월 13일 월요일.md"), None); // wrong weekday
        let m = DailyConfig { format: "YYYY-DDDD".into(), ..Default::default() }.matcher().unwrap();
        assert_eq!(m.date_of("2026-066.md"), Some(d("2026-03-07")));
        let m = DailyConfig { format: "YYYY/YYYY-MM-DD - [Daily]".into(), folder: "40_LOGS/10_DAILY".into(), ..Default::default() }
            .matcher()
            .unwrap();
        assert_eq!(m.date_of("40_LOGS/10_DAILY/2026/2026-11-13 - Daily.md"), Some(d("2026-11-13")));

        // round trip with note_path
        for cfg in [cfg, DailyConfig { folder: "D".into(), format: "YYYY년 M월 D일".into(), ..Default::default() }] {
            let date = d("2026-01-05");
            assert_eq!(cfg.matcher().unwrap().date_of(&cfg.note_path(date).unwrap()), Some(date));
        }
    }

    #[test]
    fn missing_template_gives_empty_note() {
        let dir = tempfile::tempdir().unwrap();
        let cfg = DailyConfig { template: "Nope".into(), ..Default::default() };
        assert_eq!(cfg.initial_content(dir.path(), d("2026-11-13"), "09:00"), "");
    }

    #[test]
    fn task_lines_and_append() {
        assert_eq!(task_line("  장보기\n  우유 ", Some(d("2026-11-14"))).unwrap(), "- [ ] 장보기 우유 📅 2026-11-14");
        assert_eq!(task_line("x", None).unwrap(), "- [ ] x");
        assert!(task_line("   ", None).is_err());
        assert_eq!(append_line("", "- [ ] a"), "- [ ] a\n");
        assert_eq!(append_line("# t", "- [ ] a"), "# t\n- [ ] a\n");
        assert_eq!(append_line("# t\n", "- [ ] a"), "# t\n- [ ] a\n");
    }
}

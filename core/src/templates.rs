//! Obsidian core "Templates": settings in `<vault>/.obsidian/templates.json`
//! (`{ "folder": "Templates", "dateFormat": "YYYY-MM-DD", "timeFormat": "HH:mm" }`)
//! and the variables `{{title}}`, `{{date}}`, `{{time}}`, `{{date:FORMAT}}`,
//! `{{time:FORMAT}}` (moment.js-style formats).

use crate::date::Date;
use crate::frontmatter::{self, Property};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

const DEFAULT_DATE: &str = "YYYY-MM-DD";
const DEFAULT_TIME: &str = "HH:mm";

#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
pub struct TemplateConfig {
    #[serde(default)]
    pub folder: String,
    #[serde(default, rename = "dateFormat")]
    pub date_format: String,
    #[serde(default, rename = "timeFormat")]
    pub time_format: String,
}

pub fn load_config(root: &Path) -> TemplateConfig {
    fs::read_to_string(root.join(".obsidian").join("templates.json"))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

impl TemplateConfig {
    /// The templates folder without slashes ("" when not set).
    pub fn folder(&self) -> &str {
        self.folder.trim().trim_matches('/')
    }
}

/// The local moment a template is inserted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moment {
    pub date: Date,
    pub hour: u32,
    pub minute: u32,
}

impl Moment {
    /// From `YYYY-MM-DD` and `HH:MM` as the UI sends them.
    pub fn parse(date: &str, time: &str) -> Option<Moment> {
        let (h, m) = time.split_once(':')?;
        let (hour, minute) = (h.parse().ok()?, m.parse().ok()?);
        (hour < 24 && minute < 60).then_some(Moment { date: Date::parse(date)?, hour, minute })
    }
}

/// Format with moment.js tokens in Korean (see `moment.rs`); `[text]` is literal.
pub fn format_moment(pattern: &str, at: Moment) -> String {
    crate::moment::format(pattern, at.date, Some((at.hour, at.minute)))
}

static VAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{\{\s*(title|date|time)\s*(?::([^}]*))?\}\}").unwrap());

/// Fill in `{{title}}`, `{{date[:FORMAT]}}`, `{{time[:FORMAT]}}`.
pub fn render(raw: &str, title: &str, at: Moment, config: &TemplateConfig) -> String {
    let or = |s: &str, default: &'static str| if s.trim().is_empty() { default.to_string() } else { s.trim().to_string() };
    VAR.replace_all(raw, |c: &regex::Captures| {
        let custom = c.get(2).map(|m| m.as_str().trim()).filter(|s| !s.is_empty());
        match &c[1] {
            "title" => title.to_string(),
            "date" => format_moment(&custom.map(str::to_string).unwrap_or_else(|| or(&config.date_format, DEFAULT_DATE)), at),
            _ => format_moment(&custom.map(str::to_string).unwrap_or_else(|| or(&config.time_format, DEFAULT_TIME)), at),
        }
    })
    .into_owned()
}

/// A rendered template, split like Obsidian inserts it: properties are
/// merged into the note's, the body goes at the cursor.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Rendered {
    /// The template's properties (`None` if it has no frontmatter).
    pub properties: Option<Vec<Property>>,
    pub body: String,
}

pub fn split(rendered: &str) -> Result<Rendered, String> {
    let fm = frontmatter::parse(rendered);
    if !fm.present {
        return Ok(Rendered { properties: None, body: rendered.to_string() });
    }
    if let Some(e) = fm.error {
        return Err(format!("템플릿의 속성(YAML)을 읽을 수 없습니다: {e}"));
    }
    Ok(Rendered { properties: Some(fm.properties), body: rendered[fm.body_start..].to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn at() -> Moment {
        Moment::parse("2026-11-13", "14:05").unwrap()
    }

    #[test]
    fn formats_tokens() {
        assert_eq!(format_moment("YYYY-MM-DD", at()), "2026-11-13");
        assert_eq!(format_moment("YY.M.D (ddd)", at()), "26.11.13 (금)");
        assert_eq!(format_moment("MMMM Do dddd", at()), "11월 13일 금요일");
        assert_eq!(format_moment("HH:mm", at()), "14:05");
        assert_eq!(format_moment("A h:mm", at()), "오후 2:05");
        assert_eq!(format_moment("[Week] YYYY", at()), "Week 2026");
        assert_eq!(format_moment("d", at()), "5");
    }

    #[test]
    fn renders_variables_with_config_formats() {
        let config = TemplateConfig { date_format: "YYYY.MM.DD".into(), time_format: "A h:mm".into(), ..Default::default() };
        let text = "# {{title}}\n{{date}} {{time}} / {{date:dddd}} / {{ time : HH }} / {{other}}";
        assert_eq!(render(text, "회의", at(), &config), "# 회의\n2026.11.13 오후 2:05 / 금요일 / 14 / {{other}}");
        let plain = TemplateConfig::default();
        assert_eq!(render("{{date}} {{time}}", "", at(), &plain), "2026-11-13 14:05");
    }

    #[test]
    fn splits_properties_from_body() {
        let r = split("---\ntags: [meeting]\n---\n## 안건\n").unwrap();
        assert_eq!(r.properties.unwrap()[0].value, json!(["meeting"]));
        assert_eq!(r.body, "## 안건\n");
        assert_eq!(split("plain").unwrap(), Rendered { properties: None, body: "plain".into() });
        assert!(split("---\na: [\n---\n").is_err());
    }
}

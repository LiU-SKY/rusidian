//! The settings of Obsidian's core "Templates" and "Daily notes" plugins,
//! read and written in `.obsidian/templates.json` and
//! `.obsidian/daily-notes.json` (shared with Obsidian: other keys are kept).
//! Plus a preview of what they produce, so a format that reads letters as
//! date symbols or a folder that isn't there shows before it bites.

use crate::date::Date;
use crate::obsidian_config;
use crate::vault::{self, TreeNode};
use crate::{daily, moment};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const TEMPLATES_FILE: &str = "templates.json";
pub const DAILY_FILE: &str = "daily-notes.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSettings {
    pub folder: String,
    pub date_format: String,
    pub time_format: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct DailySettings {
    pub folder: String,
    pub format: String,
    pub template: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct NoteSettings {
    pub templates: TemplateSettings,
    pub daily: DailySettings,
    /// Every folder of the vault, for picking one.
    pub folders: Vec<String>,
    /// Every note of the vault without `.md`, for picking a template.
    pub notes: Vec<String>,
}

pub fn load(root: &Path) -> NoteSettings {
    let s = |file: &str, key: &str| obsidian_config::read_key::<String>(root, file, key);
    let (mut folders, mut notes) = (Vec::new(), Vec::new());
    if let Ok(tree) = vault::build_tree(root) {
        collect(&tree, &mut folders, &mut notes);
    }
    NoteSettings {
        templates: TemplateSettings {
            folder: s(TEMPLATES_FILE, "folder"),
            date_format: s(TEMPLATES_FILE, "dateFormat"),
            time_format: s(TEMPLATES_FILE, "timeFormat"),
        },
        daily: DailySettings {
            folder: s(DAILY_FILE, "folder"),
            format: s(DAILY_FILE, "format"),
            template: s(DAILY_FILE, "template"),
        },
        folders,
        notes,
    }
}

fn collect(nodes: &[TreeNode], folders: &mut Vec<String>, notes: &mut Vec<String>) {
    for n in nodes {
        if n.is_dir {
            folders.push(n.path.clone());
            collect(&n.children, folders, notes);
        } else if let Some(stem) = n.path.strip_suffix(".md") {
            notes.push(stem.to_string());
        }
    }
}

/// Folder and note paths as Obsidian stores them: `/` separators, no
/// surrounding slashes or spaces.
fn clean(path: &str) -> String {
    path.replace('\\', "/").trim().trim_matches('/').to_string()
}

pub fn save_templates(root: &Path, s: &TemplateSettings) -> Result<(), String> {
    obsidian_config::write_key(root, TEMPLATES_FILE, "folder", &clean(&s.folder))?;
    obsidian_config::write_key(root, TEMPLATES_FILE, "dateFormat", &s.date_format.trim())?;
    obsidian_config::write_key(root, TEMPLATES_FILE, "timeFormat", &s.time_format.trim())
}

pub fn save_daily(root: &Path, s: &DailySettings) -> Result<(), String> {
    // Obsidian keeps the template without `.md` ("Templates/Daily").
    let template = clean(&s.template);
    let template = template.strip_suffix(".md").unwrap_or(&template);
    obsidian_config::write_key(root, DAILY_FILE, "folder", &clean(&s.folder))?;
    obsidian_config::write_key(root, DAILY_FILE, "format", &s.format.trim())?;
    obsidian_config::write_key(root, DAILY_FILE, "template", &template)
}

/// What the settings produce right now.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Preview {
    /// `{{date}}` and `{{time}}` in a template.
    pub template_date: String,
    pub template_time: String,
    /// The templates folder is set but not in the vault.
    pub template_folder_missing: bool,
    /// Today's daily note, or why the format can't name it.
    pub daily_path: Option<String>,
    pub daily_error: Option<String>,
    /// The daily-note folder is set but not in the vault (created on first use).
    pub daily_folder_missing: bool,
    /// The daily-note template is set but no such note exists.
    pub daily_template_missing: bool,
}

pub fn preview(root: &Path, t: &TemplateSettings, d: &DailySettings, date: Date, time: (u32, u32)) -> Preview {
    let or = |s: &str, default: &'static str| if s.trim().is_empty() { default.to_string() } else { s.trim().to_string() };
    let missing_dir = |p: &str| {
        let p = clean(p);
        !p.is_empty() && !vault::resolve(root, &p).is_ok_and(|full| full.is_dir())
    };
    let config = daily::DailyConfig { folder: clean(&d.folder), format: d.format.trim().to_string(), template: String::new() };
    let (daily_path, daily_error) = match config.note_path(date) {
        Ok(p) => (Some(p), None),
        Err(e) => (None, Some(e)),
    };
    let template = clean(&d.template);
    let template_file = if template.to_lowercase().ends_with(".md") { template.clone() } else { format!("{template}.md") };
    Preview {
        template_date: moment::format(&or(&t.date_format, "YYYY-MM-DD"), date, Some(time)),
        template_time: moment::format(&or(&t.time_format, "HH:mm"), date, Some(time)),
        template_folder_missing: missing_dir(&t.folder),
        daily_path,
        daily_error,
        daily_folder_missing: missing_dir(&d.folder),
        daily_template_missing: !template.is_empty()
            && !vault::resolve(root, &template_file).is_ok_and(|full| full.is_file()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn saves_both_files_keeping_obsidians_other_keys() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".obsidian")).unwrap();
        fs::write(root.join(".obsidian/daily-notes.json"), r#"{"autorun": true, "format": "YYYY"}"#).unwrap();
        save_daily(root, &DailySettings {
            folder: "/일지/".into(),
            format: " YYYY-MM-DD - [Daily] ".into(),
            template: "템플릿\\Daily.md".into(),
        })
        .unwrap();
        save_templates(root, &TemplateSettings { folder: "템플릿".into(), date_format: "YYYY.MM.DD".into(), time_format: "".into() })
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&fs::read_to_string(root.join(".obsidian/daily-notes.json")).unwrap()).unwrap();
        assert_eq!(v, serde_json::json!({"autorun": true, "folder": "일지", "format": "YYYY-MM-DD - [Daily]", "template": "템플릿/Daily"}));
        let s = load(root);
        assert_eq!(s.daily.template, "템플릿/Daily");
        assert_eq!(s.templates.date_format, "YYYY.MM.DD");
    }

    #[test]
    fn preview_shows_the_note_and_what_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("템플릿")).unwrap();
        fs::write(root.join("템플릿/Daily.md"), "").unwrap();
        let date = Date::new(2026, 11, 15).unwrap();
        let t = TemplateSettings { folder: "템플릿".into(), ..Default::default() };

        let ok = DailySettings { folder: "".into(), format: "YYYY/[일지]/YYYY-MM-DD - [Daily]".into(), template: "템플릿/Daily".into() };
        let p = preview(root, &t, &ok, date, (9, 5));
        assert_eq!(p.daily_path.as_deref(), Some("2026/일지/2026-11-15 - Daily.md"));
        assert_eq!((p.template_date.as_str(), p.template_time.as_str()), ("2026-11-15", "09:05"));
        assert!(!p.template_folder_missing && !p.daily_folder_missing && !p.daily_template_missing);

        // The settings this vault had: "Daily" read as symbols, folders that aren't there.
        let bad = DailySettings { folder: "40_LOGS/10_DAILY".into(), format: "YYYY/YYYY-MM-DD - Daily".into(), template: "90_TEMPLATES/Daily.md".into() };
        let p = preview(root, &t, &bad, date, (9, 5));
        assert!(p.daily_path.is_none());
        assert!(p.daily_error.unwrap().contains("[Daily]"));
        assert!(p.daily_folder_missing && p.daily_template_missing);
    }
}

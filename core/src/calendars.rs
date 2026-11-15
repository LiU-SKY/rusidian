//! External calendar subscriptions as stored in the app settings: the list
//! and its validation. Downloading lives in the app (`src-tauri`).

use std::path::Path;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct CalendarSource {
    pub id: String,
    pub name: String,
    /// `https://…`, `webcal://…` (stored as https) or a local file path.
    pub url: String,
    /// `#rrggbb`
    pub color: String,
    #[serde(default = "enabled_default")]
    pub enabled: bool,
}

fn enabled_default() -> bool {
    true
}

const DEFAULT_COLOR: &str = "#2a78d6";

pub fn is_http(url: &str) -> bool {
    let l = url.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// Check and tidy a subscription address: `webcal://` becomes `https://`,
/// `file://` becomes a path; anything else must be an existing file.
pub fn normalize_url(url: &str) -> Result<String, String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("캘린더 주소를 입력하세요.".into());
    }
    let lower = u.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("webcal://") {
        return Ok(format!("https://{}", &u[u.len() - rest.len()..]));
    }
    if is_http(u) {
        return Ok(u.to_string());
    }
    let path = if lower.starts_with("file:///") { &u[8..] } else { u };
    if Path::new(path).is_file() {
        Ok(path.to_string())
    } else {
        Err(format!("주소가 http(s)/webcal 형식이 아니고 그런 파일도 없습니다: {u}"))
    }
}

/// Clean up the list from the settings dialog: addresses checked, names and
/// colors defaulted, ids made unique.
pub fn validate(list: Vec<CalendarSource>) -> Result<Vec<CalendarSource>, String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .enumerate()
        .map(|(i, mut c)| {
            c.url = normalize_url(&c.url)?;
            c.name = c.name.trim().to_string();
            if c.name.is_empty() {
                c.name = "캘린더".into();
            }
            let hex = c.color.trim_start_matches('#');
            if hex.len() != 6 || u32::from_str_radix(hex, 16).is_err() {
                c.color = DEFAULT_COLOR.into();
            }
            if c.id.trim().is_empty() || !seen.insert(c.id.clone()) {
                c.id = format!("cal-{stamp}-{i}");
                seen.insert(c.id.clone());
            }
            Ok(c)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn normalizes_addresses() {
        assert_eq!(normalize_url(" webcal://calendar.google.com/x/basic.ics ").unwrap(), "https://calendar.google.com/x/basic.ics");
        assert_eq!(normalize_url("https://a.b/c.ics").unwrap(), "https://a.b/c.ics");
        assert!(normalize_url("ftp://a").is_err());
        assert!(normalize_url("").is_err());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cal.ics");
        fs::write(&file, "BEGIN:VCALENDAR\nEND:VCALENDAR\n").unwrap();
        let path = file.to_string_lossy().to_string();
        assert_eq!(normalize_url(&path).unwrap(), path);
    }

    #[test]
    fn validates_list() {
        let src = |id: &str, color: &str| CalendarSource {
            id: id.into(),
            name: " ".into(),
            url: "webcal://x.y/z".into(),
            color: color.into(),
            enabled: true,
        };
        let out = validate(vec![src("a", "#123456"), src("a", "red"), src("", "#abcdef")]).unwrap();
        assert_eq!(out[0].id, "a");
        assert_ne!(out[1].id, "a");
        assert_ne!(out[2].id, "");
        assert_eq!(out[1].color, DEFAULT_COLOR);
        assert_eq!(out[0].name, "캘린더");
        assert_eq!(out[0].url, "https://x.y/z");
    }
}

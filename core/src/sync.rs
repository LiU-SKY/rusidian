//! Git sync of chosen parts of a vault.
//!
//! The vault may hold documents that must never leave the computer, so
//! sync works from an allow-list: the settings folder (`.obsidian`:
//! plugins, themes, snippets, hotkeys, Obsidian and Rusidian settings) is
//! always included, note folders only when chosen. Everything else —
//! including folders created later — stays out. Files over
//! [`LARGE_BYTES`] are left out too (GitHub refuses files over 100 MB).
//! The Git repository lives outside the vault (see `src-tauri`), so
//! nothing is added to the vault itself.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// The settings folder, always synced.
pub const SETTINGS_DIR: &str = ".obsidian";
/// Files bigger than this are not synced.
pub const LARGE_BYTES: u64 = 50 * 1024 * 1024;
/// Rusidian's own settings, written into the settings folder for sync.
pub const APP_SETTINGS_FILE: &str = ".obsidian/rusidian-app.json";

/// Per-device layout files that should not travel.
const DEVICE_FILES: &[&str] = &["workspace.json", "workspace-mobile.json"];
/// OS clutter, never synced.
const JUNK: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];

/// Sync settings of one vault. The device part (remote, branch, interval)
/// lives in the app settings of each computer; the shared part (folders,
/// calendars) in the vault's `.obsidian/rusidian.json`, so every device
/// syncs the same folders.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SyncConfig {
    /// Git remote, e.g. `https://github.com/me/notes.git`.
    pub remote: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    /// Note folders to sync (vault-relative, `/`-separated).
    #[serde(default)]
    pub folders: Vec<String>,
    /// Also sync calendar subscription URLs (they hold private tokens).
    #[serde(default)]
    pub include_calendars: bool,
    /// Minutes between automatic syncs (0 = only on demand and at start).
    #[serde(default)]
    pub interval_minutes: u32,
}

fn default_branch() -> String {
    "main".into()
}

/// The part of the sync settings every device shares (synced itself).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Shared {
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub include_calendars: bool,
}

const SHARED_FILE: &str = "rusidian.json";
const SHARED_KEY: &str = "sync";

pub fn load_shared(root: &Path) -> Shared {
    crate::obsidian_config::read_key(root, SHARED_FILE, SHARED_KEY)
}

pub fn save_shared(root: &Path, shared: &Shared) -> Result<(), String> {
    let clean = Shared { folders: normalize_folders(&shared.folders), include_calendars: shared.include_calendars };
    crate::obsidian_config::write_key(root, SHARED_FILE, SHARED_KEY, &clean)
}

impl SyncConfig {
    /// This device's settings with the vault's shared ones.
    pub fn with_shared(&self, shared: &Shared) -> SyncConfig {
        SyncConfig { folders: shared.folders.clone(), include_calendars: shared.include_calendars, ..self.clone() }
    }
}

/// Folder paths without leading/trailing slashes, `\` as `/`, no `.`/`..`,
/// no hidden folders, and none inside another chosen folder.
pub fn normalize_folders(folders: &[String]) -> Vec<String> {
    let mut out: Vec<String> = folders
        .iter()
        .map(|f| f.replace('\\', "/").trim().trim_matches('/').to_string())
        .filter(|f| !f.is_empty() && !f.split('/').any(|p| p.is_empty() || p == "." || p == ".." || p.starts_with('.')))
        .collect();
    out.sort();
    out.dedup();
    let all = out.clone();
    out.retain(|f| !all.iter().any(|g| g != f && f.starts_with(&format!("{g}/"))));
    out
}

/// A path as a literal gitignore pattern (globs and leading `#`/`!` escaped).
fn escape(path: &str) -> String {
    let mut out = String::new();
    for (i, c) in path.chars().enumerate() {
        if matches!(c, '*' | '?' | '[' | ']' | '\\') || (i == 0 && matches!(c, '#' | '!')) {
            out.push('\\');
        }
        out.push(c);
    }
    // Trailing spaces are ignored by Git unless escaped.
    if out.ends_with(' ') {
        let trimmed = out.trim_end_matches(' ').len();
        let spaces = out.len() - trimmed;
        out.truncate(trimmed);
        out.push_str(&"\\ ".repeat(spaces));
    }
    out
}

/// The exclude file (gitignore syntax) for an allow-list of `folders`,
/// plus explicit `large` files to leave out.
pub fn exclude_rules(folders: &[String], large: &[String]) -> String {
    let folders = normalize_folders(folders);
    let mut lines = vec![
        "# Written by Rusidian: only the paths allowed below are synced.".to_string(),
        "/*".to_string(),
        format!("!/{SETTINGS_DIR}/"),
    ];
    for f in DEVICE_FILES {
        lines.push(format!("/{SETTINGS_DIR}/{f}"));
    }
    // Ancestors of chosen folders are opened one level at a time.
    let mut partial: Vec<String> = Vec::new();
    for f in &folders {
        let parts: Vec<&str> = f.split('/').collect();
        for i in 1..parts.len() {
            partial.push(parts[..i].join("/"));
        }
    }
    let mut entries: Vec<(String, bool)> = partial.into_iter().map(|p| (p, false)).collect();
    entries.extend(folders.iter().map(|f| (f.clone(), true)));
    entries.sort();
    entries.dedup();
    for (path, chosen) in entries {
        let p = escape(&path);
        lines.push(format!("!/{p}/"));
        if !chosen {
            lines.push(format!("/{p}/*"));
        }
    }
    for j in JUNK {
        lines.push(j.to_string());
    }
    for l in large {
        lines.push(format!("/{}", escape(l)));
    }
    lines.join("\n") + "\n"
}

/// What the allow-list would upload.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Scan {
    pub count: usize,
    pub bytes: u64,
    /// The first files (for the preview list).
    pub files: Vec<String>,
    /// Files left out for being larger than [`LARGE_BYTES`].
    pub large: Vec<(String, u64)>,
    /// Size of each chosen folder (and the settings folder), in bytes.
    pub per_folder: Vec<(String, u64)>,
}

const PREVIEW_FILES: usize = 1000;

/// Walk the settings folder and the chosen folders like Git would see them.
pub fn scan(root: &Path, folders: &[String]) -> Scan {
    let mut s = Scan::default();
    let mut tops = vec![SETTINGS_DIR.to_string()];
    tops.extend(normalize_folders(folders));
    for top in tops {
        let before = s.bytes;
        walk(root, &root.join(&top), &top, &mut s);
        s.per_folder.push((top, s.bytes - before));
    }
    s.files.sort();
    s.large.sort();
    s
}

fn walk(root: &Path, dir: &Path, rel: &str, s: &mut Scan) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        let name = entry.file_name().to_string_lossy().into_owned();
        if ft.is_symlink() || JUNK.contains(&name.as_str()) || name == ".git" {
            continue;
        }
        let path = format!("{rel}/{name}");
        if ft.is_dir() {
            walk(root, &entry.path(), &path, s);
        } else if ft.is_file() {
            if rel == SETTINGS_DIR && DEVICE_FILES.contains(&name.as_str()) {
                continue;
            }
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            if size > LARGE_BYTES {
                s.large.push((path, size));
                continue;
            }
            s.count += 1;
            s.bytes += size;
            if s.files.len() < PREVIEW_FILES {
                s.files.push(path);
            }
        }
    }
}

/// Where the other device's version of a conflicting file is kept:
/// `Folder/Note (충돌 2026-11-14 0305).md`.
pub fn conflict_name(path: &str, stamp: &str) -> String {
    let (dir, name) = match path.rsplit_once('/') {
        Some((d, n)) => (format!("{d}/"), n),
        None => (String::new(), path),
    };
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => format!("{dir}{stem} (충돌 {stamp}).{ext}"),
        _ => format!("{dir}{name} (충돌 {stamp})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list_rules() {
        let rules = exclude_rules(&["Projects/Rusidian".into(), "Daily".into(), "Daily/2026".into()], &["Daily/video.mp4".into()]);
        let lines: Vec<&str> = rules.lines().skip(1).collect();
        assert_eq!(
            lines,
            [
                "/*",
                "!/.obsidian/",
                "/.obsidian/workspace.json",
                "/.obsidian/workspace-mobile.json",
                "!/Daily/",
                "!/Projects/",
                "/Projects/*",
                "!/Projects/Rusidian/",
                ".DS_Store",
                "Thumbs.db",
                "desktop.ini",
                "/Daily/video.mp4",
            ]
        );
        // Nothing chosen: only the settings folder.
        assert!(!exclude_rules(&[], &[]).contains("!/D"));
    }

    #[test]
    fn folder_names_are_cleaned_and_escaped() {
        assert_eq!(normalize_folders(&["/a\\b/".into(), "../x".into(), ".git".into(), "a".into(), "".into()]), ["a"]);
        assert_eq!(escape("#notes [old]*"), "\\#notes \\[old\\]\\*");
        assert_eq!(escape("trail  "), "trail\\ \\ ");
    }

    #[test]
    fn scans_only_allowed_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (p, c) in [
            (".obsidian/app.json", "{}"),
            (".obsidian/workspace.json", "{}"),
            (".obsidian/plugins/x/main.js", "code"),
            ("Daily/2026-11-14.md", "hi"),
            ("Daily/.DS_Store", "x"),
            ("Secret/contract.pdf", "secret"),
            ("root.md", "no"),
        ] {
            let full = root.join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, c).unwrap();
        }
        let s = scan(root, &["Daily".into()]);
        assert_eq!(s.files, [".obsidian/app.json", ".obsidian/plugins/x/main.js", "Daily/2026-11-14.md"]);
        assert_eq!(s.count, 3);
        assert_eq!(s.bytes, 2 + 4 + 2);
        assert_eq!(s.per_folder, [(".obsidian".to_string(), 6), ("Daily".to_string(), 2)]);
    }

    /// The rules as Git itself reads them: only allowed files get staged.
    /// Skipped when Git isn't installed.
    #[test]
    fn git_honours_the_allow_list() {
        use std::process::Command;
        if Command::new("git").arg("--version").output().is_err() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let (vault, git_dir) = (dir.path().join("vault"), dir.path().join("repo.git"));
        for p in [
            ".obsidian/app.json",
            ".obsidian/workspace.json",
            "Daily/2026.md",
            "Projects/Rusidian/plan.md",
            "Projects/Secret/budget.md",
            "Secret/contract.pdf",
            "#hash [x]/a.md",
            "root.md",
        ] {
            let full = vault.join(p);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "x").unwrap();
        }
        let git = |args: &[&str]| {
            let out = Command::new("git")
                .args(args)
                .env("GIT_DIR", &git_dir)
                .env("GIT_WORK_TREE", &vault)
                .output()
                .unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        git(&["init", "-q"]);
        let rules = exclude_rules(&["Daily".into(), "Projects/Rusidian".into(), "#hash [x]".into()], &[]);
        fs::write(git_dir.join("info/exclude"), rules).unwrap();
        git(&["add", "-A"]);
        let mut staged: Vec<String> = git(&["-c", "core.quotepath=false", "diff", "--cached", "--name-only"])
            .lines()
            .map(str::to_string)
            .collect();
        staged.sort();
        assert_eq!(staged, ["#hash [x]/a.md", ".obsidian/app.json", "Daily/2026.md", "Projects/Rusidian/plan.md"]);
    }

    #[test]
    fn conflict_names() {
        assert_eq!(conflict_name("Daily/note.md", "2026-11-14 0305"), "Daily/note (충돌 2026-11-14 0305).md");
        assert_eq!(conflict_name("README", "s"), "README (충돌 s)");
        assert_eq!(conflict_name(".obsidian/.hidden", "s"), ".obsidian/.hidden (충돌 s)");
    }
}

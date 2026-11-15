//! Recently opened vaults (app settings), for the vault switcher.

use std::path::Path;

/// How many vaults the list remembers.
const MAX_RECENT: usize = 20;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct RecentVault {
    pub path: String,
    /// When it was last opened (seconds since 1970).
    #[serde(default)]
    pub opened: u64,
}

/// A recent vault as the UI shows it.
#[derive(Debug, Clone, serde::Serialize, PartialEq)]
pub struct VaultEntry {
    pub path: String,
    /// Folder name.
    pub name: String,
    /// The folder still exists.
    pub exists: bool,
    pub opened: u64,
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

/// The same folder, however it was spelled (case and `\` vs `/` on Windows).
pub fn same_path(a: &str, b: &str) -> bool {
    if let (Ok(x), Ok(y)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return x == y;
    }
    let norm = |s: &str| s.trim_end_matches(['/', '\\']).replace('\\', "/").to_lowercase();
    norm(a) == norm(b)
}

/// Move `path` to the front of `list` (newest first), keeping it short.
pub fn remember(list: &mut Vec<RecentVault>, path: &str, opened: u64) {
    list.retain(|v| !same_path(&v.path, path));
    list.insert(0, RecentVault { path: path.to_string(), opened });
    list.truncate(MAX_RECENT);
}

pub fn forget(list: &mut Vec<RecentVault>, path: &str) {
    list.retain(|v| !same_path(&v.path, path));
}

pub fn entries(list: &[RecentVault]) -> Vec<VaultEntry> {
    list.iter()
        .map(|v| {
            let p = Path::new(&v.path);
            VaultEntry {
                path: v.path.clone(),
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| v.path.clone()),
                exists: p.is_dir(),
                opened: v.opened,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newest_first_without_duplicates() {
        let mut list = Vec::new();
        remember(&mut list, "C:/Notes/Work", 1);
        remember(&mut list, "C:/Notes/Home", 2);
        remember(&mut list, r"c:\notes\work\", 3); // same folder, other spelling
        let paths: Vec<_> = list.iter().map(|v| v.path.as_str()).collect();
        assert_eq!(paths, [r"c:\notes\work\", "C:/Notes/Home"]);
        assert_eq!(list[0].opened, 3);

        for i in 0..30 {
            remember(&mut list, &format!("C:/v{i}"), 10 + i);
        }
        assert_eq!(list.len(), MAX_RECENT);
        assert_eq!(list[0].path, "C:/v29");

        forget(&mut list, "c:/V29");
        assert_eq!(list[0].path, "C:/v28");
    }

    #[test]
    fn entries_report_missing_folders() {
        let dir = std::env::temp_dir();
        let list = vec![
            RecentVault { path: dir.to_string_lossy().into_owned(), opened: 1 },
            RecentVault { path: "Z:/definitely/not/here".into(), opened: 2 },
        ];
        let e = entries(&list);
        assert!(e[0].exists);
        assert!(!e[1].exists);
        assert_eq!(e[1].name, "here");
    }
}

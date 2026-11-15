//! File recovery (like Obsidian's core plugin): earlier versions of notes,
//! kept on this computer only, outside the vault, so Git sync never sees
//! them. Before a note is overwritten its old text is kept, at most once
//! every few minutes; before it is deleted or restored it is always kept.
//!
//! Layout: `<dir>/<hash of path>/path.txt` holds the note's path and each
//! version is `<unix ms>.md` next to it.

use std::fs;
use std::path::{Path, PathBuf};

/// A new version is kept at most this often while a note is being edited.
pub const INTERVAL_MS: i64 = 5 * 60 * 1000;
/// Versions older than this are removed.
pub const KEEP_MS: i64 = 30 * 24 * 60 * 60 * 1000;
/// At most this many versions per note (the oldest go first).
pub const MAX_VERSIONS: usize = 200;
/// Bigger notes are not kept.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
const PATH_FILE: &str = "path.txt";
const EXT: &str = ".md";

#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct Version {
    /// Unix ms when it was kept; also its id.
    pub id: i64,
    /// Bytes.
    pub size: u64,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct FileHistory {
    pub path: String,
    /// Newest version's id.
    pub latest: i64,
    pub count: usize,
}

/// The kept versions of one vault.
pub struct History {
    dir: PathBuf,
}

/// FNV-1a, stable across runs (folder names must not change).
pub fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

/// Only Markdown notes are kept.
pub fn is_note(path: &str) -> bool {
    path.to_lowercase().ends_with(".md")
}

impl History {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        History { dir: dir.into() }
    }

    /// Paths differ only in case on Windows/macOS: keep them together.
    fn file_dir(&self, path: &str) -> PathBuf {
        self.dir.join(format!("{:016x}", hash(&path.to_lowercase())))
    }

    /// Keep `content` as a version of `path`. Skipped (Ok(false)) if it equals
    /// the newest version, or, unless `force`, if the newest is younger than
    /// [`INTERVAL_MS`].
    pub fn keep(&self, path: &str, content: &str, now_ms: i64, force: bool) -> Result<bool, String> {
        if !is_note(path) || content.len() > MAX_BYTES {
            return Ok(false);
        }
        let dir = self.file_dir(path);
        let versions = list_versions(&dir);
        if let Some(newest) = versions.first() {
            if !force && now_ms - newest.id < INTERVAL_MS {
                return Ok(false);
            }
            if fs::read_to_string(version_file(&dir, newest.id)).is_ok_and(|t| t == content) {
                return Ok(false);
            }
        }
        fs::create_dir_all(&dir).map_err(|e| format!("이전 버전을 저장할 폴더를 만들 수 없습니다: {e}"))?;
        fs::write(dir.join(PATH_FILE), path).map_err(|e| e.to_string())?;
        // Ids must be unique and newer than the newest.
        let id = versions.first().map_or(now_ms, |v| now_ms.max(v.id + 1));
        fs::write(version_file(&dir, id), content).map_err(|e| format!("이전 버전을 저장할 수 없습니다: {e}"))?;
        for old in versions.iter().skip(MAX_VERSIONS - 1) {
            let _ = fs::remove_file(version_file(&dir, old.id));
        }
        Ok(true)
    }

    /// Versions of `path`, newest first.
    pub fn versions(&self, path: &str) -> Vec<Version> {
        list_versions(&self.file_dir(path))
    }

    pub fn read(&self, path: &str, id: i64) -> Result<String, String> {
        fs::read_to_string(version_file(&self.file_dir(path), id))
            .map_err(|_| "이 버전을 찾을 수 없습니다.".to_string())
    }

    /// Every note with versions, newest change first (deleted notes too).
    pub fn files(&self) -> Vec<FileHistory> {
        let mut out = Vec::new();
        for (dir, path) in self.entries() {
            let versions = list_versions(&dir);
            if let Some(newest) = versions.first() {
                out.push(FileHistory { path, latest: newest.id, count: versions.len() });
            }
        }
        out.sort_by(|a, b| b.latest.cmp(&a.latest).then_with(|| a.path.cmp(&b.path)));
        out
    }

    fn entries(&self) -> Vec<(PathBuf, String)> {
        let Ok(read) = fs::read_dir(&self.dir) else { return Vec::new() };
        read.flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let path = fs::read_to_string(e.path().join(PATH_FILE)).ok()?;
                Some((e.path(), path))
            })
            .collect()
    }

    /// A file or folder moved from `from` to `to`: its notes keep their versions.
    pub fn moved(&self, from: &str, to: &str) {
        for (dir, path) in self.entries() {
            let Some(new_path) = map_path(from, to, &path) else { continue };
            let target = self.file_dir(&new_path);
            if target != dir {
                let _ = fs::create_dir_all(&target);
                for v in list_versions(&dir) {
                    let _ = fs::rename(version_file(&dir, v.id), version_file(&target, v.id));
                }
                let _ = fs::remove_dir_all(&dir);
            }
            let _ = fs::write(target.join(PATH_FILE), &new_path);
        }
    }

    /// Forget the versions of `path` (None: of every note).
    pub fn clear(&self, path: Option<&str>) -> Result<(), String> {
        let result = match path {
            Some(p) => {
                let dir = self.file_dir(p);
                if dir.exists() { fs::remove_dir_all(dir) } else { Ok(()) }
            }
            None if self.dir.exists() => fs::remove_dir_all(&self.dir),
            None => Ok(()),
        };
        result.map_err(|e| format!("이전 버전을 지울 수 없습니다: {e}"))
    }

    /// Remove versions older than `keep_ms`, and notes left with none.
    pub fn prune(&self, now_ms: i64, keep_ms: i64) {
        for (dir, _) in self.entries() {
            let versions = list_versions(&dir);
            let mut left = versions.len();
            for v in versions.iter().filter(|v| now_ms - v.id > keep_ms) {
                if fs::remove_file(version_file(&dir, v.id)).is_ok() {
                    left -= 1;
                }
            }
            if left == 0 {
                let _ = fs::remove_dir_all(&dir);
            }
        }
    }
}

fn version_file(dir: &Path, id: i64) -> PathBuf {
    dir.join(format!("{id}{EXT}"))
}

fn list_versions(dir: &Path) -> Vec<Version> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Version> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(EXT)?.parse().ok()?;
            Some(Version { id, size: e.metadata().map(|m| m.len()).unwrap_or(0) })
        })
        .collect();
    out.sort_by_key(|v| std::cmp::Reverse(v.id));
    out
}

/// `path` after moving `from` to `to` (None: it doesn't move). Case is
/// ignored, as vault paths are on Windows.
fn map_path(from: &str, to: &str, path: &str) -> Option<String> {
    let head = path.get(..from.len())?;
    if head.to_lowercase() != from.to_lowercase() {
        return None;
    }
    let rest = &path[from.len()..];
    (rest.is_empty() || rest.starts_with('/')).then(|| format!("{to}{rest}"))
}

#[derive(serde::Serialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    Same,
    Added,
    Removed,
}

#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct DiffLine {
    pub change: Change,
    pub text: String,
}

/// Above this many line pairs the middle part is shown as removed + added.
const DIFF_CELLS: usize = 4_000_000;

/// Line diff from `old` to `new` (longest common subsequence).
pub fn diff(old: &str, new: &str) -> Vec<DiffLine> {
    let a: Vec<&str> = old.lines().collect();
    let b: Vec<&str> = new.lines().collect();
    let line = |change, text: &str| DiffLine { change, text: text.to_string() };
    // Common start and end need no table.
    let pre = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let suf = a[pre..].iter().rev().zip(b[pre..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);

    let mut out: Vec<DiffLine> = a[..pre].iter().map(|t| line(Change::Same, t)).collect();
    if (ma.len() + 1) * (mb.len() + 1) > DIFF_CELLS {
        out.extend(ma.iter().map(|t| line(Change::Removed, t)));
        out.extend(mb.iter().map(|t| line(Change::Added, t)));
    } else {
        // lcs[i][j]: common lines of ma[i..] and mb[j..].
        let w = mb.len() + 1;
        let mut lcs = vec![0u32; (ma.len() + 1) * w];
        for i in (0..ma.len()).rev() {
            for j in (0..mb.len()).rev() {
                lcs[i * w + j] = if ma[i] == mb[j] {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < ma.len() || j < mb.len() {
            if i < ma.len() && j < mb.len() && ma[i] == mb[j] {
                out.push(line(Change::Same, ma[i]));
                i += 1;
                j += 1;
            } else if i < ma.len() && (j == mb.len() || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                // Removed lines come before the lines that replace them.
                out.push(line(Change::Removed, ma[i]));
                i += 1;
            } else {
                out.push(line(Change::Added, mb[j]));
                j += 1;
            }
        }
    }
    out.extend(a[a.len() - suf..].iter().map(|t| line(Change::Same, t)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60 * 1000;

    #[test]
    fn versions_are_kept_at_intervals() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path());
        assert!(h.keep("a.md", "one", 0, false).unwrap());
        // Too soon, and the same text: both skipped.
        assert!(!h.keep("a.md", "two", 2 * MIN, false).unwrap());
        assert!(!h.keep("a.md", "one", 10 * MIN, true).unwrap());
        // Forced (before a delete), or after the interval.
        assert!(h.keep("a.md", "two", 3 * MIN, true).unwrap());
        assert!(h.keep("a.md", "three", 9 * MIN, false).unwrap());
        // Not a note, or too big.
        assert!(!h.keep("a.png", "x", 0, true).unwrap());
        assert!(!h.keep("b.md", &"x".repeat(MAX_BYTES + 1), 0, true).unwrap());

        let ids: Vec<i64> = h.versions("a.md").iter().map(|v| v.id).collect();
        assert_eq!(ids, [9 * MIN, 3 * MIN, 0]);
        assert_eq!(h.read("A.md", 3 * MIN).unwrap(), "two");
        assert!(h.read("a.md", 1).is_err());
        assert_eq!(h.files(), [FileHistory { path: "a.md".into(), latest: 9 * MIN, count: 3 }]);
    }

    #[test]
    fn ids_stay_unique_and_old_ones_go() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path());
        for i in 0..(MAX_VERSIONS as i64 + 5) {
            h.keep("a.md", &format!("v{i}"), 100, true).unwrap();
        }
        let versions = h.versions("a.md");
        assert_eq!(versions.len(), MAX_VERSIONS);
        assert_eq!(h.read("a.md", versions[0].id).unwrap(), format!("v{}", MAX_VERSIONS + 4));

        let day = 24 * 60 * MIN;
        h.keep("b.md", "old", 0, true).unwrap();
        h.keep("b.md", "new", 40 * day, true).unwrap();
        h.prune(40 * day, KEEP_MS);
        assert_eq!(h.versions("b.md").len(), 1);
        assert!(h.versions("a.md").is_empty());
        assert_eq!(h.files().len(), 1);
    }

    #[test]
    fn versions_follow_moves() {
        let dir = tempfile::tempdir().unwrap();
        let h = History::new(dir.path());
        h.keep("Projects/plan.md", "plan", 0, true).unwrap();
        h.keep("Projects/Sub/deep.md", "deep", 0, true).unwrap();
        h.keep("ProjectsX.md", "other", 0, true).unwrap();
        h.moved("projects", "Work");
        let mut paths: Vec<String> = h.files().into_iter().map(|f| f.path).collect();
        paths.sort();
        assert_eq!(paths, ["ProjectsX.md", "Work/Sub/deep.md", "Work/plan.md"]);
        assert_eq!(h.read("Work/plan.md", 0).unwrap(), "plan");
        assert!(h.versions("Projects/plan.md").is_empty());

        h.clear(Some("Work/plan.md")).unwrap();
        assert_eq!(h.files().len(), 2);
        h.clear(None).unwrap();
        assert!(h.files().is_empty());
    }

    fn render(lines: &[DiffLine]) -> String {
        lines
            .iter()
            .map(|l| {
                let mark = match l.change {
                    Change::Same => ' ',
                    Change::Added => '+',
                    Change::Removed => '-',
                };
                format!("{mark}{}", l.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn line_diff() {
        assert_eq!(render(&diff("a\nb\nc", "a\nb\nc")), " a\n b\n c");
        assert_eq!(render(&diff("a\nb\nc\nd", "a\nx\nc\nd\ne")), " a\n-b\n+x\n c\n d\n+e");
        assert_eq!(render(&diff("", "new")), "+new");
        assert_eq!(render(&diff("gone", "")), "-gone");
        assert_eq!(render(&diff("1\n2\n3", "3\n1")), "-1\n-2\n 3\n+1");
    }
}

//! Merging a sync conflict: a file and the copy sync kept next to it
//! (`Note (충돌 2026-11-14 0305).md`, see `sync::conflict_name`), split
//! into the parts both share and the parts where they differ, so each
//! difference can be settled on its own.

use crate::history::{self, Change};
use regex::Regex;
use std::sync::LazyLock;

#[derive(serde::Serialize, Debug, Clone, PartialEq)]
pub struct ConflictCopy {
    /// The copy sync made.
    pub copy: String,
    /// The file it is a version of.
    pub original: String,
    /// When sync made it ("2026-11-14 0305").
    pub stamp: String,
    /// The copy is this device's own version (it was moved aside when the
    /// other device's file came down); otherwise it is the other device's.
    pub this_device: bool,
}

/// The file `path` is a conflict copy of, if it is one.
pub fn conflict_of(path: &str) -> Option<ConflictCopy> {
    static NAME: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(.*/)?(.+?) \(충돌 ([^)]+)\)(\.[^./]+)?$").unwrap());
    let c = NAME.captures(path)?;
    let dir = c.get(1).map_or("", |m| m.as_str());
    let ext = c.get(4).map_or("", |m| m.as_str());
    let stamp = c[3].to_string();
    Some(ConflictCopy {
        copy: path.to_string(),
        original: format!("{dir}{}{ext}", &c[2]),
        this_device: stamp.ends_with("이 기기"),
        stamp: stamp.trim_end_matches("이 기기").trim().to_string(),
    })
}

/// Lines both versions share, or a place where they differ.
#[derive(serde::Serialize, Debug, Clone, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    Same { lines: Vec<String> },
    /// `original` lines replaced by `copy` lines (either may be empty).
    Differ { original: Vec<String>, copy: Vec<String> },
}

/// Split two versions into shared runs and differences.
pub fn blocks(original: &str, copy: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    for line in history::diff(original, copy) {
        let text = line.text;
        match (line.change, out.last_mut()) {
            (Change::Same, Some(Block::Same { lines })) => lines.push(text),
            (Change::Same, _) => out.push(Block::Same { lines: vec![text] }),
            (Change::Removed, Some(Block::Differ { original, .. })) => original.push(text),
            (Change::Added, Some(Block::Differ { copy, .. })) => copy.push(text),
            (Change::Removed, _) => out.push(Block::Differ { original: vec![text], copy: Vec::new() }),
            (Change::Added, _) => out.push(Block::Differ { original: Vec::new(), copy: vec![text] }),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflict_names_back_to_the_file() {
        let c = conflict_of("Notes/plan (충돌 2026-11-14 0305).md").unwrap();
        assert_eq!((c.original.as_str(), c.stamp.as_str(), c.this_device), ("Notes/plan.md", "2026-11-14 0305", false));
        let c = conflict_of("todo (충돌 2026-11-14 0305 이 기기).md").unwrap();
        assert_eq!((c.original.as_str(), c.stamp.as_str(), c.this_device), ("todo.md", "2026-11-14 0305", true));
        assert_eq!(conflict_of("README (충돌 s)").unwrap().original, "README");
        assert_eq!(conflict_of("a (b) (충돌 s).md").unwrap().original, "a (b).md");
        assert!(conflict_of("Notes/plan.md").is_none());
        // Round trip with the names sync makes.
        let name = crate::sync::conflict_name("Daily/2026-11-14.md", "2026-11-14 0400");
        assert_eq!(conflict_of(&name).unwrap().original, "Daily/2026-11-14.md");
    }

    #[test]
    fn splits_into_shared_and_different_parts() {
        let b = blocks("# 계획\n- 장보기\n- 운동\n끝", "# 계획\n- 장보기 (우유)\n- 운동\n- 독서\n끝");
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(b, [
            Block::Same { lines: s(&["# 계획"]) },
            Block::Differ { original: s(&["- 장보기"]), copy: s(&["- 장보기 (우유)"]) },
            Block::Same { lines: s(&["- 운동"]) },
            Block::Differ { original: vec![], copy: s(&["- 독서"]) },
            Block::Same { lines: s(&["끝"]) },
        ]);
        assert_eq!(blocks("a", "a"), [Block::Same { lines: s(&["a"]) }]);
    }
}

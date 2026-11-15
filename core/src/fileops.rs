//! File management: new folders and notes, renaming and moving files and
//! folders. A move also rewrites every link that would stop reaching its
//! file, like Obsidian's "Automatically update internal links".

use crate::index::{PathTable, VaultIndex};
use crate::links::{LinkStyle, link_spans};
use crate::vault::{self, is_hidden_path};
use std::fs;
use std::path::Path;

/// Characters Windows refuses in file names, plus the ones that break
/// `[[links]]` (`#` heading, `^` block, `|` alias, `[ ]`).
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|', '#', '^', '[', ']'];
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9",
    "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Checks one file or folder name (not a path).
pub fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("이름이 비어 있습니다.".into());
    }
    if name != name.trim() || name.ends_with('.') {
        return Err("이름 앞뒤에 공백을 두거나 마침표로 끝낼 수 없습니다.".into());
    }
    if name.starts_with('.') {
        return Err("마침표로 시작하는 이름은 숨김 파일이 되어 보이지 않습니다.".into());
    }
    if let Some(c) = name.chars().find(|c| FORBIDDEN.contains(c) || c.is_control()) {
        return Err(format!("이름에 \"{c}\" 문자는 쓸 수 없습니다."));
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_lowercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("\"{name}\"은(는) Windows에서 쓸 수 없는 이름입니다."));
    }
    Ok(())
}

/// "A/B/c.md" -> "A/B"; "c.md" -> "".
pub fn parent_of(path: &str) -> &str {
    path.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `folder` + `/` + `name`, with the vault root as "".
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() { name.to_string() } else { format!("{folder}/{name}") }
}

/// The parent folder must be the vault root or an existing folder.
fn check_parent(root: &Path, rel: &str) -> Result<(), String> {
    let parent = parent_of(rel);
    if !parent.is_empty() && !vault::resolve(root, parent)?.is_dir() {
        return Err(format!("폴더가 없습니다: {parent}"));
    }
    Ok(())
}

/// Create an empty folder; fails if anything already has that name.
pub fn create_folder(root: &Path, rel: &str) -> Result<(), String> {
    validate_name(file_name(rel))?;
    if is_hidden_path(rel) {
        return Err("숨김 폴더 안에는 만들 수 없습니다.".into());
    }
    check_parent(root, rel)?;
    let full = vault::resolve(root, rel)?;
    fs::create_dir(&full).map_err(|e| match e.kind() {
        std::io::ErrorKind::AlreadyExists => "같은 이름의 파일이나 폴더가 이미 있습니다.".to_string(),
        _ => format!("폴더를 만들 수 없습니다 ({rel}): {e}"),
    })
}

/// A free path in `folder`: "stem.ext", else "stem 1.ext", "stem 2.ext", …
/// (what Obsidian does for "Untitled").
pub fn unique_child(root: &Path, folder: &str, stem: &str, ext: &str) -> Result<String, String> {
    for i in 0..1000 {
        let name = if i == 0 { format!("{stem}{ext}") } else { format!("{stem} {i}{ext}") };
        let rel = join(folder, &name);
        if fs::symlink_metadata(vault::resolve(root, &rel)?).is_err() {
            return Ok(rel);
        }
    }
    Err("빈 이름을 찾지 못했습니다.".into())
}

/// Something that may be deleted: an existing, visible file or folder.
pub fn check_deletable(root: &Path, rel: &str) -> Result<std::path::PathBuf, String> {
    if rel.is_empty() || is_hidden_path(rel) {
        return Err(format!("지울 수 없는 경로입니다: {rel}"));
    }
    let full = vault::resolve(root, rel)?;
    fs::symlink_metadata(&full).map_err(|_| format!("이미 없습니다: {rel}"))?;
    Ok(full)
}

/// A checked rename/move of one file or folder, with the links to fix.
pub struct MovePlan {
    pub from: String,
    pub to: String,
    /// Notes (current paths) with links that must be rewritten.
    pub relink: Vec<String>,
    before: PathTable,
    after: PathTable,
}

impl MovePlan {
    /// Where `path` ends up after the move (unchanged if it doesn't move).
    pub fn map(&self, path: &str) -> String {
        map_path(&self.from, &self.to, path)
    }
}

fn map_path(from: &str, to: &str, path: &str) -> String {
    let head_matches = path.get(..from.len()).is_some_and(|h| h.to_lowercase() == from.to_lowercase());
    if head_matches && (path.len() == from.len() || path[from.len()..].starts_with('/')) {
        format!("{to}{}", &path[from.len()..])
    } else {
        path.to_string()
    }
}

/// Validate moving `from` to `to` (both vault paths) and work out which
/// notes' links need rewriting. Nothing on disk changes yet.
pub fn plan_move(root: &Path, index: &VaultIndex, from: &str, to: &str) -> Result<MovePlan, String> {
    if from.is_empty() || to.is_empty() || is_hidden_path(from) || is_hidden_path(to) {
        return Err("옮길 수 없는 경로입니다.".into());
    }
    validate_name(file_name(to))?;
    if from == to {
        return Err("이름과 위치가 그대로입니다.".into());
    }
    let from_full = vault::resolve(root, from)?;
    let to_full = vault::resolve(root, to)?;
    let meta = fs::symlink_metadata(&from_full).map_err(|_| format!("이미 없습니다: {from}"))?;
    let case_only = from.to_lowercase() == to.to_lowercase();
    if !case_only && fs::symlink_metadata(&to_full).is_ok() {
        return Err("같은 이름의 파일이나 폴더가 이미 있습니다.".into());
    }
    if to.to_lowercase().starts_with(&format!("{}/", from.to_lowercase())) {
        return Err("폴더를 자기 안으로 옮길 수 없습니다.".into());
    }
    check_parent(root, to)?;

    let before = index.paths().clone();
    let moving: Vec<String> = if meta.is_dir() {
        before.under(from)
    } else {
        before.get(from).map(str::to_string).into_iter().collect()
    };
    let mut after = before.clone();
    for old in &moving {
        after.remove(old);
    }
    for old in &moving {
        after.add(&map_path(from, to, old));
    }

    let mut plan = MovePlan { from: from.to_string(), to: to.to_string(), relink: Vec::new(), before, after };
    for (source, links) in index.note_links() {
        let source_new = plan.map(source);
        let breaks = links.iter().any(|l| {
            plan.before.resolve(source, &l.target).is_some_and(|old| {
                plan.after.resolve(&source_new, &l.target) != Some(plan.map(&old))
            })
        });
        if breaks {
            plan.relink.push(source.to_string());
        }
    }
    Ok(plan)
}

/// `text` of the note at `source` with every link that the move would
/// break rewritten to reach the same file again. `None` if nothing changes.
pub fn relink(plan: &MovePlan, source: &str, text: &str) -> Option<String> {
    let source_new = plan.map(source);
    let mut out = String::with_capacity(text.len() + 32);
    let mut last = 0;
    for span in link_spans(text) {
        let Some(old) = plan.before.resolve(source, &span.link.target) else { continue };
        let target = plan.map(&old);
        if plan.after.resolve(&source_new, &span.link.target).as_deref() == Some(target.as_str()) {
            continue;
        }
        let written = &text[span.range.clone()];
        out.push_str(&text[last..span.range.start]);
        out.push_str(&link_text(&plan.after, &source_new, &target, written, span.style));
        last = span.range.end;
    }
    (last > 0).then(|| {
        out.push_str(&text[last..]);
        out
    })
}

/// How to write a link from `source` to `target` (both after the move),
/// keeping the original's style: bare name when it still resolves, else
/// the full vault path; `.md` only if the original had it.
fn link_text(after: &PathTable, source: &str, target: &str, written: &str, style: LinkStyle) -> String {
    let decoded = written.replace("%20", " ");
    let is_note = target.to_ascii_lowercase().ends_with(".md");
    let keep_ext = !is_note || decoded.to_ascii_lowercase().ends_with(".md");
    let shape = |p: &str| if keep_ext { p.to_string() } else { p[..p.len() - 3].to_string() };
    let reaches = |t: &str| after.resolve(source, t).as_deref() == Some(target);

    match style {
        LinkStyle::Wiki => {
            if !decoded.contains('/') {
                let name = shape(file_name(target));
                if reaches(&name) {
                    return name;
                }
            }
            shape(target)
        }
        LinkStyle::Markdown | LinkStyle::MarkdownAngle => {
            let rel = shape(&relative(parent_of(source), target));
            let chosen = if reaches(&rel) { rel } else { shape(target) };
            if style == LinkStyle::Markdown { chosen.replace(' ', "%20") } else { chosen }
        }
    }
}

/// Path of `to` relative to folder `from_dir` ("A/B", "A/C/x.md" -> "../C/x.md").
fn relative(from_dir: &str, to: &str) -> String {
    let from: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
    let to_parts: Vec<&str> = to.split('/').collect();
    let common = from
        .iter()
        .zip(&to_parts[..to_parts.len() - 1])
        .take_while(|(a, b)| a == b)
        .count();
    let mut out = "../".repeat(from.len() - common);
    out.push_str(&to_parts[common..].join("/"));
    out
}

/// Notes rewritten by a move: new path and new content.
pub struct MoveOutcome {
    pub rewritten: Vec<(String, String)>,
    /// Notes whose links could not be updated (read/write failed).
    pub failed: Vec<String>,
}

/// Move on disk, then fix the links planned in `plan`.
pub fn apply_move(root: &Path, plan: &MovePlan) -> Result<MoveOutcome, String> {
    let from = vault::resolve(root, &plan.from)?;
    let to = vault::resolve(root, &plan.to)?;
    fs::rename(&from, &to).map_err(|e| format!("옮기지 못했습니다 ({} → {}): {e}", plan.from, plan.to))?;

    let mut outcome = MoveOutcome { rewritten: Vec::new(), failed: Vec::new() };
    for source in &plan.relink {
        let path = plan.map(source);
        // Re-read from disk: the index copy may lag behind the watcher.
        let result = vault::read_text(root, &path).and_then(|text| match relink(plan, source, &text) {
            Some(new_text) => vault::write_text(root, &path, &new_text).map(|_| Some(new_text)),
            None => Ok(None),
        });
        match result {
            Ok(Some(new_text)) => outcome.rewritten.push((path, new_text)),
            Ok(None) => {}
            Err(_) => outcome.failed.push(path),
        }
    }
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault_of(files: &[(&str, &str)]) -> (tempfile::TempDir, VaultIndex) {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, content).unwrap();
        }
        let index = VaultIndex::build(dir.path()).unwrap();
        (dir, index)
    }

    fn do_move(files: &[(&str, &str)], from: &str, to: &str) -> (tempfile::TempDir, MoveOutcome) {
        let (dir, index) = vault_of(files);
        let plan = plan_move(dir.path(), &index, from, to).unwrap();
        let outcome = apply_move(dir.path(), &plan).unwrap();
        (dir, outcome)
    }

    fn read(dir: &tempfile::TempDir, rel: &str) -> String {
        fs::read_to_string(dir.path().join(rel)).unwrap()
    }

    #[test]
    fn rename_rewrites_links_and_keeps_their_style() {
        let (dir, out) = do_move(
            &[
                ("Old.md", "# old"),
                ("A/Linker.md", "[[Old]] [[Old|별칭]] ![[Old#Head]] [[old.md]] [[Old^b]]\n[x](../Old.md) [y](<../Old.md>)\n`[[Old]]`"),
                ("Untouched.md", "[[Other]] no link to old"),
            ],
            "Old.md",
            "New Name.md",
        );
        assert_eq!(
            read(&dir, "A/Linker.md"),
            "[[New Name]] [[New Name|별칭]] ![[New Name#Head]] [[New Name.md]] [[New Name^b]]\n[x](../New%20Name.md) [y](<../New Name.md>)\n`[[Old]]`"
        );
        assert_eq!(read(&dir, "New Name.md"), "# old");
        assert!(!dir.path().join("Old.md").exists());
        let paths: Vec<_> = out.rewritten.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, ["A/Linker.md"]);
    }

    #[test]
    fn move_keeps_bare_names_and_fixes_paths() {
        let (dir, _) = do_move(
            &[
                ("Inbox/Note.md", "[sib](Sibling.md) [[Sibling]] ![[img/pic.png]]"),
                ("Inbox/Sibling.md", ""),
                // a shorter namesake: once Note.md leaves Inbox, "Sibling" would mean this one
                ("A/Sibling.md", ""),
                ("img/pic.png", ""),
                ("Archive/readme.txt", ""),
                ("Other.md", "[[Note]] [[Inbox/Note]] [m](Inbox/Note.md)"),
            ],
            "Inbox/Note.md",
            "Archive/Note.md",
        );
        // bare name still unique: untouched; paths follow the move
        assert_eq!(read(&dir, "Other.md"), "[[Note]] [[Archive/Note]] [m](Archive/Note.md)");
        // the moved note's own links that relied on its old folder are re-pointed
        assert_eq!(read(&dir, "Archive/Note.md"), "[sib](../Inbox/Sibling.md) [[Inbox/Sibling]] ![[img/pic.png]]");
    }

    #[test]
    fn ambiguous_name_falls_back_to_full_path() {
        let (dir, _) = do_move(
            &[("A/Note.md", ""), ("B/Other.md", ""), ("Linker.md", "[[Other]] [[Note]]")],
            "B/Other.md",
            "B/Note.md",
        );
        // "Note" now means A/Note.md (same length, A sorts first)
        assert_eq!(read(&dir, "Linker.md"), "[[B/Note]] [[Note]]");
    }

    #[test]
    fn folder_rename_moves_everything_inside() {
        let (dir, out) = do_move(
            &[
                ("Proj/Plan.md", "[[Proj/Sub/Deep]] [[Deep]]"),
                ("Proj/Sub/Deep.md", "[up](../Plan.md)"),
                ("Index.md", "[[Proj/Plan]] [[Plan]] ![[Proj/Sub/Deep]]"),
            ],
            "Proj",
            "Projects 2026",
        );
        assert_eq!(read(&dir, "Index.md"), "[[Projects 2026/Plan]] [[Plan]] ![[Projects 2026/Sub/Deep]]");
        assert_eq!(read(&dir, "Projects 2026/Plan.md"), "[[Projects 2026/Sub/Deep]] [[Deep]]");
        // relative link inside the folder still reaches its target: untouched
        assert_eq!(read(&dir, "Projects 2026/Sub/Deep.md"), "[up](../Plan.md)");
        assert_eq!(out.rewritten.len(), 2);
    }

    #[test]
    fn case_only_rename_is_allowed() {
        let (dir, _) = do_move(&[("note.md", "x"), ("L.md", "[[note]]")], "note.md", "Note.md");
        let names: Vec<_> = fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert!(names.iter().any(|n| n == "Note.md"));
        // still resolves case-insensitively, so the link is left alone
        assert_eq!(read(&dir, "L.md"), "[[note]]");
    }

    #[test]
    fn plan_rejects_bad_moves() {
        let (dir, index) = vault_of(&[("A/x.md", ""), ("B.md", ""), (".obsidian/app.json", "{}")]);
        let r = dir.path();
        assert!(plan_move(r, &index, "A/x.md", "B.md").is_err()); // exists
        assert!(plan_move(r, &index, "A", "A/Inner").is_err()); // into itself
        assert!(plan_move(r, &index, "A/x.md", "Nope/x.md").is_err()); // no such folder
        assert!(plan_move(r, &index, "A/x.md", "A/a:b.md").is_err()); // bad name
        assert!(plan_move(r, &index, "missing.md", "m.md").is_err());
        assert!(plan_move(r, &index, "B.md", ".obsidian/B.md").is_err()); // hidden
        assert!(plan_move(r, &index, "B.md", "../B.md").is_err()); // escape
        assert!(plan_move(r, &index, "A/x.md", "x.md").is_ok()); // to root
    }

    #[test]
    fn names_are_checked() {
        for bad in ["", " a", "a ", "a.", ".hidden", "a/b", "a#b", "a|b", "a[b", "CON", "nul.md", "a\tb"] {
            assert!(validate_name(bad).is_err(), "{bad:?} should be rejected");
        }
        for ok in ["회의록", "2026-11-13", "My Note.md", "a.b.c", "con-call"] {
            assert!(validate_name(ok).is_ok(), "{ok:?} should be fine");
        }
    }

    #[test]
    fn new_folders_and_unique_names() {
        let (dir, _) = vault_of(&[("제목 없음.md", ""), ("제목 없음 1.md", "")]);
        let r = dir.path();
        assert_eq!(unique_child(r, "", "제목 없음", ".md").unwrap(), "제목 없음 2.md");
        create_folder(r, "새 폴더").unwrap();
        assert!(r.join("새 폴더").is_dir());
        assert!(create_folder(r, "새 폴더").is_err());
        assert!(create_folder(r, "없는/폴더").is_err());
        assert_eq!(unique_child(r, "새 폴더", "제목 없음", ".md").unwrap(), "새 폴더/제목 없음.md");
        assert!(check_deletable(r, "새 폴더").is_ok());
        assert!(check_deletable(r, "nope.md").is_err());
        assert!(check_deletable(r, ".obsidian").is_err());
    }

    #[test]
    fn relative_paths() {
        assert_eq!(relative("", "A/x.md"), "A/x.md");
        assert_eq!(relative("A", "A/x.md"), "x.md");
        assert_eq!(relative("A/B", "A/C/x.md"), "../C/x.md");
        assert_eq!(relative("A/B", "x.md"), "../../x.md");
    }
}

//! Vault access: an Obsidian-compatible vault is just a folder of files.
//! The files on disk are always the source of truth.

use serde::Serialize;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TreeNode {
    pub name: String,
    /// Path relative to the vault root, always with `/` separators.
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

/// Names hidden from the tree, matching Obsidian's behaviour
/// (`.obsidian`, `.git`, `.trash`, dotfiles in general).
fn is_hidden(name: &str) -> bool {
    name.starts_with('.')
}

/// True if any component of a vault-relative `/` path is hidden.
pub fn is_hidden_path(rel: &str) -> bool {
    rel.split('/').any(is_hidden)
}

/// Build the file tree of `root`. Directories come first, then files,
/// each sorted case-insensitively. Symlinks are skipped to avoid cycles.
pub fn build_tree(root: &Path) -> io::Result<Vec<TreeNode>> {
    walk(root, root)
}

fn walk(root: &Path, dir: &Path) -> io::Result<Vec<TreeNode>> {
    let mut nodes = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_hidden(&name) {
            continue;
        }
        let full = entry.path();
        let path = to_rel_string(root, &full);
        if file_type.is_dir() {
            nodes.push(TreeNode {
                name,
                path,
                is_dir: true,
                children: walk(root, &full)?,
            });
        } else if file_type.is_file() {
            nodes.push(TreeNode {
                name,
                path,
                is_dir: false,
                children: Vec::new(),
            });
        }
    }
    nodes.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(nodes)
}

fn to_rel_string(root: &Path, full: &Path) -> String {
    full.strip_prefix(root)
        .unwrap_or(full)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Resolve a vault-relative path, rejecting anything that could escape the
/// vault (absolute paths, drive prefixes, `..`).
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel_path = Path::new(rel);
    if rel.is_empty() {
        return Err("empty path".into());
    }
    for c in rel_path.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(format!("path escapes the vault: {rel}")),
        }
    }
    Ok(root.join(rel_path))
}

pub fn read_text(root: &Path, rel: &str) -> Result<String, String> {
    let path = resolve(root, rel)?;
    fs::read_to_string(&path).map_err(|e| format!("cannot read {rel}: {e}"))
}

/// The file's text, or `None` if there is no such file. Any other failure
/// (not UTF-8, locked by another program, …) is an error, so callers never
/// mistake an unreadable note for an empty one and overwrite it.
pub fn read_if_exists(root: &Path, rel: &str) -> Result<Option<String>, String> {
    let path = resolve(root, rel)?;
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {rel}: {e}")),
    }
}

/// Same text apart from line endings (`\r\n` vs `\n`).
pub fn same_text(a: &str, b: &str) -> bool {
    a == b || a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

/// `new` with the line endings `old` uses. The editor always hands back `\n`,
/// so without this every save would turn a CRLF note into an LF one (and Git
/// would see every line as changed). Text that already has `\r` is left alone.
pub fn keep_line_endings(old: &str, new: String) -> String {
    if old.contains("\r\n") && !new.contains('\r') {
        new.replace('\n', "\r\n")
    } else {
        new
    }
}

/// What saving over a file should do.
#[derive(Debug, PartialEq, Eq)]
pub enum SavePlan {
    /// Write this text (line endings already matched to the file).
    Write(String),
    /// The file changed elsewhere since the editor last read it: writing
    /// would throw that change away.
    Conflict,
}

/// Decide how to save `content` over a file that holds `disk` now, when the
/// editor last saw `expected` there (`None`: don't check).
pub fn plan_save(disk: Option<&str>, content: &str, expected: Option<&str>) -> SavePlan {
    match (disk, expected) {
        (Some(disk), Some(expected)) if !same_text(disk, expected) => SavePlan::Conflict,
        (Some(disk), _) => SavePlan::Write(keep_line_endings(disk, content.to_string())),
        // Gone (deleted elsewhere): writing brings the unsaved text back.
        (None, _) => SavePlan::Write(content.to_string()),
    }
}

/// Keep `content` next to `rel` as a conflict copy, `name (충돌 <stamp> 이 기기).ext`
/// like sync does for this device's side, never overwriting a file.
/// Returns the copy's path.
pub fn create_conflict_copy(root: &Path, rel: &str, stamp: &str, content: &str) -> Result<String, String> {
    let mut last_err = String::new();
    for n in 1..=50 {
        // "이 기기" stays last: that is how a copy is known to be this device's.
        let label = if n == 1 { format!("{stamp} 이 기기") } else { format!("{stamp} {n} 이 기기") };
        let copy = crate::sync::conflict_name(rel, &label);
        match create_text(root, &copy, content) {
            Ok(()) => return Ok(copy),
            Err(e) => {
                if !resolve(root, &copy)?.exists() {
                    return Err(e);
                }
                last_err = e;
            }
        }
    }
    Err(last_err)
}

/// Write via a temp file + rename so a crash or a concurrent reader
/// (e.g. Obsidian open on the same vault) never sees a half-written note.
pub fn write_text(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    let path = resolve(root, rel)?;
    let parent = path
        .parent()
        .ok_or_else(|| format!("invalid path: {rel}"))?;
    fs::create_dir_all(parent).map_err(|e| format!("cannot create folder for {rel}: {e}"))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("invalid path: {rel}"))?
        .to_string_lossy();
    let tmp = parent.join(format!(".{file_name}.rusidian-tmp"));
    let born = fs::metadata(&path).and_then(|m| m.created()).ok();
    fs::write(&tmp, content).map_err(|e| format!("cannot write {rel}: {e}"))?;
    fs::rename(&tmp, &path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot save {rel}: {e}")
    })?;
    keep_created(&path, born);
    Ok(())
}

/// The replacing file is a new file: give it the old one's creation time
/// (Windows usually does this itself, but not when tunnelling is off).
#[cfg(windows)]
fn keep_created(path: &Path, born: Option<std::time::SystemTime>) {
    use std::os::windows::fs::FileTimesExt;
    let Some(born) = born else { return };
    if fs::metadata(path).and_then(|m| m.created()).is_ok_and(|now| now == born) {
        return;
    }
    if let Ok(f) = fs::OpenOptions::new().write(true).open(path) {
        let _ = f.set_times(fs::FileTimes::new().set_created(born));
    }
}

#[cfg(not(windows))]
fn keep_created(_path: &Path, _born: Option<std::time::SystemTime>) {}

/// When a file was made and last changed (Unix ms; None where unknown).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FileTimes {
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub size: u64,
}

pub fn file_times(root: &Path, rel: &str) -> Result<FileTimes, String> {
    let meta = fs::metadata(resolve(root, rel)?).map_err(|e| format!("cannot read {rel}: {e}"))?;
    let ms = |t: io::Result<std::time::SystemTime>| {
        t.ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
    };
    Ok(FileTimes { created: ms(meta.created()), modified: ms(meta.modified()), size: meta.len() })
}

/// Create a new file; fails if it already exists (never overwrites).
pub fn create_text(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    use std::io::Write;
    let path = resolve(root, rel)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("cannot create folder for {rel}: {e}"))?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("cannot create {rel}: {e}"))?;
    file.write_all(content.as_bytes())
        .map_err(|e| format!("cannot write {rel}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saving_keeps_the_creation_time() {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        write_text(r, "a.md", "one").unwrap();
        let born = fs::metadata(r.join("a.md")).unwrap().created().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1200));
        write_text(r, "a.md", "two").unwrap();
        let meta = fs::metadata(r.join("a.md")).unwrap();
        assert_eq!(meta.created().unwrap(), born);
        assert!(meta.modified().unwrap() > born);
        let t = file_times(r, "a.md").unwrap();
        assert!(t.created.unwrap() < t.modified.unwrap());
        assert_eq!(t.size, 3);
        assert!(file_times(r, "none.md").is_err());
    }

    fn sample_vault() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        fs::create_dir_all(r.join(".obsidian")).unwrap();
        fs::write(r.join(".obsidian/app.json"), "{}").unwrap();
        fs::create_dir_all(r.join("Projects/Sub")).unwrap();
        fs::write(r.join("Projects/Sub/deep.md"), "deep").unwrap();
        fs::write(r.join("Projects/plan.md"), "plan").unwrap();
        fs::write(r.join("b note.md"), "b").unwrap();
        fs::write(r.join("A note.md"), "a").unwrap();
        dir
    }

    #[test]
    fn tree_hides_dotfolders_and_sorts_dirs_first() {
        let v = sample_vault();
        let tree = build_tree(v.path()).unwrap();
        let names: Vec<_> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Projects", "A note.md", "b note.md"]);
        let projects = &tree[0];
        assert_eq!(projects.children[0].name, "Sub");
        assert_eq!(projects.children[0].children[0].path, "Projects/Sub/deep.md");
        assert_eq!(projects.children[1].path, "Projects/plan.md");
    }

    #[test]
    fn resolve_rejects_escapes() {
        let v = sample_vault();
        assert!(resolve(v.path(), "../outside.md").is_err());
        assert!(resolve(v.path(), "Projects/../../x.md").is_err());
        assert!(resolve(v.path(), "").is_err());
        #[cfg(windows)]
        assert!(resolve(v.path(), "C:\\Windows\\win.ini").is_err());
        #[cfg(unix)]
        assert!(resolve(v.path(), "/etc/passwd").is_err());
        assert!(resolve(v.path(), "Projects/plan.md").is_ok());
    }

    #[test]
    fn create_never_overwrites() {
        let v = sample_vault();
        create_text(v.path(), "Fresh/new note.md", "").unwrap();
        assert_eq!(read_text(v.path(), "Fresh/new note.md").unwrap(), "");
        assert!(create_text(v.path(), "A note.md", "clobber").is_err());
        assert_eq!(read_text(v.path(), "A note.md").unwrap(), "a");
        assert!(create_text(v.path(), "../escape.md", "").is_err());
    }

    #[test]
    fn write_then_read_roundtrip_leaves_no_temp_file() {
        let v = sample_vault();
        write_text(v.path(), "New/folder/note.md", "# hi\n").unwrap();
        assert_eq!(read_text(v.path(), "New/folder/note.md").unwrap(), "# hi\n");
        write_text(v.path(), "New/folder/note.md", "# changed\n").unwrap();
        assert_eq!(read_text(v.path(), "New/folder/note.md").unwrap(), "# changed\n");
        let leftovers: Vec<_> = fs::read_dir(v.path().join("New/folder"))
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn missing_is_none_but_unreadable_is_an_error() {
        let v = sample_vault();
        assert_eq!(read_if_exists(v.path(), "A note.md").unwrap().as_deref(), Some("a"));
        assert_eq!(read_if_exists(v.path(), "nope.md").unwrap(), None);
        // CP949 "자산" is not UTF-8: must not read as "no file".
        fs::write(v.path().join("cp949.md"), [0xC0u8, 0xDA, 0xBB, 0xEA]).unwrap();
        assert!(read_if_exists(v.path(), "cp949.md").is_err());
    }

    #[test]
    fn saving_keeps_crlf() {
        assert_eq!(keep_line_endings("a\r\nb\r\n", "a\nb\nc\n".into()), "a\r\nb\r\nc\r\n");
        assert_eq!(keep_line_endings("a\nb\n", "a\nc\n".into()), "a\nc\n");
        assert_eq!(keep_line_endings("a\r\n", "x\r\ny".into()), "x\r\ny");
        assert_eq!(plan_save(Some("a\r\nb"), "a\nc", Some("a\nb")), SavePlan::Write("a\r\nc".into()));
    }

    #[test]
    fn saving_over_an_outside_change_is_a_conflict() {
        // Unchanged since read (line endings aside): write.
        assert_eq!(plan_save(Some("old"), "new", Some("old")), SavePlan::Write("new".into()));
        // Changed elsewhere (a sync pull, Obsidian): don't write.
        assert_eq!(plan_save(Some("theirs"), "mine", Some("old")), SavePlan::Conflict);
        // No check asked, or the file is gone: write.
        assert_eq!(plan_save(Some("theirs"), "mine", None), SavePlan::Write("mine".into()));
        assert_eq!(plan_save(None, "mine", Some("old")), SavePlan::Write("mine".into()));
    }

    #[test]
    fn conflict_copies_never_overwrite() {
        let v = sample_vault();
        let a = create_conflict_copy(v.path(), "Projects/plan.md", "2026-11-15 1730", "mine").unwrap();
        assert_eq!(a, "Projects/plan (충돌 2026-11-15 1730 이 기기).md");
        let b = create_conflict_copy(v.path(), "Projects/plan.md", "2026-11-15 1730", "again").unwrap();
        assert_eq!(b, "Projects/plan (충돌 2026-11-15 1730 2 이 기기).md");
        assert_eq!(read_text(v.path(), &a).unwrap(), "mine");
        assert_eq!(read_text(v.path(), &b).unwrap(), "again");
        let c = crate::merge::conflict_of(&b).unwrap();
        assert_eq!(c.original, "Projects/plan.md");
        assert!(c.this_device);
    }
}

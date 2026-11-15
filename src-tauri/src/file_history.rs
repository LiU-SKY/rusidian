//! File recovery commands. Versions live in this computer's app data
//! folder (`history/<vault>/…`), never in the vault, so they are not synced.

use crate::{current, vault_key, write_index, AppState};
use rusidian_core::history::{self, DiffLine, History, Version};
use rusidian_core::vault;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager, State};

static BASE: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup.
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().app_local_data_dir() {
        let _ = BASE.set(dir.join("history"));
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn for_vault(root: &Path) -> Option<History> {
    let base = BASE.get()?;
    Some(History::new(base.join(format!("{:016x}", history::hash(&vault_key(root))))))
}

/// Keep what `rel` holds on disk now (nothing if it doesn't exist yet).
/// Failures never stop the save or delete that follows.
fn keep_disk(root: &Path, rel: &str, force: bool) {
    if !history::is_note(rel) {
        return;
    }
    if let (Some(h), Ok(old)) = (for_vault(root), vault::read_text(root, rel))
        && let Err(e) = h.keep(rel, &old, now_ms(), force)
    {
        eprintln!("file history: {e}");
    }
}

/// Write a note, keeping its previous text first (at most every few minutes).
pub fn save_text(root: &Path, rel: &str, content: &str) -> Result<(), String> {
    keep_disk(root, rel, false);
    vault::write_text(root, rel, content)
}

/// A file or folder is about to be deleted: keep every note in it.
pub fn before_delete(root: &Path, rel: &str) {
    let Ok(full) = vault::resolve(root, rel) else { return };
    if !full.is_dir() {
        keep_disk(root, rel, true);
        return;
    }
    let mut stack = vec![full];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else { continue };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(sub) = path.strip_prefix(root) {
                let sub = sub.to_string_lossy().replace('\\', "/");
                keep_disk(root, &sub, true);
            }
        }
    }
}

/// Versions follow a renamed or moved file or folder.
pub fn moved(root: &Path, from: &str, to: &str) {
    if let Some(h) = for_vault(root) {
        h.moved(from, to);
    }
}

/// Drop old versions, off the calling thread.
pub fn prune(root: &Path) {
    if let Some(h) = for_vault(root) {
        std::thread::spawn(move || h.prune(now_ms(), history::KEEP_MS));
    }
}

fn open(state: &AppState, label: &str) -> Result<(PathBuf, History), String> {
    let (root, _) = current(state, label)?;
    let h = for_vault(&root).ok_or("이전 버전을 둘 폴더를 찾을 수 없습니다.")?;
    Ok((root, h))
}

#[derive(serde::Serialize)]
pub struct HistoryFile {
    path: String,
    latest: i64,
    count: usize,
    /// False when the note has been deleted or moved elsewhere.
    exists: bool,
}

/// Every note with earlier versions, newest change first.
#[tauri::command]
pub fn history_files(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<HistoryFile>, String> {
    let (root, h) = open(&state, window.label())?;
    Ok(h.files()
        .into_iter()
        .map(|f| HistoryFile {
            exists: vault::resolve(&root, &f.path).is_ok_and(|p| p.is_file()),
            path: f.path,
            latest: f.latest,
            count: f.count,
        })
        .collect())
}

#[tauri::command]
pub fn history_versions(state: State<AppState>, window: tauri::WebviewWindow, path: String) -> Result<Vec<Version>, String> {
    Ok(open(&state, window.label())?.1.versions(&path))
}

#[tauri::command]
pub fn history_read(state: State<AppState>, window: tauri::WebviewWindow, path: String, id: i64) -> Result<String, String> {
    open(&state, window.label())?.1.read(&path, id)
}

/// What changed from version `id` to the note as it is now. If the note
/// is gone, every line shows as removed.
#[tauri::command]
pub fn history_diff(state: State<AppState>, window: tauri::WebviewWindow, path: String, id: i64) -> Result<Vec<DiffLine>, String> {
    let (root, h) = open(&state, window.label())?;
    let old = h.read(&path, id)?;
    let now = vault::read_text(&root, &path).unwrap_or_default();
    Ok(history::diff(&old, &now))
}

/// Put version `id` back (the current text is kept as a version first).
/// A deleted note is created again.
#[tauri::command]
pub fn history_restore(state: State<AppState>, window: tauri::WebviewWindow, path: String, id: i64) -> Result<(), String> {
    let (root, index) = current(&state, window.label())?;
    let h = for_vault(&root).ok_or("이전 버전을 둘 폴더를 찾을 수 없습니다.")?;
    let text = h.read(&path, id)?;
    keep_disk(&root, &path, true);
    vault::write_text(&root, &path, &text)?;
    write_index(&index).upsert_note(&path, text);
    Ok(())
}

/// Forget the versions of one note (None: of the whole vault).
#[tauri::command]
pub fn history_clear(state: State<AppState>, window: tauri::WebviewWindow, path: Option<String>) -> Result<(), String> {
    open(&state, window.label())?.1.clear(path.as_deref())
}

use rusidian_core::date::Date;
use rusidian_core::index::{Backlink, SearchHit, TagCount, TaskItem, VaultIndex};
use rusidian_core::watch::{self, VaultWatcher};
use rusidian_core::calendar::{self, Calendar};
use rusidian_core::finance::{self, EntryKind, FinanceData};
use rusidian_core::graph::{self, Graph};
use rusidian_core::sync::{self, SyncConfig};
use rusidian_core::{daily, everything, fileops, finance_import, frontmatter, gitsync, note_settings, tasks, templates, vault};
use std::fs;
use std::path::{Path, PathBuf};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, RwLockReadGuard, RwLockWriteGuard};
use tauri::{AppHandle, Emitter, Manager, State};
use vault::TreeNode;

/// Emitted with the changed vault-relative paths when files change on disk.
const VAULT_CHANGED_EVENT: &str = "vault-changed";
const SEARCH_LIMIT: usize = 200;

struct OpenVault {
    root: PathBuf,
    index: Arc<RwLock<VaultIndex>>,
    /// Dropping it (e.g. when another vault is opened) stops watching.
    _watcher: Option<VaultWatcher>,
}

mod file_history;
mod phone_inbox;
mod storage;
mod subscriptions;
use rusidian_core::calendars::{self, CalendarSource};
use rusidian_core::recent as vaults;
use subscriptions::IcsCache;

/// The window the app starts with (see tauri.conf.json); it reopens the last vault.
const MAIN_WINDOW: &str = "main";

#[derive(Default)]
struct AppState {
    /// The vault open in each window, by window label (Obsidian style:
    /// one vault per window).
    vaults: Mutex<HashMap<String, OpenVault>>,
    /// Vault a new window should open first, by window label.
    pending: Mutex<HashMap<String, String>>,
    /// Numbers the labels of new windows.
    next_window: AtomicUsize,
    /// Downloaded external calendars (see `subscriptions.rs`).
    ics: Arc<IcsCache>,
    /// Vaults being synced right now (one sync at a time per vault).
    syncing: Mutex<std::collections::HashSet<PathBuf>>,
    /// Paths of each window's last few Everything searches: `everything_open`
    /// opens only these (and never runs a program, see `everything::runs_code`).
    everything_hits: Mutex<HashMap<String, std::collections::VecDeque<std::collections::HashSet<String>>>>,
}

#[derive(serde::Serialize)]
struct VaultInfo {
    root: String,
    name: String,
    tree: Vec<TreeNode>,
    note_count: usize,
    /// False if the OS refused to watch the folder; edits made elsewhere
    /// then only show up after a manual refresh.
    watching: bool,
}

const SETTINGS_FILE: &str = "settings.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Settings {
    last_vault: Option<String>,
    /// Vaults opened before, newest first.
    #[serde(default)]
    recent_vaults: Vec<vaults::RecentVault>,
    /// External calendar subscriptions (kept here, not in the vault).
    #[serde(default)]
    calendars: Vec<CalendarSource>,
    /// Git sync of each vault on this device (remote, branch, interval),
    /// by vault folder. Which folders sync is shared, in the vault.
    #[serde(default)]
    sync: HashMap<String, DeviceSync>,
    /// Phone notifications read into each vault on this device, by vault folder.
    #[serde(default)]
    phone: HashMap<String, phone_inbox::PhoneSetup>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
struct DeviceSync {
    remote: String,
    branch: String,
    #[serde(default)]
    interval_minutes: u32,
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(SETTINGS_FILE))
}

fn load_settings(app: &AppHandle) -> Settings {
    settings_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_settings(app: &AppHandle, settings: &Settings) {
    if let Some(p) = settings_path(app) {
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = fs::write(p, json);
        }
    }
}

/// Root and index of the vault open in window `label`.
fn current(state: &AppState, label: &str) -> Result<(PathBuf, Arc<RwLock<VaultIndex>>), String> {
    let guard = state.vaults.lock().map_err(|_| "state poisoned".to_string())?;
    let v = guard.get(label).ok_or_else(|| "이 창에 열린 vault가 없습니다.".to_string())?;
    Ok((v.root.clone(), v.index.clone()))
}

fn read_index(index: &RwLock<VaultIndex>) -> RwLockReadGuard<'_, VaultIndex> {
    index.read().unwrap_or_else(|p| p.into_inner())
}

fn write_index(index: &RwLock<VaultIndex>) -> RwLockWriteGuard<'_, VaultIndex> {
    index.write().unwrap_or_else(|p| p.into_inner())
}

/// Opens a vault: builds the tree and index off the UI thread, then starts
/// watching the folder for outside changes.
#[tauri::command]
async fn open_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    path: String,
) -> Result<VaultInfo, String> {
    let root = dunce::canonicalize(&path).map_err(|e| format!("vault 폴더를 열 수 없습니다: {path} ({e})"))?;
    if !root.is_dir() {
        return Err(format!("폴더가 아닙니다: {path}"));
    }
    let build_root = root.clone();
    let (tree, index) = tauri::async_runtime::spawn_blocking(move || {
        let tree = vault::build_tree(&build_root).map_err(|e| format!("vault를 읽을 수 없습니다: {e}"))?;
        let index = VaultIndex::build(&build_root).map_err(|e| format!("vault 색인을 만들 수 없습니다: {e}"))?;
        Ok::<_, String>((tree, index))
    })
    .await
    .map_err(|e| e.to_string())??;

    let note_count = index.note_count();
    file_history::prune(&root);
    let index = Arc::new(RwLock::new(index));
    let emitter = app.clone();
    let label = window.label().to_string();
    // File changes go only to the window that has this vault open.
    let target = label.clone();
    let watcher = watch::watch(root.clone(), index.clone(), move |paths| {
        let _ = emitter.emit_to(target.as_str(), VAULT_CHANGED_EVENT, paths);
    })
    .inspect_err(|e| eprintln!("vault watcher unavailable: {e}"))
    .ok();
    let watching = watcher.is_some();

    let name = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    // Replacing the previous vault of this window drops its watcher.
    state
        .vaults
        .lock()
        .map_err(|_| "state poisoned")?
        .insert(label.clone(), OpenVault { root, index, _watcher: watcher });
    let mut settings = load_settings(&app);
    // The main window reopens its vault next time; other windows don't change that.
    if label == MAIN_WINDOW || settings.last_vault.is_none() {
        settings.last_vault = Some(path.clone());
    }
    vaults::remember(&mut settings.recent_vaults, &path, vaults::now());
    save_settings(&app, &settings);

    Ok(VaultInfo { root: path, name, tree, note_count, watching })
}

// ---------------------------------------------------------------- windows

/// The vault a window opens when it starts: the one it was created for,
/// else (only the main window) the last vault.
#[tauri::command]
fn startup_vault(app: AppHandle, state: State<AppState>, window: tauri::WebviewWindow) -> Option<String> {
    if let Some(path) = state.pending.lock().ok().and_then(|mut p| p.remove(window.label())) {
        return Some(path);
    }
    (window.label() == MAIN_WINDOW).then(|| load_settings(&app).last_vault).flatten()
}

/// Bring forward the window (other than `except`) that has `path` open.
fn focus_existing(app: &AppHandle, state: &AppState, except: &str, path: &str) -> bool {
    let Ok(root) = dunce::canonicalize(path) else { return false };
    let label = state
        .vaults
        .lock()
        .ok()
        .and_then(|v| v.iter().find(|(l, open)| l.as_str() != except && open.root == root).map(|(l, _)| l.clone()));
    let Some(window) = label.and_then(|l| app.get_webview_window(&l)) else { return false };
    #[cfg(desktop)]
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    true
}

/// If another window already has `path` open, bring it forward (true).
#[tauri::command]
fn focus_vault_window(app: AppHandle, state: State<AppState>, window: tauri::WebviewWindow, path: String) -> bool {
    focus_existing(&app, &state, window.label(), &path)
}

/// Open a new window, empty or for `path` (a window that already has that
/// vault is brought forward instead). Async: creating a window from a
/// synchronous command can deadlock on Windows.
#[tauri::command]
async fn open_window(app: AppHandle, state: State<'_, AppState>, path: Option<String>) -> Result<(), String> {
    if let Some(p) = &path {
        if focus_existing(&app, &state, "", p) {
            return Ok(());
        }
    }
    let label = format!("vault-{}", state.next_window.fetch_add(1, Ordering::Relaxed) + 1);
    if let Some(p) = path {
        state.pending.lock().map_err(|_| "state poisoned")?.insert(label.clone(), p);
    }
    new_window(&app, &label)
}

#[cfg(desktop)]
fn new_window(app: &AppHandle, label: &str) -> Result<(), String> {
    // Same look as the main window in tauri.conf.json.
    // "index.html" is the app's root (the dev server in development).
    tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::App("index.html".into()))
        .title("Rusidian")
        .inner_size(1200.0, 800.0)
        .min_inner_size(600.0, 400.0)
        .decorations(false)
        .disable_drag_drop_handler()
        .build()
        .map_err(|e| format!("창을 열 수 없습니다: {e}"))?;
    Ok(())
}

/// A phone app has one window.
#[cfg(mobile)]
fn new_window(_app: &AppHandle, _label: &str) -> Result<(), String> {
    Err("이 기기에서는 새 창을 열 수 없습니다".into())
}

/// The phone app's vault: a folder in the app's own storage, made (with a
/// first note) on first use. Phones have no folder picker that gives a
/// plain path, so this is where the app keeps notes for now.
#[tauri::command]
fn device_vault(app: AppHandle) -> Result<String, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("Vault");
    std::fs::create_dir_all(&dir).map_err(|e| format!("vault 폴더를 만들 수 없습니다: {e}"))?;
    let empty = std::fs::read_dir(&dir).map_err(|e| e.to_string())?.next().is_none();
    if empty {
        std::fs::write(dir.join("Rusidian.md"), "# Rusidian\n\n이 기기에 만든 vault입니다.\n")
            .map_err(|e| e.to_string())?;
    }
    Ok(dir.to_string_lossy().into_owned())
}

/// Phones: may the app use vaults in shared storage? (Always on the desktop.)
#[tauri::command]
async fn storage_access(app: AppHandle) -> Result<storage::Access, String> {
    storage::access(&app).await
}

/// Phones: ask for "All files access"; the state after the user answers.
#[tauri::command]
async fn request_storage_access(app: AppHandle) -> Result<storage::Access, String> {
    storage::request_access(&app).await
}

/// Phones: choose a vault folder with the system picker (`None` = cancelled).
#[tauri::command]
async fn pick_vault_folder(app: AppHandle) -> Result<Option<String>, String> {
    storage::pick_folder(&app).await
}

/// Vaults opened before, newest first (the last vault of older settings
/// files counts too).
#[tauri::command]
fn recent_vaults(app: AppHandle) -> Vec<vaults::VaultEntry> {
    let settings = load_settings(&app);
    let mut list = settings.recent_vaults;
    if let Some(last) = settings.last_vault {
        if !list.iter().any(|v| vaults::same_path(&v.path, &last)) {
            list.insert(0, vaults::RecentVault { path: last, opened: 0 });
        }
    }
    vaults::entries(&list)
}

/// Remove a vault from the recent list (the folder itself is untouched).
#[tauri::command]
fn forget_vault(app: AppHandle, path: String) -> Vec<vaults::VaultEntry> {
    let mut settings = load_settings(&app);
    vaults::forget(&mut settings.recent_vaults, &path);
    if settings.last_vault.as_deref().is_some_and(|l| vaults::same_path(l, &path)) {
        settings.last_vault = None;
    }
    save_settings(&app, &settings);
    vaults::entries(&settings.recent_vaults)
}

#[tauri::command]
fn refresh_tree(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<TreeNode>, String> {
    let (root, _) = current(&state, window.label())?;
    vault::build_tree(&root).map_err(|e| format!("cannot read vault: {e}"))
}

#[tauri::command]
fn last_vault(app: AppHandle) -> Option<String> {
    load_settings(&app).last_vault
}

/// When a vault file was made and last changed.
#[tauri::command]
fn file_times(state: State<AppState>, window: tauri::WebviewWindow, path: String) -> Result<vault::FileTimes, String> {
    let (root, _) = current(&state, window.label())?;
    vault::file_times(&root, &path)
}

#[tauri::command]
fn read_note(state: State<AppState>,
    window: tauri::WebviewWindow, path: String) -> Result<String, String> {
    let (root, _) = current(&state, window.label())?;
    vault::read_text(&root, &path)
}

#[derive(serde::Serialize)]
struct WriteResult {
    /// The file had changed elsewhere since `expected`: it was left alone and
    /// `content` went into this conflict copy instead.
    conflict: Option<String>,
    /// What the file holds now, when there was a conflict.
    disk: Option<String>,
}

/// Save a note. With `expected` (the text the editor last read or saved
/// there), a file that changed elsewhere in the meantime (a sync pull,
/// Obsidian) is not overwritten: `content` is kept as a conflict copy
/// labelled with `stamp` ("2026-11-14 0305"). The file's line endings are kept.
#[tauri::command]
fn write_note(state: State<AppState>,
    window: tauri::WebviewWindow, path: String, content: String,
    expected: Option<String>, stamp: Option<String>) -> Result<WriteResult, String> {
    let (root, index) = current(&state, window.label())?;
    let disk = match vault::read_if_exists(&root, &path) {
        Ok(disk) => disk,
        // Unreadable now: refuse when asked to check, else write as before.
        Err(e) if expected.is_some() => return Err(e),
        Err(_) => None,
    };
    match vault::plan_save(disk.as_deref(), &content, expected.as_deref()) {
        vault::SavePlan::Write(text) => {
            file_history::save_text(&root, &path, &text)?;
            // Update right away so search/backlinks don't wait for the watcher.
            write_index(&index).upsert_note(&path, text);
            Ok(WriteResult { conflict: None, disk: None })
        }
        vault::SavePlan::Conflict => {
            let stamp = stamp.unwrap_or_else(|| "저장".into());
            let copy = vault::create_conflict_copy(&root, &path, &stamp, &content)?;
            write_index(&index).upsert_note(&copy, content);
            Ok(WriteResult { conflict: Some(copy), disk })
        }
    }
}

/// Create an empty note (used when following a link to a missing note).
/// Returns the created path.
#[tauri::command]
fn create_note(state: State<AppState>,
    window: tauri::WebviewWindow, path: String) -> Result<String, String> {
    let (root, index) = current(&state, window.label())?;
    let path = if path.to_ascii_lowercase().ends_with(".md") { path } else { format!("{path}.md") };
    vault::create_text(&root, &path, "")?;
    write_index(&index).upsert_note(&path, String::new());
    Ok(path)
}

/// "제목 없는 캔버스.canvas" (or "… 1.canvas") in `folder`; returns its path.
#[tauri::command]
fn new_canvas(state: State<AppState>,
    window: tauri::WebviewWindow, folder: String) -> Result<String, String> {
    let (root, index) = current(&state, window.label())?;
    let path = fileops::unique_child(&root, &folder, "제목 없는 캔버스", ".canvas")?;
    let empty = "{\n\t\"nodes\":[],\n\t\"edges\":[]\n}";
    vault::create_text(&root, &path, empty)?;
    write_index(&index).upsert_note(&path, empty.to_string());
    Ok(path)
}

/// "제목 없음.md" (or "제목 없음 1.md", …) in `folder`; returns its path.
#[tauri::command]
fn new_note(state: State<AppState>,
    window: tauri::WebviewWindow, folder: String) -> Result<String, String> {
    let (root, index) = current(&state, window.label())?;
    let path = fileops::unique_child(&root, &folder, "제목 없음", ".md")?;
    vault::create_text(&root, &path, "")?;
    write_index(&index).upsert_note(&path, String::new());
    Ok(path)
}

#[derive(serde::Serialize)]
struct SavedAttachment {
    /// Vault path of the saved file.
    path: String,
    /// What to insert in the note (`![[…]]`, `[[…]]` or a markdown link).
    embed: String,
}

/// Save a pasted or dropped file as an attachment of a note. The bytes are
/// the request body (raw, no JSON); percent-encoded headers carry the note
/// path (`x-note`), the file's name (`x-name`) and MIME type (`x-type`),
/// the local time (`x-stamp`, "YYYYMMDDHHmmss") and `x-pasted: 1` for a
/// clipboard image, which is named "Pasted image <time>" like Obsidian does.
#[tauri::command]
fn save_attachment(state: State<AppState>, window: tauri::WebviewWindow,
    request: tauri::ipc::Request<'_>) -> Result<SavedAttachment, String> {
    use rusidian_core::attachments;
    use std::io::Write;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("첨부할 파일 내용이 없습니다.".into());
    };
    if bytes.is_empty() {
        return Err("빈 파일은 첨부하지 않습니다.".into());
    }
    if bytes.len() > attachments::MAX_BYTES {
        return Err("50MB가 넘는 파일은 첨부할 수 없습니다.".into());
    }
    let header = |key: &str| {
        request.headers().get(key).and_then(|v| v.to_str().ok()).and_then(attachments::percent_decode).unwrap_or_default()
    };
    let (note, name, mime, stamp) = (header("x-note"), header("x-name"), header("x-type"), header("x-stamp"));
    if stamp.len() != 14 || !stamp.bytes().all(|b| b.is_ascii_digit()) {
        return Err("잘못된 시각입니다.".into());
    }
    let (root, index) = current(&state, window.label())?;
    let settings = attachments::load(&root);
    let folder = attachments::folder_for(&settings, &note);

    let mime_ext = attachments::image_extension(&mime).map(|e| format!(".{e}"));
    let (stem, ext) = match (header("x-pasted") == "1", attachments::clean_name(&name)) {
        (false, Some(named)) => named,
        (_, named) => (
            format!("Pasted image {stamp}"),
            mime_ext.or_else(|| named.map(|(_, e)| e)).unwrap_or_else(|| ".png".into()),
        ),
    };
    let rel = fileops::unique_child(&root, &folder, &stem, &ext)?;
    let full = vault::resolve(&root, &rel)?;
    if let Some(parent) = full.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("폴더를 만들 수 없습니다: {e}"))?;
    }
    // create_new: never overwrite a file that appeared in the meantime.
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(&full)
        .map_err(|e| format!("{rel}을(를) 만들 수 없습니다: {e}"))?;
    file.write_all(bytes).map_err(|e| format!("{rel}을(를) 저장할 수 없습니다: {e}"))?;
    drop(file);

    // In the index now, so the embed resolves before the watcher catches up.
    write_index(&index).refresh_path(&root, &rel);
    let file_name = rel.rsplit('/').next().unwrap_or(&rel);
    let unique = read_index(&index).paths().named(file_name) <= 1;
    Ok(SavedAttachment { embed: attachments::embed_text(&settings, &rel, &note, unique), path: rel })
}

// ------------------------------------------------------- bookmarks & links

/// Obsidian's bookmarks (`.obsidian/bookmarks.json` items, as stored).
#[tauri::command]
fn get_bookmarks(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<serde_json::Value>, String> {
    let (root, _) = current(&state, window.label())?;
    Ok(rusidian_core::bookmarks::load(&root))
}

/// Bookmark a file or remove its bookmark; returns whether it is bookmarked now.
#[tauri::command]
fn toggle_bookmark(state: State<AppState>, window: tauri::WebviewWindow, path: String, ctime: i64) -> Result<bool, String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::bookmarks::toggle_file(&root, &path, ctime)
}

/// Remove the bookmark at `at` (indices from the top down into groups).
#[tauri::command]
fn remove_bookmark(state: State<AppState>, window: tauri::WebviewWindow, at: Vec<usize>) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::bookmarks::remove_at(&root, &at)
}

#[tauri::command]
fn outgoing_links(state: State<AppState>, window: tauri::WebviewWindow, path: String) -> Result<Vec<rusidian_core::index::OutLink>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).outgoing(&path))
}

#[tauri::command]
fn unlinked_mentions(state: State<AppState>, window: tauri::WebviewWindow, path: String) -> Result<Vec<rusidian_core::index::Mention>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).unlinked_mentions(&path))
}

/// Turn the plain mention of `term` on `line` of `source` into a link to `target` (a note path).
#[tauri::command]
fn link_mention(state: State<AppState>, window: tauri::WebviewWindow,
    source: String, line: usize, raw: String, term: String, target: String) -> Result<(), String> {
    let link = {
        let (_, index) = current(&state, window.label())?;
        let idx = read_index(&index);
        idx.link_target(&target)
    };
    let new_line = rusidian_core::mentions::link_mention(&raw, &term, &link).ok_or("그 줄에서 이름을 찾지 못했습니다.")?;
    rewrite_note(&state, window.label(), &source, |text| rusidian_core::markdown::replace_line(text, line, &raw, Some(&new_line)))
}

/// "새 폴더" (or "새 폴더 1", …) in `parent`; returns its path.
#[tauri::command]
fn new_folder(state: State<AppState>,
    window: tauri::WebviewWindow, parent: String) -> Result<String, String> {
    let (root, _) = current(&state, window.label())?;
    let path = fileops::unique_child(&root, &parent, "새 폴더", "")?;
    fileops::create_folder(&root, &path)?;
    Ok(path)
}

#[derive(serde::Serialize)]
struct MoveResult {
    /// New path of the moved file or folder.
    path: String,
    /// Notes whose links were rewritten to follow the move.
    updated: Vec<String>,
    /// Notes whose links could not be rewritten.
    failed: Vec<String>,
}

fn move_to(state: &AppState,
    label: &str, from: &str, to: &str) -> Result<MoveResult, String> {
    let (root, index) = current(state, label)?;
    let plan = fileops::plan_move(&root, &read_index(&index), from, to)?;
    let outcome = fileops::apply_move(&root, &plan)?;
    // Bookmarks follow the file (as in Obsidian); a failure here never undoes the move.
    let _ = rusidian_core::bookmarks::renamed(&root, &plan.from, &plan.to);
    file_history::moved(&root, &plan.from, &plan.to);
    let mut idx = write_index(&index);
    idx.moved(&root, &plan.from, &plan.to);
    for (path, content) in &outcome.rewritten {
        idx.upsert_note(path, content.clone());
    }
    Ok(MoveResult {
        path: plan.to.clone(),
        updated: outcome.rewritten.into_iter().map(|(p, _)| p).collect(),
        failed: outcome.failed,
    })
}

/// Rename a file or folder in place (`new_name` includes any extension).
#[tauri::command]
fn rename_path(state: State<AppState>,
    window: tauri::WebviewWindow, from: String, new_name: String) -> Result<MoveResult, String> {
    let to = fileops::join(fileops::parent_of(&from), &new_name);
    move_to(&state, window.label(), &from, &to)
}

/// Move a file or folder into `folder` ("" = vault root).
#[tauri::command]
fn move_path(state: State<AppState>,
    window: tauri::WebviewWindow, from: String, folder: String) -> Result<MoveResult, String> {
    let name = from.rsplit('/').next().unwrap_or(&from).to_string();
    move_to(&state, window.label(), &from, &fileops::join(&folder, &name))
}

/// Send a file or folder to the system trash (Recycle Bin), as Obsidian does
/// by default. Links to it are left as they are.
#[tauri::command]
fn trash_path(state: State<AppState>,
    window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    let (root, index) = current(&state, window.label())?;
    let full = fileops::check_deletable(&root, &path)?;
    // Recoverable even after the trash is emptied.
    file_history::before_delete(&root, &path);
    move_to_trash(&root, &full).map_err(|e| format!("휴지통으로 옮기지 못했습니다 ({path}): {e}"))?;
    write_index(&index).forget(&path);
    Ok(())
}

#[cfg(desktop)]
fn move_to_trash(_root: &Path, full: &Path) -> Result<(), String> {
    trash::delete(full).map_err(|e| e.to_string())
}

/// Phones have no system trash: use the vault's `.trash` folder, as
/// Obsidian's "Move to Obsidian trash" option does (hidden from the tree).
#[cfg(mobile)]
fn move_to_trash(root: &Path, full: &Path) -> Result<(), String> {
    let bin = root.join(".trash");
    std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
    let name = full.file_name().ok_or("이름이 없는 경로입니다")?;
    let mut target = bin.join(name);
    let (stem, ext) = {
        let p = Path::new(name);
        (p.file_stem().unwrap_or(name).to_string_lossy().into_owned(),
         p.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default())
    };
    let mut n = 1;
    while target.exists() {
        target = bin.join(format!("{stem} {n}{ext}"));
        n += 1;
    }
    std::fs::rename(full, &target).map_err(|e| e.to_string())
}

/// Explorer / Finder only; phones have nothing to show a file in.
fn reveal_in_file_manager(path: impl AsRef<Path>) -> Result<(), String> {
    #[cfg(desktop)]
    return tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| e.to_string());
    #[cfg(mobile)]
    {
        let _ = path;
        Err("이 기기에서는 파일 관리자로 보여 줄 수 없습니다".into())
    }
}

/// Show a file or folder ("" = the vault itself) in Explorer / Finder.
#[tauri::command]
fn reveal_path(state: State<AppState>,
    window: tauri::WebviewWindow, path: String) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    let full = if path.is_empty() { root } else { vault::resolve(&root, &path)? };
    reveal_in_file_manager(full)
}

/// Parse a note's YAML frontmatter. Pure function: the caller passes the
/// current editor text so properties reflect unsaved edits too.
#[tauri::command]
fn parse_frontmatter(content: String) -> frontmatter::Frontmatter {
    frontmatter::parse(&content)
}

#[derive(serde::Serialize)]
struct TemplateList {
    /// The templates folder from `.obsidian/templates.json` ("" if unset).
    folder: String,
    /// Notes in that folder (and below), sorted.
    files: Vec<String>,
}

#[tauri::command]
fn list_templates(state: State<AppState>, window: tauri::WebviewWindow) -> Result<TemplateList, String> {
    let (root, index) = current(&state, window.label())?;
    let config = templates::load_config(&root);
    let folder = config.folder().to_string();
    let mut files: Vec<String> = if folder.is_empty() {
        Vec::new()
    } else {
        read_index(&index).paths().under(&folder).into_iter().filter(|p| p.to_lowercase().ends_with(".md")).collect()
    };
    files.sort_by_key(|p| p.to_lowercase());
    Ok(TemplateList { folder, files })
}

/// The Templates and Daily notes settings (shared with Obsidian), with the
/// vault's folders and notes to pick from.
#[tauri::command]
fn note_settings(state: State<AppState>, window: tauri::WebviewWindow) -> Result<note_settings::NoteSettings, String> {
    let (root, _) = current(&state, window.label())?;
    Ok(note_settings::load(&root))
}

#[tauri::command]
fn set_note_settings(state: State<AppState>, window: tauri::WebviewWindow,
    templates: note_settings::TemplateSettings, daily: note_settings::DailySettings) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    note_settings::save_templates(&root, &templates)?;
    note_settings::save_daily(&root, &daily)
}

/// What the settings being edited produce at `date` `time` (today, now).
#[tauri::command]
fn preview_note_settings(state: State<AppState>, window: tauri::WebviewWindow,
    templates: note_settings::TemplateSettings, daily: note_settings::DailySettings,
    date: String, time: String) -> Result<note_settings::Preview, String> {
    let (root, _) = current(&state, window.label())?;
    let date = parse_date(&date)?;
    let (h, m) = time.split_once(':').and_then(|(h, m)| Some((h.parse().ok()?, m.parse().ok()?))).unwrap_or((0, 0));
    Ok(note_settings::preview(&root, &templates, &daily, date, (h, m)))
}

/// A template with `{{title}}` (the target note's title), `{{date}}` and
/// `{{time}}` filled in, split into properties and body for inserting.
#[tauri::command]
fn render_template(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    path: String,
    title: String,
    date: String,
    time: String,
) -> Result<templates::Rendered, String> {
    let (root, _) = current(&state, window.label())?;
    let at = templates::Moment::parse(&date, &time).ok_or("잘못된 날짜/시간입니다.")?;
    let raw = vault::read_text(&root, &path)?;
    templates::split(&templates::render(&raw, &title, at, &templates::load_config(&root)))
}

/// `content` with its frontmatter rewritten to `properties` (the editor
/// applies the result, so undo and autosave work as for typing).
#[tauri::command]
fn set_properties(content: String, properties: Vec<frontmatter::Property>) -> String {
    frontmatter::replace(&content, &properties)
}

// ------------------------------------------------------- vault:// protocol

/// Content type for files shown inline (images, PDF, audio, video).
fn mime_of(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "ogv" => "video/ogg",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// `%XX` decoding of a URL path (UTF-8).
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn respond(status: u16, mime: &str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", mime)
        // Files may change on disk; the app refreshes views itself.
        .header("Cache-Control", "no-cache")
        .header("Access-Control-Allow-Origin", "*")
        .body(body)
        .unwrap_or_default()
}

/// Serves `vault://localhost/<vault-relative path>` (on Windows
/// `http://vault.localhost/...`) from the vault of the window asking
/// (`label`), read-only, so notes can show embedded images, PDFs and media.
/// Hidden folders and paths leaving the vault are refused.
fn serve_vault_file(app: &AppHandle, label: &str, uri_path: &str) -> tauri::http::Response<Vec<u8>> {
    let not_found = || respond(404, "text/plain", b"not found".to_vec());
    let Some(rel) = percent_decode(uri_path.trim_start_matches('/')) else { return not_found() };
    if rel.is_empty() || vault::is_hidden_path(&rel) {
        return not_found();
    }
    let state = app.state::<AppState>();
    let Ok((root, _)) = current(&state, label) else { return not_found() };
    match vault::resolve(&root, &rel).and_then(|p| fs::read(p).map_err(|e| e.to_string())) {
        Ok(bytes) => respond(200, mime_of(&rel), bytes),
        Err(_) => not_found(),
    }
}

#[tauri::command]
async fn search_vault(state: State<'_, AppState>,
    window: tauri::WebviewWindow, query: String) -> Result<Vec<SearchHit>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).search(&query, SEARCH_LIMIT))
}

// ------------------------------------------------------- Everything search

/// Results asked of Everything at most (it counts all matches anyway).
const EVERYTHING_MAX: u32 = 200;
/// Searches whose hits stay openable. More than one: answers can come back out
/// of order while typing, and the list shown may not be the last one asked.
const EVERYTHING_KEEP: usize = 4;

#[derive(serde::Serialize)]
struct EverythingHit {
    #[serde(flatten)]
    hit: everything::Hit,
    /// Vault-relative path when the hit is in this window's vault (and not hidden).
    vault_path: Option<String>,
}

#[derive(serde::Serialize)]
struct EverythingFound {
    total: u32,
    hits: Vec<EverythingHit>,
}

/// Is Everything running, which version, and has it loaded its index?
#[tauri::command]
async fn everything_status() -> Result<everything::Status, String> {
    tauri::async_runtime::spawn_blocking(everything::status).await.map_err(|e| e.to_string())
}

/// Search the whole PC with Everything (its own query syntax). Works with or
/// without a vault open; hits inside the vault carry their vault path.
#[tauri::command]
async fn everything_search(state: State<'_, AppState>, window: tauri::WebviewWindow,
    query: String, limit: Option<u32>) -> Result<EverythingFound, String> {
    let root = current(&state, window.label()).ok().map(|(root, _)| root);
    let max = limit.unwrap_or(EVERYTHING_MAX).clamp(1, EVERYTHING_MAX);
    let found = tauri::async_runtime::spawn_blocking(move || everything::search(&query, max))
        .await
        .map_err(|e| e.to_string())??;
    let hits: Vec<EverythingHit> = found.hits.into_iter().map(|hit| {
        let vault_path = root.as_deref()
            .and_then(|r| everything::vault_relative(r, &hit.path))
            .filter(|p| !vault::is_hidden_path(p));
        EverythingHit { hit, vault_path }
    }).collect();
    let paths = hits.iter().map(|h| h.hit.path.clone()).collect();
    let mut kept = state.everything_hits.lock().map_err(|_| "state poisoned".to_string())?;
    let recent = kept.entry(window.label().to_string()).or_default();
    recent.push_back(paths);
    while recent.len() > EVERYTHING_KEEP {
        recent.pop_front();
    }
    Ok(EverythingFound { total: found.total, hits })
}

/// Open a hit of a recent Everything search with its default app, or show it
/// in Explorer (`reveal`). Programs and scripts are only ever shown, never run:
/// the page can search for any path, so this must not be a way to start one.
/// Returns whether it was opened (false: shown in Explorer).
#[tauri::command]
fn everything_open(state: State<AppState>, window: tauri::WebviewWindow,
    path: String, reveal: bool) -> Result<bool, String> {
    let known = state.everything_hits.lock().map_err(|_| "state poisoned".to_string())?
        .get(window.label()).is_some_and(|recent| recent.iter().any(|paths| paths.contains(&path)));
    if !known {
        return Err("Everything 검색 결과에 있는 파일만 열 수 있습니다.".to_string());
    }
    if reveal || everything::runs_code(&path) {
        reveal_in_file_manager(&path)?;
        Ok(false)
    } else {
        tauri_plugin_opener::open_path(&path, None::<&str>).map_err(|e| format!("열지 못했습니다 ({path}): {e}"))?;
        Ok(true)
    }
}

/// Every tag in the vault with how many notes carry it.
#[tauri::command]
async fn list_tags(state: State<'_, AppState>, window: tauri::WebviewWindow) -> Result<Vec<TagCount>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).tag_counts())
}

#[tauri::command]
async fn backlinks(state: State<'_, AppState>,
    window: tauri::WebviewWindow, path: String) -> Result<Vec<Backlink>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).backlinks(&path))
}

/// Resolve a `[[link]]` written in `source` to a file path, if it exists.
#[tauri::command]
fn resolve_link(state: State<AppState>,
    window: tauri::WebviewWindow, source: String, target: String) -> Result<Option<String>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).resolve(&source, &target))
}

/// Dates come from the UI as `YYYY-MM-DD` in the user's local time zone.
fn parse_date(s: &str) -> Result<Date, String> {
    Date::parse(s).ok_or_else(|| format!("잘못된 날짜: {s}"))
}

#[tauri::command]
async fn list_tasks(state: State<'_, AppState>,
    window: tauri::WebviewWindow, include_done: bool) -> Result<Vec<TaskItem>, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(read_index(&index).tasks(include_done))
}

/// Complete / reopen a task in its note, Tasks-plugin style. `raw` is the
/// line as the UI saw it; the edit is refused if the file changed since.
#[tauri::command]
fn toggle_task(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    path: String,
    line: usize,
    raw: String,
    today: String,
) -> Result<(), String> {
    let (root, index) = current(&state, window.label())?;
    let today = parse_date(&today)?;
    let text = vault::read_text(&root, &path)?;
    let new_text = tasks::toggle(&text, line, &raw, today)?;
    file_history::save_text(&root, &path, &new_text)?;
    write_index(&index).upsert_note(&path, new_text);
    Ok(())
}

/// Read `path`, apply `change` to its text, write it back and re-index.
fn rewrite_note(
    state: &AppState,
    label: &str,
    path: &str,
    change: impl FnOnce(&str) -> Result<String, String>,
) -> Result<(), String> {
    let (root, index) = current(state, label)?;
    let text = vault::read_text(&root, path)?;
    let new_text = change(&text)?;
    file_history::save_text(&root, path, &new_text)?;
    write_index(&index).upsert_note(path, new_text);
    Ok(())
}

/// Rewrite a task line from the edit dialog. `raw` is the line as the UI
/// saw it; the edit is refused if the file changed since.
#[tauri::command]
fn update_task(state: State<AppState>,
    window: tauri::WebviewWindow, path: String, line: usize, raw: String, edit: tasks::TaskEdit) -> Result<(), String> {
    rewrite_note(&state, window.label(), &path, |text| tasks::edit(text, line, &raw, &edit))
}

/// Remove a task line (same `raw` check as `update_task`).
#[tauri::command]
fn delete_task(state: State<AppState>,
    window: tauri::WebviewWindow, path: String, line: usize, raw: String) -> Result<(), String> {
    rewrite_note(&state, window.label(), &path, |text| tasks::delete(text, line, &raw))
}

/// Open tasks with a reminder or start time between `from` and `to`
/// (`YYYY-MM-DD HH:MM`, local time).
#[tauri::command]
async fn task_reminders(state: State<'_, AppState>,
    window: tauri::WebviewWindow, from: String, to: String) -> Result<Vec<calendar::Reminder>, String> {
    let (root, index) = current(&state, window.label())?;
    let matcher = daily::load_config(&root).matcher();
    Ok(calendar::reminders(&read_index(&index), matcher.as_ref(), &from, &to))
}

/// Append a task to today's daily note (created from the daily-note
/// template if missing). Returns the daily note's path.
#[tauri::command]
fn add_task(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    text: String,
    due: Option<String>,
    today: String,
    time: String,
) -> Result<String, String> {
    let (root, index) = current(&state, window.label())?;
    let today = parse_date(&today)?;
    let due = match due.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(parse_date(s)?),
        None => None,
    };
    let line = daily::task_line(&text, due)?;
    let config = daily::load_config(&root);
    append_to_daily(&root, &index, &config, today, &time, &line)
}

/// Append `line` to the daily note for `day` (from the template if new).
fn append_to_daily(
    root: &std::path::Path,
    index: &RwLock<VaultIndex>,
    config: &daily::DailyConfig,
    day: Date,
    time: &str,
    line: &str,
) -> Result<String, String> {
    let path = config.note_path(day)?;
    let existing = if vault::resolve(root, &path)?.exists() {
        vault::read_text(root, &path)?
    } else {
        config.initial_content(root, day, time)
    };
    let new_text = daily::append_line(&existing, line);
    file_history::save_text(root, &path, &new_text)?;
    write_index(index).upsert_note(&path, new_text);
    Ok(path)
}

/// The whole link graph (the UI filters it, e.g. to a note's neighbourhood).
#[tauri::command]
async fn link_graph(state: State<'_, AppState>, window: tauri::WebviewWindow) -> Result<Graph, String> {
    let (_, index) = current(&state, window.label())?;
    Ok(graph::build(&read_index(&index)))
}

/// Money records dated in `start..=end` (plus undated ones and all categories).
#[tauri::command]
async fn finance_range(state: State<'_, AppState>,
    window: tauri::WebviewWindow, start: String, end: String) -> Result<FinanceData, String> {
    let (root, index) = current(&state, window.label())?;
    let (start, end) = (parse_date(&start)?, parse_date(&end)?);
    if start.days_until(end) < 0 || start.days_until(end) > 400 {
        return Err("재무 조회 범위가 잘못되었습니다.".into());
    }
    let matcher = daily::load_config(&root).matcher();
    Ok(read_index(&index).finance(matcher.as_ref(), &start.to_string(), &end.to_string()))
}

/// Append a money record to today's daily note. `📅 date` is written only
/// when it isn't today's or the daily note's date can't be read from its path.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn add_finance(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    kind: EntryKind,
    amount: String,
    memo: String,
    category: String,
    date: String,
    today: String,
    time: String,
) -> Result<String, String> {
    let (root, index) = current(&state, window.label())?;
    let (date, today) = (parse_date(&date)?, parse_date(&today)?);
    let config = daily::load_config(&root);
    let path = config.note_path(today)?;
    let note_has_date = config.matcher().and_then(|m| m.date_of(&path)) == Some(today);
    let explicit = (date != today || !note_has_date).then_some(date);
    let line = finance::entry_line(kind, &amount, &memo, &category, explicit)?;
    append_to_daily(&root, &index, &config, today, &time, &line)
}

/// Rewrite a money record from the edit dialog (`raw` = the line as the UI saw it).
#[tauri::command]
fn update_finance(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    path: String,
    line: usize,
    raw: String,
    edit: finance::EntryEdit,
) -> Result<(), String> {
    rewrite_note(&state, window.label(), &path, |text| finance::edit(text, line, &raw, &edit))
}

/// Write a settlement under a money record (`raw` = the record as the UI saw it).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
fn add_settlement(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    path: String,
    line: usize,
    raw: String,
    amount: String,
    memo: String,
    date: Option<String>,
) -> Result<(), String> {
    let date = date.as_deref().filter(|d| !d.is_empty()).map(parse_date).transpose()?;
    rewrite_note(&state, window.label(), &path, |text| finance::insert_settlement(text, line, &raw, &amount, &memo, date))
}

#[tauri::command]
fn delete_finance(state: State<AppState>,
    window: tauri::WebviewWindow, path: String, line: usize, raw: String) -> Result<(), String> {
    rewrite_note(&state, window.label(), &path, |text| finance::delete(text, line, &raw))
}

// ------------------------------------------------------- statement import

const IMPORT_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// A CSV file the user picked (anywhere on disk).
fn read_statement(file: &str) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(file).map_err(|e| format!("파일을 열 수 없습니다: {e}"))?;
    if meta.len() > IMPORT_MAX_BYTES {
        return Err("파일이 너무 큽니다 (10MB까지).".into());
    }
    fs::read(file).map_err(|e| format!("파일을 읽을 수 없습니다: {e}"))
}

/// Every dated money record of the vault, for the duplicate check.
fn all_finance(root: &std::path::Path, index: &RwLock<VaultIndex>) -> Vec<finance::FinanceItem> {
    let matcher = daily::load_config(root).matcher();
    read_index(index).finance(matcher.as_ref(), "0000-01-01", "9999-12-31").entries
}

#[tauri::command]
fn import_csv_preview(file: String, password: Option<String>) -> Result<finance_import::Preview, String> {
    let table = statement_rows(&file, password.as_deref())?;
    if table.rows.is_empty() {
        return Err("파일에서 읽을 줄이 없습니다.".into());
    }
    Ok(finance_import::preview_rows(table.rows, &table.format))
}

/// A statement file (CSV, xlsx, xls, a password-protected Excel file, NH's PDF) as rows.
/// Errors "PASSWORD_REQUIRED" / "WRONG_PASSWORD" let the dialog ask for it.
fn statement_rows(file: &str, password: Option<&str>) -> Result<rusidian_core::statement::Table, String> {
    rusidian_core::statement::read(&read_statement(file)?, password).map_err(|e| e.message())
}

fn plan_statement(
    state: &AppState,
    label: &str,
    file: &str,
    header: usize,
    mapping: &finance_import::Mapping,
    default_category: &str,
    password: Option<&str>,
) -> Result<Vec<finance_import::PlannedRow>, String> {
    let (root, index) = current(state, label)?;
    let rows = statement_rows(file, password)?.rows;
    Ok(finance_import::plan(&rows, header, mapping, default_category, &all_finance(&root, &index)))
}

/// Each row as it would be imported (ok / duplicate / skipped).
#[tauri::command]
fn import_csv_plan(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    file: String,
    header: usize,
    mapping: finance_import::Mapping,
    default_category: String,
    password: Option<String>,
) -> Result<Vec<finance_import::PlannedRow>, String> {
    plan_statement(&state, window.label(), &file, header, &mapping, &default_category, password.as_deref())
}

#[derive(serde::Serialize)]
struct ImportResult {
    /// The note the records were added to.
    path: String,
    added: usize,
    duplicates: usize,
    skipped: usize,
}

/// Append the importable rows as records to `note` (created if missing).
#[tauri::command]
fn import_csv_commit(
    state: State<AppState>,
    window: tauri::WebviewWindow,
    file: String,
    header: usize,
    mapping: finance_import::Mapping,
    default_category: String,
    note: String,
    password: Option<String>,
) -> Result<ImportResult, String> {
    let (root, index) = current(&state, window.label())?;
    let planned = plan_statement(&state, window.label(), &file, header, &mapping, &default_category, password.as_deref())?;
    let lines = finance_import::lines(&planned);
    let count = |s| planned.iter().filter(|p| p.status == s).count();
    let (duplicates, skipped) = (count(finance_import::RowStatus::Duplicate), count(finance_import::RowStatus::Skipped));
    if lines.is_empty() {
        return Err("가져올 새 기록이 없습니다.".into());
    }
    let path = append_to_note(&root, &index, &note, &lines)?;
    Ok(ImportResult { path, added: lines.len(), duplicates, skipped })
}

/// Append `lines` to `note` (a vault path, `.md` optional; created with a
/// title if missing). Returns the note's path.
fn append_to_note(
    root: &std::path::Path,
    index: &RwLock<VaultIndex>,
    note: &str,
    lines: &[String],
) -> Result<String, String> {
    let note = note.trim().trim_matches('/').replace('\\', "/");
    let note = if note.to_lowercase().ends_with(".md") { note } else { format!("{note}.md") };
    for part in note[..note.len() - 3].split('/') {
        fileops::validate_name(part)?;
    }
    let path = read_index(index).file_path(&note).map(str::to_string).unwrap_or(note);
    let mut text = if vault::resolve(root, &path)?.exists() {
        vault::read_text(root, &path)?
    } else {
        let title = path.rsplit('/').next().unwrap_or(&path).trim_end_matches(".md");
        format!("# {title}\n")
    };
    for line in lines {
        text = daily::append_line(&text, line);
    }
    file_history::save_text(root, &path, &text)?;
    write_index(index).upsert_note(&path, text);
    Ok(path)
}

#[tauri::command]
fn get_budgets(state: State<AppState>, window: tauri::WebviewWindow) -> Result<finance::Budgets, String> {
    let (root, _) = current(&state, window.label())?;
    Ok(finance::load_budgets(&root))
}

#[tauri::command]
fn set_budgets(state: State<AppState>,
    window: tauri::WebviewWindow, budgets: finance::Budgets) -> Result<finance::Budgets, String> {
    let (root, _) = current(&state, window.label())?;
    finance::save_budgets(&root, &budgets)
}

/// Every account balance record in the vault, oldest first.
#[tauri::command]
fn balances(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<rusidian_core::assets::BalanceItem>, String> {
    let (root, index) = current(&state, window.label())?;
    let matcher = daily::load_config(&root).matcher();
    Ok(read_index(&index).balances(matcher.as_ref()))
}

/// Append a balance record (`- 🏦 …`) to `note`, creating it if needed.
#[tauri::command]
fn add_balance(state: State<AppState>, window: tauri::WebviewWindow,
    note: String, amount: String, account: String, group: String, date: String) -> Result<(), String> {
    let (root, index) = current(&state, window.label())?;
    let note = note.trim().trim_matches('/').to_string();
    let note = if note.is_empty() { "자산.md".to_string() } else if note.to_lowercase().ends_with(".md") { note } else { format!("{note}.md") };
    let line = rusidian_core::assets::balance_line(&amount, &account, &group, parse_date(&date)?)?;
    let mut next = vault::read_text(&root, &note).unwrap_or_default();
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&line);
    next.push('\n');
    file_history::save_text(&root, &note, &next)?;
    write_index(&index).upsert_note(&note, next);
    Ok(())
}

/// Custom hotkeys (`.obsidian/hotkeys.json`, shared with Obsidian).
#[tauri::command]
fn get_hotkeys(state: State<AppState>, window: tauri::WebviewWindow)
    -> Result<std::collections::BTreeMap<String, Vec<rusidian_core::hotkeys::Hotkey>>, String> {
    let (root, _) = current(&state, window.label())?;
    Ok(rusidian_core::hotkeys::read(&root))
}

/// Give command `id` these keys (None: back to its defaults).
#[tauri::command]
fn set_hotkey(state: State<AppState>, window: tauri::WebviewWindow, id: String,
    keys: Option<Vec<rusidian_core::hotkeys::Hotkey>>) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::hotkeys::set(&root, &id, keys.as_deref())
}

#[tauri::command]
fn get_savings(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<String>, String> {
    let (root, _) = current(&state, window.label())?;
    Ok(rusidian_core::assets::load_savings(&root))
}

#[tauri::command]
fn set_savings(state: State<AppState>, window: tauri::WebviewWindow, list: Vec<String>) -> Result<Vec<String>, String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::assets::save_savings(&root, &list)
}

/// The fixed-expense list, this month's payments of it, and monthly-looking
/// payments that are not on it yet.
#[derive(serde::Serialize)]
struct FixedOverview {
    list: Vec<rusidian_core::fixed::FixedExpense>,
    status: Vec<rusidian_core::fixed::FixedStatus>,
    suggestions: Vec<rusidian_core::fixed::Suggestion>,
    /// Days before the due day to remind (None: off).
    remind_days: Option<u32>,
    /// How many suggestions were hidden.
    ignored: usize,
}

#[tauri::command]
async fn fixed_expenses(state: State<'_, AppState>,
    window: tauri::WebviewWindow, month: String) -> Result<FixedOverview, String> {
    use rusidian_core::fixed;
    let (root, index) = current(&state, window.label())?;
    parse_date(&format!("{month}-01"))?;
    let items = all_finance(&root, &index);
    let list = fixed::load(&root);
    let ignored = fixed::load_ignored(&root);
    Ok(FixedOverview {
        status: fixed::status(&list, &items, &month),
        suggestions: fixed::suggest(&items, &list, &ignored),
        remind_days: fixed::load_remind_days(&root),
        ignored: ignored.len(),
        list,
    })
}

/// Fixed expenses to remind of today (none when reminders are off).
#[tauri::command]
async fn fixed_reminders(state: State<'_, AppState>,
    window: tauri::WebviewWindow, today: String) -> Result<Vec<rusidian_core::fixed::FixedReminder>, String> {
    use rusidian_core::fixed;
    let (root, index) = current(&state, window.label())?;
    let today = parse_date(&today)?;
    let Some(before) = fixed::load_remind_days(&root) else { return Ok(Vec::new()) };
    let list = fixed::load(&root);
    if list.is_empty() {
        return Ok(Vec::new());
    }
    Ok(fixed::reminders(&list, &all_finance(&root, &index), today, before))
}

#[tauri::command]
fn set_fixed_remind(state: State<AppState>, window: tauri::WebviewWindow, days: Option<u32>) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::fixed::save_remind_days(&root, days)
}

/// Hide a suggestion by its keyword (None: show the hidden ones again).
#[tauri::command]
fn ignore_fixed(state: State<AppState>, window: tauri::WebviewWindow, keyword: Option<String>) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::fixed::ignore(&root, keyword.as_deref()).map(|_| ())
}

#[tauri::command]
fn set_fixed_expenses(state: State<AppState>, window: tauri::WebviewWindow,
    list: Vec<rusidian_core::fixed::FixedExpense>) -> Result<Vec<rusidian_core::fixed::FixedExpense>, String> {
    let (root, _) = current(&state, window.label())?;
    rusidian_core::fixed::save(&root, &list)
}

/// Write the records dated `start..=end` as CSV to `dest` (a path the user
/// picked in the save dialog). Returns how many rows were written.
#[tauri::command]
async fn export_finance_csv(state: State<'_, AppState>,
    window: tauri::WebviewWindow, dest: String, start: String, end: String) -> Result<usize, String> {
    let (root, index) = current(&state, window.label())?;
    let (start, end) = (parse_date(&start)?, parse_date(&end)?);
    let matcher = daily::load_config(&root).matcher();
    let data = read_index(&index).finance(matcher.as_ref(), &start.to_string(), &end.to_string());
    fs::write(&dest, finance::to_csv(&data.entries)).map_err(|e| format!("{dest}에 저장하지 못했습니다: {e}"))?;
    Ok(data.entries.len())
}

#[tauri::command]
fn graph_groups(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<graph::ColorGroup>, String> {
    let (root, _) = current(&state, window.label())?;
    Ok(graph::load_groups(&root))
}

#[tauri::command]
fn set_graph_groups(state: State<AppState>,
    window: tauri::WebviewWindow, groups: Vec<graph::ColorGroup>) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    graph::save_groups(&root, &groups)
}

// ------------------------------------------------------------- git sync

fn vault_key(root: &std::path::Path) -> String {
    root.to_string_lossy().into_owned()
}

#[derive(serde::Serialize)]
struct SyncInfo {
    /// Installed Git's version (None: Git is not installed).
    git: Option<String>,
    /// This vault's settings, if sync is set up on this device.
    config: Option<SyncConfig>,
    /// The folders chosen for this vault (shared by all devices).
    folders: Vec<String>,
    include_calendars: bool,
}

#[tauri::command]
fn get_sync(app: AppHandle, state: State<AppState>, window: tauri::WebviewWindow) -> Result<SyncInfo, String> {
    let (root, _) = current(&state, window.label())?;
    let shared = sync::load_shared(&root);
    let config = load_settings(&app).sync.get(&vault_key(&root)).map(|d| {
        SyncConfig {
            remote: d.remote.clone(),
            branch: d.branch.clone(),
            interval_minutes: d.interval_minutes,
            ..Default::default()
        }
        .with_shared(&shared)
    });
    Ok(SyncInfo { git: gitsync::git_version(), config, folders: shared.folders, include_calendars: shared.include_calendars })
}

/// Save (or with `None`, turn off on this device) the vault's sync settings.
#[tauri::command]
fn set_sync(
    app: AppHandle,
    state: State<AppState>,
    window: tauri::WebviewWindow,
    config: Option<SyncConfig>,
) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    let mut settings = load_settings(&app);
    match config {
        None => {
            settings.sync.remove(&vault_key(&root));
        }
        Some(c) => {
            let remote = c.remote.trim().to_string();
            if remote.is_empty() {
                return Err("원격 저장소 주소를 입력하세요.".into());
            }
            let branch = if c.branch.trim().is_empty() { "main".to_string() } else { c.branch.trim().to_string() };
            sync::save_shared(&root, &sync::Shared { folders: c.folders, include_calendars: c.include_calendars })?;
            settings.sync.insert(vault_key(&root), DeviceSync { remote, branch, interval_minutes: c.interval_minutes });
        }
    }
    save_settings(&app, &settings);
    Ok(())
}

/// What the given folders (plus the settings folder) would upload.
#[tauri::command]
async fn sync_scan(state: State<'_, AppState>, window: tauri::WebviewWindow, folders: Vec<String>) -> Result<sync::Scan, String> {
    let (root, _) = current(&state, window.label())?;
    tauri::async_runtime::spawn_blocking(move || sync::scan(&root, &folders)).await.map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct SyncReport {
    #[serde(flatten)]
    outcome: gitsync::Outcome,
    /// Rusidian settings that arrived from another device, to apply.
    ui: Option<serde_json::Value>,
}

/// The Rusidian settings travelling with the vault (`.obsidian/rusidian-app.json`).
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Clone)]
struct AppSettingsFile {
    #[serde(default)]
    ui: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    calendars: Option<Vec<CalendarSource>>,
}

/// Sync this window's vault now. `ui` is this device's settings (theme,
/// font size, …) when they changed since the last sync, else null — so a
/// Conflict copies sync left in the vault, newest first.
#[tauri::command]
fn list_conflicts(state: State<AppState>, window: tauri::WebviewWindow) -> Result<Vec<rusidian_core::merge::ConflictCopy>, String> {
    let (_, index) = current(&state, window.label())?;
    let mut list: Vec<_> = read_index(&index).paths().all().filter_map(rusidian_core::merge::conflict_of).collect();
    list.sort_by(|a, b| b.stamp.cmp(&a.stamp).then_with(|| a.copy.cmp(&b.copy)));
    Ok(list)
}

#[derive(serde::Serialize)]
struct ConflictView {
    #[serde(flatten)]
    conflict: rusidian_core::merge::ConflictCopy,
    /// The original file is still there (else the copy can only replace it).
    original_exists: bool,
    /// Both read as text (else they can't be compared line by line).
    text: bool,
    blocks: Vec<rusidian_core::merge::Block>,
    /// Line endings and final newline of the original, kept when merging.
    crlf: bool,
    trailing_newline: bool,
}

/// A conflict copy next to its file, split into shared and differing parts.
#[tauri::command]
fn conflict_blocks(state: State<AppState>, window: tauri::WebviewWindow, copy: String) -> Result<ConflictView, String> {
    let (root, _) = current(&state, window.label())?;
    let conflict = rusidian_core::merge::conflict_of(&copy).ok_or("충돌 사본이 아닙니다.")?;
    let original_exists = vault::resolve(&root, &conflict.original).is_ok_and(|p| p.is_file());
    let theirs = vault::read_text(&root, &copy);
    let ours = if original_exists { vault::read_text(&root, &conflict.original) } else { Ok(String::new()) };
    let (Ok(ours), Ok(theirs)) = (ours, theirs) else {
        return Ok(ConflictView { conflict, original_exists, text: false, blocks: Vec::new(), crlf: false, trailing_newline: false });
    };
    let sample = if original_exists { &ours } else { &theirs };
    Ok(ConflictView {
        blocks: rusidian_core::merge::blocks(&ours, &theirs),
        crlf: sample.contains("\r\n"),
        trailing_newline: sample.ends_with('\n'),
        conflict,
        original_exists,
        text: true,
    })
}

/// change made on another device isn't overwritten by an old local copy.
/// `stamp` labels commits and conflict copies.
#[tauri::command]
async fn sync_now(
    app: AppHandle,
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
    ui: Option<serde_json::Value>,
    stamp: String,
) -> Result<SyncReport, String> {
    let (root, _) = current(&state, window.label())?;
    let settings = load_settings(&app);
    let device = settings.sync.get(&vault_key(&root)).cloned().ok_or("이 vault는 동기화가 설정되어 있지 않습니다.")?;
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("sync");
    {
        let mut busy = state.syncing.lock().map_err(|_| "state poisoned")?;
        if !busy.insert(root.clone()) {
            return Err("이미 동기화하는 중입니다.".into());
        }
    }
    let result = tauri::async_runtime::spawn_blocking({
        let root = root.clone();
        let calendars = settings.calendars.clone();
        move || -> Result<(gitsync::Outcome, Option<AppSettingsFile>), String> {
            let shared = sync::load_shared(&root);
            // Rusidian's own settings go along in the settings folder, rewritten
            // only when something changed here.
            let path = root.join(sync::APP_SETTINGS_FILE);
            let existing: Option<AppSettingsFile> =
                fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok());
            let file = AppSettingsFile {
                ui: ui.or_else(|| existing.as_ref().and_then(|e| e.ui.clone())),
                calendars: shared.include_calendars.then_some(calendars),
            };
            if existing.as_ref() != Some(&file) && (file.ui.is_some() || file.calendars.is_some()) {
                let json = serde_json::to_string_pretty(&file).map_err(|e| e.to_string())?;
                fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
                fs::write(&path, json).map_err(|e| e.to_string())?;
            }
            let base = SyncConfig {
                remote: device.remote,
                branch: device.branch,
                interval_minutes: device.interval_minutes,
                ..Default::default()
            };
            let repo = gitsync::Repo::for_vault(&data_dir, &root);
            let outcome = repo.sync(&|| base.with_shared(&sync::load_shared(&root)), &stamp)?;
            let incoming = outcome
                .pulled
                .iter()
                .any(|p| p == sync::APP_SETTINGS_FILE)
                .then(|| fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str::<AppSettingsFile>(&s).ok()))
                .flatten();
            Ok((outcome, incoming))
        }
    })
    .await
    .map_err(|e| e.to_string());
    if let Ok(mut busy) = state.syncing.lock() {
        busy.remove(&root);
    }
    let (outcome, incoming) = result??;
    let mut ui = None;
    if let Some(file) = incoming {
        if let Some(calendars) = file.calendars {
            let mut s = load_settings(&app);
            s.calendars = calendars;
            save_settings(&app, &s);
        }
        ui = file.ui;
    }
    Ok(SyncReport { outcome, ui })
}

// ------------------------------------------------------ external calendars

#[tauri::command]
fn list_calendars(app: AppHandle) -> Vec<CalendarSource> {
    load_settings(&app).calendars
}

#[tauri::command]
fn set_calendars(app: AppHandle, calendars: Vec<CalendarSource>) -> Result<Vec<CalendarSource>, String> {
    let clean = calendars::validate(calendars)?;
    let mut settings = load_settings(&app);
    settings.calendars = clean.clone();
    save_settings(&app, &settings);
    Ok(clean)
}

#[derive(serde::Serialize)]
struct ExternalEvent {
    /// The subscription's id.
    calendar: String,
    #[serde(flatten)]
    occurrence: rusidian_core::ics::Occurrence,
}

#[derive(serde::Serialize)]
struct ExternalError {
    calendar: String,
    name: String,
    message: String,
}

#[derive(serde::Serialize)]
struct ExternalEvents {
    events: Vec<ExternalEvent>,
    errors: Vec<ExternalError>,
}

/// Events of the enabled subscriptions overlapping `start..=end`. Downloads
/// are reused for a while unless `force` (the refresh button).
#[tauri::command]
async fn external_events(
    app: AppHandle,
    state: State<'_, AppState>,
    start: String,
    end: String,
    force: bool,
) -> Result<ExternalEvents, String> {
    let (start, end) = (parse_date(&start)?, parse_date(&end)?);
    let sources: Vec<CalendarSource> = load_settings(&app).calendars.into_iter().filter(|c| c.enabled).collect();
    let cache = state.ics.clone();
    let dir = app.path().app_cache_dir().ok().map(|d| d.join("calendars"));
    tauri::async_runtime::spawn_blocking(move || {
        // One thread per calendar, so a slow server doesn't hold up the others.
        let results: Vec<(CalendarSource, (Option<String>, Option<String>))> = std::thread::scope(|s| {
            let handles: Vec<_> = sources
                .iter()
                .map(|src| s.spawn(|| (src.clone(), subscriptions::load(&cache, dir.as_deref(), src, force))))
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        let mut out = ExternalEvents { events: Vec::new(), errors: Vec::new() };
        for (src, (text, error)) in results {
            if let Some(text) = text {
                out.events.extend(
                    rusidian_core::ics::occurrences(&text, start, end)
                        .into_iter()
                        .map(|occurrence| ExternalEvent { calendar: src.id.clone(), occurrence }),
                );
            }
            if let Some(message) = error {
                out.errors.push(ExternalError { calendar: src.id.clone(), name: src.name.clone(), message });
            }
        }
        out
    })
    .await
    .map_err(|e| e.to_string())
}

/// Daily notes and tasks for the days `start..=end` (a month grid).
#[tauri::command]
async fn calendar_range(state: State<'_, AppState>,
    window: tauri::WebviewWindow, start: String, end: String) -> Result<Calendar, String> {
    let (root, index) = current(&state, window.label())?;
    let config = daily::load_config(&root);
    calendar::build(&read_index(&index), &config, parse_date(&start)?, parse_date(&end)?)
}

/// Path of the daily note for `date`, creating it from the template if it
/// doesn't exist yet (what Obsidian does when you open a day).
#[tauri::command]
fn open_daily_note(state: State<AppState>,
    window: tauri::WebviewWindow, date: String, time: String) -> Result<String, String> {
    let (root, index) = current(&state, window.label())?;
    let date = parse_date(&date)?;
    let config = daily::load_config(&root);
    let path = config.note_path(date)?;
    if let Some(existing) = read_index(&index).file_path(&path) {
        return Ok(existing.to_string());
    }
    let content = config.initial_content(&root, date, &time);
    vault::create_text(&root, &path, &content)?;
    write_index(&index).upsert_note(&path, content);
    Ok(path)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(storage::init());
    // Updates from the GitHub releases (see `plugins.updater` in tauri.conf.json).
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init());
    builder
        .manage(AppState::default())
        .setup(|app| {
            file_history::init(app.handle());
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("vault", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let label = ctx.webview_label().to_string();
            let path = request.uri().path().to_string();
            // File reads stay off the protocol thread.
            std::thread::spawn(move || responder.respond(serve_vault_file(&app, &label, &path)));
        })
        // A closed window lets go of its vault (watcher and index).
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                let state = window.state::<AppState>();
                if let Ok(mut vaults) = state.vaults.lock() {
                    vaults.remove(window.label());
                }
                if let Ok(mut pending) = state.pending.lock() {
                    pending.remove(window.label());
                };
            }
        })
        .invoke_handler(tauri::generate_handler![
            open_vault,
            note_settings,
            set_note_settings,
            preview_note_settings,
            refresh_tree,
            last_vault,
            startup_vault,
            focus_vault_window,
            open_window,
            device_vault,
            storage_access,
            request_storage_access,
            pick_vault_folder,
            recent_vaults,
            forget_vault,
            read_note,
            file_times,
            write_note,
            create_note,
            new_note,
            new_folder,
            new_canvas,
            rename_path,
            move_path,
            trash_path,
            reveal_path,
            parse_frontmatter,
            set_properties,
            list_templates,
            render_template,
            search_vault,
            everything_status,
            everything_search,
            everything_open,
            list_tags,
            backlinks,
            resolve_link,
            list_tasks,
            toggle_task,
            update_task,
            delete_task,
            task_reminders,
            add_task,
            calendar_range,
            get_sync,
            set_sync,
            sync_scan,
            sync_now,
            list_conflicts,
            conflict_blocks,
            list_calendars,
            set_calendars,
            external_events,
            open_daily_note,
            finance_range,
            add_finance,
            update_finance,
            delete_finance,
            get_budgets,
            set_budgets,
            get_bookmarks,
            toggle_bookmark,
            remove_bookmark,
            outgoing_links,
            unlinked_mentions,
            link_mention,
            balances,
            add_balance,
            get_savings,
            get_hotkeys,
            set_hotkey,
            set_savings,
            add_settlement,
            save_attachment,
            fixed_expenses,
            set_fixed_expenses,
            fixed_reminders,
            set_fixed_remind,
            ignore_fixed,
            export_finance_csv,
            import_csv_preview,
            import_csv_plan,
            import_csv_commit,
            phone_inbox::phone_get,
            phone_inbox::phone_set,
            phone_inbox::phone_changed,
            phone_inbox::phone_process,
            phone_inbox::phone_reset,
            file_history::history_files,
            file_history::history_versions,
            file_history::history_read,
            file_history::history_diff,
            file_history::history_restore,
            file_history::history_clear,
            graph_groups,
            set_graph_groups,
            link_graph
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

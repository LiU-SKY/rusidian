//! Phone notifications → money records.
//!
//! The phone appends its bank and pay notifications to an inbox file that a
//! sync app brings to this computer (see `rusidian_core::phone`). The window
//! asks now and then whether the file changed, saves its unsaved notes, and
//! has the new notifications written into the vault. What was handled, and
//! the setup, stay on this computer (app config folder), never in the vault.

use crate::{all_finance, append_to_note, current, load_settings, parse_date, save_settings, vault_key, AppState};
use rusidian_core::phone::{self, LogItem, Rules, Status};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

/// This computer's setup for one vault.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
#[serde(default)]
pub struct PhoneSetup {
    pub enabled: bool,
    /// The inbox file the phone writes (synced to this computer).
    pub file: String,
    /// The note records go to; `YYYY` and `MM` are the record's year and month.
    pub note: String,
    pub rules: Rules,
}

const STATE_FILE: &str = "phone-inbox.json";
/// Keys remembered per vault (far more than a year of notifications).
const MAX_SEEN: usize = 20_000;
const MAX_LOG: usize = 60;
const MAX_INBOX_BYTES: u64 = 20 * 1024 * 1024;

#[derive(serde::Serialize, serde::Deserialize, Default)]
#[serde(default)]
struct InboxState {
    /// Keys of the records handled, oldest first.
    seen: Vec<String>,
    /// Size and modification time of the file when it was last read.
    stamp: Option<String>,
    /// What happened to recent notifications, newest first.
    log: Vec<LogItem>,
}

/// One run at a time, whichever window asks.
static RUNNING: Mutex<()> = Mutex::new(());

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(STATE_FILE))
}

fn load_states(app: &AppHandle) -> HashMap<String, InboxState> {
    state_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_states(app: &AppHandle, states: &HashMap<String, InboxState>) -> Result<(), String> {
    let p = state_path(app).ok_or("설정 폴더를 찾을 수 없습니다.")?;
    if let Some(dir) = p.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string(states).map_err(|e| e.to_string())?;
    fs::write(p, json).map_err(|e| format!("처리 기록을 저장할 수 없습니다: {e}"))
}

fn stamp(file: &Path) -> Option<String> {
    let meta = fs::metadata(file).ok()?;
    let modified = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis();
    Some(format!("{}:{modified}", meta.len()))
}

#[derive(serde::Serialize)]
pub struct PhoneInfo {
    setup: Option<PhoneSetup>,
    /// Recent notifications and what became of them, newest first.
    log: Vec<LogItem>,
    /// The inbox file exists (sync has brought it here).
    file_found: bool,
    /// What the phone's automation should append for each notification.
    template: &'static str,
}

#[tauri::command]
pub fn phone_get(app: AppHandle, state: State<AppState>, window: tauri::WebviewWindow) -> Result<PhoneInfo, String> {
    let (root, _) = current(&state, window.label())?;
    let key = vault_key(&root);
    let setup = load_settings(&app).phone.get(&key).cloned();
    let file_found = setup.as_ref().is_some_and(|s| !s.file.is_empty() && Path::new(&s.file).is_file());
    let log = load_states(&app).remove(&key).map(|s| s.log).unwrap_or_default();
    Ok(PhoneInfo { setup, log, file_found, template: phone::MACRODROID_TEMPLATE })
}

/// Save this vault's setup (`None` turns it off on this computer).
#[tauri::command]
pub fn phone_set(
    app: AppHandle,
    state: State<AppState>,
    window: tauri::WebviewWindow,
    setup: Option<PhoneSetup>,
) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    let mut settings = load_settings(&app);
    match setup {
        None => {
            settings.phone.remove(&vault_key(&root));
        }
        Some(mut s) => {
            s.file = s.file.trim().to_string();
            if s.enabled && s.file.is_empty() {
                return Err("휴대폰이 알림을 적는 파일을 고르세요.".into());
            }
            if !s.file.is_empty() && !Path::new(&s.file).is_absolute() {
                return Err("알림 파일은 전체 경로로 적어 주세요. (예: C:\\Users\\…\\Sync\\가계부 알림.txt)".into());
            }
            let sample = phone::note_for(&s.note, "2026-01-01");
            for part in sample[..sample.len() - 3].split('/') {
                rusidian_core::fileops::validate_name(part).map_err(|e| format!("기록할 노트 이름: {e}"))?;
            }
            s.note = s.note.trim().to_string();
            s.rules.own = s.rules.own.iter().map(|o| o.trim().to_string()).filter(|o| !o.is_empty()).collect();
            s.rules.settle_below = s.rules.settle_below.max(0);
            settings.phone.insert(vault_key(&root), s);
        }
    }
    save_settings(&app, &settings);
    Ok(())
}

/// The inbox file changed since it was last read (cheap; asked every so often).
#[tauri::command]
pub fn phone_changed(app: AppHandle, state: State<AppState>, window: tauri::WebviewWindow) -> Result<bool, String> {
    let (root, _) = current(&state, window.label())?;
    let key = vault_key(&root);
    let Some(setup) = load_settings(&app).phone.remove(&key).filter(|s| s.enabled && !s.file.is_empty()) else {
        return Ok(false);
    };
    let now = stamp(Path::new(&setup.file));
    Ok(now.is_some() && now != load_states(&app).remove(&key).and_then(|s| s.stamp))
}

#[derive(serde::Serialize)]
pub struct PhoneResult {
    /// Records written now.
    added: usize,
    /// Notes they went to.
    notes: Vec<String>,
    /// This run's notifications (newest first).
    log: Vec<LogItem>,
}

/// Write the inbox's new notifications into the vault. `today` dates
/// records whose header has no date.
#[tauri::command]
pub fn phone_process(
    app: AppHandle,
    state: State<AppState>,
    window: tauri::WebviewWindow,
    today: String,
) -> Result<PhoneResult, String> {
    let _running = RUNNING.lock().unwrap_or_else(|p| p.into_inner());
    let (root, index) = current(&state, window.label())?;
    let key = vault_key(&root);
    let setup = load_settings(&app).phone.remove(&key).ok_or("휴대폰 알림이 설정되어 있지 않습니다.")?;
    let file = PathBuf::from(&setup.file);
    let before = stamp(&file);
    let meta = fs::metadata(&file).map_err(|_| format!("알림 파일이 없습니다: {}", setup.file))?;
    if meta.len() > MAX_INBOX_BYTES {
        return Err("알림 파일이 너무 큽니다 (20MB까지). 휴대폰에서 파일을 비워 주세요.".into());
    }
    let bytes = fs::read(&file).map_err(|e| format!("알림 파일을 읽을 수 없습니다: {e}"))?;
    let text = String::from_utf8_lossy(&bytes);

    let mut states = load_states(&app);
    let inbox = states.entry(key).or_default();
    let seen: HashSet<String> = inbox.seen.iter().cloned().collect();
    let history = all_finance(&root, &index);
    let batch = phone::process(&text, &seen, &setup.rules, &history, parse_date(&today)?);

    // Each record to its month's note, in file order.
    let mut by_note: Vec<(String, Vec<String>)> = Vec::new();
    for r in &batch.records {
        let note = phone::note_for(&setup.note, &r.date);
        match by_note.iter_mut().find(|(n, _)| *n == note) {
            Some((_, lines)) => lines.push(r.line()),
            None => by_note.push((note, vec![r.line()])),
        }
    }
    let mut notes = Vec::new();
    for (note, lines) in &by_note {
        // On failure nothing is marked handled; the next run finds the
        // records already written as duplicates.
        notes.push(append_to_note(&root, &index, note, lines)?);
    }

    inbox.seen.extend(batch.keys.iter().cloned());
    if inbox.seen.len() > MAX_SEEN {
        let extra = inbox.seen.len() - MAX_SEEN;
        inbox.seen.drain(..extra);
    }
    inbox.stamp = before;
    let mut log: Vec<LogItem> = batch.log.iter().rev().cloned().collect();
    log.extend(std::mem::take(&mut inbox.log));
    log.truncate(MAX_LOG);
    inbox.log = log;
    save_states(&app, &states)?;

    let added = batch.log.iter().filter(|l| l.status == Status::Added).count();
    Ok(PhoneResult { added, notes, log: batch.log.into_iter().rev().collect() })
}

/// Forget which notifications were handled, so the whole file is read
/// again (records already in the vault are still recognized).
#[tauri::command]
pub fn phone_reset(app: AppHandle, state: State<AppState>, window: tauri::WebviewWindow) -> Result<(), String> {
    let (root, _) = current(&state, window.label())?;
    let mut states = load_states(&app);
    states.remove(&vault_key(&root));
    save_states(&app, &states)
}

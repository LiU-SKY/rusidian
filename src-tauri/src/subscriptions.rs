//! Downloading external calendars (ICS URLs or local `.ics` files).
//! The subscription list and its checks are in `rusidian_core::calendars`.
//!
//! The list lives in the app settings on this computer, not in the vault:
//! subscription URLs often carry private tokens (Google's "secret address").
//! Downloads are cached in memory for a while and on disk as a fallback,
//! so the calendar still shows the last known events when offline.

use rusidian_core::calendars::{CalendarSource, is_http};
use rusidian_core::ics;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a download is reused before fetching again.
const REFRESH: Duration = Duration::from_secs(15 * 60);
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Default)]
pub struct IcsCache {
    entries: Mutex<HashMap<String, (Instant, String)>>,
}

fn fetch(url: &str) -> Result<String, String> {
    let text = if is_http(url) {
        let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(TIMEOUT)).build().into();
        let mut resp = agent
            .get(url)
            .header("User-Agent", "Rusidian")
            .call()
            .map_err(|e| format!("받아오지 못했습니다: {e}"))?;
        resp.body_mut()
            .with_config()
            .limit(MAX_BYTES)
            .read_to_string()
            .map_err(|e| format!("받아오지 못했습니다: {e}"))?
    } else {
        fs::read_to_string(url).map_err(|e| format!("파일을 읽지 못했습니다: {e}"))?
    };
    if !ics::looks_like_ics(&text) {
        return Err("iCalendar(ICS) 형식이 아닙니다. 주소가 로그인 페이지나 웹 페이지를 가리키는지 확인하세요.".into());
    }
    Ok(text)
}

/// File name for the on-disk copy of `url` (FNV-1a hash, no secrets in names).
fn cache_file(dir: &Path, url: &str) -> PathBuf {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    dir.join(format!("{h:016x}.ics"))
}

/// The calendar text for `source`: a recent download, a new one, or (when
/// that fails) the last saved copy. The second value is an error to show.
pub fn load(cache: &IcsCache, dir: Option<&Path>, source: &CalendarSource, force: bool) -> (Option<String>, Option<String>) {
    let url = &source.url;
    if !force {
        if let Some((at, text)) = cache.entries.lock().ok().and_then(|m| m.get(url).cloned()) {
            if at.elapsed() < REFRESH {
                return (Some(text), None);
            }
        }
    }
    match fetch(url) {
        Ok(text) => {
            if let Ok(mut m) = cache.entries.lock() {
                m.insert(url.clone(), (Instant::now(), text.clone()));
            }
            if let Some(dir) = dir {
                let _ = fs::create_dir_all(dir);
                let _ = fs::write(cache_file(dir, url), &text);
            }
            (Some(text), None)
        }
        Err(e) => {
            let stale = cache.entries.lock().ok().and_then(|m| m.get(url).map(|(_, t)| t.clone()));
            let saved = stale.or_else(|| dir.and_then(|d| fs::read_to_string(cache_file(d, url)).ok()));
            match saved {
                Some(text) => (Some(text), Some(format!("{e} (마지막으로 받아 둔 일정을 보여줍니다)"))),
                None => (None, Some(e)),
            }
        }
    }
}

// Tests for the list and addresses live in `rusidian_core::calendars`: the
// unit-test binary of a Tauri app can't start on Windows (it lacks the app
// manifest some GUI code needs), so logic worth testing belongs in core.

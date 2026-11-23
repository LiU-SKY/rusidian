//! Searching the whole PC with voidtools Everything (https://www.voidtools.com).
//!
//! Talks to a running Everything through its own IPC (the protocol the
//! Everything SDK speaks: `WM_COPYDATA` to Everything's hidden window, results
//! sent back to a window of ours), so no `Everything64.dll` or `es.exe` has to
//! ship with the app. Works with Everything 1.4 and the 1.5 alpha.
//!
//! The reply parsing is plain Rust and tested everywhere; the window calls
//! only exist on Windows.

// Elsewhere only the parsing (tested) and the stubs below are built.
#![cfg_attr(not(windows), allow(dead_code))]

use serde::Serialize;
use std::path::Path;

/// A file or folder Everything found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hit {
    /// Full path, Windows style (`C:\Users\me\file.txt`).
    pub path: String,
    /// File or folder name (`file.txt`); for a drive, `C:`.
    pub name: String,
    pub folder: bool,
    /// Bytes, when Everything indexes sizes.
    pub size: Option<u64>,
    /// Unix milliseconds, when Everything indexes modification dates.
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Found {
    /// How many items match in all (`hits` holds at most the asked maximum).
    pub total: u32,
    pub hits: Vec<Hit>,
}

/// Whether Everything can be asked right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub running: bool,
    /// `1.4.1` style, when running.
    pub version: Option<String>,
    /// False while Everything is still building its index after starting.
    pub db_loaded: bool,
}

// ------------------------------------------------------------ IPC constants
// From the Everything SDK's everything_ipc.h.

/// `EVERYTHING_IPC_COPYDATA_QUERY2W`
const COPYDATA_QUERY2W: usize = 18;
const REQUEST_NAME: u32 = 0x0000_0001;
const REQUEST_PATH: u32 = 0x0000_0002;
const REQUEST_SIZE: u32 = 0x0000_0010;
const REQUEST_DATE_MODIFIED: u32 = 0x0000_0040;
/// What we ask for; the reply lays the fields out in this bit order.
const REQUEST: u32 = REQUEST_NAME | REQUEST_PATH | REQUEST_SIZE | REQUEST_DATE_MODIFIED;
/// `EVERYTHING_SORT_NAME_ASCENDING` (always fast, needs no extra index).
const SORT_NAME_ASCENDING: u32 = 1;
/// `EVERYTHING_IPC_FOLDER`, `EVERYTHING_IPC_DRIVE` item flags.
const ITEM_FOLDER: u32 = 0x1;
const ITEM_DRIVE: u32 = 0x2;
/// Our `reply_copydata_message`: tells our reply apart from other `WM_COPYDATA`.
const REPLY_ID: u32 = 0x5255_5349; // "RUSI"

/// Windows FILETIME (100 ns since 1601) of the Unix epoch.
const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

// ----------------------------------------------------------------- parsing

/// The `EVERYTHING_IPC_QUERY2` message: seven DWORDs, then the search text
/// as NUL-terminated UTF-16.
fn encode_query(reply_hwnd: u32, text: &str, max: u32) -> Vec<u8> {
    let header = [
        reply_hwnd,
        REPLY_ID,
        0, // search_flags: no match case / whole word / regex / match path
        0, // offset
        max,
        REQUEST,
        SORT_NAME_ASCENDING,
    ];
    let mut out = Vec::with_capacity(28 + (text.len() + 1) * 2);
    for v in header {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for unit in text.encode_utf16().chain(std::iter::once(0)) {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let buf: &'a [u8] = self.buf;
        let end = self.at.checked_add(n).filter(|&e| e <= buf.len()).ok_or_else(truncated)?;
        let out = &buf[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
    /// DWORD length in UTF-16 units, the text, then a NUL.
    fn string(&mut self) -> Result<String, String> {
        let len = self.u32()? as usize;
        let raw = self.bytes(len.checked_add(1).and_then(|n| n.checked_mul(2)).ok_or_else(truncated)?)?;
        let units: Vec<u16> = raw[..len * 2].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        Ok(String::from_utf16_lossy(&units))
    }
}

fn truncated() -> String {
    "Everything 응답이 잘렸습니다.".to_string()
}

fn filetime_to_unix_ms(ft: u64) -> Option<i64> {
    if ft == u64::MAX || ft < FILETIME_UNIX_EPOCH {
        return None;
    }
    i64::try_from((ft - FILETIME_UNIX_EPOCH) / 10_000).ok()
}

/// Read an `EVERYTHING_IPC_LIST2` reply to a query made with `REQUEST`.
fn parse_list2(buf: &[u8]) -> Result<Found, String> {
    let mut head = Reader { buf, at: 0 };
    let total = head.u32()?;
    let count = head.u32()?;
    let _offset = head.u32()?;
    let flags = head.u32()?;
    let _sort = head.u32()?;
    if flags & (REQUEST_NAME | REQUEST_PATH) != (REQUEST_NAME | REQUEST_PATH) {
        return Err("Everything이 파일 이름을 돌려주지 않았습니다.".to_string());
    }
    let mut hits = Vec::with_capacity(count.min(10_000) as usize);
    for _ in 0..count {
        let item_flags = head.u32()?;
        let data_offset = head.u32()? as usize;
        if data_offset > buf.len() {
            return Err(truncated());
        }
        // Fields come in request-bit order; only the ones Everything says it filled.
        let mut data = Reader { buf, at: data_offset };
        let name = data.string()?;
        let parent = data.string()?;
        let size = if flags & REQUEST_SIZE != 0 { Some(data.u64()?) } else { None };
        let modified = if flags & REQUEST_DATE_MODIFIED != 0 { Some(data.u64()?) } else { None };
        let path = if parent.is_empty() {
            name.clone()
        } else if parent.ends_with('\\') {
            format!("{parent}{name}")
        } else {
            format!("{parent}\\{name}")
        };
        hits.push(Hit {
            path,
            name,
            folder: item_flags & (ITEM_FOLDER | ITEM_DRIVE) != 0,
            size: size.filter(|&s| s != u64::MAX),
            modified: modified.and_then(filetime_to_unix_ms),
        });
    }
    Ok(Found { total, hits })
}

/// `full` (as Everything gives it) relative to the vault at `root`, with `/`
/// separators, or None when it is outside the vault (or is the vault itself).
/// Windows paths ignore case.
pub fn vault_relative(root: &Path, full: &str) -> Option<String> {
    let norm = |s: &str| s.replace('/', "\\");
    let root = norm(&root.to_string_lossy());
    let root = root.trim_end_matches('\\');
    let full = norm(full);
    // `\\?\C:\…` (verbatim) and `C:\…` name the same folder.
    let strip = |s: &str| s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or_else(|| s.to_string());
    let (root, full) = (strip(root), strip(&full));
    if root.is_empty() || full.len() <= root.len() + 1 {
        return None;
    }
    // None when the cut falls inside a character: then it can't be the vault.
    let head = full.get(..root.len())?;
    if head.to_lowercase() != root.to_lowercase() {
        return None;
    }
    let rest = full[root.len()..].strip_prefix('\\')?;
    Some(rest.trim_end_matches('\\').replace('\\', "/")).filter(|r| !r.is_empty())
}

// ---------------------------------------------------------------- Windows

#[cfg(windows)]
mod win {
    use super::*;
    use std::cell::RefCell;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::DataExchange::COPYDATASTRUCT;
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, FindWindowW, MsgWaitForMultipleObjects,
        PeekMessageW, RegisterClassExW, SendMessageTimeoutW, TranslateMessage, HWND_MESSAGE, MSG, PM_REMOVE,
        QS_ALLINPUT, SMTO_ABORTIFHUNG, WM_COPYDATA, WM_USER, WNDCLASSEXW,
    };

    /// Everything 1.4, then the 1.5 alpha (which runs as instance "1.5a").
    const EVERYTHING_CLASSES: [&str; 2] =
        ["EVERYTHING_TASKBAR_NOTIFICATION", "EVERYTHING_TASKBAR_NOTIFICATION_(1.5a)"];
    const REPLY_CLASS: &str = "RUSIDIAN_EVERYTHING_REPLY";
    /// `EVERYTHING_IPC_GET_MAJOR_VERSION` … and `EVERYTHING_IPC_IS_DB_LOADED`.
    const IPC_MAJOR: usize = 0;
    const IPC_MINOR: usize = 1;
    const IPC_REVISION: usize = 2;
    const IPC_IS_DB_LOADED: usize = 401;
    const SEND_TIMEOUT_MS: u32 = 2_000;
    const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

    thread_local! {
        /// The reply our window got (each search runs on its own thread's window).
        static REPLY: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn not_running() -> String {
        "Everything이 실행 중이 아닙니다. Everything을 켠 뒤 다시 시도하세요.".to_string()
    }

    fn everything_window() -> Option<HWND> {
        EVERYTHING_CLASSES.iter().find_map(|class| {
            let class = wide(class);
            let hwnd = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
            (!hwnd.is_null()).then_some(hwnd)
        })
    }

    /// `SendMessageTimeoutW`, None when Everything doesn't answer in time.
    fn send(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<usize> {
        let mut result: usize = 0;
        let ok = unsafe { SendMessageTimeoutW(hwnd, msg, wparam, lparam, SMTO_ABORTIFHUNG, SEND_TIMEOUT_MS, &mut result) };
        (ok != 0).then_some(result)
    }

    unsafe extern "system" fn reply_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if msg == WM_COPYDATA && lparam != 0 {
            // Valid only during this call, so copy it out.
            let cds = unsafe { &*(lparam as *const COPYDATASTRUCT) };
            if cds.dwData == REPLY_ID as usize {
                let bytes = if cds.lpData.is_null() || cds.cbData == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(cds.lpData as *const u8, cds.cbData as usize) }.to_vec()
                };
                REPLY.with(|r| *r.borrow_mut() = Some(bytes));
                return 1;
            }
        }
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    fn register_class() -> Result<(), String> {
        static REGISTERED: OnceLock<bool> = OnceLock::new();
        let ok = *REGISTERED.get_or_init(|| {
            let name = wide(REPLY_CLASS);
            let mut wc: WNDCLASSEXW = unsafe { std::mem::zeroed() };
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(reply_proc);
            wc.hInstance = unsafe { GetModuleHandleW(std::ptr::null()) };
            // Windows copies the name; it need not outlive the call.
            wc.lpszClassName = name.as_ptr();
            unsafe { RegisterClassExW(&wc) != 0 }
        });
        if ok { Ok(()) } else { Err("Everything 응답 창을 만들지 못했습니다.".to_string()) }
    }

    /// A message-only window on this thread, destroyed when dropped.
    struct ReplyWindow(HWND);

    impl ReplyWindow {
        fn new() -> Result<Self, String> {
            register_class()?;
            let class = wide(REPLY_CLASS);
            let hwnd = unsafe {
                CreateWindowExW(0, class.as_ptr(), std::ptr::null(), 0, 0, 0, 0, 0, HWND_MESSAGE,
                    std::ptr::null_mut(), GetModuleHandleW(std::ptr::null()), std::ptr::null())
            };
            if hwnd.is_null() {
                return Err("Everything 응답 창을 만들지 못했습니다.".to_string());
            }
            Ok(Self(hwnd))
        }
    }

    impl Drop for ReplyWindow {
        fn drop(&mut self) {
            unsafe { DestroyWindow(self.0) };
        }
    }

    pub fn status() -> Status {
        let Some(hwnd) = everything_window() else {
            return Status { running: false, version: None, db_loaded: false };
        };
        let part = |which| send(hwnd, WM_USER, which, 0);
        let version = match (part(IPC_MAJOR), part(IPC_MINOR), part(IPC_REVISION)) {
            (Some(a), Some(b), Some(c)) => Some(format!("{a}.{b}.{c}")),
            _ => None,
        };
        let db_loaded = part(IPC_IS_DB_LOADED).is_some_and(|v| v != 0);
        Status { running: true, version, db_loaded }
    }

    pub fn search(text: &str, max: u32) -> Result<Found, String> {
        let everything = everything_window().ok_or_else(not_running)?;
        let window = ReplyWindow::new()?;
        REPLY.with(|r| *r.borrow_mut() = None);

        // HWNDs fit in 32 bits (they are shared with 32-bit processes).
        let mut query = encode_query(window.0 as usize as u32, text, max);
        let cds = COPYDATASTRUCT {
            dwData: COPYDATA_QUERY2W,
            cbData: query.len() as u32,
            lpData: query.as_mut_ptr().cast(),
        };
        let accepted = send(everything, WM_COPYDATA, window.0 as WPARAM, &cds as *const COPYDATASTRUCT as LPARAM);
        if accepted.is_none_or(|v| v == 0) {
            return Err("Everything이 검색 요청을 받지 않았습니다. Everything 1.4 이상인지 확인하세요.".to_string());
        }

        // Everything answers with a WM_COPYDATA of its own; pump messages until then.
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if let Some(bytes) = REPLY.with(|r| r.borrow_mut().take()) {
                return parse_list2(&bytes);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("Everything이 제때 답하지 않았습니다. 색인을 만드는 중이면 잠시 뒤 다시 시도하세요.".to_string());
            }
            unsafe { MsgWaitForMultipleObjects(0, std::ptr::null(), 0, left.as_millis() as u32, QS_ALLINPUT) };
        }
    }
}

/// Is Everything running (and done loading its index)?
pub fn status() -> Status {
    #[cfg(windows)]
    {
        win::status()
    }
    #[cfg(not(windows))]
    {
        Status { running: false, version: None, db_loaded: false }
    }
}

/// Search with Everything's own syntax (`ext:pdf`, `"two words"`, `C:\work\ report`, …).
/// Blocks until Everything answers (a few ms usually; gives up after 5 s), so
/// call it off the UI thread.
pub fn search(text: &str, max: u32) -> Result<Found, String> {
    if text.trim().is_empty() {
        return Ok(Found { total: 0, hits: Vec::new() });
    }
    #[cfg(windows)]
    {
        win::search(text, max)
    }
    #[cfg(not(windows))]
    {
        let _ = max;
        Err("Everything 검색은 Windows에서만 쓸 수 있습니다.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn put_string(out: &mut Vec<u8>, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_le_bytes());
        for u in units.iter().chain(std::iter::once(&0)) {
            out.extend_from_slice(&u.to_le_bytes());
        }
    }

    /// An `EVERYTHING_IPC_LIST2` as Everything would send it.
    fn list2(total: u32, items: &[(u32, &str, &str, u64, u64)]) -> Vec<u8> {
        let header_len = 20 + items.len() * 8;
        let mut data = Vec::new();
        let mut offsets = Vec::new();
        for &(_, name, path, size, date) in items {
            offsets.push((header_len + data.len()) as u32);
            put_string(&mut data, name);
            put_string(&mut data, path);
            data.extend_from_slice(&size.to_le_bytes());
            data.extend_from_slice(&date.to_le_bytes());
        }
        let mut out = Vec::new();
        for v in [total, items.len() as u32, 0, REQUEST, SORT_NAME_ASCENDING] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for (i, &(flags, ..)) in items.iter().enumerate() {
            out.extend_from_slice(&flags.to_le_bytes());
            out.extend_from_slice(&offsets[i].to_le_bytes());
        }
        out.extend_from_slice(&data);
        out
    }

    #[test]
    fn reads_files_folders_and_drives() {
        // 2024-01-01T00:00:00Z
        let ft = FILETIME_UNIX_EPOCH + 1_704_067_200_000 * 10_000;
        let buf = list2(42, &[
            (0, "보고서.pdf", r"C:\Users\me\문서", 1234, ft),
            (ITEM_FOLDER, "notes", r"D:\", u64::MAX, u64::MAX),
            (ITEM_DRIVE, "E:", "", u64::MAX, 0),
        ]);
        let found = parse_list2(&buf).unwrap();
        assert_eq!(found.total, 42);
        assert_eq!(found.hits, vec![
            Hit { path: r"C:\Users\me\문서\보고서.pdf".into(), name: "보고서.pdf".into(), folder: false,
                size: Some(1234), modified: Some(1_704_067_200_000) },
            Hit { path: r"D:\notes".into(), name: "notes".into(), folder: true, size: None, modified: None },
            Hit { path: "E:".into(), name: "E:".into(), folder: true, size: None, modified: None },
        ]);
    }

    #[test]
    fn rejects_truncated_replies() {
        let buf = list2(1, &[(0, "a.txt", r"C:\x", 1, 1)]);
        for cut in [3, 19, 27, buf.len() - 1] {
            assert!(parse_list2(&buf[..cut]).is_err(), "cut at {cut}");
        }
        // A data offset past the end.
        let mut bad = buf.clone();
        bad[24..28].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_list2(&bad).is_err());
    }

    #[test]
    fn empty_reply() {
        let found = parse_list2(&list2(0, &[])).unwrap();
        assert_eq!(found, Found { total: 0, hits: vec![] });
    }

    #[test]
    fn query_layout() {
        let q = encode_query(0x1234, "ext:md 노트", 50);
        let dword = |i: usize| u32::from_le_bytes(q[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!([dword(0), dword(1), dword(4), dword(5)], [0x1234, REPLY_ID, 50, REQUEST]);
        let text: Vec<u16> = q[28..].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        assert_eq!(text.last(), Some(&0));
        assert_eq!(String::from_utf16(&text[..text.len() - 1]).unwrap(), "ext:md 노트");
    }

    #[test]
    fn maps_paths_into_the_vault() {
        let root = PathBuf::from(r"C:\Users\Me\Vault");
        assert_eq!(vault_relative(&root, r"C:\Users\Me\Vault\Daily\2024-01-01.md").as_deref(), Some("Daily/2024-01-01.md"));
        assert_eq!(vault_relative(&root, r"c:\users\me\vault\a.md").as_deref(), Some("a.md"));
        assert_eq!(vault_relative(&root, r"C:\Users\Me\Vault\Folder").as_deref(), Some("Folder"));
        assert_eq!(vault_relative(&PathBuf::from(r"\\?\C:\Users\Me\Vault\"), r"C:\Users\Me\Vault\a.md").as_deref(), Some("a.md"));
        assert_eq!(vault_relative(&root, r"C:\Users\Me\Vault"), None);
        assert_eq!(vault_relative(&root, r"C:\Users\Me\Vault2\a.md"), None);
        assert_eq!(vault_relative(&root, r"D:\a.md"), None);
    }

    #[test]
    fn empty_query_finds_nothing() {
        assert_eq!(search("  ", 10).unwrap().hits.len(), 0);
    }
}

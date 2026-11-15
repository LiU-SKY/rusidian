//! Obsidian's bookmarks (`.obsidian/bookmarks.json`), read and written the
//! way Obsidian does so both apps share them:
//!
//! ```json
//! {"items": [
//!   {"type": "file", "ctime": 1731550000000, "path": "회의/1114.md"},
//!   {"type": "group", "ctime": …, "title": "업무", "items": [ … ]},
//!   {"type": "search", "ctime": …, "query": "tag:#todo"},
//!   {"type": "heading", "ctime": …, "path": "a.md", "subpath": "#제목"},
//!   {"type": "folder", "ctime": …, "path": "회의"}
//! ]}
//! ```
//!
//! Items are kept as JSON values, so fields and item types this app does not
//! know (graph views, URLs, titles) survive a round trip untouched.

use serde_json::{json, Value};
use std::path::Path;

const FILE: &str = "bookmarks.json";
const KEY: &str = "items";

pub fn load(root: &Path) -> Vec<Value> {
    crate::obsidian_config::read_key(root, FILE, KEY)
}

fn save(root: &Path, items: &[Value]) -> Result<(), String> {
    crate::obsidian_config::write_key(root, FILE, KEY, &items)
}

fn is_file_item(v: &Value, path: &str) -> bool {
    v.get("type").and_then(Value::as_str) == Some("file") && v.get("path").and_then(Value::as_str) == Some(path)
}

/// Is `path` bookmarked as a file anywhere (groups included)?
pub fn contains_file(items: &[Value], path: &str) -> bool {
    items.iter().any(|v| {
        is_file_item(v, path) || v.get("items").and_then(Value::as_array).is_some_and(|inner| contains_file(inner, path))
    })
}

/// Drop every file bookmark of `path`; true if any was there.
fn remove_file(items: &mut Vec<Value>, path: &str) -> bool {
    let before = items.len();
    items.retain(|v| !is_file_item(v, path));
    let mut removed = items.len() != before;
    for v in items.iter_mut() {
        if let Some(inner) = v.get_mut("items").and_then(Value::as_array_mut) {
            removed |= remove_file(inner, path);
        }
    }
    removed
}

/// Bookmark `path` (at the end), or remove it if it already is one.
/// Returns whether it is bookmarked now. `ctime` is milliseconds since 1970.
pub fn toggle_file(root: &Path, path: &str, ctime: i64) -> Result<bool, String> {
    let mut items = load(root);
    let on = if remove_file(&mut items, path) {
        false
    } else {
        items.push(json!({ "type": "file", "ctime": ctime, "path": path }));
        true
    };
    save(root, &items)?;
    Ok(on)
}

/// Remove the item at `at`: indices from the top level down into groups.
pub fn remove_at(root: &Path, at: &[usize]) -> Result<(), String> {
    let mut items = load(root);
    let (last, parents) = at.split_last().ok_or("지울 북마크가 없습니다.")?;
    let mut list = &mut items;
    for &i in parents {
        list = list
            .get_mut(i)
            .and_then(|v| v.get_mut("items"))
            .and_then(Value::as_array_mut)
            .ok_or("북마크 목록이 바뀌었습니다. 다시 시도하세요.")?;
    }
    if *last >= list.len() {
        return Err("북마크 목록이 바뀌었습니다. 다시 시도하세요.".into());
    }
    list.remove(*last);
    save(root, &items)
}

/// Follow a rename/move: `from` itself, or anything inside folder `from`.
fn rename_in(items: &mut [Value], from: &str, to: &str) -> bool {
    let prefix = format!("{from}/");
    let mut changed = false;
    for v in items.iter_mut() {
        let new_path = match v.get("path").and_then(Value::as_str) {
            Some(p) if p == from => Some(to.to_string()),
            Some(p) => p.strip_prefix(&prefix).map(|rest| format!("{to}/{rest}")),
            None => None,
        };
        if let Some(p) = new_path {
            v["path"] = Value::String(p);
            changed = true;
        }
        if let Some(inner) = v.get_mut("items").and_then(Value::as_array_mut) {
            changed |= rename_in(inner, from, to);
        }
    }
    changed
}

/// Keep bookmarks pointing at a file or folder after it is renamed or moved.
pub fn renamed(root: &Path, from: &str, to: &str) -> Result<(), String> {
    let mut items = load(root);
    if rename_in(&mut items, from, to) {
        save(root, &items)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("rusidian-bookmarks-{}-{:?}", std::process::id(), std::thread::current().id()));
        std::fs::create_dir_all(dir.join(".obsidian")).unwrap();
        dir
    }

    #[test]
    fn toggles_and_keeps_unknown_items() {
        let dir = vault();
        std::fs::write(
            dir.join(".obsidian/bookmarks.json"),
            r#"{"items":[{"type":"graph","ctime":1,"title":"내 그래프","options":{"x":1}},{"type":"group","ctime":2,"title":"업무","items":[{"type":"file","ctime":3,"path":"a.md"}]}],"other":true}"#,
        )
        .unwrap();
        // In a group: found, and toggling removes it from there.
        assert!(contains_file(&load(&dir), "a.md"));
        assert!(!toggle_file(&dir, "a.md", 10).unwrap());
        assert!(!contains_file(&load(&dir), "a.md"));
        // Not there: added at the end.
        assert!(toggle_file(&dir, "b.md", 11).unwrap());
        let items = load(&dir);
        assert_eq!(items.last().unwrap(), &json!({"type": "file", "ctime": 11, "path": "b.md"}));
        // The graph item, its options and the other top-level key survive.
        assert_eq!(items[0]["options"]["x"], 1);
        let raw: Value = serde_json::from_str(&std::fs::read_to_string(dir.join(".obsidian/bookmarks.json")).unwrap()).unwrap();
        assert_eq!(raw["other"], true);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn follows_renames_and_removes_by_position() {
        let dir = vault();
        std::fs::write(
            dir.join(".obsidian/bookmarks.json"),
            r##"{"items":[{"type":"folder","ctime":1,"path":"회의"},{"type":"group","ctime":2,"title":"g","items":[{"type":"heading","ctime":3,"path":"회의/1114.md","subpath":"#결정"},{"type":"search","ctime":4,"query":"회의"}]},{"type":"file","ctime":5,"path":"회의록.md"}]}"##,
        )
        .unwrap();
        renamed(&dir, "회의", "미팅").unwrap();
        let items = load(&dir);
        assert_eq!(items[0]["path"], "미팅");
        assert_eq!(items[1]["items"][0]["path"], "미팅/1114.md");
        assert_eq!(items[1]["items"][0]["subpath"], "#결정");
        assert_eq!(items[2]["path"], "회의록.md"); // only the folder and what is inside it
        remove_at(&dir, &[1, 1]).unwrap();
        assert_eq!(load(&dir)[1]["items"].as_array().unwrap().len(), 1);
        assert!(remove_at(&dir, &[1, 5]).is_err());
        assert!(remove_at(&dir, &[0, 0]).is_err()); // a folder item has no children
        std::fs::remove_dir_all(&dir).ok();
    }
}
